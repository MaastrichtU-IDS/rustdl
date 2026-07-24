# rustdl `--json` output mode — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a machine-readable `--json` output mode to rustdl's `classify`, `consistent`, and `realize` CLI subcommands — the stable bridge contract the Protégé plugin (and any tool) parses.

**Architecture:** A new `json_out` module in the CLI crate defines `#[derive(Serialize)]` structs and pure `build_*` functions that convert the reasoner's `Classification`/`Realization` into deterministic (sorted) JSON. The three command handlers gain a `--json` flag that, when set, prints the JSON object to stdout (all diagnostics stay on stderr) instead of the existing human-readable text.

**Tech Stack:** Rust, `serde`/`serde_json` (already workspace deps), `clap` (existing CLI), the `owl-dl-reasoner` crate.

**Plan A of 3** for the Protégé-plugin spec (`docs/superpowers/specs/2026-07-24-protege-plugin-design.md` §3–4). Plans B (cross-platform CLI-binary CI) and C (the Java plugin) follow and depend on this.

## Global Constraints

- **Build/test with `RUSTUP_TOOLCHAIN=stable`** — the pinned `1.95.0` toolchain has no `cargo`; a bare `cargo` fails or silently reuses a stale binary. Every `cargo` command below is prefixed accordingly.
- **CI denies warnings** (`RUSTFLAGS: -D warnings`, clippy pedantic on): all new code must be `cargo fmt`-clean and `cargo clippy --all-targets -- -D warnings`-clean (watch `doc_markdown` — backtick terms like `ABox`).
- **Deterministic output:** every JSON array (classes, subsumption pairs, equivalence groups, unsat list, individual types) MUST be **sorted** (byte order on the IRI strings; pairs sorted by `(sub, sup)`; each equivalence group sorted internally, groups sorted by first member). This makes the JSON stable for golden tests and for the plugin's diffing.
- **`schema_version` = 1** on every JSON object — the versioned contract with the plugin.
- **No behavior change to the existing text output.** `--json` is purely additive; without it, output is byte-identical to today.
- **Verdict fidelity:** the JSON must carry exactly what the reasoner computed (no re-derivation) — including the `incomplete` flag when class pairs timed out.

## File Structure

- **Create** `crates/owl-dl-cli/src/json_out.rs` — the `Serialize` structs (`ClassifyJson`, `ConsistentJson`, `RealizeJson` + row types) and the pure `build_classify_json` / `build_consistent_json` / `build_realize_json` functions. One responsibility: reasoner result → JSON value. No I/O, no clap.
- **Modify** `crates/owl-dl-cli/src/main.rs` — declare `mod json_out;`; add a `#[arg(long)] json: bool` to the `Classify`, `Consistent`, `Realize` command variants; in each handler, branch on `json` to print `serde_json::to_string_pretty(&build_*_json(…))` to stdout.
- **Modify** `crates/owl-dl-cli/Cargo.toml` — add `serde`, `serde_json` (workspace deps).
- **Create** `crates/owl-dl-cli/tests/json_output.rs` — end-to-end golden tests invoking the built binary on fixtures.
- **Create** fixtures `crates/owl-dl-cli/tests/fixtures/json/{consistent_tiny,inconsistent_tiny,abox_tiny}.ofn`.

---

### Task 1: `json_out` module — structs + build functions (pure, unit-tested)

**Files:**
- Modify: `crates/owl-dl-cli/Cargo.toml`
- Create: `crates/owl-dl-cli/src/json_out.rs`
- Modify: `crates/owl-dl-cli/src/main.rs` (add `mod json_out;` near the other `mod` lines)

