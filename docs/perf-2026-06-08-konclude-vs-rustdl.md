# rustdl vs Konclude vs HermiT — full-corpus classification benchmark (2026-06-08)

**Headline correction:** measured against the **native Konclude binary** (not
docker), Konclude wins on every ontology with real reasoning work, and rustdl's
faster-looking out-of-EL numbers are *incomplete* (timed-out pairs) or DNF (wine).
The earlier "rustdl beats Konclude wall-to-wall" / "ORE-10908 ≤5× (3.1×)" headlines
(docs `perf-2026-06-03/04`) were **docker-startup artifacts** and do not survive a
native comparison. See "Wall-basis change" below.

## Methodology

- **rustdl**: HEAD v0.3.6 + reform increment 1 (`512fe56`), `cargo build --release
  -p owl-dl-cli`, all defaults ON (wedge, `trust_sat`, Horn-shortcircuit, snapshot
  cache, DifferentIndividuals→wedge). `classify --pair-timeout-ms 200 <in.ofn>`,
  wall via `date +%s%N` under `timeout 180s`. Slow rows re-run serially on an idle
  host.
- **Konclude**: native binary **v0.7.0-1138** (Jun 18 2021), `Konclude classification
  -w AUTO -i <in.owx> -o <out.owx>`. Total wall + the `Finished class classification
  in N ms` reasoning line. Inputs: pristine `.owx` where present, else ROBOT v1.9.6
  `convert` from `.ofn` (14 conversions, class counts cross-checked).
- **Spot-check (this author, independent of the run):** native Konclude galen
  0.125 s / 7 ms, ore-10908 0.080 s / 22 ms; rustdl galen 0.58 s complete — confirms
  the table's basis and the docker-artifact reversal.

### Wall-basis change — ratios NOT comparable to the 2026-06-04 doc
Prior docs timed Konclude under **docker** (≈0.5–2.2 s walls, ~1.5 s of which is
container startup — the docs flagged this but still divided by it). This run uses
the **native binary** (total wall ≈ reasoning + ~30 ms process floor; 0.03–0.9 s).
The denominator lost docker's startup, so every ratio here is ~5–30× larger than
06-04 for the *same engine work*. **The cross-doc-stable metric is Konclude
reasoning-ms** (1–334 ms here ≈ 9–127 ms in 06-04). Use reasoning-ms for fair
engine-vs-engine; total-wall ratio is real end-to-end on the native basis.

## Results (sorted by ratio = rustdl wall / Konclude total wall)

| Ontology | #cls | rustdl frag | rustdl wall | Konclude wall | Konclude reason | ratio | note |
|---|---:|---|---:|---:|---:|---:|---|
| anch-module | 12 | out-of-EL | 0.01 s | 0.030 s | 1 ms | 0.3× | noise floor |
| asp-module | 20 | out-of-EL | 0.01 s | 0.030 s | 1 ms | 0.3× | noise floor |
| sio-450-module | 10 | out-of-EL | 0.01 s | 0.030 s | 1 ms | 0.3× | noise floor |
| bibtex | 15 | pure-EL | 0.01 s | 0.028 s | 0 ms | 0.4× | noise floor |
| sulo-stripped | 17 | out-of-EL | 0.02 s | 0.028 s | 2 ms | 0.7× | noise floor |
| alehif-test | 167 | Horn | 0.16 s | 0.185 s | 1 ms | 0.9× | complete; borderline tie |
| sulo | 17 | out-of-EL | 0.03 s | 0.028 s | 2 ms | 1.1× | noise floor |
| galen | 2748 | Horn | 0.59 s | 0.272 s | 12 ms | 2.2× | **complete** (closure 27 997) |
| ro | 58 | out-of-EL | 0.51 s | 0.206 s | 2 ms | 2.5× | complete |
| ro-stripped | 58 | out-of-EL | 0.52 s | 0.192 s | 2 ms | 2.7× | complete |
| notgalen | 3087 | Horn | 1.05 s | 0.282 s | 17 ms | 3.7× | **complete** (closure 32 739) |
| go-basic | 51937 | pure-EL | 18.42 s | 4.13 s | 295 ms | 4.5× | complete; both load-dominated |
| ore-15516-alchoiq | 84 | out-of-EL | 0.41 s | 0.076 s | N-A | 6.1× | both: inconsistent / all-unsat (agree) |
| shoiq-knowledge | 144 | out-of-EL | 1.13 s | 0.177 s | 3 ms | 6.4× | complete |
| sio-fp-module | 15 | out-of-EL | 0.22 s | 0.033 s | 2 ms | 6.7× | complete |
| sio-fp2-module | 74 | out-of-EL | 0.46 s | 0.038 s | 5 ms | 12.1× | complete |
| sio-fp3-module | 74 | out-of-EL | 0.46 s | 0.035 s | 6 ms | 13.1× | complete |
| np-module | 34 | out-of-EL | 1.32 s | 0.037 s | 5 ms | 35.7× | **INCOMPLETE** (4 timed-out pairs) |
| pizza | 99 | out-of-EL | 2.07 s | 0.055 s | 15 ms | 37.6× | **INCOMPLETE** (4 timed-out pairs) |
| ore-10908-sroiq | 692 | out-of-EL | 5.43 s | 0.080 s | 23 ms | 67.9× | complete |
| sio | 1585 | out-of-EL | 32.00 s | 0.235 s | 59 ms | 136× | **INCOMPLETE** (8 timed-out) |
| sio-stripped | 1585 | out-of-EL | 31.97 s | 0.220 s | 55 ms | 145× | **INCOMPLETE** (8 timed-out) |
| ore-15672-shoin | 82 | out-of-EL | 29.11 s | 0.036 s | 5 ms | 809× | **INCOMPLETE** (109 timed-out) |
| wine | 137 | out-of-EL | **DNF (>180 s)** | 0.127 s | 33 ms | — | DNF@200ms; 54.5 s@25ms with 9210 timed-out pairs |
| family† | 58 | out-of-EL | 81.15 s | 0.887 s | N-A | — | **NOT COMPARABLE** (Konclude inconsistent) |
| family-stripped† | 58 | out-of-EL | 80.85 s | 0.887 s | N-A | — | **NOT COMPARABLE** (Konclude inconsistent) |

