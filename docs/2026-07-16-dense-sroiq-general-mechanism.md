# Dense-SROIQ stall — the general mechanism (2026-07-16)

Reframed (correctly) as a **general reasoning gap vs Konclude**, not an `ore_ont_10019`
quirk. This note records the measured mechanism, why the cheap sound lever is dead, and
what the honest residual is.

## The pivotal measurement: search-depth, not graph growth

`RUSTDL_TRACE` / `hyper-sat` / `hyper-classify-probe` on `ore_ont_10019`:

- The depth-256 stall is the **`solve` decision stack** (`max_branch_depth = init_depth −
  depth + 1`, decremented only on ⊔ branches and ≤n merges), **not** a completion-graph
  node chain.
- **Blocking works**: `total_blocks_fired ≈ 2.4M`. The graph is small and bounded
  (`is_blocked` ≈ 10 calls/branch). So this is **not** the alehif ancestor-only-blocking
  RSS problem — the two are distinct.
- Every branch is restored (`restores == branches`): the search explores a huge all-failing
  region and hits the depth cap.

## Cross-domain recurrence (generality gate — PASSED, with caveat)

Same `disj=branches, merge=0, restores=branches, depth=256` signature on **`ore_ont_12653`**
(`PathOfLength4`, LOD path analysis — a completely different domain: 195842 branches). But
**`ore_ont_10407`** (also dense-cardinality chemistry) has **0 stalls**. So the pathology is
real and cross-domain, but tied to the **shared-conjunct / recursive-defined-class
structure**, not to "dense cardinality" per se.

## Two faces of one disease

1. **Wedge ⊔ (+ ≤n merge) branching thrash** — `KetoneGroup ⊑ X` (15 pairs) stall in the
   wedge at depth 256. The complete refutation explodes.
2. **Wedge trust-sat incompleteness** — `SulfoxideGroup ⊑ SinicAcid` does *not* stall in the
   wedge; the wedge returns `Sat` fast (a MISS — it drops the constraints that would refute).
   The *full* tableau (`explain`) is the one that stalls (>60 s). Same root: complete
   refutation of cardinality-bearing defined-class subsumptions explodes.

## The cheap sound lever is dead (measured, not assumed)

Candidate: **cardinality-implied ∃-marker body absorption** — add the `∃R.C` marker implied
by each `≥n/=n R.C` (n≥1) conjunct to the defined-class sufficient-direction clause *body*,
so it fires only on nodes with the role structure. Equivalence-preserving (sound). But:

- **Redundant for the structure-less case.** Bare `CarbonAtom` branches ~0 *now* (the "7016"
  was **pre**-card-disjunct-atoms-fix). The shipped fix (evaluatable `AtMost`/`AtLeast` head
  disjuncts → clause skipped when `¬M` is already satisfied) already delivers the pruning the
  ∃-marker trigger would.
- **Cannot touch the residual.** The residual `KetoneGroup` stall is on the **structure-
  bearing** node — the ∃-markers are present there, so the clause fires regardless. The lever
  prunes exactly the nodes that no longer stall, and nothing on the nodes that do.

## The honest residual = two measured-out frontiers, unified with the wine wall

1. **Structure-bearing disjunctive explosion** (`KetoneGroup` sat probe: 249370 disj, merge=0,
   all restored). A satisfiable class whose model search re-explores the same failing
   sub-configurations without sound reuse. Closing it needs sound model/status caching or
   1-UIP CDCL — the **reuse-trap frontier** (see `[[next-big-bet-reuse-trap-nominal-termination]]`,
   `[[conflict-learning-simple-is-weak]]`), earned-NO-GO on soundness.
2. **Merge-dominated subsumption stall** (`KetoneGroup ⊑ AcylGroup`: merge=21650 > disj=14395).
   `solve_at_most`'s partition-exhaustion site reports `DepSet::ALL` → backjumping defeated on
   the merge branches → the **wine bjgap≈1 mechanism** (`[[wine-wall-bjgap1-genuine]]`) in a
   chemistry costume.

**Unification result:** the dense-SROIQ classification tail and the wine wall are the same
unsolved mechanism (search reuse / backjumping-under-merge). This is a real conclusion, not a
failure — it says the general lever is *not* cheap and *not* separately-scoped.

## The one thread that may be genuinely untried

