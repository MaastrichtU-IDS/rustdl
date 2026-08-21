# The `tier_walk` bucket: the tableau phase contributes NOTHING on 11 of 13 — ~89× available, blocked

**Date:** 2026-08-21 · Executes `docs/superpowers/plans/2026-08-21-tier-walk-400x-gap.md`
· Target chosen by reading ONE failing ontology, per this repo's own method note.

**Headline: on 11 of the 13 `tier_walk` tail members, `--saturation-only` produces BYTE-IDENTICAL
output — aggregate ~1,596 s → ~18 s (89×). But 2 of 13 gain real entailments from the tableau, so
an unconditional skip is a completeness regression, and the certifier that would make it safe is a
fragment gate needing a PROOF.**

## Step 1 (the gate): no completeness gap — the 400× is purely performance

`ore_ont_10460`, **601 classes**, 750 `SubClassOf`: Konclude **0.22 s** / 1,226 subsumptions vs
rustdl **88.59 s** / 588 rows. That row gap is **not** a defect: all 601 differences are
`X ⊑ owl:Thing`, one per class, rustdl-only is **0**, and 2,859 + 601 = 3,460 = Konclude's count
exactly. The same TOP-convention artifact as the KM head-to-head. **Closures agree; this is a pure
perf problem.**

Running this step first was load-bearing — taking 588-vs-1,226 at face value would have sent the
investigation after a completeness bug that does not exist.

## The probed pairs are NON-CONCLUDABLE

| `--pair-timeout-ms` | wall | rows | incomplete pairs |
|---|---:|---:|---:|
| 5 (default) | 88.6 s | 588 | **6,936** |
| 50 | **DNF at 300 s** | 0 | — |

Cost scales linearly with the per-pair budget and buys nothing. These are not slow-but-decidable
pairs; they absorb whatever budget they are given. **A bigger budget is not the lever.**

## The tableau phase yields ZERO subsumptions

`ore_ont_10460`'s own banner says it: `subsumption: saturation=2085 tableau=0`. Confirmed
end-to-end — `--saturation-only` gives all 588 rows in **0.02 s** against 88.59 s, row sets
**identical**. 4,400× for the same answer.

## Step 5 — the bucket, measured

| ontology | sat-only | full | verdict |
|---|---:|---:|---|
| `ore_ont_10460` | 0.02 s | 88.60 s | sat suffices |
| `ore_ont_10807` | 0.02 s | 69.69 s | sat suffices |
| `ore_ont_2901` | 0.02 s | 78.06 s | sat suffices |
| `ore_ont_7828` | 0.02 s | 160.68 s | sat suffices |
| `ore_ont_16371` | 0.03 s | 142.05 s | sat suffices |
| `ore_ont_5764` | 0.03 s | 161.11 s | sat suffices |
| `ore_ont_9890` | 0.03 s | 83.74 s | sat suffices |
| `ore_ont_10949` | 0.10 s | 167.11 s | sat suffices |
| `ore_ont_7409` | 3.58 s | 88.63 s | sat suffices |
| `ore_ont_16462` | 6.35 s | 171.39 s | sat suffices |
| `ore_ont_10568` | 6.41 s | 190.74 s | sat suffices |
| **`ore_ont_10517`** | 0.03 s | 119.74 s | **tableau contributes 6 rows** |
| **`ore_ont_8388`** | 1.38 s | 74.21 s | **tableau contributes 10 closure pairs** |

**Aggregate over the 11: ~1,596 s → ~18 s (89×), output identical.**

### The 2 exceptions are what block the obvious fix

`ore_ont_10517` gains 6 rows and `ore_ont_8388` gains 10 closure pairs from the tableau. **So
skipping the tableau unconditionally loses entailments.** The lever requires knowing *in advance*
that the tableau cannot contribute — which is a fragment gate, and the record is explicit that
*"a certifier is a fragment gate needing a PROOF, not a benchmark."* Eleven-of-thirteen is a
benchmark.

