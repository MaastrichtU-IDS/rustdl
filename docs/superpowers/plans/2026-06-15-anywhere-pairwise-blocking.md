# Plan: Anywhere (pairwise/double) blocking in the main SROIQ tableau

Date: 2026-06-15
Branch: worktree-agent-aa9e9b0dee52b1c16
Status: in progress

## Goal

Change the main tableau's `is_blocked` (`crates/owl-dl-tableau/src/lib.rs:765`)
from **ancestor-scoped** pairwise/double blocking to **anywhere-scoped**
pairwise blocking: a node `y` may be blocked by ANY node `x'` *created before*
`y`, satisfying the SAME pairwise conditions (1)–(4). This keeps the completion
graph small on large/generative ABoxes (family's 1848 individuals) so the main
tableau terminates instead of hanging. Behind `RUSTDL_ANYWHERE_BLOCKING`,
**default OFF**.

## Soundness foundation (verified before designing)

1. **`NodeId.index()` IS creation order.** `push_node_with_parent`
   (graph.rs:306) assigns `id = nodes.len()` and only ever pushes; rollback
   `truncate_nodes` (graph.rs:333) removes the tail. So at any point the live
   node ids are `0..len` and `x'.index() < y.index()` ⟺ "x' created before y".
   This is strict, total, and acyclic — gives termination, no separate `order`
   field needed (the wedge in `hyper.rs` carries an explicit `order`; we don't).

2. **Generation is gated ONLY on direct `is_blocked(node)`.** All six callers
   (rules.rs:713/788/892/1147/1317/1408) are `if ctx.is_blocked(node) { skip }`.
   There is NO indirect/ancestor-blocked suspension anywhere in the crate
   (grep for `indirect|ancestor_blocked|suspend` returns only unrelated hits).
   Consequence (advisor's analysis): every node that is NOT directly blocked is
   fully expanded, so its label set is complete and it is a VALID blocker. The
   classic anywhere-blocking soundness bug (a blocker whose ancestor is blocked
   has stale labels) **cannot bite here** because no node has stale labels —
   nothing is suspended on ancestor-block. Therefore conditions (1)–(4) + strict
   creation-order suffice; we do NOT add a recursive "blocker must be unblocked"
   check (that would only under-block → risk non-termination).
   - Direct-blocked blocker: also auto-handled. (2)(3)(4) are transitive, so if
     any candidate qualifies, the minimum-index qualifying candidate is provably
     not itself directly-blocked. The boolean "a qualifying candidate exists"
     already coincides with "an unblocked qualifying candidate exists."

3. **Candidate exclusions (FP-critical):**
   - `parent = None` (roots / orphans) — already excluded (condition 1).
   - `merged_into.is_some()` (redirected nodes) — their state lives on the
     representative; a stale label set. Exclude.
   - **nominal-labelled** nodes — a node carrying a `Concept::Nominal(a)` label
     denotes a specific individual `{a}`; blocking `y` against it asserts a loop
     back to `a`, which is unsound (`y` need not be `a`). Exclude any candidate
     whose label set contains a `Nominal` concept.

4. **Conditions (2),(3),(4) preserved EXACTLY** — only the SCOPE of candidate
   `x'` changes (ancestor-chain → any earlier non-excluded node). (2) parent_role
   match, (3) `L(y) ⊆ L(x')`, (4) `L(parent(y)) ⊆ L(parent(x'))`. The `label_sig`
   subset prefilter is kept verbatim.

5. **Family termination is also the indirect-blocking soundness canary.** Family
   must return **inconsistent**, not merely terminate. Terminating-but-consistent
   = a clash was masked by an unsound blocker = soundness failure.
   (Closing family via the default `is_consistent` ALSO needs separate wedge
   role-chain work — out of scope. Scope here: the MAIN tableau path terminates
   and is sound.)

## Design

### Gate
Module-level `fn anywhere_blocking_enabled() -> bool` in `owl-dl-tableau/src/lib.rs`,
opt-IN polarity (default OFF):
```rust
pub fn anywhere_blocking_enabled() -> bool {
    std::env::var_os("RUSTDL_ANYWHERE_BLOCKING").is_some_and(|v| v == "1")
}
```
Cached once into a new `TableauContext` field `anywhere_blocking: bool`
(reading env on every `is_blocked` call would be far too hot). Set in all three
constructors (`new`, `with_tbox`, `with_tbox_and_hierarchy`).
Mirror gate fn in `owl-dl-reasoner/src/lib.rs` alongside the other `*_enabled()`
(diagnostic / discoverability; the tableau reads its own cached copy).

### `is_blocked` dispatch
`is_blocked(&self, y)` keeps its current ancestor walk as
`is_blocked_ancestor(y)` (rename-by-extraction: move the existing body into a
private fn). New `is_blocked_anywhere(y)`. Top of `is_blocked`:
```rust
if self.anywhere_blocking { self.is_blocked_anywhere(y) } else { self.is_blocked_ancestor(y) }
```

### `is_blocked_anywhere`
Two-phase per the advisor (correctness first, then index):
- **Phase A (this plan, correctness):** O(N) scan of candidate ids
  `0..y.index()`. For each candidate `x'`:
  - skip if `x'.parent` is `None` (root/orphan) — condition (1);
  - skip if `x'.parent_role != parent_role(y)` — condition (2);
  - skip if `merged_into(x')` is `Some` (redirected);
  - skip if any label of `x'` is a `Nominal` concept (nominal exclusion);
    also skip if any label of `y` is `Nominal` (a nominal `y` must not be
    blocked — it denotes a fixed individual);
  - `label_sig` prefilter (kept verbatim from the ancestor path): reject if
    `yl_sig & !x_sig != 0` or `yp_sig & !xp_sig != 0`;
  - full subset scan: `L(y) ⊆ L(x')` and `L(parent(y)) ⊆ L(parent(x'))`.
  - first match ⇒ blocked. Strict `x'.index() < y.index()` guarantees acyclicity.
  The `&self` signature is unchanged (no stats mutation needed; counters are
  cfg-gated and the existing ancestor path already bumps them — anywhere path
  bumps the same counters via the macro which is a no-op without the feature).
- **Phase B (follow-up task, perf):** add `block_index: HashMap<Role,Vec<NodeId>>`
  to `CompletionGraph`, maintained in `push_node_with_parent` (push when
  `parent_role` is `Some`) and `truncate_nodes` (pop tail entries `>= new_len`).
  `is_blocked_anywhere` then iterates only `block_index[parent_role(y)]` filtered
  by `< y.index()`. MUST re-run the full corpus gate and confirm byte-identical
  verdicts vs Phase A. Index rollback is the unsound direction (stale entry →
  block against a dead node → masked clash); guard the scan with
  `cand.index() < y.index()` and arena-bounds regardless of index state.

## TDD tasks (bite-sized)

### T1 — gate plumbing + extraction (no behaviour change)
- Add `anywhere_blocking_enabled()` (tableau) + cached field + set in 3 ctors.
- Add mirror `anywhere_blocking_enabled()` in reasoner lib.rs.
- Extract current `is_blocked` body into `is_blocked_ancestor`; `is_blocked`
  dispatches (anywhere arm stubbed to call ancestor for now OR returns the new
  fn once T2 lands — to keep T1 a pure no-op, dispatch both arms to ancestor).
- Test: `anywhere_gate_defaults_off` (env unset ⇒ `anywhere_blocking == false`).
- `cargo test -p owl-dl-tableau` green; fmt; clippy.

### T2 — `is_blocked_anywhere` (Phase A, O(N) scan) + white-box unit tests
Write tests FIRST (all in `lib.rs` `mod tests`, constructing `TableauContext`
directly and forcing the field on via a test-only setter or by constructing with
the env set — use a small `set_anywhere_blocking(bool)` test helper to avoid env
flakiness across parallel tests):
- (a) `anywhere_blocks_non_ancestor_earlier_node`: build root r0; two siblings
  s1, s2 as successors of DIFFERENT parents p1, p2 (so s2 is NOT a tree-ancestor
  of s1), same parent_role, `L(s1) ⊆ L(s2)`, `L(p1) ⊆ L(p2)`, s2 created before
  s1. Assert `is_blocked(s1)` is TRUE under anywhere, FALSE under ancestor-only.
- (b) `anywhere_does_not_block_when_parent_condition_fails` (inverse-role guard):
  same as (a) but `L(p1) ⊄ L(p2)` (parent condition (4) fails). Assert FALSE
  under anywhere. (This is the condition that makes it sound with inverses.)
- (b2) `anywhere_does_not_block_on_parent_role_mismatch`: parent_role differs ⇒
  FALSE.
- (b3) `anywhere_excludes_nominal_candidate`: candidate x' carries a Nominal
  label ⇒ not a blocker even when (1)–(4) hold.
- (b4) `anywhere_excludes_merged_candidate`: candidate merged_into Some ⇒ skipped.
- (c) termination unit: a tiny cyclic-TBox existential (`A ⊑ ∃r.A`) that under
  ancestor blocking blocks on the tree-ancestor — assert it STILL terminates and
  blocks under anywhere (same verdict, smaller/equal graph). Use the existing
  saturate/expand entry if available; else a hand-built graph asserting
  `is_blocked` fires once the loop label set repeats.
- (d) `anywhere_real_clash_stays_unsat`: a node with `{A, ¬A}` (or a genuine
  clash reachable through generation) stays Unsat — blocking must not hide it.
  (Blocking only suppresses GENERATION; a clash in an existing label set is
  found by the clash rule regardless. Assert verdict unchanged ancestor vs
  anywhere on a small unsat fixture.)
- Implement `is_blocked_anywhere` to make them pass.
- fmt; clippy; `cargo test -p owl-dl-tableau`.

### T3 — termination acceptance on family (main-tableau path)
- An `#[ignore]`d integration test (corpus is gitignored / fetched on demand)
  in `owl-dl-reasoner` that loads `ontologies/real/family.ofn`, forces the MAIN
  tableau consistency path (bypass the wedge `is_consistent` short-circuit —
  call the main-tableau `decide`/consistency directly, or set the env that
  disables the wedge), with `RUSTDL_ANYWHERE_BLOCKING=1` and a generous wall cap.
  Assert it TERMINATES within the cap (vs hanging) and document the verdict
  (target: inconsistent). If the main-tableau entry isn't directly callable,
  measure via a CLI/bench invocation in the report instead and document exactly
  what was run. This is the operational soundness canary.

### T4 — Phase B index (perf) — OPTIONAL within scope, gated on T2+gate green
- Add `block_index` to `CompletionGraph`; maintain on create/truncate.
- `is_blocked_anywhere` iterates the bucket.
- Re-run the corpus gate; verdicts MUST be byte-identical to Phase A.
- If index rollback proves fiddly, SHIP Phase A (O(N)) — correctness over speed.

## SOUNDNESS GATE (mandatory, do not skip, do not merge before pass)
Run the full corpus closure-diff BOTH with gate ON and (sanity) OFF:
```sh
RUSTDL_ANYWHERE_BLOCKING=1 cargo test -p owl-dl-reasoner \
  --test konclude_closure_diff --release -- --ignored --nocapture
```
Every fixture must stay **FP=0 and MISSED=0**: galen 27997, notgalen 32739,
sio 8904, ore-10908 6001, ore-15672 142, wine 653, pizza 499, alehif 247,
shoiq-knowledge 449, ro 158, sulo 51, bibtex 16. Any FP or any lost MISSED=0 ⇒
STOP (soundness/completeness regression). Report classify wall deltas.
Then fmt + clippy clean under `-D warnings`. Then Opus self-review of the diff
focused on inverse-role + cardinality + dynamic/ordering soundness.

## STOP after the gate. Do NOT merge, do NOT push, do NOT delete the worktree.
