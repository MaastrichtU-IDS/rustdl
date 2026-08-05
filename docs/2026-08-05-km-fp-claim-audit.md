# Audit: the "KM 10 ontologies FP / ~1795 spurious pairs" claim does not reproduce

**Date:** 2026-08-05 · **KM:** `v0.2.5` (commit `408dee4`), built from source
**Claim audited:** project memory `km-headtohead-rustdl-fp.md` and its writeup
`docs/superpowers/specs/2026-07-18-km-headtohead-and-rustdl-FP.md` —
*"KM vs Konclude (n=1356): 1138 exact + 12 km>kon → **10 GENUINE KM FPs** (unsat-normalized km
still > Konclude AND HermiT: 6833+787, 4577+404, 11647+349, 12270+123,
9054/16708/6967/3685+29, 15063+14, 7517+2 ≈1795 spurious)"*, summarised as
*"rustdl WINS all axes incl soundness"*.

**Why it was audited:** on 2026-08-05 I twice misread another reasoner's output format — once
reading Konclude's `EquivalentClasses(Thing Nothing …)` as "consistent with N unsat classes", and
once reading KM's boolean `CONSISTENT 0` as a subsumption count, the latter in a **public** issue.
Both are convention-misreads of exactly the kind that could manufacture a phantom FP, so the
standing FP claim needed checking rather than repeating.

## Result: FP = 0 on every testable ontology

Run with `normalise.py compare`, which excludes unsatisfiable and `⊤`-equivalent classes
**symmetrically** across candidate and oracle (rule R4):

| ontology | claimed excess | KM closure | Konclude closure | excluded classes | **FP** | MISSED |
|---|---:|---:|---:|---:|---:|---:|
| `ore_ont_6833` | +787 | 3356 | 3356 | 1 | **0** | 0 |
| `ore_ont_4577` | +404 | 1227 | 1227 | 1 | **0** | 0 |
| `ore_ont_12270` | +123 | 2403 | 2403 | 10 | **0** | 0 |
| `ore_ont_9054` | +29 | 675 | 676 | 0 | **0** | 1 |
| `ore_ont_16708` | +29 | 680 | 681 | 0 | **0** | 1 |
| `ore_ont_6967` | +29 | 712 | 713 | 0 | **0** | 1 |
| `ore_ont_3685` | +29 | 698 | 699 | 0 | **0** | 1 |
| `ore_ont_7517` | +2 | 187 | 187 | 0 | **0** | 0 |
| `ore_ont_11647` | +349 | — | — | — | **untested** | — |
| `ore_ont_15063` | +14 | 168 | 0 | — | **not adjudicable** | — |

`11647`: KM `v0.2.5` exits `worker engine exited -1`. `15063`: Konclude's output normalises to 0
pairs, so there is no oracle. Neither is evidence either way.

## The largest component is a METHOD artifact, independent of KM's version

**1,314 of the ~1,795 pairs (73%)** — the `6833`, `4577` and `12270` rows — are the `⊤`-equivalence
enumeration convention, not a KM defect. `ore_ont_6833` asserts, at line 6572 of the source:

```
SubClassOf(owl:Thing <http://www.absoluteiri.edu/RELAPPROXC38616>)
```

so `RELAPPROXC38616` is genuinely `⊤`-equivalent and **every** class is subsumed by it. KM
enumerates those subsumptions (`BSPO_0000029 ⊑ RELAPPROXC38616`, …); Konclude **collapses** them
into `EquivalentClasses(owl:Thing, RELAPPROXC38616)` and emits **zero** explicit `SubClassOf` into
it. Comparing the two closures without normalising that convention makes KM look like it invented
787 subsumptions it in fact correctly derived. Excluding the one `⊤`-equivalent class makes the two
closures **exactly equal, 3356 = 3356**. Same shape for `4577` (1 class) and `12270` (10 classes,
plus 4 unsat disagreements).

**This part of the retraction does not depend on KM's version.** It is a property of the output
conventions, so it would have been an artifact on 2026-07-19 too. The original analysis explicitly
normalised the `⊥` side (*"unsat-normalized"*, and it caught 2 cases that way — `6951`/`7496`) but
the record never mentions the `⊤` side. `normalise.py` R4 — written later, in August — handles both,
which is why the artifact is visible now.

## The remaining ~132 pairs: FP = 0 today, cause not separable

The `+29 × 4`, `+14` and `+2` rows have **no** `⊤`-equivalent classes, so the artifact above does not
explain them — and on `v0.2.5` they are nonetheless **FP = 0**, with KM *missing* one pair on four of
them. Two explanations are consistent with this and **I cannot separate them**: either they were real
FPs that KM has since fixed (plausible — this is 181+ commits and 5 releases later, and KM ships
`docs/CONTESTED-GOLD.md` plus per-release soundness reports), or they were a further artifact. The
memory's content-level diagnosis for this group — `AboveRoomTemperature ⊑ Cold/Heat`,
`FastExposure ⊑ SlowExposure`, `LargeFormat ⊑ MediumFormat`, attributed to concrete-domain collapse
since Sequoia has no datatypes — is specific and cannot be produced by a format misread, so it was
probably genuine when written. Those class names still appear in `v0.2.5` output but **no longer as
subsumption pairs**.

## What the record should say

- **Withdraw "~1795 spurious pairs".** 73% of it is provably a `⊤`-equivalence convention artifact;
  the rest is not reproducible on current KM.
- **Withdraw "rustdl WINS all axes incl soundness"** as a present-tense claim. On the nine cited
  ontologies I can measure, current KM has **no** false positive against Konclude.
- **Keep**, because it is independently established and unaffected: rustdl's own FP = 0 record, and
  KM's *incompleteness* relative to Konclude (it misses 1 pair on four of these).
- **Keep the version caveat in both directions.** The claim may well have been true when measured;
  what is now established is that it is not true today, and that its largest component was never
  sound methodology.

## Instrument fixes made

- `normalise.py parse_km` accepted only KM's pre-`v0.2.4` dict-of-bare-names format and raised
  `AttributeError` on the current list-of-IRI-pairs format. Both are now accepted
  (harness `1e027af`). It failed **loudly**, which is correct — I initially mistook it for a
  silent-zero bug because I had run it under `2>/dev/null`. Worth stating plainly: the tool was
  right and my invocation was wrong.
- Companion, from earlier the same day: `triage.py` now emits a distinct `INCONSISTENT` verdict
  (harness `a16a128`).

## Method note

**A cross-reasoner "FP" that consists of one class receiving thousands of new subsumers is a
convention artifact until proven otherwise.** The tell in all three big cases is the shape of the
excess — 787 pairs all sharing a single superclass — not its size. Check whether that superclass is
`⊤`-equivalent (or the subclass `⊥`-equivalent) *before* attributing it to the engine. Both the `⊥`
and `⊤` collapses must be normalised, and only the `⊥` half was.
