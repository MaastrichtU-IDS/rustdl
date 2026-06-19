# SP1: Semantic Branching + Disjunct Reordering in the Wedge — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port `search.rs`'s restricted semantic branching + disjunct reordering into the hypertableau wedge's (`hyper.rs`) syntactic `solve` disjunction loop, flag-gated, to collapse wine's 1.49M-branch find-the-model failure while preserving FP=0 and byte-identical corpus closures.

**Architecture:** Add an engine complement map (`d → ¬d`) threaded from `HyperCache`; add a `semantic_branching` flag field; in `solve`'s `find_open_disjunction` branch, (1) reorder head atoms cheapest-first and (2) carry each failed `Class` disjunct's literal complement, asserted dep-tagged on subsequent siblings. Flag-off path is byte-identical to current `main`.

**Tech Stack:** Rust (edition 2024), `cargo` workspace; engine in `crates/owl-dl-tableau/src/hyper.rs`; orchestrator in `crates/owl-dl-reasoner/src/lib.rs`. Toolchain on PATH: `$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin`.

**Branch:** `feat/wedge-semantic-branching-sp1` (already created, off `main`).

---

## File Structure

- `crates/owl-dl-tableau/src/hyper.rs` — engine: new `complements` + `semantic_branching` fields; `with_complements` / `with_semantic_branching` builders; `complement_of`, `score_disjunct`, `reorder_disjunction_heads` methods; modified `solve` disjunction loop.
- `crates/owl-dl-reasoner/src/lib.rs` — `HyperCache` stores `complements`; `semantic_branching_enabled()` flag helper; thread both into every engine construction.
- `crates/owl-dl-reasoner/tests/wedge_semantic_branching.rs` — new: verdict-preservation, disjunctive-Unsat completeness canary, P0 wine-collapse canary.

## Reference: the current `solve` disjunction loop (in `hyper.rs`, the `find_open_disjunction` arm)

```rust
        if let Some((ci, node, binding)) = self.find_open_disjunction() {
            if depth == 0 {
                return HyperResult::Stalled;
            }
            self.track_depth(depth);
            let d = u32::try_from(self.init_depth - depth).unwrap_or(u32::MAX);
            let body_deps = self.clause_body_deps(ci, node, &binding);
            let decision_deps = body_deps.insert(d);
            let head_len = self.clauses[ci].head.len();
            let mut any_stalled = false;
            let mut combined = DepSet::EMPTY;
            for k in 0..head_len {
                let head_atom = self.clauses[ci].head[k];
                let saved = self.save();
                self.stats.branches_taken += 1;
                self.stats.disj_branches += 1;
                let _ = self.apply_head_atom(head_atom, node, &binding, decision_deps);
                match self.solve(depth - 1) {
                    HyperResult::Sat => return HyperResult::Sat,
                    HyperResult::Unsat => {
                        let child_deps = self.clash_deps;
                        self.restore(saved);
                        if !child_deps.contains(d) {
                            self.clash_deps = child_deps;
                            return HyperResult::Unsat;
                        }
                        combined = combined.union(child_deps);
                    }
                    HyperResult::Stalled => {
                        self.restore(saved);
                        any_stalled = true;
                    }
                }
            }
            if any_stalled {
                return HyperResult::Stalled;
            }
            self.clash_deps = combined.remove(d);
            return HyperResult::Unsat;
        }
```