**Interfaces:**
- Consumes: `owl_dl_reasoner::Classification` (methods `classes() -> &[String]`, `direct_subsumers(&str) -> Vec<&str>`, `equivalent_classes(&str) -> Vec<&str>`, `unsatisfiable_classes() -> Vec<&str>`, `stats() -> ClassificationStats` with `.inconsistent: bool`, `.timed_out_pairs: usize`); `owl_dl_reasoner::Realization` (methods `individuals() -> &[String]`, `entailed_types(&str) -> &[String]`, `most_specific_types(&str) -> &[String]`).
- Produces (used by Task 2): `json_out::build_classify_json(&Classification) -> ClassifyJson`, `json_out::build_consistent_json(bool) -> ConsistentJson`, `json_out::build_realize_json(&Realization) -> RealizeJson`; all three types `#[derive(serde::Serialize)]`.

- [ ] **Step 1: Add serde deps.** In `crates/owl-dl-cli/Cargo.toml`, under `[dependencies]`, add:

```toml
serde = { workspace = true }
serde_json = { workspace = true }
```

- [ ] **Step 2: Write the failing unit test** (append to a new `#[cfg(test)] mod tests` at the bottom of `crates/owl-dl-cli/src/json_out.rs` — create the file with just this test first):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use horned_owl::io::ofn::reader::read as read_ofn;
    use horned_owl::io::ParserConfiguration;
    use horned_owl::model::RcStr;
    use horned_owl::ontology::set::SetOntology;
    use std::io::Cursor;

    fn classify_ofn(src: &str) -> owl_dl_reasoner::Classification {
        let (onto, _): (SetOntology<RcStr>, _) =
            read_ofn(&mut Cursor::new(src.to_owned()), ParserConfiguration::default()).unwrap();
        owl_dl_reasoner::classify(&onto).unwrap()
    }

    #[test]
    fn classify_json_is_sorted_and_carries_verdict() {
        // B ⊑ A, C ⊑ B ⇒ direct B⊑A, C⊑B; consistent; nothing unsat.
        let h = classify_ofn(
            r"Prefix(:=<http://ex/#>)
              Ontology(<http://ex/>
                Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))
                SubClassOf(:B :A) SubClassOf(:C :B))",
        );
        let j = build_classify_json(&h);
        assert_eq!(j.schema_version, 1);
        assert!(j.consistent);
        assert!(!j.incomplete);
        assert!(j.unsatisfiable.is_empty());
        // direct edges present, sorted by (sub, sup):
        assert!(j.direct_subsumptions.contains(&[
            "http://ex/#B".to_owned(),
            "http://ex/#A".to_owned()
        ]));
        assert!(j.direct_subsumptions.contains(&[
            "http://ex/#C".to_owned(),
            "http://ex/#B".to_owned()
        ]));
        // sorted invariant:
        let mut sorted = j.direct_subsumptions.clone();
        sorted.sort();
        assert_eq!(j.direct_subsumptions, sorted);
    }
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-cli --lib json_out 2>&1 | tail -20`
Expected: FAIL — `build_classify_json` / `ClassifyJson` not found.

- [ ] **Step 4: Write the module** (prepend above the `mod tests`, in `crates/owl-dl-cli/src/json_out.rs`):

```rust
//! Machine-readable JSON output for the CLI (`--json`). The stable bridge
//! contract consumed by the Protégé plugin. All arrays are sorted for
//! determinism; `schema_version` guards drift.
use owl_dl_reasoner::{Classification, Realization};
use serde::Serialize;

const SCHEMA_VERSION: u32 = 1;

#[derive(Serialize)]
pub struct ClassifyJson {
    pub schema_version: u32,
    pub consistent: bool,
    pub incomplete: bool,
    pub unsatisfiable: Vec<String>,
    pub equivalent_groups: Vec<Vec<String>>,
    pub direct_subsumptions: Vec<[String; 2]>,
}

#[derive(Serialize)]
pub struct ConsistentJson {
    pub schema_version: u32,
    pub consistent: bool,
}

#[derive(Serialize)]
pub struct IndividualTypesJson {
    pub iri: String,
    pub types: Vec<String>,
    pub direct_types: Vec<String>,
}

#[derive(Serialize)]
pub struct RealizeJson {
    pub schema_version: u32,
    pub individuals: Vec<IndividualTypesJson>,
}

