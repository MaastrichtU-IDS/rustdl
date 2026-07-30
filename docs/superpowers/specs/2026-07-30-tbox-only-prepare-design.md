# TBox-only `PreparedOntology` construction — **NO-GO** (payoff measured at zero), and the redirect

**Date:** 2026-07-30
**Status:** **RETIRED before implementation.** The design would have removed 0 of the 49.6M
concept rules it was aimed at. Kept as a death certificate plus three salvaged findings, two of
them better-evidenced than the retired proposal.
**Evidence:** `docs/2026-07-29-memory-tail-localization.md` §8–§10, plus the adjudication below.

## What was proposed

`PreparedOntology::from_internal` allocates **+42.3 GB** on `ore_ont_9347` (8.6 MB input, **114
classes**): ~17.9 GB `HyperCache::build` + ~24.5 GB absorb, with `tbox-stats` reporting
`concept_rules: 49,571,087`. Stripping the ABox at **source** gives 0.01 GB and the run
completes — 4600×. Lever A already computes `abox_irrelevant_to_classify` but reads it only at
two per-pair-seed sites, so the ABox is dropped at consumption and paid for at construction. The
proposal: filter ABox axioms out of the IR before construction, behind
`RUSTDL_TBOX_ONLY_PREPARE`.

## Why it is retired — the filter removes nothing that matters

`is_abox_axiom` (`classify.rs:1089-1096`) matches five forms: `ClassAssertion`,
`ObjectPropertyAssertion`, `NegativeObjectPropertyAssertion`, `SameIndividual`,
`DifferentIndividuals`. Removing exactly those from `ore_ont_9347` — **36,286 lines** —
leaves:

```
# concept_rules:        49571087      (byte-identical to the full ontology)
#   residual_or:        159
```

**Zero reduction.** The 49.6M rules are `Axiom::DisjointClasses(DKey, DKey)` axioms that
`convert_ontology` **mints** from `DataPropertyAssertion`s via `seed_disjoint_bucket`
(`convert.rs:2793`, gated `RUSTDL_BOUNDED_DKEY_DISJOINT`, default ON). Those are **TBox-shaped**
— no ABox filter matches them. Corroborating arithmetic on a second target: `ore_ont_5368` has
6,101 distinct literals, C(6101,2) = 18,608,050, and its reported `concept_rules` is 18,620,251.

**The measurement that motivated this spec was the wrong experiment.** Stripping assertions at
*source* stops conversion from ever minting the DKeys; filtering the *IR* after conversion leaves
them in place. Those are not the same intervention, and the 4600× belonged to the former. This is
the same error the evidence doc's own §9 records (selecting for a feature being present rather
than binding) applied one layer down — and applied to the document that recorded the lesson.

Predicted payoff of the design as written: **0 of 5** target ontologies.

## Also wrong: the recommended shape could not have worked

The spec recommended Option A (a classify-specific constructor) and justified its cheapness with
"`classify.rs:786` already builds a separate `PreparedOntology` for exactly this." That line is
inside the **fast-path** branch which `return`s `classify_pure_el`. The path `9347` takes is
`classify_top_down_internal`, which builds **one** `PreparedOntology` (`classify.rs:1626`) and
reads `prepared.abox_verdict()` off that same object (`:1631`). Routing that construction to a
TBox-only constructor makes the inconsistency verdict vacuous — violating the spec's own stated
contract — while adding a second full build reallocates the memory the lever exists to avoid.

Mechanically worse: `collect_abox` runs *after* `nnf_axioms` (`lib.rs:4625/4633`), and
`abox_check::check` early-returns `Unknown` when `abox.individuals.is_empty()`
(`abox_check.rs:139`). Also `internal_has_abox(filtered)` is false, so
`abox_irrelevant_to_classify` **inverts** to false inside the filtered build and `consistency`
becomes `None` — the spec claimed the field "becomes redundant".

## The soundness contract was mis-stated (correct it wherever it is repeated)

The spec framed soundness as conditional: "nominal-free **and consistent** ⟹ the TBox determines
subsumption." That is a **completeness** statement. Soundness is unconditional:

> The TBox-only build is a literal axiom **subset**. Entailment is monotone and the reasoner is
> sound on any input, so every positive it reports holds in the full KB — regardless of nominals
> or consistency.

