# ABox Consistency Check Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Detect a tractable class of ABox-driven ontology-level inconsistencies via a sound, fast pre-check that runs before the tableau, wired into `is_consistent` and `classify` so neither path can confidently emit a normal verdict on an inconsistent input it could have caught cheaply.

**Architecture:** One new pass (`abox_consistency_check`) consumes the already-populated `Abox` struct plus the EL saturator's `Subsumers` closure. Returns `AboxVerdict::Inconsistent { reason }` or `Unknown`. Cached in a `OnceCell` field on `PreparedOntology` so classify and the per-pair loop don't recompute. Seven sound clash patterns (P1–P7) implemented one at a time, each with its own unit + negative-case test.

**Tech Stack:** Rust 2024 edition, workspace crates `owl-dl-reasoner` / `owl-dl-core` / `owl-dl-saturation`. No new external dependencies. Tests are standard `cargo test` integration tests under `crates/owl-dl-reasoner/tests/`.

**Spec:** [docs/superpowers/specs/2026-06-04-abox-consistency-check-design.md](../specs/2026-06-04-abox-consistency-check-design.md)

---

## File Structure

| File | Responsibility | Status |
|---|---|---|
| `crates/owl-dl-reasoner/src/abox_check.rs` | New. The 7-pattern check; `AboxVerdict`, `ClashReason`, `check()` entry, helper `per_individual_types()`. | Create |
| `crates/owl-dl-reasoner/src/union_find.rs` | New. `UnionFind<u32>` over individual indices. Path compression + union by rank. ~50 LoC. | Create |
| `crates/owl-dl-reasoner/src/lib.rs` | Modify. Add `abox_check_enabled()` env-gate helper; add `abox_verdict: OnceCell<AboxVerdict>` field on `PreparedOntology`; consult it in `is_consistent_internal_full`. Wire new modules. | Modify |
| `crates/owl-dl-reasoner/src/classify.rs` | Modify. Add `inconsistent: bool` field to `ClassificationStats`; new `classify_inconsistent()` helper that marks every class unsat; consult `abox_verdict()` in `classify_top_down_internal`. | Modify |
| `crates/owl-dl-cli/src/main.rs` | Modify. Print `# abox_check: ...` line in the classify banner. | Modify |
| `crates/owl-dl-reasoner/tests/abox_consistency.rs` | New. 7 pattern fixtures + 7 negative near-miss fixtures, each asserts `is_consistent`'s verdict. | Create |
| `crates/owl-dl-reasoner/tests/fixtures/abox/p*.ofn` (14 fixtures) | New. Synthetic OFN test inputs. | Create |
| `crates/owl-dl-reasoner/tests/konclude_closure_diff.rs` | Modify. Add `family_stripped_inconsistency_detected` + `family_inconsistency_detected` corpus tests (`#[ignore]`d). | Modify |

---

## Pre-flight: branch + baseline

### Task 0: Verify baseline + create working branch

**Files:** none

- [ ] **Step 1: Confirm clean working tree**

```sh
cd /data/dumontier/rustdl
git status
```

Expected: `nothing to commit, working tree clean` (or untracked files only — `.claude/settings.json`, scripts in `scripts/`, flamegraphs).

- [ ] **Step 2: Confirm we are on main and up to date**

```sh
git rev-parse --abbrev-ref HEAD
git log --oneline -3
```

Expected: `main`. Top commit should be `b9c43da spec: ABox consistency check — design`.

- [ ] **Step 3: Run the existing fast tests as a baseline**

```sh
cargo build --workspace --release 2>&1 | tail -5
cargo test -p owl-dl-reasoner --release --lib 2>&1 | tail -10
```

Expected: build succeeds; tests pass (no failures). Note the wall time — we'll compare against it after wiring.

- [ ] **Step 4: Confirm `rustdl consistent` baseline behavior on family-stripped**

```sh
timeout 5 ./target/release/rustdl consistent ontologies/real/family-stripped.ofn 2>&1 | tail -3
```

Expected: timeout (no output, exit code 124). This is the gap we're closing.

---

## Phase 1: Union-Find Helper

### Task 1: `UnionFind<u32>` module

**Files:**
- Create: `crates/owl-dl-reasoner/src/union_find.rs`
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (add `mod union_find;`)
- Test: same file (`#[cfg(test)] mod tests`)

- [ ] **Step 1: Create the file with failing tests**

Write `crates/owl-dl-reasoner/src/union_find.rs`:

```rust
//! Disjoint-set union (union-find) over `u32` indices.
//!
//! Used by the ABox consistency check (`abox_check.rs`) to track
//! merge-equivalence classes induced by `SameIndividual` axioms and
//! `FunctionalObjectProperty` / `InverseFunctionalObjectProperty`
//! inferences. The keys are indices into `Abox::individuals`, not
//! `IndividualId`s — the caller maintains the index map.
//!
//! Path compression on `find`; union by rank on `union`.

#[derive(Debug, Clone)]
pub(crate) struct UnionFind {
    parent: Vec<u32>,
    rank: Vec<u8>,
}

impl UnionFind {
    pub(crate) fn new(n: usize) -> Self {
        let parent = (0..u32::try_from(n).expect("n fits in u32")).collect();
        let rank = vec![0u8; n];
        Self { parent, rank }
    }

    pub(crate) fn find(&mut self, x: u32) -> u32 {
        let mut root = x;
        while self.parent[root as usize] != root {
            root = self.parent[root as usize];
        }
        // Path compression: point every node on the path directly at root.
        let mut cur = x;
        while self.parent[cur as usize] != root {
            let next = self.parent[cur as usize];
            self.parent[cur as usize] = root;
            cur = next;
        }
        root
    }

    /// Returns `true` iff the two elements were in distinct classes
    /// (i.e., a merge actually happened).
    pub(crate) fn union(&mut self, a: u32, b: u32) -> bool {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra == rb {
            return false;
        }
        let (ra_idx, rb_idx) = (ra as usize, rb as usize);
        match self.rank[ra_idx].cmp(&self.rank[rb_idx]) {
            std::cmp::Ordering::Less => self.parent[ra_idx] = rb,
            std::cmp::Ordering::Greater => self.parent[rb_idx] = ra,
            std::cmp::Ordering::Equal => {
                self.parent[rb_idx] = ra;
                self.rank[ra_idx] += 1;
            }
        }
        true
    }

    pub(crate) fn same(&mut self, a: u32, b: u32) -> bool {
        self.find(a) == self.find(b)
    }
}

#[cfg(test)]
mod tests {
    use super::UnionFind;

    #[test]
    fn singletons_are_distinct() {
        let mut uf = UnionFind::new(3);
        assert!(!uf.same(0, 1));
        assert!(!uf.same(1, 2));
    }

    #[test]
    fn union_merges_components() {
        let mut uf = UnionFind::new(4);
        assert!(uf.union(0, 1));
        assert!(uf.same(0, 1));
        assert!(uf.union(2, 3));
        assert!(!uf.same(0, 2));
        assert!(uf.union(1, 3));
        assert!(uf.same(0, 3));
    }

    #[test]
    fn redundant_union_returns_false() {
        let mut uf = UnionFind::new(2);
        assert!(uf.union(0, 1));
        assert!(!uf.union(0, 1));
        assert!(!uf.union(1, 0));
    }

    #[test]
    fn path_compression_after_find() {
        // Force a 3-deep chain via union order, then check find collapses it.
        let mut uf = UnionFind::new(4);
        uf.union(0, 1);
        uf.union(2, 3);
        uf.union(0, 2);
        let root = uf.find(3);
        // After find(3), parent[3] should point straight at root.
        assert_eq!(uf.parent[3], root);
    }
}
```

Add to `crates/owl-dl-reasoner/src/lib.rs` right after the existing top-level module declarations (search for the line `mod classify;` or similar — add alongside it):

```rust
mod union_find;
```

- [ ] **Step 2: Run the unit tests to verify they pass**

```sh
cargo test -p owl-dl-reasoner --lib union_find:: 2>&1 | tail -10
```

Expected:
```
running 4 tests
test union_find::tests::singletons_are_distinct ... ok
test union_find::tests::union_merges_components ... ok
test union_find::tests::redundant_union_returns_false ... ok
test union_find::tests::path_compression_after_find ... ok
```

- [ ] **Step 3: Confirm clippy is clean**

```sh
cargo clippy -p owl-dl-reasoner --all-targets -- -D warnings 2>&1 | tail -5
```

Expected: no warnings.

- [ ] **Step 4: Commit**

```sh
git add crates/owl-dl-reasoner/src/union_find.rs crates/owl-dl-reasoner/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(abox-check): T1 — UnionFind<u32> helper

Path-compressed, union-by-rank disjoint-set over u32 indices. Will
be used by P4 (SameAs∩DifferentFrom) and P5 (Functional+two-
distinct-witnesses) in the ABox consistency check pre-pass.

Spec: docs/superpowers/specs/2026-06-04-abox-consistency-check-design.md
EOF
)"
```

---

## Phase 2: Skeleton + env gate + wiring (no patterns yet)

### Task 2: `abox_check` module skeleton + `AboxVerdict` + env gate

**Files:**
- Create: `crates/owl-dl-reasoner/src/abox_check.rs`
- Modify: `crates/owl-dl-reasoner/src/lib.rs`
- Test: skeleton-only test in `crates/owl-dl-reasoner/src/abox_check.rs`

- [ ] **Step 1: Create the skeleton module**

Write `crates/owl-dl-reasoner/src/abox_check.rs`:

```rust
//! Sound ABox-driven ontology-level inconsistency pre-check.
//!
//! Runs after `collect_abox` and the EL saturator (so both the
//! `Abox` struct and `Subsumers` closure are available). Returns
//! [`AboxVerdict::Inconsistent`] on a detected clash;
//! [`AboxVerdict::Unknown`] otherwise. The caller falls through to
//! the existing tableau path on `Unknown`.
//!
//! Sound under-approximation: every positive verdict is a direct
//! semantic clash on the ABox; no inferred subsumption is created.
//!
//! Seven clash patterns implemented incrementally (P1 direct-Bot
//! assertion, P2 disjoint types per individual, P3 NegOPA-vs-OPA,
//! P4 SameAs∩DifferentFrom, P5 Functional+two-distinct-witnesses,
//! P6 Asymmetric/Irreflexive violations, P7 domain/range as a
//! stretch).
//!
//! Spec: `docs/superpowers/specs/2026-06-04-abox-consistency-check-design.md`

use owl_dl_core::ir::ClassId;
use owl_dl_core::ir::RoleId;
use owl_dl_core::ir::IndividualId;

/// Verdict from the ABox consistency check.
///
/// Sound under-approximation: `Inconsistent` is unconditional;
/// `Unknown` means "we couldn't catch a clash with the cheap
/// patterns" — caller should fall through to the full tableau.
#[derive(Debug, Clone)]
pub(crate) enum AboxVerdict {
    Inconsistent { reason: ClashReason },
    Unknown,
}

/// The specific clash the check detected. Surfaced in `RUSTDL_TRACE`
/// output and intended for a future `consistent --explain` extension
/// (not part of this project's scope).
#[derive(Debug, Clone)]
pub(crate) enum ClashReason {
    /// P1: `ClassAssertion(C, a)` with `Subsumers::is_unsatisfiable(C)`.
    AssertedBot { individual: IndividualId, class: ClassId },
    /// P2 / P7: individual `a` has both `c` and `d` in its asserted-
    /// or-derived type set, and `(c, d)` is in `told_disjoint_pairs`.
    DisjointTypes { individual: IndividualId, c: ClassId, d: ClassId },
    /// P3: positive `R(a, b)` and `NegativeObjectPropertyAssertion(R, a, b)`.
    NegOpaConflict { from: IndividualId, role: RoleId, to: IndividualId },
    /// P4 / P5: `(a, b)` in `DifferentIndividuals` and union-find
    /// (post-`SameIndividual` and post-functional-merges) finds them equal.
    SameDifferent { a: IndividualId, b: IndividualId },
    /// P5 detail: `Functional(R) ∧ R(a, b1) ∧ R(a, b2)` forced a
    /// merge of `b1` and `b2` that subsequently clashed with a
    /// `DifferentIndividuals` declaration.
    FunctionalDiff { role: RoleId, a: IndividualId, b1: IndividualId, b2: IndividualId },
    /// P6: `Asymmetric(R) ∧ R(a, b) ∧ R(b, a)`.
    AsymmetricViolation { role: RoleId, a: IndividualId, b: IndividualId },
    /// P6: `Irreflexive(R) ∧ R(a, a)` (or `R(a, b)` with `a ≡ b` after merge).
    IrreflexiveViolation { role: RoleId, a: IndividualId },
}

/// Entry point. Returns `Unknown` for now; patterns land in later tasks.
pub(crate) fn check(_prepared: &crate::PreparedOntology) -> AboxVerdict {
    AboxVerdict::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skeleton_returns_unknown_for_empty_abox() {
        // Build the tiniest InternalOntology (no axioms), wrap in
        // PreparedOntology, and confirm the skeleton check returns
        // Unknown. This guards the entry-point signature; pattern
        // tests live in tests/abox_consistency.rs.
        use owl_dl_core::ir::ConceptPool;
        use owl_dl_core::ontology::InternalOntology;
        use owl_dl_core::vocab::Vocabulary;
        let internal = InternalOntology {
            vocabulary: Vocabulary::default(),
            concepts: ConceptPool::default(),
            axioms: Vec::new(),
        };
        let prepared = crate::PreparedOntology::from_internal(internal)
            .expect("empty ontology prepares");
        assert!(matches!(check(&prepared), AboxVerdict::Unknown));
    }
}
```

> **Note:** If the field names of `InternalOntology` differ in the current source, adapt the literal — the goal is "an empty ontology, however the struct is shaped." Open `crates/owl-dl-core/src/ontology.rs` to check.

- [ ] **Step 2: Add env gate + module declaration to `lib.rs`**

Add the env helper near the other `*_enabled()` helpers (search for `snapshot_capture_enabled` to find the cluster — around line 678). Add at the end of that cluster:

```rust
/// ABox consistency-check pre-pass toggle. **Default ON.** Runs a
/// sound under-approximation check before the tableau in
/// `is_consistent` and `classify`. Set `RUSTDL_ABOX_CHECK=0` (or
/// empty) to skip the check entirely (today's tableau-only
/// behaviour). Sibling-style env helper.
///
/// Spec: `docs/superpowers/specs/2026-06-04-abox-consistency-check-design.md`
#[must_use]
pub fn abox_check_enabled() -> bool {
    std::env::var_os("RUSTDL_ABOX_CHECK").map_or(true, |v| v != "0" && !v.is_empty())
}
```

Add the module declaration near `mod union_find;`:

```rust
mod abox_check;
```

- [ ] **Step 3: Run the skeleton test**

```sh
cargo test -p owl-dl-reasoner --lib abox_check:: 2>&1 | tail -10
```

Expected:
```
test abox_check::tests::skeleton_returns_unknown_for_empty_abox ... ok
```

If `InternalOntology`'s field layout differs, fix the literal until the test compiles + passes.

- [ ] **Step 4: Confirm clippy is clean**

```sh
cargo clippy -p owl-dl-reasoner --all-targets -- -D warnings 2>&1 | tail -5
```

Expected: no warnings.

- [ ] **Step 5: Commit**

```sh
git add crates/owl-dl-reasoner/src/abox_check.rs crates/owl-dl-reasoner/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(abox-check): T2 — module skeleton, AboxVerdict, env gate

Skeleton `abox_check` module with AboxVerdict / ClashReason types
and a stub `check()` that returns Unknown. Env gate
RUSTDL_ABOX_CHECK defaults ON. Skeleton test confirms the entry
point compiles and signature is callable. No patterns wired yet.

Spec: docs/superpowers/specs/2026-06-04-abox-consistency-check-design.md
EOF
)"
```

---

### Task 3: Wire `abox_verdict` into `PreparedOntology`

**Files:**
- Modify: `crates/owl-dl-reasoner/src/lib.rs`

- [ ] **Step 1: Add a `OnceCell` field on `PreparedOntology`**

In `crates/owl-dl-reasoner/src/lib.rs`, find the `pub(crate) struct PreparedOntology { ... }` block (around line 1686). Add a new field at the end:

```rust
    /// Cached ABox consistency check verdict. Populated on first
    /// call to [`Self::abox_verdict`]. `None` until then (lazy).
    /// See [`abox_check`].
    abox_verdict: std::cell::OnceCell<abox_check::AboxVerdict>,
```

In `from_internal` (around line 1727), initialize the new field at the struct construction site (search for `model_cache: ...`, add alongside the other field initialisers):

```rust
            abox_verdict: std::cell::OnceCell::new(),
```

Add the lazy accessor on `impl PreparedOntology` (just after `from_internal`):

```rust
    /// Lazy accessor for the ABox consistency check verdict.
    /// Honours [`abox_check_enabled`]: if the gate is off, always
    /// returns `Unknown` without invoking the check.
    pub(crate) fn abox_verdict(&self) -> &abox_check::AboxVerdict {
        self.abox_verdict.get_or_init(|| {
            if abox_check_enabled() {
                abox_check::check(self)
            } else {
                abox_check::AboxVerdict::Unknown
            }
        })
    }
```

- [ ] **Step 2: Confirm the workspace still builds**

```sh
cargo build --workspace --release 2>&1 | tail -5
```

Expected: build succeeds. If `OnceCell` isn't in `std::cell` for the Rust edition we're on, use `std::sync::OnceLock` instead (and switch `&AboxVerdict` accordingly).

- [ ] **Step 3: Confirm tests still pass**

```sh
cargo test -p owl-dl-reasoner --lib 2>&1 | tail -10
```

Expected: all passing.

- [ ] **Step 4: Clippy clean**

```sh
cargo clippy -p owl-dl-reasoner --all-targets -- -D warnings 2>&1 | tail -5
```

Expected: no warnings.

- [ ] **Step 5: Commit**

```sh
git add crates/owl-dl-reasoner/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(abox-check): T3 — wire abox_verdict OnceCell on PreparedOntology

Lazy field + accessor. Honours the RUSTDL_ABOX_CHECK env gate (returns
Unknown when disabled, never calls check()). No consumer wired yet —
that lands in T4 (consistent) and T5 (classify).

Spec: docs/superpowers/specs/2026-06-04-abox-consistency-check-design.md
EOF
)"
```

---

### Task 4: Consult `abox_verdict` in `is_consistent_internal_full`

**Files:**
- Modify: `crates/owl-dl-reasoner/src/lib.rs`

- [ ] **Step 1: Refactor `is_consistent_internal_full` to use `PreparedOntology` directly**

Find the function (around line 1492). The current body is:

```rust
fn is_consistent_internal_full(
    internal: InternalOntology,
) -> Result<(bool, QueryStats), ReasonError> {
    let consistent = run_satisfiability(internal, ConceptPool::top)?;
    Ok((
        consistent,
        QueryStats {
            answered_by_saturation: false,
            pure_el_mode: false,
        },
    ))
}
```

Replace with:

```rust
fn is_consistent_internal_full(
    internal: InternalOntology,
) -> Result<(bool, QueryStats), ReasonError> {
    let prepared = PreparedOntology::from_internal(internal)?;
    // Sound pre-check: a positive verdict short-circuits the tableau.
    if let abox_check::AboxVerdict::Inconsistent { reason } = prepared.abox_verdict() {
        if std::env::var_os("RUSTDL_TRACE").is_some() {
            eprintln!("abox_check: inconsistent — {reason:?}");
        }
        return Ok((
            false,
            QueryStats { answered_by_saturation: false, pure_el_mode: false },
        ));
    }
    // Fall through: existing tableau-based satisfiability of Top.
    let consistent = prepared.decide(ConceptPool::top)?;
    Ok((
        consistent,
        QueryStats { answered_by_saturation: false, pure_el_mode: false },
    ))
}
```

> **Why the refactor:** the old version called `run_satisfiability` (which constructs `PreparedOntology` internally). We want to build `PreparedOntology` once, consult `abox_verdict`, then fall through to `prepared.decide(ConceptPool::top)` directly — avoiding a double build.

- [ ] **Step 2: Build + run the existing consistency tests**

```sh
cargo build --workspace --release 2>&1 | tail -5
cargo test -p owl-dl-reasoner --release --test '*' is_consistent 2>&1 | tail -10
```

Expected: build succeeds; any existing consistency tests still pass. Skeleton `check()` returns `Unknown`, so behaviour is unchanged.

- [ ] **Step 3: Clippy clean**

```sh
cargo clippy -p owl-dl-reasoner --all-targets -- -D warnings 2>&1 | tail -5
```

Expected: no warnings.

- [ ] **Step 4: Commit**

```sh
git add crates/owl-dl-reasoner/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(abox-check): T4 — consult abox_verdict in is_consistent

Build PreparedOntology once, consult abox_verdict before falling
through to the tableau decide(Top) call. Skeleton check returns
Unknown, so behaviour is unchanged today; patterns land next.

Spec: docs/superpowers/specs/2026-06-04-abox-consistency-check-design.md
EOF
)"
```

---

### Task 5: Wire `inconsistent` into `ClassificationStats` + classify

**Files:**
- Modify: `crates/owl-dl-reasoner/src/classify.rs`

- [ ] **Step 1: Add the stats field**

In `crates/owl-dl-reasoner/src/classify.rs`, find the `ClassificationStats` struct (around line 116). Add at the end of the field list (just before the closing `}`):

```rust
    /// ABox consistency check fired (and the verdict was
    /// `Inconsistent`). When true, every class is unsatisfiable; the
    /// classify result mirrors Konclude's behaviour on inconsistent
    /// input. See `docs/superpowers/specs/2026-06-04-abox-consistency-check-design.md`.
    pub inconsistent: bool,
```

- [ ] **Step 2: Add a helper that builds an "every class unsat" Classification**

