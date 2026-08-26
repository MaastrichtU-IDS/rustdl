# Release process

> **Setting this up on another machine?** `owl-reasoner-harness/docs/SETUP-ON-A-NEW-MACHINE.md`
> covers provisioning, which nothing here does: the ORE corpus source (Zenodo DOI
> 10.5281/zenodo.18578, `ore2015_sample.zip`, md5 `109f04cf8f124eb551d33c100e549730`, unzipping to
> **1,920** `.owl` files), peer-reasoner acquisition and its `Binaries/Konclude` trap, the pin-and-
> verify two-arm procedure, and what each metric means with its attached trap. Until 2026-08-26 that
> information existed only in an uncommitted file on one host.

Every gate here exists because omitting it shipped, or nearly shipped, a wrong number.
Cross-references name the incident.

## 1. Gates, in the order they should run

| gate | command | catches | cost |
|---|---|---|---|
| unit + lint | `cargo test --workspace --exclude owl-dl-py`, `cargo clippy … -D warnings`, `cargo fmt --all -- --check` | ordinary regressions | minutes |
| **FP=0 net** | `./scripts/run-soundness-diff.sh` | false subsumptions on curated fixtures | ~4 min |
| **corpus report + verdict gate** | `owl-reasoner-harness/scripts/release-corpus-report.sh <version> <binary> <prev-baseline.json>` | **answer changes**, wall/RSS drift | ~20 min |
| **two-arm full sweep** | harness `run` over all 1,920, previous release vs candidate | `ok → dnf` regressions | ~4 h/arm |
| **MISSED net** (when trading completeness) | `missed-net.sh sweep/net` | ΔMISSED vs Konclude ∪ HermiT | ~10 min/arm |

`owl-dl-py` is excluded from tests only because pyo3 cannot link libpython in this
environment — a pre-existing environment issue, not a skip.

## 2. The three gates are not substitutes for each other

This is the most expensive lesson in the list, and it was learned twice.

* **CI green does not imply FP=0.** The `closure-diff soundness net` CI job is
  `workflow_dispatch`-only and its fixtures are never provisioned. Green CI means fmt +
  clippy + unit tests.
* **The MISSED net cannot see an `ok → dnf`.** Its frame is drawn from *completers*, so
  it structurally cannot observe an ontology falling off the cap. A flip needs the full
  sweep too.
* **Neither the sweep nor the MISSED net can see a VERDICT change.** On 2026-08-15,
  `--pair-timeout-ms 5` passed both pre-registered clauses (`ok → dnf` = 0, ΔMISSED
  +0.78%) while flipping `ore_ont_16372` from `consistent=false` to `consistent=true`
  on an ontology Konclude, HermiT and rustdl's own `consistent` all call inconsistent.
  Both arms *completed*, so it was not `ok → dnf`; and `aligned_closures` excludes
  unsatisfiable classes on both sides, so an all-unsat ontology contributes ~nothing to
  ΔMISSED. That is what the corpus report's verdict gate is for.
  See `docs/2026-08-15-pair5-default-blocked.md`.

**The pre-registered rule for any change trading completeness for speed is now three
clauses:**

> Ship iff `ok → dnf` = 0 **AND** ΔMISSED < 5% **AND** no ontology changes its
> `consistent` verdict.

## 3. The corpus report

`owl-reasoner-harness/scripts/release-corpus-report.sh` runs a fixed population and
emits a markdown block for the release notes plus a baseline JSON for the next release
to diff against. It exits non-zero if a verdict flipped or an ontology was lost.

**The population is 400 stratified + 24 sentinels** (`baselines/release-population.txt`).
The sentinels are not decoration: `ore_ont_16372` — the ontology behind the verdict gate
— is **not** in the stratified 400, so that population is blind to the defect that
motivated the gate. A stratified sample is drawn to be *representative*; these cases
matter precisely because they are not. The sentinel list is every ontology that has
exposed a real defect here (the verdict flip, the `unsat_probe` cluster, the `saturate`
bucket, `10019`, the DKey discriminators `9347`/`5368`, the O(k²) print loop `10125`,
the RSS tail `11085`, and the four that went `ok → dnf` in v0.4.8).

**Add a sentinel whenever an ontology exposes a defect.** That is the maintenance rule.

### v0.4.18 baseline

424 ontologies, 60 s cap, 1 thread, binary `17aeec66e978`:

| | classified | DNF | empty output |
|---|---|---|---|
| count | **414** | 10 | 0 |

| | mean | median | p90 | max |
|---|---|---|---|---|
| wall (s) | 3.95 | 0.21 | 7.33 | 59.84 |
| peak RSS (MiB) | 151.7 | 19.5 | 356.7 | 6711.6 |

13 reported inconsistent · 27 flagged incomplete.

## 4. Comparison rules that have each cost a wrong result

* **Compare CLOSURES, not reductions.** `direct_subsumptions` is a transitive
  *reduction*: losing one subsumption promotes an endpoint to a direct edge, so a diff
  of `direct` rows shows *additions* where the closure only shrank. That produced three
  false soundness alarms in one sitting (2026-08-15).
* **Strip `#` banner lines before diffing output.** They carry per-phase wall timings
  and differ between any two runs. Including them reported **1,322 unexplained
  differences** where the truth was 1,702 identical + 50 permitted + **0** (2026-08-14).
* **Verify the instrument can see the thing.** A first pass compared the harness's
  `out_sha256` and reported "0 differences" — that field is `None` for every case in
  both arms, because the wrapper redirects stdout. It was comparing `None != None`
  while 50 ontologies had genuinely changed output, one by 978,892 rows (2026-08-14).
* **A broken instrument must not read as a result.** The corpus report's first
  invocation said "0 classified, 424 DNF" — a plausible catastrophic regression that
  was a bad `--only` format. `skipped` rows now abort with the harness's own reason.
* **Judge peer outcome from CONTENT, not exit code** — and point the check at the right
  stream. Konclude exits 0 on junk input and writes an ~896-byte `Thing`/`Nothing`
  hierarchy; it also prints its *log* to stdout and the taxonomy to `-o`. Reading the
  log once made a successful 55 ms classification look like a refusal (2026-08-14).
* **Normalise both sides before calling anything an FP.** Expand `EquivalentClasses`
  into pairwise subsumptions and exclude thing-equivalent and unsatisfiable classes
  symmetrically. A raw `SubClassOf` diff has produced a spurious FP figure **three**
  times, most recently 3,577 pairs that were zero after normalisation.
* **Pin binaries per configuration and verify the pin discriminates.** Name the path
  after the configuration, immediately after the build, and check it against an input
  whose answer differs between arms. A shared build path has twice measured the wrong
  configuration, once wasting a two-hour scan.

## 5. Cutting the release

1. Run the gates above; the corpus report must exit 0.
2. Bump `Cargo.toml` and `protege/update.properties`.
3. **Re-measure the `wine` freshness canary and update `CLAUDE.md` if it moved.** At
   v0.4.18 it went ~74 s → **~38 s** unbounded and ~4.6 s → **~2.7 s** at
   `--pair-timeout-ms 25`. Without the update the next reader sees 38 s and diagnoses a
   stale binary — a failure already on record.
4. Write `docs/releases/v<version>.md`. **Lead with any output-visible change**, not
   the performance numbers: v0.4.18 removed `direct` rows for unsatisfiable subjects
   (pizza 314 → 188 rows), which matters more to a downstream parser than any speedup.
5. Paste the corpus report block into the notes.
6. Commit, tag `v<version>`, push both.