## Completeness caveat (essential for honest reading)
rustdl's `--pair-timeout-ms 200` is a **sound under-approximation**: pairs over
budget default to *not-subsumed*. Every **INCOMPLETE** row may be missing real
subsumptions while Konclude completed — so those ratios compare rustdl's
*truncated* time to Konclude's *complete* time. To match Konclude's closure rustdl
would need an unbounded budget and be far slower or DNF. The **complete** rows
(Horn/pure-EL with 0 timed-out pairs: galen, notgalen, go-basic, alehif, bibtex,
ro, ro-stripped, shoiq-knowledge, sulo*, anch/asp, sio-450, sio-fp*) are the honest
complete-vs-complete comparisons. **wine** is the extreme: rustdl doesn't classify
it (DNF@200ms; @25ms it returns in 54.5 s but defaults 9210 pairs); Konclude does
it completely in 0.13 s.

## Where rustdl "wins" — only where there's no work
Noise-floor wins (both sub-50 ms; rustdl's process floor undercutting Konclude's
~30 ms native load floor): anch, asp, bibtex, sio-450, sulo-stripped (0.3–0.7×).
alehif-test (0.9×, 167-class Horn) is the only borderline tie with non-trivial
size. **No ontology with meaningful reasoning work has rustdl ahead on native
walls.**

## Where rustdl loses — out-of-EL SROIQ (the engine gap)
ore-15672 **809×**, sio/sio-stripped **136–145×**, ore-10908 **67.9×** (complete),
pizza/np **~36×**. And the docker-era proud claim "ORE-10908 ≤5× (3.1×)" lived
entirely in docker's ~1.7 s denominator; native it is ~68×. Even on the **complete
Horn** path rustdl trails: galen **2.2×**, notgalen **3.7×**.

## Disagreements / not-comparable
- **family / family-stripped**: Konclude reports **INCONSISTENT**; rustdl reports
  consistent + classifies (its frontend silently drops unrecognized data-property
  axioms — sound under-approximation, `owl-dl-datatypes`). Different problems → no
  ratio. (ROBOT preserved the data axioms; verified Konclude still goes inconsistent.)
- **ore-15516-alchoiq**: Konclude **inconsistent**; rustdl reports **all 84 classes
  unsatisfiable** (its inconsistency mirror). They **agree operationally**; ratio kept.

