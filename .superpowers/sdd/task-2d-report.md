# Task 2d — Review Finding Implementation Report

Branch: `feat/conjunctive-unsat-negation-gci`

---

## Finding 1 — `classify --json` self-contradiction fix

### Problem
`classify_pure_el` built `ClassificationStats` with `inconsistent = false` even when
the KB contained `⊤ ⊑ ⊥`. The saturation engine correctly set `global_unsat` and
enqueued every user class as unsatisfiable, so the `unsatisfiable` list was populated —
but `stats.inconsistent` stayed false. Result: `classify --json` emitted
`"consistent": true` alongside a non-empty `"unsatisfiable"` list.

### Fix
Added `globally_inconsistent: bool` to `Subsumers` (in `crates/owl-dl-saturation/src/lib.rs`),
set from `engine.rules.global_unsat` after each saturation run:

```rust
// in saturate_with_config, saturate_with_exists_facts, saturate_for_realize:
engine.subsumers.globally_inconsistent = engine.rules.global_unsat;
```

Added accessor:
```rust
pub fn globally_inconsistent(&self) -> bool { self.globally_inconsistent }
```

In `classify_pure_el` (`crates/owl-dl-reasoner/src/classify.rs`), before Pass 1:
```rust
if closure.globally_inconsistent() {
    stats.inconsistent = true;
}
```

**Control-flow safety check:** Checked all three readers of `stats.inconsistent`:
- `json_out.rs:194` — `consistent: !stats.inconsistent` → now correctly emits `false`
- `main.rs:712` — changes `# abox_check:` label from `unknown` to `inconsistent` (cosmetic)
- `diagnose.rs:89` — already has a separate guard at line 99–106 for this case; setting
  the flag makes it take the cleaner early-exit instead of the slower fallback

The unsatisfiable list is NOT gated on `!inconsistent`, so it remains populated.

Also added `#[allow(clippy::struct_field_names)]` to suppress the pre-existing
`subsumers: IdMatrix` field-name lint that became visible once `pedantic` hit this struct.

### Test added
`top_bot_classify_stats_inconsistent` in `crates/owl-dl-reasoner/tests/conjunctive_unsat.rs`:
asserts `stats.inconsistent == true` and `unsatisfiable_classes()` is non-empty on a
`⊤ ⊑ ⊥` KB.

---

## Finding 2 — canary for the newly-activated data-clash path

### Problem
`data_axioms.rs` emits `SubClassOf(owl:Thing, owl:Nothing)` at 7 sites when a
data-range violation is detected. Before the `⊤ ⊑ ⊥` fix, the saturator silently
dropped this axiom while certifying the closure complete. Now `global_unsat` fires and
every user class is reported unsatisfiable. No test pinned this behaviour.

### Fix
Added canary `data_range_violation_marks_classes_unsat` to
`crates/owl-dl-reasoner/tests/datatype_inconsistency.rs`.

Fixture: `DataPropertyRange(:p xsd:integer)` + `DataPropertyAssertion(:p :i "foo")` +
`SubClassOf(:A :B)`. Before the fix: `classify` reported `:A ⊑ :B` with an empty
unsat list (silent inconsistency). After the fix: `A` and `B` are both reported
unsatisfiable.

Also added `classify` to the import line (was `is_consistent` only).

---

## Finding 3 — stronger FP guards for the `⊤ ⊑ ⊥` fix

### Added tests (append-only to `conjunctive_unsat.rs`)

**`subclass_of_thing_a_stays_sat`**: `SubClassOf(owl:Thing, :A)` takes the
`top_subsumers` path and must NOT trigger `global_unsat`. Zero unsatisfiable classes.

**`scoped_bot_does_not_globalise`**: `SubClassOf(:A, owl:Nothing)` + `SubClassOf(:C, :A)`:
only `A` and `C` are unsat; unrelated declared `B` stays satisfiable. Pins that the
confined-bot path doesn't globalise.

Both tests passed immediately (reviewer confirmed the code was already correct).

---

## Finding 4 (minor) — two cleanups

### 4a: `realize` returns empty instead of `Err(Inconsistent)` on `⊤ ⊑ ⊥`
`abox_saturation::saturate_abox_consistency` handles only atomic-LHS `SubClassOf`;
it cannot detect `SubClassOf(owl:Thing, owl:Nothing)`. So `realize_internal` fell
through to the saturation path and returned an empty realization instead of
`Err(ReasonError::Inconsistent)`.

Fix: added helper `has_top_subclass_bot` in `crates/owl-dl-reasoner/src/realize.rs`:
```rust
fn has_top_subclass_bot(internal: &InternalOntology) -> bool {
    let pool = &internal.concepts;
    internal.axioms.iter().any(|ax| {
        if let Axiom::SubClassOf { sub, sup } = ax {
            matches!(pool.get(*sub), ConceptExpr::Top)
                && matches!(pool.get(*sup), ConceptExpr::Bot)
        } else {
            false
        }
    })
}
```

Called after the ABox-saturation check in `realize_internal`. O(n) axiom scan, cheap.

Added two tests to `crates/owl-dl-reasoner/tests/realize_inconsistent_shortcircuit.rs`:
- `realize_errors_on_top_subclass_bot`: asserts `Err(Inconsistent)` on a `⊤ ⊑ ⊥` KB
- `realize_consistent_with_individual_succeeds`: guard that the scan doesn't over-fire