Add this function below `classify_pure_el` (around line 680, after that function's closing brace):

```rust
/// Build a `Classification` representing an inconsistent ontology:
/// every class is unsatisfiable and therefore a subclass of every
/// other class (the trivial entailment under inconsistency). Mirrors
/// Konclude's behaviour. Used when the ABox consistency pre-check
/// fires.
fn classify_inconsistent(
    classes: Vec<String>,
    index: HashMap<String, usize>,
    fragment: FragmentClassification,
) -> Classification {
    let n = classes.len();
    let entailed = vec![vec![true; n]; n];
    let unsatisfiable_idxs: HashSet<usize> = (0..n).collect();
    let stats = ClassificationStats {
        inconsistent: true,
        fragment,
        ..ClassificationStats::default()
    };
    Classification { classes, index, entailed, unsatisfiable_idxs, stats }
}
```

- [ ] **Step 3: Consult `abox_verdict` in `classify_top_down_internal`**

In `classify_top_down_internal` (around line 809), insert the pre-check **right after** `prepared` is constructed (around line 849) and **before** the existing `let mut stats = ...` block. Find:

```rust
    let prepared = PreparedOntology::from_internal(internal.clone())?;

    // Per-class unsat probes — identical to the naive path. Reuse
    // the same parallel pattern.
    let mut stats = ClassificationStats {
```

Insert between the `let prepared = ...` line and the comment:

```rust
    // Sound ABox-driven inconsistency pre-check. If it fires, return
    // an every-class-unsatisfiable Classification (mirroring Konclude).
    if let crate::abox_check::AboxVerdict::Inconsistent { reason } = prepared.abox_verdict() {
        if std::env::var_os("RUSTDL_TRACE").is_some() {
            eprintln!("abox_check: inconsistent — {reason:?}");
        }
        return Ok(classify_inconsistent(classes, index, analyze_fragment(internal)));
    }

```

Also add the same pre-check to the *pure-EL / Horn* fast-path branch above. Find:

```rust
    if is_pure_el(internal)
        || (crate::horn_shortcircuit_enabled()
            && matches!(analyze_fragment(internal), FragmentClassification::Horn))
    {
        return Ok(classify_pure_el(internal, &classes, &index, &closure));
    }
```

Replace with:

```rust
    if is_pure_el(internal)
        || (crate::horn_shortcircuit_enabled()
            && matches!(analyze_fragment(internal), FragmentClassification::Horn))
    {
        // Build PreparedOntology just for the ABox check on the fast path
        // (the saturation closure is already computed above). On Unknown,
        // proceed with the pure-EL path; on Inconsistent, short-circuit.
        if crate::abox_check_enabled() {
            let prepared = PreparedOntology::from_internal(internal.clone())?;
            if let crate::abox_check::AboxVerdict::Inconsistent { reason } =
                prepared.abox_verdict()
            {
                if std::env::var_os("RUSTDL_TRACE").is_some() {
                    eprintln!("abox_check: inconsistent — {reason:?}");
                }
                return Ok(classify_inconsistent(classes, index, analyze_fragment(internal)));
            }
        }
        return Ok(classify_pure_el(internal, &classes, &index, &closure));
    }
```

> **Why both spots:** GALEN-style ABox-free Horn ontologies take the fast path; any ABox-bearing Horn-fragment ontology should still get the check.

- [ ] **Step 4: Build + run all existing classify tests**

```sh
cargo build --workspace --release 2>&1 | tail -5
cargo test -p owl-dl-reasoner --release 2>&1 | tail -15
```

Expected: build succeeds; all tests pass. Skeleton check returns `Unknown`, so behaviour is unchanged. If a clippy lint fires on the new `classify_inconsistent` function (e.g. `too_many_arguments` won't because it's 3, but `needless_pass_by_value` might), satisfy it inline.

- [ ] **Step 5: Clippy clean**

```sh
cargo clippy -p owl-dl-reasoner --all-targets -- -D warnings 2>&1 | tail -5
```

Expected: no warnings.

- [ ] **Step 6: Commit**

```sh
git add crates/owl-dl-reasoner/src/classify.rs
git commit -m "$(cat <<'EOF'
feat(abox-check): T5 — wire abox_verdict into classify

Add ClassificationStats.inconsistent, classify_inconsistent() helper
(every class unsat, mirroring Konclude), and a pre-check at both
classify entry points (pure-EL/Horn fast path + general top-down).
Skeleton check returns Unknown, so behaviour is unchanged today;
patterns land next.

Spec: docs/superpowers/specs/2026-06-04-abox-consistency-check-design.md
EOF
)"
```

---

### Task 6: Surface `# abox_check:` in the CLI banner

**Files:**
- Modify: `crates/owl-dl-cli/src/main.rs`

- [ ] **Step 1: Add the banner line**

In `crates/owl-dl-cli/src/main.rs`, find the existing `Command::Classify` arm (or whatever prints the `# classes: N` / `# fragment: ...` banner — search for `# fragment:` to locate). After the `# fragment:` line, add:

```rust
println!(
    "# abox_check: {}",
    if !owl_dl_reasoner::abox_check_enabled() {
        "skipped"
    } else if stats.inconsistent {
        "inconsistent"
    } else {
        "unknown"
    }
);
```

> Adapt the variable name (`stats`) to whatever the surrounding code uses for `&ClassificationStats`.

- [ ] **Step 2: Smoke-test the CLI banner**

```sh
./target/release/rustdl classify --saturation-only ontologies/real/family-stripped.ofn 2>&1 | head -10
```

Expected: somewhere in the banner you should see `# abox_check: unknown` (the patterns aren't wired yet — `unknown` is the right answer).

```sh
RUSTDL_ABOX_CHECK=0 ./target/release/rustdl classify --saturation-only ontologies/real/family-stripped.ofn 2>&1 | head -10
```

Expected: `# abox_check: skipped`.

- [ ] **Step 3: Clippy clean**

```sh
cargo clippy -p owl-dl-cli --all-targets -- -D warnings 2>&1 | tail -5
```

Expected: no warnings.

- [ ] **Step 4: Commit**

```sh
git add crates/owl-dl-cli/src/main.rs
git commit -m "$(cat <<'EOF'
feat(abox-check): T6 — CLI banner surfaces abox_check verdict

New banner line `# abox_check: inconsistent | unknown | skipped`
follows the existing `# fragment:` line. Lets users (and tests)
see at a glance whether the pre-check fired or was disabled.

Spec: docs/superpowers/specs/2026-06-04-abox-consistency-check-design.md
EOF
)"
```

---

## Phase 3: Pattern P1 — Direct ⊥ assertion

### Task 7: P1 fixtures + harness

**Files:**
- Create: `crates/owl-dl-reasoner/tests/fixtures/abox/p1_direct_bot.ofn`
- Create: `crates/owl-dl-reasoner/tests/fixtures/abox/p1_no_bot.ofn`
- Create: `crates/owl-dl-reasoner/tests/abox_consistency.rs`
- Modify: `crates/owl-dl-reasoner/src/abox_check.rs` (implement P1)

- [ ] **Step 1: Write the P1-positive fixture**

`crates/owl-dl-reasoner/tests/fixtures/abox/p1_direct_bot.ofn`:

```
Prefix(:=<http://t/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(rdf:=<http://www.w3.org/1999/02/22-rdf-syntax-ns#>)
Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)
Prefix(xml:=<http://www.w3.org/XML/1998/namespace>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://t/p1>
  Declaration(Class(:Unsat))
  Declaration(NamedIndividual(:a))
  SubClassOf(:Unsat owl:Nothing)
  ClassAssertion(:Unsat :a)
)
```

- [ ] **Step 2: Write the P1-negative fixture (near-miss)**

`crates/owl-dl-reasoner/tests/fixtures/abox/p1_no_bot.ofn`:

```
Prefix(:=<http://t/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(rdf:=<http://www.w3.org/1999/02/22-rdf-syntax-ns#>)
Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)
Prefix(xml:=<http://www.w3.org/XML/1998/namespace>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://t/p1n>
  Declaration(Class(:Unsat))
  Declaration(Class(:Sat))
  Declaration(NamedIndividual(:a))
  SubClassOf(:Unsat owl:Nothing)
  ClassAssertion(:Sat :a)
)
```

> The Unsat class exists but no individual is asserted in it.

- [ ] **Step 3: Write the test harness file with the P1 cases**

`crates/owl-dl-reasoner/tests/abox_consistency.rs`:

```rust
//! Pattern unit tests for the ABox consistency pre-check.
//!
//! Each pattern (P1–P7) has a positive fixture (asserts
//! `is_consistent → false`) and a negative near-miss (asserts
//! `is_consistent → true`). Sound-positive AND sound-negative
//! coverage.
//!
//! Spec: `docs/superpowers/specs/2026-06-04-abox-consistency-check-design.md`

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::is_consistent;
use std::fs;
use std::io::Cursor;
use std::path::Path;

const FIXTURE_DIR: &str = "tests/fixtures/abox";

fn check_consistency(name: &str) -> bool {
    let path = Path::new(FIXTURE_DIR).join(format!("{name}.ofn"));
    let src = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    is_consistent(&onto).expect("is_consistent succeeds")
}

// ── P1: Direct ⊥ assertion ──────────────────────────────────────────

#[test]
fn p1_direct_bot_is_inconsistent() {
    assert!(!check_consistency("p1_direct_bot"),
        "P1: ClassAssertion(C, a) + C ⊑ ⊥ should be inconsistent");
}

#[test]
fn p1_no_bot_assertion_is_consistent() {
    assert!(check_consistency("p1_no_bot"),
        "P1 negative: Unsat class with no asserted member should stay consistent");
}
```

- [ ] **Step 4: Run the new tests — they should FAIL (P1 positive) and PASS (P1 negative)**

```sh
cargo test -p owl-dl-reasoner --release --test abox_consistency 2>&1 | tail -15
```

Expected:
- `p1_direct_bot_is_inconsistent` — **fails** (skeleton check returns Unknown → tableau runs; the existing tableau may or may not catch the trivial case; in practice it does, because `decide(Top)` clashes immediately on `a ⊑ Unsat ⊑ ⊥`. If it passes already, that's fine — it confirms the existing path handles this case, and the new fast path will be even faster.)
- `p1_no_bot_assertion_is_consistent` — passes.

If both pass without P1 wired, the win we measure is purely a **performance** win on P1 (skip the tableau). The soundness is already provided by the existing tableau for this case.

- [ ] **Step 5: Implement P1 in `abox_check.rs`**

Replace the stub `check()` body in `crates/owl-dl-reasoner/src/abox_check.rs` with the P1 implementation:

```rust
pub(crate) fn check(prepared: &crate::PreparedOntology) -> AboxVerdict {
    // Early return: no individuals → no ABox → no clash possible.
    if prepared.abox.individuals.is_empty() {
        return AboxVerdict::Unknown;
    }
    // P1: direct-⊥ assertion. For each ClassAssertion(C, a), if
    // C = Atomic(c) and the EL saturator deems `c` unsatisfiable,
    // the ABox is inconsistent.
    let closure = &prepared.closure;
    let pool = &prepared.pool;
    for &(individual, class_concept) in &prepared.abox.class_assertions {
        if let owl_dl_core::ir::ConceptExpr::Atomic(c) = pool.get(class_concept) {
            if closure.is_unsatisfiable(*c) {
                return AboxVerdict::Inconsistent {
                    reason: ClashReason::AssertedBot { individual, class: *c },
                };
            }
        }
    }
    AboxVerdict::Unknown
}
```

> **Two field additions needed on `PreparedOntology`** to make this compile. In `crates/owl-dl-reasoner/src/lib.rs`, add to the struct:
>
> ```rust
>     /// Public-to-crate: ABox check needs read access. See `abox_check`.
>     pub(crate) closure: owl_dl_saturation::Subsumers,
> ```
>
> The fields `pool` and `abox` already exist (search for them in the struct definition). `pool` is the field named — check current names; might be called `concepts` (from `InternalOntology::concepts`). Match whatever's there.
>
> In `from_internal`, compute `let closure = owl_dl_saturation::saturate(&internal);` BEFORE `let abox = collect_abox(&mut internal);` (`collect_abox` mutates the pool but doesn't touch the closure). Store both. If `closure` is already computed somewhere in the existing flow (e.g. inside `hyper_cache::build`), reuse that — search for existing `saturate(` calls in `from_internal` before adding a new one.

- [ ] **Step 6: Run the P1 tests — both should now pass**

```sh
cargo test -p owl-dl-reasoner --release --test abox_consistency p1_ 2>&1 | tail -10
```

Expected:
```
test p1_direct_bot_is_inconsistent ... ok
test p1_no_bot_assertion_is_consistent ... ok
```

Both pass. The win is correct: positive fixture is detected by the cheap check; negative stays consistent.

- [ ] **Step 7: Confirm no regression in existing tests**

```sh
cargo test -p owl-dl-reasoner --release --lib 2>&1 | tail -5
cargo test -p owl-dl-reasoner --release --test konclude_closure_diff 2>&1 | tail -10
```

Expected: all passing. This is the FP=0 tripwire from the spec — if the new check ever flags a consistent ontology as inconsistent, every closure-diff test for that ontology dies.

- [ ] **Step 8: Clippy clean**

```sh
cargo clippy -p owl-dl-reasoner --all-targets -- -D warnings 2>&1 | tail -5
```

Expected: no warnings.

- [ ] **Step 9: Commit**

```sh
git add crates/owl-dl-reasoner/src/abox_check.rs crates/owl-dl-reasoner/src/lib.rs \
        crates/owl-dl-reasoner/tests/abox_consistency.rs \
        crates/owl-dl-reasoner/tests/fixtures/abox/p1_direct_bot.ofn \
        crates/owl-dl-reasoner/tests/fixtures/abox/p1_no_bot.ofn
git commit -m "$(cat <<'EOF'
feat(abox-check): T7 — P1 direct-Bot assertion

ClassAssertion(C, a) with C atomic and Subsumers::is_unsatisfiable(C)
→ Inconsistent. Sound: every flagged class is genuinely subsumed by
⊥ in the EL closure, so any individual asserted in it forces empty
model. Two fixtures + two tests (positive + near-miss negative).
Per-individual type-set helper (P2) lands next.

Spec: docs/superpowers/specs/2026-06-04-abox-consistency-check-design.md
EOF
)"
```

---

## Phase 4: Pattern P2 — Pairwise disjoint types

### Task 8: Per-individual type-set helper + P2 fixtures + impl

**Files:**
- Modify: `crates/owl-dl-reasoner/src/abox_check.rs`
- Create: `crates/owl-dl-reasoner/tests/fixtures/abox/p2_disjoint_types.ofn`
- Create: `crates/owl-dl-reasoner/tests/fixtures/abox/p2_disjoint_different_individuals.ofn`
- Modify: `crates/owl-dl-reasoner/tests/abox_consistency.rs`

- [ ] **Step 1: Write the P2-positive fixture**

`crates/owl-dl-reasoner/tests/fixtures/abox/p2_disjoint_types.ofn`:

```
Prefix(:=<http://t/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(rdf:=<http://www.w3.org/1999/02/22-rdf-syntax-ns#>)
Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)
Prefix(xml:=<http://www.w3.org/XML/1998/namespace>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://t/p2>
  Declaration(Class(:Male))
  Declaration(Class(:Female))
  Declaration(NamedIndividual(:pat))
  DisjointClasses(:Male :Female)
  ClassAssertion(:Male :pat)
  ClassAssertion(:Female :pat)
)
```

- [ ] **Step 2: Write the P2-negative fixture**

`crates/owl-dl-reasoner/tests/fixtures/abox/p2_disjoint_different_individuals.ofn`:

```
Prefix(:=<http://t/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(rdf:=<http://www.w3.org/1999/02/22-rdf-syntax-ns#>)
Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)
Prefix(xml:=<http://www.w3.org/XML/1998/namespace>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://t/p2n>
  Declaration(Class(:Male))
  Declaration(Class(:Female))
  Declaration(NamedIndividual(:pat))
  Declaration(NamedIndividual(:chris))
  DisjointClasses(:Male :Female)
  ClassAssertion(:Male :pat)
  ClassAssertion(:Female :chris)
)
```

- [ ] **Step 3: Add the P2 tests to `abox_consistency.rs`**

Append to `crates/owl-dl-reasoner/tests/abox_consistency.rs`:

```rust
// ── P2: Disjoint types on the same individual ───────────────────────

#[test]
fn p2_disjoint_types_is_inconsistent() {
    assert!(!check_consistency("p2_disjoint_types"),
        "P2: same individual asserted in two disjoint classes should be inconsistent");
}

#[test]
fn p2_disjoint_different_individuals_is_consistent() {
    assert!(check_consistency("p2_disjoint_different_individuals"),
        "P2 negative: disjoint classes asserted on DIFFERENT individuals should stay consistent");
}
```

- [ ] **Step 4: Run the new tests — positive should fail (no P2 yet), negative should pass**

```sh
cargo test -p owl-dl-reasoner --release --test abox_consistency p2_ 2>&1 | tail -10
```

Expected: positive may pass already (existing tableau catches it) or fail (timeout / heavy tableau call). Either way the negative should pass.

- [ ] **Step 5: Implement P2 in `abox_check.rs`**

Replace the `check()` body:

```rust
pub(crate) fn check(prepared: &crate::PreparedOntology) -> AboxVerdict {
    if prepared.abox.individuals.is_empty() {
        return AboxVerdict::Unknown;
    }
    let closure = &prepared.closure;
    let pool = &prepared.pool;

    // P1: direct-⊥ assertion (unchanged).
    for &(individual, class_concept) in &prepared.abox.class_assertions {
        if let owl_dl_core::ir::ConceptExpr::Atomic(c) = pool.get(class_concept) {
            if closure.is_unsatisfiable(*c) {
                return AboxVerdict::Inconsistent {
                    reason: ClashReason::AssertedBot { individual, class: *c },
                };
            }
        }
    }

    // Build per-individual type-set: index → HashSet<ClassId>.
    // For each ClassAssertion(C, a) with C atomic, insert c and
    // every subsumer of c per Subsumers::subsumers_of.
    let n = prepared.abox.individuals.len();
    let ind_index: std::collections::HashMap<owl_dl_core::ir::IndividualId, usize> =
        prepared.abox.individuals.iter().enumerate().map(|(i, (id, _))| (*id, i)).collect();
    let mut types: Vec<std::collections::HashSet<owl_dl_core::ir::ClassId>> =
        vec![std::collections::HashSet::new(); n];
    for &(individual, class_concept) in &prepared.abox.class_assertions {
        if let Some(&i) = ind_index.get(&individual) {
            if let owl_dl_core::ir::ConceptExpr::Atomic(c) = pool.get(class_concept) {
                types[i].insert(*c);
                for s in closure.subsumers_of(*c) {
                    types[i].insert(s);
                }
            }
        }
    }

    // P2: pairwise told-disjoint over types[i]. Iterate ascending
    // pairs of `c` in types[i]; for each, ask told.are_told_disjoint.
    let told = &prepared.told;
    for (i, type_set) in types.iter().enumerate() {
        let (individual, _) = prepared.abox.individuals[i];
        let cs: Vec<_> = type_set.iter().copied().collect();
        for a in 0..cs.len() {
            for b in (a + 1)..cs.len() {
                if told.are_told_disjoint(cs[a], cs[b]) {
                    return AboxVerdict::Inconsistent {
                        reason: ClashReason::DisjointTypes { individual, c: cs[a], d: cs[b] },
                    };
                }
            }
        }
    }

    AboxVerdict::Unknown
}
```

> **`prepared.told` field requirement:** add `pub(crate) told: owl_dl_core::told::ToldTables,` to `PreparedOntology` in `lib.rs` and populate it in `from_internal` via `let told = owl_dl_core::told::build_told_tables(&internal);` (the free constructor in `crates/owl-dl-core/src/told.rs:106`). If `told` is already built elsewhere in the pipeline (e.g., used by absorb), reuse the existing one — don't build twice.

- [ ] **Step 6: Run the P2 tests — both should pass**

```sh
cargo test -p owl-dl-reasoner --release --test abox_consistency 2>&1 | tail -10
```

Expected: P1 + P2 tests all pass.

- [ ] **Step 7: Confirm no regression**

```sh
cargo test -p owl-dl-reasoner --release --lib 2>&1 | tail -5
cargo test -p owl-dl-reasoner --release --test konclude_closure_diff 2>&1 | tail -10
```

Expected: all passing.

- [ ] **Step 8: Clippy clean**

```sh
cargo clippy -p owl-dl-reasoner --all-targets -- -D warnings 2>&1 | tail -5
```

Expected: no warnings.

- [ ] **Step 9: Commit**

```sh
git add crates/owl-dl-reasoner/src/abox_check.rs crates/owl-dl-reasoner/src/lib.rs \
        crates/owl-dl-reasoner/tests/abox_consistency.rs \
        crates/owl-dl-reasoner/tests/fixtures/abox/p2_disjoint_types.ofn \
        crates/owl-dl-reasoner/tests/fixtures/abox/p2_disjoint_different_individuals.ofn
git commit -m "$(cat <<'EOF'
feat(abox-check): T8 — P2 disjoint types per individual

Per-individual atomic-type set built from ClassAssertions + EL closure
subsumers_of. Pairwise told-disjoint scan via ToldTables.
are_told_disjoint. Sound: only direct DisjointClasses asserts produce
told_disjoint_pairs, and subsumers_of is a sound EL closure.
Fixtures: same individual in Male+Female (positive); different
individuals one each (negative).

Spec: docs/superpowers/specs/2026-06-04-abox-consistency-check-design.md
EOF
)"
```

---

## Phase 5: Pattern P3 — NegOPA vs OPA

### Task 9: P3 fixtures + impl

**Files:**
- Modify: `crates/owl-dl-reasoner/src/abox_check.rs`
- Create: `crates/owl-dl-reasoner/tests/fixtures/abox/p3_neg_opa.ofn`
- Create: `crates/owl-dl-reasoner/tests/fixtures/abox/p3_neg_opa_no_clash.ofn`
- Modify: `crates/owl-dl-reasoner/tests/abox_consistency.rs`

- [ ] **Step 1: Write the P3-positive fixture**

`crates/owl-dl-reasoner/tests/fixtures/abox/p3_neg_opa.ofn`:

```
Prefix(:=<http://t/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(rdf:=<http://www.w3.org/1999/02/22-rdf-syntax-ns#>)
Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)
Prefix(xml:=<http://www.w3.org/XML/1998/namespace>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://t/p3>
  Declaration(ObjectProperty(:hasFather))
  Declaration(NamedIndividual(:alice))
  Declaration(NamedIndividual(:bob))
  ObjectPropertyAssertion(:hasFather :alice :bob)
  NegativeObjectPropertyAssertion(:hasFather :alice :bob)
)
```

- [ ] **Step 2: Write the P3-negative fixture (different objects)**

`crates/owl-dl-reasoner/tests/fixtures/abox/p3_neg_opa_no_clash.ofn`:

```
Prefix(:=<http://t/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(rdf:=<http://www.w3.org/1999/02/22-rdf-syntax-ns#>)
Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)
Prefix(xml:=<http://www.w3.org/XML/1998/namespace>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://t/p3n>
  Declaration(ObjectProperty(:hasFather))
  Declaration(NamedIndividual(:alice))
  Declaration(NamedIndividual(:bob))
  Declaration(NamedIndividual(:carl))
  ObjectPropertyAssertion(:hasFather :alice :bob)
  NegativeObjectPropertyAssertion(:hasFather :alice :carl)
)
```

- [ ] **Step 3: Add the P3 tests**

Append to `crates/owl-dl-reasoner/tests/abox_consistency.rs`:

```rust
// ── P3: NegativeObjectPropertyAssertion vs ObjectPropertyAssertion ──

