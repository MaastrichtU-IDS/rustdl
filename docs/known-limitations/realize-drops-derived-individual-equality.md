# `realize` ignores DERIVED individual equality, silently

**Found:** 2026-08-18 · **Status:** **BOTH halves now FIXED (2026-08-18).** Functional half by
option A (the gate refuses a functional/inverse-functional role together with an
`ObjectPropertyAssertion`, so the tableau realizes it, and the tableau folds functional
merges). Inverse-functional half by `RUSTDL_INVERSE_FUNC_MAX` — **default OFF pending a
corpus sweep**, see the section at the end. · **Severity:** missed entailments with **no
incompleteness signal**, and two surfaces of one binary contradicting each other.

Found by following up the open question left by
`docs/2026-08-18-fp-critical-audit.md` §1 — whether inverse-functional + `ABox` is complete on
the `is_pure_el` path for `realize`, where individual identity is observable. **It is not.**

## Reproducer

`crates/owl-dl-reasoner/tests/fixtures/realize_derived_same/inverse-functional.ofn`:

```
InverseFunctionalObjectProperty(:r)
ClassAssertion(:A :x)
ClassAssertion(:B :y)
ObjectPropertyAssertion(:r :x :z)
ObjectPropertyAssertion(:r :y :z)
```

`r` is inverse-functional and both `x` and `y` are `r`-predecessors of `z`, so **`x = y`** is
entailed. Therefore `x : A`, `x : B`, `y : A`, `y : B` all hold.

| surface | result |
|---|---|
| `rustdl realize` | **`x : A` and `y : B`** — 2 type assertions missing |
| `rustdl individuals --json` | `same_groups: [["x","y"]]` — **the equality IS derived** |
| `rustdl realize --json` | had **no `incomplete` field at all** — the miss was silent. **A field was added 2026-08-18**, but it reports only CUT PROBES; it does NOT cover this defect, because no probe is cut here — the equality is simply never folded. So this miss remains silent even with the new signal. |

## The gap is DERIVED vs ASSERTED, isolated by control

Adding an explicit `SameIndividual(:x :y)` to the same file:

| | realize output |
|---|---|
| asserted `SameIndividual(x,y)` | `x : A, B` and `y : A, B` — **correct** |
| derived (inverse-functional) | `x : A` and `y : B` — **incomplete** |

`individuals` reports `same_groups: [["x","y"]]` in **both** cases. So the equality is known;
only realize's type computation fails to use it.

## Why asserted works and derived does not

`realize` has **no equality folding of its own**. `realize_saturation_eligible`
(`realize.rs:747`) simply refuses `Axiom::SameIndividual(_) => false`, pushing such ontologies
off the saturation fast path to the tableau, which merges the nodes — that is the whole
mechanism behind the working asserted case.

Nothing plays that role for a *derived* equality:

* `saturator_complete_fragment` **admits** `InverseFunctionalRole` (see the FP-critical audit
  §1: sound for CLASS classification, because the canonical model is a tree), so the ontology
  is not kicked off the fast path.
* The EL saturator never reads `Axiom::InverseFunctionalRole`, so it derives no merge.
* **For inverse-functional, the tableau path misses it too**: `RUSTDL_REALIZE_SATURATION=0`
  gives the same `x : A`, `y : B`. So for that construct it is not merely a fragment-gate
  problem, and the flag is not a workaround.

## REFINEMENT (2026-08-18): the two paths fail differently, and it matters

Repeating the experiment for a **functional**-forced equality —
`FunctionalObjectProperty(:r)` with `r(x,y)`, `r(x,z)`, so `y = z`
(`fixtures/realize_derived_same/functional.ofn`) — separates the two mechanisms.
Both fixtures, both paths, 2 runs each, stable:

| forced equality | saturation path (**default**) | tableau path (`RUSTDL_REALIZE_SATURATION=0`) |
|---|---|---|
| inverse-functional | `x : A`, `y : B` ✗ | `x : A`, `y : B` ✗ |
| **functional** | `y : A`, `z : B` ✗ | **`y : A, B` and `z : A, B` ✓** |

So:

* **The saturation realize path is uniformly wrong** — it drops BOTH functional and
  inverse-functional forced equalities. This is the single defect responsible for the default
  behaviour in both cases, and folding `SaturationResult.derived_same` would fix both.
