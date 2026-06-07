# Scope: classify recovery-sweep gap + ABox-seed tableau pathology (2026-06-07)

Scoping for the threads surfaced by `docs/wine-residual-31-diagnosis-2026-06-07.md`.
The diagnosis split wine's 31 residual MISSES into **2 (cluster A) + 29 (B/C/D)**
and uncovered a separate perf/termination issue. Three actionable items, in
ascending cost/risk.

---

## 1. Cluster A — extend the defined-sup sweep to defined SUBS (cheap, sound) ✅ DONE (commit 3dbe3d8)

**Implemented + validated:** companion defined-SUB sweep in `classify.rs`; for
each `C ≡ D₁⊔…⊔Dₙ`, candidate sups = `∩ᵢ subsumers(Dᵢ)`, added directly (no
tableau — sound by construction). **wine MISSED 31→29** (closure 622→624,
recovers `food#Fruit ⊑ EdibleThing` + `⊑ ConsumableThing`), FP=0 across
wine/ore-10908/ore-15672/shoiq/sio/alehif. Regression test
`defined_union_sub_under_primitive_sup`. Original scoping below.


**Bug.** The top-down classifier (`classify.rs`) places classes by a tier-walk
ordered on EL-closure-subsumer count, then recovers walk-missed pairs with a
**defined-sup sweep** (`classify.rs:1130`) that only tests pairs whose **sup** is
a defined class (`EquivalentClasses(Name, ComplexExpr)`). `Fruit ⊑ EdibleThing`
(and `Fruit ⊑ ConsumableThing`) is **defined-SUB ⊑ primitive-SUP**
(`Fruit ≡ NonSweetFruit ⊔ SweetFruit`; `EdibleThing` is `SubClassOf`-only). The
walk can't see it (not in the EL closure), and the sweep's sup side never
includes a primitive class → never tested. Yet
`prepared.hyper_decide(Fruit, EdibleThing, 200 ms)` = **Subsumed in 0 ms**.

