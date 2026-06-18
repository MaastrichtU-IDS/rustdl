# First-Class Data Properties — Sub-project 2: Tableau/Solver Validation — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Lock in (with gate-isolated canaries) the data-property reasoning shapes that already work via approach B, fix the one core completeness gap found in discovery (unqualified data cardinality), and document the deferred gap (disjoint-dp value-identity).

**Architecture:** Approach B (data property = object role; literal = `DKey(point v)`) was POC-validated in sub-project 1. A discovery spike found the remaining shapes mostly work via the reused object + concrete-domain machinery. Two gaps: (1) unqualified `DataMin/Max/ExactCardinality` drops; (2) `DisjointDataProperties` same-value clash not detected. Gap 2 is DEFERRED (sound under-approximation, rare, fix risks the node model). This sub-project fixes gap 1 and validates the rest. All behind `RUSTDL_DATA_PROPERTIES` (default OFF).

**Tech Stack:** Rust, `owl-dl-core` convert/IR, `owl-dl-reasoner` `is_consistent`, `cargo test`.

**Discovery findings (verified via `rustdl consistent`, gate OFF vs ON):**
- ∀-over-data + ABox out-of-range assertion → gate-ON inconsistent, gate-OFF consistent (WORKS, gate-isolated).
- Termination with data leaf nodes → consistent, terminates ~6ms (safe).
- Data range + out-of-bounds value, and *qualified* `≤n dp.T` → caught (also via existing preprocessing).
- GAP 1: unqualified `≤n dp` / `≥n dp` (range = `rdfs:Literal`) silently drops.
- GAP 2 (DEFERRED): `Disjoint(dp,dq) + dp(a,v) + dq(a,v)` not detected (two distinct `DKey(v)` value-nodes never merged). Sound miss (incompleteness, not FP).

---

## File structure
- `crates/owl-dl-core/src/convert.rs` — gap-1 fix: a gated, `rdfs:Literal`-restricted unqualified-cardinality fallback added to the three data-cardinality class-expression arms.
- `crates/owl-dl-reasoner/tests/data_properties.rs` — extend with validation canaries (∀-over-data, termination, gap-1).
- `docs/superpowers/specs/2026-06-17-first-class-data-properties-design.md` — document gap 2 as a deferred sound under-approximation; mark gap 1 fixed.

## Reference shapes (verified)
- Cardinality arms at `convert.rs:~895-960`: `ClassExpression::DataMinCardinality { n, dp, dr }` / `DataMaxCardinality` / `DataExactCardinality`, each an `.or_else(...)` chain over `lower_<type>_data_cardinality(*n, dp, dr, vocab, pool, want_min, want_max)`; all-fail ⇒ `Err(ConversionError::UnsupportedDataRange)` ⇒ `ce_or_skip!` drops the enclosing axiom.
- Constructors: `pool.min(n, role, c)`, `pool.max(n, role, c)`, `pool.top()`. No `pool.exact`; exact = `pool.and([min, max])` (see the `(true,true)` branch of `lower_int_data_cardinality` at ~`convert.rs:985`).
- dp role: `Role::named(vocab.intern_role(dp.0.as_ref()))`.
- `data_properties_enabled()` (added sub-project 1) — the gate.
- `rdfs:Literal` IRI: `http://www.w3.org/2000/01/rdf-schema#Literal`.
- horned-owl `DataRange`: `DataRange::Datatype(Datatype)` where `Datatype.0` is the IRI; unqualified data cardinality parses to `dr = DataRange::Datatype(rdfs:Literal)`.

---

### Task 1: Gap-1 fix — gated, `rdfs:Literal`-restricted unqualified data cardinality

**SOUNDNESS CRUX (read first):** The fallback may lower to a `⊤` filler **only** when `dr` is exactly `rdfs:Literal` (the all-values data range). For any *other* unrecognized datatype it must keep dropping. Reason: `≤n dp.⊤` constrains *all* dp-values; `≤n dp.SomeType` only the typed ones — using `⊤` for a specific type would over-constrain a max restriction and could make a satisfiable class unsatisfiable (a false clash = FP). For min restrictions `⊤` is sound (all dp-values are literals), but we gate on `rdfs:Literal` uniformly for clarity and safety.