* **The tableau handles functional merges but not inverse-functional ones.** So there are two
  independent gaps, not one, and they are in different engines.
* **A workaround therefore exists for the functional case only**:
  `RUSTDL_REALIZE_SATURATION=0` gives the correct answer. There is no workaround for the
  inverse-functional case.

This corrects the bullet above, which was written from the inverse-functional fixture alone and
generalised one construct too far.

## Why it is not a false positive

Subtractive only: the missing rows are entailments rustdl fails to report. FP=0 is unaffected.
But it is worse than an ordinary MISS in one respect — `realize --json` emits no `incomplete`
field, so a consumer cannot distinguish "these are all the types" from "some types were
dropped". Classification's `incomplete` flag has no analogue here.

## Toward a fix (not attempted)

`SaturationResult.derived_same` already records functional / inverse-functional-forced
equalities — the data exists. A fix would union types across those groups in
`realize_via_saturation_internal`, which is sound because `derived_same` holds only entailed
equalities.

**The refinement above strengthens the case for exactly this fix.** Because the saturation
path is the DEFAULT and is wrong for *both* constructs, folding `derived_same` there would
correct the default behaviour in both cases — not just one. It would leave a residual: the
tableau path would still miss inverse-functional merges, so `RUSTDL_REALIZE_SATURATION=0`
would remain wrong for that construct. That residual is a second, independent gap in a
different engine and should be tracked separately rather than blocking the first fix.

Not attempted here because `realize` has no folding infrastructure to extend — this is a
designed change, and an ad-hoc attempt in this area was already reverted once today
(`accelerator_share_deadline`,
`docs/2026-08-17-classify-has-no-budget-allocation.md`). Scoping it properly means deciding
where the fold lives, whether `most_specific_types` is recomputed after folding, and how the
absent `incomplete` signal should behave when folding is skipped.

**Pinned by** `crates/owl-dl-reasoner/tests/realize_derived_same.rs`: the asserted-equality
control asserts today's correct behaviour and runs; the derived-equality test asserts the
CORRECT (currently failing) behaviour and is `#[ignore]`d with this file referenced. Remove the
`#[ignore]` when fixing.

## Adjudication status

Not peer-adjudicated: the HermiT wrapper produced no output for this fixture and Konclude's
CLI path here is classification, not realization. The entailment is nonetheless certain —
`InverseFunctional(r) + r(x,z) + r(y,z) ⊨ x = y` is definitional, and **rustdl's own
`individuals` query already derives it**, so the reasoner contradicts itself without needing an
external oracle.

---

## THE INVERSE-FUNCTIONAL HALF IS FIXED — and the mechanism was one missing axiom

**Date:** 2026-08-18 · Flag `RUSTDL_INVERSE_FUNC_MAX`, **default OFF**.

| | `x` | `y` |
|---|---|---|
| default (flag OFF) | `A` | `B` |
| **`RUSTDL_INVERSE_FUNC_MAX=1`** | **`A`, `B`** | **`A`, `B`** |

### What was actually missing

The section above says the tableau path misses inverse-functional merges and calls that "a
second, independent gap in a different engine". **That framing was wrong in a useful way: the
merge was already implemented and default-ON.** `hyper.rs` walks a node's `preds` and merges
`r`-predecessors under `RUSTDL_INVERSE_FUNC_MERGE` — but it is triggered by `node.at_most`, i.e.
by an explicit `≤1` constraint on the node. Nothing ever put one there.

`convert.rs::derive_functional_max_cardinality` emits `∃r.⊤ ⊑ ≤1 r.⊤` for
`FunctionalRole(r)` and **had no inverse-functional counterpart**, so in the reproducer node
`z` — the shared filler, the node whose predecessors must merge — never acquired the `≤1 r⁻`
constraint that fires the merge it needed. The fix emits the missing GCI:

```
InverseFunctionalRole(r)  ⟹  ∃r⁻.⊤ ⊑ ≤1 r⁻.⊤
```

which is the *definition* of inverse-functionality, so it is entailed and cannot introduce a
false positive. **This is the "two engines" reading corrected: one engine, one absent input.**

### Why the fast path is not lost

A derived `≤1` is an unrecognised `Max` to `saturator_complete_fragment`, which would have
pushed **every** inverse-functional-bearing ontology off the saturation fast path — a large
silent perf regression from a flag whose purpose is a narrow realize fix. So the gate learned
the new shape (`is_derived_inverse_functional_max`), exactly as it already knew the functional
one. Verified: all three `inverse_functional/` fixtures report `# mode: pure EL` at **both**
flag settings.

