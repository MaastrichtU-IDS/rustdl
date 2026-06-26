# Precise ≤n merge-causation: dependency-directed backjumping for the at-most rule — Design

**First buildable increment of the Konclude-class architecture** (build-once redesign).
Replaces the at-most (`≤n`) merge rule's conservative `DepSet::ALL` dependency reporting
with precise dependency-directed backjumping, mirroring the wedge's existing ⊔-rule
backjumping. This is Konclude's `CMERGEDependencyNode` branch-tag mechanism realized on
rustdl's `CompletionGraph`/trail. **Deliverable = the architecture (sound, general); the bar
is FP=0 corpus-wide + verdict-preservation, with branch-count/bjgap improvement as the
evidence the mechanism works.** Wine is an incidental stress fixture, not the goal.

## Background: the gap

The Konclude source study + reading rustdl's merge (`crates/owl-dl-tableau/src/hyper.rs`)
established:

- rustdl's `merge_with_cause` (hyper.rs:2121) **already** two-pointer-joins merged labels
  (`merged_deps = c_deps ∪ cause_deps`) and the **nominal** NN-merge (hyper.rs:2735) already
  passes a real causation dep. So most of "the Konclude difference" is already present.
- The **≤n** merge path is the gap: `fn merge` (hyper.rs:2103) passes `cause_deps =
  DepSet::EMPTY`, **dropping the merge causation**. Because it is untracked, a merge-inherited
  `≤n` is tainted (`at_most_tainted = true`, hyper.rs:2188–2194), and `card_clash_deps`
  (hyper.rs:838–866) falls back to `DepSet::ALL`. Additionally `solve_at_most`
  (hyper.rs:2003) returns `DepSet::ALL` at partition exhaustion. These two `DepSet::ALL`
  reports are what defeat dependency-directed backjumping on cardinality-heavy ontologies
  (bjgap≈1) — NOT a full-ancestor fold (the simplified prior model was wrong).
- The ⊔ rule (hyper.rs:1707–1752) already does textbook dependency-directed backjumping over
  a structurally identical nondeterministic enumeration: it assigns a decision level `d`,
  passes `decision_deps = body_deps ∪ {d}`, accumulates child clash deps, backjumps when a
  child clash does not contain `d`, and at exhaustion propagates `combined.remove(d)`.

The increment: **make the ≤n partition enumeration do the same backjumping the ⊔ rule does.**

## Mechanism

`solve_at_most(succs, n, depth)` enumerates partitions of the `≤n`-violating node's successors
into ≤ n mergeable blocks (`partition_rec`), merging each block and recursing via `solve`.
Today every merge passes `EMPTY` and exhaustion reports `DepSet::ALL`. Change:

1. **Decision level.** At `solve_at_most` entry, compute `d = init_depth - depth` (the same
   formula the ⊔ rule uses at hyper.rs:1712) — the decision level of this `≤n` choice.
2. **Track merge causation.** At the partition-complete merge site (`partition_rec`
   hyper.rs:2031–2039), call a new `merge` variant that passes `cause_deps` carrying `{d}`
   **unioned with the ≤n constraint's own derivation dep** (`at_most_dep` of the violating
   node) — i.e. `cause = at_most_dep ∪ {d}`. Then:
   - merged labels carry `c_deps ∪ cause` (existing two-pointer join, now non-empty),
   - the merge-inherited `≤n` carries `at_most_dep ∪ cause` and is **not** tainted (drop the
     `at_most_tainted = true` set at hyper.rs:2193 for this precise path).
3. **Accumulate + backjump.** `partition_rec` already recurses via `solve`. Thread a
   `combined: DepSet` and `any_stalled` (already present) so that, on a partition's `Unsat`,
   it reads `self.clash_deps` (the child clash dep), and:
   - if the child clash does **not** contain `d` → backjump: set `self.clash_deps =
     child_deps`, return immediately (skip remaining partitions), exactly as the ⊔ rule does
     at hyper.rs:1729–1736;
   - else `combined = combined.union(child_deps)`.
4. **Exhaustion.** When all partitions clash with `d` in their deps, `solve_at_most` sets
   `self.clash_deps = combined.remove(d)` (replacing the `DepSet::ALL` at hyper.rs:2003),
   exactly as the ⊔ rule does at hyper.rs:1750.

## Soundness (FP-critical — this is the increment-3 / merge graveyard)

