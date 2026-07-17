# Wedge-reuse measurement — findings (2026-07-17)

The architecture-deciding measurement mandated by the START-HERE block of
`2026-07-17-saturator-backward-propagation-scoping.md`: **rule out wedge-reuse
before building Sequoia-style contexts in the fact-based saturator.**

Binary: `target/release/rustdl` @ 831a9c9 (engine unchanged since build; only
docs commits since). All runs on this machine (32-core/251GB).

## Finding 1 — the wedge IS symmetric-capable, deterministically

4-axiom counterexample (`scratchpad/sym-counterexample.ofn`):
```
X ⊑ ∃R.C ;  ∃R.X ⊑ D ;  ∃R.D ⊑ E ;  Symmetric(R)   ⟹  X ⊑ E
```
- EL saturator (closure): **MISSES `X⊑E`** (as the counterexample predicts — no
  backward propagation).
- Wedge (`hyper-classify-probe`): derives `X⊑E` as a **subsumption with
  `pairs_branched: 0`** — i.e. via its deterministic Horn hyperresolution
  fixpoint, no ¬B-injection branching.
- Differential (drop `Symmetric(R)`): subsumptions 1 → 0, and `explain` X⊑E
  flips yes→no. Confirms the 1 subsumption IS the symmetry-dependent `X⊑E`.

**The wedge already does the backward/symmetric propagation the fact-based EL
saturator lacks.** (SP1 wired domain/range through symmetric+inverse roles; this
confirms it reaches named-class subsumption, not just consistency.)

## Finding 2 — the giant target is fully Horn

`ore_ont_3914` (the dense 12k the scoping doc names as the tractability crux):
- `clause-stats`: **12857 clauses, 0 disjunctive, 0 deferred** — fully Horn.
- `tbox-stats`: 12709 concept rules, 0 nominal, 0 residual GCIs.
- ~12.4k named classes ⇒ O(n²) ≈ 161M ordered pairs.

## Finding 3 — per-class wedge cost is tiny; the DNF is the O(n²) pair loop

`hyper-sat ore_ont_3914` (per-class satisfiability, O(n)):
- **12437 classes sat-checked in 193.5 ms total; total_branches 0;
  classes_branched 0; stalled 0.**

The wedge saturates every class deterministically and cheaply. The classify DNF
is therefore the **n² pair multiplier** (161M pairs × per-probe overhead), NOT a
symmetric calculus gap and NOT per-pair search blowup. This is exactly the
advisor's START-HERE hypothesis.

## Reframing (for decision)

- Option B in the scoping doc = "reimplement Sequoia contexts in the fact-based
  EL saturator (multi-week)". But the wedge **already is** a context-graph-shaped
  Horn hyperresolution engine that handles symmetric/inverse and terminates
  cheaply. Building context machinery in the *other* engine would be redundant.
- **CORRECTION (advisor 2026-07-17): the wedge is refutation-per-pair, NOT
  one-pass emission.** `hyper-classify-probe` injects ¬sup for *every* ordered
  pair and checks unsat; `subsumptions: 1` = 1 of `pairs_tested: 12` refutations
  closed. There is no free "harvest the closure" — a true one-pass forward Horn
  closure is a *build* (= roadmap D2a Phase A, a Pred-rule extension), not a
  read-off. So "reuse the wedge" means **get the pair count below n²
  (pruning/traversal)**, not harvest a closure.
- **Completeness is NOT the crux on Horn.** Fully-Horn (`deferred:0`,
  `disjunctive:0`) ⇒ hyperresolution refutation is complete; the probe's "Sat
  not sound" caveat doesn't bite here. **Tractability (avoiding n²) is the crux.**

## The decision (reframed)

Both live options **reject Option B** (Sequoia contexts in the fact-based
saturator — wrong engine: the back-edge needs per-successor structure the wedge
already has and the fact-based saturator lacks). The live fork is:

- **A. Cheap pruning on the already-complete Horn wedge** — cut the pair count
  below n² (the label heuristic / traversal already do this; question is how well).
- **D2a Phase A. Build the Horn-SROIQ forward closure** — one-pass, list
  subsumptions from a saturation, sound-by-construction on Horn.