#[test]
fn p3_neg_opa_is_inconsistent() {
    assert!(!check_consistency("p3_neg_opa"),
        "P3: positive OPA + NegOPA on same (a, R, b) should be inconsistent");
}

#[test]
fn p3_neg_opa_no_clash_is_consistent() {
    assert!(check_consistency("p3_neg_opa_no_clash"),
        "P3 negative: NegOPA to a DIFFERENT target should stay consistent");
}
```

- [ ] **Step 4: Run the new tests — positive likely passes (tableau), negative should pass**

```sh
cargo test -p owl-dl-reasoner --release --test abox_consistency p3_ 2>&1 | tail -10
```

- [ ] **Step 5: Implement P3 in `abox_check.rs`**

> **Note on data shape:** `Abox::negative_property_assertions` stores `(individual, ∀R.¬{b}_concept_id)` — the NegOPA was lowered to a `∀` form during `collect_abox`. We need to **recover** the original `(a, R, b)` triple. Add a sibling raw field to `Abox` during this task, or scan `internal.axioms` directly for `Axiom::NegativeObjectPropertyAssertion`. The raw-field approach keeps `abox_check` self-contained.
>
> Add to the `Abox` struct in `crates/owl-dl-reasoner/src/lib.rs` (around line 1937):
>
> ```rust
>     /// P3 input: raw `(subject, role_id, object)` triples from
>     /// `NegativeObjectPropertyAssertion` axioms. Polarity normalised
>     /// (inverse-role assertions swap subject/object). The `∀`-form
>     /// stored in `negative_property_assertions` is for the tableau;
>     /// this is for the ABox consistency check.
>     pub(crate) negative_property_triples: Vec<(IndividualId, RoleId, IndividualId)>,
> ```
>
> Populate it inside `collect_abox` in the same arm that builds `negative_property_assertions`. Add just before the `let nom_b = ...` line:
>
> ```rust
>     let (from, to) = if role.is_inverse() {
>         (*object, *subject)
>     } else {
>         (*subject, *object)
>     };
>     abox.negative_property_triples.push((from, role.role_id(), to));
> ```

Then extend `check()` with P3 — insert before the final `AboxVerdict::Unknown`:

```rust
    // P3: NegativeObjectPropertyAssertion vs positive
    // ObjectPropertyAssertion. Build a HashSet of positive triples
    // and test each negative against it.
    let pos: std::collections::HashSet<(owl_dl_core::ir::IndividualId,
                                        owl_dl_core::ir::RoleId,
                                        owl_dl_core::ir::IndividualId)> =
        prepared.abox.property_assertions.iter().copied().collect();
    for &(from, role, to) in &prepared.abox.negative_property_triples {
        if pos.contains(&(from, role, to)) {
            return AboxVerdict::Inconsistent {
                reason: ClashReason::NegOpaConflict { from, role, to },
            };
        }
        // Role-hierarchy upward propagation: a positive assertion on
        // any super-role of `role` also implies the negated one.
        for &super_role in prepared.hierarchy.super_roles(role) {
            if pos.contains(&(from, super_role, to)) {
                return AboxVerdict::Inconsistent {
                    reason: ClashReason::NegOpaConflict { from, role, to },
                };
            }
        }
    }
