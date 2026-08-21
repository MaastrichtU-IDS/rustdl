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

---

## PROFILE (2026-08-21, once `perf` was usable): 46% of the time is the ALLOCATOR

`perf` had to be installed first — `/usr/bin/perf` is a dispatcher keyed on `uname -r`, the host
had `linux-tools` for 5.15.0-190 while booted into 5.15.0-97, and `command -v perf` succeeds
regardless. (The versioned binary under `/usr/lib/linux-tools/<ver>/perf` also works unprivileged.)

`ore_ont_10460`, 88.6 s, single-thread, 199 Hz:

| frames | share |
|---|---:|
| **allocator + memmove** (`_int_malloc` 11.4, `_int_free` 8.3, `malloc` 7.7, `cfree` 4.2, `malloc_consolidate` 3.4, `unlink_chunk` 3.0, `realloc` 2.9, memmove 5.3) | **≈46%** |
| `[vdso]` — almost certainly `clock_gettime`, i.e. deadline polling | 5.6% |
| reasoning (`is_blocked` 5.4, `save` 4.9, `solve` 4.0, `apply_concept_rules` 2.9, `apply_role_rules` 1.7, …) | ≈28% |

**Not `match_body`.** That frame dominates the pizza-style workload (26%) and the record's
wedge-classify note, but on this workload the allocator does.

### Root cause: one FULL-GRAPH CLONE per branch

`HyperEngine::save` (`hyper.rs:3268`) deep-clones the whole state on every branch —
`nodes`, `representative`, `neq`, `block_index`, `origin`, and the worklist. The wedge is
**copy-on-branch**, where the main tableau is **trail-based** (`TableauTrail`, log-and-undo via
`Checkpoint`). `hyper-sat` on this ontology:

| | |
|---|---:|
| total branches | **4,632,278** |
| `node_clones` | **4,632,278** (exactly 1:1) |
| `match_attempts` | 160,127,802 |
| `is_blocked` calls | 173,912,436 |
| classes: sat / **unsat** / stalled | 557 / **0** / 44 |

**The CLI already names the fix:** `# node_clones: … (save/restore — trail target)`.

### Two further facts the probe settles

* **`unsat = 0` across all 601 classes.** The wedge never proves a single subsumption here, which
  is the same fact as the banner's `subsumption: saturation=2085 tableau=0` — now confirmed from
  the engine's own side.
* **Branching is MERGE-driven, not disjunctive.** Per stalled class: `disj=36,828` vs
  `merge=119,520` — merges are ~76% of branches. And every stalled class ends at `depth=256`
  (the cap) with `restores` **exactly equal to** `branches`, which is precisely the
  `is_diverging` signature `RUSTDL_ADAPTIVE_BUDGET` exists to early-cut
  (`restores≈branches` at saturated depth). It is nonetheless burning the full 5 s per class, so
  either the probe path does not enable that cut or the window never triggers — **worth checking
  before anything else is built.**

### What this changes about the earlier conclusion

The "documented negative" above stands for the *skip* — that still needs a certifier. But the
profile opens a **second, soundness-free lever the earlier analysis missed**: the futile work is
also grossly inefficient. Converting the wedge's `save`/`restore` from copy-on-branch to a trail
is a pure performance change with identical search semantics — no fragment gate, no proof, no
completeness risk. On this ontology it addresses ~46% of the wall directly.

**Caveat that must travel with it:** making futile work 2× faster leaves it futile. The value is
that several of these 13 sit just outside a 60 s cap, so a ~2× could convert DNFs into
completions — that is a measurement to make, not a claim to assert.

### CHECKED: the adaptive early-cut is NOT the gap — it already fires

The previous section flagged "either the probe path does not enable that cut or the window never
triggers — worth checking before anything else is built." Checked, and it is the former:

* `adaptive_budget` defaults to **`false`** in `HyperEngine`'s constructors and is enabled by
  `with_adaptive_budget()`, called from `owl-dl-reasoner/src/lib.rs:1145` behind
  `adaptive_budget_enabled()`. **Classify enables it; the `hyper-sat` diagnostic does not** — which
  is the whole reason the probe shows 156,000 branches and a saturated depth per stalled class.
* In classify the cut demonstrably fires: `tier_walk` 17,943 ms over 6,936 incomplete pairs is
  **2.59 ms per pair against a 5 ms cap**, i.e. pairs are being cut *before* the cap. And
  6,936 pairs × the ~500-branch `DIV_WINDOW` ≈ 3.47 M branches, the same order as the probe's
  4.63 M `node_clones`.

**So the branch COUNT is already minimised.** What remains is the per-branch cost: ~500 branches
per pair, each deep-cloning the whole graph. That is why `DIV_WINDOW` was recorded as "lower gains
more" — each branch is expensive in itself.

### Therefore: the trail conversion is the lever, and it is now well-founded

