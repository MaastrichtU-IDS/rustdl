# Konclude-parity + proofs — implementation plan

> **For agentic workers:** dispatch one subagent per task. Each task is gated by the
> **standing gates** (below) — FP=0/MISSED=0 is SACRED. Parallel tasks run in isolated
> git worktrees off `main` to avoid edit conflicts; integrate + re-gate at merge.

**Goal (headline, advisor-corrected):** the defensible, differentiated product is
**DL proofs + ELK/HermiT-competitive EL/Horn + sound SROIQ degradation + embeddable
Rust** — it does NOT require beating Konclude. "Match Konclude" is a *stretch*, not the
success criterion (see Track A). rustdl on EL/Horn is currently ELK/HermiT-class (galen
2.2× / notgalen 3.7× behind Konclude; ~8.5× behind ELK on go-basic), so there is real
*algorithmic* headroom (we trail even the EL specialist) — but leapfrogging to
Konclude's tier likely needs Konclude's saturation techniques, not allocation polish.
**0.2 is the go/no-go** that tells us which.

**Architecture:** three workstreams. **A (EL/Horn perf)** — close the 2–4× constant-factor
gap on the saturation fast path (the achievable "match Konclude" target). **B (proofs)** —
opt-in inference recording in the production saturator → step-level proofs for the
saturation-covered fragment + shipped axiom-level justifications elsewhere (the
differentiator). **C (SROIQ structural)** — anywhere-blocking + sound label-set
(un)sat caching in the tableau (the hard, longer parity bet). The retired CB engine
(`owl-dl-cb`, branch `feat/cb-b1-integration`) is NOT part of this; work is on `main`.

**Tech stack:** Rust (edition 2024), crates `owl-dl-saturation` / `owl-dl-tableau` /
`owl-dl-reasoner` / `owl-dl-cli` / `owl-dl-bench`; native Konclude `konclude/konclude:latest`
(docker) + ROBOT `obolibrary/robot` for `.ofn`→`.owx`; `cargo flamegraph`/`perf`.

---

## Standing gates (EVERY task, before "done")

1. **Soundness (SACRED): FP=0.** Corpus closure-diff vs the Konclude∩HermiT oracle shows
   **no `only_in_rustdl` (false subsumption) and no spurious unsat** on any fixture.
   `scripts/`-style closure-diff over galen, notgalen, sio, wine, ore-10908, ore-15672,
   shoiq-knowledge, alehif, ro, pizza, bibtex. A single FP = STOP + revert.
