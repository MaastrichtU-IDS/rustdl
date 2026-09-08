# `VarCap`: the Horn-bodied refusals exist, and they are inert — WS2 closed as a negative

**Verdict: stop.** The roadmap's kill condition fires, but on B2 rather than B1, and the route
there corrects two things that were previously inferred rather than measured.

## What was expected, and why that was wrong

`docs/2026-08-03-max-body-vars.md` found 23 binders when raising `MAX_BODY_VARS` 8 → 16 and
recorded that **in each case the withheld clause is disjunctive**. I predicted B1 would therefore
find ~0 Horn-bodied refusals and the kill condition would fire immediately.

**It did not. 11 of 37 refusing ontologies are all-Horn.**

The prior was not wrong — it measured a *different population*. "Binds when the cap is raised to
16" and "is refused at 8" are not the same set. Both hold at once: the bodies that start firing at
16 are disjunctive, while other bodies refused at 8 are Horn. This is why B1 was specified as a
measurement rather than an inference, and it is worth keeping as an instance of a recorded number
being true and still not answering the question asked of it.

## B1 — the census

Instrument: `RUSTDL_TRACE_BODY_VARS=1` over `classify`, all 1,920, 30 s cap, 6-way.
Joined against `examples/clause_stats_probe` (clausify-only) over the same 1,920 to identify
ontologies with `disjunctive=0` — an ontology with only Horn clauses can only have Horn-bodied
refusals, which makes that half of the split *provable* rather than inferred.

| | count |
|---|---:|
| refusing ontologies | **37** |
| of which **all-Horn** (`disjunctive=0`) | **11** |
| of which mixed (`disjunctive>0`) — body indeterminate | 26 |
| unmeasured at the 30 s cap | 134 |
| corpus split: all-Horn / mixed / unmeasured | 1,226 / 689 / 5 |

The 11: `ore_ont_` `1182, 3529, 3575, 5218, 7361, 7775, 9855, 11629, 11745, 12432, 14572`.
(The earlier census said 39; the difference sits inside the 134 unmeasured at this cap.)

**INSTRUMENT VALIDATION, and it nearly cost the whole result.** The first positive control —
twelve `∃`-conjuncts, 13 distinct body variables — produced **zero** trace lines. Had the census
run then, "0 refusals" would have read as "0 addressable" from an instrument that was not firing.
Cause: the fixture was pure-EL, so `classify` took the saturation fast path and the hyper engine
never indexed a clause. Adding one `∀` to force the hybrid path made it fire at once
(`[mbv] refused body: vars=13 ... reason=VarCap { vars: 9, cap: 8 }`). Both negative controls
(`ro`, `pizza`) stay silent. **Prove the instrument fires, on the path the defect lives on.**

## The cap is off by ONE, not mis-tuned across a range

Every one of the 11 refuses at **exactly 9 variables** — not the 9/11/12/16/25/133 spread the
earlier census recorded for binders.

## B2 — does the withheld clause change an ANSWER?

Two arms (`RUSTDL_WIDE_BODY_VARS` 0 vs 1, cap 8 vs 16 — enough to admit every one of the 11 at
9 vars), arm order alternated, 300 s cap, compared on the transitive closure with
`scripts/closure-diff.py`.

| | count |
|---|---:|
| completed in both arms | 4 |
| **answer changes among them** | **0** (gained 0 / lost 0, all four) |
| DNF **symmetrically** in both arms — UNMEASURED | 7 |

`ore_ont_1182` 1,835 · `ore_ont_3529` 3,278 · `ore_ont_7775` 3,765 · `ore_ont_12432` 27,997
closure entries, identical across arms.

**The 7 unmeasured do not weaken the verdict**: they produce no answer in *either* arm, so there
is no answer for a withheld clause to change within reach of this budget. The claim is bounded
accordingly — 0 of 4 measurable, not 0 of 11.

## Why the naive fix stays disqualified

`ore_ont_7775` is in the 11 **and** is one of the three completers `docs/2026-08-03-max-body-vars.md`
records as destroyed by cap 16 (3.14 s → DNF). Raising the cap admits the Horn bodies and the
disjunctive ones together; only a Horn/disjunctive split separates them.

**One discrepancy, recorded not resolved:** here `ore_ont_7775` **completes** in the wide arm
(3 s → 11 s, 3.7× slower, answers identical), where that document records a DNF. Different
conditions are the likely explanation and I did not chase it. Anyone reviving this must re-measure
that case rather than trust either number.

## Verdict

**Kill condition fires on B2.** The shape the roadmap hoped for is real and larger than expected —
11 ontologies, all one variable over the cap — but **inert**: it changes no answer anywhere it can
be measured. A branching-cap policy in the wedge's hot path cannot be justified by a population
whose withheld clauses derive nothing, and the one case that would most benefit is the same case
the cap increase is known to destroy.

**Do not re-propose without new evidence.** The cheapest thing that would change this verdict is a
single ontology where a 9-variable Horn body changes an answer; none of the 4 measurable ones does.
