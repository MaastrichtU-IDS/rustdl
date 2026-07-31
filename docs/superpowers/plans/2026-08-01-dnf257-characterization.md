# Characterizing and Profiling the 257 DNF Ontologies

> **For agentic workers:** REQUIRED SUB-SKILLS: `superpowers:subagent-driven-development` for execution; `superpowers:systematic-debugging` for every root-cause phase; and `owl-reasoner-harness`'s `skills/corpus-measurement` for **every** measurement in this plan. The third is not optional — this plan exists because a previous attempt at the same question was built on a mislabelled roster.

**Goal:** Turn "rustdl fails to classify 257 of 1,920 ORE ontologies" from a count into a **mechanism-partitioned, peer-triaged, code-attributed** account, and fix what is fixable under FP=0.

**Population:** the 257 ontologies that exceed **120 s** single-threaded on rustdl v0.4.6. Derived from `owl-reasoner-harness` `baselines/2026-07-31-ore-rustdl-v046-t1-c30.jsonl` (312 over 30 s) followed by a full 120 s re-run (55 complete, 257 remain). **55 of the original 312 were budget artifacts, not failures** — that distinction is the reason this plan starts where it does.

**Tech Stack:** Rust (edition 2024); `owl-reasoner-harness` for all measurement; Konclude v0.7.0-1138 (native), HermiT 1.4.3 (docker+JVM, 0.56 s floor), KM `c6ced84` (20 GB cap **mandatory**) as peers.

---

## Global Constraints — read before any task

- **Toolchain.** `cargo` is NOT on `PATH`. Prefix everything with
  `export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"` and use `RUSTUP_TOOLCHAIN=stable cargo …`. `owl-dl-py` does not build here — `--exclude owl-dl-py`.
- **Warnings are errors**; clippy `pedantic`; `clippy::doc_markdown` ON (backtick `DKey`/`ABox`/`TBox`/`HyperCache`). This has bitten six tasks.
- **FP=0 is absolute and non-negotiable.** No fix ships that adds a subsumption the Konclude∩HermiT oracle does not have. Every candidate fix is gated by `./scripts/run-soundness-diff.sh` (22/0, closures exact: galen 27997, notgalen 32739, sio 8904, ore-10908 6001, wine 653, pizza 499, alehif 247, ro 158, ore-15672 142, sulo 51, bibtex 16).
- **Measurement discipline is mandatory, not advisory.** Pin binaries per configuration; verify a diagnostic is *in* the binary (`strings <bin> | grep <marker>`) before interpreting its silence; smoke-test any harness on 3 known cases; state exclusions with every population figure; never `cmd | tail` then read `$?`; never `pkill -f <pattern>` matching your own command line; kill by PID file.
- **Never run KM without its 20 GB cap.** Uncapped it reached 237 GB on a 100-class ontology and OOM-killed the host mid-sweep. Use `wrappers/run-km.sh`.
- **Check what else is running before launching anything heavy.** A concurrent 237 GB process contaminated a live sweep in this arc.
- **Payoff-vs-cost is an admissible stop at every phase.** Two engine arcs closed on it. "This cluster is intrinsic, do not fix" is a successful outcome, not a failure.

---

## Strategy: peer triage first, because it partitions the work

The single highest-value cut is **not** rustdl-internal. It is:

> Does a peer reasoner classify this ontology?

- **rustdl DNF + a peer succeeds** ⇒ an *algorithmic gap in rustdl*. Tractable, high value, and the peer's wall gives a target.
- **rustdl DNF + no peer succeeds** ⇒ *intrinsic hardness*. Low value; do not spend engine work.

Everything else (phase attribution, clustering, code review) is scoped to the gap set. Running that triage before any profiling avoids the mistake this arc already made twice: investigating a cluster because it was the one in front of me rather than because it was the largest tractable one.

---

## Phase 0 — Harness extensions (blocking; do first)

Three gaps hit during the previous sweep. All are small and all remove a manual workaround.