2. **Completeness non-regression: MISSED=0** on every fixture that was MISSED=0 before the
   change (don't trade completeness for speed).
3. **Verdict-identity:** the reported hierarchy is byte-identical (or closure-identical) to
   pre-change on every corpus fixture, unless the task's explicit purpose is to *recover*
   missed pairs (then: only additions, all oracle-confirmed).
4. **Perf measured, not claimed:** report rustdl wall (1T + parallel) + Konclude wall +
   reasoning-ms on the affected fixtures, before/after. Re-measure with the harness; don't
   trust stale numbers.
5. **Hygiene:** `cargo test --workspace`, `cargo fmt --all -- --check`,
   `cargo clippy --workspace --all-targets --all-features -- -D warnings` all clean.
6. Default behaviour unchanged unless the task says otherwise (new features behind flags).

---

## Phase 0 — Baseline, gate harness, attribution (PREREQ, sequential, do first)

### Task 0.1 — Standing benchmark + closure-diff harness
**Files:** `scripts/bench-konclude-parity.sh` (create or extend `bench-rustdl-modes.sh`),
`scripts/closure-diff.sh` (extend `cmp_konclude.py`).
- [ ] Script: for each corpus fixture, run rustdl `classify` (1T via `RAYON_NUM_THREADS=1`
  and parallel), native Konclude (docker), emit a table: rustdl wall | Konclude wall |
  reasoning-ms | ratio | rustdl frag | MISSED | FP. Reuse oracles in `ontologies/external/*-classified.owx`.
- [ ] Closure-diff: transitive-closure compare rustdl hierarchy vs oracle → `only_in_rustdl`
  (FP, must be 0), `only_in_oracle` (MISSED). This is gate #1/#2 mechanized.
- [ ] Commit the harness + a `docs/perf-baseline-2026-06-16.md` snapshot table (seed from
  `docs/perf-2026-06-08-konclude-vs-rustdl.md`, re-measured this host).
**Gate:** **FIRST validate the Konclude+ROBOT pipeline itself** — prove it runs end-to-end
and its output verdicts MATCH the existing oracles (`*-classified.owx`) on ≥3 fixtures incl.
the 14 ROBOT-converted ones (class-count cross-check), before trusting any harness number
(the whole FP=0/perf gate rests on this pipeline). Then: harness reproduces the known gap
table; closure-diff shows FP=0 on all fixtures.

### Task 0.2 — Flamegraph attribution (EL/Horn) — drives Track A
**Files:** `docs/flamegraphs/` (new svgs), append to `docs/perf-baseline-2026-06-16.md`.
- [ ] `cargo flamegraph` rustdl `classify` on galen, notgalen, go-basic (the complete EL/Horn
  path, where rustdl is 2.2–4.5× and parity is realistic). Attribute %: parse/convert,
  saturation loop (which rules — CR5/role-chain/told?), interning, output serialization,
  fixed setup. Identify the top 3 constant-factor costs.
**Gate:** a ranked attribution table; the top-3 EL/Horn hot frames named (file:line, %).

### Task 0.3 — Flamegraph + memory attribution (SROIQ) — drives Track C
**Files:** `docs/flamegraphs/`, `docs/perf-baseline-2026-06-16.md`.
- [ ] Flamegraph + peak-RSS rustdl on sio and ore-10908 (and wine@25ms). Attribute the
  tableau cost: completion-graph build/blocking (`graph.rs` `is_blocked`/merge/clash),
  per-pair orchestrator loop (`reasoner`), wedge. Confirm the huge-graph hypothesis
  (sio 800 MB) and quantify repeated-subproblem cost (how many pairs re-solve overlapping
  label sets — instrument a counter).
**Gate:** attribution table; verdict on whether anywhere-blocking (graph size) vs caching
(repeat-solves) is the bigger SROIQ lever, with numbers.

---

## Track A — EL/Horn perf (target: ELK/HermiT-class; Konclude-class = stretch)

> Depends on 0.2 (which is the GO/NO-GO). Worktree `wt-perf-elhorn`. Crate:
> `owl-dl-saturation` (+ `reasoner` glue). **Success criterion = close the gap to
> ELK/HermiT-class on EL/Horn (evidence-backed achievable), with Konclude-class as a
> stretch.** If 0.2 shows the residual is irreducible Rust-vs-hyper-optimized-C++
> constant factor (not algorithmic), STOP, say so, and reframe the headline to the
> differentiated product (proofs + competitive-not-fastest + embeddable) — do NOT chase
> a mirage with the implementation budget.

### Task A.1 — Cut the top constant-factor cost from 0.2
**Files:** per 0.2 attribution — likely `crates/owl-dl-saturation/src/*.rs` (the fixpoint
loop / index structures) and/or `crates/owl-dl-core/src/convert.rs` (parse/convert), and
output serialization in `owl-dl-cli`.
- [ ] Implement the #1 attributed optimization (e.g. reduce allocations in the saturation
  hot loop / better indexing / avoid redundant re-derivation / faster interning). Exact
  change determined by 0.2 — do NOT guess before the flamegraph.
- [ ] Microbench the changed path; then the standing gates.
**Gate:** galen/notgalen/go-basic wall ↓ (target galen < 0.4 s, toward Konclude 0.27 s);
FP=0/MISSED=0 + closure-identical; full corpus non-regression.

### Task A.2 — Second + third constant-factor cuts (iterate)
- [ ] Repeat A.1 for the #2/#3 hot frames. Re-flamegraph after each to confirm the cost
  moved. Stop when galen is within ~1.3× of Konclude OR the remaining cost is irreducible
  Rust-vs-C++ constant factor (document which).
**Gate:** updated gap table; honest note on the residual (closeable vs inherent).

### Task A.3 — Fixed-overhead audit (parallel-safe small wins)
**Files:** `owl-dl-reasoner` (orchestrator setup), `owl-dl-cli` (parse/print).
- [ ] On small fixtures rustdl rides a large fixed floor vs Konclude's ~30 ms. Profile
  startup/parse/print; trim (lazy structures, avoid building label cache when pure-EL, etc.).
**Gate:** small-fixture walls ↓ toward the 30 ms floor; no change on big fixtures; gates.

