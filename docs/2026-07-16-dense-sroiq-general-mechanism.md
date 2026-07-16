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

## `≠`-provenance follow-up — SOUND cheap lever, but partial + instance-specific

**Room probe** (optimistic candidate `card_clash_over_ignore_neq(node,succs) ∪ ⋃child_clash_deps`,
bypassing the `≠`-guard): `ore_ont_10019` **31.1 %** of 43048 clashes bounded (mean popcount
**18.7** vs mean decision-depth **83.7** — real backjump room); `ore_ont_12653` **0 %** (stalls
deep, >128 levels, exceeding the 128-bit `DepSet`).

**Soundness of dropping the `≠`-guard — VERIFIED by code read (not assumed):**
`add_neq` is called at **exactly one site** — line 4023 in `generate_at_least` — and
`generate_at_least` sets both successors' `birth_deps = deps` (the `≥n` firing's dep, line
4003). So every engine `≠` is `≥n`-generated and its justification is already carried in the
successors' `birth_deps`, which `over` unions. Therefore replacing the `≠`-guard's `ALL` with
`over` is a **sound** over-approximation (given the single-`add_neq` invariant — assert it), and
no separate `neq_deps` threading is needed. (Hole A's downstream deps are covered by unioning
`child_acc`, already in the probe.)

**Three deflators (advisor, all real):**
1. **128-bit confound** — `opt_bounded` requires `!overflow`, set at depth ≥128, so the 31 %
   is structurally the shallow-clash fraction; the deep 69 % is `DepSet`-capacity-bound, a
   *distinct* lever (wider dep representation), not `≠`-provenance.
2. **Proxy ≠ gate** — the real go/no-go is the **branch-count delta on `KetoneGroup ⊑ AcylGroup`**,
   not "% bounded." 69 % of clashes still at `ALL` means any `ALL`-node in the re-explored loop
   re-stalls it; whether the sound 31 % collapses the stall is unmeasured until the narrowing is
   actually wired to backjumping.
3. **`ore_ont_12653 = 0 %`** — the cross-domain *generality* instance gets zero benefit. This is
   an `ore_ont_10019`-shaped lever, not the general one.

**Status: sound cheap lever identified; build-vs-stop is a proportionality call.** The next step
(if taken) is verdict-affecting + FP-critical: wire the guard-relax (`over ∪ child_acc` in place
of `ALL`, single-`add_neq` invariant asserted) to backjumping behind a flag, then gate on the
`KetoneGroup ⊑ AcylGroup` branch-count delta AND corpus FP=0/MISSED=0 (the non-Horn
`ore_ont_13723` oracle + curated byte-identity). Given the partial (31 %, shallow-only) and
instance-specific (0 % on the generality case) payoff, proportionality argues for a hard
branch-count gate before committing.

## DepSet-capacity lever — MEASURED NO-GO (2026-07-16)

The deep 69 % (and all of `ore_ont_12653`) is bound not by `≠`-provenance but by the **128-bit
`DepSet` overflowing at depth >128** (`d = init_depth − depth ∈ [0,256]`). Widening is a
*distinct*, potentially-more-general lever (backjump precision for any deep search). No read-only
probe exists (overflow already destroyed the sparsity), so it was build-to-measure: a crude
two-word `DepSet` (`u128 + u128_hi`, 256-bit, overflow only at ≥256), uncommitted throwaway.

**Widening ALONE: 0 branch-count reduction** — `PathOfLength4` sat 195842→195842 identical,
`KetoneGroup ⊑ AcylGroup` 36045→36045 identical, all 15 stalled pairs unchanged. It *does*
recover dep precision (12653 exhaustion clashes 0 %→80.9 % bounded), but nothing *consumes* it:
the exhaustion site still returns `ALL` (the `≠`-guard relax was never wired), and the dominant
**disj-only** stalls have dense deps (no room even when precise). Widening and the `≠`-guard
relax are **complementary, not alternatives** — neither reduces branch-count alone.

**The decisive discriminator (advisor) — filter the probe to STALLING decides (`max_depth=256`):**

| instance | group | exhaustion clashes | opt_bounded | mean popcount / depth |
|---|---|---|---|---|
| `ore_ont_10019` | **STALL** | 10825 | **0.2 %** | 18 / 46 |
| `ore_ont_10019` | terminating | 32201 | 54.7 % | 18.8 / 99.5 |
| `ore_ont_12653` | **STALL** | 393 | 80.9 % | 4.3 / 254.7 |

**The room is a mirage where it matters.** On `ore_ont_10019` the 41 % aggregate was almost
entirely in *fast-terminating* decides; in the actual stalls it is **0.2 %**. On `ore_ont_12653`
the 80.9 % is real but sits in the *subsumption* exhaustion clashes — its **headline stall
(`PathOfLength4` sat) is disj-only** (no exhaustion clashes → dead regardless). So the combination
(widening + `≠`-guard relax) would help essentially nothing that actually stalls.

**Verdict: DepSet-capacity is a measured NO-GO** (widening reverted, uncommitted). Note: the
earlier hot-path-tax worry is dropped — EL/Horn deps are `EMPTY`, cheap even at two words; the
blocker is simply that the recovered room is not in the stalls.

## Complete mapping of the dense-SROIQ tail (this session)

1. **Structure-less over-branching** — SOLVED (card-disjunct-atoms, shipped, default-ON).
2. **Structure-bearing disj-only stalls** (`PathOfLength4`, `KetoneGroup` sat; the *dominant*
   stall class) — **dense deps, no backjump room**; the reuse-trap / CDCL search-reuse frontier,
   ruled NO-GO on soundness (`[[next-big-bet-reuse-trap-nominal-termination]]`).
3. **Merge-exhaustion stalls** — the `≠`-guard returns `ALL` (over-conservative; sound to relax
   per the single-`add_neq` code read), and the 128-bit `DepSet` caps deep deps. Both are
   fixable, but the recovered room is a **mirage in the actual stalls** (0.2 % on `ore_ont_10019`)
   and the payoff is **wall-only** (~0 MISSED). NO-GO on proportionality.

No cheap sound lever reduces the dense-SROIQ stalls. The remaining frontier is search-reuse
(caching/CDCL), which is the soundness-ruled-out reuse-trap.