```

> **`prepared.hierarchy.super_roles(role)` API:** confirmed at `crates/owl-dl-core/src/role_hierarchy.rs:115` — returns `&[RoleId]`. The field name on `PreparedOntology` is already `hierarchy: RoleHierarchy` (lib.rs:1689). The field is currently visibility-private; widen the declaration to `pub(crate) hierarchy: RoleHierarchy,` so `abox_check` can read it. No new field needed.

- [ ] **Step 6: Run the P3 tests — both should pass**

```sh
cargo test -p owl-dl-reasoner --release --test abox_consistency 2>&1 | tail -10
```

Expected: all P1/P2/P3 tests pass.

- [ ] **Step 7: Confirm no regression**

```sh
cargo test -p owl-dl-reasoner --release 2>&1 | tail -5
```

Expected: all passing.

- [ ] **Step 8: Clippy clean**

```sh
cargo clippy -p owl-dl-reasoner --all-targets -- -D warnings 2>&1 | tail -5
```

Expected: no warnings.

- [ ] **Step 9: Commit**

```sh
git add crates/owl-dl-reasoner/src/abox_check.rs crates/owl-dl-reasoner/src/lib.rs \
        crates/owl-dl-reasoner/tests/abox_consistency.rs \
        crates/owl-dl-reasoner/tests/fixtures/abox/p3_neg_opa.ofn \
        crates/owl-dl-reasoner/tests/fixtures/abox/p3_neg_opa_no_clash.ofn
git commit -m "$(cat <<'EOF'
feat(abox-check): T9 — P3 NegOPA vs OPA

HashSet of positive (a, R, b) triples; scan each NegOPA against it
and against role-hierarchy super-role assertions. New Abox field
negative_property_triples (raw, polarity-normalised) feeds the
check without re-walking the ∀-encoded form. Sound: NegOPA(R, a, b)
+ OPA(R', a, b) with R' ⊑ R is a direct semantic clash.

Spec: docs/superpowers/specs/2026-06-04-abox-consistency-check-design.md
EOF
)"
```

---

## Phase 6: Pattern P4 — SameAs ∩ DifferentFrom

### Task 10: P4 fixtures + impl

**Files:**
- Modify: `crates/owl-dl-reasoner/src/abox_check.rs`
- Create: `crates/owl-dl-reasoner/tests/fixtures/abox/p4_same_different.ofn`
- Create: `crates/owl-dl-reasoner/tests/fixtures/abox/p4_same_without_different.ofn`
- Modify: `crates/owl-dl-reasoner/tests/abox_consistency.rs`

- [ ] **Step 1: Write the P4-positive fixture**

`crates/owl-dl-reasoner/tests/fixtures/abox/p4_same_different.ofn`:

```
Prefix(:=<http://t/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(rdf:=<http://www.w3.org/1999/02/22-rdf-syntax-ns#>)
Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)
Prefix(xml:=<http://www.w3.org/XML/1998/namespace>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://t/p4>
  Declaration(NamedIndividual(:a))
  Declaration(NamedIndividual(:b))
  Declaration(NamedIndividual(:c))
  SameIndividual(:a :b)
  SameIndividual(:b :c)
  DifferentIndividuals(:a :c)
)
```

- [ ] **Step 2: Write the P4-negative fixture**

`crates/owl-dl-reasoner/tests/fixtures/abox/p4_same_without_different.ofn`:

```
Prefix(:=<http://t/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(rdf:=<http://www.w3.org/1999/02/22-rdf-syntax-ns#>)
Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)
Prefix(xml:=<http://www.w3.org/XML/1998/namespace>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://t/p4n>
  Declaration(NamedIndividual(:a))
  Declaration(NamedIndividual(:b))
  Declaration(NamedIndividual(:c))
  Declaration(NamedIndividual(:d))
  SameIndividual(:a :b)
  DifferentIndividuals(:c :d)
)
```

- [ ] **Step 3: Add the P4 tests**

Append to `crates/owl-dl-reasoner/tests/abox_consistency.rs`:

```rust
// ── P4: SameAs ∩ DifferentFrom (transitive) ─────────────────────────

#[test]
fn p4_same_then_different_is_inconsistent() {
    assert!(!check_consistency("p4_same_different"),
        "P4: SameIndividual(a,b) + SameIndividual(b,c) + DifferentIndividuals(a,c) inconsistent");
}

#[test]
fn p4_same_without_different_is_consistent() {
    assert!(check_consistency("p4_same_without_different"),
        "P4 negative: SameAs(a,b) + DifferentFrom(c,d) over disjoint pairs is consistent");
}
```

- [ ] **Step 4: Run new tests — positive likely fails or slow, negative passes**

```sh
cargo test -p owl-dl-reasoner --release --test abox_consistency p4_ 2>&1 | tail -10
```

- [ ] **Step 5: Implement P4 in `abox_check.rs`**

Add at the top of the file:

```rust
use crate::union_find::UnionFind;
```

In the `check()` body, after P3, insert P4:

```rust
    // P4: SameAs ∩ DifferentFrom. Build union-find over individual
    // indices via same_pairs; check each different_pair against it.
    let n_ind = prepared.abox.individuals.len();
    let mut uf = UnionFind::new(n_ind);
    // ind_index was computed earlier for P2; reuse it. (If P2 was
    // ever skipped, recompute here.)
    for &(a, b) in &prepared.abox.same_pairs {
        if let (Some(&i), Some(&j)) = (ind_index.get(&a), ind_index.get(&b)) {
            uf.union(u32::try_from(i).expect("fits"), u32::try_from(j).expect("fits"));
        }
    }
    for &(a, b) in &prepared.abox.different_pairs {
        if let (Some(&i), Some(&j)) = (ind_index.get(&a), ind_index.get(&b)) {
            if uf.same(u32::try_from(i).expect("fits"), u32::try_from(j).expect("fits")) {
                return AboxVerdict::Inconsistent {
                    reason: ClashReason::SameDifferent { a, b },
                };
            }
        }
    }
```

> **Hold onto the `uf` value** — P5 will extend it with functional-role merges before the next clash check.

- [ ] **Step 6: Run all tests**

```sh
cargo test -p owl-dl-reasoner --release --test abox_consistency 2>&1 | tail -15
```

Expected: P1–P4 all pass.

- [ ] **Step 7: Confirm no regression**

```sh
cargo test -p owl-dl-reasoner --release 2>&1 | tail -5
```

Expected: all passing.

- [ ] **Step 8: Clippy clean**

```sh
cargo clippy -p owl-dl-reasoner --all-targets -- -D warnings 2>&1 | tail -5
```

Expected: no warnings.

- [ ] **Step 9: Commit**

```sh
git add crates/owl-dl-reasoner/src/abox_check.rs \
        crates/owl-dl-reasoner/tests/abox_consistency.rs \
        crates/owl-dl-reasoner/tests/fixtures/abox/p4_same_different.ofn \
        crates/owl-dl-reasoner/tests/fixtures/abox/p4_same_without_different.ofn
git commit -m "$(cat <<'EOF'
feat(abox-check): T10 — P4 SameAs ∩ DifferentFrom

Union-find over individual indices populated from same_pairs; each
different_pairs entry checked against it. Sound + transitive (chains
of SameAs through union-find).