**Fix.** Add a companion sweep over **defined SUBS**: for each
`C ≡ D₁ ⊔ … ⊔ Dₙ` (union-defined), test `C ⊑ X` for candidate sups `X`. Keep it
cheap by restricting `X` to the **common supersumers of all `Dᵢ`** (intersection
of the `Dᵢ`'s closure-subsumers) — for `Fruit`, `EdibleThing`/`ConsumableThing`
fall out of `NonSweetFruit`'s ∩ `SweetFruit`'s subsumers. Adjudicate each
candidate with the existing wedge/`subsumes_via_tableau` (per-pair budget,
parallel), same as the defined-sup sweep.

**Cost.** Bounded by `(#union-defined classes) × (avg common-subsumer count)` —
tiny (wine has a handful of union-defined classes). Reuses the existing sweep
machinery.

**Soundness.** No risk to FP=0: every recovered pair is wedge-/tableau-verified
(the sweep doesn't assert, it asks the engine). Worst case it tests a few extra
pairs that come back not-subsumed.

**Payoff.** wine 31 → 29, budget-independent (the wedge proves them fast).
Generalizes to any covering/union-defined sub ⊑ primitive sup — a real
completeness class, not a wine quirk.

**Gate.** corpus closure-diff FP=0 unchanged; wine MISSED 31→29; a non-ignored
test asserting `Fruit ⊑ EdibleThing` ∈ the wine closure (the existing
`#[ignore]`d `wine_fruit_prepared_vs_fresh_probe` documents the pre-fix state).

---

## 2. ABox-seeded `prepared.decide` non-termination (perf; investigate before fixing)

**Symptom.** `prepared.decide(Fruit ⊓ ¬EdibleThing)` does **not terminate in
150 s** (times out at 5 s) while the fresh `run_satisfiability` of the same query
is **0.01 s** (`wine_fruit_prepared_vs_fresh_probe`). `PreparedOntology` is
ABox/nominal-seeded once (`from_internal`), so every per-pair `decide` drags
wine's 207-nominal ABox into the tableau. Irrelevant to cluster A (the wedge
short-circuits before the tableau), but it is the **fallback for B/C/D** (whose
wedge does not prove them) and likely inflates wine's ~311 s classify wall.

**DIAGNOSED (2026-06-07, `--features counters` on the 5 s `decide`): NOT
non-termination — a bounded but massive redundant fixpoint.** Counter histogram:
`is_blocked_calls = 8,854,932`; each `apply_*` rule = 1,475,822; `add_label_calls
= 3,292,064` but **`add_label_inserted = 33,255`**; `add_edge_calls = 7,751`. So
the graph is small and bounded (33 K labels, 7.7 K edges) — no runaway
`≥n`/nominal generation. The cost is the completion **re-processing the seeded
207-nominal ABox ~1.48 M times** (8.85 M `is_blocked` checks, each an O(n) scan;
3.29 M `add_label` calls, 99 % redundant). Every per-pair `decide` redundantly
completes wine's entire ABox — even for `Fruit`, which is **disconnected from
every individual**. It terminates eventually (hence >150 s, not ∞), but the work
is pure waste for ABox-irrelevant pairs. Dominant cost = `is_blocked`'s O(n) scan
× 8.85 M calls.

**Fix directions:** (a) **don't seed/complete the ABox for a per-pair `decide`
whose sub/sup cannot reach an individual** — for `Fruit`/`EdibleThing` (no
nominals) the 207 ABox roots are a disconnected component that only adds cost.
Sound: if neither class reaches a nominal/individual, the (consistent) ABox can't
change the `C ⊓ ¬D` verdict; ABox *inconsistency* is handled separately by the
Phase-A1 `abox_check` (which marks all classes unsat). Worst case it's a sound
under-approximation. Limited reach (see below). (b) fix the perf directly — the
real lever.

**Refinement (refutes the "fresh seam" idea): there is NO fast tableau path.**
`run_satisfiability` is literally `PreparedOntology::from_internal(internal)
.decide(...)` (`lib.rs:1733`) — the same ABox-seeded tableau. The "fresh 0.01 s"
for `is_subclass_of(Fruit, EdibleThing)` was the **wedge short-circuit** (step 4
of `is_subclass_of_internal_full`, `lib.rs:1682`), not a faster tableau. So:
- **Cluster A never hit this** — its wedge proves it; classify's
  `subsumes_via_tableau` also tries the wedge first, so the tableau is unreached.
  (Cluster A was the sweep-coverage gap, fixed in item 1.)
- The 150 s tableau bites the **B/C/D** pairs, whose wedge does NOT prove them →
  fall to the seeded tableau → timeout.
- **Option (a) has limited reach**: ABox-disconnected pairs (food) are mostly
  proved by the wedge already; and B/C/D are ABox-*connected* (they need the
  nominals), so (a) does not help them. (a) only trims wall on disconnected
  wedge-failures.
- **The real lever is (b): the `is_blocked` O(n) scan × 8.85 M + non-deduping
  worklist (re-deriving 33 K facts ~44×).** Same `is_blocked` hot path Phase
  3b/3e fought on the tableau side. Speeding it cuts wine's wall on *every*
  nominal pair.

**But weigh the payoff honestly:** B/C/D appear to be genuine **modeling gaps**
(Beaujolais⊑Gamay / DryWhiteWine⊑WhiteNonSweetWine time out even at `trust_sat=0`
*unbounded* — the seeded tableau builds an open under-approximated model, i.e. it
would return `Sat`=not-subsumed, not `Unsat`, if it finished). So a faster
`is_blocked`/worklist recovers **wall time, not completeness** — B/C/D stay
missed (they need the nominal-completeness work, item 3). Confirm this first
(does a B/C `decide` ever return `Unsat`, or only stall→`Sat`?) before investing
in the tableau perf fix: if it's wall-only, scope it as a perf task, not a
completeness one.

**Note — the `unwrap_or_default()` latent bug.** `find_direct_parents_top_down`
(`classify.rs:1392/1408`) turns a tableau **timeout** (`Ok(None)`) into
`false` (`unwrap_or_default()`) while pruning the reachability walk — so a
timed-out intermediate silently drops every subsumption reachable only through
it. NOT cluster A's cause (single-thread + wedge-proof ruled it out), but a real
correctness-of-completeness hazard that this ABox-seed slowness makes *likely* to
fire on nominal ontologies. Treating a timeout as "unknown, keep exploring" (or
testing reachable sups independently) would be the safe behavior.

---

## 3. Clusters B/C/D — the nominal-completeness project (29 pairs, deferred)

The dominant remainder. Genuine reasoning gaps in the under-approximated nominal
semantics (`∃R.{a}`+functional ⟹ `∀R.{a}`, `{a}⊆{a,b}` nominal-set, region→grape
+ `≤1`). Representatives don't resolve even unbounded (`trust_sat=0` timeout).
This is the same lever the nominal-lever scoping doc deferred; out of scope for
the two cheap classify fixes above. NB: item 2 may reveal some B/C/D timeouts are
partly the ABox-seed perf bug rather than pure modeling — re-check a B/C pair via
the fresh path once item 2 is addressed.

---

## Recommended order

1. **Cluster A sweep extension** — cheap, sound, classify-only, +2 pairs. Do first.
2. **ABox-seed non-termination diagnosis** — higher value (wine wall + maybe some
   B/C/D), but investigate before fixing; the `unwrap_or_default` hazard rides along.
3. **Nominal-completeness (B/C/D)** — the real frontier, a standalone project.
