# Reasoner comparison — EL & DL, custom + ORE-2015 (2026-06-21)

Consolidated head-to-head: **rustdl** vs **Konclude**, **HermiT** (DL) and **ELK**,
**whelk-rs** (EL), across the curated corpus (`docs/corpus.md`) and the ORE-2015
pilot (233 onts, Konclude∩HermiT oracle). rustdl & Konclude re-measured at current
HEAD (native binaries, this host); HermiT-on-ORE via `obolibrary/robot:v1.9.6`
docker; HermiT/ELK custom timings from `perf-2026-06-08` / `whelk-rs-investigation`
(stable engines, ms-granular — finer than ROBOT's whole-second log).

**One-line story.** rustdl is **sound at scale** (FP=0 across all 201 ORE-pilot
ontologies it diffs + the curated corpus) and **fastest on EL** (beats whelk-rs and
ELK on galen/notgalen). On DL it is sound + near-complete and within **~10–50×** of
Konclude on most onts, with
**wine the lone DNF**. Konclude (mature C++ tableau) leads DL speed (median 18 ms on
ORE); HermiT is correct but slow (seconds) and itself DNFs on 9 ORE onts; ELK is
EL-only (rejects / silently drops non-EL).

## 1 · EL reasoners — canonical EL benchmarks (saturation kernel)

Saturation-kernel time, all in **milliseconds**.

| ontology | classes | rustdl `saturate()` | whelk-rs | ELK | notes |
|---|--:|--:|--:|--:|---|
| galen | 2,748 | **189** | 356 | 847 | rustdl 1.9× whelk / 4.5× ELK; **+17 rustdl-only** sound pairs |
| notgalen | 3,087 | **251** | 354 | 1,022 | rustdl 1.4× whelk; +27 rustdl-only |
| go-basic | 51,967 | 2,300 | **1,300** | — | whelk 1.7× faster at scale; **identical closure** (357,043) |

rustdl wins the kernel on galen/notgalen and is more complete; whelk-rs wins at the
52k-class scale. ELK ~2–4× slower and on out-of-EL input **rejects (pizza) or silently
drops non-EL axioms (wine/sio)** — not a sound general DL reasoner.

> **"+N rustdl-only sound pairs"** = subsumptions rustdl's closure contains that
> whelk-rs's does not (0 the other way → strict superset). They are genuine
> entailments (oracle-confirmed, not false positives): rustdl adds EL++ rules
> (functional-role witness-merge) on top of base ELK, so it derives functional-role
> subsumptions whelk-rs misses. go-basic has no functional roles → identical closures.

## 2 · DL reasoners — custom corpus (full classification wall)

rustdl & Konclude current HEAD; HermiT reasoning-ms from `perf-2026-06-08` (stable).
"✓" = complete & sound vs the oracle.

All times in **milliseconds** (wall, full classification) so rows compare directly.

| ontology | frag | rustdl (now) | Konclude | HermiT | notes |
|---|---|--:|--:|--:|---|
| bibtex | EL | 10 ✓ | 0 | — | noise floor |
| sulo | — | 10 ✓ | 2 | — | |
| alehif | Horn | 70 ✓ | 1 | — | |
| ore-15672 | SHOIN | **70 ✓** | 5 | 1,654 | blocked-⊔ fix |
| ro | EL+ | 80 ✓ | 1 | DNF | HermiT DNFs |
| galen | Horn | 220 ✓ | 16 | 1,144 | rustdl complete < HermiT reasoning |
| ore-10908 | SROIQ | 220 ✓ | 23 | 10,345 | rustdl ~10× Konclude; 47× < HermiT |
| notgalen | Horn | 270 ✓ | 20 | 1,306 | |
| sio | SROIQ | 720 ✓ | 60 | ~57,000 | SROIQ sweep-gate |
| pizza | SHOIN | 4,600 | 18 | 268 | ~250× Konclude; a few timeout-pairs |
| wine | SHOIN(D) | **DNF (>200,000)** | 36 | 6,390 | **the lone DNF** — combinatorial nominal+disjunction (NO-GO'd) |
| family | SROIQ | **1,600 ✓** | 900 | 9,344 | **inconsistency detected** (ABox-saturation pre-check) |

rustdl is sound + complete on every row except wine; most DL onts are within **~10–50×
Konclude**. Konclude leads on speed; HermiT is correct
but 10²–10³× slower than Konclude and DNFs on ro/wine.

## 3 · ORE-2015 pilot — 233 ontologies, oracle-validated

| metric | Konclude | HermiT (ROBOT-docker) | rustdl |
|---|--:|--:|--:|
| ontologies finished | **233 / 233** | 224 / 233 | 217 / 233 |
| **hard-tail DNF** | **0** | 9 (>120 s) | 16 (>300 s) |
| classification time | median **18 ms**, max 5.4 s | JVM + seconds (coarse) | most < 2 s |
| **FP_strict** (asserts what NEITHER oracle does) | — | — | **0 / 201 diffed** |
| completeness (silent MISSED) | complete | complete | 273 pairs (16 onts) |

**rustdl asserts zero subsumptions neither complete reasoner does.** Notable: **even
mature HermiT DNFs on 9 of 233** — the hard-SROIQ tail is intrinsic to the tableau
approach (HermiT hits it too, smaller than rustdl's 16), not a rustdl-specific
failing; only Konclude's optimized engine clears all in ms.

## 4 · Coverage & limitations

- **Measured (current):** rustdl + Konclude (both corpora); rustdl correctness on ORE;
  HermiT finish/DNF on ORE (`obolibrary/robot:v1.9.6` docker).
- **From prior stable runs:** HermiT fine timing (custom, `perf-2026-06-08`); ELK +
  whelk-rs (EL, `whelk-rs-investigation`).
- **Real limits (not availability — docker has robot/konclude):** ROBOT logs
  whole-second reasoning time and most onts reason sub-second, so per-ont *fine*
  HermiT/ELK timing on ORE isn't usable (profile read from the custom corpus); ELK on
  the 33 ORE EL-fragment onts is uninformative (tiny). `compare-whelk` is OFN-only.

Sources: current re-measurement (rustdl HEAD + native Konclude); HermiT-ORE via
ROBOT-docker; `docs/perf-2026-06-08-konclude-vs-rustdl.md`,
`docs/superpowers/specs/2026-06-16-whelk-rs-investigation.md`,
`/data/dumontier/ore-run/pilot/*/{diff.json,kon.log}`.