The change *narrows* reported dependency sets, which is the false-`Unsat` (FP) direction: an
under-reported clash dep can trigger an unsound backjump past a genuinely-relevant decision.
So soundness rests on `cause = at_most_dep ∪ {d}` (plus the per-fact `c_deps` and the
survivor's existing `birth_deps`) being the **complete** causation of every merge-derived
fact. Argument, mirroring the ⊔ rule's proven correctness:

- The `≤n` partition choice is a single nondeterministic decision at level `d`; every merge in
  the partition exists *because of* that decision ⟹ `{d}` is necessary and (for the choice
  itself) sufficient, exactly as `d` is for a ⊔ disjunct.
- A merged fact `f` on the survivor is there because (a) `f` was derived on the absorbed node
  (`c_deps`, which already transitively carries that node's `birth_deps`) and (b) the merge
  happened (`cause`). The two-pointer join `c_deps ∪ cause` captures both. The survivor's own
  existence is its pre-existing `birth_deps`.
- The `≤n` constraint's own provenance is `at_most_dep`, folded into `cause`.

**The one hole: ≠-provenance.** A merge's validity also depends on the *absence* of a `≠`
between the merged nodes, and rustdl does not track `≠` provenance. So the precise path is
**unsound wherever a `≠` participates**. The design keeps the existing conservative
`DepSet::ALL` there:

- `merge_with_cause`'s `are_neq` early-out (hyper.rs:2134) already returns `DepSet::ALL` — kept.
- If **any** merge in a partition, or the violating node's successor set, carries a
  decision-derived `≠` or a merge-taint that the precise scheme cannot attribute, the whole
  `solve_at_most` call **falls back to `DepSet::ALL`** at exhaustion (a per-call
  `precise_ok: bool` flag, set false on any conservative trigger). This preserves the exact
  current behavior on the hard cases and goes precise only on clean merges.
- The existing `card_clash_deps` conservative fallbacks (hyper.rs:838–866: own-successor,
  `≠`-only, merge-taint) are **retained** — the increment adds precision at the
  `solve_at_most`/`partition_rec` level, it does not weaken `card_clash_deps`'s guards.

By construction the precise path only ever *removes* an over-approximation on merges that carry
no `≠`/taint provenance gap; every other path is byte-identical to today.

## Gating: env flag for A/B + default

Gate behind `RUSTDL_PRECISE_MERGE_DEPS` (default decided by the gate result):
- **default OFF** during the gate (so the flag-OFF path is provably byte-identical and the
  corpus closure-diff baseline is the current main);
- flip to **default ON** only if the gate passes FP=0 corpus-wide AND shows a real
  branch/bjgap improvement (the precise-card-deps precedent: shipped default-ON after FP=0).

## Components

- `crates/owl-dl-tableau/src/hyper.rs`:
  - `solve_at_most` / `partition_rec`: thread `d`, `combined`, `precise_ok`; the precise
    exhaustion dep; the conservative fallback flag.
  - `fn merge` → a `merge` call carrying the real `cause` on the precise path (reuse
    `merge_with_cause`, which already handles non-empty `cause`).
  - drop the `at_most_tainted` set on the precise path (hyper.rs:2193) guarded by `precise_ok`.
  - `RUSTDL_PRECISE_MERGE_DEPS` flag read (one `*_enabled()` helper, mirroring the existing
    env-gate helpers).

## Testing (negatives-first; FP=0 is the bar)

1. **Verdict-preservation white-box tests** (mirror `precise_card_deps_preserves_{unsat,sat}_
   verdict`): a `≤n` merge scenario where precise backjumping must NOT flip Sat↔Unsat, with
   the flag ON and OFF; plus a `≠`-participating scenario asserting the conservative
   `DepSet::ALL` fallback still fires (precise path correctly declined).
2. **Backjumping-improvement canary**: a synthetic cardinality+disjunction ontology where the
   ⊔-above-a-≤n-merge is independent of the merge — assert the flag-ON run reports a
   bjgap > 1 / fewer branches than flag-OFF, proving the precise path actually backjumps.
3. **FP=0 corpus gate** (the architecture bar): `konclude_closure_diff` with the flag ON,
   byte-identical FP=0/MISSED=0 across the oracled fixtures (wine, sio, pizza, ro, bibtex,
   galen, notgalen, ore-15672 at minimum — the cardinality-bearing ones are load-bearing:
   wine, sio, ore-15672, ore-10908). Flag-OFF must be byte-identical to current main.
4. **Branch/bjgap measurement** (architecture-working evidence, NOT a pass/fail bar): reuse
   the per-pair probe harness to record branch counts flag-OFF vs ON on the cardinality-heavy
   fixtures. A measurable reduction is the evidence the architecture delivers; no reduction
   with FP=0 held means the increment is sound-but-inert (report honestly, reconsider).

## Success criteria

- **FP=0 corpus-wide with the flag ON** (closure-diff byte-identical; verdict-preservation
  tests green) — the hard bar; a single FP is a NO-GO (revert, it's the graveyard).
- Flag-OFF byte-identical to current main.
- A measurable branch-count/bjgap improvement on ≥1 cardinality-heavy fixture (evidence the
  precise path fires and helps). If FP=0 holds but improvement is nil, the increment is a
  sound-but-inert foundation — surface that result, do not flip default-ON.
- On full success: flip `RUSTDL_PRECISE_MERGE_DEPS` default-ON and commit on
  `feat/build-once-redesign` as the first architecture increment.

## What this is NOT

Not the saturation deterministic-expansion cache (Konclude's second edge — a separate, larger
later increment). Not a wine-collapse guarantee (wine is incidental; this is one architectural
correctness/precision improvement, general across the corpus).
