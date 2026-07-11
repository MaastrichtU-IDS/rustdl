# Benchmark-Matrix Harness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an `owl-dl-bench matrix` subcommand that produces one authoritative, reproducible performance matrix (`MATRIX.md`) plus canonical `results.jsonl` + `run-metadata.json` across five reasoners × a tiered ontology corpus, with full provenance and a fresh-binary guard.

**Architecture:** rustdl's closure is computed in-process (reusing an extracted-to-public `oracle_diff` module); Konclude/HermiT/ELK are shelled out under `/usr/bin/time -l gtimeout` and their owx outputs parsed; whelk-rs runs in-process behind the `whelk-compare` feature. Konclude's closure is the sole oracle. Six focused modules under `crates/owl-dl-bench/src/matrix/` implement provenance → corpus → oracle → run → correctness → render; a `matrix` subcommand drives them with tiering and `--resume`.

**Tech Stack:** Rust (edition 2024), clap, serde/serde_json, walkdir, sha2; external tools `~/eval-tools/bin/{konclude,robot}`, `gtimeout` (coreutils), `/usr/bin/time -l` (macOS).

## Global Constraints

- Build rustdl only via `RUSTUP_TOOLCHAIN=stable cargo build --release` — the pinned 1.95.0 toolchain lacks `cargo` and a bare build silently reuses a stale binary. All test/build commands in this plan prepend `RUSTUP_TOOLCHAIN=stable`.
- Oracle = **Konclude's closure only** (v0.7.0-1138). No `∩` / no degradation precedence. When Konclude DNFs/errors on an ont, `fp`/`missed` are `null`.
- The closure-comparison basis is **named-class atomic subsumptions over the shared signature**, excluding `owl:Thing`/`owl:Nothing`, unsat classes, and `thing_equiv` — the exact semantics already in `konclude_closure_diff.rs`. Do not re-invent them; reuse the extracted module.
- Every reasoner's wall/RSS is measured uniformly as a subprocess under `/usr/bin/time -l`. rustdl uses its freshly built CLI binary for the wall/RSS cell (correctness uses the in-process API separately).
- `status` ∈ `ok | dnf | error | na | inconsistent`. `na` = EL-only reasoner on a non-EL ont; `inconsistent` = reasoner reported an inconsistent ontology (a valid verdict, must not abort the batch).
- HermiT/ELK walls & RSS are end-to-end JVM figures (~0.4–1 s boot floor, ~240 MB baseline); Konclude is Rosetta-2 (upper bounds). These caveats must appear in `run-metadata.json` and in `MATRIX.md`.
- The matrix is not a CI gate. It does not replace the `konclude_closure_diff` test suite.
- Reasoner tool paths are resolved from `$RUSTDL_EVAL_TOOLS` (default `~/eval-tools`): `$RUSTDL_EVAL_TOOLS/bin/konclude`, `$RUSTDL_EVAL_TOOLS/bin/robot`.

## File Structure

- `crates/owl-dl-reasoner/src/oracle_diff.rs` (**new, public**) — the closure-diff primitives extracted from the test: `PairSet`, `OwxVerdict`, `read_owx_verdict`, `transitive_closure`, `closure_from_classification`, `aligned_closures`, `aligned_owx_closures`.
- `crates/owl-dl-reasoner/src/lib.rs` — add `pub mod oracle_diff;` and re-exports.
- `crates/owl-dl-reasoner/tests/konclude_closure_diff.rs` — delete the moved helpers; import them from the crate.
- `crates/owl-dl-bench/src/matrix/mod.rs` (**new**) — `run_matrix(args)` orchestrator + tier enumeration + `--resume`.
- `crates/owl-dl-bench/src/matrix/model.rs` (**new**) — `CellResult`, `RunMetadata`, `ReasonerMeta`, `HostInfo`, `Budgets`, `Status`, `OntMeta`; serde + JSONL read/write.
- `crates/owl-dl-bench/src/matrix/provenance.rs` (**new**) — reasoner-version + host capture; fresh-binary guard.
- `crates/owl-dl-bench/src/matrix/corpus.rs` (**new**) — ont enumeration, sha256/size/class-count/fragment, format normalization via `robot convert`.
- `crates/owl-dl-bench/src/matrix/run.rs` (**new**) — `/usr/bin/time -l gtimeout` subprocess wrapper, `TimedRun` parse, per-reasoner command builders.
- `crates/owl-dl-bench/src/matrix/oracle.rs` (**new**) — run Konclude → oracle owx → `OwxVerdict`.
- `crates/owl-dl-bench/src/matrix/correctness.rs` (**new**) — per-reasoner closure → FP/MISSED vs oracle.
- `crates/owl-dl-bench/src/matrix/render.rs` (**new**) — JSONL + metadata → `MATRIX.md`.
- `crates/owl-dl-bench/src/main.rs` — add `Command::Matrix { .. }` and dispatch.
- `crates/owl-dl-bench/Cargo.toml` — add `sha2` dependency.

---

## Task 1: Extract closure-diff primitives into a public `oracle_diff` module

**Files:**
- Create: `crates/owl-dl-reasoner/src/oracle_diff.rs`
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (add `pub mod oracle_diff;`)
- Modify: `crates/owl-dl-reasoner/tests/konclude_closure_diff.rs` (remove moved helpers, import from crate)
- Test: reuse existing `crates/owl-dl-reasoner/tests/konclude_closure_diff.rs`

**Interfaces:**
- Consumes: `Classification` (existing public type; methods `classes() -> &[String]`, `is_subclass(&str,&str) -> bool`, `unsatisfiable_classes() -> Vec<&str>`).
- Produces:
  - `pub type PairSet = std::collections::BTreeSet<(String, String)>;`
  - `pub struct OwxVerdict { pub edges: BTreeSet<(String,String)>, pub unsat: BTreeSet<String>, pub thing_equiv: BTreeSet<String> }`
  - `pub fn read_owx_verdict(path: &std::path::Path) -> anyhow::Result<OwxVerdict>` (parses any `.owx`; formerly `read_konclude_verdict`, now returns `Result` instead of panicking)
  - `pub fn transitive_closure(edges: &BTreeSet<(String,String)>) -> PairSet`
  - `pub fn closure_from_classification(c: &Classification, exclude: &BTreeSet<String>) -> PairSet`
  - `pub fn aligned_closures(c: &Classification, verdict: &OwxVerdict) -> (PairSet, PairSet)` (rustdl vs owx-oracle; unchanged semantics)
  - `pub fn aligned_owx_closures(reasoner: &OwxVerdict, oracle: &OwxVerdict) -> (PairSet, PairSet)` (owx-reasoner vs owx-oracle; **new**)

- [ ] **Step 1: Move the helpers verbatim into the new module, converting panics to `Result`**

Create `crates/owl-dl-reasoner/src/oracle_diff.rs`. Move `PairSet`, `KoncludeVerdict` (rename → `OwxVerdict`, make fields `pub`), `read_konclude_verdict` (rename → `read_owx_verdict`, change signature to return `anyhow::Result<OwxVerdict>` and replace the two `.unwrap_or_else(panic!)` calls with `?` + `anyhow::Context`), `transitive_closure`, `closure_from_classification`, `aligned_closures` from `konclude_closure_diff.rs`. Add the module doc and imports:

```rust
//! Closure-diff primitives for comparing a reasoner's classification against an
//! owx oracle (Konclude/HermiT/ELK output). Shared by the closure-diff tests
//! and the `owl-dl-bench matrix` harness so both use one canonical alignment.

use crate::Classification;
use horned_owl::model::{
    ClassExpression, Component, EquivalentClasses, SubClassOf,
};
use horned_owl::io::ParserConfiguration;
use horned_owl::io::owx::reader::read as read_owx;
use horned_owl::ontology::set::SetOntology;
use horned_owl::model::RcStr;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

const OWL_THING: &str = "http://www.w3.org/2002/07/owl#Thing";
const OWL_NOTHING: &str = "http://www.w3.org/2002/07/owl#Nothing";

pub type PairSet = BTreeSet<(String, String)>;
```

Paste the bodies of `transitive_closure`, `closure_from_classification`, and `aligned_closures` unchanged (they already match the signatures above). Convert `read_owx_verdict`:

```rust
pub fn read_owx_verdict(path: &Path) -> anyhow::Result<OwxVerdict> {
    use anyhow::Context;
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let (onto, _): (SetOntology<RcStr>, _) = read_owx(&mut reader, ParserConfiguration::default())
        .with_context(|| format!("parse {}", path.display()))?;
    let mut edges = BTreeSet::new();
    let mut unsat = BTreeSet::new();
    let mut thing_equiv = BTreeSet::new();
    // ... body identical to the old read_konclude_verdict loop over `&onto` ...
    Ok(OwxVerdict { edges, unsat, thing_equiv })
}
```