Key facts the implementer must rely on:
- `Atom` (from `owl_dl_core::clause`): `Class(ClassId, Var)`, `Role(..)`, `Exists(Role, ClassId, Var)`, `AtMost(..)`, `AtLeast(..)`, `Equal(..)`.
- `HyperNode.labels: Vec<ClassId>` (sorted; use `binary_search`).
- `self.add_label(target: HNode, c: ClassId, deps: DepSet) -> bool` is the label-assert primitive (`apply_head_atom`'s `Class` arm uses it).
- `resolve_var(v: Var, xnode: HNode, binding: &Binding) -> Option<HNode>` resolves a clause var to a node (used in `apply_head_atom`).
- Builder pattern example (`with_adaptive_budget`): `pub fn with_x(mut self) -> Self { self.x = true; self }`.

---

### Task 1: Thread the complement map into the engine

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` (engine struct fields, `new`/`new_seeded`/`new_with_prebuilt` initializers, builders, `complement_of`)
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (`HyperCache` stores + threads `complements`)

- [ ] **Step 1: Add the engine field.** In `hyper.rs`, in the `HyperEngine` struct (near `sub_roles: Option<RoleHierarchy>,`), add:

```rust
    /// SP1 semantic-branching complement map: `complements[c.index()]` is the
    /// literal complement `¬c` of class `c`, or `None` if `c` has no registered
    /// complement. Threaded from `HyperCache` (built by the §2 complement
    /// machinery). Used only when `semantic_branching` is on.
    complements: Vec<Option<ClassId>>,
    /// SP1: when `true`, the `solve` disjunction loop reorders disjuncts
    /// cheapest-first and carries failed `Class` disjuncts' literal complements
    /// onto subsequent siblings (restricted semantic branching). Off ⇒ the loop
    /// is byte-identical to pre-SP1.
    semantic_branching: bool,
```

- [ ] **Step 2: Initialize the fields** in every constructor that builds a `HyperEngine` literal. Find each struct-literal initializer (search `sub_roles: None,` — there are several, e.g. around the `new`, `new_seeded`, `new_with_prebuilt` constructors). Beside each `sub_roles: None,` add:

```rust
            complements: Vec::new(),
            semantic_branching: false,
```

- [ ] **Step 3: Add the builders + accessor.** Near `with_adaptive_budget`:

```rust
    /// Provide the literal-complement map for SP1 semantic branching. `map[i]`
    /// (if `Some`) is `¬c` for the class with index `i`. No index rebuild needed
    /// (complements don't affect clause trigger indexes).
    #[must_use]
    pub fn with_complements(mut self, map: Vec<Option<ClassId>>) -> Self {
        self.complements = map;
        self
    }

    /// Enable SP1 restricted semantic branching + disjunct reordering.
    #[must_use]
    pub fn with_semantic_branching(mut self) -> Self {
        self.semantic_branching = true;
        self
    }

    /// The registered literal complement `¬c`, if any.
    fn complement_of(&self, c: ClassId) -> Option<ClassId> {
        self.complements.get(c.index() as usize).copied().flatten()
    }
```

- [ ] **Step 4: Store the complements in `HyperCache`.** In `lib.rs`, add a field to `struct HyperCache` (near `sub_roles: RoleHierarchy,`):

```rust
    /// SP1 literal-complement map (`c.index()` → `¬c`), built by the §2
    /// complement machinery during `build`. Threaded into every engine for
    /// semantic branching.
    complements: Vec<Option<owl_dl_core::ir::ClassId>>,
```

- [ ] **Step 5: Populate it in `HyperCache::build`.** The local `complements: HashMap<ClassId, ClassId>` is already filled by `build_sup_neg_map`. After that call, before constructing `Self { .. }`, convert it to the indexed Vec and store it:

```rust
        // SP1: flatten the complement HashMap into an index-keyed Vec for the engine.
        let max_idx = complements
            .keys()
            .chain(complements.values())
            .map(|c| c.index() as usize)
            .max()
            .map_or(0, |m| m + 1);
        let mut complements_vec: Vec<Option<ClassId>> = vec![None; max_idx];
        for (&c, &nc) in &complements {
            complements_vec[c.index() as usize] = Some(nc);
        }
```

Then add `complements: complements_vec,` to the `Self { .. }` initializer.

- [ ] **Step 6: Build + test it compiles.** Run: `cargo build -p owl-dl-reasoner --release`
Expected: `Finished` with no errors. (No behavior change yet — fields are unused; allow a temporary `dead_code` only if the build fails on it, otherwise Task 3/4 consume them.)

- [ ] **Step 7: Commit.**

```bash
git add crates/owl-dl-tableau/src/hyper.rs crates/owl-dl-reasoner/src/lib.rs
git commit -m "feat(wedge): thread complement map + semantic_branching flag into engine (SP1, inert)"
```

---

### Task 2: Flag helper + wire into engine constructions (still inert)

**Files:**
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (flag helper; `with_complements`/`with_semantic_branching` calls in `decide_with_stats`, `sat_only_with_stats`, `classify_labels`)

- [ ] **Step 1: Add the flag helper.** In `lib.rs`, near `adaptive_budget_enabled`:

```rust
/// SP1 wedge semantic branching (`RUSTDL_WEDGE_SEMANTIC_BRANCHING`). **Default
/// off** during build/validation; flip to default-on after the FP=0
/// byte-identical corpus gate passes. When off, the wedge disjunction loop is
/// byte-identical to pre-SP1.
#[must_use]
pub(crate) fn semantic_branching_enabled() -> bool {
    std::env::var_os("RUSTDL_WEDGE_SEMANTIC_BRANCHING").is_some_and(|v| v == "1")
}
```

- [ ] **Step 2: Wire into `decide_with_stats`.** In `HyperCache::decide_with_stats`, after the other `if … { engine = engine.with_*() }` blocks (just before `let result = engine.decide_with_deadline(...)`), add:

```rust
        if crate::semantic_branching_enabled() {
            engine = engine
                .with_complements(self.complements.clone())
                .with_semantic_branching();
        }
```

- [ ] **Step 3: Wire into `sat_only_with_stats`** — add the identical block in the same position (before its `decide_with_deadline`).

- [ ] **Step 4: Wire into `classify_labels`** — add the identical block before its `decide_with_deadline` call.

- [ ] **Step 5: Build + verify flag-off is inert.** Run: `cargo build --workspace --release`
Expected: `Finished`, no errors.

- [ ] **Step 6: Commit.**

```bash
git add crates/owl-dl-reasoner/src/lib.rs
git commit -m "feat(wedge): RUSTDL_WEDGE_SEMANTIC_BRANCHING flag + wiring (default off, inert)"
```

---

### Task 3: Disjunct reordering

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` (`score_disjunct`, `reorder_disjunction_heads`, `solve` loop order)
- Test: `crates/owl-dl-reasoner/tests/wedge_semantic_branching.rs`

- [ ] **Step 1: Write the verdict-preservation test (failing until wired).** Create `crates/owl-dl-reasoner/tests/wedge_semantic_branching.rs`:

```rust
//! SP1: semantic branching + disjunct reordering verdict-preservation + canaries.
#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::HyperResult;
use std::io::Cursor;

// Disjunction where one branch is SAT and another self-clashes: A ≡ (B ⊔ C),
// A ⊑ ¬C  ⟹  A is satisfiable via B. Verdict must be identical flag-on/off.
const SAT_ONT: &str = "Prefix(:=<urn:s#>)
Ontology(
  Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))
  EquivalentClasses(:A ObjectUnionOf(:B :C))
  SubClassOf(:A ObjectComplementOf(:C))
)";

fn load(s: &str) -> SetOntology<RcStr> {
    let mut r = Cursor::new(s.as_bytes().to_vec());
    read_ofn(&mut r, ParserConfiguration::default()).expect("parse").0
}

fn sat(ont: &SetOntology<RcStr>, c: &str) -> HyperResult {
    owl_dl_reasoner::sat_class_probe(ont, c, 256, None)
        .expect("probe")
        .expect("class")
        .0
}

struct EnvGuard(&'static str);
impl Drop for EnvGuard {
    fn drop(&mut self) {
        // SAFETY: test-local env restore.
        unsafe { std::env::remove_var(self.0) };
    }
}

#[test]
fn reorder_preserves_sat_verdict() {
    let ont = load(SAT_ONT);
    // SAFETY: serialized by the harness; restored on drop.
    unsafe { std::env::remove_var("RUSTDL_WEDGE_SEMANTIC_BRANCHING") };
    let off = sat(&ont, "urn:s#A");
    let _g = EnvGuard("RUSTDL_WEDGE_SEMANTIC_BRANCHING");
    unsafe { std::env::set_var("RUSTDL_WEDGE_SEMANTIC_BRANCHING", "1") };
    let on = sat(&ont, "urn:s#A");
    assert_eq!(off, HyperResult::Sat, "A is satisfiable via the B disjunct");
    assert_eq!(on, off, "reordering must not change the verdict");
}
```

- [ ] **Step 2: Run it — expect FAIL** (flag-on currently does nothing useful, but more importantly `with_semantic_branching` exists yet the loop ignores it; the test should still pass *verdict-wise* since the loop is unchanged — so this test guards against regressions. Run to confirm it passes with the inert wiring, establishing the baseline):

Run: `RUSTDL_WEDGE_SEMANTIC_BRANCHING=1 cargo test -p owl-dl-reasoner --release --test wedge_semantic_branching reorder_preserves_sat_verdict`
Expected: PASS (both verdicts `Sat`). If it does not compile, fix imports. This is the regression baseline; later steps must keep it green.

- [ ] **Step 3: Implement scoring + reorder.** In `hyper.rs`, add (near `find_open_disjunction`):

```rust
    /// SP1 disjunct score (lower tried first): 3 = the atom's complement is
    /// already in the target's label (asserting clashes immediately) ⇒ last;
    /// 2 = generating/compound (`Exists`/`AtLeast`/`AtMost`) ⇒ creates a
    /// successor or fires a merge; 1 = plain `Class`/`Equal`/`Role`.
    fn score_disjunct(&self, atom: Atom, node: HNode, binding: &Binding) -> u8 {
        match atom {
            Atom::Class(c, v) => {
                if let Some(target) = resolve_var(v, node, binding)
                    && let Some(neg) = self.complement_of(c)
                {
                    let t = self.resolve(target);
                    if self.nodes[t.index()].labels.binary_search(&neg).is_ok() {
                        return 3;
                    }
                }
                1
            }
            Atom::Exists(..) | Atom::AtLeast(..) | Atom::AtMost(..) => 2,
            Atom::Role(..) | Atom::Equal(..) => 1,
        }
    }

    /// SP1: indices of clause `ci`'s head atoms, cheapest-first. Stable on the
    /// original index (deterministic). Identity order if scoring is uniform.
    fn reorder_disjunction_heads(&self, ci: usize, node: HNode, binding: &Binding) -> Vec<usize> {
        let head = &self.clauses[ci].head;
        let mut idx: Vec<usize> = (0..head.len()).collect();
        idx.sort_by_key(|&k| (self.score_disjunct(head[k], node, binding), k));
        idx
    }
```

- [ ] **Step 4: Use the order in `solve`.** Replace the disjunction-loop body. Change `let head_len = self.clauses[ci].head.len();` to keep it, then after computing `decision_deps`, add:

```rust
            let order: Vec<usize> = if self.semantic_branching {
                self.reorder_disjunction_heads(ci, node, &binding)
            } else {
                (0..head_len).collect()
            };
```

and change the loop header from `for k in 0..head_len {` to `for &k in &order {`.

- [ ] **Step 5: Run the verdict test + a clash-last unit test.** Add to the test file:

```rust
// Reordering must put an obviously-clashing disjunct last: A ⊑ ¬B, A ≡ (B ⊔ D).
// With flag on, the search should try D (sat) first, so it finds Sat with fewer
// branches than trying B (immediate clash) first — but the VERDICT is the gate here.
#[test]
fn reorder_obvious_clash_last_still_sat() {
    let ont = load(
        "Prefix(:=<urn:r#>)
Ontology(
  Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:D))
  EquivalentClasses(:A ObjectUnionOf(:B :D))
  SubClassOf(:A ObjectComplementOf(:B))
)",
    );
    let _g = EnvGuard("RUSTDL_WEDGE_SEMANTIC_BRANCHING");
    // SAFETY: serialized.
    unsafe { std::env::set_var("RUSTDL_WEDGE_SEMANTIC_BRANCHING", "1") };
    assert_eq!(sat(&ont, "urn:r#A"), HyperResult::Sat);
}
```

Run: `cargo test -p owl-dl-reasoner --release --test wedge_semantic_branching`
Expected: both tests PASS (flag-off run of `reorder_preserves_sat_verdict` is `Sat`; flag-on runs are `Sat`).

- [ ] **Step 6: Commit.**

```bash
git add crates/owl-dl-tableau/src/hyper.rs crates/owl-dl-reasoner/tests/wedge_semantic_branching.rs
git commit -m "feat(wedge): SP1 disjunct reordering (cheapest-first, gated, verdict-neutral)"
```

---

### Task 4: Restricted semantic branching

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` (`solve` loop: `literal_complements` accumulator + dep-tagged sibling assertion + carry-forward)
- Test: `crates/owl-dl-reasoner/tests/wedge_semantic_branching.rs`

- [ ] **Step 1: Add the accumulator + assertion in `solve`.** Just before the `for &k in &order {` loop, add:

```rust
            // SP1 restricted semantic branching: literal complements of failed
            // disjuncts, re-asserted (dep-tagged) on each subsequent sibling so
            // a re-derivation of the failed disjunct clashes immediately.
            let mut literal_complements: Vec<ClassId> = Vec::new();
```

Immediately after `let saved = self.save();` and the two `self.stats.*` bumps, before `apply_head_atom`, add:

```rust
                for &comp in &literal_complements {
                    let _ = self.add_label(node, comp, decision_deps);
                }
```

In the `HyperResult::Unsat` arm, after `combined = combined.union(child_deps);`, add the carry-forward:

```rust
                        if self.semantic_branching
                            && let Atom::Class(c, _) = self.clauses[ci].head[k]
                            && let Some(comp) = self.complement_of(c)
                        {
                            literal_complements.push(comp);
                        }
```

(Note: `self.add_label(node, comp, …)` asserts on the matched `node`; this mirrors `search.rs`, which asserts complements on the branching node. The `saved` checkpoint precedes the complement assertions, so `restore(saved)` rolls them back and they are re-asserted from the accumulator next iteration.)

- [ ] **Step 2: Add the completeness canary (disjunctive-Unsat still proven).** Add to the test file:

```rust
// Semantic branching must NOT break disjunctive-Unsat proofs. A ≡ (B ⊔ C),
// A ⊑ ¬B, A ⊑ ¬C ⟹ A ⊑ ⊥ (unsat) — both disjuncts clash. Flag-on must still
// return Unsat (a dropped clash would be a MISSED subsumption).
#[test]
fn semantic_branching_preserves_unsat() {
    let ont = load(
        "Prefix(:=<urn:u#>)
Ontology(
  Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))
  EquivalentClasses(:A ObjectUnionOf(:B :C))
  SubClassOf(:A ObjectComplementOf(:B))
  SubClassOf(:A ObjectComplementOf(:C))
)",
    );
    let _g = EnvGuard("RUSTDL_WEDGE_SEMANTIC_BRANCHING");
    // SAFETY: serialized.
    unsafe { std::env::set_var("RUSTDL_WEDGE_SEMANTIC_BRANCHING", "1") };
    assert_eq!(sat(&ont, "urn:u#A"), HyperResult::Unsat, "A ⊑ ⊥ must still be proven");
}
```

- [ ] **Step 3: Run all SP1 tests.** Run: `cargo test -p owl-dl-reasoner --release --test wedge_semantic_branching`
Expected: 3 tests PASS (`reorder_preserves_sat_verdict`, `reorder_obvious_clash_last_still_sat`, `semantic_branching_preserves_unsat`).

- [ ] **Step 4: Clippy clean.** Run: `cargo clippy -p owl-dl-tableau -p owl-dl-reasoner --release --all-targets -- -D warnings`
Expected: no warnings. (If `let`-chains trip a lint, the codebase already uses them — match the existing style.)

- [ ] **Step 5: Commit.**

```bash
git add crates/owl-dl-tableau/src/hyper.rs crates/owl-dl-reasoner/tests/wedge_semantic_branching.rs
git commit -m "feat(wedge): SP1 restricted semantic branching (literal complements, dep-tagged, gated)"
```

---

### Task 5: P0 GATE — wine collapse measurement (go/no-go)

**Files:**
- Test: `crates/owl-dl-reasoner/tests/wedge_semantic_branching.rs` (ignored canary)

- [ ] **Step 1: Add the wine-collapse canary.** Needs the gitignored ore-15672/wine fixtures; `#[ignore]`d. Add:

```rust
// P0 GATE (run manually): sat(CabernetFranc) on wine collapses >=10x with the
// flag on. Baseline (flag off): ~1.49M branches / Stalled at 10s. Needs fixture.
#[test]
#[ignore = "P0 perf gate; needs ontologies/real/wine.ofn"]
fn p0_wine_cabernet_franc_collapses() {
    use std::time::Duration;
    let path = "../../ontologies/real/wine.ofn";
    let Ok(src) = std::fs::read_to_string(path) else {
        eprintln!("SKIP: missing {path}");
        return;
    };
    let mut r = Cursor::new(src.into_bytes());
    let ont: SetOntology<RcStr> = read_ofn(&mut r, ParserConfiguration::default()).unwrap().0;
    let cf = "http://www.w3.org/TR/2003/PR-owl-guide-20031209/wine#CabernetFranc";
    let _g = EnvGuard("RUSTDL_WEDGE_SEMANTIC_BRANCHING");
    // SAFETY: serialized; big stack via the spawned thread below.
    unsafe {
        std::env::set_var("RUSTDL_WEDGE_SEMANTIC_BRANCHING", "1");
        std::env::set_var("RUSTDL_ADAPTIVE_BUDGET", "0");
    }
    let child = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024 * 1024)
        .spawn(move || {
            owl_dl_reasoner::sat_class_probe(&ont, cf, 256, Some(Duration::from_secs(30)))
                .unwrap()
                .unwrap()
        })
        .unwrap();
    let (res, s, wall) = child.join().unwrap();
    eprintln!("P0 sat(CabernetFranc) flag-on: {res:?} wall={wall:.0}ms branches={}", s.branches_taken);
    assert_eq!(res, HyperResult::Sat, "CabernetFranc is satisfiable; flag-on should find the model");
    assert!(
        s.branches_taken < 150_000,
        "P0 GATE: branches must collapse >=10x (baseline ~1.49M, target <150k); got {}",
        s.branches_taken
    );
}
```

