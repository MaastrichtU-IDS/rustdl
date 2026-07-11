# Authoritative benchmark matrix — design

**Date:** 2026-07-11
**Status:** approved (design); pending implementation plan

## Goal

One reproducible command that produces a single, authoritative performance
matrix across all reasoners and all ontologies, with full provenance —
reasoner versions, ontology source/hash/size, date, and host environment — so
the paper and the docs cite *one* current source of truth instead of the
scattered, drift-prone ad-hoc numbers accumulated across prior sessions.

## Motivation

Performance numbers currently live in several docs
(`reasoner-comparison-2026-06-21.md`, `perf-2026-06-27-bench-snapshot.md`,
`paper-evidence-native-2026-07-10.md`, README, CLAUDE.md). They were produced
by different ad-hoc harnesses on different dates, and at least one round was
corrupted by a **stale rustdl binary** (a two-week-old `target/release/rustdl`
produced a spurious `wine` DNF and inflated pizza/family walls). The matrix
replaces all of that with a versioned, regenerable artifact that records
exactly what was run, against what, when, and on which machine.

## Architecture

A new `matrix` subcommand on the **existing `owl-dl-bench` binary**.
`owl-dl-bench` already depends on `owl-dl-reasoner`, reads the curated corpus,
and has a `compare-whelk` mode — so rustdl's closure (for correctness) runs
**in-process**, reusing the closure-alignment code, while the external
reasoners (Konclude, HermiT, ELK, whelk-rs) are shelled out. One command drives
all stages; outputs land in a committed, versioned results directory.

```
owl-dl-bench matrix \
  --tier {curated|ore|bioportal|all} \
  --out docs/benchmarks/<date>-<tier>/ \
  --pair-timeout-ms 25 --global-timeout-s 120 \
  [--resume]
```

Code lives under `crates/owl-dl-bench/src/matrix/`, one focused module per
stage:

| Module | Responsibility |
|---|---|
| `provenance.rs` | Capture reasoner versions + host env → `run-metadata.json` |
| `corpus.rs` | Enumerate onts for the tier; compute sha256, size, class-count, fragment per ont |
| `oracle.rs` | Run Konclude per ont; produce the oracle closure |
| `run.rs` | Per (ont, reasoner): wall + peak RSS + status via a uniform `/usr/bin/time -l` subprocess wrapper |
| `correctness.rs` | FP/MISSED for each reasoner's output closure vs the Konclude oracle |
| `render.rs` | `results.jsonl` + `run-metadata.json` → committed `MATRIX.md` |

### Uniform measurement