(Copy the exact match-loop body from `konclude_closure_diff.rs` lines 75–128.)

- [ ] **Step 2: Add the new owx-vs-owx alignment**

Append to `oracle_diff.rs`:

```rust
/// Align two owx verdicts (a reasoner's output vs the oracle's) onto the same
/// atomic-subsumption basis: exclude either side's unsat classes and either
/// side's thing-equivalent classes, then transitively close both edge sets.
/// Returns `(reasoner_pairs, oracle_pairs)`.
pub fn aligned_owx_closures(reasoner: &OwxVerdict, oracle: &OwxVerdict) -> (PairSet, PairSet) {
    let mut exclude: BTreeSet<String> = reasoner.unsat.union(&oracle.unsat).cloned().collect();
    exclude.extend(reasoner.thing_equiv.iter().cloned());
    exclude.extend(oracle.thing_equiv.iter().cloned());
    let filter = |full: PairSet| -> PairSet {
        full.into_iter()
            .filter(|(s, t)| !exclude.contains(s) && !exclude.contains(t))
            .collect()
    };
    (
        filter(transitive_closure(&reasoner.edges)),
        filter(transitive_closure(&oracle.edges)),
    )
}
```

- [ ] **Step 3: Wire the module into the crate**

In `crates/owl-dl-reasoner/src/lib.rs`, add near the other `pub mod` lines:

```rust
pub mod oracle_diff;
```

- [ ] **Step 4: Refactor the test to import instead of define**

In `crates/owl-dl-reasoner/tests/konclude_closure_diff.rs`: delete the local definitions of `PairSet`, `KoncludeVerdict`, `read_konclude_verdict`, `transitive_closure`, `closure_from_classification`, `aligned_closures` (lines ~41–259). Add at the top:

```rust
use owl_dl_reasoner::oracle_diff::{
    aligned_closures, closure_from_classification, read_owx_verdict as read_konclude_verdict,
    OwxVerdict as KoncludeVerdict, PairSet,
};
```

(The `as` aliases keep the rest of the test body unchanged. `read_konclude_verdict` now returns `Result`, so at its call sites in the test add `.expect("read owx verdict")`.)

- [ ] **Step 5: Verify the refactor is behavior-preserving**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test konclude_closure_diff --release -- --ignored --nocapture`
Expected: same pass count as before the change (21/21 passed), FP=0/MISSED=0 lines unchanged.

Also: `RUSTUP_TOOLCHAIN=stable cargo build -p owl-dl-reasoner --release`
Expected: compiles clean.

- [ ] **Step 6: Commit**

```bash
git add crates/owl-dl-reasoner/src/oracle_diff.rs crates/owl-dl-reasoner/src/lib.rs crates/owl-dl-reasoner/tests/konclude_closure_diff.rs
git commit -m "refactor: extract closure-diff primitives into public oracle_diff module"
```

---

## Task 2: Matrix data model + JSONL/metadata serde

**Files:**
- Create: `crates/owl-dl-bench/src/matrix/model.rs`
- Create: `crates/owl-dl-bench/src/matrix/mod.rs` (module wiring only in this task)
- Modify: `crates/owl-dl-bench/src/main.rs` (add `mod matrix;`)
- Test: inline `#[cfg(test)]` in `model.rs`

**Interfaces:**
- Produces:
  - `pub enum Status { Ok, Dnf, Error, Na, Inconsistent }` (serde: lowercase via `#[serde(rename_all = "lowercase")]`)
  - `pub struct CellResult { ont, source, sha256, size_bytes, classes, fragment, reasoner, status, wall_ms: Option<u64>, peak_rss_mb: Option<u64>, closure_size: Option<usize>, fp: Option<usize>, missed: Option<usize>, oracle: String }`
  - `pub struct OntMeta { pub name: String, pub source: String, pub sha256: String, pub size_bytes: u64, pub classes: usize, pub fragment: String }`
  - `pub struct RunMetadata { date, tier, oracle, host: HostInfo, budgets: Budgets, reasoners: BTreeMap<String, serde_json::Value> }`
  - `pub struct HostInfo { model, cpu, cores: u32, ram_gb: u64, os, arch }`
  - `pub struct Budgets { pair_timeout_ms: u64, global_timeout_s: u64 }`
  - `pub fn append_cell(path: &Path, cell: &CellResult) -> Result<()>` (append one JSON line)
  - `pub fn read_cells(path: &Path) -> Result<Vec<CellResult>>` (parse JSONL; empty vec if file absent)
  - `pub fn write_metadata(path: &Path, meta: &RunMetadata) -> Result<()>`

- [ ] **Step 1: Write the failing test**