The wine NO-GO was pinned to `merge_with_cause` folding causation into `birth_deps` (the
**nominal** merge). The chemistry merge branches here come from `solve_at_most`'s
**cardinality partition-exhaustion**, which deliberately kept `DepSet::ALL` (precise-card-deps
narrowed only the ≤n *pre-check* site, not this one). Whether narrowing `solve_at_most`'s
exhaustion deps — with the shadow-superset soundness discipline — cuts the merge-branch
backjumping on `KetoneGroup ⊑ AcylGroup` is an **untried, distinct** measurement. Gate any
build on that branch-count delta first (the same measure-before-build discipline that killed
the ∃-marker lever on paper here).

## The untried thread, MEASURED — NO-GO (2026-07-16)

Built a **read-only, verdict-preserving** probe (`RUSTDL_AT_MOST_EXHAUST_PROBE`, default OFF):
at the `solve_at_most` exhaustion site compute the real narrowing candidate
`card_clash_deps(node,succs) ∪ ⋃child_clash_deps` and record whether it is bounded
(sub-`ALL`) — WITHOUT using it (real `clash_deps` stays `DepSet::ALL` ⇒ byte-identical
verdicts). Advisor-gated: measure before building the flag + backjump wiring.

Two desk checks first, both favorable: (a) `solve_at_most`'s merge passes `DepSet::EMPTY`
cause ⇒ the wine `birth_deps` pollution (`merge_with_cause` fold) does **not** apply here —
genuinely distinct site; (b) the `DepSet::ALL` here was "Hole A" in
`backjump-reconcile-2026-06-06.md` — reasoned-and-reverted (deeper failures depend on
broader-graph decisions not in `succs`/`parent`), never measured, and the ruling was
wine-scoped (nominal). My candidate unions the child-partition deps, which closes Hole A's
`card_clash_deps`-only under-report.

**Result (classify path, `precise_card_deps` ON):**

| ontology | exhaustion clashes | `card_clash_deps` alone bounded | full candidate bounded |
|---|---|---|---|
| `ore_ont_10019` | 43730 | **0 (0.0 %)** | **0 (0.0 %)** |
| `ore_ont_12653` | 393 | **0 (0.0 %)** | **0 (0.0 %)** |

**`local_bounded = 0` is the killer:** `card_clash_deps(node,succs)` *itself* overflows to
`ALL` on every exhaustion clash — before the child union even matters. So reusing it at the
exhaustion site gives backjumping **zero room** on the actual stalling instances.

**Guard attribution — the barrier is NOT wine, and NOT Hole B (both predicted, both
falsified by measurement):**

| ontology | exhaustion clashes | `at_most_tainted` | own-succ (Hole B) | **`≠`-only** | `over` overflow |
|---|---|---|---|---|---|
| `ore_ont_10019` | 43576 | 0 | 0 | **43576 (100 %)** | 0 |
| `ore_ont_12653` | 393 | 0 | 0 | **393 (100 %)** | 0 |

`card_clash_deps` returns `ALL` via the **`≠`-only distinctness guard** (`are_neq(a,b) &&
!labels_disjoint(a,b)`) on 100 % of clashes. Mechanism: `≥n R.C` with a *single* filler `C`
generates `n` **same-labelled** successors forced pairwise-`≠` to count as `n` distinct. The
`≠`-forced distinctness is real, but its **provenance (why they are `≠`) is not tracked as a
dep**, so `card_clash_deps` conservatively returns `ALL`.

This is **distinct from wine** (nominal `birth_deps` pollution — `at_most_tainted=0` and the
guard trips *before* the `over`/`birth_deps` union) **and distinct from Hole B** (own-successor
— `ownsucc=0`; the `≥n` successors *are* own-generated). It is the **`≠`-provenance gap**.

**Verdict: NO-GO on reusing `card_clash_deps`** — measured, decisive. The root barrier is
narrower and more nameable than "the wine frontier": it is the untracked `≠`-forced-distinctness
provenance on `≥n R.C` single-filler successors.

**The genuinely-next lever (untried, its own scoping question):** track a `≠`-assertion
dependency (`neq_deps` — the dep of the `≥n` constraint that forced the successors distinct),
so `card_clash_deps` can return a *bounded* dep for `≠`-forced-distinct successors instead of
`ALL`. Whether that bounded dep then **excludes on-stack decisions** (the actual backjump-room
question) is a fresh measurement — gate a `≠`-provenance prototype on it before building the
narrowing, exactly as this probe gated the `card_clash_deps`-reuse idea. This is a bigger,
threaded change (`≠` provenance) with its own soundness surface (under-report ⇒ FP); it is
neither the wine re-architecture nor a trivial fix.

The probe is a read-only diagnostic (`RUSTDL_AT_MOST_EXHAUST_PROBE`, default OFF, verdicts
byte-identical — confirmed OFF-vs-ON `entailed=157 stalled=15` on `ore_ont_10019`) retained
for reproducibility.