#[must_use]
pub fn build_classify_json(h: &Classification) -> ClassifyJson {
    let stats = h.stats();

    let mut unsatisfiable: Vec<String> =
        h.unsatisfiable_classes().into_iter().map(str::to_owned).collect();
    unsatisfiable.sort();

    // Equivalence groups: for each class, its equivalence peers; canonicalise
    // by sorting each group and deduping groups (a group is emitted once).
    let mut groups: Vec<Vec<String>> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for c in h.classes() {
        if seen.contains(c) {
            continue;
        }
        let mut group: Vec<String> = h.equivalent_classes(c).into_iter().map(str::to_owned).collect();
        if !group.iter().any(|g| g == c) {
            group.push(c.clone());
        }
        group.sort();
        group.dedup();
        for g in &group {
            seen.insert(g.clone());
        }
        if group.len() > 1 {
            groups.push(group);
        }
    }
    groups.sort();

    let mut direct_subsumptions: Vec<[String; 2]> = Vec::new();
    for c in h.classes() {
        for sup in h.direct_subsumers(c) {
            direct_subsumptions.push([c.clone(), sup.to_owned()]);
        }
    }
    direct_subsumptions.sort();
    direct_subsumptions.dedup();

    ClassifyJson {
        schema_version: SCHEMA_VERSION,
        consistent: !stats.inconsistent,
        incomplete: stats.timed_out_pairs > 0,
        unsatisfiable,
        equivalent_groups: groups,
        direct_subsumptions,
    }
}

#[must_use]
pub fn build_consistent_json(consistent: bool) -> ConsistentJson {
    ConsistentJson { schema_version: SCHEMA_VERSION, consistent }
}

#[must_use]
pub fn build_realize_json(r: &Realization) -> RealizeJson {
    let mut individuals: Vec<IndividualTypesJson> = r
        .individuals()
        .iter()
        .map(|ind| {
            let mut types: Vec<String> = r.entailed_types(ind).iter().map(str::to_owned).collect();
            types.sort();
            let mut direct_types: Vec<String> =
                r.most_specific_types(ind).iter().map(str::to_owned).collect();
            direct_types.sort();
            IndividualTypesJson { iri: ind.clone(), types, direct_types }
        })
        .collect();
    individuals.sort_by(|a, b| a.iri.cmp(&b.iri));
    RealizeJson { schema_version: SCHEMA_VERSION, individuals }
}
```

Also add `mod json_out;` to `crates/owl-dl-cli/src/main.rs` (next to the other top-level `mod`/`use` declarations).

- [ ] **Step 5: Run test to verify it passes**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-cli --lib json_out 2>&1 | tail -20`
Expected: PASS (1 test).

- [ ] **Step 6: fmt + clippy**

Run: `RUSTUP_TOOLCHAIN=stable cargo fmt -p owl-dl-cli && RUSTUP_TOOLCHAIN=stable cargo clippy -p owl-dl-cli --all-targets -- -D warnings 2>&1 | tail -3`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add crates/owl-dl-cli/Cargo.toml crates/owl-dl-cli/src/json_out.rs crates/owl-dl-cli/src/main.rs
git commit -m "feat(cli): json_out module — classify/consistent/realize JSON builders"
```

---

### Task 2: wire `--json` into the three command handlers

**Files:**
- Modify: `crates/owl-dl-cli/src/main.rs` — `Classify`/`Consistent`/`Realize` variants (~lines 51, 78, 174) + their handlers (`Command::Consistent` ~712, `Command::Classify` ~745, `Command::Realize` ~825).

**Interfaces:**
- Consumes: `json_out::{build_classify_json, build_consistent_json, build_realize_json}` from Task 1.
- Produces: the three subcommands accept `--json`; when set they print one JSON object to stdout via `serde_json::to_string_pretty(...)` and suppress the text writer. Existing text output unchanged when the flag is absent.

- [ ] **Step 1: Add `--json` to the three command variants.** In each of `Consistent`, `Classify`, `Realize` in the `enum Command`, add:

```rust
        /// Emit a single machine-readable JSON object on stdout (schema v1);
        /// diagnostics stay on stderr. The stable contract for tooling.
        #[arg(long)]
        json: bool,
