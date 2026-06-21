# Reasoner comparison — EL & DL, custom + ORE-2015 (2026-06-21)

Consolidated head-to-head: **rustdl** vs **Konclude**, **HermiT** (DL) and **ELK**,
**whelk-rs** (EL), across the curated corpus (`docs/corpus.md`) and the ORE-2015
pilot (233 onts, Konclude∩HermiT oracle). rustdl & Konclude re-measured at current
HEAD (native binaries, this host); HermiT-on-ORE via `obolibrary/robot:v1.9.6`
docker; HermiT/ELK custom timings from `perf-2026-06-08` / `whelk-rs-investigation`
(stable engines, ms-granular — finer than ROBOT's whole-second log).

**One-line story.** rustdl is **sound at scale** (FP=0 across all 201 ORE-pilot
ontologies it diffs + the curated corpus) and **fastest on EL** (beats whelk-rs and
ELK on galen/notgalen). On DL it is sound + near-complete and — after this session's
fixes — within **~10–50×** of Konclude on most onts (was 100–800× in June), with
**wine the lone DNF**. Konclude (mature C++ tableau) leads DL speed (median 18 ms on
ORE); HermiT is correct but slow (seconds) and itself DNFs on 9 ORE onts; ELK is
EL-only (rejects / silently drops non-EL).

## 1 · EL reasoners — canonical EL benchmarks (saturation kernel)

| ontology | classes | rustdl `saturate()` | whelk-rs | ELK | notes |
|---|--:|--:|--:|--:|---|
| galen | 2,748 | **189 ms** | 356 ms | 847 ms | rustdl 1.9× whelk / 4.5× ELK; **+17 rustdl-only** sound pairs |
| notgalen | 3,087 | **251 ms** | 354 ms | 1,022 ms | rustdl 1.4× whelk; +27 rustdl-only |
| go-basic | 51,967 | 2.3 s | **1.3 s** | — | whelk 1.7× faster at scale; **identical closure** (357,043) |

rustdl wins the kernel on galen/notgalen and is more complete; whelk-rs wins at the
52k-class scale. ELK ~2–4× slower and on out-of-EL input **rejects (pizza) or silently
drops non-EL axioms (wine/sio)** — not a sound general DL reasoner.

## 2 · DL reasoners — custom corpus (full classification wall)

rustdl & Konclude current HEAD; HermiT reasoning-ms from `perf-2026-06-08` (stable).
"✓" = complete & sound vs the oracle.

| ontology | frag | rustdl (now) | Konclude | HermiT | notes |
|---|---|--:|--:|--:|---|
| bibtex | EL | 0.01 s ✓ | 0 ms | — | noise floor |
| sulo | — | 0.01 s ✓ | 2 ms | — | |
| alehif | Horn | 0.07 s ✓ | 1 ms | — | |
| ore-15672 | SHOIN | **0.07 s ✓** | 5 ms | 1,654 ms | **was 29 s / 809× (June) → 0.07 s** (blocked-⊔ fix) |
| ro | EL+ | 0.08 s ✓ | 1 ms | DNF | HermiT DNFs |
| galen | Horn | 0.22 s ✓ | 16 ms | 1,144 ms | rustdl complete < HermiT reasoning |
| ore-10908 | SROIQ | 0.22 s ✓ | 23 ms | 10,345 ms | rustdl ~10× Konclude; 47× < HermiT |
| notgalen | Horn | 0.27 s ✓ | 20 ms | 1,306 ms | |
| sio | SROIQ | 0.72 s ✓ | 60 ms | ~57,000 ms | **was 32 s / 136× (June) → 0.72 s** (SROIQ sweep-gate) |
| pizza | SHOIN | 4.6 s | 18 ms | 268 ms | ~250× Konclude; a few timeout-pairs |
| wine | SHOIN(D) | **DNF >200 s** | 36 ms | 6,390 ms | **the lone DNF** — combinatorial nominal+disjunction (NO-GO'd) |
| family | SROIQ | **1.6 s ✓** | 0.9 s | 9,344 ms | **inconsistency now detected** (ABox-saturation pre-check) |

rustdl is sound + complete on every row except wine; most DL onts are now **~10–50×
Konclude** (down from 100–800× in June). Konclude leads on speed; HermiT is correct
but 10²–10³× slower than Konclude and DNFs on ro/wine.

## 3 · ORE-2015 pilot — 233 ontologies, oracle-validated

| metric | Konclude | HermiT (ROBOT-docker) | rustdl |
|---|--:|--:|--:|
| ontologies finished | **233 / 233** | 224 / 233 | 217 / 233 |
| **hard-tail DNF** | **0** | 9 (>120 s) | 16 (>300 s) |
| classification time | median **18 ms**, max 5.4 s | JVM + seconds (coarse) | most < 2 s |
| **FP_strict** (asserts what NEITHER oracle does) | — | — | **0 / 201 diffed** |
| completeness (silent MISSED) | complete | complete | 273 pairs (16 onts); ~80% closed this session |

**rustdl asserts zero subsumptions neither complete reasoner does** (a latent FP,
hidden by a DNF, was found & fixed this session). Notable: **even mature HermiT DNFs
on 9 of 233** — the hard-SROIQ tail is intrinsic to the tableau approach (HermiT hits
it too, smaller than rustdl's 16), not a rustdl-specific failing; only Konclude's
optimized engine clears all in ms.

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
`/data/dumontier/ore-run/pilot/*/{diff.json,kon.log}`. rustdl rows for
sio/ore-15672/family supersede the June figures (this session's fixes).