---

## Track B — DL proofs (the differentiator)

> Mostly independent of A/C (additive, opt-in). Worktree `wt-proofs`. Crates:
> `owl-dl-saturation` (recording) + `owl-dl-reasoner`/`owl-dl-cli` (extraction/CLI).
> Design carries over from `docs/superpowers/specs/2026-06-16-cb-inference-record-design.md`
> but RE-TARGETED to the production EL saturator (not the retired owl-dl-cb).

### Task B.1 — Inference recording in the production saturator (opt-in, zero-cost-off)
**Files:** `crates/owl-dl-saturation/src/*.rs` (the fixpoint), new `proof.rs`.
- [ ] Add `record_proofs: bool` (env `RUSTDL_PROOF`, default off) + a side-table keyed by a
  stable derived-fact id mapping each derived subsumption/`⊥` to its `(rule, premise_ids)`.
  Record at the single derivation chokepoint for each saturation rule (told, CR5/∃, role
  hierarchy, chain, ⊓, Bot, functional-merge). When off: one bool check, no allocation.
- [ ] Unit test: recording on, the trace for a small EL chain contains the expected rule
  applications.
**Gate:** off-path is verified zero-cost (a perf run with flag off is identical to baseline);
recording observational (cannot change any verdict — re-run corpus FP=0/MISSED=0 unchanged);
gates.

### Task B.2 — Proof extraction + rendering
**Files:** `owl-dl-saturation/src/proof.rs`, `owl-dl-reasoner` (API), reuse the Manchester
writer + justification rendering.
- [ ] `prove(onto, sub, sup) -> Option<Proof>`: backward DAG from the witnessing derived
  fact to ontology-axiom leaves; memoize → DAG; render each step `premises ⊢_rule conclusion`
  via Manchester. For a subsumption NOT derived by the saturator (SROIQ/tableau-only): fall
  back to the shipped black-box **justification** (axiom set) + a clear "step-proof
  unavailable (out-of-saturation-fragment); axiom justification:" note.
- [ ] Faithfulness check: a proof-checker that re-verifies each recorded step is a valid
  rule instance (cheap, run in tests).
**Gate:** smoke tests — `prove` on a by-cases-style EL chain returns a correct multi-step
proof; on an out-of-fragment pair returns the justification fallback; proof-checker passes.

### Task B.3 — `rustdl prove` CLI + docs
**Files:** `crates/owl-dl-cli/src/main.rs`, README.
- [ ] `rustdl prove <onto> <sub> <sup>` prints the proof tree (or justification fallback).
  Behind no flag for output, but recording auto-enabled only for the `prove` subcommand.
**Gate:** CLI works end-to-end on 3 ontologies; default `classify` perf unaffected; gates.

---

## Track C — SROIQ structural (the harder parity bet)

> Depends on 0.3. Worktree `wt-sroiq`. Crate: `owl-dl-tableau` (+ `reasoner`). C.1 before C.2
> (smaller graphs → cleaner cache). This is the multi-month, FP-delicate track.

### Task C.1 — Anywhere blocking: VALIDATE & ENABLE (it already exists — do NOT reimplement)
**ALREADY IMPLEMENTED, default-OFF.** `crates/owl-dl-tableau/src/graph.rs` has the
anywhere-blocking candidate index + `block_index_enabled` (~270–410);
`crates/owl-dl-tableau/src/lib.rs` has the `anywhere_blocking` field +
`RUSTDL_ANYWHERE_BLOCKING` env (default `false`) + `set_anywhere_blocking()`; design at
`docs/superpowers/plans/2026-06-15-anywhere-pairwise-blocking.md`.
- [ ] **FIRST: find out WHY it's default-OFF.** Read the existing impl + the 2026-06-15
  plan doc + git log for that work. The reason it's off almost certainly encodes the real
  problem (a soundness gap for SROIQ inverse/number-restrictions? incomplete? a perf
  regression? unfinished?). Report the finding before touching anything.
- [ ] If OFF for an unfinished/soundness reason: complete/fix the blocking condition
  (correct pairwise/double blocking for inverse + number restrictions). If OFF only for
  caution: validate + enable.