- [ ] **Step 2: Run the P0 gate.** Run: `cargo test -p owl-dl-reasoner --release --test wedge_semantic_branching -- --ignored --nocapture p0_wine_cabernet_franc_collapses`
Expected: PASS — `Sat`, `branches_taken < 150_000` (ideally far less; baseline 1.49M).

- [ ] **Step 3: GO/NO-GO decision.**
  - **PASS** → proceed to Task 6.
  - **FAIL** (no collapse, or not `Sat`): **STOP.** Do not proceed to flag-flip. Report the branch count and the `decide_pair_probe`/`sat_class_probe` stats; the reordering/semantic-branching as ported did not bite, and the design needs rethinking (likely SP2's heuristic or SP3's BCP is the actual lever). Record the negative result in the spec doc and surface to the human.

- [ ] **Step 4: Commit the canary** (regardless of pass — it documents the gate):

```bash
git add crates/owl-dl-reasoner/tests/wedge_semantic_branching.rs
git commit -m "test(wedge): SP1 P0 wine-collapse gate (sat(CabernetFranc) <150k branches)"
```

---

### Task 6: FP=0 GATE — byte-identical corpus closures (mandatory, blocking)

**Files:** none (validation only)

- [ ] **Step 1: Run the full corpus closure-diff with the flag ON.** Run:

```bash
RUSTDL_WEDGE_SEMANTIC_BRANCHING=1 RUSTDL_TEST_PAIR_MS=1000 \
  cargo test -p owl-dl-reasoner --test konclude_closure_diff --release -- --ignored --nocapture
```

Expected: every fixture line reads `FP=0 MISSED=0` with `rustdl_closure == konclude_closure` (galen, notgalen, sio, ore-10908, ore-15672, wine, ro, sulo, bibtex, shoiq-knowledge). The only acceptable failure is the pre-existing `family_inconsistency_detected` stretch sentinel (it fails identically flag-on and flag-off — confirm by also running with the flag off).

- [ ] **Step 2: GO/NO-GO.**
  - **All FP=0/MISSED=0, closures byte-identical** → proceed.
  - **Any new FP or new MISSED** → **STOP.** Semantic branching introduced an unsound complement assertion or a dropped clash. Do not flip the flag. Diagnose (likely the dep-tagging on the carried complement) before proceeding.

- [ ] **Step 3: Run the full non-ignored suite flag-on AND flag-off** to confirm no regressions either way:

```bash
cargo test --workspace --release
RUSTDL_WEDGE_SEMANTIC_BRANCHING=1 cargo test --workspace --release
```

Expected: identical pass counts; 0 failures in both.