**Task 0.1 — `run --only <listfile>`.** The 312 re-run needed a directory of symlinks because `run` only accepts `--corpus <dir>`. Add `--only` taking a file of ontology stems.

**Task 0.2 — `report` must refuse a `dnf` count without printing the cap.** Same rule already applied to RSS-without-thread-pin. This plan's entire premise was a `dnf` count read without its cap; the tool should make that impossible.

**Task 0.3 — the cross-reasoner output normaliser.** Four formats: Konclude OWL/XML hierarchy, HermiT `SubClassOf( <iri> <iri> )` lines, KM JSON keyed by class, rustdl `direct<TAB>sub<TAB>sup`. Normalise each to a sorted `sub<TAB>sup` set so `compare` can compute FP/MISSED across reasoners, not just wall/RSS.
- **KM must have `Q_*` Tseitin definers filtered** or it reports spurious subsumptions.
- Reuse `/data/dumontier/ore-run/work/diff.py` if it still applies rather than rewriting.
- **Gate:** on the curated corpus, normalised Konclude output must reproduce the committed oracle closures exactly (galen 27997 etc.). If it does not, the normaliser is wrong and every downstream FP/MISSED number would be garbage.

---

## Phase 1 — Peer triage of all 257 (parallel, delegate)

**Task 1.1 — run each peer over the 257.** Cap 120 s, single-thread, pinned binaries, one harness run per reasoner so each carries its own provenance header.

| reasoner | note |
|---|---|
| Konclude | native; the primary signal |
| HermiT | subtract/state the 0.56 s floor; JVM RSS has a ~240 MB baseline |
| KM | 20 GB cap mandatory; expect many aborts — an abort is `ErrCrash`, **not** `Dnf`, and must stay distinct |

**Task 1.2 — build the triage table.** Per ontology: rustdl outcome, each peer's outcome + wall + RSS. Partition:

- **Set A — gap:** ≥1 peer completes. *The work set.*
- **Set B — intrinsic:** no peer completes. Record and stop.
- **Set C — peer-disagreement:** peers disagree with each other (a peer FP/MISSED signal, e.g. KM's known unsoundness). Interesting for the FP=0 story, not for perf.

**Report |A|, |B|, |C| with the cap and exclusions stated.** If |A| is small (<20), say so plainly — that bounds this entire plan's value and is a legitimate reason to stop after Phase 2.

**Delegation:** 3 parallel agents, one per reasoner, each producing a harness run file. Independent, no shared state. A 4th agent builds the triage table once all three land.

---

## Phase 2 — Phase attribution and clustering on Set A (parallel, delegate)

For each Set A ontology, attribute *where* rustdl spends itself. No new instrumentation needed — these exist:

| probe | what it isolates |
|---|---|
| `tbox-stats` | conversion only (wall, `concept_rules`) |
| `classify --saturation-only` | saturation without tableau |
| `RUSTDL_TRACE_RSS=1` | phase markers: `entry` → `after_saturate` → `before_prepared` → `after_prepared` → `after_label_cache`; **the last marker emitted localises the stall** |
| `# wall breakdown ms:` banner | `label_cache_build` / `snapshot_cache_build` / `tier_walk` — **only prints on completion, useless on a DNF** |
| `# mode:` / `# fragment:` | pure-EL / Horn / hybrid dispatch |
| `RUSTDL_DATA_PROPERTIES=0`, `RUSTDL_LABEL_HEURISTIC=0`, `--pair-timeout-ms N` | channel and phase ablations |

**Structural profile per ontology:** classes, `SubClassOf`, `EquivalentClasses`, ∃/∀, cardinality (`Object/Data Max/Min/Exact`), functional/inverse-functional, nominals (`ObjectOneOf`/`ObjectHasValue`), role hierarchy depth, chains, transitive/symmetric, ABox size, `DataPropertyAssertion`, distinct literals, input bytes, and `concept_rules` vs classes ratio.

**Then cluster by (last-phase-reached × structural signature), not by intuition.** Report cluster sizes. Known signatures from this arc, as priors to confirm or refute — **not** to assume:
- *conversion-bound + data-heavy* (the `16632` shape: 11 classes, 17k data assertions, 6.6 M axioms)
- *saturator-resident + many classes* (the `11085` shape: 22.6 k classes, geometric doubling to ≥21.7 GB, **cause still unidentified — D4's eager-matrix and Phase-2a hypotheses both refuted**)
- *label-cache-build-bound* / *tier-walk-bound* — **note the A/B taxonomy that previously split these was falsified**: it was an artifact of which budget each phase honours. Re-derive, don't inherit.
- *disjunctive-search-bound* (the wine shape; repeatedly NO-GO'd)

**Delegation:** chunk Set A across N agents (~20–30 ontologies each), each returning a per-ontology attribution row. Clustering is a single agent over the pooled rows. **Constraint: agents must run measurements sequentially within their chunk and must not launch anything uncapped** — concurrency inflates walls and one uncapped process already corrupted a sweep.

---

## Phase 3 — Per-cluster root cause + targeted code review (parallel, delegate)

One agent per cluster, each running `superpowers:systematic-debugging` to completion. **The Iron Law applies: no fix proposed before root cause.**

Each agent delivers:
1. **Component-boundary isolation** narrowing the cost to one function/loop.
2. **A direct count**, never an arithmetic coincidence. (In this arc, `17,415 assertions → ~303 M calls ≈ 12 s` matched a measured 12.36 s and was still wrong: keys were deduplicated, real cost 6%.)
3. **A code-level finding** in one of these categories, with `file:line`:
   - **inefficient** — e.g. an unbounded O(k²) walk (`seed_bucket` has no component bound, `convert.rs`), a per-call clone that should be amortized (`self.clauses.clone()` per class, `lib.rs:2900`; v0.3.39 fixed the per-*pair* sibling but not this one), a rebuild-per-iteration
   - **incorrect** — a gate that certifies completeness while the engine drops the axiom (the "D10 class"; three instances shipped this year)
   - **missing** — an algorithm absent entirely (e.g. arithmetic data-cardinality counting), or present but not wired to a path
4. **A falsifiable prediction** for the fix's effect, stated *before* implementing.

**Mandatory code-review sweep, delegated per subsystem** (independent, parallelisable), looking specifically for the three categories above:

| agent | subsystem | known starting points |
|---|---|---|
| R1 | `owl-dl-core/convert.rs` + `data_axioms.rs` | `seed_bucket` unbounded O(k²); DKey channel; `dkey_components` |
| R2 | `owl-dl-saturation` | the `11085` doubling container; dense per-class structures |
| R3 | `owl-dl-tableau/hyper.rs` | `match_body`/`enumerate_matches` fire loop (~25% self-time, known); `solve` deadline granularity (`FIXPOINT_ITERS = 100_000` uninterrupted — structurally true, but an in-loop check measured as **no** improvement) |
| R4 | `owl-dl-reasoner` classify/label-cache | per-class `classify_labels` clone; `~10–27×` per-class budget overrun measured, ~94% unattributed |

Each R-agent must **verify any claim by measurement**, and is explicitly told that three prior "obvious" hotspots in this area (axiom volume, deadline enforcement, the clause clone) were each refuted.

---

## Phase 4 — FP=0-gated fix assessment, with and without (per fix)

Every candidate fix follows the same pipeline. This is where the harness earns itself.

1. **Report-only first** where the logic is subtle: implement the *decision* so it only counts, validate it, then let it act. Verify report-only is inert (byte-identical output). This arc's collapse/broadcast work predicted 9,515 axioms in report-only mode and the acting implementation produced exactly 9,515 — far stronger evidence than any test.
2. **Flag it, default OFF.** One env flag per fix, `=0` reverting.
3. **Canaries before the fix**, negatives-first, and **sabotage them** to prove non-vacuity. A differential test in this arc survived three sabotages of the very property it guarded.
4. **FP=0 net** with the flag ON — 22/0, closures exact.
5. **Flag-OFF byte-identity** on the curated ABox/data fixtures — proves the OFF path is untouched.
6. **`harness compare` ON vs OFF over the affected population**, checking `answer identity` and outcome transitions. Report `RECOVERED` and, critically, any `LOST_BY_ON` — a subtractive fix should be answer-identical.
7. **Report the fraction of the ceiling achieved, never the ceiling.** A flag-OFF/channel-disabled measurement is an *upper bound*; citing it as a result was retracted once in this arc.
8. **Flip the default only if 4–6 pass.** If the fix helps wall/RSS but recovers no ontology, say that in the headline.

**Documented per fix:** mechanism, `file:line`, prediction vs measurement, gate results, per-ontology before/after (wall, RSS, `concept_rules`, outcome), and the honest scope — including how many of the 257 it does *not* help.

---

## Phase 5 — Documentation and interpretation

- **`docs/benchmarks/2026-08-01-dnf257-characterization.md`** — the triage table (A/B/C), cluster sizes, per-cluster mechanism, and what each peer solves that rustdl does not. Raw runs committed to `owl-reasoner-harness/baselines/` (a run whose numbers are cited must be committed — three artifacts were nearly lost to gitignored paths in this arc).
- **Per-cluster root-cause docs**, one each, including refuted hypotheses. Negative results are first-class: this arc's most reusable outputs were "axiom volume is not Bucket B's blocker" and "the A/B taxonomy is falsified".
- **CLAUDE.md** entries for anything shipped, with its measurement trap noted (e.g. "`9347` cannot discriminate this lever").
- **Interpretation section answering:** how much of the 257 is intrinsic vs gap; which subsystem carries the most gap; whether rustdl's remaining weakness is wall or RSS (CLAUDE.md currently says RSS — test it); and what the corpus says about the EL/Horn-vs-SROIQ split.

---

## Parallelisation summary

| phase | agents | independent? | notes |
|---|---|---|---|
| 0 | 3 | yes | harness features; 0.3 gates on reproducing oracle closures |
| 1 | 3 + 1 | yes, then join | one per peer reasoner; then triage table |
| 2 | N (~10) + 1 | yes, then join | chunked attribution; then clustering |
| 3 | per-cluster + 4 R-agents | yes | root cause; subsystem code review |
| 4 | per-fix, sequential per fix | no | gates must run on a quiet host |
| 5 | 1 | — | synthesis |

**Host discipline:** Phases 1–2 may run agents concurrently **only** with per-ontology single-thread pins and no uncapped process. Phase 4 gate measurements must run on a quiet host — a concurrent 237 GB allocation already corrupted one sweep in this arc.

---

## Stopping rules (operational, not decorative)

The previous plan in this arc had a stopping rule that *passed* while the design was aimed at inert axioms. These bind to outcomes:

- **After Phase 1:** if |Set A| < 20, stop and report. The population is mostly intrinsic and engine work is not justified.
- **After Phase 2:** if no cluster exceeds 10 ontologies, prefer the *cheapest* cluster, not the most interesting one.
- **Per fix, before implementing:** demonstrate the targeted work is *consumed* — delete/disable it and show an **answer changes**. If output is byte-identical, the work is inert and the fix is pointless. (This exact test would have redirected the rejected data-cardinality plan.)
- **Per fix, after implementing:** if `compare` shows 0 outcome transitions and no wall/RSS change beyond noise, revert and record it as measured-out.

## What this plan does not assume

- That the 257 share a cause. Evidence so far says ≥2 mechanisms and one falsified taxonomy.
- That RSS is the dominant axis. CLAUDE.md says so; Phase 2 must confirm or refute it.
- That peers are correct. KM has documented FP; Konclude has under-reported at least once (`ore_ont_10407`). Adjudicate FP against **Konclude ∪ HermiT**, and treat a lone-peer disagreement as Set C rather than as truth.