```

- [ ] **Step 2: Write the failing test** (append to `crates/owl-dl-cli/tests/json_output.rs`, create the file):

```rust
use std::process::Command;

fn rustdl() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rustdl"))
}

fn tiny_consistent() -> &'static str {
    "crates/owl-dl-cli/tests/fixtures/json/consistent_tiny.ofn"
}

#[test]
fn classify_json_parses_and_reports_consistent() {
    let out = rustdl().args(["classify", "--json", tiny_consistent()]).output().unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("stdout is valid JSON");
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["consistent"], true);
    assert!(v["direct_subsumptions"].is_array());
}
```

Add `serde_json = { workspace = true }` to `crates/owl-dl-cli/Cargo.toml` `[dev-dependencies]` if not already present.

- [ ] **Step 3: Create the fixture** `crates/owl-dl-cli/tests/fixtures/json/consistent_tiny.ofn`:

```
Prefix(:=<http://ex/#>)
Ontology(<http://ex/>
  Declaration(Class(:A)) Declaration(Class(:B))
  SubClassOf(:B :A))
```

- [ ] **Step 4: Run test to verify it fails**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-cli --test json_output classify_json_parses 2>&1 | tail -20`
Expected: FAIL — `--json` is an unknown argument (non-zero exit) OR no JSON on stdout.

- [ ] **Step 5: Implement the handler branches.** In `Command::Classify { file, … , json }`, after the `Classification` is computed (the handler already builds `h`), before `write_classification`/`print_classification`:

```rust
            if json {
                println!("{}", serde_json::to_string_pretty(&json_out::build_classify_json(&h))?);
                return Ok(());
            }
```

In `Command::Consistent { file, json }`, after the boolean `consistent` verdict is computed:

```rust
            if json {
                println!("{}", serde_json::to_string_pretty(&json_out::build_consistent_json(consistent))?);
                return Ok(());
            }
```

In `Command::Realize { file, saturation_only, properties, json }`, after the `Realization` `r` is computed:

```rust
            if json {
                println!("{}", serde_json::to_string_pretty(&json_out::build_realize_json(&r))?);
                return Ok(());
            }
```

(Match the exact local variable names already in each handler — `h` for the classification, the consistency bool, `r`/`realization` for realize. Adjust the identifier if the handler uses a different name.)

- [ ] **Step 6: Run test to verify it passes**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-cli --test json_output classify_json_parses 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 7: fmt + clippy**

Run: `RUSTUP_TOOLCHAIN=stable cargo fmt -p owl-dl-cli && RUSTUP_TOOLCHAIN=stable cargo clippy -p owl-dl-cli --all-targets -- -D warnings 2>&1 | tail -3`
Expected: clean.

- [ ] **Step 8: Commit**

```bash
git add crates/owl-dl-cli/src/main.rs crates/owl-dl-cli/Cargo.toml crates/owl-dl-cli/tests/
git commit -m "feat(cli): --json flag on classify/consistent/realize"
```

---

### Task 3: golden end-to-end tests (inconsistent + realize) + schema doc

**Files:**
- Modify: `crates/owl-dl-cli/tests/json_output.rs`
- Create: `crates/owl-dl-cli/tests/fixtures/json/{inconsistent_tiny,abox_tiny}.ofn`
- Create: `docs/json-schema.md` (the versioned contract, for the plugin author).

**Interfaces:**
- Consumes: the `rustdl` binary with `--json` (Task 2).
- Produces: regression coverage for the inconsistent verdict and the realize shape; a documented schema.

- [ ] **Step 1: Create fixtures.**
`crates/owl-dl-cli/tests/fixtures/json/inconsistent_tiny.ofn`:

```
Prefix(:=<http://ex/#>)
Ontology(<http://ex/>
  Declaration(Class(:A)) Declaration(Class(:B)) Declaration(NamedIndividual(:x))
  DisjointClasses(:A :B) ClassAssertion(:A :x) ClassAssertion(:B :x))
```

`crates/owl-dl-cli/tests/fixtures/json/abox_tiny.ofn`:

```
Prefix(:=<http://ex/#>)
Ontology(<http://ex/>
  Declaration(Class(:A)) Declaration(Class(:B)) Declaration(NamedIndividual(:x))
  SubClassOf(:B :A) ClassAssertion(:B :x))
```

- [ ] **Step 2: Write the failing tests** (append to `crates/owl-dl-cli/tests/json_output.rs`):

```rust
#[test]
fn consistent_json_reports_inconsistent() {
    let out = rustdl()
        .args(["consistent", "--json", "crates/owl-dl-cli/tests/fixtures/json/inconsistent_tiny.ofn"])
        .output().unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["consistent"], false);
}

#[test]
fn realize_json_reports_types() {
    let out = rustdl()
        .args(["realize", "--json", "crates/owl-dl-cli/tests/fixtures/json/abox_tiny.ofn"])
        .output().unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let inds = v["individuals"].as_array().unwrap();
    let x = inds.iter().find(|i| i["iri"] == "http://ex/#x").expect("x realized");
    let types: Vec<&str> = x["types"].as_array().unwrap().iter().map(|t| t.as_str().unwrap()).collect();
    assert!(types.contains(&"http://ex/#A"));
    assert!(types.contains(&"http://ex/#B"));
}
```

- [ ] **Step 3: Run to verify they pass** (the impl already exists from Tasks 1–2):

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-cli --test json_output 2>&1 | tail -20`
Expected: PASS (all json_output tests). If `realize_json_reports_types` fails because `x`'s inferred types don't include `A`, that indicates a realize gap, not a JSON bug — capture the actual output and report before adjusting the assertion.

- [ ] **Step 4: Write the schema doc** `docs/json-schema.md`:

```markdown
# rustdl `--json` output schema (v1)

Consumed by the Protégé plugin. Every object carries `"schema_version": 1`.
All arrays are sorted (byte order); pairs `[sub, sup]`.

## `classify --json`
{ "schema_version": 1, "consistent": bool, "incomplete": bool,
  "unsatisfiable": [iri], "equivalent_groups": [[iri, …]],
  "direct_subsumptions": [[sub_iri, sup_iri], …] }

`incomplete` = some class pair hit the time budget (defaulted to not-subsumed);
the hierarchy is sound (no false subsumptions) but may miss real ones.

## `consistent --json`
{ "schema_version": 1, "consistent": bool }

## `realize --json`
{ "schema_version": 1,
  "individuals": [ { "iri": iri, "types": [iri], "direct_types": [iri] } ] }
```

- [ ] **Step 5: Commit**

```bash
git add crates/owl-dl-cli/tests/ docs/json-schema.md
git commit -m "test(cli): golden json_output tests (inconsistent, realize) + schema doc"
```

---

## Self-Review

- **Spec coverage (§4 of the design):** `classify --json` (Task 1/2, schema in Task 3), `consistent --json` (Task 1/2), `realize --json` (Task 1/2), `schema_version`+sorted determinism (Global Constraints + Task 1), `incomplete` flag (Task 1, `timed_out_pairs`). The v1.x extras (`sat`/`instance`/`justify`/`repair`/`diagnose --json`) are explicitly out of this plan (spec §4). ✓
- **Placeholders:** none — every step shows the code/command.
- **Type consistency:** `build_classify_json`/`build_consistent_json`/`build_realize_json` names + signatures are identical in Task 1 (definition) and Task 2 (use); `ClassifyJson`/`ConsistentJson`/`RealizeJson`/`IndividualTypesJson` fields match the schema doc and the tests.
- **Open dependency the implementer must verify at Step 2.5:** the exact local-variable names in the three existing handlers (`h`, the consistency bool, `r`). If a handler currently computes the value inside a `println!`/`write_*` call rather than binding it, bind it to a local first, then add the `if json { … }` branch. This is the one place to read the surrounding handler code.