- [ ] **Step 4: Record the gate result** in `docs/superpowers/specs/2026-06-19-wedge-semantic-branching-sp1-design.md` under a new "## Results" section (closure counts, wine branch collapse, flag-on/off parity). Commit:

```bash
git add docs/superpowers/specs/2026-06-19-wedge-semantic-branching-sp1-design.md
git commit -m "docs(wedge): SP1 results — P0 collapse + FP=0 byte-identical corpus"
```

---

### Task 7: Flip flag to default-on + document

**Files:**
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (`semantic_branching_enabled` default)
- Modify: `CLAUDE.md` (soundness-contract / engine notes)

- [ ] **Step 1: Flip the default** in `lib.rs`:

```rust
pub(crate) fn semantic_branching_enabled() -> bool {
    std::env::var_os("RUSTDL_WEDGE_SEMANTIC_BRANCHING").is_none_or(|v| v != "0" && !v.is_empty())
}
```

- [ ] **Step 2: Re-run the FP=0 gate at the NEW default** (flag now on by default; `=0` is the opt-out):

```bash
RUSTDL_TEST_PAIR_MS=1000 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release -- --ignored --nocapture
RUSTDL_WEDGE_SEMANTIC_BRANCHING=0 RUSTDL_TEST_PAIR_MS=1000 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release -- --ignored --nocapture
```

