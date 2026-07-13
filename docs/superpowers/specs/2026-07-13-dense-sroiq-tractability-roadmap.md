# Dense-SROIQ Classify Tractability — Roadmap Design

**Status:** revised after advisor review (2026-07-13) — for user review
**Goal:** make rustdl's classifier **decide before the deadline** (bounded +
complete) on dense-SROIQ inputs where the hypertableau wedge currently stalls
and classify falls through to the non-terminating `search.rs` fallback.

> Note on "terminate": the wedge already **terminates** — blocking (`is_blocked`,
> `hyper.rs:1436`) + the depth cap make it return `Stalled` rather than loop. The
> deliverable is *deciding within budget*, not termination per se.

## Problem & measured diagnosis

`classify ore_ont_10019` (47 classes; dense SROIQ: `=n` cardinality, recursive
union definitions, 55 disjointness) does not decide within any practical budget.
Konclude 90 ms, HermiT 360 ms.

`rustdl hyper-sat ore_ont_10019 --per-class-timeout-ms 300`:

```
47 classes -> 14 Sat, 0 Unsat, 33 STALLED (all disjunction branching, merge=0)
  KetoneGroup  branches=1812 depth=76   OxygenAtom branches=1796 depth=75
  match_attempts=116,912,850   fixpoint_passes=48,711   restores≈branches
```

**Corrected cost attribution (per advisor review):**

- **Branch count is modest** (~1600/class over 33 classes) — this is **not** a
  2^depth blowup; the shipped `DepSet` backjumping (`hyper.rs` `solve`,
  `search.rs:244-294`) is already pruning hard.