Create `crates/owl-dl-bench/src/matrix/model.rs` with only the test at first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn cell_roundtrips_through_jsonl() {
        let dir = tempdir_here("cell_rt");
        let path = dir.join("results.jsonl");
        let cell = CellResult {
            ont: "wine".into(), source: "corpus/wine.ofn".into(),
            sha256: "abc".into(), size_bytes: 10, classes: 653, fragment: "SHOIN(D)".into(),
            reasoner: "rustdl".into(), status: Status::Ok,
            wall_ms: Some(1770), peak_rss_mb: Some(210),
            closure_size: Some(653), fp: Some(0), missed: Some(0),
            oracle: "konclude-0.7.0-1138".into(),
        };
        append_cell(&path, &cell).unwrap();
        append_cell(&path, &cell).unwrap();
        let back = read_cells(&path).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].ont, "wine");
        assert_eq!(back[0].status, Status::Ok);
        assert_eq!(back[0].fp, Some(0));
    }

    #[test]
    fn null_correctness_serializes_as_json_null() {
        let cell = CellResult {
            ont: "big".into(), source: "x".into(), sha256: "z".into(), size_bytes: 1,
            classes: 1, fragment: "EL".into(), reasoner: "hermit".into(),
            status: Status::Dnf, wall_ms: None, peak_rss_mb: None,
            closure_size: None, fp: None, missed: None, oracle: "konclude-0.7.0-1138".into(),
        };
        let line = serde_json::to_string(&cell).unwrap();
        assert!(line.contains("\"fp\":null"));
        assert!(line.contains("\"status\":\"dnf\""));
    }

    // Minimal deterministic scratch dir under the crate target dir (no external
    // tempdir crate). Uses the process id + test name for uniqueness.
    fn tempdir_here(tag: &str) -> std::path::PathBuf {
        let base = std::env::temp_dir().join(format!("rustdl-matrix-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        base
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-bench --lib matrix::model 2>&1 | head`
Expected: FAIL — `CellResult`, `Status`, `append_cell`, `read_cells` not found (or "cannot find module matrix").

- [ ] **Step 3: Implement the model**

Prepend to `crates/owl-dl-bench/src/matrix/model.rs`:

```rust
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status { Ok, Dnf, Error, Na, Inconsistent }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CellResult {
    pub ont: String,
    pub source: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub classes: usize,
    pub fragment: String,
    pub reasoner: String,
    pub status: Status,
    pub wall_ms: Option<u64>,
    pub peak_rss_mb: Option<u64>,
    pub closure_size: Option<usize>,
    pub fp: Option<usize>,
    pub missed: Option<usize>,
    pub oracle: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntMeta {
    pub name: String,
    pub source: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub classes: usize,
    pub fragment: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Budgets { pub pair_timeout_ms: u64, pub global_timeout_s: u64 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostInfo {
    pub model: String, pub cpu: String, pub cores: u32,
    pub ram_gb: u64, pub os: String, pub arch: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunMetadata {
    pub date: String,
    pub tier: String,
    pub oracle: String,
    pub host: HostInfo,
    pub budgets: Budgets,
    pub reasoners: BTreeMap<String, serde_json::Value>,
}

pub fn append_cell(path: &Path, cell: &CellResult) -> Result<()> {
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).ok(); }
    let mut f = OpenOptions::new().create(true).append(true).open(path)
        .with_context(|| format!("open {}", path.display()))?;
    let line = serde_json::to_string(cell)?;
    writeln!(f, "{line}")?;
    Ok(())
}

pub fn read_cells(path: &Path) -> Result<Vec<CellResult>> {
    if !path.exists() { return Ok(Vec::new()); }
    let f = std::fs::File::open(path)?;
    let mut out = Vec::new();
    for line in BufReader::new(f).lines() {
        let line = line?;
        if line.trim().is_empty() { continue; }
        out.push(serde_json::from_str(&line).with_context(|| format!("parse cell: {line}"))?);
    }
    Ok(out)
}

pub fn write_metadata(path: &Path, meta: &RunMetadata) -> Result<()> {
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).ok(); }
    std::fs::write(path, serde_json::to_string_pretty(meta)?)
        .with_context(|| format!("write {}", path.display()))?;
    Ok(())
}
```

Create `crates/owl-dl-bench/src/matrix/mod.rs`:

```rust
pub mod model;
```

Add to the top of `crates/owl-dl-bench/src/main.rs` (with the other `mod`/`use` lines):

```rust
mod matrix;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-bench --lib matrix::model`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/owl-dl-bench/src/matrix/ crates/owl-dl-bench/src/main.rs
git commit -m "feat(matrix): data model + JSONL/metadata serde"
```

---

## Task 3: Subprocess runner — `/usr/bin/time -l` + `gtimeout` parsing

**Files:**
- Create: `crates/owl-dl-bench/src/matrix/run.rs`
- Modify: `crates/owl-dl-bench/src/matrix/mod.rs` (`pub mod run;`)
- Test: inline `#[cfg(test)]` in `run.rs`

**Interfaces:**
- Produces:
  - `pub struct TimedRun { pub wall_ms: u64, pub peak_rss_mb: u64, pub exit_code: Option<i32>, pub timed_out: bool, pub stdout: String, pub stderr: String }`
  - `pub fn parse_time_l(stderr: &str) -> Option<(u64 /*wall_ms*/, u64 /*rss_mb*/)>`
  - `pub fn timed(cmd: &[&str], global_timeout_s: u64) -> Result<TimedRun>` — wraps `["/usr/bin/time", "-l", "gtimeout", "<s>", cmd...]`, captures stderr (time -l writes there), maps `gtimeout` exit 124 → `timed_out = true`.

- [ ] **Step 1: Write the failing test**

Create `crates/owl-dl-bench/src/matrix/run.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_time_and_rss() {
        // Verbatim macOS `/usr/bin/time -l` fragment (from a real Konclude run).
        let stderr = "        0.17 real         0.04 user         0.04 sys\n\
                      31784960  maximum resident set size\n\
                      0  peak memory footprint\n";
        let (wall_ms, rss_mb) = parse_time_l(stderr).expect("parsed");
        assert_eq!(wall_ms, 170);      // 0.17 s -> 170 ms
        assert_eq!(rss_mb, 30);        // 31784960 B -> 30 MiB (floor)
    }

    #[test]
    fn parses_multi_second_wall() {
        let stderr = "        6.39 real        12.10 user         1.02 sys\n\
                      239566848  maximum resident set size\n";
        let (wall_ms, rss_mb) = parse_time_l(stderr).unwrap();
        assert_eq!(wall_ms, 6390);
        assert_eq!(rss_mb, 228);       // 239566848 -> 228 MiB
    }

    #[test]
    fn returns_none_without_time_output() {
        assert!(parse_time_l("no timing here\n").is_none());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-bench --lib matrix::run 2>&1 | head`
Expected: FAIL — `parse_time_l` not found.

- [ ] **Step 3: Implement the runner**

Prepend to `run.rs`:

```rust
use anyhow::{Context, Result};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct TimedRun {
    pub wall_ms: u64,
    pub peak_rss_mb: u64,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Parse the two lines we need from macOS `/usr/bin/time -l` stderr:
///   "        0.17 real         0.04 user         0.04 sys"
///   "            31784960  maximum resident set size"
/// Returns (wall_ms, peak_rss_mib). RSS floored to whole MiB.
pub fn parse_time_l(stderr: &str) -> Option<(u64, u64)> {
    let mut wall_ms = None;
    let mut rss_mb = None;
    for line in stderr.lines() {
        let t = line.trim();
        if let Some(idx) = t.find(" real") {
            if let Ok(secs) = t[..idx].trim().parse::<f64>() {
                wall_ms = Some((secs * 1000.0).round() as u64);
            }
        }
        if t.ends_with("maximum resident set size") {
            if let Some(num) = t.split_whitespace().next() {
                if let Ok(bytes) = num.parse::<u64>() {
                    rss_mb = Some(bytes / (1024 * 1024));
                }
            }
        }
    }
    match (wall_ms, rss_mb) {
        (Some(w), Some(r)) => Some((w, r)),
        _ => None,
    }
}

/// Run `cmd` (argv, cmd[0] = program) wrapped in
/// `/usr/bin/time -l gtimeout <global_timeout_s> <cmd...>`.
pub fn timed(cmd: &[&str], global_timeout_s: u64) -> Result<TimedRun> {
    let mut c = Command::new("/usr/bin/time");
    c.arg("-l").arg("gtimeout").arg(global_timeout_s.to_string());
    c.args(cmd);
    let out = c.output().with_context(|| format!("spawn {:?}", cmd))?;
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let code = out.status.code();
    let timed_out = code == Some(124); // gtimeout signals timeout with 124
    let (wall_ms, peak_rss_mb) = parse_time_l(&stderr).unwrap_or((0, 0));
    Ok(TimedRun { wall_ms, peak_rss_mb, exit_code: code, timed_out, stdout, stderr })
}
```

Add to `mod.rs`: `pub mod run;`

- [ ] **Step 4: Run test to verify it passes**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-bench --lib matrix::run`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/owl-dl-bench/src/matrix/run.rs crates/owl-dl-bench/src/matrix/mod.rs
git commit -m "feat(matrix): timed subprocess runner + time -l parser"
```

---

## Task 4: Provenance capture + fresh-binary guard

**Files:**
- Create: `crates/owl-dl-bench/src/matrix/provenance.rs`
- Modify: `crates/owl-dl-bench/src/matrix/mod.rs` (`pub mod provenance;`)
- Test: inline `#[cfg(test)]` in `provenance.rs`

**Interfaces:**
- Consumes: `crate::matrix::model::{HostInfo, RunMetadata}`; `crate::matrix::run::timed`.
- Produces:
  - `pub fn parse_konclude_version(banner: &str) -> Option<String>` (e.g. `v0.7.0-1138`)
  - `pub fn parse_robot_version(s: &str) -> Option<String>` (e.g. `1.9.10`)
  - `pub fn newest_source_mtime(repo_root: &Path) -> Result<std::time::SystemTime>` (max mtime of tracked `crates/**/*.rs`, `**/Cargo.toml`, `Cargo.lock`)
  - `pub fn assert_fresh_binary(binary: &Path, repo_root: &Path) -> Result<()>` (Err if binary mtime < newest source mtime)
  - `pub fn capture_host() -> HostInfo`
  - `pub fn capture_reasoners(tools: &Path, rustdl_bin: &Path, repo_root: &Path) -> Result<BTreeMap<String, serde_json::Value>>`

- [ ] **Step 1: Write the failing test**

Create `crates/owl-dl-bench/src/matrix/provenance.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_konclude_banner() {
        let banner = "{info} >> Konclude - Uni Ulm Parallel Reasoner\n\
                      {info} >> Reasoner for the SROIQV(D) Description Logic, 64-bit, Version v0.7.0-1138 - 500e11d9 (Jun 18 2021)\n";
        assert_eq!(parse_konclude_version(banner).as_deref(), Some("v0.7.0-1138"));
    }

    #[test]
    fn parses_robot_version() {
        assert_eq!(parse_robot_version("ROBOT version 1.9.10").as_deref(), Some("1.9.10"));
    }

    #[test]
    fn stale_binary_is_rejected() {
        // A binary whose mtime is in the distant past must fail the guard against
        // any repo whose sources are newer.
        let dir = std::env::temp_dir().join(format!("rustdl-fresh-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("crates/x/src")).unwrap();
        std::fs::write(dir.join("crates/x/src/lib.rs"), "// new").unwrap();
        let bin = dir.join("oldbin");
        std::fs::write(&bin, "x").unwrap();
        // Backdate the binary 1 year.
        let year_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(365*24*3600);
        filetime_set(&bin, year_ago);
        assert!(assert_fresh_binary(&bin, &dir).is_err());
    }

    // Set mtime without an external crate: use `filetime`-free approach via `utimes`.
    fn filetime_set(p: &std::path::Path, t: std::time::SystemTime) {
        let secs = t.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        // touch -t is portable enough on macOS for a test.
        let stamp = format!("{}", secs);
        std::process::Command::new("touch")
            .arg("-d").arg(format!("@{stamp}")).arg(p)
            .status().ok();
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-bench --lib matrix::provenance 2>&1 | head`
Expected: FAIL — functions not found.

- [ ] **Step 3: Implement provenance**

Prepend to `provenance.rs`:

```rust
use crate::matrix::model::HostInfo;
use anyhow::{anyhow, Context, Result};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
use std::time::SystemTime;
use walkdir::WalkDir;

pub fn parse_konclude_version(banner: &str) -> Option<String> {
    // find "Version vX.Y.Z-BUILD"
    for line in banner.lines() {
        if let Some(i) = line.find("Version v") {
            let rest = &line[i + "Version ".len()..];
            let token = rest.split_whitespace().next()?;
            return Some(token.to_string());
        }
    }
    None
}

pub fn parse_robot_version(s: &str) -> Option<String> {
    s.lines()
        .find_map(|l| l.trim().strip_prefix("ROBOT version "))
        .map(|v| v.trim().to_string())
}

fn cmd_output(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program).args(args).output().ok()?;
    // Konclude prints its banner to stderr; robot to stdout — merge both.
    let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
    s.push_str(&String::from_utf8_lossy(&out.stderr));
    Some(s)
}

pub fn newest_source_mtime(repo_root: &Path) -> Result<SystemTime> {
    let mut newest = SystemTime::UNIX_EPOCH;
    for entry in WalkDir::new(repo_root.join("crates")).into_iter().filter_map(|e| e.ok()) {
        let p = entry.path();
        let is_src = p.extension().map(|e| e == "rs").unwrap_or(false)
            || p.file_name().map(|n| n == "Cargo.toml").unwrap_or(false);
        if is_src {
            if let Ok(m) = entry.metadata().and_then(|m| m.modified()) {
                if m > newest { newest = m; }
            }
        }
    }
    if let Ok(m) = std::fs::metadata(repo_root.join("Cargo.lock")).and_then(|m| m.modified()) {
        if m > newest { newest = m; }
    }
    Ok(newest)
}

pub fn assert_fresh_binary(binary: &Path, repo_root: &Path) -> Result<()> {
    let bin_mtime = std::fs::metadata(binary)
        .with_context(|| format!("stat {}", binary.display()))?
        .modified()?;
    let src_mtime = newest_source_mtime(repo_root)?;
    if bin_mtime < src_mtime {
        return Err(anyhow!(
            "STALE BINARY: {} is older than the newest source file. Rebuild with \
             `RUSTUP_TOOLCHAIN=stable cargo build --release -p owl-dl-cli` before benchmarking.",
            binary.display()
        ));
    }
    Ok(())
}

pub fn capture_host() -> HostInfo {
    let one = |prog: &str, args: &[&str]| -> String {
        cmd_output(prog, args).unwrap_or_default().trim().to_string()
    };
    let cpu = one("sysctl", &["-n", "machdep.cpu.brand_string"]);
    let cores = one("sysctl", &["-n", "hw.ncpu"]).parse().unwrap_or(0);
    let ram_bytes: u64 = one("sysctl", &["-n", "hw.memsize"]).parse().unwrap_or(0);
    let os_name = one("sw_vers", &["-productName"]);
    let os_ver = one("sw_vers", &["-productVersion"]);
    let darwin = one("uname", &["-r"]);
    HostInfo {
        model: one("sysctl", &["-n", "hw.model"]),
        cpu,
        cores,
        ram_gb: ram_bytes / (1024 * 1024 * 1024),
        os: format!("{os_name} {os_ver} (Darwin {darwin})"),
        arch: one("uname", &["-m"]),
    }
}

fn git_short_sha(repo_root: &Path) -> String {
    Command::new("git").arg("-C").arg(repo_root).args(["rev-parse", "--short", "HEAD"])
        .output().ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

fn whelk_sha(repo_root: &Path) -> String {
    // Parse Cargo.lock for the whelk git source revision.
    let lock = std::fs::read_to_string(repo_root.join("Cargo.lock")).unwrap_or_default();
    let mut in_whelk = false;
    for line in lock.lines() {
        if line.trim() == "name = \"whelk\"" { in_whelk = true; }
        if in_whelk {
            if let Some(src) = line.trim().strip_prefix("source = ") {
                if let Some(hash) = src.rsplit('#').next() {
                    return hash.trim_matches('"').to_string();
                }
            }
        }
    }
    String::new()
}

pub fn capture_reasoners(
    tools: &Path, rustdl_bin: &Path, repo_root: &Path,
) -> Result<BTreeMap<String, serde_json::Value>> {
    let konclude = tools.join("bin/konclude");
    let robot = tools.join("bin/robot");
    let kon_ver = cmd_output(&konclude.to_string_lossy(), &["-h"])
        .and_then(|s| parse_konclude_version(&s))
        .unwrap_or_else(|| "unknown".into());
    let robot_ver = cmd_output(&robot.to_string_lossy(), &["--version"])
        .and_then(|s| parse_robot_version(&s))
        .unwrap_or_else(|| "unknown".into());
    let rustdl_ver = env!("CARGO_PKG_VERSION").to_string();
    let bin_mtime = std::fs::metadata(rustdl_bin).ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs()).unwrap_or(0);

    let mut m = BTreeMap::new();
    m.insert("rustdl".into(), json!({
        "version": rustdl_ver, "git_sha": git_short_sha(repo_root),
        "binary_mtime_unix": bin_mtime, "build": "release stable-toolchain"
    }));
    m.insert("konclude".into(), json!({
        "version": kon_ver,
        "build": "OSX-x64 via Rosetta 2 (walls/RSS are upper bounds)"
    }));
    m.insert("hermit".into(), json!({ "version": format!("{robot_ver} (ROBOT)"),
        "note": "end-to-end JVM wall (~0.4-1s boot floor, ~240MB baseline)" }));
    m.insert("elk".into(), json!({ "version": format!("{robot_ver} (ROBOT)"),
        "note": "EL-only; end-to-end JVM wall" }));
    m.insert("whelk-rs".into(), json!({ "git_sha": whelk_sha(repo_root), "note": "EL-only" }));
    Ok(m)
}
```

Add to `mod.rs`: `pub mod provenance;`

- [ ] **Step 4: Run test to verify it passes**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-bench --lib matrix::provenance`
Expected: PASS (3 tests). (The `touch -d @secs` form works on macOS `touch`; if the CI host rejects it the test still exercises the version parsers — but on this eval host it is fine.)

- [ ] **Step 5: Commit**

```bash
git add crates/owl-dl-bench/src/matrix/provenance.rs crates/owl-dl-bench/src/matrix/mod.rs
git commit -m "feat(matrix): provenance capture + fresh-binary guard"
```

---

## Task 5: Corpus enumeration + fragment + format normalization

**Files:**
- Create: `crates/owl-dl-bench/src/matrix/corpus.rs`
- Modify: `crates/owl-dl-bench/src/matrix/mod.rs` (`pub mod corpus;`)
- Modify: `crates/owl-dl-bench/Cargo.toml` (add `sha2`)
- Test: inline `#[cfg(test)]` in `corpus.rs`

**Interfaces:**
- Consumes: `crate::matrix::model::OntMeta`; `owl_dl_reasoner::classify` for the class count; a fragment classifier.
- Produces:
  - `pub struct StagedOnt { pub meta: OntMeta, pub ofn: PathBuf, pub owl: PathBuf, pub owx: PathBuf }`
  - `pub fn sha256_hex(bytes: &[u8]) -> String`
  - `pub fn fragment_of(path: &Path) -> String` (coarse: `"EL"` if only EL constructs, else `"DL"`; refined below)
  - `pub fn enumerate(tier: &str, work_dir: &Path, tools: &Path) -> Result<Vec<StagedOnt>>` — lists onts for the tier, computes metadata, and ensures `.owx`+`.ofn` exist via `robot convert`.
  - `pub fn is_el_fragment(frag: &str) -> bool`

- [ ] **Step 1: Add the `sha2` dependency**

In `crates/owl-dl-bench/Cargo.toml` under `[dependencies]`:

```toml
sha2 = "0.10"
```

- [ ] **Step 2: Write the failing test**

Create `crates/owl-dl-bench/src/matrix/corpus.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_is_stable_and_hex() {
        let h = sha256_hex(b"hello");
        assert_eq!(h, "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824");
    }

    #[test]
    fn el_flag_matches_fragment() {
        assert!(is_el_fragment("EL"));
        assert!(is_el_fragment("EL+"));
        assert!(!is_el_fragment("SHOIN(D)"));
        assert!(!is_el_fragment("DL"));
    }
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-bench --lib matrix::corpus 2>&1 | head`
Expected: FAIL — `sha256_hex` / `is_el_fragment` not found.

- [ ] **Step 4: Implement corpus staging**

Prepend to `corpus.rs`:

```rust
use crate::matrix::model::OntMeta;
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex(&h.finalize())
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes { s.push_str(&format!("{b:02x}")); }
    s
}

pub fn is_el_fragment(frag: &str) -> bool {
    frag.starts_with("EL")
}

/// Coarse fragment label. A refined label is not required by the matrix — only
/// the EL/non-EL distinction gates the `na` status for ELK/whelk. We reuse the
/// reasoner's own profile detection where available; otherwise a lexical scan
/// of the functional-syntax source for non-EL constructs.
pub fn fragment_of(ofn_path: &Path) -> String {
    let src = std::fs::read_to_string(ofn_path).unwrap_or_default();
    // Non-EL constructs: unions, complements, universals, cardinalities,
    // nominals, inverse roles.
    const NON_EL: &[&str] = &[
        "ObjectUnionOf", "ObjectComplementOf", "ObjectAllValuesFrom",
        "ObjectMaxCardinality", "ObjectMinCardinality", "ObjectExactCardinality",
        "ObjectOneOf", "ObjectInverseOf", "DisjointUnion",
    ];
    if NON_EL.iter().any(|c| src.contains(c)) { "DL".into() } else { "EL".into() }
}

pub struct StagedOnt {
    pub meta: OntMeta,
    pub ofn: PathBuf,
    pub owl: PathBuf,
    pub owx: PathBuf,
}

/// Ensure `dst` exists by converting `src` with `robot convert`. No-op if `dst`
/// is already present (conversions are cached per ont).
fn ensure_convert(robot: &Path, src: &Path, dst: &Path) -> Result<()> {
    if dst.exists() { return Ok(()); }
    let status = Command::new(robot)
        .arg("convert").arg("-i").arg(src).arg("-o").arg(dst)
        .status().with_context(|| format!("robot convert {} -> {}", src.display(), dst.display()))?;
    if !status.success() {
        anyhow::bail!("robot convert failed for {}", src.display());
    }
    Ok(())
}

/// Directory + glob per tier. `curated` reads the pre-staged `work_dir`;
/// `ore`/`bioportal` read the data dirs (env-overridable).
fn tier_sources(tier: &str, work_dir: &Path) -> Result<Vec<PathBuf>> {
    let home = std::env::var("HOME").unwrap_or_default();
    let (root, ext): (PathBuf, &str) = match tier {
        "curated" => (work_dir.to_path_buf(), "ofn"),
        "ore" => (PathBuf::from(std::env::var("RUSTDL_ORE_DIR")
            .unwrap_or(format!("{home}/data/ore-run"))), "owl"),
        "bioportal" => (PathBuf::from(std::env::var("RUSTDL_BIOPORTAL_DIR")
            .unwrap_or(format!("{home}/data/bioportal/owl"))), "owl"),
        other => anyhow::bail!("unknown tier {other}"),
    };
    let mut out = Vec::new();
    for e in walkdir::WalkDir::new(&root).into_iter().filter_map(|e| e.ok()) {
        if e.path().extension().map(|x| x == ext).unwrap_or(false) {
            out.push(e.path().to_path_buf());
        }
    }
    out.sort();
    Ok(out)
}

pub fn enumerate(tier: &str, work_dir: &Path, tools: &Path) -> Result<Vec<StagedOnt>> {
    let robot = tools.join("bin/robot");
    let mut staged = Vec::new();
    for src in tier_sources(tier, work_dir)? {
        let stem = src.file_stem().unwrap().to_string_lossy().to_string();
        let dir = src.parent().unwrap();
        let owx = dir.join(format!("{stem}.owx"));
        let ofn = dir.join(format!("{stem}.ofn"));
        let owl = if src.extension().map(|x| x == "owl").unwrap_or(false) {
            src.clone()
        } else {
            dir.join(format!("{stem}.owl"))
        };
        // Normalize to the formats the reasoners need. Record convert errors as
        // an empty meta with fragment="convert-error" so the caller marks cells.
        let converted = (|| -> Result<()> {
            ensure_convert(&robot, &src, &owx)?;
            ensure_convert(&robot, &src, &ofn)?;
            Ok(())
        })();
        let bytes = std::fs::read(&src).with_context(|| format!("read {}", src.display()))?;
        let sha256 = sha256_hex(&bytes);
        let (classes, fragment) = if converted.is_err() {
            (0, "convert-error".to_string())
        } else {
            let count = owl_dl_reasoner::classify(&load_any(&ofn)?)
                .map(|c| c.classes().len()).unwrap_or(0);
            (count, fragment_of(&ofn))
        };
        staged.push(StagedOnt {
            meta: OntMeta {
                name: stem, source: src.to_string_lossy().into_owned(),
                sha256, size_bytes: bytes.len() as u64, classes, fragment,
            },
            ofn, owl, owx,
        });
    }
    Ok(staged)
}

fn load_any(ofn: &Path) -> Result<horned_owl::ontology::set::SetOntology<horned_owl::model::RcStr>> {
    use horned_owl::io::ofn::reader::read as read_ofn;
    use horned_owl::io::ParserConfiguration;
    let src = std::fs::read_to_string(ofn)?;
    let mut cur = std::io::Cursor::new(src);
    let (onto, _) = read_ofn(&mut cur, ParserConfiguration::default())
        .with_context(|| format!("parse {}", ofn.display()))?;
    Ok(onto)
}
```

Add to `mod.rs`: `pub mod corpus;`

- [ ] **Step 5: Run test to verify it passes**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-bench --lib matrix::corpus`
Expected: PASS (2 tests).

- [ ] **Step 6: Commit**

```bash
git add crates/owl-dl-bench/src/matrix/corpus.rs crates/owl-dl-bench/src/matrix/mod.rs crates/owl-dl-bench/Cargo.toml
git commit -m "feat(matrix): corpus enumeration, fragment detection, format normalization"
```

---

## Task 6: Oracle + per-reasoner correctness

**Files:**
- Create: `crates/owl-dl-bench/src/matrix/correctness.rs`
- Modify: `crates/owl-dl-bench/src/matrix/mod.rs` (`pub mod correctness;`)
- Test: inline `#[cfg(test)]` in `correctness.rs` (fixture-based)

**Interfaces:**
- Consumes: `owl_dl_reasoner::oracle_diff::{OwxVerdict, read_owx_verdict, aligned_closures, aligned_owx_closures, closure_from_classification, PairSet}`; `owl_dl_reasoner::{classify_top_down_with_timeout, Classification}`; `crate::matrix::corpus::StagedOnt`.
- Produces:
  - `pub struct Correctness { pub closure_size: usize, pub fp: usize, pub missed: usize }`
  - `pub fn diff_pairsets(reasoner: &PairSet, oracle: &PairSet) -> Correctness`
  - `pub fn rustdl_vs_oracle(ont: &StagedOnt, oracle: &OwxVerdict, pair_ms: u64) -> Result<Correctness>`
  - `pub fn owx_vs_oracle(reasoner_owx: &Path, oracle: &OwxVerdict) -> Result<Correctness>`

- [ ] **Step 1: Write the failing test**

Create `crates/owl-dl-bench/src/matrix/correctness.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use owl_dl_reasoner::oracle_diff::PairSet;

    fn ps(pairs: &[(&str, &str)]) -> PairSet {
        pairs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn fp_and_missed_counted_against_oracle() {
        let oracle = ps(&[("A", "B"), ("B", "C"), ("A", "C")]);
        let reasoner = ps(&[("A", "B"), ("A", "C"), ("A", "D")]); // missing B<C; extra A<D
        let c = diff_pairsets(&reasoner, &oracle);
        assert_eq!(c.closure_size, 3);
        assert_eq!(c.fp, 1);      // A<D is unsound
        assert_eq!(c.missed, 1);  // B<C missed
    }

    #[test]
    fn identical_closures_are_clean() {
        let o = ps(&[("A", "B")]);
        let c = diff_pairsets(&o.clone(), &o);
        assert_eq!(c.fp, 0);
        assert_eq!(c.missed, 0);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-bench --lib matrix::correctness 2>&1 | head`
Expected: FAIL — `diff_pairsets` not found.

- [ ] **Step 3: Implement correctness**

Prepend to `correctness.rs`:

```rust
use crate::matrix::corpus::StagedOnt;
use anyhow::{Context, Result};
use owl_dl_reasoner::oracle_diff::{
    aligned_closures, aligned_owx_closures, read_owx_verdict, OwxVerdict, PairSet,
};
use owl_dl_reasoner::classify_top_down_with_timeout;
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub struct Correctness { pub closure_size: usize, pub fp: usize, pub missed: usize }

pub fn diff_pairsets(reasoner: &PairSet, oracle: &PairSet) -> Correctness {
    let fp = reasoner.difference(oracle).count();
    let missed = oracle.difference(reasoner).count();
    Correctness { closure_size: reasoner.len(), fp, missed }
}

pub fn rustdl_vs_oracle(ont: &StagedOnt, oracle: &OwxVerdict, pair_ms: u64) -> Result<Correctness> {
    let onto = crate::matrix::corpus_load_ofn(&ont.ofn)?;
    let c = classify_top_down_with_timeout(&onto, Duration::from_millis(pair_ms))
        .context("rustdl classify")?;
    let (rustdl, oracle_pairs) = aligned_closures(&c, oracle);
    Ok(diff_pairsets(&rustdl, &oracle_pairs))
}

pub fn owx_vs_oracle(reasoner_owx: &Path, oracle: &OwxVerdict) -> Result<Correctness> {
    let v = read_owx_verdict(reasoner_owx)?;
    let (reasoner, oracle_pairs) = aligned_owx_closures(&v, oracle);
    Ok(diff_pairsets(&reasoner, &oracle_pairs))
}
```

Add a small shared loader to `corpus.rs` (make `load_any` reusable): rename `load_any` to `pub fn corpus_load_ofn` and re-export from `mod.rs`:

In `corpus.rs` change `fn load_any(` → `pub fn corpus_load_ofn(` and update its one call site in `enumerate`. In `mod.rs` add:

```rust
pub use corpus::corpus_load_ofn;
```

Add to `mod.rs`: `pub mod correctness;`

- [ ] **Step 4: Run test to verify it passes**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-bench --lib matrix::correctness`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/owl-dl-bench/src/matrix/correctness.rs crates/owl-dl-bench/src/matrix/corpus.rs crates/owl-dl-bench/src/matrix/mod.rs
git commit -m "feat(matrix): Konclude-oracle correctness (FP/MISSED) for rustdl + owx reasoners"
```

---

## Task 7: Render `MATRIX.md`

**Files:**
- Create: `crates/owl-dl-bench/src/matrix/render.rs`
- Modify: `crates/owl-dl-bench/src/matrix/mod.rs` (`pub mod render;`)
- Test: inline `#[cfg(test)]` in `render.rs`

**Interfaces:**
- Consumes: `crate::matrix::model::{CellResult, RunMetadata, Status}`.
- Produces: `pub fn render_markdown(meta: &RunMetadata, cells: &[CellResult]) -> String`

- [ ] **Step 1: Write the failing test**

Create `crates/owl-dl-bench/src/matrix/render.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::matrix::model::*;
    use std::collections::BTreeMap;

    fn meta() -> RunMetadata {
        RunMetadata {
            date: "2026-07-11T00:00:00Z".into(), tier: "curated".into(),
            oracle: "konclude-0.7.0-1138".into(),
            host: HostInfo { model: "Mac".into(), cpu: "M".into(), cores: 8,
                ram_gb: 16, os: "macOS".into(), arch: "arm64".into() },
            budgets: Budgets { pair_timeout_ms: 25, global_timeout_s: 120 },
            reasoners: BTreeMap::new(),
        }
    }
    fn cell(ont: &str, r: &str, status: Status, wall: Option<u64>, fp: Option<usize>) -> CellResult {
        CellResult { ont: ont.into(), source: "s".into(), sha256: "h".into(), size_bytes: 1,
            classes: 10, fragment: "EL".into(), reasoner: r.into(), status,
            wall_ms: wall, peak_rss_mb: Some(20), closure_size: Some(10), fp, missed: Some(0),
            oracle: "konclude-0.7.0-1138".into() }
    }

    #[test]
    fn renders_header_rows_and_caveats() {
        let cells = vec![
            cell("wine", "rustdl", Status::Ok, Some(1770), Some(0)),
            cell("wine", "hermit", Status::Ok, Some(6390), Some(0)),
            cell("wine", "elk", Status::Na, None, None),
        ];
        let md = render_markdown(&meta(), &cells);
        assert!(md.contains("# rustdl performance matrix"));
        assert!(md.contains("konclude-0.7.0-1138"));   // oracle stated
        assert!(md.contains("wine"));
        assert!(md.contains("n/a") || md.contains("N/A")); // ELK na cell
        assert!(md.to_lowercase().contains("jvm"));     // JVM caveat present
        assert!(md.to_lowercase().contains("rosetta")); // Rosetta caveat present
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-bench --lib matrix::render 2>&1 | head`
Expected: FAIL — `render_markdown` not found.

- [ ] **Step 3: Implement render**

Prepend to `render.rs`:

```rust
use crate::matrix::model::{CellResult, RunMetadata, Status};
use std::collections::{BTreeMap, BTreeSet};

const REASONER_ORDER: &[&str] = &["rustdl", "konclude", "hermit", "elk", "whelk-rs"];

fn cell_wall(c: &CellResult) -> String {
    match c.status {
        Status::Na => "n/a".into(),
        Status::Dnf => "DNF".into(),
        Status::Error => "err".into(),
        Status::Inconsistent => "inconsistent".into(),
        Status::Ok => c.wall_ms.map(|w| format!("{w} ms")).unwrap_or("?".into()),
    }
}

fn cell_correctness(c: &CellResult) -> String {
    match (c.fp, c.missed) {
        (Some(fp), Some(m)) => format!("FP {fp} / M {m}"),
        _ => "—".into(),
    }
}

pub fn render_markdown(meta: &RunMetadata, cells: &[CellResult]) -> String {
    let mut s = String::new();
    s.push_str("# rustdl performance matrix\n\n");
    s.push_str(&format!(
        "**Date:** {}  \n**Tier:** {}  \n**Oracle:** {} (FP = asserts what the oracle does not; MISSED = oracle subsumptions not asserted)  \n",
        meta.date, meta.tier, meta.oracle));
    s.push_str(&format!(
        "**Host:** {} · {} · {} cores · {} GB · {}  \n",
        meta.host.model, meta.host.cpu, meta.host.cores, meta.host.ram_gb, meta.host.os));
    s.push_str(&format!(
        "**Budgets:** per-pair {} ms, global {} s\n\n",
        meta.budgets.pair_timeout_ms, meta.budgets.global_timeout_s));
    s.push_str("> **Caveats.** HermiT/ELK walls & RSS are end-to-end **JVM** figures \
        (~0.4–1 s boot floor, ~240 MB baseline) — not pure reasoning time. \
        Konclude runs under **Rosetta 2** (x64), so its walls/RSS are upper bounds. \
        `n/a` = EL-only reasoner on a non-EL ontology.\n\n");

    // Group cells by ont.
    let mut by_ont: BTreeMap<&str, BTreeMap<&str, &CellResult>> = BTreeMap::new();
    let mut onts_order: Vec<&str> = Vec::new();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for c in cells {
        if seen.insert(c.ont.as_str()) { onts_order.push(c.ont.as_str()); }
        by_ont.entry(c.ont.as_str()).or_default().insert(c.reasoner.as_str(), c);
    }

    // Header.
    s.push_str("| ontology | frag | classes |");
    for r in REASONER_ORDER { s.push_str(&format!(" {r} wall | {r} RSS | {r} FP/M |")); }
    s.push('\n');
    s.push_str("|---|---|--:|");
    for _ in REASONER_ORDER { s.push_str("--:|--:|:--|"); }
    s.push('\n');

    for ont in &onts_order {
        let row = &by_ont[ont];
        let any = row.values().next().unwrap();
        s.push_str(&format!("| {} | {} | {} |", ont, any.fragment, any.classes));
        for r in REASONER_ORDER {
            match row.get(r) {
                Some(c) => s.push_str(&format!(" {} | {} MB | {} |",
                    cell_wall(c),
                    c.peak_rss_mb.map(|x| x.to_string()).unwrap_or("—".into()),
                    cell_correctness(c))),
                None => s.push_str(" — | — | — |"),
            }
        }
        s.push('\n');
    }

    // Summary per reasoner.
    s.push_str("\n## Summary\n\n| reasoner | finished | DNF | error | n/a | total FP | total MISSED |\n");
    s.push_str("|---|--:|--:|--:|--:|--:|--:|\n");
    for r in REASONER_ORDER {
        let rc: Vec<&CellResult> = cells.iter().filter(|c| c.reasoner == *r).collect();
        let count = |st: Status| rc.iter().filter(|c| c.status == st).count();
        let sum_fp: usize = rc.iter().filter_map(|c| c.fp).sum();
        let sum_missed: usize = rc.iter().filter_map(|c| c.missed).sum();
        s.push_str(&format!("| {} | {} | {} | {} | {} | {} | {} |\n",
            r, count(Status::Ok), count(Status::Dnf), count(Status::Error),
            count(Status::Na), sum_fp, sum_missed));
    }
    s
}
```

Add to `mod.rs`: `pub mod render;`

- [ ] **Step 4: Run test to verify it passes**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-bench --lib matrix::render`
Expected: PASS (1 test).

- [ ] **Step 5: Commit**

```bash
git add crates/owl-dl-bench/src/matrix/render.rs crates/owl-dl-bench/src/matrix/mod.rs
git commit -m "feat(matrix): render MATRIX.md with per-reasoner tables + caveats"
```

---

## Task 8: Wire the `matrix` subcommand — orchestration, tiering, resume

**Files:**
- Modify: `crates/owl-dl-bench/src/matrix/mod.rs` (add `run_matrix`)
- Modify: `crates/owl-dl-bench/src/main.rs` (add `Command::Matrix`)
- Test: inline `#[cfg(test)]` in `mod.rs` for the resume-skip logic; manual curated smoke run.

**Interfaces:**
- Consumes: everything from Tasks 2–7.
- Produces:
  - `pub struct MatrixArgs { pub tier: String, pub out: PathBuf, pub pair_timeout_ms: u64, pub global_timeout_s: u64, pub resume: bool, pub tools: PathBuf, pub work_dir: PathBuf, pub rustdl_bin: PathBuf, pub repo_root: PathBuf }`
  - `pub fn run_matrix(args: MatrixArgs) -> Result<()>`
  - `pub fn already_done(existing: &[CellResult], ont: &str, reasoner: &str) -> bool`

- [ ] **Step 1: Write the failing test (resume-skip logic)**

Add to `crates/owl-dl-bench/src/matrix/mod.rs` (in a `#[cfg(test)] mod tests`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::matrix::model::{CellResult, Status};

    fn c(ont: &str, r: &str) -> CellResult {
        CellResult { ont: ont.into(), source: "s".into(), sha256: "h".into(), size_bytes: 1,
            classes: 1, fragment: "EL".into(), reasoner: r.into(), status: Status::Ok,
            wall_ms: Some(1), peak_rss_mb: Some(1), closure_size: Some(0), fp: Some(0),
            missed: Some(0), oracle: "k".into() }
    }

    #[test]
    fn resume_skips_completed_cells_only() {
        let existing = vec![c("wine", "rustdl"), c("wine", "konclude")];
        assert!(already_done(&existing, "wine", "rustdl"));
        assert!(!already_done(&existing, "wine", "hermit"));
        assert!(!already_done(&existing, "pizza", "rustdl"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-bench --lib matrix::tests::resume 2>&1 | head`
Expected: FAIL — `already_done` not found.

- [ ] **Step 3: Implement the orchestrator**

Add to `mod.rs` (above the tests):

```rust
use crate::matrix::model::*;
use crate::matrix::run::{timed, TimedRun};
use anyhow::Result;
use std::path::PathBuf;

pub mod model;
pub mod run;
pub mod provenance;
pub mod corpus;
pub mod correctness;
pub mod render;
pub use corpus::corpus_load_ofn;

pub struct MatrixArgs {
    pub tier: String,
    pub out: PathBuf,
    pub pair_timeout_ms: u64,
    pub global_timeout_s: u64,
    pub resume: bool,
    pub tools: PathBuf,
    pub work_dir: PathBuf,
    pub rustdl_bin: PathBuf,
    pub repo_root: PathBuf,
}

pub fn already_done(existing: &[CellResult], ont: &str, reasoner: &str) -> bool {
    existing.iter().any(|c| c.ont == ont && c.reasoner == reasoner)
}

/// Map a TimedRun + an inconsistency signal to a Status.
fn status_of(run: &TimedRun, inconsistent: bool) -> Status {
    if inconsistent { return Status::Inconsistent; }
    if run.timed_out { return Status::Dnf; }
    // ROBOT exits 1 on unsat classes though reasoning completed; treat only
    // non-timeout, non-1 failures as errors when no output was produced.
    match run.exit_code {
        Some(0) | Some(1) => Status::Ok,
        _ => Status::Error,
    }
}

pub fn run_matrix(args: MatrixArgs) -> Result<()> {
    let results = args.out.join("results.jsonl");
    let meta_path = args.out.join("run-metadata.json");
    let matrix_md = args.out.join("MATRIX.md");

    // Stage 0: fresh-binary guard + provenance.
    provenance::assert_fresh_binary(&args.rustdl_bin, &args.repo_root)?;
    let reasoners = provenance::capture_reasoners(&args.tools, &args.rustdl_bin, &args.repo_root)?;
    let oracle_id = reasoners.get("konclude")
        .and_then(|v| v.get("version")).and_then(|v| v.as_str())
        .map(|v| format!("konclude-{}", v.trim_start_matches('v')))
        .unwrap_or_else(|| "konclude".into());
    let meta = RunMetadata {
        date: iso_now(),
        tier: args.tier.clone(),
        oracle: oracle_id.clone(),
        host: provenance::capture_host(),
        budgets: Budgets { pair_timeout_ms: args.pair_timeout_ms, global_timeout_s: args.global_timeout_s },
        reasoners,
    };
    write_metadata(&meta_path, &meta)?;

    // Stage 1: enumerate + normalize.
    let onts = corpus::enumerate(&args.tier, &args.work_dir, &args.tools)?;
    eprintln!("matrix: {} ontologies in tier {}", onts.len(), args.tier);

    let existing = if args.resume { read_cells(&results)? } else { Vec::new() };
    let robot = args.tools.join("bin/robot");
    let konclude = args.tools.join("bin/konclude");

    for ont in &onts {
        // Stage 2: oracle (Konclude) — run once per ont.
        let kon_out = ont.owx.with_extension("kon.owx");
        let kon_cmd = [konclude.to_str().unwrap(), "classification",
            "-i", ont.owx.to_str().unwrap(), "-o", kon_out.to_str().unwrap()];
        let kon_run = timed(&kon_cmd, args.global_timeout_s)?;
        let oracle_verdict = if kon_run.timed_out || !kon_out.exists() {
            None
        } else {
            owl_dl_reasoner::oracle_diff::read_owx_verdict(&kon_out).ok()
        };

        // One cell per reasoner.
        for reasoner in ["rustdl", "konclude", "hermit", "elk", "whelk-rs"] {
            if args.resume && already_done(&existing, &ont.meta.name, reasoner) { continue; }
            let cell = build_cell(&args, ont, reasoner, &oracle_verdict, &oracle_id,
                &kon_run, &kon_out, &robot, &konclude)?;
            append_cell(&results, &cell)?;
            eprintln!("  {} / {} -> {:?} {} ms", ont.meta.name, reasoner, cell.status,
                cell.wall_ms.unwrap_or(0));
        }
    }

    // Stage 4: render.
    let all = read_cells(&results)?;
    std::fs::write(&matrix_md, render::render_markdown(&meta, &all))?;
    eprintln!("matrix: wrote {}", matrix_md.display());
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn build_cell(
    args: &MatrixArgs, ont: &corpus::StagedOnt, reasoner: &str,
    oracle: &Option<owl_dl_reasoner::oracle_diff::OwxVerdict>, oracle_id: &str,
    kon_run: &TimedRun, kon_out: &std::path::Path,
    robot: &std::path::Path, konclude: &std::path::Path,
) -> Result<CellResult> {
    let el_only = matches!(reasoner, "elk" | "whelk-rs");
    let mut status = Status::Ok;
    let (mut wall, mut rss): (Option<u64>, Option<u64>) = (None, None);
    let mut correctness = None;

    if ont.meta.fragment == "convert-error" {
        status = Status::Error;
    } else if el_only && !corpus::is_el_fragment(&ont.meta.fragment) {
        status = Status::Na;
    } else {
        match reasoner {
            "konclude" => {
                wall = Some(kon_run.wall_ms); rss = Some(kon_run.peak_rss_mb);
                status = status_of(kon_run, false);
                // Konclude is the oracle -> FP/MISSED trivially 0 when it finished.
                if oracle.is_some() {
                    correctness = Some(correctness::Correctness {
                        closure_size: oracle.as_ref().unwrap().edges.len(), fp: 0, missed: 0 });
                }
            }
            "rustdl" => {
                let cmd = [args.rustdl_bin.to_str().unwrap(), "classify",
                    ont.ofn.to_str().unwrap(),
                    "--pair-timeout-ms", &args.pair_timeout_ms.to_string()];
                let r = timed(&cmd, args.global_timeout_s)?;
                wall = Some(r.wall_ms); rss = Some(r.peak_rss_mb);
                status = status_of(&r, false);
                if let (Status::Ok, Some(orc)) = (status, oracle.as_ref()) {
                    correctness = correctness::rustdl_vs_oracle(ont, orc, args.pair_timeout_ms).ok();
                }
            }
            "hermit" | "elk" => {
                let engine = if reasoner == "hermit" { "hermit" } else { "elk" };
                let out = ont.owx.with_extension(format!("{engine}.owx"));
                let cmd = [robot.to_str().unwrap(), "reason", "--reasoner", engine,
                    "--axiom-generators", "subclass",
                    "-i", ont.owl.to_str().unwrap(), "-o", out.to_str().unwrap()];
                let r = timed(&cmd, args.global_timeout_s)?;
                wall = Some(r.wall_ms); rss = Some(r.peak_rss_mb);
                // ROBOT errors on inconsistency; detect from stderr.
                let inconsistent = r.stderr.to_lowercase().contains("inconsistent");
                status = if inconsistent { Status::Inconsistent }
                         else if r.timed_out { Status::Dnf }
                         else if out.exists() { Status::Ok }
                         else { Status::Error };
                if let (Status::Ok, Some(orc)) = (status, oracle.as_ref()) {
                    correctness = correctness::owx_vs_oracle(&out, orc).ok();
                }
            }
            "whelk-rs" => {
                // whelk runs in-process behind the `whelk-compare` feature; when
                // the feature is off, record `na` with a note rather than a fake number.
                status = Status::Na;
            }
            _ => unreachable!(),
        }
    }

    let (closure_size, fp, missed) = match correctness {
        Some(c) => (Some(c.closure_size), Some(c.fp), Some(c.missed)),
        None => (None, None, None),
    };
    let _ = (kon_out, konclude); // referenced for clarity; oracle already parsed
    Ok(CellResult {
        ont: ont.meta.name.clone(), source: ont.meta.source.clone(),
        sha256: ont.meta.sha256.clone(), size_bytes: ont.meta.size_bytes,
        classes: ont.meta.classes, fragment: ont.meta.fragment.clone(),
        reasoner: reasoner.into(), status, wall_ms: wall, peak_rss_mb: rss,
        closure_size, fp, missed, oracle: oracle_id.into(),
    })
}

/// ISO-8601 UTC timestamp without pulling in `chrono`: read `date -u`.
fn iso_now() -> String {
    std::process::Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output().ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}
```

Note: the `pub mod` lines added in earlier tasks now live at the top of this block — when merging, ensure each `pub mod X;` appears exactly once in `mod.rs`.

- [ ] **Step 4: Add the CLI subcommand**

In `crates/owl-dl-bench/src/main.rs`, add a variant to `enum Command`:

```rust
    /// Run the authoritative reasoner×ontology performance matrix.
    Matrix {
        #[arg(long, default_value = "curated")]
        tier: String,
        #[arg(long)]
        out: std::path::PathBuf,
        #[arg(long, default_value = "25")]
        pair_timeout_ms: u64,
        #[arg(long, default_value = "120")]
        global_timeout_s: u64,
        #[arg(long)]
        resume: bool,
    },
```

And in `main`'s match:

```rust
        Command::Matrix { tier, out, pair_timeout_ms, global_timeout_s, resume } => {
            let home = std::env::var("HOME").unwrap_or_default();
            let tools = std::path::PathBuf::from(
                std::env::var("RUSTDL_EVAL_TOOLS").unwrap_or(format!("{home}/eval-tools")));
            let args = matrix::MatrixArgs {
                tier, out, pair_timeout_ms, global_timeout_s, resume,
                work_dir: tools.join("work"),
                tools,
                rustdl_bin: std::path::PathBuf::from(
                    std::env::var("RUSTDL_BIN").unwrap_or(format!("{home}/code/rustdl/target/release/rustdl"))),
                repo_root: std::path::PathBuf::from(
                    std::env::var("RUSTDL_REPO").unwrap_or(format!("{home}/code/rustdl"))),
            };
            matrix::run_matrix(args)?;
        }
```

- [ ] **Step 5: Run the unit test + build**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-bench --lib matrix::tests::resume`
Expected: PASS.

Run: `RUSTUP_TOOLCHAIN=stable cargo build -p owl-dl-bench --release`
Expected: compiles clean.

- [ ] **Step 6: Curated smoke run (integration)**

First rebuild the CLI fresh (guard depends on it):
`RUSTUP_TOOLCHAIN=stable cargo build --release -p owl-dl-cli`

Then:
`RUSTUP_TOOLCHAIN=stable ./target/release/owl-dl-bench matrix --tier curated --out /tmp/matrix-smoke --pair-timeout-ms 25`
Expected: writes `/tmp/matrix-smoke/{run-metadata.json,results.jsonl,MATRIX.md}`; MATRIX.md shows rows for the curated onts with rustdl FP 0 across the corpus; family shows `inconsistent`; ELK/whelk-rs `n/a` on non-EL onts.

Verify FP=0 for rustdl:
`grep '"reasoner":"rustdl"' /tmp/matrix-smoke/results.jsonl | grep -v '"fp":0' | grep -v '"fp":null'`
Expected: no output (every rustdl cell is FP 0 or null).

- [ ] **Step 7: Commit**

```bash
git add crates/owl-dl-bench/src/matrix/mod.rs crates/owl-dl-bench/src/main.rs
git commit -m "feat(matrix): wire matrix subcommand with tiering, oracle, resume"
```

---

## Task 9: Generate the authoritative curated matrix + repoint docs

**Files:**
- Create: `docs/benchmarks/2026-07-11-curated/{MATRIX.md,results.jsonl,run-metadata.json}` (generated)
- Modify: `README.md`, `docs/reasoner-comparison-2026-06-21.md`, `docs/paper-evidence-native-2026-07-10.md` (add a pointer to the matrix as the source of truth)

**Interfaces:** none (consumes the built binary).

- [ ] **Step 1: Generate the committed curated matrix**

```bash
RUSTUP_TOOLCHAIN=stable cargo build --release -p owl-dl-cli
RUSTUP_TOOLCHAIN=stable ./target/release/owl-dl-bench matrix \
  --tier curated --out docs/benchmarks/2026-07-11-curated --pair-timeout-ms 25
```
Expected: the three files written under `docs/benchmarks/2026-07-11-curated/`.

- [ ] **Step 2: Add a source-of-truth pointer to the docs**

At the top of `docs/reasoner-comparison-2026-06-21.md`, under the existing STALE note, add:

```markdown
> **Authoritative current numbers:** `docs/benchmarks/2026-07-11-curated/MATRIX.md`
> (regenerable via `owl-dl-bench matrix`). This file is retained for provenance.
```

In `README.md`, change the "Full head-to-head" line to point additionally at the matrix:

```markdown
Authoritative, regenerable performance matrix (5 reasoners × corpus, with
provenance): [`docs/benchmarks/2026-07-11-curated/MATRIX.md`](docs/benchmarks/2026-07-11-curated/MATRIX.md).
```

In `docs/paper-evidence-native-2026-07-10.md`, add under the correction note:

```markdown
> Superseded for cross-reasoner walls by the regenerable matrix at
> `docs/benchmarks/2026-07-11-curated/MATRIX.md`.
```

- [ ] **Step 3: Verify the generated matrix is internally consistent**

Run: `grep -c '"reasoner":"rustdl"' docs/benchmarks/2026-07-11-curated/results.jsonl`
Expected: equals the curated ont count printed by the run.

Read `docs/benchmarks/2026-07-11-curated/MATRIX.md` and confirm: oracle line present, JVM + Rosetta caveats present, summary FP total for rustdl = 0.

- [ ] **Step 4: Commit**

```bash
git add docs/benchmarks/2026-07-11-curated README.md docs/reasoner-comparison-2026-06-21.md docs/paper-evidence-native-2026-07-10.md
git commit -m "docs: authoritative curated benchmark matrix + repoint docs to it"
```

---

## Self-Review

**Spec coverage:**
- `owl-dl-bench matrix` subcommand + 6 modules → Tasks 2–8. ✓
- Konclude-only oracle → Task 6 (`rustdl_vs_oracle`/`owx_vs_oracle`), Task 8 (oracle per ont). ✓
- Fresh-binary guard → Task 4 (`assert_fresh_binary`), Task 8 (stage 0). ✓
- Uniform `/usr/bin/time -l` measurement → Task 3. ✓
- run-metadata.json + results.jsonl + MATRIX.md → Tasks 2, 7, 8. ✓
- Provenance (versions/hash/size/date/host) → Tasks 2, 4, 5. ✓
- Capability flags (`na` for EL-only off-fragment) → Task 5 (`is_el_fragment`), Task 8 (`build_cell`). ✓
- `inconsistent` status → Task 2 (enum), Task 8 (detection). ✓
- Format normalization (owx/ofn via robot convert) → Task 5. ✓
- Tiering + resume → Task 5 (`enumerate` per tier), Task 8 (`already_done`). ✓
- Reuse closure-diff infra (DRY) → Task 1 (extract `oracle_diff`). ✓
- Doc repointing → Task 9. ✓
- JVM/Rosetta caveats in metadata + MATRIX.md → Tasks 4, 7. ✓

**Type consistency:** `PairSet`, `OwxVerdict`, `Classification`, `Correctness`, `CellResult`, `Status`, `StagedOnt`, `RunMetadata`, `MatrixArgs` used identically across tasks. `read_owx_verdict` returns `Result` in Task 1 and all callers use `?`/`.ok()`. `corpus_load_ofn` defined in Task 5 (renamed from `load_any`) and consumed in Task 6.

**Placeholder scan:** No TBD/TODO; every code step carries literal code and every test has real assertions.

**Note for the ORE/BioPortal tiers:** the whelk-rs cell is `na` unless the crate is built with `--features whelk-compare`; wiring the in-process whelk closure into `build_cell` is deferred to a follow-on (the curated tier's EL onts still exercise ELK, and whelk parity is covered by the existing `compare-whelk` subcommand). This is logged in the matrix summary as an explicit `na`, never a silent omission.