- [ ] Adversarial opus review of the blocking condition's SROIQ soundness before trusting.
**Gate:** with anywhere-blocking ON — FP=0/MISSED=0 corpus-wide (sacred — wrong blocking =
FP or nontermination); sio/wine peak-RSS ↓ ≥5×; sio/ore-10908 wall ↓; wine no longer DNFs
at a usable timeout. Independent adversarial verification. (Net new code likely small — the
machinery exists; the work is the soundness validation + whatever made it unsafe to enable.)

### Task C.2 — Sound label-set (un)sat caching
**Files:** `crates/owl-dl-tableau/src/` (new `satcache.rs`), `reasoner` (wire into the
per-pair loop). **Distinct from the retired model-snapshot cache** (`RUSTDL_SNAPSHOT_CAPTURE`,
default OFF, FP-unsound — DO NOT revive that).
- [ ] Cache **satisfiability FACTS of label sets** (model-independent), dependency-aware
  (record the context — predecessors/nominals/blocking state — under which a cached
  (un)sat result is valid for reuse), reused across the n² per-pair classification. This is
  the sound analog of Konclude's `CReuseCompletionGraphCache` — caches facts, NOT models, so
  it sidesteps the reuse-trap that sank the snapshot cache.
- [ ] **SOUNDNESS GATE (the crux):** a label-set may be cached `unsat` only when proven unsat
  independent of the reuse context; `sat` reuse only where the dependency set is subsumed.
  Adversarial review (opus) + a dedicated FP-hunt (fuzz: random SROIQ, cache-on vs cache-off
  verdict-identity) BEFORE trusting. This is exactly where the snapshot cache emitted 30+ FP.
**Gate:** cache-on verdict-IDENTICAL to cache-off on the full corpus + a 200-ontology SROIQ
fuzz (FP=0, the sacred bar); per-pair tableau wall ↓ on sio/ore/wine; adversarial review passed.

---

## Orchestration / parallelism

- **Sequential prereq:** Phase 0 (0.1 → {0.2, 0.3}). 0.2 unblocks A; 0.3 unblocks C; B is
  independent (needs only 0.1's gate harness).
- **Parallel after Phase 0:** Track C (`wt-sroiq`, `owl-dl-tableau` — different crate) runs
  concurrently with the saturator work. **A BEFORE B in the saturator (advisor-corrected):**
  Track A restructures the saturation fixpoint; Track B attaches `(rule, premises)` recording
  hooks to those same rules. Parallel-editing the hot loop conflicts at merge and B's hooks
  may attach to code A is rewriting. So: **A's loop changes land first, THEN B's recording
  goes onto the stabilized loop.** B's *spec* (read-only, running now) is fine in parallel;
  B's *implementation* waits for A.
- **Integration:** each task merges to `main` only after its gate AND a re-run of the full
  standing-gate harness on the merged tree (catch cross-track regressions). FP=0 re-verified
  at every merge.
- **Review:** the SROIQ-soundness tasks (C.1, C.2) get an independent adversarial opus review
  of the soundness argument + an FP-fuzz before merge — same discipline that caught the CB
  cruxes. Perf/proofs tasks get the standing gates.

## Sequencing recommendation (honest, advisor-corrected)
**HOLD the implementation wave until Phase 0 data lands** — do not pre-launch. The single
highest-value output is **0.2's attribution**: it decides whether Track A → ELK-class is
real and whether → Konclude-class is reachable, and that result should *reshape the headline*
before committing the implementation budget. Then: **A first** (saturator perf, to its
ELK/HermiT-class bar), **then B** onto the stabilized loop (proofs — the differentiator).
**C.1 in parallel** (different crate) but as VALIDATE-&-ENABLE of the existing
anywhere-blocking, not a from-scratch build; treat C as the uncertain longer bet (its
FP=0-under-blocking + sound-caching gate is the hardest in the repo). Ship A+B as the
near-term differentiated win; let C mature. Proofs work even if Track A only reaches
ELK-class — the product doesn't depend on beating Konclude.

## Self-review notes
- No task optimizes before its attribution (A depends on 0.2, C on 0.3) — no guessing.
- Every task carries the FP=0 sacred gate; C.1/C.2 additionally carry adversarial review +
  fuzz (they're where soundness historically broke).
- Proofs (B) are opt-in/observational → cannot regress soundness or default perf.
- The plan does NOT promise broad SROIQ parity (honest: C is a bet); it promises the
  achievable EL/Horn-perf + proofs headline and a clear, gated structural track.