- **The dominant cost is per-branch re-saturation.** `horn_fixpoint`
  (`hyper.rs:1526`) is *already* the event-driven seminaive drain
  (`docs/hypertableau-seminaive-scoping.md`, shipped: SIO 52 M→2 M matches). The
  117 M here comes from the deliberate **re-seed-per-call**
  (`hyper.rs:1532` clears + re-pushes every node's labels/edges) that `solve`
  invokes at the top of *every* recursive frame (`hyper.rs:2194`).
  `fixpoint_passes ≈ branches` ⇒ ~one full re-seed per branch.
- **Depth 75-76 is a red flag that blocking is NOT firing** on the recursive
  bond-successor chain (subset pair-blocking should cap depth far below 75 if
  successors were label-comparable). Blocking effectiveness is currently
  unmeasured and is the cheapest potential lever.

So the real levers, in cost-attribution order: (1) per-branch re-seed cost,
(2) blocking not collapsing the successor recursion, and only then (3) branch
count (already small).

## What exists (do not rebuild)

- **Hypertableau wedge** — `Unsat`-sound; **backjumping already shipped** in both
  engines. Subset + optional double blocking. `≤n` partition-enumeration merge.
  `horn_fixpoint` is already seminaive/event-driven (re-seeded per call).
- **LabelOracle label cache** (default ON) — prunes 96-100 % of pairs; today's
  main lever.
- **ModelCache** — never-wired stub. **SnapshotCache** — parked default-OFF,
  FP-unsound on disjunction. **CDBL** in `search.rs` — lookup disabled (unsound
  keying). A prior *sound* node-local CDBL was built, hit 19×, moved wall **0 %**,
  and was reverted (`docs/cdbl-plan.md` §A/§B).

## Success criteria & the non-negotiable invariant

- Primary: `classify ore_ont_10019` returns the **complete** hierarchy (= Konclude)
  in a few seconds. **Honest tail gate:** *all 33* stalled classes must decide
  inside the wedge — OR the `search.rs` fallback must itself be bounded — otherwise
  the completeness goal fails on the residual tail.
- Invariant: **FP = 0 must never regress.** Because the curated corpus is mostly
  EL/Horn (where least-model reasoning is canonical and disjunction-FP cannot
  manifest), the FP gate MUST include an **adversarial non-Horn oracle**: the
  ORE regressors that killed SnapshotCache (`ore_ont_13723`-class), diffed against
  Konclude. Curated FP=0 alone is insufficient.

## Decomposition (measure-first; each gated on FP=0 incl. the non-Horn oracle)

Re-measure `ore_ont_10019` after each step — an earlier lever may close the case
and make later ones unnecessary.

### SP0 — Measurement probe (blocking + ontology features) · risk: NONE
- Instrument `blocks_fired` / `block_eligible` per class; run on the depth-75
  stalled classes to confirm whether subset-blocking is firing on the successor
  recursion. Also confirm `ore_ont_10019`'s use of **inverse roles / nominals /
  `=n`** (this gates SP2's soundness form — see below).
- Output decides SP1 vs a blocking fix as the primary lever, and whether pure
  label-set no-goods are even admissible.
- Deliverable: a short findings note; no shipped code.

### SP1 — Incremental horn_fixpoint across branch save/restore · risk: LOW
- **What:** stop re-seeding the whole worklist at every `solve` frame
  (`hyper.rs:1532`); instead snapshot/restore the fixpoint's derived-state and
  worklist across `save`/`restore` so a branch only processes the *delta* its
  decision added. (This revisits the seminaive doc §4/§6 "re-seed per call"
  decision, which was chosen when branching was rare — the opposite of this
  regime.)
- **Why:** attacks the dominant cost (117 M ≈ one re-seed × 55 k branches).
  Plausibly closes `ore_ont_10019` **on its own**; re-measure expecting that.
- **Soundness:** pure performance — **verdict-identical** to today, verified by
  differential testing across the corpus (the pizza/SIO harness catches a dropped
  firing immediately). No FP surface.
- **Note:** more work than it sounds — correct save/restore invalidation of the
  incremental state is the risky part (why the seminaive doc deferred it).

### SP2 — Blocking fix and/or sound UNSAT memoization · risk: MEDIUM, CONDITIONAL
Only pursued for whatever tail SP0/SP1 leave. Two candidate levers, chosen by SP0:

- **(2a) Stronger blocking** — if SP0 shows subset-blocking not firing at depth
  75, add core/label-normalized (or enable/extend double) blocking so the
  bond-successor recursion is capped. Low FP risk, but **completeness-affecting**
  (a blocking change can drop real subsumptions), so gated by `MISSED=0`, not just
  `FP=0`. Likely the higher-value branch given depth-75.
- **(2b) Full-label-set UNSAT memoization** (a *new* no-good layer in the wedge,
  not "reviving" search.rs CDBL) — cache "this generated-successor label-set is
  unsatisfiable" and prune matching successors. **UNSAT is the only
  monotone-safe cache direction** (SAT is anti-monotone). Soundness is
  airtight-or-FP:
  - A no-good is sound to generalize across nodes **only for clashes with no
    edge/successor/`≤n`-merge/`∀`/inverse evidence** (node-local; `cdbl-plan.md`
    §55-63). Edge-dependent clashes must be **edge-qualified** or excluded.
  - If SP0 finds inverse roles / nominals present, pure label-set no-goods are
    **disqualified** — use edge-qualified keys or drop 2b.
  - Cache is **per-pair/per-solve, never classify-global** (a shared cross-pair
    cache reintroduces the `reuse-trap-A1` cross-context surface).
  - Empirical caveat: the prior sound CDBL moved wall 0 %; 2b must argue why the
    wedge regime differs before building, and prove out on measurement.
- Gate: FP=0 on curated **and** the non-Horn adversarial oracle.

### (DROPPED) SAT sub-model caching
Removed. The proposed "forbid reuse where back-propagation alters the cached
node" guard is exactly the mutation-sentinel that `reuse-trap-A1` disproved
(100 FP with **zero** back-prop events — a zero-mutation FP the sentinel is
structurally blind to). SAT reuse is anti-monotone: any extra constraint the
new context imposes can invalidate a cached witness, so sound reuse requires
context-identity, which never fires. The only monotone-safe cache is UNSAT
memoization (folded into 2b).

## Cross-cutting verification

- Each shipped sub-project: default-OFF env flag → validate **FP=0 AND MISSED=0
  (byte-identical curated closures)** on the curated matrix **and** the non-Horn
  ORE-regressor FP oracle + no curated wall regression → flip default-ON in a
  separate reviewed commit.
  - `MISSED=0` is load-bearing for SP1: its only real failure mode is a dropped
    clause-firing on incremental restore, which manifests as a silent MISS /
    non-identical closure, NOT an FP — the FP oracle alone would not catch it.
    The curated corpus is MISSED=0 today, so this is a live differential gate.
- Re-run `hyper-sat` stall-count on the ORE pilot + `ore_ont_10019` classify
  wall/completeness vs Konclude after each step.

## Out of scope

- Wedge `≥n`/inverse-`≥n`/`≈` completeness holes (TODO-HF3) — not what stalls
  this ontology (merge=0).
- Nominal-cardinality semantics.
- Replacing `search.rs` (stays as the complete backstop; may be bounded if the
  tail gate needs it).
</content>
