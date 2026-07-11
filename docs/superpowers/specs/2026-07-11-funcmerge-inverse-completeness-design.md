# Functional-merge-across-inverse completeness fix — design

**Date:** 2026-07-11
**Status:** approved (design); pending implementation plan

## Problem

rustdl classifies galen as **Horn** and prints "hyper Horn fixpoint is complete" — a
completeness guarantee — yet misses **10 genuine subsumptions** that Konclude and
HermiT both derive. Per the completeness contract (`completeness_guaranteed()` true
⟹ Horn/PureEl + no timeout ⟹ MISSED = 0), this is a **contract violation**, not a
tolerable engineering gap.

rustdl remains fully **sound** throughout (FP = 0). Only completeness is affected.

## Root cause (precisely localized)

The 10 galen misses all reduce to one pattern: a **functional (`≤1`) role merge across
an inverse-induced edge**, in a cyclic model. Minimal reproduction (5 classes / 3 roles;
`ontologies/` is gitignored, so this fixture lives **inline as an OFN string constant in
the test**, not as a committed file):

```
A  ⊑ ∃f.N
f  ≡ inverse(g)
Functional(g)
N  ≡ ∃g.(Y ⊓ ∃h.LFC)
Y  ⊑ Z
LFC ≡ ∃g.A          # the cycle: without it, rustdl derives A ⊑ Y correctly
```

Intended derivation (Konclude/HermiT both do it in ms): `A —f→ n:N`; since
`f ≡ inverse(g)`, `n —g→ A`; `N ⊑ ∃g.(Y⊓…)` gives `n —g→ m` with `m : Y`; `Functional(g)`
on `n` forces its two g-successors `A` and `m` to **merge** ⟹ `A : Y ⊑ Z`.

The wedge (`crates/owl-dl-tableau/src/hyper.rs`) **never triggers the merge**:
`distinct_role_succ` (`hyper.rs:2411`) — the function `find_open_at_most`
(`hyper.rs:2583`) uses to count a node's `≤n`-role successors — scans **only the node's
outgoing `edges`**, never its incoming `preds` with the role polarity flipped. So `A`
(reachable as `n`'s g-successor only via the inverse of the `A —f→ n` edge, i.e. via
`n.preds`) is invisible; `n` appears to have **1** g-successor (`m`), not 2; the `≤1 g`
constraint is never seen as violated; `merge(A, m)` never runs.

This is a "split-brain": the `≤1 g` constraint **is correctly attached** to `n` (via the
inverse-aware `enumerate_matches`, `hyper.rs:2861`, which body-gates it), but the
violation **check** (`distinct_role_succ`) under-counts. `enumerate_matches`
(`hyper.rs:2890–2906`) already performs the correct `edges` + `preds`/flip union — a
proven in-file pattern the fix mirrors.

Blocking is **not** the cause (confirmed: `is_blocked` is not consulted in
`find_open_at_most` or its caller). Earlier blocking-flag experiments therefore had no
effect, consistent with this diagnosis.

## Fix

### Primary — inverse-aware successor counting

Make `distinct_role_succ(node, role, qual)` (`hyper.rs:2411`) union:
- outgoing matches: `node.edges` where `role_matches(edge.role, role, hier)` (current behavior), and
- inverse matches: `node.preds` where `role_matches(pred.role.flip(), role, hier)` (new; mirrors `enumerate_matches` at `hyper.rs:2902–2906`),

then **dedupe by `resolve()`** so each distinct (canonical) successor node is counted
once. The qualifier (`qual`) filtering already applied to forward matches applies
identically to the inverse matches. No change is needed to `must_be_distinct` /
`labels_disjoint` (`hyper.rs:2433`) or the partition/merge loop (`hyper.rs:2521`) — they
operate on the `succs` vector `distinct_role_succ` returns.

### Correctness repair — root-folded merge

Because the merge can now fold a node reached via `preds` — possibly the **root** `A`
(node 0) — audit and repair readers that assume forward-only, root-successor merges:
- `root_labels()` (`hyper.rs:3336`) reads `self.nodes[0].labels` directly; make it
  `resolve()`-safe (read the labels of `resolve(HNode(0))`).
- Grep every other reader of `.labels` / `.edges` / `nodes[0]` that does not `resolve()`
  first; fix any that sit on a correctness path.
- Confirm `merge_with_cause`'s label/edge copy + `representative` update
  (`hyper.rs:2627–2761`) is sound when the folded node is an ancestor/root. Add
  predecessor redirection **only if** the repro or corpus regression shows it is needed;
  otherwise rely on `resolve()` plus the existing invariant that a stale predecessor edge
  remains a sound R-relationship (`hyper.rs:2896–2901`).

## Soundness argument

Counting an inverse-induced successor is *correct*: `n —g→ A` genuinely holds (it is the
inverse of the asserted `A —f→ n`), so `≤1 g` fires a merge only when the model truly
forces two g-successors to be identical. Merges under `≤n` are sound by construction
(they add an equality the constraint entails). Therefore the fix cannot introduce a false
positive. This is nonetheless **gated empirically**: corpus-wide FP must stay 0.

## Testing

- **TDD anchor (RED→GREEN):** a test parsing the inline `funcmerge-cyclic` OFN string
  and asserting rustdl derives `A ⊑ Y` and `A ⊑ Z`. Fails before the fix, passes after.
- **Unit:** a focused `distinct_role_succ` test on a hand-built node with one forward and
  one inverse successor of the same (inverse-related) role, asserting the count is 2.
- **Regression / gates (all must hold):**
  - `konclude_closure_diff` suite: FP = 0 preserved on every fixture; MISSED
    unchanged-or-better (no new misses introduced anywhere).
  - galen: `MISSED 10 → 0` against the Konclude oracle; classification still terminates;
    no material wall-time regression.
  - `cargo clippy -D warnings` and `cargo fmt --check` clean.

## Non-goals / follow-up

- **HF3 — general predecessor-aware merge:** the deferred work to redirect in-edges so
  merges are correct for arbitrary (non-root-successor) nodes. Filed as a scoped
  follow-up issue with this analysis; not part of this fix. This fix does the minimal,
  sound change that closes galen's 10 (and the `funcmerge-cyclic` pattern).
- Not touching the raw tableau (`is_subclass_of`'s non-terminating path on cyclic galen)
  — a separate scalability concern, out of scope here.

## Files

- `crates/owl-dl-tableau/src/hyper.rs` — `distinct_role_succ` (primary), `root_labels`,
  possibly `merge_with_cause`.
- `crates/owl-dl-reasoner/tests/` — the funcmerge regression test with the OFN fixture
  inline as a string constant (and/or a `owl-dl-tableau` unit test for
  `distinct_role_succ`). `ontologies/` is gitignored, so no committed fixture file.
