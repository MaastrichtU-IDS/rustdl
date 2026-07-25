# Surface Dropped Axioms + Graceful Degradation (#43) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Make `convert_ontology` degrade gracefully — drop+record unsupported axioms (instead of silently dropping data ranges / hard-refusing other unsupported constructs) — and surface a `DroppedAxioms` diagnostic on the reasoner API + CLI `--json`/stderr + Python.

**Architecture:** `ce_or_skip!` stops swallowing `UnsupportedDataRange`; `convert_ontology`'s loop catches every `Err` (records `kind→count` on a new `InternalOntology.dropped`) and continues instead of `?`-aborting. A standalone `dropped_axioms(&onto)` reasoner fn exposes the map; the CLI/Python surface it. Dropping is a sound under-approximation; empty map ⇒ byte-identical behavior.

**Tech Stack:** Rust (edition 2024), horned-owl `Component`, clap/serde_json, PyO3.

## Global Constraints
- Build/test `RUSTUP_TOOLCHAIN=stable cargo …` (pinned 1.95.0 often lacks cargo). Clippy pedantic `-D warnings`; `unwrap`/`dbg` only under `#[cfg(test)]`; rustfmt max_width=100 (`cargo fmt --all`); raw string w/o inner `#`/`"` → `r"..."`.
- **Soundness:** dropping an axiom weakens the KB ⇒ sound under-approximation (miss entailments; never a false one; consistency can only miss an inconsistency). No FP risk. Fully-supported ontology ⇒ `dropped` empty ⇒ closures byte-identical to baseline (the regression gate).
- **Isolated worktree** `/Users/micheldumontier/code/rustdl-wt/surface-dropped`, branch `feat/surface-dropped-axioms-43`. A concurrent agent works elsewhere in the repo — never leave this worktree; never touch other branches/worktrees.
- `--json` `schema_version` stays `1` (`dropped` is an additive optional block). Single PR closing #43.

## File Structure
- `crates/owl-dl-core/src/convert.rs` — `ce_or_skip!` change; `convert_ontology` loop catch+record; `component_kind`/`drop_label` helpers.
- `crates/owl-dl-core/src/dropped.rs` *(new)* — `DroppedAxioms` type.
- `crates/owl-dl-core/src/ontology.rs` — `InternalOntology.dropped` field.
- `crates/owl-dl-core/src/lib.rs` — `mod dropped; pub use`.
- `crates/owl-dl-reasoner/src/lib.rs` — `pub fn dropped_axioms` + `pub use DroppedAxioms`.
- `crates/owl-dl-cli/src/{json_out.rs,main.rs}` — `dropped` block + stderr warning.
- `crates/owl-dl-py/{src/queries.rs,python/rustdl/__init__.py,__init__.pyi,tests/python/test_queries.py}` — `dropped_axioms` binding.

---

## Task 1: `DroppedAxioms` type + graceful-degradation refactor in `convert.rs`

**Files:** create `crates/owl-dl-core/src/dropped.rs`; modify `convert.rs`, `ontology.rs`, `lib.rs`. Tests: `convert.rs` `#[cfg(test)]`.

**Interfaces — Produces:**
```rust
// owl-dl-core/src/dropped.rs
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DroppedAxioms { by_kind: std::collections::BTreeMap<String, u64> }
impl DroppedAxioms {
    #[must_use] pub fn is_empty(&self) -> bool { self.by_kind.is_empty() }
    #[must_use] pub fn total(&self) -> u64 { self.by_kind.values().sum() }
    #[must_use] pub fn by_kind(&self) -> &std::collections::BTreeMap<String, u64> { &self.by_kind }
    pub fn record(&mut self, kind: String) { *self.by_kind.entry(kind).or_insert(0) += 1; }
}
```
`InternalOntology` gains `pub dropped: DroppedAxioms` (init `Default` in `InternalOntology::new`).