**Files:** Modify `crates/owl-dl-core/src/convert.rs`.

- [ ] **Step 1: Add a helper** near the `lower_*_data_cardinality` fns (~`convert.rs:970`):

```rust
/// Whether `dr` is exactly the unqualified `rdfs:Literal` data range (the
/// "any value" range). Only this range may fall back to a `⊤` filler for
/// data cardinality — a specific unrecognized datatype must keep dropping
/// (using `⊤` there would over-constrain a `≤n` restriction → unsound FP).
fn is_rdfs_literal<A: ForIRI>(dr: &DataRange<A>) -> bool {
    matches!(dr, DataRange::Datatype(dt)
        if dt.0.as_ref() == "http://www.w3.org/2000/01/rdf-schema#Literal")
}

/// Gated fallback for UNQUALIFIED data cardinality (`≥n dp` / `≤n dp` over
/// `rdfs:Literal`): lower to the same cardinality over the IR `⊤` filler, so
/// the existing object ≤n/≥n machinery + DKey-disjointness reason about it.
/// `None` ⇒ not applicable (gate off, or range is not `rdfs:Literal`) ⇒ caller
/// keeps dropping. Sound: see `is_rdfs_literal`.
fn lower_unqualified_data_cardinality<A: ForIRI>(
    n: u32,
    dp: &horned_owl::model::DataProperty<A>,
    dr: &DataRange<A>,
    vocab: &mut Vocabulary,
    pool: &mut ConceptPool,
    want_min: bool,
    want_max: bool,
) -> Option<ConceptId> {
    if !data_properties_enabled() || !is_rdfs_literal(dr) {
        return None;
    }
    let role = Role::named(vocab.intern_role(dp.0.as_ref()));
    let top = pool.top();
    Some(match (want_min, want_max) {
        (true, false) => pool.min(n, role, top),
        (false, true) => pool.max(n, role, top),
        (true, true) => {
            let lo = pool.min(n, role, top);
            let hi = pool.max(n, role, top);
            pool.and(vec![lo, hi])
        }
        (false, false) => return None, // unreachable in practice
    })
}
```
(Verify `pool.and` takes a `Vec<ConceptId>` — match the `(true,true)` branch of `lower_int_data_cardinality`; if it uses a different combiner, mirror it exactly.)

- [ ] **Step 2: Add the fallback to the END of all three cardinality `or_else` chains.** For `DataMinCardinality { n, dp, dr }` append `.or_else(|_| lower_unqualified_data_cardinality(*n, dp, dr, vocab, pool, true, false).ok_or(ConversionError::UnsupportedDataRange))`. For `DataMaxCardinality` use `(false, true)`. For `DataExactCardinality` use `(true, true)`. Example (DataMaxCardinality):

```rust
        ClassExpression::DataMaxCardinality { n, dp, dr } => {
            lower_int_data_cardinality(*n, dp, dr, vocab, pool, false, true)
                // ... existing .or_else chain unchanged ...
                .or_else(|_| {
                    lower_datetime_oneof_data_cardinality(*n, dp, dr, vocab, pool, false, true)
                })
                .or_else(|_| {
                    lower_unqualified_data_cardinality(*n, dp, dr, vocab, pool, false, true)
                        .ok_or(ConversionError::UnsupportedDataRange)
                })
        }
```

- [ ] **Step 3: Run existing tests (no gate-OFF regression — fallback is gated + Literal-only):**

Run: `export PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"; cargo test -p owl-dl-core 2>&1 | tail -4`
Expected: all PASS (gate OFF ⇒ helper returns None ⇒ unchanged drop).

- [ ] **Step 4: Add a gate-isolated integration canary** in `crates/owl-dl-reasoner/tests/data_properties.rs` (the file has `DpGuard`/`DP_ENV_MUTEX`/`onto` from sub-project 1):