The soundness argument for admitting it is the same one the FP-critical audit established for
the bare `InverseFunctionalRole` admission (`docs/2026-08-18-fp-critical-audit.md` §1): in that
fragment there are no nominals, no `ABox` and no inverse role *use*, so the canonical model is a
tree, every witness has exactly one predecessor, and an at-most-one bound on `r⁻` holds by
construction. The saturator dropping it costs nothing there; the GCI exists for the **wedge**,
which does enforce it.

### Evidence

* **The canary is retired** — `derived_equality_should_share_types` was `#[ignore]`d *for
  failing* and now runs and passes with the flag set.
* **A negative control pins the flag load-bearing** —
  `default_off_still_drops_derived_inverse_functional_equality` asserts the default is *still*
  incomplete, so the fix cannot silently become a no-op. It carries an instruction to delete
  itself when the default flips.
* **Closures identical ON vs OFF** on the three `inverse_functional/` fixtures, `pizza`, `ro`
  and `sio` — the direction of risk here is FP (the change ADDS a constraint), so this was
  checked rather than assumed.

### Why it ships default OFF

The change emits an axiom into **every** ontology carrying an inverse-functional role, and the
wedge then enforces a `≤1` it previously did not. That is a behavioural change on a broad
population, and this repo's own record is explicit that a 12-ontology benchmark is not a
population — a flag flipped on one took four ontologies from ~5 s to DNF. **A flip needs the
two-arm ORE sweep plus a ΔMISSED arm.** Neither has been run.

Note the flip is *also* what would let the shipped `RUSTDL_PSEUDO_MODEL` default recover its
falsified soundness-by-construction argument, since the witness would then apply
inverse-functional merges. That makes the sweep worth running, not a reason to skip it.

---

## THE MECHANISM IS BROADER THAN INVERSE-FUNCTIONAL: cardinality-induced merges too (2026-08-22)

This document, and the `pseudo_model_enabled` doc comment, localise the falsified
soundness-by-construction argument to **inverse-functional** merging ("the witness applies FUNCTIONAL
merges but not INVERSE-functional ones"). **That localisation is too narrow.**

Found by the breadth arm of the pseudo-model bake-off — the frame deliberately chosen because the
falsified clause was *not* expected to bite there: ABox-bearing ORE ontologies carrying **no**
`InverseFunctionalObjectProperty`.

| ontology | ON pairs | OFF pairs | lost | `incomplete` (both arms) |
|---|---:|---:|---:|---|
| `ore_ont_10009` | 86 | 88 | **2** | **false** |
| `ore_ont_11533` | 47 | 49 | **2** | **false** |

**Neither ontology contains `InverseFunctionalObjectProperty`, `FunctionalObjectProperty`, or
`InverseObjectProperties`.** Both contain `ObjectMaxCardinality` (12) and `ObjectExactCardinality`
(19), plus one `TransitiveObjectProperty`. So the merge the witness fails to apply is
**cardinality-induced**, not inverse-functional. They are near-duplicates of each other (identical
construct counts, different md5), so this is **one** pattern seen twice, not two independent ones.

Verified not to be an artifact:

* **Deterministic** — identical loss, identical individuals (`a32071928c`, `a72192307c`) and class
  (`sqdsq`), across repeated runs of both arms.
* **Not a deadline effect** — reproduced at `RUSTDL_REALIZE_PAIR_TIMEOUT_MS=10000`, and
  `incomplete` is `false` on **both** sides, so no probe was cut on either.
* **Subtractive only** — 0 gained pairs, which is the instrument's own check (a gain would mean the
  comparison is broken, since the prune can only remove).

### Consequence

**The loss is SILENT.** Both arms report `incomplete: false`, so `Realization::incomplete` does not
cover this — exactly as this document already warns for the inverse-functional case, and for the
same reason: no probe is cut, the prune simply never asks.

The fix direction is unchanged and now better motivated: the `ABox`-seeded wedge consistency
completion must apply the merges that make the witness a model — and that set includes
**`≤n` / exact-cardinality** merges, not only inverse-functional ones. Restating the general form:
*any* merge rule the real completion applies but the witness build does not is a source of silently
pruned entailments.