## Finding 4 — `ore_ont_3914` does NOT DNF on current main (169 s), but is a SCALE case

`classify ore_ont_3914` (default, 32-core):
- **Elapsed 169 s (2:49), completes — NOT a DNF.** RSS **166 GB**.
- `subsumption: saturation=20682 tableau=0` — tableau/wedge **never consulted**.
- `satisfiability probes: saturation=12437 tableau=0`.
- `label heuristic: pruned=8,835,188 pass_through=0 misses=0` — near-perfect prune.
- `wall breakdown ms: label_cache_build=1580 tier_walk=152720` — the tier walk dominates.

So the doc's "all DNF on the O(n²) per-pair path" is **stale for this ont on
current main** (advisor's warning confirmed). The real cost here is the top-down
**tier-walk + 166 GB RSS** = a scale/memory case (D4), not a symmetric-calculus gap.

Symmetric role usage in `ore_ont_3914`: 1 symmetric role (`RO_0002220`), **NOT
write-only** — it appears in 9 `EquivalentClasses` definitions whose sufficient
direction (`∃R.C ⊓ D ⊑ A`) is exactly the antecedent trigger a symmetric
back-edge would feed. 0 domain, 0 chains/inverse.

## Finding 5 — the DECISION-FLIPPING mechanism: classify never *enumerates* symmetric candidates

On the 4-axiom counterexample:
- `subclass X E` = **yes** (single-pair path forces ¬E injection → wedge refutes).
- `classify` = **0 edges, misses X⊑E** — and NOT because of the label heuristic:
  with `RUSTDL_LABEL_HEURISTIC=0` AND `RUSTDL_HYPERTABLEAU_TRUST_SAT=0` it STILL
  emits 0 edges and runs `subsumption: saturation=0 tableau=0` — **it never tests
  the (X,E) pair at all.**

**Why:** classify's top-down walk (`find_direct_parents_top_down`) enumerates
candidate pairs from the **saturation closure** (told/EL subsumers seed the
lattice). The closure misses the symmetric edge, so `(X,E)` is never a candidate,
so the symmetric-capable wedge is never asked. Confirmed contrast: EL-derivable
edges (B⊑K, G⊑K) DO print; the symmetric one does not.

**Consequence for the A-vs-B decision:** "the wedge handles symmetric per-pair"
does NOT cheaply give classification completeness. Completeness needs symmetric
subsumptions to be **enumerated as candidates**, which requires either (i) testing
toward n² pairs through the wedge (the explosion the top-down walk exists to
avoid — ~154M wedge calls at 12k) or (ii) a symmetric-complete **forward closure**
to seed the candidates — which is the backward-propagation closure work itself
(D2a Phase A / the Pred rule). Option A ("fewer pair-probes on the wedge") does
not, by itself, close the completeness gap.

## Finding 6 — the go/no-go diff: methodology + confounds (the measurement is only trustworthy after defusing three)

Oracle: `hyper-classify-probe --dump-subsumptions` is **complete on fully-Horn
input** (`disjunctive:0 deferred:0` ⇒ ¬sup refutation is complete), and was shown
strictly stronger than classify (caught the counterexample's `X⊑E`). Diff its
subsumption set `P` against classify's transitively-closed `direct` edges `C`.
`P\C` = subsumptions classify MISSES.

Three confounds each inflate `P\C` with non-misses; all must be filtered or the
number is garbage (each caught empirically):
1. **DKey datatype synthetics** (`urn:rustdl-dkey:*`) — the probe counts them,
   classify filters them from the reported hierarchy (`reportable_class_iris`).
   Filter from both sets.
2. **Unsatisfiable classes** — an unsat class is `⊑` everything; the probe emits
   the full explosion, classify collapses it to an `unsat` line. E.g.
   `ore_ont_1618`: 26 unsat classes produced a spurious `P\C≈650`. Exclude any
   pair touching a class classify reports `unsat`.
3. **Target-set contamination** — `work/sym` (868 onts) is bucketed by *some*
   symmetric signal but is NOT all symmetric-role-bearing: `ore_ont_1618` has
   **zero** `SymmetricObjectProperty`. The symmetric question needs onts that
   **declare a symmetric role used as an antecedent** (`∃R` on a SubClassOf LHS,
   inside an `EquivalentClasses` definition, or a domain axiom). Correct target:
   **63 such onts ≤1500 classes.**

Clean sanity points before the full run: `ore_ont_7464` (160 cls, Horn,
symmetric) → `P=C=63`, `P\C=0` (agreement); the synthetic counterexample →
`P\C=1` (the known `X⊑E` miss). Harness: `scratchpad/sym_diff.py`.

## RESULT (the discriminating measurement) — no confirmed symmetric miss; payoff bounded

Target refinement: of 63 onts that **declare a symmetric role used as an
antecedent** (≤1500 classes), only **6 are Horn** (symmetric overwhelmingly
co-occurs with disjunction; on non-Horn the probe is a sound lower bound, not a
complete oracle). 4 of the 6 are small enough for n² probing.

**Clean complete-oracle diff (probe vs classify), 4 small Horn symmetric onts:**

| ont | classes | probe | classify | P\\C (miss) | C\\P |
|-----|--------:|------:|---------:|-----------:|-----:|
| ore_ont_6589  | 322 | 87  | 87  | **0** | 0 |
| ore_ont_13132 | 422 | 184 | 184 | **0** | 0 |
| ore_ont_13581 | 702 | 512 | 512 | **0** | 0 |
| ore_ont_6298  | 909 | 895 | 895 | **0** | 0 |

**classify == the complete Horn oracle. Zero misses.** (Plus `ore_ont_7464`, 160
cls: 63=63.)

**Non-vacuousness check — symmetric is INERT on these onts.** Removing the
`SymmetricObjectProperty` axiom changes neither classify's hierarchy nor the
**complete probe's** subsumption count (delta=0 on all 4, both engines). The
symmetric role produces **zero subsumptions**. So the mechanism proven on the
adversarial synthetic (Finding 5) is never exercised here.

**Broad sweep — symmetric non-inertness across the small symmetric-antecedent
set (probe with vs without the symmetric axiom):** **52 onts measured, delta = 0
on every one; 0 non-inert; 6 stall-timeouts** (the dense stallers, e.g.
`ore_ont_6437`/`ore_ont_15273`). No ontology has a single subsumption that depends
on its symmetric role. (`scratchpad/sym_impact.txt`.)

**Why the synthetic bites but real onts don't:** the counterexample is
adversarial — X and E have zero told/closure relationship, so classify never
enumerates the candidate. On real onts the symmetric definitions are structured
so any symmetric-reachable subsumption sits between already-linked classes (and,
empirically, symmetric adds no subsumption at all — the roles are effectively
write-only for classification, confirming the roadmap's grep hint).

## Finding 7 — pattern-bearing onts: corpus-wide structural scan + the giant

The advisor's sharp catch: Findings 4–6 are all on *inert* onts (symmetric never
exercised), so they're vacuous for the mechanism. I need pattern-bearing onts.

**Corpus scan (235 onts that actually declare a symmetric role; `work/sym`'s 868
includes 633 with none, e.g. `ore_ont_1618`):** only **24** are even
*potentially* non-inert (have BOTH a forward `∃R` AND an antecedent `∃R`/domain on
a symmetric role). The other **211 are trivially inert** (symmetric never an
antecedent trigger ⇒ no back-edge can fire ⇒ cannot affect classification).

**The giant `ore_ont_3914` (the doc's canonical pattern-bearing case) — inert by
construction.** Precise necessary condition for symmetric to add a class
subsumption (traced from the counterexample): a forward `n⊑∃R.C` and a definition
`G≡∃R.T⊓K` with `n⊑T` and `C⊑K` in the hierarchy. Computed from classify's
hierarchy: **0 hits.** The symmetric back-edge chain is never satisfiable ⇒
symmetric changes nothing (independent of the ambiguous classify-delta=0).

**Complete-oracle confirmation on the giant's actual pattern:** extracted the
symmetric-axiom module of `ore_ont_3914` (37 axioms, real ORE-derived); classify
== complete probe (9=9), and probe-delta with/without symmetric = 0.

**The remaining 5 "flagged" onts are structural false positives / unverifiable:**
a cheap structural sub-check flagged `ore_ont_9835` (323) + 4 SIO onts (1 each),
but the check has an **over-generalization bug** — it concludes `C⊑G` on the
*filler* class, whereas the sound conclusion is on the *subject* `n` via a
**two-definition** chain (the same witness-vs-class trap the architecture finding
names). Confirmed false positive: on `ore_ont_7532` the flagged `SIO_000275⊑
SIO_000342` has its **reverse** (`SIO_000342⊑SIO_000275`) already in classify's
output — my check flagged the wrong direction. And the flagged onts are either
46k-class giants or non-Horn (47 disj clauses) where the complete per-pair check
(`¬G` injection = disjunctive) **stalls >120s** — so they cannot be oracled
cheaply. No *confirmed* real symmetric miss exists anywhere in the measured corpus.

**Fundamental verification limit (honest):** the complete check of a symmetric
candidate pair `Z⊑G` (G a defined class) injects `¬G = ∀R.¬C ⊔ ¬D` — disjunctive,
which stalls the tableau on the giants; and the forward closure classify uses is
provably symmetric-*incomplete* (the counterexample). So on the giants there is
**no cheap complete oracle** — the NO-GO rests on: (a) every oracle-able ont inert,
(b) the one cleanly-analyzable giant showing zero hits on the (imperfect, filler-
over-approximating) structural check plus a complete-oracle-inert extracted module,
(c) 211/235 trivially
inert, (d) zero confirmed misses, (e) the mechanism firing only in an adversarial
synthetic — not on a proof that no miss exists at 46k+ scale.

## CONCLUSION → the recommendation the measurement supports

**Scope: this measures the SYMMETRIC sub-increment only** (the Phase A inc-2
"symmetric back-edge" the scoping doc names). The backward-prop engine also serves
**inverse** roles, which dominate the OBO tail — see the inverse note below; the
symmetric result does not by itself settle the inverse case.

1. **Option B (Sequoia contexts in the fact-based saturator): REJECTED — robustly,
   independent of every flag below.** Finding 1 (the wedge already derives the
   symmetric counterexample per-pair) + Finding 5 (classify's miss is a
   *candidate-enumeration* gap, not an engine gap) mean: **if** a symmetric miss is
   ever real, the fix is candidate-enumeration / wedge-routing (Option A — reuse the
   engine that already handles symmetric); if it's not real, nothing is needed.
   Either branch rejects rebuilding contexts in the fact-based saturator.

2. **NO-GO on the backward-propagation engine build as a *symmetric-completeness
   fix* — on expected-payoff-vs-cost, not on a proof of inertness.** Payoff is
   upper-bounded: of 235 symmetric onts, **211 are trivially inert** (symmetric is
   never an antecedent `∃R` trigger) and **24 are even potentially non-inert**; of
   those, every case that is oracle-able shows **zero classify misses**, and no
   *confirmed* real miss exists anywhere. Against a multi-week, FP-net-less engine
   build, that expected payoff does not justify the cost. **My tooling is
   confirm-only** (a probe/module diff can prove a miss exists, never that none
   does), so this is "zero confirmed misses + bounded payoff," **not** "proven
   inert."

3. **D4 scale/memory is the *plausible next target* for this tail, not an
   established fix.** `ore_ont_3914` **completes** (169 s, 166 GB RSS, tableau never
   consulted) — it is not even in the DNF tail; its cost is scale, not calculus. But
   I have not characterized *why* the actually-DNF-ing {disjoint,symmetric} onts DNF
   (memory vs time, which onts). The 166 GB RSS on a 12k Horn ont is the striking,
   independently-valid datum; whether anywhere-blocking (D4) is the fix needs its own
   measure-first pass.

**Inverse (MEASURED 2026-07-17, follow-up):** complete-oracle probe-vs-classify
diff across **53 small Horn inverse-using onts** (of 311 inverse-using in
`work/sym`; sampling caveat — `work/sym` is symmetric-biased, but OBO onts
typically carry both). **28/29 measured onts PmC=0** — classify == the complete
Horn oracle, no inverse misses. The lone PmC>0 (`ore_ont_13077`, 2 pairs) is **NOT
inverse** — it's the union-LHS bug below. So classify is inverse-complete on the
measured set, consistent with `[[inverse-aware-classification-no-win]]`
(tableau=0; saturator answers 100 % of positives). **The whole-engine NO-GO now
covers both symmetric and inverse.** (Two clause-giants `ore_ont_13912` [90k
clauses] / `ore_ont_9881` [1.5M] and `ore_ont_12901` excluded/timed out —
un-oracle-able, same principled limit as the symmetric giants.)

## BONUS — a REAL classify completeness bug found en route (union-LHS, orthogonal)

The one genuine miss surfaced this session (everything else was confounds): on
`ore_ont_13077`, `SubClassOf(ObjectUnionOf(Osteuropaeer Lateinamerikaner)
Auslaender)` entails `Lateinamerikaner ⊑ Auslaender`. `subclass` = yes; **classify's
hierarchy omits it.** Minimal repro (`scratchpad/union-lhs.ofn`):
`SubClassOf(ObjectUnionOf(:A :B) :C)` (Horn, 2 clauses) → `subclass A C` = yes,
`classify` emits **0 edges** (misses `A⊑C` AND `B⊑C`).

**Root cause = Finding 5's candidate-enumeration gap, biting on real data via a
different construct.** `(A⊔B)⊑C` is left as a **residual-`or` GCI**
(`tbox-stats`: `residual_or:1`) — `absorb.rs` does not distribute union-on-LHS,
the saturation closure never derives `A⊑C`, so classify's top-down walk never
enumerates the pair (the tableau catches it only per-pair via `explain`).
**Fix is standard and sound**: distribute `(A⊔…)⊑C` into `A⊑C ∧ B⊑C ∧ …`
(sound equivalence, Horn). Low prevalence in this sample (1/868) but a genuine
completeness gap; orthogonal to the backward-prop question.

### FIX SHIPPED (2026-07-17, TDD)

Two complementary changes, both sound-by-construction (each emitted axiom is
entailed; the removed axiom is logically equivalent to their conjunction):
- **`crates/owl-dl-core/src/disjunctive_antecedent.rs`** (new pass, wired into
  `convert_ontology` before `derive_disjunction_existentials`): splits
  `SubClassOf(ObjectUnionOf(D₁…Dₙ), C)` → `SubClassOf(Dᵢ, C)`. Runs at the shared
  axiom level so **both** the EL saturator (which dropped union-LHS) and the
  tableau see the atomic-LHS form. This is what fixes the observed classify miss.
- **`crates/owl-dl-core/src/absorb.rs`** (`absorb_sub_sup`): distributes a union
  LHS at internalization too, covering the equivalence-derived (`X ≡ A⊔B`) and
  `DisjointUnion` union-LHS cases for the tableau (the `DisjointUnion` half now
  correctly emits `Cᵢ ⊑ P` rules instead of a residual Or-GCI).

**Verification:** new unit tests (`union_lhs_distributes_into_per_disjunct_rules`,
`union_lhs_subclass_splits_into_one_axiom_per_disjunct`, `atomic_lhs_...unchanged`,
updated `disjoint_union_...`), all owl-dl-core lib tests green; end-to-end
`union-lhs.ofn` and `ore_ont_13077` (`Lateinamerikaner⊑Auslaender`,
`Osteuropaeer⊑Auslaender`) recovered in classify (default + `--saturation-only`);
**FP=0/MISSED=0 closure-diff gate 22/22 green** (galen/alehif/sio 499=499/wine
653=653/ore-10908/ore-15672, all precision=recall=1.0); fmt + clippy
(`-D warnings`) clean. (One unrelated workspace test fails on a missing gitignored
corpus fixture `ontologies/regression/funcmerge-cyclic.ofn` — pre-existing env,
not this change.)

### Prevalence / impact (broadened over the full 1920-ont ORE 2015 sample)

Grep + baseline-vs-fixed transitive-closure diff:
- **Direct union-LHS `SubClassOf(ObjectUnionOf(…), C)`: 2 onts / 1920 (4 axioms).**
  Closure diff: `ore_ont_13077` **+2** (`Lateinamerikaner⊑Auslaender`,
  `Osteuropaeer⊑Auslaender`), `ore_ont_1672` **+0** (its union-LHS members were
  already `⊑ C` via other axioms). Observed corpus impact = **2 subsumptions on 1
  ontology** — a genuine but LOW-prevalence completeness gap. This is what the
  `convert`-level split closes (it feeds the saturator, classify's closure source).
- **`EquivalentClasses(X, ObjectUnionOf(…))`: 468 onts / 1920 (24 %) — but NOT a
  gap.** The **baseline (pre-fix) binary already derives `A⊑X` for `X≡A⊔B`**
  (verified on the synthetic and corpus): the saturator handles the
  equivalence-union case already. ⇒ the `absorb`-level change (equivalence-derived
  + `DisjointUnion` union-LHS at internalization) closes **zero observed misses**;
  it is a sound tableau-internalization generalization (union-LHS no longer a
  residual Or-GCI), corpus impact 0. Keep-or-drop is a scope call — the `convert`
  split alone closes the one real gap.

**Bottom line:** the fix is correct and sound (FP=0/MISSED=0), but measured corpus
payoff is small (1 ORE ont, 2 subsumptions) — a completeness-gap closure, not a
broad win. Ship for correctness, not impact.

**Two flagged onts (the only unrefuted cases) — adjudicated by reading axioms:**
- **4 SIO onts** (`SIO_000275⊑SIO_000342`): **confirmed false positive** — the
  structural check's reversed-order regex swapped the defined class `G` with the
  intersect type `K` (`SIO_000342` is a `K`), flagging the reverse of a real
  subsumption classify already has (`SIO_000342⊑SIO_000275`).
- **`ore_ont_9835`** (`part_of` declared **symmetric** — a source modeling error,
  but live): flagged `UBERON_0001017⊑UBERON_0000470` is the witness-vs-class
  over-generalization (the *witness* becomes a `UBERON_0000470`, the class does
  not). **CLOSED by complete oracle on a 1-hop extracted module** (178 classes,
  fully Horn, `part_of` symmetric, the full flagged chain + definitional context
  present, `scratchpad/mod2_9835.ofn`): **probe == classify (211=211, PmC=0)** and
  **probe-delta with/without symmetric `part_of` = 0**. Even the one genuinely-live
  symmetric role is inert — zero misses, symmetric adds nothing. (Residual caveat:
  1-hop extraction; a longer chain in the full 46k ont is un-oracle-able and
  theoretically possible but unevidenced — the `¬G` complete check is disjunctive
  and stalls on the whole giant.)

**Fundamental verification limit (honest):** the pattern-bearing onts are exactly
the ones where the complete per-pair check (`¬G` = `∀R.¬C ⊔ ¬D`, disjunctive)
stalls, and classify's forward closure is provably symmetric-*incomplete*
(Finding 5). So on the giants there is **no cheap complete oracle** — which is why
the standard of proof here is "no confirmed miss + bounded payoff," not "refuted at
scale." Reusable detectors: `scratchpad/sym_impact.py` (probe-delta),
`scratchpad/struct_scan.py` (structural precondition; note its filler-conclusion
over-approximation).

## The discriminating measurement (superseded by Finding 6)

Run **actual `classify ore_ont_3914`** (all probes so far are per-class SAT,
which is trivially cheap on Horn and does NOT measure hierarchy computation —
hence the unreconciled 193 ms vs the doc's 58 s one-pass figure; different things):
1. Does it actually DNF on current main? (label heuristic prunes 96–100 %
   elsewhere; on tight Horn `labels(C)` it may not DNF at all.)
2. Time split: label-cache build vs surviving pair loop.
3. **Label-heuristic prune rate** — the number that decides the fork. 99.9 %
   prune ⇒ n² is a non-issue, maybe no problem to solve; poor prune ⇒ that's the
   lever (Option A pruning, or D2a Phase A if pruning can't be made complete/cheap).