```rust
#[test]
fn poc_unqualified_max_cardinality_merges_distinct_values() {
    let _lock = DP_ENV_MUTEX.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = DpGuard::on();
    // ≤1 dp (unqualified, rdfs:Literal) + class restriction value 5 + assertion 6.
    // Gate ON: ≤1 dp.⊤ forces the two distinct value-nodes to merge → DKey(5) ⊓
    // DKey(6) disjoint → inconsistent. (Two direct assertions would be masked by
    // D4; the class-restriction shape isolates approach B.)
    let o = onto(
        "Declaration(DataProperty(:dp)) Declaration(Class(:C)) Declaration(NamedIndividual(:a))\n\
         SubClassOf(:C DataMaxCardinality(1 :dp))\n\
         SubClassOf(:C DataHasValue(:dp \"5\"^^xsd:integer))\n\
         ClassAssertion(:C :a)\n\
         DataPropertyAssertion(:dp :a \"6\"^^xsd:integer)",
    );
    assert!(!owl_dl_reasoner::is_consistent(&o).unwrap(),
        "≤1 unqualified dp with two distinct values must be inconsistent (gate ON)");
}

#[test]
fn poc_unqualified_max_cardinality_consistent_gate_off() {
    let _lock = DP_ENV_MUTEX.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = DpGuard::off();
    // Same ontology, gate OFF: DataMaxCardinality(1 :dp) drops, the dp assertion
    // drops → consistent. Proves the inconsistency is approach B's, not D4's.
    let o = onto(
        "Declaration(DataProperty(:dp)) Declaration(Class(:C)) Declaration(NamedIndividual(:a))\n\
         SubClassOf(:C DataMaxCardinality(1 :dp))\n\
         SubClassOf(:C DataHasValue(:dp \"5\"^^xsd:integer))\n\
         ClassAssertion(:C :a)\n\
         DataPropertyAssertion(:dp :a \"6\"^^xsd:integer)",
    );
    assert!(owl_dl_reasoner::is_consistent(&o).unwrap(),
        "gate OFF: unqualified cardinality + dp assertion drop → consistent");
}
```

- [ ] **Step 5: Run the canaries (the gate-differential proves soundness + isolation):**

Run: `cargo test -p owl-dl-reasoner --test data_properties 2>&1 | tail -10`
Expected: all pass, including both new ones. **If `poc_unqualified_max_cardinality_consistent_gate_off` is inconsistent, STOP** — the fallback is firing gate-OFF (gate bug) or over-constraining; report it.

- [ ] **Step 6: clippy + fmt + commit:**

```bash
cargo clippy -p owl-dl-core -p owl-dl-reasoner --all-targets -- -D warnings 2>&1 | tail -5
cargo fmt --all
git add crates/owl-dl-core/src/convert.rs crates/owl-dl-reasoner/tests/data_properties.rs
git commit -m "feat(convert): gated unqualified (rdfs:Literal) data cardinality → cardinality over Top filler"
```

---

### Task 2: Validation canaries for the already-working shapes

**Files:** Modify `crates/owl-dl-reasoner/tests/data_properties.rs`.

- [ ] **Step 1: Add the ∀-over-data + ABox canary (gate-isolated) and its in-range negative, plus a termination guard:**