Spec: docs/superpowers/specs/2026-06-04-abox-consistency-check-design.md
EOF
)"
```

---

## Phase 7: Pattern P5 — Functional + two distinct witnesses

### Task 11: P5 fixtures + impl

**Files:**
- Modify: `crates/owl-dl-reasoner/src/abox_check.rs`
- Create: `crates/owl-dl-reasoner/tests/fixtures/abox/p5_functional_diff.ofn`
- Create: `crates/owl-dl-reasoner/tests/fixtures/abox/p5_functional_same_target.ofn`
- Modify: `crates/owl-dl-reasoner/tests/abox_consistency.rs`

- [ ] **Step 1: Write the P5-positive fixture**

`crates/owl-dl-reasoner/tests/fixtures/abox/p5_functional_diff.ofn`:

```
Prefix(:=<http://t/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(rdf:=<http://www.w3.org/1999/02/22-rdf-syntax-ns#>)
Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)
Prefix(xml:=<http://www.w3.org/XML/1998/namespace>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://t/p5>
  Declaration(ObjectProperty(:hasFather))
  Declaration(NamedIndividual(:child))
  Declaration(NamedIndividual(:dad1))
  Declaration(NamedIndividual(:dad2))
  FunctionalObjectProperty(:hasFather)
  ObjectPropertyAssertion(:hasFather :child :dad1)
  ObjectPropertyAssertion(:hasFather :child :dad2)
  DifferentIndividuals(:dad1 :dad2)
)
```

- [ ] **Step 2: Write the P5-negative fixture (two assertions to the SAME target → no clash)**

`crates/owl-dl-reasoner/tests/fixtures/abox/p5_functional_same_target.ofn`:

```
Prefix(:=<http://t/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(rdf:=<http://www.w3.org/1999/02/22-rdf-syntax-ns#>)
Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)
Prefix(xml:=<http://www.w3.org/XML/1998/namespace>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://t/p5n>
  Declaration(ObjectProperty(:hasFather))
  Declaration(NamedIndividual(:child))
  Declaration(NamedIndividual(:dad1))
  Declaration(NamedIndividual(:dad2))
  FunctionalObjectProperty(:hasFather)
  ObjectPropertyAssertion(:hasFather :child :dad1)
  ObjectPropertyAssertion(:hasFather :child :dad2)
)
```

> Without `DifferentIndividuals(dad1, dad2)`, the OWA allows `dad1 = dad2` and the ontology is consistent.

- [ ] **Step 3: Add the P5 tests**

Append to `crates/owl-dl-reasoner/tests/abox_consistency.rs`:

```rust
// ── P5: Functional role + two distinct witnesses ────────────────────

#[test]
fn p5_functional_distinct_witnesses_is_inconsistent() {
    assert!(!check_consistency("p5_functional_diff"),
        "P5: Functional(R) + R(a,b1) + R(a,b2) + Different(b1,b2) should be inconsistent");
}

#[test]
fn p5_functional_no_different_is_consistent() {
    assert!(check_consistency("p5_functional_same_target"),
        "P5 negative: same two facts WITHOUT DifferentIndividuals stay consistent (OWA)");
}
```

- [ ] **Step 4: Run the new tests — positive likely fails or slow**

```sh
cargo test -p owl-dl-reasoner --release --test abox_consistency p5_ 2>&1 | tail -10
```

- [ ] **Step 5: Implement P5 in `abox_check.rs`**

P5 extends the same `uf` from P4. Insert between the P4 same/different merging and the P4 different-pair clash check, OR (cleaner) run P5 AFTER the P4 clash check and re-check with a separate per-merge loop. Use the **AFTER** version — it keeps the patterns visually separable:

After the P4 different-pair clash check, add:

```rust
    // P5: Functional + two-distinct-witnesses. For each functional
    // role R, group property_assertions by `(from, role)`; for each
    // group with ≥2 distinct `to`'s, merge them in uf. After every
    // batch of merges, re-test all `different_pairs` (cheap: only
    // touched pairs can fire). Same for inverse-functional via the
    // swapped role.
    use std::collections::HashMap;
    let mut functional_roles: std::collections::HashSet<owl_dl_core::ir::RoleId> =
        std::collections::HashSet::new();
    let mut inverse_functional_roles: std::collections::HashSet<owl_dl_core::ir::RoleId> =
        std::collections::HashSet::new();
    for ax in &prepared.axioms {
        match ax {
            owl_dl_core::ontology::Axiom::FunctionalRole(r) => {
                functional_roles.insert(r.role_id());
            }
            owl_dl_core::ontology::Axiom::InverseFunctionalRole(r) => {
                inverse_functional_roles.insert(r.role_id());
            }
            _ => {}
        }
    }

    // Group (from, role) → Vec<to>.
    let mut by_from_role: HashMap<(owl_dl_core::ir::IndividualId,
                                   owl_dl_core::ir::RoleId),
                                   Vec<owl_dl_core::ir::IndividualId>> = HashMap::new();
    for &(from, role, to) in &prepared.abox.property_assertions {
        if functional_roles.contains(&role) {
            by_from_role.entry((from, role)).or_default().push(to);
        }
    }
    for ((from, role), tos) in &by_from_role {
        if tos.len() < 2 { continue; }
        let first = tos[0];
        let Some(&i0) = ind_index.get(&first) else { continue };
        for &b in &tos[1..] {
            if let Some(&j) = ind_index.get(&b) {
                if uf.union(u32::try_from(i0).expect("fits"),
                            u32::try_from(j).expect("fits")) {
                    // A new merge happened. Re-check all different_pairs.
                    for &(da, db) in &prepared.abox.different_pairs {
                        if let (Some(&ip), Some(&jp)) =
                            (ind_index.get(&da), ind_index.get(&db))
                        {
                            if uf.same(u32::try_from(ip).expect("fits"),
                                       u32::try_from(jp).expect("fits"))
                            {
                                return AboxVerdict::Inconsistent {
                                    reason: ClashReason::FunctionalDiff {
                                        role: *role, a: *from, b1: first, b2: b,
                                    },
                                };
                            }
                        }
                    }
                }
            }
        }
    }

    // Inverse-functional: group (role, to) → Vec<from>, merge as above.
    let mut by_role_to: HashMap<(owl_dl_core::ir::RoleId,
                                 owl_dl_core::ir::IndividualId),
                                 Vec<owl_dl_core::ir::IndividualId>> = HashMap::new();
    for &(from, role, to) in &prepared.abox.property_assertions {
        if inverse_functional_roles.contains(&role) {
            by_role_to.entry((role, to)).or_default().push(from);
        }
    }
    for ((role, to), froms) in &by_role_to {
        if froms.len() < 2 { continue; }
        let first = froms[0];
        let Some(&i0) = ind_index.get(&first) else { continue };
        for &a in &froms[1..] {
            if let Some(&j) = ind_index.get(&a) {
                if uf.union(u32::try_from(i0).expect("fits"),
                            u32::try_from(j).expect("fits")) {
                    for &(da, db) in &prepared.abox.different_pairs {
                        if let (Some(&ip), Some(&jp)) =
                            (ind_index.get(&da), ind_index.get(&db))
                        {
                            if uf.same(u32::try_from(ip).expect("fits"),
                                       u32::try_from(jp).expect("fits"))
                            {
                                return AboxVerdict::Inconsistent {
                                    reason: ClashReason::FunctionalDiff {
                                        role: *role, a: *to, b1: first, b2: a,
                                    },
                                };
                            }
                        }
                    }
                }
            }
        }
    }
```

> **`prepared.axioms` field:** if not currently exposed on `PreparedOntology`, add it. Look at the `InternalOntology` field passed to `from_internal` — the axiom list is `internal.axioms`. Store it on `PreparedOntology` as `pub(crate) axioms: Vec<owl_dl_core::ontology::Axiom>` (clone is fine; we only need read access).

- [ ] **Step 6: Run all tests**

```sh
cargo test -p owl-dl-reasoner --release --test abox_consistency 2>&1 | tail -15
```

Expected: P1–P5 all pass.

- [ ] **Step 7: Confirm no regression**

```sh
cargo test -p owl-dl-reasoner --release 2>&1 | tail -5
```

Expected: all passing.

- [ ] **Step 8: Clippy clean**

```sh
cargo clippy -p owl-dl-reasoner --all-targets -- -D warnings 2>&1 | tail -5
```

Expected: no warnings.

- [ ] **Step 9: Commit**

```sh
git add crates/owl-dl-reasoner/src/abox_check.rs crates/owl-dl-reasoner/src/lib.rs \
        crates/owl-dl-reasoner/tests/abox_consistency.rs \
        crates/owl-dl-reasoner/tests/fixtures/abox/p5_functional_diff.ofn \
        crates/owl-dl-reasoner/tests/fixtures/abox/p5_functional_same_target.ofn
git commit -m "$(cat <<'EOF'
feat(abox-check): T11 — P5 functional + two distinct witnesses

Functional and InverseFunctional grouped over property assertions;
each multi-target group merges in the P4 union-find and re-tests
DifferentIndividuals pairs. Sound by functionality semantics.

Spec: docs/superpowers/specs/2026-06-04-abox-consistency-check-design.md
EOF
)"
```

---

## Phase 8: Pattern P6 — Asymmetric / Irreflexive violations

### Task 12: P6 fixtures + impl

**Files:**
- Modify: `crates/owl-dl-reasoner/src/abox_check.rs`
- Create: 4 fixture files under `crates/owl-dl-reasoner/tests/fixtures/abox/`
- Modify: `crates/owl-dl-reasoner/tests/abox_consistency.rs`

- [ ] **Step 1: Write the P6 fixtures (asymmetric + irreflexive, both polarities)**

`crates/owl-dl-reasoner/tests/fixtures/abox/p6_asymmetric.ofn`:

```
Prefix(:=<http://t/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(rdf:=<http://www.w3.org/1999/02/22-rdf-syntax-ns#>)
Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)
Prefix(xml:=<http://www.w3.org/XML/1998/namespace>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://t/p6a>
  Declaration(ObjectProperty(:strictlyOlder))
  Declaration(NamedIndividual(:a))
  Declaration(NamedIndividual(:b))
  AsymmetricObjectProperty(:strictlyOlder)
  ObjectPropertyAssertion(:strictlyOlder :a :b)
  ObjectPropertyAssertion(:strictlyOlder :b :a)
)
```

`crates/owl-dl-reasoner/tests/fixtures/abox/p6_asymmetric_one_way.ofn`:

```
Prefix(:=<http://t/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(rdf:=<http://www.w3.org/1999/02/22-rdf-syntax-ns#>)
Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)
Prefix(xml:=<http://www.w3.org/XML/1998/namespace>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://t/p6an>
  Declaration(ObjectProperty(:strictlyOlder))
  Declaration(NamedIndividual(:a))
  Declaration(NamedIndividual(:b))
  AsymmetricObjectProperty(:strictlyOlder)
  ObjectPropertyAssertion(:strictlyOlder :a :b)
)
```

`crates/owl-dl-reasoner/tests/fixtures/abox/p6_irreflexive.ofn`:

```
Prefix(:=<http://t/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(rdf:=<http://www.w3.org/1999/02/22-rdf-syntax-ns#>)
Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)
Prefix(xml:=<http://www.w3.org/XML/1998/namespace>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://t/p6i>
  Declaration(ObjectProperty(:parentOf))
  Declaration(NamedIndividual(:a))
  IrreflexiveObjectProperty(:parentOf)
  ObjectPropertyAssertion(:parentOf :a :a)
)
```

`crates/owl-dl-reasoner/tests/fixtures/abox/p6_irreflexive_distinct.ofn`:

```
Prefix(:=<http://t/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(rdf:=<http://www.w3.org/1999/02/22-rdf-syntax-ns#>)
Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)
Prefix(xml:=<http://www.w3.org/XML/1998/namespace>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://t/p6in>
  Declaration(ObjectProperty(:parentOf))
  Declaration(NamedIndividual(:a))
  Declaration(NamedIndividual(:b))
  IrreflexiveObjectProperty(:parentOf)
  ObjectPropertyAssertion(:parentOf :a :b)
)
```

- [ ] **Step 2: Add the P6 tests**

Append to `crates/owl-dl-reasoner/tests/abox_consistency.rs`:

```rust
// ── P6: Asymmetric / Irreflexive violations ─────────────────────────