- [ ] **Step 1 — RED (core unit tests in `convert.rs`):**
```rust
#[test]
fn convert_records_dropped_unsupported_axiom_and_continues() {
    // An anonymous-individual assertion (ConversionError::AnonymousIndividual today
    // aborts the whole conversion). After the fix: the KB converts, the axiom is
    // recorded as dropped, and the supported axioms survive.
    let src = r#"Prefix(:=<http://ex/#>)
      Ontology(<http://ex/>
        Declaration(Class(:A)) Declaration(Class(:B)) SubClassOf(:A :B)
        ClassAssertion(:A _:anon1))"#;
    let (onto, _) = read_ofn_str(src);            // helper: read_ofn into SetOntology
    let internal = convert_ontology(&onto).expect("must not abort");
    assert!(internal.axioms.iter().any(|a| matches!(a, Axiom::SubClassOf { .. })), "supported axiom survives");
    assert_eq!(internal.dropped.total(), 1, "one dropped axiom recorded, got {:?}", internal.dropped.by_kind());
    assert!(internal.dropped.by_kind().keys().any(|k| k.contains("anonymous individual")));
}

#[test]
fn convert_records_dropped_data_range_axiom() {
    // A SubClassOf whose filler is an unsupported nested composite data range:
    // silently dropped today (ce_or_skip → Ok(None)); now recorded.
    let src = r#"Prefix(:=<http://ex/#>) Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
      Ontology(<http://ex/>
        Declaration(Class(:A)) Declaration(DataProperty(:p))
        SubClassOf(:A DataSomeValuesFrom(:p DataComplementOf(DataUnionOf(xsd:integer xsd:string)))))"#;
    let (onto, _) = read_ofn_str(src);
    let internal = convert_ontology(&onto).expect("must not abort");
    assert_eq!(internal.dropped.total(), 1);
    assert!(internal.dropped.by_kind().keys().any(|k| k.contains("data range")));
}

#[test]
fn convert_benign_drops_not_recorded() {
    // Metadata / annotations must NOT count as dropped.
    let src = r#"Prefix(:=<http://ex/#>)
      Ontology(<http://ex/>
        Declaration(Class(:A)) Declaration(Class(:B)) SubClassOf(:A :B)
        AnnotationAssertion(<http://x/lbl> :A "hi"))"#;
    let (onto, _) = read_ofn_str(src);
    let internal = convert_ontology(&onto).expect("ok");
    assert!(internal.dropped.is_empty(), "benign drops not recorded, got {:?}", internal.dropped.by_kind());
}
```
Add a `read_ofn_str` test helper if none exists (mirror other test modules' `read_ofn` usage). Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-core convert_records convert_benign`. Expected FAIL: the anon-individual case ABORTS (Err), the data-range case has no `dropped` field.

- [ ] **Step 2 — GREEN, part A: the type.** Create `dropped.rs` (above); wire `mod dropped; pub use dropped::DroppedAxioms;` in `owl-dl-core/src/lib.rs`; add `pub dropped: DroppedAxioms` to `InternalOntology` (`ontology.rs`) + init in `new()`.

- [ ] **Step 3 — GREEN, part B: stop swallowing in `ce_or_skip!`.** Change the macro so it propagates *all* errors (no special `Ok(None)` for `UnsupportedDataRange`):
```rust
macro_rules! ce_or_skip {
    ($expr:expr) => {
        match $expr { Ok(c) => c, Err(e) => return Err(e) }
    };
}
```
(Now a data-range-bearing axiom returns `Err(UnsupportedDataRange)` from `convert_component` instead of `Ok(None)`.)

- [ ] **Step 4 — GREEN, part C: catch+record in `convert_ontology`.** Replace the `?`-propagating loop body:
```rust
for ac in components {
    match convert_component(&ac.component, &mut out.vocabulary, &mut out.concepts) {
        Ok(Some(axiom)) => out.axioms.push(axiom),
        Ok(None) => {}  // benign: metadata / annotation / declaration — no reasoning content
        Err(e) => out.dropped.record(drop_label(&ac.component, &e)),
    }
}
```
Add the helpers (near `convert_component`):
```rust
/// Stable discriminant name for the axiom-carrying components that can drop.
fn component_kind<A: ForIRI>(c: &Component<A>) -> &'static str {
    use Component as C;
    match c {
        C::SubClassOf(_) => "SubClassOf",
        C::EquivalentClasses(_) => "EquivalentClasses",
        C::DisjointClasses(_) => "DisjointClasses",
        C::DisjointUnion(_) => "DisjointUnion",
        C::ClassAssertion(_) => "ClassAssertion",
        C::ObjectPropertyAssertion(_) => "ObjectPropertyAssertion",
        C::NegativeObjectPropertyAssertion(_) => "NegativeObjectPropertyAssertion",
        C::ObjectPropertyDomain(_) => "ObjectPropertyDomain",
        C::ObjectPropertyRange(_) => "ObjectPropertyRange",
        C::SubObjectPropertyOf(_) => "SubObjectPropertyOf",
        C::EquivalentObjectProperties(_) => "EquivalentObjectProperties",
        C::DisjointObjectProperties(_) => "DisjointObjectProperties",
        _ => "Other",
    }
}
/// `"<component>: <reason>"` — the diagnostic kind label.
fn drop_label<A: ForIRI>(c: &Component<A>, e: &ConversionError) -> String {
    let comp = component_kind(c);
    match e {
        ConversionError::UnsupportedDataRange => format!("{comp}: unsupported data range"),
        ConversionError::AnonymousIndividual => format!("{comp}: anonymous individual"),
        ConversionError::UnsupportedConcept { kind } => format!("{comp}: unsupported concept ({kind})"),
        ConversionError::UnsupportedAxiom { kind } => format!("{comp}: unsupported axiom ({kind})"),
    }
}
```
VERIFY against horned-owl: the exact `Component` variant names above (adjust any that differ in the pinned fork), and confirm `convert_component`'s catch-all arm for genuinely-unknown components returns `Err(ConversionError::UnsupportedAxiom{..})` (so SWRL/etc. are recorded, not panicked). If any variant name is wrong, the `_ => "Other"` fallback keeps it sound (just a coarser label).

- [ ] **Step 5 — run:** the three Step-1 tests pass; add a 4th: a fully-supported ontology ⇒ `internal.dropped.is_empty()` AND all axioms present (byte-identical axiom set to pre-change — spot check). `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-core`; clippy `-p owl-dl-core --all-targets -- -D warnings`; `cargo fmt --all`.

- [ ] **Step 6 — commit:** `feat(convert): graceful degradation — drop+record unsupported axioms (#43)`.

## Task 2: reasoner `dropped_axioms` accessor + graceful-degradation integration

**Files:** `crates/owl-dl-reasoner/src/lib.rs`. Test: `crates/owl-dl-reasoner/tests/dropped_axioms.rs` (new).

**Interfaces — Produces:**
```rust
pub use owl_dl_core::DroppedAxioms;
/// The axioms conversion could not represent (sound under-approximation).
/// # Errors
/// [`ReasonError::Conversion`] only on a genuinely fatal conversion failure
/// (unsupported constructs are recorded, not errored).
pub fn dropped_axioms<A: ForIRI>(onto: &SetOntology<A>) -> Result<DroppedAxioms, ReasonError> {
    Ok(owl_dl_core::convert::convert_ontology(onto)?.dropped)
}
```

- [ ] **Step 1 — RED:** in `tests/dropped_axioms.rs` (`#![allow(clippy::unwrap_used)]`): an ontology with an unsupported/anon axiom + a valid `SubClassOf` — assert (a) `classify(&onto)` now **succeeds** (RED: aborts today) and yields the supported subsumption, and (b) `dropped_axioms(&onto).unwrap().total() >= 1` with a kind label containing the reason. Run; confirm classify aborts today.
- [ ] **Step 2 — GREEN:** add `dropped_axioms` + `pub use DroppedAxioms` in `lib.rs`. (No other change needed — Task 1 already made `convert_ontology` degrade gracefully, so `classify`/`is_consistent`/`realize` now succeed on such inputs automatically.)
- [ ] **Step 3 — migration check:** grep `ConversionError`/`ReasonError::Conversion` in tests; confirm no ACTIVE test asserts an abort on an unsupported-construct ontology (the earlier audit found only `#[ignore]`d doc-strings + the Python error mapping — the mapping stays valid for any residual fatal `ConversionError`). Fix any that break.
- [ ] **Step 4 — run:** `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner`; clippy; fmt.
- [ ] **Step 5 — commit:** `feat(reasoner): dropped_axioms accessor + graceful degradation (#43)`.

## Task 3: CLI — stderr warning + `dropped` `--json` block

**Files:** `crates/owl-dl-cli/src/{json_out.rs,main.rs}`, `tests/json_output.rs` + a fixture.

**Interfaces:** each relevant `--json` output gains a top-level `"dropped": { "<kind>": <count>, … }` (empty object when none). A stderr warning prints when non-empty.

- [ ] **Step 1 — RED (golden test + fixture):** fixture `tests/fixtures/json/dropped_tiny.ofn` with a supported `SubClassOf(:A :B)` + one unsupported axiom (anon-individual `ClassAssertion(:A _:x)`). Test: `rustdl classify --json <fixture>` → `status.success()`, `v["dropped"]` is an object with total ≥1 (a key containing "anonymous individual"), and the supported subsumption is still present in `direct_subsumptions`. Run; FAIL (no `dropped` field; may currently error).
- [ ] **Step 2 — GREEN — json_out:** add `dropped: BTreeMap<String,u64>` to `ClassifyJson`, `ConsistentJson`, `RealizeJson` (and optionally the query structs). Add a shared helper `pub(crate) fn dropped_block(onto: &SetOntology<RcStr>) -> BTreeMap<String,u64>` calling `owl_dl_reasoner::dropped_axioms(onto).map(|d| d.by_kind().clone()).unwrap_or_default()`; thread its result into the builders (builders take the map as a param). Keep `SCHEMA_VERSION = 1`.
- [ ] **Step 3 — GREEN — main dispatch:** in each `--json` arm compute `let dropped = json_out::dropped_block(&onto);` once, pass to the builder, and after printing (JSON path returns before the human path) — for the non-JSON path, print a stderr warning helper `warn_if_dropped(&dropped)` = when non-empty, `eprintln!("warning: {} axiom(s) not understood and dropped ({}); results are a sound under-approximation", total, kinds_summary)`. For the `--json` path the `dropped` block IS the signal (no stderr needed, keeps stdout clean); for human output, emit the stderr warning. (One extra `convert_ontology` per invocation — negligible vs reasoning; accepted trade-off, noted.)
- [ ] **Step 4 — run:** `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-cli --test json_output`; clippy `-p owl-dl-cli --all-targets -- -D warnings`; fmt.
- [ ] **Step 5 — commit:** `feat(cli): surface dropped axioms (stderr + --json dropped block) (#43)`.

## Task 4: Python — `dropped_axioms` binding

**Files:** `crates/owl-dl-py/src/queries.rs`, `python/rustdl/__init__.py`, `__init__.pyi`, `tests/python/test_queries.py`.

**Interfaces:** `dropped_axioms(path) -> dict[str, int]`.

- [ ] **Step 1 — RED:** test: an ontology with an anon-individual axiom ⇒ `rustdl.dropped_axioms(path)` returns a non-empty dict whose keys mention the reason, and `rustdl.classify`/`rustdl.consistent` on it does NOT raise. Run (`maturin develop` then pytest); FAIL (`AttributeError`).
- [ ] **Step 2 — GREEN:** add `#[pyfunction] pub(crate) fn dropped_axioms(path: &str) -> PyResult<std::collections::BTreeMap<String,u64>>` in `queries.rs` (load via `crate::load::load_path`, call `owl_dl_reasoner::dropped_axioms(&o)`, map err via `crate::errors::reason_error_to_py`, return `d.by_kind().clone()`). Register it. Add the `name as name` re-export + `__all__` entry in `__init__.py`; add a `.pyi` stub `def dropped_axioms(path: str) -> dict[str, int]: ...` under a `# ── conversion diagnostics ──` header.
- [ ] **Step 3 — run:** `cd crates/owl-dl-py && export VIRTUAL_ENV="$PWD/.venv" PATH="$PWD/.venv/bin:$PATH" RUSTUP_TOOLCHAIN=stable && maturin develop && python -m pytest tests/python/test_queries.py tests/python/test_stubs.py -q`; `cargo clippy -p owl-dl-py --all-targets -- -D warnings`; fmt.
- [ ] **Step 4 — commit:** `feat(python): dropped_axioms accessor (#43)`.

## Task 5: validation + close #43
- [ ] **Step 1 — full suite:** `RUSTUP_TOOLCHAIN=stable cargo test --workspace --exclude owl-dl-py`; workspace clippy `--all-targets --all-features -- -D warnings`; Python suite.
- [ ] **Step 2 — no-regression / byte-identical gate:** classify `pizza` + `sulo` (both fully-supported, no unsupported constructs) pre-vs-post (temporarily on `origin/main`'s convert or via a stash) and confirm the classify closures are **byte-identical** AND `dropped` is empty — proving the refactor is inert where nothing is dropped. Spot-check one ontology WITH an unsupported construct (e.g. a fixture with a SWRL rule if available) reasons + reports the drop.
- [ ] **Step 3 — PR** closing #43: describe the graceful-degradation contract change, the diagnostic surface, the migration (no active test asserted the old abort), and the byte-identical-when-empty guarantee.

---

## Self-Review

**1. Spec coverage:** §3 refactor → Task 1 (Steps 3-4). §4 type → Task 1 Step 2. §5.1 reasoner accessor → Task 2. §5.2 CLI → Task 3. §5.3 Python → Task 4. §6 testing → Tasks 1/3/4 canaries + Task 5 gate. §7 migration → Task 2 Step 3. §8 non-goals (no --strict) respected. ✓

**2. Placeholder scan:** No TBD. The two "VERIFY against horned-owl" notes (Component variant names; the convert_component catch-all reason) name exactly what to confirm with a safe `_ => "Other"` fallback — not blanks.

**3. Type consistency:** `DroppedAxioms::{is_empty,total,by_kind,record}` used identically across core→reasoner→CLI→Python. `dropped_axioms(&onto) -> Result<DroppedAxioms, ReasonError>` (Rust) / `dropped_axioms(path) -> dict[str,int]` (Python) / `dropped_block(&onto) -> BTreeMap<String,u64>` (CLI) consistent. `component_kind`/`drop_label` defined in Task 1, used only there. `InternalOntology.dropped` set in Task 1, read by `dropped_axioms` in Task 2.

**Decisions locked (from spec's open items):** standalone `dropped_axioms` fn (NOT threaded through `ClassificationStats` — leaner; one extra cheap convert accepted); Python accessor-only (no `DroppedAxiomsWarning` — the CLI carries the stderr warning, Python users call the accessor).
