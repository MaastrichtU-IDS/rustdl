# The pseudo-model bake-off, run because its soundness argument was falsified

**Date:** 2026-08-18 · Runs the ORE verdict-identity comparison that
`realize.rs` called "the recommended CI/Linux confirmation" and deferred. It stopped being
optional when the shortcut's *sound by construction* clause was falsified
(`docs/known-limitations/realize-drops-derived-individual-equality.md`).

**Result: the defect is REACHABLE on the real corpus and HERMIT-CONFIRMED — 1 of 19 usable
ontologies loses a genuinely entailed type at the default — and the prune is worth 64% of
realize wall. Both halves are real; neither side wins outright.**

## Method

60 `ABox`-bearing ORE ontologies under 3 MB (10 of them carrying a functional /
inverse-functional role), `realize` in two arms — `RUSTDL_PSEUDO_MODEL=1` (default) vs `=0` —
single-threaded, 60 s cap, comparing output hashes.

Sampled **broadly**, not only the 10 merge-forcing ones: the falsified claim was general ("an
entailed type is in every model, hence in the witness"), so restricting to where it is already
known to fail would only re-confirm a synthetic fixture. The question was whether the witness
is unfaithful in ways not yet localised.

**One-sided by construction.** The prune only ever returns `Ok(false)` early, so `ON ⊆ OFF`
and any difference is the default having dropped something. No external oracle is needed for
*this* question — `=0` is the reference.

## Result

| | |
|---|---|
| ontologies run | 60 |
| **usable pairs** (output in both arms) | **19** |
| DNF/empty in ≥1 arm — **uninformative, excluded** | 41 |
| output differs | **1** (`ore_ont_10009`) |
| wall over usable pairs | ON **48.6 s** vs OFF **136.4 s** — the prune is worth **−64%** |

The 41 excluded pairs are not evidence of agreement. Counting them as "no difference" would
repeat the vacuous-measurement error made earlier the same day.

### The one difference, and a metric error it exposed

`ore_ont_10009` reports **55 rows in both arms**, which first read as "same count, different
content" — impossible for a subtractive prune. It was a metric error on my part: **rows are
per INDIVIDUAL, not per type.** The actual difference:

```
ON  : a32071928c → zqedzbx
OFF : a32071928c → sqdsq, zqedzbx
```

Two individuals (`a32071928c`, `a72192307c`) each lose the type `sqdsq` at the default. So the
behaviour IS subtractive, and this is a **real corpus instance** of the defect rather than only
the synthetic fixture.

### ADJUDICATED BY HERMIT (2026-08-18): the default MISSES a genuinely entailed type

`robot reason --reasoner hermit --axiom-generators ClassAssertion --include-indirect true`
on `ore_ont_10009` returns:

```
ClassAssertion(<sqdsq> <a32071928c>)
ClassAssertion(<zqedzbx> <a32071928c>)
```

**So `sqdsq` IS entailed, `RUSTDL_PSEUDO_MODEL=0` is correct, and the DEFAULT drops it.** This
settles the fork left open below: it is a MISS, not the prune masking an over-report. FP=0 is
untouched; a genuine entailment is lost.

Both arms are also **deterministic** — three runs each produce identical hashes
(`60260a56…` ON, `94750757…` OFF) — so this is a fixed behavioural difference, not witness
nondeterminism. That was worth ruling out first, since the witness is one arbitrary `Sat`
completion.

### MECHANISM STILL UNIDENTIFIED — three hypotheses refuted

Ablation over `ore_ont_10009` (each construct class removed, testing whether the ON-vs-OFF
difference survives; ~50 s per check):

| construct | verdict |
|---|---|
| `ObjectPropertyAssertion` (44) | **NECESSARY** — difference lost when removed |
| `DataPropertyAssertion` | **NECESSARY** — difference lost when removed |
| `DataMaxCardinality` (18), `DataExactCardinality` (14), `ObjectMaxCardinality` (12) | all removable, difference survives |
| `Declaration` | removable |
| `EquivalentClasses`, `DataHasValue`, `DataSomeValuesFrom`, `FunctionalDataProperty`, `DisjointClasses`, `InverseObjectProperties` | **absent from the ontology** |

Three mechanism hypotheses, each tested by a synthetic fixture and each **refuted**:

1. **`≤n`-forced merges** — a `≤1` fixture (`fixtures/pseudo_model_merge/max-cardinality-one.ofn`)
   is CORRECT in both arms, and the ablation shows cardinality is removable here anyway.
2. **Data-property-driven** — the data-restriction constructs that could give a positive type
   (`DataHasValue`, `DataSomeValuesFrom`, `FunctionalDataProperty`) are all **absent**.
3. **Disjunctive-domain over-report** — `ObjectPropertyDomain(r, A ⊔ B)` with `x : A` and
   `r(x,y)` is CORRECT in both arms; and HermiT above refutes the over-report reading outright.

With **no `EquivalentClasses` anywhere**, the only route to a non-asserted type is property
domain/range — and the individual's edges carry *disjunctive* domains
(`ObjectPropertyDomain(sqndsqgy, zqedzbx ⊔ sqdsq)`), which is suggestive but did not reproduce
in isolation. **Whoever continues should run ddmin, not another hypothesis.** The predicate is
`/tmp/red/check.sh`-shaped (difference present ⇒ exit 0) and costs ~50 s per check, so a full
reduction is hours — but three cheap guesses have now failed, and the constraints above
(ObjectPropertyAssertion + DataPropertyAssertion both necessary, no defined classes) narrow it
considerably.

### How far the claim can be pushed, and where it stops

What is established: the tableau probe reports the membership, the prune skips the probe, and
the prune is subtractive by construction — so **the default loses a type the engine itself
derives.**

What is *not* independently adjudicated: whether `sqdsq` is truly entailed. It does **not**
follow from subsumption — `justify subclass zqedzbx sqdsq` returns *"not entailed (no
justification)"*, and `zqedzbx` has no named subsumers — so it must arise at the individual
level (that individual carries three object-property and two data-property assertions, and
`sqdsq`'s neighbourhood uses `ObjectMaxCardinality`/`DataExactCardinality`). Individual-level
entailment is not class-level subsumption, so this is consistent.

`justify … instance` could not adjudicate it: **`a32071928c` has zero `NamedIndividual`
declarations** — an undeclared individual — and the query fails to resolve it in either
argument order. So the strongest honest statement is *"the engine's own probe says yes and the
prune discards it"*, resting on tableau soundness (the project's core invariant, FP=0
corpus-wide) rather than on an external oracle.