```rust
#[test]
fn poc_data_all_values_from_abox_out_of_range_inconsistent() {
    let _lock = DP_ENV_MUTEX.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = DpGuard::on();
    // a ∈ C ⇒ ∀dp.[≤3]; dp(a,5) ⇒ ∃dp.DKey(5); ∀ pushes the range onto the
    // value-node ⇒ 5 ∉ (-∞,3] → inconsistent.
    let o = onto(
        "Declaration(DataProperty(:dp)) Declaration(Class(:C)) Declaration(NamedIndividual(:a))\n\
         SubClassOf(:C DataAllValuesFrom(:dp DatatypeRestriction(xsd:integer xsd:maxInclusive \"3\"^^xsd:integer)))\n\
         ClassAssertion(:C :a)\n\
         DataPropertyAssertion(:dp :a \"5\"^^xsd:integer)",
    );
    assert!(!owl_dl_reasoner::is_consistent(&o).unwrap(),
        "∀dp.[≤3] + dp(a,5) must be inconsistent (gate ON)");
}

#[test]
fn poc_data_all_values_from_abox_in_range_consistent() {
    let _lock = DP_ENV_MUTEX.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = DpGuard::on();
    let o = onto(
        "Declaration(DataProperty(:dp)) Declaration(Class(:C)) Declaration(NamedIndividual(:a))\n\
         SubClassOf(:C DataAllValuesFrom(:dp DatatypeRestriction(xsd:integer xsd:maxInclusive \"3\"^^xsd:integer)))\n\
         ClassAssertion(:C :a)\n\
         DataPropertyAssertion(:dp :a \"2\"^^xsd:integer)",
    );
    assert!(owl_dl_reasoner::is_consistent(&o).unwrap(),
        "∀dp.[≤3] + dp(a,2) must be consistent (in range)");
}

#[test]
fn poc_data_leaf_nodes_terminate_under_object_cycle() {
    let _lock = DP_ENV_MUTEX.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = DpGuard::on();
    // Cyclic object role + data leaf must terminate and be consistent.
    let o = onto(
        "Declaration(ObjectProperty(:r)) Declaration(DataProperty(:dp))\n\
         Declaration(Class(:C)) Declaration(NamedIndividual(:a))\n\
         SubClassOf(:C ObjectSomeValuesFrom(:r :C))\n\
         SubClassOf(:C DataSomeValuesFrom(:dp xsd:integer))\n\
         ClassAssertion(:C :a)",
    );
    assert!(owl_dl_reasoner::is_consistent(&o).unwrap(),
        "cyclic object role with data leaves must stay consistent and terminate");
}
```

- [ ] **Step 2: Run + clippy + fmt + commit:**

```bash
cargo test -p owl-dl-reasoner --test data_properties 2>&1 | tail -12   # all pass
cargo clippy -p owl-dl-reasoner --test data_properties -- -D warnings 2>&1 | tail -5
cargo fmt --all
git add crates/owl-dl-reasoner/tests/data_properties.rs
git commit -m "test(data): validation canaries — ∀-over-data ABox clash (gate-isolated), in-range negative, termination guard"
```

---

### Task 3: Document the deferred gap 2

**Files:** Modify `docs/superpowers/specs/2026-06-17-first-class-data-properties-design.md`.

- [ ] **Step 1: Add a "Sub-project 2 outcome" subsection** under the sub-projects list recording: gap 1 (unqualified data cardinality) FIXED; gap 2 (DisjointDataProperties same-value clash) DEFERRED as a sound under-approximation — `Disjoint(dp,dq) + dp(a,v) + dq(a,v)` is not detected because the two `DKey(v)` value-nodes are distinct anonymous nodes never merged; a missed clash is incompleteness (sound, never an FP); rare construct; the clean fix (value-node canonicalization) touches the tableau node-creation model and is out of scope. Revisit only on a measured need. (Plain prose, ~6 lines; no code.)

- [ ] **Step 2: Commit:**

```bash
git add docs/superpowers/specs/2026-06-17-first-class-data-properties-design.md
git commit -m "docs(data): sub-project 2 outcome — gap 1 fixed, gap 2 (disjoint-dp) deferred (sound under-approx)"
```

---

### Task 4: Wrap-up verification

- [ ] **Step 1:** `cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3` (clean).
- [ ] **Step 2:** `cargo test -p owl-dl-core -p owl-dl-reasoner 2>&1 | grep 'test result' | tail` (all pass).
- [ ] **Step 3:** Confirm gate-OFF byte-identity is preserved: the gap-1 fix is gated AND `rdfs:Literal`-restricted; the gate-OFF cardinality canary passing (Task 1 Step 5) confirms it. Report.
- [ ] **Step 4:** Do NOT push. Report completion + the gate-differential verdicts.

## Notes for the implementer
- The gap-1 soundness crux (`rdfs:Literal` only) is non-negotiable — never fall back to `⊤` for an arbitrary unrecognized datatype.
- Every new canary that claims to validate approach B must be gate-isolated (gate-ON inconsistent, gate-OFF consistent) OR clearly a regression/termination guard.
- Gap 2 stays unimplemented by design (user-approved defer).