## Honest one-line summary
On the native Konclude binary, **rustdl wins only where there is essentially no
work** (tiny EL/Horn fixtures, riding its process floor); on every ontology with
real reasoning Konclude wins — **2.2× (galen, complete) to 809× (ore-15672,
incomplete)** — and rustdl's faster-looking hard-case numbers are themselves
**incomplete** or **DNF**. The prior "beats Konclude" / "ORE-10908 ≤5×" headlines
were docker-startup artifacts. rustdl's genuine strengths remain **soundness
(FP=0/MISSED=0 corpus-wide)** and the **EL/Horn saturation fast path** (complete
and sub-second on GALEN-scale Horn TBoxes); its gap is the **out-of-EL SROIQ
tableau**, exactly the frontier this session measured out (1-UIP NO-GO, A1 dead,
B-perf 0%, M1 ruled out, M2 shipped sound but wall-flat, M3 premise red).

## Three-way — HermiT added

**HermiT 1.4.5.456** via ROBOT v1.9.6 (`robot reason --reasoner HermiT`, docker).
Same JVM/docker-startup trap as docker-Konclude: HermiT **total wall is
startup-dominated (~1.6 s floor, NOT comparable to native walls)**. The fair
column is HermiT **reasoning-ms**, read from ROBOT `-vv` timestamps (`Starting
reasoning…` → `Reasoning took…`); spot-validated by this author (galen 1,248 ms /
4.0 s total). Startup floor ≈ 1.6 s (bibtex/anch trivial rows).

| Ontology | #cls | rustdl wall (complete?) | Konclude reason-ms | **HermiT reason-ms** | note |
|---|---:|---|---:|---:|---|
| galen | 2748 | 0.59 s (Horn, **complete**) | 12 | **1144** | **rustdl complete < HermiT reasoning** |
| notgalen | 3087 | 1.05 s (Horn, **complete**) | 17 | **1306** | **rustdl complete ≈ HermiT reasoning** |
| ro / ro-stripped | 58 | 0.5 s (**complete**) | 2 | **DNF >300 s** | HermiT stuck in RBox/object-property precompute; rustdl+Konclude trivial |
| go-basic | 51937 | 18.4 s (EL, complete) | 295 | 4380 | |
| alehif-test | 167 | 0.16 s (Horn, complete) | 1 | 233 | |
| shoiq-knowledge | 144 | 1.13 s (complete) | 3 | 8556 | HermiT ~2850× Konclude (nominal/card) |
| ore-10908 | 692 | 5.43 s (complete) | 23 | 10345 | HermiT ~450× Konclude |
| ore-15672 | 82 | 29 s (**INCOMPLETE**) | 5 | 1654 | HermiT completes; rustdl truncates |
| sio / sio-stripped | 1585 | 32 s (**INCOMPLETE**) | 55–59 | ~57000 | HermiT ~1000× Konclude |
| wine | 137 | **DNF**@200ms | 33 | 6390 | **HermiT completes; rustdl DNFs** |
| family / family-stripped | 58 | 81 s (not-comparable) | — | 9344–9450 | **INCONSISTENT** (HermiT agrees Konclude) |
| ore-15516 | 84 | 0.41 s (all-unsat mirror) | — | 58 | INCONSISTENT (all three agree) |

### Honest three-way reading

- **Konclude is the speed king — over *both* rustdl and HermiT.** On reasoning-ms
  Konclude beats HermiT by 1–3 orders of magnitude on real work (ore-10908 ~450×,
  sio ~990×, shoiq-knowledge ~2850×, galen ~95×). This matches HermiT's known
  ORE-competition profile: robust + complete, but slow. Konclude's saturation
  architecture is simply far faster.
- **rustdl is genuinely competitive-to-BETTER than HermiT on the EL/Horn path.**
  rustdl classifies **galen complete in 0.59 s vs HermiT's 1.25 s core reasoning**,
  **notgalen 1.05 s vs 1.31 s**, and — most strikingly — **ro/ro-stripped complete
  in 0.5 s where HermiT DNFs (>300 s, RBox precompute)**. On GALEN-scale Horn and
  RBox-heavy EL, rustdl's saturation fast path *outperforms a mature complete
  reasoner*. This is rustdl's real, defensible niche.
- **rustdl loses to HermiT on hard SROIQ** — it **DNFs wine** (HermiT: 6.4 s),
  and its ore-15672/sio numbers are **incomplete** (truncated) where HermiT
  completes. rustdl's apparent speed there is an artifact of defaulting timed-out
  pairs to not-subsumed, not capability.
- **Correctness: no HermiT-vs-Konclude disagreement** (HermiT built the oracle):
  family, family-stripped, ore-15516 all inconsistent on both; np/pizza incoherent
  on both. rustdl agrees except family* (drops data axioms → reports consistent).