#[test]
fn p6_asymmetric_two_way_is_inconsistent() {
    assert!(!check_consistency("p6_asymmetric"),
        "P6: Asymmetric(R) + R(a,b) + R(b,a) should be inconsistent");
}

#[test]
fn p6_asymmetric_one_way_is_consistent() {
    assert!(check_consistency("p6_asymmetric_one_way"),
        "P6 negative: Asymmetric(R) + R(a,b) alone should stay consistent");
}

#[test]
fn p6_irreflexive_self_loop_is_inconsistent() {
    assert!(!check_consistency("p6_irreflexive"),
        "P6: Irreflexive(R) + R(a,a) should be inconsistent");
}

#[test]
fn p6_irreflexive_distinct_pair_is_consistent() {
    assert!(check_consistency("p6_irreflexive_distinct"),
        "P6 negative: Irreflexive(R) + R(a,b) with distinct a,b should stay consistent");
}
```

- [ ] **Step 3: Run new tests — at least the negatives should pass**

```sh
cargo test -p owl-dl-reasoner --release --test abox_consistency p6_ 2>&1 | tail -10
```

- [ ] **Step 4: Implement P6 in `abox_check.rs`**

Insert P6 in `check()` after P5 and before the final `AboxVerdict::Unknown`:

```rust
    // P6: Asymmetric + Irreflexive.
    let mut asymmetric_roles: std::collections::HashSet<owl_dl_core::ir::RoleId> =
        std::collections::HashSet::new();
    let mut irreflexive_roles: std::collections::HashSet<owl_dl_core::ir::RoleId> =
        std::collections::HashSet::new();
    for ax in &prepared.axioms {
        match ax {
            owl_dl_core::ontology::Axiom::AsymmetricRole(r) => {
                asymmetric_roles.insert(r.role_id());
            }
            owl_dl_core::ontology::Axiom::IrreflexiveRole(r) => {
                irreflexive_roles.insert(r.role_id());
            }
            _ => {}
        }
    }
    // Asymmetric: scan for (a, R, b) and (b, R, a) both present. Use
    // the `pos` set built in P3.
    for &(from, role, to) in &prepared.abox.property_assertions {
        if asymmetric_roles.contains(&role) && pos.contains(&(to, role, from)) {
            return AboxVerdict::Inconsistent {
                reason: ClashReason::AsymmetricViolation { role, a: from, b: to },
            };
        }
    }
    // Irreflexive: any (a, R, a). Also fires when SameAs merges
    // collapsed from == to: scan property_assertions and test via uf.
    for &(from, role, to) in &prepared.abox.property_assertions {
        if !irreflexive_roles.contains(&role) { continue; }
        if from == to {
            return AboxVerdict::Inconsistent {
                reason: ClashReason::IrreflexiveViolation { role, a: from },
            };
        }
        if let (Some(&i), Some(&j)) = (ind_index.get(&from), ind_index.get(&to)) {
            if uf.same(u32::try_from(i).expect("fits"), u32::try_from(j).expect("fits")) {
                return AboxVerdict::Inconsistent {
                    reason: ClashReason::IrreflexiveViolation { role, a: from },
                };
            }
        }
    }
```

- [ ] **Step 5: Run all tests**

```sh
cargo test -p owl-dl-reasoner --release --test abox_consistency 2>&1 | tail -20
```

Expected: P1–P6 all pass.

- [ ] **Step 6: Confirm no regression**

```sh
cargo test -p owl-dl-reasoner --release 2>&1 | tail -5
```

Expected: all passing.

- [ ] **Step 7: Clippy clean**

```sh
cargo clippy -p owl-dl-reasoner --all-targets -- -D warnings 2>&1 | tail -5
```

Expected: no warnings.

- [ ] **Step 8: Commit**

```sh
git add crates/owl-dl-reasoner/src/abox_check.rs \
        crates/owl-dl-reasoner/tests/abox_consistency.rs \
        crates/owl-dl-reasoner/tests/fixtures/abox/p6_asymmetric.ofn \
        crates/owl-dl-reasoner/tests/fixtures/abox/p6_asymmetric_one_way.ofn \
        crates/owl-dl-reasoner/tests/fixtures/abox/p6_irreflexive.ofn \
        crates/owl-dl-reasoner/tests/fixtures/abox/p6_irreflexive_distinct.ofn
git commit -m "$(cat <<'EOF'
feat(abox-check): T12 — P6 Asymmetric / Irreflexive violations

Direct scan: Asymmetric(R) ∧ R(a,b) ∧ R(b,a); Irreflexive(R) ∧
R(a,a). Irreflexive also fires when SameAs merges (via the P4
union-find) collapse from == to. Sound by definition.

Spec: docs/superpowers/specs/2026-06-04-abox-consistency-check-design.md
EOF
)"
```

---

## Phase 9: Pattern P7 — Domain/range disjointness propagation (stretch)

### Task 13: P7 fixtures + impl

**Files:**
- Modify: `crates/owl-dl-reasoner/src/abox_check.rs`
- Create: `crates/owl-dl-reasoner/tests/fixtures/abox/p7_range_disjoint.ofn`
- Create: `crates/owl-dl-reasoner/tests/fixtures/abox/p7_range_compatible.ofn`
- Modify: `crates/owl-dl-reasoner/tests/abox_consistency.rs`

- [ ] **Step 1: Write the P7-positive fixture**

`crates/owl-dl-reasoner/tests/fixtures/abox/p7_range_disjoint.ofn`:

```
Prefix(:=<http://t/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(rdf:=<http://www.w3.org/1999/02/22-rdf-syntax-ns#>)
Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)
Prefix(xml:=<http://www.w3.org/XML/1998/namespace>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://t/p7>
  Declaration(Class(:Male))
  Declaration(Class(:Female))
  Declaration(ObjectProperty(:hasMother))
  Declaration(NamedIndividual(:child))
  Declaration(NamedIndividual(:m))
  DisjointClasses(:Male :Female)
  ObjectPropertyRange(:hasMother :Female)
  ObjectPropertyAssertion(:hasMother :child :m)
  ClassAssertion(:Male :m)
)
```

- [ ] **Step 2: Write the P7-negative fixture**

`crates/owl-dl-reasoner/tests/fixtures/abox/p7_range_compatible.ofn`:

```
Prefix(:=<http://t/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(rdf:=<http://www.w3.org/1999/02/22-rdf-syntax-ns#>)
Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)
Prefix(xml:=<http://www.w3.org/XML/1998/namespace>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://t/p7n>
  Declaration(Class(:Male))
  Declaration(Class(:Female))
  Declaration(ObjectProperty(:hasMother))
  Declaration(NamedIndividual(:child))
  Declaration(NamedIndividual(:m))
  DisjointClasses(:Male :Female)
  ObjectPropertyRange(:hasMother :Female)
  ObjectPropertyAssertion(:hasMother :child :m)
  ClassAssertion(:Female :m)
)
```

- [ ] **Step 3: Add P7 tests**

Append to `crates/owl-dl-reasoner/tests/abox_consistency.rs`:

```rust
// ── P7: Domain/range disjointness propagation (stretch) ─────────────

#[test]
fn p7_range_clashes_with_assertion_is_inconsistent() {
    assert!(!check_consistency("p7_range_disjoint"),
        "P7: range(R)=Female + R(c,m) + ClassAssertion(Male,m) + Male/Female disjoint inconsistent");
}

#[test]
fn p7_range_compatible_is_consistent() {
    assert!(check_consistency("p7_range_compatible"),
        "P7 negative: range and explicit class agree → consistent");
}
```

- [ ] **Step 4: Implement P7 in `abox_check.rs`**

Insert P7 after P6 and before the final `AboxVerdict::Unknown`. The implementation augments the per-individual `types` set with domain/range targets, then re-runs the P2 pairwise-disjoint scan over the augmented set:

```rust
    // P7 stretch: domain/range propagation. For each
    // ObjectPropertyDomain(R, D) and assertion R(a, _), add D's
    // class (if atomic) + its EL subsumers to types[a]. Same for
    // range applied to the object. Then re-run the P2 scan.
    let mut domains: Vec<(owl_dl_core::ir::RoleId,
                          owl_dl_core::ir::ConceptId)> = Vec::new();
    let mut ranges: Vec<(owl_dl_core::ir::RoleId,
                         owl_dl_core::ir::ConceptId)> = Vec::new();
    for ax in &prepared.axioms {
        match ax {
            owl_dl_core::ontology::Axiom::ObjectPropertyDomain { role, domain } => {
                domains.push((role.role_id(), *domain));
            }
            owl_dl_core::ontology::Axiom::ObjectPropertyRange { role, range } => {
                ranges.push((role.role_id(), *range));
            }
            _ => {}
        }
    }

    let mut augmented = false;
    for &(from, role, to) in &prepared.abox.property_assertions {
        for &(d_role, d_concept) in &domains {
            if d_role != role { continue; }
            if let owl_dl_core::ir::ConceptExpr::Atomic(c) = pool.get(d_concept) {
                if let Some(&i) = ind_index.get(&from) {
                    let inserted_new = types[i].insert(*c);
                    augmented |= inserted_new;
                    for s in closure.subsumers_of(*c) {
                        augmented |= types[i].insert(s);
                    }
                }
            }
        }
        for &(r_role, r_concept) in &ranges {
            if r_role != role { continue; }
            if let owl_dl_core::ir::ConceptExpr::Atomic(c) = pool.get(r_concept) {
                if let Some(&i) = ind_index.get(&to) {
                    augmented |= types[i].insert(*c);
                    for s in closure.subsumers_of(*c) {
                        augmented |= types[i].insert(s);
                    }
                }
            }
        }
    }

    if augmented {
        for (i, type_set) in types.iter().enumerate() {
            let (individual, _) = prepared.abox.individuals[i];
            let cs: Vec<_> = type_set.iter().copied().collect();
            for a in 0..cs.len() {
                for b in (a + 1)..cs.len() {
                    if told.are_told_disjoint(cs[a], cs[b]) {
                        return AboxVerdict::Inconsistent {
                            reason: ClashReason::DisjointTypes {
                                individual, c: cs[a], d: cs[b],
                            },
                        };
                    }
                }
            }
        }
    }
```

> If `told` was scoped inside the P2 block, hoist it to function scope so P7 can reuse it. Same for `types` and `ind_index`.

- [ ] **Step 5: Run all tests**

```sh
cargo test -p owl-dl-reasoner --release --test abox_consistency 2>&1 | tail -20
```

Expected: P1–P7 all pass (14 total + skeleton = 15).

- [ ] **Step 6: Confirm no regression**

```sh
cargo test -p owl-dl-reasoner --release 2>&1 | tail -5
```

Expected: all passing.

- [ ] **Step 7: Clippy clean**

```sh
cargo clippy -p owl-dl-reasoner --all-targets -- -D warnings 2>&1 | tail -5
```

Expected: no warnings.

- [ ] **Step 8: Commit**

```sh
git add crates/owl-dl-reasoner/src/abox_check.rs \
        crates/owl-dl-reasoner/tests/abox_consistency.rs \
        crates/owl-dl-reasoner/tests/fixtures/abox/p7_range_disjoint.ofn \
        crates/owl-dl-reasoner/tests/fixtures/abox/p7_range_compatible.ofn