Every reasoner — **including rustdl** — is measured for wall/RSS by running it
as a subprocess under `/usr/bin/time -l` (macOS: `real` = wall, "maximum
resident set size" = peak RSS in bytes). rustdl uses its **freshly built CLI
binary**, so its performance is measured the *same way* as Konclude's. rustdl's
closure for the FP/MISSED computation is obtained in-process via the reasoner
API (perf is irrelevant there); external reasoners' closures come from their
output files.

### Per-reasoner invocation

Each cell wraps the reasoner in `gtimeout <global_timeout_s>` under
`/usr/bin/time -l`:

| Reasoner | Command (input format) | Closure source |
|---|---|---|
| rustdl | `rustdl classify <ont>.ofn --pair-timeout-ms N` (fresh CLI) | in-process reasoner API |
| Konclude | `konclude classification -i <ont>.owx -o <out>.owx` | output owx (`read_konclude_verdict`) |
| HermiT | `robot reason --reasoner hermit --axiom-generators "subclass" -i <ont>.owl -o <out>.hermit.owx` | output owx |
| ELK | `robot reason --reasoner elk --axiom-generators "subclass" -i <ont>.owl -o <out>.elk.owx` | output owx |
| whelk-rs | `compare-whelk` (OFN-only) | whelk closure |

`robot` = `~/eval-tools/bin/robot` (Homebrew OpenJDK 17 arm64 + `robot.jar`
1.9.10, bundling HermiT + ELK). Verified: HermiT on `sulo` → 0.42 s wall,
239 MB RSS, 66 inferred `SubClassOf` axioms in the output owx.

**JVM-floor caveat (HermiT & ELK).** Their `/usr/bin/time -l` wall is
**end-to-end**: JVM boot + parse + reason + serialize. ROBOT logs only
whole-second reasoning time, so there is no clean pure-reasoning number; every
HermiT/ELK wall carries a ~0.4–1 s JVM-boot floor, and peak RSS carries a
~240 MB JVM baseline. Both are recorded verbatim but **labeled in `MATRIX.md`
as end-to-end JVM figures** so a small-ont "HermiT 420 ms vs rustdl 10 ms" is
not misread as a reasoning-speed gap. rustdl and Konclude native walls have no
comparable fixed floor.

**Exit codes.** ROBOT `reason` exits 1 when it finds unsatisfiable classes even
though reasoning completed — the exit code is ignored; status is derived from
the output, not the code.

### Fresh-binary guard (enforces the stale-binary lesson)

Stage 0 rebuilds rustdl with `RUSTUP_TOOLCHAIN=stable cargo build --release`,
records `git_sha` + `binary_mtime`, and **aborts the run** if the binary's
mtime predates the latest source commit. The repo pins toolchain 1.95.0 which
lacks the `cargo` binary, so a bare `cargo build` fails and silently reuses a
stale binary; this guard makes that failure mode impossible to benchmark
through.

## Data schema

### `run-metadata.json` (one per run — the provenance header)

```json
{
  "date": "2026-07-11T00:00:00Z",
  "tier": "curated",
  "oracle": "konclude-0.7.0-1138",
  "host": {
    "model": "MacBook…", "cpu": "Apple M…", "cores": 0, "ram_gb": 0,
    "os": "macOS 15.5 (Darwin 25.5.0)", "arch": "arm64"
  },
  "budgets": { "pair_timeout_ms": 25, "global_timeout_s": 120 },
  "reasoners": {
    "rustdl":   { "version": "0.3.21", "git_sha": "b19afb8", "binary_mtime": "…", "build": "release stable-toolchain" },
    "konclude": { "version": "0.7.0-1138", "build": "OSX-x64 via Rosetta 2 (walls/RSS are upper bounds)" },
    "hermit":   { "version": "1.9.10 (ROBOT)", "jdk": "OpenJDK 17.0.19" },
    "elk":      { "version": "0.6.0 (ROBOT 1.9.10)", "note": "EL-only" },
    "whelk-rs": { "git_sha": "701710d58b6039794bc5a4348880d813eecf2bbb", "note": "EL-only" }
  }
}
```

### `results.jsonl` (one line per (ontology, reasoner) cell)

```json
{
  "ont": "wine", "source": "corpus/wine.ofn",
  "sha256": "…", "size_bytes": 0, "classes": 653, "fragment": "SHOIN(D)",
  "reasoner": "rustdl",
  "status": "ok",
  "wall_ms": 1770, "peak_rss_mb": 210,
  "closure_size": 653, "fp": 0, "missed": 0,
  "oracle": "konclude-0.7.0-1138"
}
```

`status` ∈ `ok | dnf | error | na | inconsistent`, where `na` = out-of-fragment
for an EL-only reasoner (ELK/whelk-rs on a non-EL ont) and `inconsistent` = the
reasoner detected an inconsistent ontology (a *valid verdict*, e.g. `family`,
not a failure — ROBOT `reason` errors out on these, so the harness catches it
as `inconsistent` and does not let it abort the batch). When the oracle is
unavailable (Konclude itself DNFs/errors on the ont), `fp` and `missed` are
`null` and the cell keeps only wall/RSS/status.

### `MATRIX.md` (rendered, paper-/human-facing)

The metadata header, then per-tier tables (rows = onts; columns grouped per
reasoner: wall / RSS / FP / MISSED / status), then a summary block
(finished/DNF counts per reasoner, median & max wall, RSS tail, total
FP / total MISSED). `MATRIX.md` is a pure function of the two data files and is
regenerable without re-benchmarking.

## Oracle & correctness semantics

- **Oracle = Konclude's closure**, always. Konclude is complete (SROIQ) and
  finished every ORE-2015 pilot ont in prior runs (0 DNF), so it is a reliable
  single oracle. No intersection logic, no degradation precedence.
- **FP** (unsound) = subsumptions a reasoner asserts that the Konclude oracle
  does **not**. FP > 0 is a genuine unsoundness finding — the mechanism by
  which rustdl caught whelk-rs's 1350 over-derivations.
- **MISSED** (incomplete) = oracle subsumptions the reasoner fails to assert.
  For rustdl, MISSED > 0 only outside its guaranteed-complete fragment.
- **HermiT** is a benchmarked reasoner row (its own wall/RSS/status and its own
  FP/MISSED vs the Konclude oracle — a HermiT-vs-Konclude agreement check), but
  is **not** used to build the oracle.
- **Closure alignment** reuses the existing `aligned_closures` /
  `read_konclude_verdict` from `crates/owl-dl-reasoner/tests/konclude_closure_diff.rs`,
  restricted to **named-class atomic subsumptions over the shared signature** —
  the established comparison basis — so ELK/HermiT/whelk owx closures diff on
  equal footing with rustdl's.
- **Capability flags:** ELK and whelk-rs are EL reasoners; on a non-EL ont they
  reject input or silently drop axioms, so their cell is marked `status: "na"`
  (determined from the fragment computed in `corpus.rs`) rather than recording a
  misleading fast-but-incomplete number.
- **Konclude Rosetta caveat** rides in metadata; its walls/RSS are labeled
  upper bounds throughout.

## Tiering, resumability, rendering, testing

- **Tiering.** `--tier curated` (the ~dozen characterization onts) runs first
  and is the authoritative core the paper cites — fast and fully inspectable.
  `--tier ore` (the ORE-2015 pilot set staged in `~/data/ore-run`) and
  `--tier bioportal` (~900 BioPortal EL onts in `~/data/bioportal/owl`) are
  resumable follow-ons; each tier's exact ont count is whatever the stage-1
  enumeration finds at runtime, recorded in the metadata. Each tier writes to
  its own `docs/benchmarks/<date>-<tier>/`.
- **Resumability.** `--resume` skips any (ont, reasoner) cell already present in
  `results.jsonl`, so a killed or overnight BioPortal batch restarts where it
  stopped. Progress is logged per cell; any bounded/skipped coverage is logged
  explicitly so partial coverage never masquerades as full.
- **Rendering** is a pure, re-runnable function of the data files; `MATRIX.md`
  is regenerable and diffable.
- **Testing.** Unit tests cover the pieces that can lie silently: fragment
  classification, closure alignment / FP-MISSED counting against fixtures with
  known answers, the JSONL→Markdown renderer, and metadata capture. The full
  corpus run is the integration test; the existing `konclude_closure_diff` suite
  remains the correctness backstop.

## Corpus & environment (this host)

- Curated corpus: `docs/corpus.md` set, staged as `.ofn`/`.owl`/`.owx` in
  `~/eval-tools/work/`.
- ORE-2015 pilot + BioPortal: `~/data/ore-run`, `~/data/bioportal/owl`.
- Reasoner binaries/wrappers: `~/eval-tools/bin/{robot,konclude}`,
  `~/eval-tools/robot.jar` (ROBOT 1.9.10 = ELK + HermiT), Konclude v0.7.0-1138
  (OSX-x64 under Rosetta 2), OpenJDK 17 (Homebrew, native arm64).
- Peak RSS = `/usr/bin/time -l` "maximum resident set size"; per-ont caps via
  `gtimeout` (coreutils).

## Non-goals

- Not a CI gate (it runs external reasoners and a multi-hour BioPortal batch);
  it is an on-demand authoritative-artifact generator.
- Does not replace the `konclude_closure_diff` test suite (the FP=0/MISSED=0
  gate) — it complements it.
- No new reasoner integrations beyond the five already provisioned.

## Consumers to update after the first curated run

README, `CLAUDE.md`, `docs/reasoner-comparison-2026-06-21.md`,
`docs/paper-evidence-native-2026-07-10.md`, and the paper's Tables 1/2/4 should
cite `docs/benchmarks/<date>-curated/MATRIX.md` as the source of truth.