Correspondingly, **`RUSTDL_PSEUDO_MODEL=0` is a weaker workaround than advertised** — separately
measured as intractable on 37 of 54 consistent inverse-functional ORE ontologies even at a 600 s cap
(`docs/benchmarks/2026-08-22-pseudo-model-bakeoff.md`).

### Incidental gap found while investigating

`rustdl justify <file> instance I C` cannot resolve an individual that appears only in assertions
and is never `Declaration(NamedIndividual(...))`-declared: it reports
`class IRI not in ontology: <the individual>` in either argument order. That blocked confirming the
lost entailment by justification the way the inverse-functional case was confirmed. Minor, separate,
unfixed.

### The fix is NOT "apply the missing merge" — the flaw is deeper (2026-08-22)

The sections above say the fix "belongs in the witness build: the `ABox`-seeded wedge consistency
completion must apply inverse-functional merges, as it already does functional ones", and the
cardinality finding above extended that to `≤n` merges. **Both statements assume the witness read
loses a merge. It does not.**

`HyperEngine::seeded_individual_labels` (`hyper.rs:2561`) already resolves through the union-find
before reading:

```rust
let rep = self.resolve(HNode(individual_idx));
Some(self.nodes[rep.index()].labels.clone())
```

So a merge that *does* fire is correctly reflected in the witness. The pruned entailments are
therefore ones the completion **never derived**, not ones it derived and then lost on read.

That relocates the flaw in the soundness-by-construction argument, and makes it more fundamental
than a missing rule. The argument reads: *"an entailed type is in every model, hence in the witness,
hence never pruned."* The step that fails is the second one — it conflates **being true in the model**
with **appearing in the completion's label set**. A `Sat` completion is a *pre-model*: node labels
contain what the rules were forced to derive along the branch that happened to close, not everything
true of that node. An entailed membership that holds in every branch by case analysis need not be
labelled in the single branch the search returned.

**Consequence for anyone planning the fix.** Adding merge rules to the witness build may close
specific instances (the two observed mechanisms), but it cannot restore the general argument, because
the gap is between "derived on this branch" and "entailed". Making the prune sound in general would
require the witness to be label-complete for the individuals it prunes — i.e. an intersection over
completions, not one completion — which is a different and much more expensive object, and is the
same reason the FP-unsound `RUSTDL_SNAPSHOT_CAPTURE` trap exists in the opposite direction.

Practical reading, given the measured cost is tiny and the measured benefit is large (see
`docs/benchmarks/2026-08-22-pseudo-model-bakeoff.md`): treat this as a **documented sound
under-approximation of `realize`**, not as a bug awaiting a small patch. What is genuinely missing is
**observability** — the loss is silent, with `incomplete: false` on both arms.

### CONFIRMED: the mechanism is CASE ANALYSIS, on a real-world ontology with no merges at all

The previous section argued from code reading that the flaw is not a missing merge rule but the
gap between *derived on the closing branch* and *entailed*. The breadth arm then produced the
discriminating instance.

**`ore_ont_3892`** — Semantic Web Dog Food (`data.semanticweb.org`), a real-world ontology, **not**
an OAEI benchmark variant:

| construct | count |
|---|---:|
| `ObjectUnionOf` | 21 |
| `DisjointClasses` | 171 |
| `ObjectAllValuesFrom` | 54 |
| `ObjectMaxCardinality` | **0** |
| `ObjectExactCardinality` | **0** |
| `FunctionalObjectProperty` | **0** |

**There is no merge-inducing construct in this ontology, and it still loses 3 entailed types.** So no
addition of merge rules — inverse-functional, `≤n`, or otherwise — could have fixed it. That settles
the question the two earlier sections got wrong.

The lost entailments are `deri-nui-galway : Group`, `talis-information-limited : Group`,
`university-of-manchester-uk : Group` — the covering-disjunction shape. `Group` follows by **case
analysis** over a union (with disjointness eliminating the alternatives), so it holds in every model
while the single `Sat` completion commits to one disjunct and labels only that branch's outcome.
That is the predicted failure mode, observed.

### Final characterisation of the defect

* **Not** a missing merge rule. Three distinct construct profiles produce it: inverse-functional
  (the original fixture), `≤n`/exact cardinality (the OAEI family), and pure
  disjunction + disjointness with no merges (`ore_ont_3892`).
