# rustdl reports `consistent: true` on two ORE ontologies Konclude AND KM call inconsistent

**Status: FIXED in v0.4.25 by #97**, which consults the wedge consistency route in
classify's inconsistency pre-check. Re-measured on the released binary: `classify --json`
reports `consistent: false` with 40 unsatisfiable classes on BOTH ontologies, agreeing
with `rustdl consistent`, Konclude and KM.

Was: live on v0.4.24 (`0642465`) and identically on v0.4.23 — PRE-EXISTING, not a release
regression. Sound (a MISS, never an invented entailment). Two-peer confirmed.

**This page stayed marked "live" for the whole of 0.4.25's preparation while its sibling
`dkey-numeric-buckets-are-not-disjoint.md` was corrected — the same drift, caught in one
place and missed in the other, and only found by re-measuring the inventory instead of
listing it from memory.** #97's own finding is the part worth carrying: the verdict
already existed as `consistency: wedge Unsat` and was simply not consulted from classify,
so neither the DKey-aware ABox saturation nor the `decide(Top)` probe that #89 proposed
was needed. The recorded dead-end for `decide(Top)` concerns an UNBOUNDED probe; the
wedge route is bounded and measures 2.34 ms on pizza.

## The disagreement

| | `ore_ont_16321` | `ore_ont_4198` |
|---|---|---|
| Konclude v0.7.0 | `owl:Thing ≡ owl:Nothing` → **inconsistent** | **inconsistent** |
| Kobayashi-MaRust v0.2.32 | `consistent: false` | `consistent: false` |
| rustdl `classify --json` (v0.4.24) | `consistent: true`, `unsatisfiable: []` | same |
| rustdl `classify --json` (v0.4.25, FIXED) | `consistent: false`, 40 unsat | same |
| rustdl `classify --json` (v0.4.23) | `consistent: true` | same |
| rustdl `consistent` subcommand | `consistent` | `consistent` |

Both are 40-class ontologies in the BioPAX-level-2 namespace (`dlapproximated/00009`,
`.../00010`-family), no `owl:imports`. Konclude's "41 unsat classes" is 40 classes plus
`owl:Thing` — i.e. Konclude is reporting KB inconsistency, not 41 independent unsat
classes. That is what makes KM's `consistent: false` a *corroboration* of the same claim
rather than a separate one.

**rustdl's two surfaces AGREE here.** This is NOT a #66-style classify-vs-`subclass`
internal disagreement: `classify`, the `consistent` subcommand, and the per-pair `sat`
probe all say satisfiable/consistent. It is a genuine completeness gap in inconsistency
detection, not a dispatch bug.

## Why it matters more than "41 missed unsat classes"

The MISSED net scores these as 41 missed unsat classes each — 82 of the corpus-wide 89.
Read that way they look like a large completeness deficit concentrated in two files.
Read correctly they are **one defect each**: a missed KB inconsistency, from which the 41
follow. Do not double-count them against completeness metrics.

## Known class, now with a concrete instance

CLAUDE.md already records that classify's inconsistency detection is a sound
UNDER-approximation — `abox_saturation` + `top_is_unsat` are pre-checks, and a
tableau-only inconsistency can be missed with `consistent: true` reported. What was
missing was a real corpus instance. These are two, both peer-confirmed.

## Evidence caveat that made this findable

Before KM, 82 of the 89 corpus-wide missed-unsat classes rested on a **Konclude-only**
oracle: HermiT returned `NO_OUTPUT` on both of these ontologies, so the union oracle
degraded to a single peer, and this repo's own rule is that a single peer is weak
evidence (Konclude is documented to under-report elsewhere). Running KM supplied the
missing second peer and turned an unverified single-oracle claim into a confirmed one.

**KM is NOT an oracle** — it is measured-unsound on ~1795 ORE ontologies and it misses
things (on `ore_ont_6951` it agrees with rustdl's `unsat=0` against Konclude AND HermiT,
which both say 2). Its value here is narrow and specific: an independent second voice on
a claim that otherwise had one.

## Severity

Sound — rustdl under-reports, never over-reports. But **silent**: a consumer reading
`consistent: true` has no signal. Two ontologies of 424 in the release population.