Expected: default-on is FP=0/MISSED=0 byte-identical; `=0` reverts to the pre-SP1 closures (also FP=0/MISSED=0).

- [ ] **Step 3: Update the test guards** in `wedge_semantic_branching.rs`: the `reorder_preserves_sat_verdict` test's flag-off arm must now explicitly set `RUSTDL_WEDGE_SEMANTIC_BRANCHING=0` (since default flipped). Change the `remove_var` in that test to `set_var(.., "0")`, keep the `EnvGuard` cleanup. Run the test file to confirm green.

- [ ] **Step 4: Document** in `CLAUDE.md` — add a bullet under the `owl-dl-tableau` engine notes describing SP1 (semantic branching + reordering, env `RUSTDL_WEDGE_SEMANTIC_BRANCHING` default-on, wine collapse figure, FP=0/MISSED=0 preserved, gated opt-out). Keep the wording style of the existing phase bullets.

- [ ] **Step 5: Final clippy + fmt.** Run:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Expected: both clean.

- [ ] **Step 6: Commit.**

```bash
git add crates/owl-dl-reasoner/src/lib.rs crates/owl-dl-reasoner/tests/wedge_semantic_branching.rs CLAUDE.md
git commit -m "feat(wedge): SP1 semantic branching default-on (wine collapse, FP=0/MISSED=0 preserved)"
```

---

## Self-Review

**Spec coverage:** A (algorithm: reordering Task 3, semantic branching Task 4) ✓; B (soundness: verdict-preservation + Unsat canary Tasks 3–4, FP=0 gate Task 6) ✓; C (components: complement threading Task 1, flag Task 2) ✓; D (flag default-off→on Tasks 2,7) ✓; Validation (P0 Task 5, FP=0 Task 6, completeness canary Task 4, wine canary Task 5) ✓.

**Placeholder scan:** no TBD/TODO; all code blocks complete; P0 NO-GO path explicit.

**Type consistency:** `complements: Vec<Option<ClassId>>` (engine) ↔ built from `HashMap<ClassId,ClassId>` (HyperCache) in Task 1 Step 5; `complement_of(c) -> Option<ClassId>`, `score_disjunct(Atom, HNode, &Binding) -> u8`, `reorder_disjunction_heads(usize, HNode, &Binding) -> Vec<usize>`, `add_label(HNode, ClassId, DepSet) -> bool`, `semantic_branching_enabled() -> bool` used consistently. `Atom` variants match `clause.rs`.

**Note for implementer:** `ClassId::index()` returns `u32`; `self.complements.get(c.index() as usize)`. If the codebase forbids bare `as`, use `usize::try_from(c.index()).unwrap()` to match local style.
