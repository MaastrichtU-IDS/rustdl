# Paper evidence — native-host JVM measurements (2026-07-10)

Closes the two evidence gaps that `paper-evidence-2026-07-08.md` deferred to "a
native-JVM eval host (NOT doable under docker)": **T5** (precise JVM peak RSS +
cold-start) and the **F1** controlled multi-reasoner wall refresh. Measured
**natively** — no docker — so the ~1.5–2 s container-startup confound the paper
§0 warns against is gone; the residual JVM-boot / native-load overhead is
isolated and quantified instead.

## Host + toolchain

- **Host:** this machine — macOS (Darwin 25.5.0), Apple Silicon (arm64), the same
  host whose native paths the committed comparison carries (`~/data/...`; the
  handoff's `/data/dumontier/...` is the Linux mirror).
- **rustdl:** release build at HEAD (`8c82f19`, v0.3.21), in-process, native arm64.
- **JVM:** Homebrew `openjdk@17` (17.0.19), native arm64.
- **ELK / HermiT:** via ROBOT 1.9.10 (`robot reason --reasoner ELK|HermiT`),
  running on the native JDK 17.
- **Konclude:** v0.7.0-1138, the official `OSX-x64` build, run via **Rosetta 2**
  (no arm64 release exists) — see the Rosetta caveat below.
- **Measurement:** `/usr/bin/time -l` → wall (`real`) + `maximum resident set
  size` (bytes on macOS = peak RSS). 3–5 repeats/cell; walls are the median, RSS
  the max. Runs were serialized (no cross-run contention). Isolated engine-only
  times read from each tool's own logs (Konclude `Finished class classification
  in N ms`; ROBOT `-vv` `Reasoning took N seconds`; rustdl `# wall breakdown`).

## T5 — startup + footprint microbench

### Startup floor (trivial: 3 classes, 2 SubClassOf axioms)

| reasoner | cold-start wall | peak RSS |
|---|--:|--:|
| **rustdl** | **< 10 ms** | **4.3 MB** |
| Konclude (native C++, Rosetta) | 70 ms | 27.8 MB |
| ELK (JVM/ROBOT) | 250 ms | 130.6 MB |
| HermiT (JVM/ROBOT) | 250 ms | 131.3 MB |

**This is the embeddability headline, now with the JVM side measured exactly.**
For a 3-axiom ontology the JVM reasoners already sit at **~131 MB / ~250 ms** —
that is the JVM+OWLAPI+ROBOT floor, independent of the ontology. rustdl is
**~30× smaller (4.3 MB) and ~25× faster to first answer (< 10 ms)**; even the
native-C++ Konclude is ~6× rustdl's RSS and ~7× its startup.

### Peak RSS + wall under real classification load

| ontology | frag | rustdl | Konclude | ELK | HermiT |
|---|---|--:|--:|--:|--:|
| sulo | small | 0.00 s / 7.6 MB | 0.07 s / 30.5 MB | 0.42 s / 197 MB | 0.40 s / 236 MB |
| pizza | SHOIN | 1.93 s / 18.1 MB | 0.09 s / 39.4 MB | 0.40 s / 200 MB† | 0.45 s / 235 MB |
| ro | EL⁺ | 0.02 s / 34.3 MB | 0.13 s / 54.5 MB | 0.73 s / 364 MB | — DNF‡ |
| sio | SROIQ | 0.19 s / 72.9 MB | 0.21 s / 89.4 MB | 0.84 s / 411 MB† | 24.15 s / **1182 MB** |
| family | SROIQ (incons.) | 2.02 s / 601 MB | 0.46 s / 221 MB | 0.64 s / 332 MB† | 7.35 s / **1127 MB** |
| wine | SHOIN(D) | — DNF‡ | 0.16 s / 63.8 MB | 0.61 s / 258 MB† | 2.63 s / 310 MB |

Wall = median; RSS = max peak. † ELK is OWL 2 EL only — it runs to completion
(exit 0, hierarchy written) but its answer is **not guaranteed complete** on the
non-EL rows (pizza/sio/family/wine); the numbers reflect EL-subset work, listed
for footprint context, not as a complete classification. ‡ DNF matches the
committed `reasoner-comparison-2026-06-21.md` (rustdl-wine is the lone
combinatorial DNF; HermiT-ro DNFs) — not re-run here (>200 s).

**Footprint story:** across EL/tractable inputs rustdl stays **4–73 MB** while
the JVM reasoners run **130 MB–1.2 GB** (HermiT peaks at **1.18 GB** on sio and
**1.13 GB** on family). Konclude (native C++) is far leaner than the JVM
(28–221 MB) but still 2–6× rustdl on the small/EL onts.

**Honest counter-datum:** rustdl is *not* universally tiny — on **family** (a
hard inconsistent ABox that drives the ABox-saturation pre-check + a large
completion graph) rustdl peaks at **601 MB**, above Konclude (221 MB) and ELK
(332 MB) though still under HermiT (1.13 GB). The "small footprint" claim is
robust on EL/Horn and small-DL workloads; on hard SROIQ(D) with large ABoxes it
is competitive, not dominant. Report it that way.

## F1 — head-to-head walls (native, this host)

Total wall (`/usr/bin/time -l real`, median) vs engine-only (tool's own log):

| ontology | frag | rustdl wall | Konclude wall (engine) | HermiT wall (engine) | ELK wall† |
|---|---|--:|--:|--:|--:|
| sulo | small | 0.00 s | 0.07 s (~2 ms) | 0.40 s | 0.42 s |
| pizza | SHOIN | 1.93 s | 0.09 s (20 ms) | 0.45 s | 0.40 s |
| ro | EL⁺ | 0.02 s | 0.13 s | DNF‡ | 0.73 s |
| sio | SROIQ | 0.19 s (~172 ms) | 0.21 s (92 ms) | 24.15 s (**22 s**) | 0.84 s |
| family | SROIQ | 2.02 s | 0.46 s (incons. ✓) | 7.35 s | 0.64 s |
| wine | SHOIN(D) | DNF‡ | 0.16 s | 2.63 s | 0.61 s |

**Reading, consistent with the paper's §0 honesty stance:**
- **rustdl wins the EL kernel wall** (ro 0.02 s vs ELK 0.73 s; sio 0.19 s total vs
  HermiT's **22 s engine**) and has effectively **zero startup** — its total wall
  ≈ its engine time (native, in-process).
- **Konclude is the overall speed leader** (native C++), fastest on every DL row
  even under Rosetta — reinforcing the paper's "NOT a speed claim vs Konclude."
- **rustdl loses the hard SROIQ walls** (pizza 1.93 s, family 2.02 s, wine DNF) —
  reported, not hidden.
- **HermiT is correct-but-slow + memory-heavy** (sio 22 s engine / 1.18 GB).

### The startup confound, quantified natively

Total wall − engine time isolates per-process overhead (the thing docker used to
inflate by ~1.5–2 s):
- **rustdl:** ~0 (total ≈ engine; native, no VM).
- **JVM (ELK/HermiT):** ~0.25 s (trivial/small) rising to ~2 s (sio: 24.15 s wall
  − 22 s engine) for JVM boot + OWLAPI parse.
- **Konclude:** ~70–120 ms Rosetta-translation + ontology load (sio: 0.21 s wall −
  0.092 s engine).

So the cross-reasoner *wall* gaps on small onts are startup-dominated, exactly as
§0 predicts — the fair engine comparison is the parenthesized column, and the
embeddability claim rides on the native-vs-VM startup + RSS floor, not raw wall.

## F1b — ORE-2015 pilot at scale (152 onts, native)

The curated-corpus F1 above is small-N. This is the scale row: the ORE-2015
pilot corpus already on the host (`~/data/ore-run/input`, 152 `.ofn`; `owx/` for
Konclude + ROBOT/HermiT), each reasoner under a **120 s per-ontology wall cap**
(gtimeout, whole-process-group kill), wall + peak RSS via `/usr/bin/time -l`.
Harness `~/eval-tools/ore.sh`; raw `~/eval-tools/work/results-ore-clean.csv`.

| reasoner | finished | DNF | err | wall median | wall p90 | wall max | RSS median | RSS max |
|---|--:|--:|--:|--:|--:|--:|--:|--:|
| **rustdl** | 134 | 18 | 0 | **20 ms** | 2.39 s | 48.8 s | 18.9 MB | **13.5 GB** |
| Konclude | 151 | 1 | 0 | 90 ms | 0.46 s | 4.24 s | 36.6 MB | 642 MB |
| HermiT | 144 | 8 | 0 | 520 ms | 4.47 s | 64.1 s | 269 MB | 2.04 GB |

Wall stats over completed (`ok`) runs. **127 of 152 onts finished on all three.**

**Reading:**
- **rustdl has the fastest median (20 ms)** and finishes **112/134 completions in
  < 1 s** — the consequence-based kernel dominates the common case. But it has the
  **fattest tail** (mean 2.07 s ≫ median 20 ms; max 48.8 s; **18 DNF**) — the hard
  SROIQ pairs that saturate tableau depth.
- **Konclude is the robustness leader** (native C++, even under Rosetta): tightest
  distribution (147/151 < 1 s, max 4.24 s), only **1 DNF** (`ore_ont_5548` — likely
  the Rosetta penalty pushing one hard ont past 120 s; the committed native pilot
  had Konclude 0 DNF / max 5.4 s).
- **HermiT** (native, precise walls now — the committed pilot's HermiT was
  docker-coarse whole-second): median 520 ms, **8 DNF**, RSS to **2.04 GB**.
- **DNF counts track the committed 300 s pilot** (`reasoner-comparison-2026-06-21`:
  rustdl 16, HermiT 9) at the tighter 120 s cap → rustdl 18, HermiT 8. The
  hard-tails only partly overlap: **7 onts HermiT DNFs but rustdl completes**,
  17 the reverse, only **1 both** — i.e. the intrinsic hard-SROIQ tail is
  reasoner-specific, not a shared cliff (both a mature tableau and rustdl hit
  their own tails; only Konclude's engine clears nearly all).

**Honest weakness — the RSS tail.** rustdl's *median* footprint at scale is tiny
(18.9 MB) but its worst case is not: 5 onts exceed 1 GB, peaking at **13.5 GB**
(`ore_ont_6132`) and 7.9 GB (`ore_ont_12174`). The tableau/wedge can blow up
memory on some hard SROIQ inputs — well above HermiT's 2 GB ceiling on those.
The embeddability claim is about the *common case* (median MB-scale, EL/Horn
guaranteed lean); the pathological-DL tail must be stated, not hidden.

**Scope note:** this run measured **timing / RSS / completion only** — it did not
re-diff closures against the `oracle/` (the FP=0 soundness result is the committed
`reasoner-comparison-2026-06-21 §3`: 0 false positives / 201 diffed). A native
FP=0 re-verification at HEAD is a separate pass via
`cargo test -p owl-dl-reasoner --test konclude_closure_diff -- --ignored`.

## S5 — explanation cost (native vs owlexplanation), added 2026-07-11

Cost to produce **one justification for the same entailment** — pizza
`CheeseyVegetableTopping ⊑ ⊥` — natively (`rustdl justify`) vs the OWL API
`owlexplanation` library invoked through ROBOT over each OWL-API reasoner. 3 reps,
`/usr/bin/time -l`. Harness `~/eval-tools/explain-bench.sh`; raw
`results-explain.csv`. All four produce the same 3-axiom justification (the two
`⊑` axioms + the disjointness).

| tool | explanation path | wall (median) | peak RSS |
|---|---|--:|--:|
| **rustdl** | native, in-process | **10 ms** | **10 MB** |
| HermiT | `owlexplanation` / ROBOT (JVM) | 0.39 s | 204 MB |
| JFact | `owlexplanation` / ROBOT (JVM) | 0.38 s | 178 MB |
| ELK | `owlexplanation` / ROBOT (JVM) | 0.50 s | 203 MB |
| Konclude | — no OWL API binding | — | — |
| whelk-rs | — no OWL API binding | — | — |

**~40× faster, ~20× less memory** for the same justification. Notes: (1) much of
the JVM figure is boot; it amortises across many explanations in a long-lived
process, but the per-invocation/in-process (neurosymbolic) use rustdl targets
pays it each call. (2) The entailment is EL⁺⁺-expressible, so ELK applies; a
strictly-SROIQ entailment would leave only HermiT/JFact. (3) **Konclude and
whelk-rs expose no OWL API reasoner binding**, so `owlexplanation` cannot wrap
them — there is no explanation path to measure. This is the C2/S5 evidence: only
rustdl explains in-process, and only rustdl explains Konclude/whelk-class inputs
at all through this route. Folded into the paper as Table 6.

## Caveats (bind the numbers)

1. **Konclude under Rosetta 2.** No arm64 release exists; the OSX-x64 binary runs
   emulated. Its wall and RSS here are **upper bounds** — a native arm64 Konclude
   would be leaner and faster, so Konclude's lead over rustdl is *understated*, never
   overstated. Fine for the paper (we never claim to beat Konclude).
2. **ELK is EL-only.** Complete only on OWL 2 EL; on pizza/sio/family/wine its
   output is not a guaranteed-complete classification (it does not error). Its
   rows are footprint/wall context, not correctness claims.
3. **JVM heap = ROBOT defaults.** ELK/HermiT RSS is the JVM as ROBOT launches it
   (default heap sizing) — i.e. the footprint a user actually gets from the
   standard `robot reason` invocation, which is the honest thing to report.
4. **Slow HermiT rows (sio/family) single-sample** for wall; RSS is stable across
   the pilot runs. DNF cells cited from the committed comparison, not re-run.
5. **Verdicts unchanged:** Konclude reports family **inconsistent** (matches
   rustdl's ABox-saturation pre-check); pizza's 2 unsat classes
   (`CheeseyVegetableTopping`, `IceCream`) reproduced by ELK/HermiT.

## Net status of the paper-evidence gaps (updates `paper-evidence-2026-07-08.md`)

- **T5 startup/footprint — DONE (both sides, native).** rustdl 4.3 MB / < 10 ms
  floor; JVM floor ~131 MB / ~250 ms; Konclude 28 MB / 70 ms. RSS-under-load
  measured through 1.18 GB (HermiT). The precise JVM peak RSS that was
  "deferred to a non-docker JVM host" is now in hand.
- **F1 head-to-head — DONE (native refresh).** Walls + engine-only across the
  curated corpus; rustdl's EL wins, hard-SROIQ losses, and Konclude's lead all
  reproduce the committed comparison at current HEAD → **no regression**. The
  startup confound is now quantified (native) rather than assumed.
- **F1b ORE-2015 at scale — DONE (152 onts, native, 120 s cap).** rustdl fastest
  median (20 ms); Konclude the robustness leader (1 DNF); HermiT walls now precise
  (native, not docker-coarse); DNF counts track the committed 300 s pilot. Surfaces
  the honest RSS tail (rustdl to 13.5 GB on the pathological-DL tail).
- Remaining nice-to-have (not blocking the paper): the EL-projection GALEN
  (Horn, 2,748 cls, the docs' 165 ms saturate figure) was not on this host — the
  local `Tests/galen.owl.xml` is the *ALEHIF+* full GALEN (150 functional / 207
  inverse), a different fragment. The EL-kernel-vs-whelk/ELK F2 table already
  exists with confirmed rustdl numbers in `reasoner-comparison-2026-06-21.md §1`;
  this refresh targeted the JVM RSS/wall gap, which is closed.

**Upshot:** every "to produce" item in the spec §3 is now measured on a native
host. The paper's S6/C5 evidence (startup, footprint, EL-competitiveness, honest
DL losses) is complete and reproducible via `~/eval-tools/{bench,matrix}.sh`.