The two findings compose rather than compete. The early-cut bounds how many branches happen; the
copy-on-branch `save` makes each one cost an allocation of the entire state. Attacking the second
is the only remaining move that needs no soundness argument.

**Estimated effect, stated as an estimate:** removing ~46% would take `ore_ont_10460`'s `tier_walk`
from 17.9 s to ~10 s and its unbounded wall from 88.6 s to ~48 s. Several of the 13 sit just
outside a 60 s cap, so this could convert DNFs into completions — **to be measured, not assumed.**

**Scope warning for whoever builds it:** `save`/`restore` is in the wedge's hot loop and the wedge
is the default accelerator for every classify. A trail rewrite is a large change to
correctness-critical code whose payoff here is on provably-futile work. It should be gated exactly
as the release process requires — and the honest framing is "makes a futile phase cheaper", not
"fixes the tail".

---

## CORRECTION: the workload is DEADLINE-BOUND, so a faster engine does NOT reduce wall

The section above concluded the trail conversion "addresses ~46% of the wall directly."
**That is wrong, and this section retracts it.**

Two measurements, both cheap, both of which I should have run before writing that:

**1. `DIV_WINDOW` is not the binding constraint.** Made env-tunable and swept on
`ore_ont_10460`:

| `DIV_WINDOW` | 500 | 200 | 100 | 50 |
|---|---:|---:|---:|---:|
| wall | 88.6 s | 88.1 s | 88.0 s | 87.8 s |
| rows | 588 | 588 | 588 | 588 |

A 10× reduction in the divergence window moves the wall **1%**. So `is_diverging` is not what
terminates these pairs — which also qualifies the record's "lower `DIV_WINDOW` gains more" for this
workload.

**2. The per-pair DEADLINE is what terminates them.** Wall against per-pair budget:

| `--pair-timeout-ms` | 1 | 2 | 5 | 10 |
|---|---:|---:|---:|---:|
| wall | 18.6 s | 36.1 s | 88.6 s | 176.1 s |
| **wall / pt** | **18.6** | **18.1** | **17.7** | **17.6** |
| rows | 588 | 588 | 588 | 588 |

**`wall / pt` is constant across a 10× range.** The workload is exactly
`wall = #pairs × per-pair-deadline`: every probed pair burns its entire budget and concludes
nothing, at any budget.

### Why that kills the trail lever *for this workload*

If each pair spends its whole deadline regardless, a 2×-faster engine performs 2× the branches in
the same 5 ms and the wall does not move. The 46% allocator share is a real *profile* observation —
it is simply not convertible into wall here. **Engine speed is irrelevant to a deadline-bound
phase.**

The only two levers that touch `wall = #pairs × deadline` are:

* **fewer pairs** — which is the pruning/certifier problem, already blocked above; or
* **a smaller deadline** — already the shipped default (5 ms), and the record establishes the
  timeout defaults are corpus-optimal, with a *smaller* per-pair budget carrying its own
  documented starvation coupling.

Note in passing that `--pair-timeout-ms 1` gives **18.6 s with identical 588 rows** on this
ontology — 4.8× for free *here* — but that is a corpus-wide default already measured out, not a
finding.

### Net position after the correction

* The tableau phase is futile on 11 of 13 (**stands**).
* An unconditional skip is unsound; no construct profile separates the 2 contributors (**stands**).
* The adaptive early-cut already fires and is not a gap (**stands**).
* **The trail conversion would not reduce wall on this bucket (NEW — retracts the prior section).**
  It may still be worth doing for branch-bound workloads elsewhere; it is not a tail lever.

**So this bucket has no available lever that does not require the certifier.** That is the honest
end state, and it is a stronger negative than the previous section implied.

**Method note.** The retracted claim came from reading a profile and inferring a wall saving
without checking whether the phase was deadline-bound. A profile tells you where time *goes*, not
whether removing it *saves* anything — under a deadline, freed capacity is immediately spent. The
check was two commands.

### The deadline-bound finding GENERALISES — it is not a `tier_walk` fact

Anything that burns a per-pair or per-class budget **to exhaustion** is immune to engine
speed-ups by the same arithmetic. Two buckets of the current tail qualify on their own recorded
numbers:

* **`tier_walk` (13)** — measured above.
* **`label_cache_build` (78)** — the census records a median **17.3 s of a 20 s budget** with
  `pruned=0` on all 78. A phase that consumes its whole budget and prunes nothing is
  deadline-bound by definition, so profiling it and costing out the hot frame will *also* fail to
  convert into wall. Consistent with the already-measured negative there: disabling the phase
  outright over 78×2 arms rescued **2** ontologies and left the aggregate wall **flat (+0.8%)** —
  the freed time was absorbed by the next phase.

**So the general rule for this tail: before costing out any hot frame, check whether the phase is
budget-bound.** If `wall / budget` is constant across budgets, the frame's cost is irrelevant and
the only levers are *fewer units of work* or *a smaller budget*. This is cheap to check — two runs
at different budgets — and it invalidates a profile-derived estimate outright.