Also added helper `parse_ofn` to that test file to avoid duplicating parse boilerplate.

### 4b: Comment rot fix
In `crates/owl-dl-saturation/src/lib.rs` line ~3749, a comment cited the propagation
site using a quoted code snippet:
```
// propagation at line `if self.subsumers.is_unsatisfiable(d) { ... }`. Sound:
```
Replaced with the function name:
```
// propagation in `process_subsumer`. Sound:
```

---

## Files changed

- `crates/owl-dl-saturation/src/lib.rs` — `Subsumers` struct: added `globally_inconsistent: bool` field + accessor + `#[allow(clippy::struct_field_names)]`; set `engine.subsumers.globally_inconsistent = engine.rules.global_unsat` in all three saturate return sites; fixed comment rot
- `crates/owl-dl-reasoner/src/classify.rs` — `classify_pure_el`: set `stats.inconsistent = true` when `closure.globally_inconsistent()`
- `crates/owl-dl-reasoner/src/realize.rs` — added `has_top_subclass_bot` helper; call it in `realize_internal` after the ABox check
- `crates/owl-dl-reasoner/tests/conjunctive_unsat.rs` — appended 3 tests (Finding 1 + Finding 3 × 2)
- `crates/owl-dl-reasoner/tests/datatype_inconsistency.rs` — added `classify` import; appended 1 test (Finding 2)
- `crates/owl-dl-reasoner/tests/realize_inconsistent_shortcircuit.rs` — added `parse_ofn` helper; appended 2 tests (Finding 4)

---

## Command outputs (verbatim)

### 1. `cargo test -p owl-dl-reasoner --test conjunctive_unsat`

```
   Compiling owl-dl-saturation v0.4.5 (/data/dumontier/rustdl/crates/owl-dl-saturation)
   Compiling owl-dl-tableau v0.4.5 (/data/dumontier/rustdl/crates/owl-dl-tableau)
   Compiling owl-dl-reasoner v0.4.5 (/data/dumontier/rustdl/crates/owl-dl-reasoner)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 1.94s
     Running tests/conjunctive_unsat.rs (target/debug/deps/conjunctive_unsat-f73ae6eaf25598c7)

running 16 tests
test top_bot_all_classes_unsat ... ok
test scoped_bot_does_not_globalise ... ok
test conjunctive_bot_derives_unsat ... ok
test some_top_bot_does_not_over_fire ... ok
test range_bot_derives_unsat ... ok
test conjunctive_bot_does_not_over_fire ... ok
test subclass_of_thing_a_stays_sat ... ok
test domain_bot_does_not_over_fire ... ok
test domain_bot_derives_unsat ... ok
test top_bot_no_fp_without_global_axiom ... ok
test some_bot_does_not_over_fire ... ok
test range_bot_does_not_over_fire ... ok
test top_bot_classify_stats_inconsistent ... ok
test some_bot_derives_unsat ... ok
test conjunctive_bot_matches_disjoint_classes_spelling ... ok
test some_top_bot_derives_unsat ... ok

test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

### 2. `cargo test -p owl-dl-saturation`

```
   Compiling owl-dl-saturation v0.4.5 (/data/dumontier/rustdl/crates/owl-dl-saturation)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 1.02s
     Running unittests src/lib.rs (target/debug/deps/owl_dl_saturation-6df6983380f1ab6a)

running 81 tests
[... 78 ok tests ...]

test result: ok. 78 passed; 0 failed; 3 ignored; 0 measured; 0 filtered out; finished in 0.31s

   Doc-tests owl_dl_saturation

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

### 3. `cargo test -p owl-dl-core`

```
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.09s
     Running unittests src/lib.rs (target/debug/deps/owl_dl_core-230e858063e37cb7)

running 236 tests
[... 236 ok tests ...]

test result: ok. 236 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
[... proptests: 6+1+1 passed ...]
```

### 4. `cargo test -p owl-dl-reasoner`

All test groups: ok (0 failed across all test files).
Sample results:
```
test result: ok. 173 passed; 0 failed; 3 ignored; ...
test result: ok. 16 passed; 0 failed; 0 ignored; ...  (conjunctive_unsat)
test result: ok. 73 passed; 0 failed; 0 ignored; ...  (datatype_inconsistency)
test result: ok. 4 passed; 0 failed; 0 ignored; ...   (realize_inconsistent_shortcircuit)
```

### 5. `cargo fmt --all -- --check`

```
(no output — clean)
```

### 6. `cargo clippy -p owl-dl-saturation -p owl-dl-reasoner -p owl-dl-core -p owl-dl-cli --all-targets --all-features -- -D warnings`

```
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.18s
```

---

## Notes / concerns

- The `#[allow(clippy::struct_field_names)]` on `Subsumers` suppresses a lint on the
  **pre-existing** `subsumers: IdMatrix` field (the name repeats the struct name). The
  lint became visible because my change caused the struct to be re-evaluated by clippy's
  pedantic pass. The allow is the right fix per clippy's own suggestion.
- `globally_inconsistent` is a `pub` field on `Subsumers` (for direct struct-literal
  construction in tests), but access is also gated via the `globally_inconsistent()`
  accessor for normal use. No external crate directly constructs `Subsumers`, so the
  field visibility is fine.
- The `has_top_subclass_bot` scan in `realize_internal` runs on every realize call,
  but it is O(n) in axioms and bounded (no saturation overhead). On consistent inputs
  it always returns false in microseconds.