git commit -m "$(cat <<'EOF'
feat(abox-check): T13 — P7 domain/range disjointness (stretch)

ObjectPropertyDomain / Range augment the per-individual type set
(with EL subsumer closure) and re-run the P2 pairwise-disjoint scan.
Sound: derived types are direct semantic consequences of the
assertion + the domain/range axiom.

Spec: docs/superpowers/specs/2026-06-04-abox-consistency-check-design.md
EOF
)"
```

---

## Phase 10: Corpus regression + perf measurement

### Task 14: Add corpus closure-diff tests for family / family-stripped

**Files:**
- Modify: `crates/owl-dl-reasoner/tests/konclude_closure_diff.rs`

- [ ] **Step 1: Add the corpus tests**

Find the existing `sulo_closure_matches_konclude` (around line 392). Add right after its closing `}`:

```rust
#[test]
#[ignore = "Phase A1 corpus regression — family is HermiT/Konclude-inconsistent; checks rustdl's abox_check detects it (stretch: may not close without functional-merge work). Needs family.ofn + family-stripped.ofn."]
fn family_inconsistency_detected() {
    let path = Path::new("../../ontologies/real/family.ofn");
    if !path.exists() {
        eprintln!("SKIP: missing family.ofn");
        return;
    }
    let src = std::fs::read_to_string(path).expect("read family.ofn");
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse");
    let consistent = owl_dl_reasoner::is_consistent(&onto).expect("is_consistent");
    eprintln!("family is_consistent = {consistent} (oracle: HermiT/Konclude inconsistent)");
    assert!(!consistent, "family should be detected as inconsistent (stretch goal)");
}

#[test]
#[ignore = "Phase A1 corpus regression — family-stripped is HermiT/Konclude-inconsistent (no data axioms); checks rustdl's abox_check detects it (stretch). Needs family-stripped.ofn."]
fn family_stripped_inconsistency_detected() {
    let path = Path::new("../../ontologies/real/family-stripped.ofn");
    if !path.exists() {
        eprintln!("SKIP: missing family-stripped.ofn");
        return;
    }
    let src = std::fs::read_to_string(path).expect("read family-stripped.ofn");
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse");
    let consistent = owl_dl_reasoner::is_consistent(&onto).expect("is_consistent");
    eprintln!("family-stripped is_consistent = {consistent} (oracle: HermiT/Konclude inconsistent)");
    assert!(!consistent, "family-stripped should be detected as inconsistent (stretch goal)");
}
```

> Both are `#[ignore]`d — they're stretch-goal regression guards. If P7 closes one or both, great; the test becomes an active guard. If not, the test's docstring documents the remaining gap.

- [ ] **Step 2: Run the new tests with a tight timeout**

```sh
timeout 60 cargo test -p owl-dl-reasoner --release --test konclude_closure_diff -- --ignored --nocapture family 2>&1 | tail -20
```

Expected: each test either prints `is_consistent = false` (pass — we closed the gap) or times out / prints `is_consistent = true` (fail — stretch goal not met). Either way, capture the verdict.

- [ ] **Step 3: Run all existing corpus closure-diffs for FP=0 invariant**

```sh
RUSTDL_HORN_SHORTCIRCUIT=0 cargo test -p owl-dl-reasoner --release --test konclude_closure_diff -- --ignored --nocapture 2>&1 | tail -40
```

Expected: shoiq_knowledge, sio, alehif, ore-10908, ore-15672, ro, sulo all still pass with FP=0/MISSED=0 (unchanged from the pre-task baseline). This is the soundness tripwire.

- [ ] **Step 4: Measure perf — GALEN classify wall**

```sh
time ./target/release/rustdl classify ontologies/real/galen.ofn > /tmp/galen-with-abox.out 2>&1
tail -10 /tmp/galen-with-abox.out
```

Expected: wall ≈ 455.73 s ± 2% (Phase 7 baseline). The new check adds one early-return on ABox-free ontologies.

If wall regression exceeds 5%, investigate: most likely the `prepared.axioms` clone is too aggressive (switch to a reference) or the per-individual type-set is being built for too many ABox-free individuals.

- [ ] **Step 5: Measure perf — ORE-10908 + ORE-15672**

```sh
time ./target/release/rustdl classify ontologies/real/ore-10908.ofn > /tmp/ore10908-with-abox.out 2>&1
tail -5 /tmp/ore10908-with-abox.out
time ./target/release/rustdl classify ontologies/real/ore-15672.ofn > /tmp/ore15672-with-abox.out 2>&1
tail -5 /tmp/ore15672-with-abox.out
```

Expected: ±5% from prior baselines.

- [ ] **Step 6: Measure family verdict + wall**

```sh
time ./target/release/rustdl consistent ontologies/real/family-stripped.ofn 2>&1 | tail -5
```

Expected: ideally ≤ 1 s with `inconsistent`. If still slow / `consistent`, P7 didn't close the family gap (the stretch case from the spec). Document the result in the task notes.

- [ ] **Step 7: Commit**

```sh
git add crates/owl-dl-reasoner/tests/konclude_closure_diff.rs
git commit -m "$(cat <<'EOF'
test(abox-check): T14 — family / family-stripped inconsistency regression

Two #[ignore]d corpus regression tests gating rustdl's verdict against
HermiT/Konclude (both call family inconsistent in <1s). Stretch goal
tests: if P7 closes either, the test becomes an active regression
guard; if not, the test's docstring documents the remaining gap. The
existing seven corpus closure-diff tests are the primary FP=0
tripwire — re-confirmed unchanged.

Spec: docs/superpowers/specs/2026-06-04-abox-consistency-check-design.md
EOF
)"
```

---

## Phase 11: Documentation + project handoff

### Task 15: Update CLAUDE.md + write project handoff

**Files:**
- Modify: `CLAUDE.md`
- Create: `docs/abox-consistency-check-handoff.md`

- [ ] **Step 1: Update `CLAUDE.md`**

In `CLAUDE.md`, find the `owl-dl-reasoner` bullet (the one describing classify, realize, etc.). Append a new sub-bullet after the existing Phase 7 paragraph:

```markdown
  Phase A1 (commit `<TBD>`) added a sound ABox-driven inconsistency
  pre-check at `crates/owl-dl-reasoner/src/abox_check.rs`. Runs
  before the tableau in both `is_consistent` and `classify`; on a
  positive verdict, classify mirrors Konclude's behaviour (every
  class marked unsatisfiable). Seven clash patterns (P1 direct-Bot,
  P2 disjoint types, P3 NegOPA-vs-OPA, P4 SameAs∩DifferentFrom, P5
  Functional+two-distinct-witnesses, P6 Asymmetric/Irreflexive, P7
  domain/range stretch). Env gate `RUSTDL_ABOX_CHECK=0` reverts to
  pre-Phase-A1 tableau-only behaviour. See
  `docs/superpowers/specs/2026-06-04-abox-consistency-check-design.md`
  + `docs/abox-consistency-check-handoff.md`.
```

Replace `<TBD>` with the actual commit SHA from `git rev-parse --short HEAD` once Task 14 is committed (do this in step 3 of this task).

- [ ] **Step 2: Write the handoff doc**

`docs/abox-consistency-check-handoff.md`:

```markdown
# ABox Consistency Check — Handoff

## Status: <SHIPPED | PARTIAL>

Replace with `SHIPPED` if both `family` corpus tests pass, `PARTIAL` if only the synthetic patterns close.

## Scope shipped

- T1–T6: scaffolding (union_find module, abox_check module skeleton,
  env gate, wiring into is_consistent + classify, CLI banner).
- T7–T13: clash patterns P1 (direct-Bot assertion), P2 (disjoint
  types per individual), P3 (NegOPA vs OPA), P4 (SameAs ∩
  DifferentFrom), P5 (functional + two distinct witnesses), P6
  (Asymmetric / Irreflexive), P7 (domain/range disjointness).
- T14: corpus regression tests for family / family-stripped.

## Test harness

- `crates/owl-dl-reasoner/tests/abox_consistency.rs` — 14 synthetic
  tests (7 positive + 7 negative), all non-ignored.
- `crates/owl-dl-reasoner/tests/konclude_closure_diff.rs` —
  `family_inconsistency_detected` and
  `family_stripped_inconsistency_detected` (`#[ignore]`d).

## Performance impact

(Fill in measured numbers from Task 14 steps 4–6.)

- GALEN classify wall: <BEFORE> → <AFTER> s (<+/-X%>)
- ORE-10908 classify wall: <BEFORE> → <AFTER> s (<+/-X%>)
- ORE-15672 classify wall: <BEFORE> → <AFTER> s (<+/-X%>)
- family-stripped `is_consistent`: <BEFORE: 180 s timeout> → <AFTER> s

## Soundness invariant

FP=0 vs Konclude preserved across all seven corpus closure-diff
tests (shoiq_knowledge, sio, alehif, ore-10908, ore-15672, ro,
sulo). The cheap ABox check never flagged a consistent ontology as
inconsistent.

## Known gaps (not addressed)

- Functional-role merge step needed for family-style multi-step
  clashes (`∃hasSex.Male ⊓ ∃hasSex.Female + Functional(hasSex) →
  Male⊓Female → ⊥`). P7 covers the range step but not the
  functional-collapse step. If family-stripped still times out
  after this project, that's the next scoping target.
- ABox-level realization (per-individual most-specific type) — out
  of scope.
- Concrete-domain reasoning on `DataPropertyAssertion` literal
  values — out of scope (D5 covers TBox side).
```

- [ ] **Step 3: Replace `<TBD>` in CLAUDE.md with the latest SHA**

```sh
git log --oneline -1 | awk '{print $1}'
# Edit CLAUDE.md to replace <TBD> with this short SHA.
```

- [ ] **Step 4: Commit**

```sh
git add CLAUDE.md docs/abox-consistency-check-handoff.md
git commit -m "$(cat <<'EOF'
docs: ABox consistency check — handoff + CLAUDE.md update

Project handoff summarising T1–T14 (scaffolding + 7 clash patterns +
corpus regression). CLAUDE.md gains a Phase A1 bullet on the
owl-dl-reasoner block pointing at the spec + handoff. Performance
numbers + the SHIPPED/PARTIAL designation reflect the measurements
from T14.

Spec: docs/superpowers/specs/2026-06-04-abox-consistency-check-design.md
EOF
)"
```

---

## End-state checklist

- [ ] All 15 tasks committed, working tree clean.
- [ ] `cargo build --workspace --release` succeeds.
- [ ] `cargo test --workspace` succeeds.
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings` succeeds.
- [ ] `cargo test -p owl-dl-reasoner --release --test abox_consistency` shows 14 passing tests.
- [ ] `cargo test -p owl-dl-reasoner --release --test konclude_closure_diff -- --ignored` shows all FP=0/MISSED=0 (existing 7 corpus fixtures unchanged).
- [ ] `RUSTDL_ABOX_CHECK=0 ./target/release/rustdl classify ontologies/real/family-stripped.ofn` prints `# abox_check: skipped` in the banner.
- [ ] Default-on classify of any ABox-bearing ontology prints `# abox_check: inconsistent | unknown`.
- [ ] Handoff doc populated with measured perf numbers.