## What this changes

**It does not condemn the default.** 1 of 19, and the prune buys 64% of realize wall — on the
19 usable pairs, OFF costs 2.8× as much. A blanket flip would be a poor trade.

**It does not exonerate it either.** The stated basis for shipping default-ON was
*soundness by construction*, and that is false; what remains is an empirical
"1-in-19, cheap" argument, which is a different and weaker claim that should be recorded as
such rather than restored to the old wording.

**The fix target is unchanged and now better justified**: make the `ABox`-seeded wedge
consistency completion apply inverse-functional merges as it already does functional ones.
That removes the known mechanism without giving up the 64%.

## Threats to validity

* **19 usable of 60** is a thin base. The realize DNF rate on ABox-bearing ORE at a 60 s cap
  is the limiting factor, not the comparison.
* The sample was capped at 3 MB and drawn from the first 500 files; it is not stratified.
* `ore_ont_10009` is an OAEI benchmark ontology with obfuscated IRIs and undeclared
  individuals, i.e. not typical of curated biomedical inputs — its presence shows the defect is
  reachable, not that it is common.
* The sample was capped and unstratified (see above).

## THE CORPUS INSTANCE IS A DIFFERENT MECHANISM — and I could not identify it

`ore_ont_10009` carries **zero `FunctionalObjectProperty`** and **zero
`InverseFunctionalObjectProperty`** (and no nominals, no `SameIndividual`), but **12
`ObjectMaxCardinality`**. So its lost types are **not** the inverse-functional mechanism
documented in the limitation file — they are a second unfaithfulness in the same witness build.

I hypothesised the cause was `≤n`: semantically `≤1 r` forces individual merges exactly as
`Functional(r)` does, and a code-path split (the wedge enforces functionality via a
`FunctionalRole` bitset, while `Max` is a concept constructor handled elsewhere) would explain
one being applied and the other not.

**That hypothesis is REFUTED.** A synthetic fixture mirroring the functional/inverse-functional
pair — `Owner ⊑ ≤1 r`, `x : Owner`, `r(x,y)`, `r(x,z)`, `y : A`, `z : B`, so `y = z` is forced —
returns the **correct** answer in BOTH arms:

```
PSEUDO_MODEL=1  x Owner   y A B   z A B
PSEUDO_MODEL=0  x Owner   y A B   z A B
```

So the witness **does** apply `ObjectMaxCardinality`-forced merges. Revised table:

| forcing construct | witness applies it? | evidence |
|---|---|---|
| `FunctionalObjectProperty` | **yes** | functional fixture correct at the default |
| `ObjectMaxCardinality` (`≤1`) | **yes** | this fixture, correct in both arms |
| `InverseFunctionalObjectProperty` | **no** | synthetic fixture loses both types |
| whatever `ore_ont_10009` exercises | **no** | 2 types lost, mechanism **UNIDENTIFIED** |

**So `ore_ont_10009`'s loss has no identified cause.** Remaining candidates, untested:
`DataExactCardinality` (it has those), a data-property-driven merge, its undeclared
individuals, or an unfaithfulness unrelated to merging at all. Anyone continuing here should
start by reducing `ore_ont_10009` rather than by generalising from the inverse-functional case.

**Method note, since it is the point of this section.** I flagged the elimination argument as
the weakest claim in this document and said a synthetic `≤1` fixture "would settle it in
minutes". It did, and it settled it *against* the hypothesis. An elimination argument over a
36-axiom obfuscated benchmark ontology was worth exactly as much as it cost to test — which is
why it was tested before being written up as a finding.

## Net position

* The falsified soundness clause is corrected at the source and in `CLAUDE.md`; what justifies
  the default now is an empirical **1-in-19, worth 64% of wall**, not a construction argument.
* **Two** distinct witness unfaithfulnesses exist: inverse-functional merging (localised, with a
  fixture and a canary) and whatever `ore_ont_10009` hits (unidentified).
* A blanket `RUSTDL_PSEUDO_MODEL=0` flip is not justified — 2.8× realize wall to fix 1 of 19.