* **The invariant that fails** is "an entailed type appears in the witness". A `Sat` completion is a
  **pre-model**: its labels are what the rules were forced to derive along one branch, which is a
  subset of what is entailed whenever the entailment needs case analysis.
* **Sound** (subtractive — costs entailments, never FP), **deterministic**, and **silent** until
  `witness_prune_active` (added 2026-08-22).
* **Measured frequency on ORE:** 7 of 304 comparable ABox-bearing ontologies, 15 pairs of 4.38M.
  Six are OAEI benchmark variants of one reference ontology; **one is real-world**, which is the
  member that matters for user impact.

**Correct fix, if ever wanted:** make the witness label-complete for the individuals it prunes —
i.e. intersect labels over completions rather than trusting one. That is a materially more expensive
object than the current one-shot witness, and it is the same reason the FP-unsound
`RUSTDL_SNAPSHOT_CAPTURE` trap exists in the opposite direction. Given the measured cost (15 pairs in
4.38M) against the measured benefit (2.4x more completions), the shipped position is deliberate:
keep the prune, document it as a sound under-approximation of `realize`, and report
`witness_prune_active` so a consumer can tell.


---

## INVERSE-FUNCTIONAL ARM CLOSED (2026-08-22) — the other two mechanisms STAND

`RUSTDL_INVERSE_FUNC_MAX` is **default ON** as of 2026-08-22, gated by
`inv_func_merge_consumable`. The GCI is emitted, the predecessor-walking merge fires in the
witness completion, and the pseudo-model prune no longer discards the inverse-functional
entailment. The fixture goes `x:[A]`,`y:[B]` → `x:[A,B]`,`y:[A,B]` at the default.

**What unblocked the flip was a consumability gate, not more budget.** The three ontologies
that blocked it (`ore_ont_9662`, `7532`, `9786`) carry **8**
`InverseFunctionalObjectProperty` each and **zero** `ObjectPropertyAssertion` — so the GCI
placed an `at_most` on eight inverse roles across 288–484 existentials for a merge that could
never fire (19×/47×/31× slowdowns). Emitting only for roles occurring in an
`ObjectPropertyAssertion` removes all three (1.00× each) while preserving the fix.

Evidence: two-arm sweep over all **109** `InverseFunctional`-bearing ORE ontologies — **99
IDENTICAL, 0 ok→DNF, 0 DNF→ok**, wall +0.5%, the single DIFFER adjudicated to concurrency
nondeterminism (four sequential runs byte-identical). ΔMISSED is **subsumed**: identical
classify output cannot have changed MISSED, and realize over the 73 ABox+`InverseFunctional`
frame is 39 comparable with **0 gains and 0 losses**.

**Framed honestly: a correctness fix with ZERO measured corpus benefit and zero measured
cost** — the same basis on which its functional sibling already shipped (recorded as firing on
0 of 64 qualifying ORE ontologies). Do not cite it as a corpus win. The gate deliberately
forgoes `ore_ont_13859`'s classify gain, which also has no `ObjectPropertyAssertion`.

### The falsification is NARROWED, not retired

`pseudo_model_prunes_entailed_type` carried an instruction to retire "both this test and the
falsification note in `realize.rs`" if the default ever stopped pruning. **That instruction
predates the evidence and only half of it is right.** Measured with the flag ON:

| ontology | mechanism | lost |
|---|---|---:|
| `ore_ont_10009` | `ObjectMaxCardinality` / `ObjectExactCardinality` | **2** |
| `ore_ont_3892` | 21 `ObjectUnionOf` + 171 `DisjointClasses`, **no merge construct** | **3** |

So the general failure — a `Sat` completion is a **pre-model** whose labels miss entailments
requiring case analysis — is untouched. Only the inverse-functional arm is closed.
`witness_prune_active` remains the signal, and `ore_ont_3892` remains the discriminator.

**Transferable: a sentinel's own instruction can be stale.** This one was written when
inverse-functional was believed to be the only mechanism; following it literally would have
deleted a still-valid falsification.

### A latent trap that default flips create

`types_of_with_inverse_func_max`'s `false` arm expressed "off" as **`remove_var`** — correct
only while the default was OFF. Under the house idiom (`is_none_or(|v| v != "0")`, absent
ENABLES) the flip silently turned that arm into "on", so the negative control began asserting
against itself. Both arms now set the value explicitly. **When flipping any default, grep the
tests for `remove_var` on that variable.**
