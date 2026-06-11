# Anytime-under-deadline experiment — design (2026-06-11)

## Goal

Produce the central empirical evidence for the paper's claim — **sound anytime
OWL classification with calibrated incompleteness**: at any deadline `T`, rustdl
returns a hierarchy that is (a) **sound** (every reported subsumption holds —
precision 1.0, FP=0), (b) **partially complete** (recall rises with `T`), and
(c) **calibrated** (every missed subsumption is flagged undecided — zero *silent*
misses). And the differentiator vs a complete reasoner: below Konclude's
completion time, Konclude returns *nothing*; rustdl returns a usable sound
partial hierarchy with known gaps.

This is a measurement study, not a feature. It quantifies behavior rustdl
already largely has (per-pair `--pair-timeout-ms`, sound timed-out defaults, the
`timed_out_pairs` signal) and adds the one missing mechanism (a global
wall-clock deadline) the classic "anytime" framing needs.

## Design inputs (decided in brainstorming)

- **Deadline axis: phased.** Phase 1 sweeps the *existing per-pair* timeout (no
  engine build — validates metrics + harness). Phase 2 builds a *global
  wall-clock* deadline and re-runs.
- **Metrics:** precision(T) / recall(T) / silent-miss(T) **+** an explicit
  "vs Konclude all-or-nothing" contrast.
- **Corpus:** hard-SROIQ where the deadline bites — `sio`, `wine`, `ore-10908`,
  `ore-15672`, `alehif` — plus `galen` as a zero-overhead EL baseline.
- **Harness:** extend the trusted `tests/konclude_closure_diff.rs` (reuses
  oracle loading + closure diff + the `RUSTDL_TEST_PAIR_MS` sweep infra).

## Metric definitions (vs the HermiT/Konclude oracle closure `True`)

For a run at deadline `T` producing reported subsumption set `S(T)` and
flagged-undecided set `U(T)`:
- **precision(T)** = |S(T) ∩ True| / |S(T)| — the soundness axis. **Expected
  1.0 at every T** (FP=0). A value < 1.0 at any T is a real soundness bug, not
  an experiment result — the run **fails** (see gates).
- **recall(T)** = |S(T) ∩ True| / |True| — the completeness axis. Rises with T;
  flat at 1.0 for `galen` (EL baseline → zero anytime overhead).
- **silent-miss(T)** = |missed(T) \ U(T)|, where missed(T) = True \ S(T). **The
  calibration axis. Expected 0** — every missed subsumption is in the flagged
  undecided set (rustdl never silently drops an edge). Theoretical basis: at
  infinite budget MISSED=0 (corpus parity), so every finite-budget miss is a
  timeout, and a timeout flags the pair undecided — calibration holds by
  construction; the experiment *verifies* it empirically.

## Phase 1 — per-pair sweep (measurement + one small exposure)

**The one small build:** `Classification` exposes only the `timed_out_pairs`
*count*; silent-miss needs the *set*. The internal per-pair records already
carry the `timed_out` flag (`classify.rs:603`, the
`(i, j, entailed, used_saturation, timed_out)` tuple). Add: store the timed-out
pair indices on `Classification` + a public `undecided_pairs(&self) ->
Vec<(&str, &str)>`. Read-only reporting, soundness-neutral, gated by the
existing classify tests (hierarchy/stats unchanged).

**The sweep** (new `#[ignore]`d test in `konclude_closure_diff.rs`): for each
fixture, for each `T ∈ {5, 25, 100, 250, 1000} ms`:
1. `classify_top_down_with_timeout(onto, Duration::from_millis(T))` → `S(T)`,
   stats, `U(T) = undecided_pairs()`.
2. Diff `S(T)` vs the oracle closure (existing logic) → missed(T), fp(T).
3. Record `T, recall, precision, silent_miss, wall_ms, |U|, timed_out_count`.
4. Emit a per-fixture table to stdout + append a row to a CSV.

Runnable immediately after the exposure. Produces the per-pair evidence table.

## Phase 2 — global wall-clock deadline (build) + Konclude contrast

**Build:** thread one shared `global_deadline: Instant` through
`classify_top_down_internal` (new `classify_*_with_global_deadline` entry
point). Reuse the cooperative-deadline plumbing (`decide_with_deadline` already
polls `Instant::now()` in the tableau/wedge search loop) — but with a single
deadline for the whole run. When it fires: every pair not yet *confirmed*
(in-flight and not-yet-reached by the tier walk) becomes undecided → defaults to
"not subsumed" + joins `U`; the rayon loop short-circuits remaining work.

**Soundness (by construction):** reported subsumptions come only from confirmed
channels (told / saturator / tableau-`Unsat`); undecided defaults to
not-subsumed. So the partial hierarchy at any T is a sound under-approximation
and `U` is the honest unknown set. No new FP surface (nothing is *asserted* on
deadline — only *omitted*).

**Re-run** the Phase-1 metrics sweeping *global* `T ∈ {100ms, 1s, 10s, 30s}`.

**Konclude contrast:** measure `W_k` = native-Konclude classify wall per fixture
(reuse the existing native-Konclude timing path; the binary already produced the
oracle). Konclude's output is a step function: nothing for `T < W_k`, complete
at `W_k`. Tabulate `W_k` and rustdl's recall at `T = {fractions of W_k}` (e.g.
1%, 10%, 50% of `W_k`) — showing rustdl delivers a sound partial hierarchy
throughout the interval where Konclude delivers nothing.

## Output artifact

- A committed CSV `docs/anytime-results-2026-06-11.csv`: columns
  `fixture, phase (per_pair|global), deadline_ms, recall, precision, silent_miss,
  wall_ms, undecided, konclude_wall_ms`.
- A short `docs/anytime-results-2026-06-11.md`: the headline tables (precision
  flat at 1.0; recall-vs-T curves; silent-miss=0; the Konclude step-function
  contrast) + 2–3 sentences of interpretation per fixture, written for direct
  reuse in the paper's evaluation section.

## Verification / soundness gates

- **FP=0 at every deadline is the headline result AND the gate.** precision(T)
  < 1.0 at any T ⇒ a real soundness regression surfaced by the sweep ⇒ stop and
  treat as a bug, not a data point. (This is the cardinal invariant; the
  experiment is partly a stress test of it.)
- **silent-miss(T) = 0 expected.** A non-zero value is the *interesting*
  finding either way: if real, it falsifies the calibration claim and must be
  explained (a miss source that isn't a timeout — e.g. a trust_sat wedge miss);
  the paper must report it honestly. Investigate before publishing.
- **Phase-2 differential:** at a global deadline ≥ the fixture's max untimed
  wall, the global-deadline hierarchy must equal the untimed hierarchy
  (FP=0/MISSED=0, byte-identical) — proves the global-deadline mechanism drops
  nothing spuriously.
- Existing classify tests stay green (the `undecided_pairs` exposure is
  additive).

## Scope / non-goals

- **In scope:** the 6 fixtures; per-pair + global deadline sweeps; the three
  metrics + Konclude contrast; the CSV + results doc.
- **Not in scope:** changing the reasoner's *defaults* (the experiment uses
  explicit deadlines); per-class placement quality (deferred — pairwise recall
  is the agreed metric); a global deadline as a shipped product default (the
  build is for measurement; productionizing it is a separate decision informed
  by the results); ontologies without an oracle.
- **Reuses, does not duplicate:** oracle loading + closure diff (the
  `konclude_closure_diff.rs` harness), the cooperative-deadline plumbing
  (`decide_with_deadline`), the native-Konclude timing path.