Nominal-freedom and consistency are the conditions for **not losing entailments**. Framing them
as the FP guard mis-aims the gates (the spec's gate 3 is a MISSED gate, not an FP gate) and
invites a future reviewer to "relax the nominal test" believing they are trading completeness
when they think they are protecting FP=0.

The one genuine FP surface is plumbing, not logic: **two `PreparedOntology` objects built from
different axiom sets have different Tseitin/var/nominal id allocations.** The `ConsistencyCache`
doc comment already records the failure mode ("a mismatched hierarchy would let an unrelated edge
satisfy a super-role atom = false clash", `lib.rs:3159`). Any future design producing two prepared
objects on one path must forbid cross-use of `hyper`/`consistency`/`told`/`pool` between them.

## Salvaged finding 1 — the real target is DKey disjointness at conversion

`seed_disjoint_bucket` is already the *bounded* version and still emits ~49.6M
`DisjointClasses(DKey,DKey)` axioms for 13,561 string values on a 114-class ontology whose 113
`SubClassOf` axioms mention no data range. Two directions, both extending the argument bounded
seeding already uses:

- make DKey disjointness a **side-table** consulted by the concrete-domain clash rule, instead of
  materialised `DisjointClasses` axioms;
- drop DKeys not reachable from any TBox concept position.

This is where the 42.3 GB actually lives, and it needs no nominal-free premise and no ABox
contract. `RUSTDL_DATA_PROPERTIES=0` collapses `concept_rules` to 113, confirming the channel.

## Salvaged finding 2 — the fast path builds a full `PreparedOntology` just to read a verdict

At `classify.rs:785-787` and `:1606-1608` the fast path constructs a **full** `PreparedOntology`
— HyperCache + NNF + absorb — solely to read `abox_verdict()`, then discards it. Measured with
`RUSTDL_ABOX_CHECK=0` (which skips exactly that construction), single-threaded:

| ontology | assertions | check ON | check OFF | delta |
|---|---|---|---|---|
| `ore_ont_1043` | 137,569 | 2.34 s / 526 MB | 1.64 s / 341 MB | **−30% wall, −35% RSS** |
| `ore_ont_10068` | 180,242 | 3.65 s / 373 MB | 2.25 s / 351 MB | **−38% wall** |
| `ore_ont_11311` | 45,179 | DNF @300 s | DNF @300 s | wall lever, not a DNF lever |

`abox_check` reads only `{abox, axioms, told, pool, inverse_pairs, hierarchy,
disjoint_role_pairs, closure}` — **never `hyper` or `tbox`**, which are the entire blowup. A
reduced constructor for it is **verdict-identical by construction**, needs **no** nominal-free
premise and **no** ABox-irrelevance contract, and applies to nominal-**bearing** ontologies too.
`classify.rs:588-594`'s comment already names this cost but acts on it only for ABox-free inputs.

**This is the measurable version of the "avoided wasted work" argument the retired spec reached
for.** Rank it first.

## Salvaged finding 3 — `ontology_uses_nominals` misses two axiom forms

It scans `SubClassOf`/`EquivalentClasses`/`DisjointClasses`/`DisjointUnion`/`ClassAssertion` and
**skips `ObjectPropertyDomain { domain }` and `ObjectPropertyRange { range }`**, the only other
`ConceptId`-bearing forms. Counterexample, verified to return `false`:

```
ObjectPropertyDomain(:r ObjectOneOf(:a))
SubClassOf(:C ObjectSomeValuesFrom(:r owl:Thing))
ClassAssertion(:D :a)   SubClassOf(:D :E)        ⊨ C ⊑ D ⊑ E
```

rustdl misses `C ⊑ D` today at `TBOX_ONLY=1`, `=0`, and `=0 TRUST_SAT=0` — a **pre-existing
latent MISS, not a regression**. But `lib.rs:4276`'s "**provably** irrelevant" is false as
written, and the scan fix is four lines.

## The corpus cannot gate any of this

Exactly **one** curated fixture is ABox-bearing AND nominal-free: `alehif-test.ofn` (7,169
assertions, 0 `ObjectOneOf`/`ObjectHasValue`). Every other ABox fixture carries nominals
(`wine` 474/207, `family` 1858/5, `pizza` 10/7, `ro` 37/2, `ore-15516` 732/8, `ore-10908` 18/4,
`ore-15672` 48/2) and so fails `abox_irrelevant_to_classify`. On `alehif`: full 0.10 s / 21.6 MB
vs stripped 0.04 s / 8.5 MB, closures byte-identical, `before_prepared`→`after_prepared` both
0.01 GB.

**So "flag ON-vs-OFF byte-identity on the curated corpus" is a near-vacuous gate for anything in
this area.** Any future lever here must be validated by an **ORE on-vs-off sweep** in Lever A's
style (271 ontologies, 0 answer changes), not by corpus byte-identity.

## Recommended order

1. **Reduced-input `abox_check`** (salvaged finding 2). Contract-free, verdict-identical by
   construction, 30–38% wall measured, applies broadly including to nominal-bearing ontologies.
2. **DKey disjointness at conversion** (salvaged finding 1). Where the 42.3 GB actually is.
3. **Fix `ontology_uses_nominals`** (salvaged finding 3), or delete "provably" from the field doc.
4. Re-run the ORE memory benchmark with a budget exceeding conversion time — the evidence doc §5
   records the current RSS column is timeout-truncated, so it cannot rank memory work, and §6
   records that some "reasoning timeouts" are conversion timeouts.

**Do not resurrect the ABox-filter design without first showing, per candidate, that removing
only the five `is_abox_axiom` forms reduces `concept_rules`.** On `ore_ont_9347` it does not.
