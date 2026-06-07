# Scope: classify recovery-sweep gap + ABox-seed tableau pathology (2026-06-07)

Scoping for the threads surfaced by `docs/wine-residual-31-diagnosis-2026-06-07.md`.
The diagnosis split wine's 31 residual MISSES into **2 (cluster A) + 29 (B/C/D)**
and uncovered a separate perf/termination issue. Three actionable items, in
ascending cost/risk.

---

## 1. Cluster A — extend the defined-sup sweep to defined SUBS (cheap, sound)

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

**>150 s on a 137-class ontology is not "slow" — it is probable
non-termination.** First step is *diagnosis, not a fix*: instrument
`prepared.decide` on `Fruit ⊓ ¬EdibleThing` (RUSTDL_TRACE / RUSTDL_COUNTERS) to
see whether blocking ever fires on the seeded-nominal graph, or whether
`≥n`/nominal generation loops. Hypothesis: a double-blocking / nominal-merge
interaction that doesn't terminate when the graph is pre-seeded with 207
mutually-related nominal roots.

**Possible fixes (pending the diagnosis):** (a) don't seed the full ABox for a
per-pair `decide` whose sub/sup can't reach an individual (sound iff the
reachability check is conservative); (b) fix the termination bug directly (the
principled fix — the seeded state should block/terminate like the fresh one
does); (c) cap+fall-back to the fresh un-seeded tableau on stall — **but** verify
this is sound for nominal-*dependent* pairs (the fresh path may omit ABox-driven
entailments, which would be a *completeness* loss, acceptable as a sound
under-approximation, not an FP risk). Recommend (a)/(b) over (c).

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