`ore_ont_8388` also needed care to read: it shows **−14 `direct` rows** under full classify, which
looks like a completeness loss. On the **closure** it is 162,708 → 162,718, i.e. **+10 gained, 0
lost** — the deficit was Hasse re-parenting. That trap (comparing `direct` rows instead of
closures) has now produced a false reading three times in three days; always close first.

## What did NOT work

* **`perf` is unusable on this host** — `command -v perf` succeeds but the kernel tools are absent
  (`WARNING: perf not found for kernel 5.15.0-97`). Judge a tool by its output, not its exit code.
* **Construct ablation is confounded here.** Removing any one of `DisjointUnion` (2),
  `DisjointClasses` (10) or `TransitiveObjectProperty` (2) drops 88.6 s → 8.0 s *identically*,
  with unchanged rows. Each deletes 2–10 axioms at once, so no construct attribution is
  justified — the same mis-attribution the record warns about ("a controlled deletion is only
  controlled if the intervention changed ONE thing"). The uniform 8.0 s across three different
  ablations suggests an interaction, not a single culprit. **Unresolved.**

## Where this leaves it

The prize is real and large (89× on 11 tail members, and these are *tail* members — several would
classify well inside a 60 s cap). The blocker is precise: **an unconditional skip is unsound, and a
sound gate needs a proof that saturation is complete on the input.** The two exceptions are the
counterexamples that any proposed gate must exclude, and they are cheap to test against.

**Next step for whoever continues:** characterise what distinguishes `10517`/`8388` from the other
eleven. If the distinguishing feature is structural and cheaply detectable, it is a gate; if it is
not, this stays a documented negative. Do NOT propose the skip without it.

---

## OUTCOME: no gate is available, and the plan's kill criterion applies

The plan's next step was to characterise what distinguishes the 2 contributors from the 11 futile
members, with the stated rule: *"Structural and cheaply detectable → that's the gate. Otherwise
this stays a documented negative."*

**There is no separator.** Construct profiles of the 2 vs the 11:

| construct | in the 2 contributors | in the 11 futile |
|---|---|---|
| `ObjectAllValuesFrom` | **0 / 2** | present in **4 / 11** |
| `DisjointUnion` | 0 / 2 | present in 2 / 11 |
| `ObjectSomeValuesFrom` | 2 / 2 | 10 / 11 |
| `ObjectUnionOf` | 2 / 2 | 10 / 11 |
| `EquivalentClasses` | 2 / 2 | 11 / 11 |
| `DisjointClasses` | 2 / 2 | 10 / 11 |
| cardinality / nominals / inverse / functional | present in ≤1 of 2 | present in 1–8 of 11 |

Every construct in the contributors is also in most of the futile ones, and `∀` — the obvious
candidate for "needs a tableau" — is **absent from both contributors** while present in four futile
members. So the discriminator is not the construct profile. **No cheap static gate exists on this
evidence, and per the plan this is now a documented negative rather than a fix.**

## The number that reframes it

| | |
|---|---|
| tableau-phase cost across the 13 | **1,578 s** |
| entailments it produced | **16** (`10517` +6 rows, `8388` +10 closure pairs) |
| **cost per extra entailment** | **≈99 s** |
| members where it produced nothing | **11 of 13** |
| speedup if skipped | **89×** |

Those 16 are also proportionally negligible where they occur — 6 of 1,140 rows, and 10 of 162,718
closure pairs (0.006%).

So this is not a correctness question but a **cost/benefit** one, and rustdl already exposes the
choice: `--saturation-only` is a documented sound under-approximation, and classify already
reports an `incomplete` signal. On this bucket, that flag is **89× faster and on 11 of 13 loses
nothing at all**.

**Actionable conclusion, no engine change:** the honest deliverable here is guidance, not a gate —
on `tier_walk`-bound ontologies, `--saturation-only` buys 89× for a ~0.006% completeness risk that
is *zero* on 11 of the 13 measured. An automatic switch remains blocked on the certifier problem,
and the 2 contributors are the counterexamples any future gate must exclude.