### Net positioning (three-way)
**Konclude ≫ { rustdl-on-EL/Horn ≈ HermiT-core, rustdl better on RO } ≫
rustdl-on-hard-SROIQ < HermiT.** rustdl is not a Konclude-class speed reasoner, but
on EL/Horn it is competitive with — and on RO faster than — HermiT, while staying
sound (FP=0/MISSED=0). Its gap is purely the out-of-EL SROIQ tableau.

**Doc correction:** `docs/abox-consistency-check-handoff.md` cites family as
"HermiT/Konclude-inconsistent in <1 s"; HermiT actually takes **~9.4 s** to detect
it (Konclude 0.9 s wall). Worth fixing if that <1 s figure is relied on.

## ELK baseline (EL specialist) — added for the paper (C4)

**ELK** via ROBOT v1.9.6 (`robot reason --reasoner ELK`), reasoning-ms from `-vv`
timestamps (same ~1.6 s startup floor as HermiT). EL-only reasoner.

| Ontology | ELK reason | rustdl (complete?) | Konclude reason | HermiT reason | note |
|---|---:|---|---:|---:|---|
| bibtex | 165 ms | 0.01 s (EL, complete) | 0 | — | |
| sulo / sulo-stripped | 353 / 354 ms | 0.02–0.03 s | 2 | 74 / 77 | |
| alehif | 468 ms | 0.16 s (Horn, complete) | 1 | 233 | |
| galen | 847 ms | **0.59 s (Horn, complete)** | 12 | 1144 | rustdl ≈/< ELK *if* ELK complete (closure-diff TODO) |
| notgalen | 1022 ms | 1.05 s (Horn, complete) | 17 | 1306 | ~tie |
| ro / ro-stripped | 830 / 862 ms | 0.5 s (complete) | 2 | **DNF** | rustdl+ELK fast; HermiT DNFs |
| go-basic | **2154 ms** | 18.4 s (EL, complete) | 295 | 4380 | **ELK wins big** (~8.5× rustdl); Konclude still ~7× ELK |
| pizza | **rc=1 REJECT** | 2.07 s | 15 | 268 | ELK hard-fails on out-of-EL |
| wine / sio | 427 / 822 ms (**EL-fragment only, INCOMPLETE**) | DNF / 32 s | 33 / 59 | 6390 / ~57000 | ELK silently drops non-EL axioms |

**Findings (honest):**
- **Konclude beats even ELK on big EL** (go-basic 295 ms vs 2154 ms, ~7×) — it is
  in its own tier above all three others.
- **rustdl is competitive with ELK on mid-size Horn** (galen, notgalen) and *faster
  than HermiT* there; **ELK wins on large pure-EL** (go-basic ~8.5×). So C4 = "rustdl
  competitive with the mature reasoners on mid-size EL/Horn," NOT "beats the EL
  specialist."
- **Graceful-degradation point (supports the contract thesis):** ELK *hard-rejects*
  pizza and *silently drops* wine/sio's non-EL axioms (incomplete, no signal),
  whereas rustdl attempts them and returns a sound under-approximation *with* an
  explicit incompleteness flag. The EL specialist either errors or under-reports
  without telling you; rustdl degrades soundly and self-aware.
- **TODO before the paper claims C4 complete-vs-complete:** transitive-closure diff
  of ELK's output vs the oracle on the EL/Horn subset (raw SubClassOf counts are
  inconclusive — ELK 9116 vs oracle 6480 on galen is serialization, not
  incompleteness). Also: clean same-host re-timing of all four.

## Correction: "INCOMPLETE" labels were the signal, not actual MISSED

The `INCOMPLETE (N timed-out)` notes in the tables above are the *conservative
signal* (`timed_out_pairs>0`), **not** measured incompleteness. The anytime sweep
(2026-06-08) checked actual MISSED vs oracle: **ore-15672 and sio are MISSED=0
(complete) at 25/100/1000 ms** — their timed-out pairs are non-subsumptions that
default correctly. Only **pizza @25 ms** is genuinely incomplete (MISSED=4,
recovered by 100 ms). So the signal **over-warns**: `complete=true ⟹ MISSED=0`
holds, but `complete=false` includes actually-complete ontologies. FP=0 held at
every budget on every ontology (soundness floor). See `paper-claims-2026-06-08.md`
§8.

## Note on stale claims
The 06-03/06-04 docs and any "beats Konclude"/"≤5×"/"Konclude ratio N×" figures
derived from **docker** walls overstate rustdl by the ~1.5 s docker startup in the
denominator. The cross-doc-stable comparison is Konclude **reasoning-ms**. Prefer
this doc's native walls for end-to-end and reasoning-ms for engine-vs-engine.
