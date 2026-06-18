# Wedge inverse/symmetric domain-range firing (SP1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the hypertableau wedge fire `domain`/`range` (single-role-body clauses) through inverse and symmetric roles, closing the calculus half of the `family` inconsistency and a general SROIQ completeness gap.

**Architecture:** Two sound, additive changes. **Part 1 (shared):** when an edge `src—p→tgt` is added, also fire *inverse first-leg* clauses (`Atom::Role(Inverse(p),X,y)→…`) at `tgt` — a new `inverse_first_trigger` index mirroring the existing `role_back_trigger`. **Part 2 (two variants, compared in worktrees):** symmetric/self-inverse handling — **Variant R** teaches `role_matches` symmetric-awareness; **Variant M** materializes the reverse edge. Winner chosen on FP=0 → perf → simplicity.

**Tech Stack:** Rust (edition 2024), workspace crates `owl-dl-core`, `owl-dl-tableau`, `owl-dl-reasoner`; Konclude (docker `konclude/konclude:latest`) + ROBOT (`obolibrary/robot:v1.9.6`) as oracle; corpus closure-diff net (`konclude_closure_diff` test + `scripts/closure-diff.sh`).

**Spec:** `docs/superpowers/specs/2026-06-18-wedge-declared-inverse-symmetric-design.md`

**Soundness law (non-negotiable):** FP=0. Any corpus false-positive subsumption/inconsistency → revert the offending change. Every change here is additive (fires a clause that *should* fire / matches an entailed edge), so it is sound by construction; the risk is implementation bugs (over-trigger, over-match, re-materialization loop), caught by negative-control tests + the corpus net.

---

## Conventions

- Build: `cargo build --release -p owl-dl-cli` (binary `./target/release/rustdl`).
- Toolchain on PATH: `export PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"`.
- Oracle check for a `/tmp/X.ofn` (must already be reachable in `/tmp/ddmin_work`):
  ```sh
  docker run --rm -v /tmp/ddmin_work:/w -w /w obolibrary/robot:v1.9.6 robot convert --input X.ofn --format owx --output X.owx >/dev/null 2>&1
  docker run --rm -v /tmp/ddmin_work:/w -w /w konclude/konclude:latest consistency -i /w/X.owx 2>&1 | grep -oiE 'is (in)?consistent'
  ```
- Wedge-only consistency (disables A1 ABox pre-check, forces the engine): `RUSTDL_ABOX_CHECK=0 ./target/release/rustdl consistent FILE.ofn`.
- The family core fixture already exists: `docs/family-mech4-ddmin-core.ofn` (Konclude: inconsistent).

---

## Task 0: Branch

**Files:** none (git only).

- [ ] **Step 1: Create the feature branch off main**

```sh
cd /data/dumontier/rustdl
git checkout main && git checkout -b feat/wedge-inverse-symmetric-sp1
```

- [ ] **Step 2: Confirm clean baseline build + suite**

Run: `cargo build --release -p owl-dl-cli && cargo test -p owl-dl-reasoner --test functional_enforcement`
Expected: builds; `forward_functional_merge_disjoint_inconsistent` passes.

---

## Task 1: Symmetric-role detection in `RoleHierarchy` (shared infra)

Collect symmetric roles (from `SymmetricObjectProperty` **and** self-inverse `InverseObjectProperties(p,p)`) into `RoleHierarchy` so both Part-2 variants can consult it. This task adds the data + accessor and the builder ingest; it changes no behavior yet.

**Files:**
- Modify: `crates/owl-dl-core/src/role_hierarchy.rs`
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (`build_role_hierarchy`)
- Test: `crates/owl-dl-core/src/role_hierarchy.rs` (unit tests module)

- [ ] **Step 1: Add a failing unit test for the symmetric set**

In `role_hierarchy.rs` `#[cfg(test)] mod tests`, add:

```rust
#[test]
fn symmetric_set_roundtrips() {
    let mut b = RoleHierarchyBuilder::with_roles(3);
    b.mark_symmetric(r(1));
    let h = b.build();
    assert!(!h.is_symmetric(r(0)));
    assert!(h.is_symmetric(r(1)));
    assert!(!h.is_symmetric(r(2)));
}
```

- [ ] **Step 2: Run it; expect failure**

Run: `cargo test -p owl-dl-core role_hierarchy::tests::symmetric_set_roundtrips`
Expected: FAIL — `mark_symmetric` / `is_symmetric` not found.

- [ ] **Step 3: Implement the symmetric set**

In `RoleHierarchyBuilder` add a field and method:

```rust
#[derive(Debug, Default, Clone)]
pub struct RoleHierarchyBuilder {
    direct_super: Vec<SmallVec<[RoleId; 4]>>,
    symmetric: Vec<bool>,
}
```

In `with_roles`:

```rust
    pub fn with_roles(n: u32) -> Self {
        Self {
            direct_super: (0..n as usize).map(|_| SmallVec::new()).collect(),
            symmetric: vec![false; n as usize],
        }
    }
```

Add the marker method (panics out-of-range, matching `add_sub_role`):

```rust
    /// Record that `role` is symmetric (`role ≡ role⁻`).
    ///
    /// # Panics
    /// Panics if `role` is out of range for this builder.
    pub fn mark_symmetric(&mut self, role: RoleId) {
        self.symmetric[role.index() as usize] = true;
    }
```

In `build`, move `symmetric` into the frozen struct:

```rust
        RoleHierarchy {
            super_closure,
            sub_closure,
            symmetric: self.symmetric.into_boxed_slice(),
        }
```

In `RoleHierarchy` add the field + accessor:

```rust
pub struct RoleHierarchy {
    super_closure: Vec<Box<[RoleId]>>,
    sub_closure: Vec<Box<[RoleId]>>,
    symmetric: Box<[bool]>,
}
```

```rust
    /// Returns `true` iff `role` was declared symmetric (`role ≡ role⁻`),
    /// directly via `SymmetricObjectProperty` or via self-inverse
    /// `InverseObjectProperties(role, role)`.
    ///
    /// # Panics
    /// Panics if `role` is out of range.
    #[must_use]
    pub fn is_symmetric(&self, role: RoleId) -> bool {
        self.symmetric[role.index() as usize]
    }
```

- [ ] **Step 4: Run the unit test; expect pass**

Run: `cargo test -p owl-dl-core role_hierarchy::tests::symmetric_set_roundtrips`
Expected: PASS. Also run `cargo test -p owl-dl-core role_hierarchy` — existing hierarchy tests still pass.

- [ ] **Step 5: Feed symmetric roles in `build_role_hierarchy`**

In `crates/owl-dl-reasoner/src/lib.rs`, inside `build_role_hierarchy`, after the builder is created and the sub-role axioms are added (locate the `for ax in &internal.axioms` loop that calls `builder.add_sub_role`), add handling for the two symmetric sources. Use the canonical role id (`role_id()`):

```rust
        match ax {
            // ... existing SubObjectPropertyOf / InverseObjectProperties arms ...
            Axiom::SymmetricRole(role) => {
                builder.mark_symmetric(role.role_id());
            }
            Axiom::InverseObjectProperties(a, b) if a.role_id() == b.role_id() => {
                // Self-inverse `R ≡ R⁻` is exactly symmetry.
                builder.mark_symmetric(a.role_id());
            }
            _ => {}
        }
```

(If the existing loop is a `for` with `if let`, add these as additional `if let`/`match` arms in the same loop rather than a second pass. Do not remove the existing `InverseObjectProperties` canon handling — self-inverse must *also* mark symmetric.)

- [ ] **Step 6: Build + commit**

Run: `cargo build --release -p owl-dl-cli && cargo test -p owl-dl-core role_hierarchy`
Expected: builds; all `role_hierarchy` tests pass.

```sh
git add crates/owl-dl-core/src/role_hierarchy.rs crates/owl-dl-reasoner/src/lib.rs
git commit -m "feat(wedge): RoleHierarchy symmetric-role set + builder ingest (SP1 infra)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: Part 1 — inverse first-leg triggering

Fire inverse first-leg clauses at the edge **target**. Closes the inverse cases (tinv, trinv, declared H4) — symmetric still pending Part 2.

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` (`ClauseIndexes`, index builder, `Event::Edge`)
- Test: `crates/owl-dl-reasoner/tests/inverse_symmetric_domain.rs` (new)

- [ ] **Step 1: Write failing motif tests (inverse half)**

Create `crates/owl-dl-reasoner/tests/inverse_symmetric_domain.rs`. Reuse the engine-only helper pattern from `functional_enforcement.rs` (copy the `ENV_MUTEX` / `SetEnvGuard` / `consistent_engine_only` plumbing verbatim — it sets `RUSTDL_ABOX_CHECK=0`). Then:

```rust
// tinv: domain on a syntactic ObjectInverseOf(p) — INCONSISTENT
#[test]
fn inverse_domain_syntactic_inconsistent() {
    assert!(!consistent_engine_only(
        r"    Declaration(ObjectProperty(:p)) Declaration(Class(:C)) Declaration(Class(:D))
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
    ObjectPropertyDomain(ObjectInverseOf(:p) :C)
    DisjointClasses(:C :D)
    ObjectPropertyAssertion(:p :a :b)
    ClassAssertion(:D :b)"
    ));
}

// trinv: range on ObjectInverseOf(p) — INCONSISTENT
#[test]
fn inverse_range_syntactic_inconsistent() {
    assert!(!consistent_engine_only(
        r"    Declaration(ObjectProperty(:p)) Declaration(Class(:C)) Declaration(Class(:D))
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
    ObjectPropertyRange(ObjectInverseOf(:p) :C)
    DisjointClasses(:C :D)
    ObjectPropertyAssertion(:p :a :b)
    ClassAssertion(:D :a)"
    ));
}

// H4: declared InverseObjectProperties(p,q) + domain(q,C) — INCONSISTENT
#[test]
fn inverse_domain_declared_inconsistent() {
    assert!(!consistent_engine_only(
        r"    Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:q))
    Declaration(Class(:C)) Declaration(Class(:D))
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
    InverseObjectProperties(:p :q)
    ObjectPropertyDomain(:q :C)
    DisjointClasses(:C :D)
    ObjectPropertyAssertion(:p :a :b)
    ClassAssertion(:D :b)"
    ));
}

// NEGATIVE control: unrelated role, no inverse declaration — CONSISTENT
#[test]
fn unrelated_role_domain_stays_consistent() {
    assert!(consistent_engine_only(
        r"    Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:r))
    Declaration(Class(:C)) Declaration(Class(:D))
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
    ObjectPropertyDomain(:r :C)
    DisjointClasses(:C :D)
    ObjectPropertyAssertion(:p :a :b)
    ClassAssertion(:D :b)"
    ));
}
```

- [ ] **Step 2: Run; expect the three positives to FAIL, the negative to PASS**

Run: `cargo test -p owl-dl-reasoner --test inverse_symmetric_domain`
Expected: `inverse_domain_syntactic_inconsistent`, `inverse_range_syntactic_inconsistent`, `inverse_domain_declared_inconsistent` FAIL (return consistent); `unrelated_role_domain_stays_consistent` PASS.

- [ ] **Step 3: Add the `inverse_first_trigger` index field**

In `crates/owl-dl-tableau/src/hyper.rs`, in `struct ClauseIndexes`, after `role_back_trigger`:

```rust
    /// By role index: clauses with a FIRST-leg (`u == X`) body role atom
    /// on an INVERSE role `Atom::Role(Inverse(_), X, v)`. Such a clause is
    /// satisfied by an INCOMING edge at the home node (the node's
    /// `Inverse(p)`-successor = its `p`-predecessor). `Event::Edge` fires
    /// `role_trigger` only at the edge SOURCE, so the home node (the edge
    /// TARGET) never re-fires. This index closes that gap: fire at `tgt`.
    /// `match_body` re-verifies, so an over-fire is a perf no-op.
    inverse_first_trigger: Vec<Vec<usize>>,
```

Initialize it wherever `ClauseIndexes` is constructed (search for `role_back_trigger:` in the constructor / `Default`-like init and add `inverse_first_trigger: vec![Vec::new(); n]` with the same length as `role_trigger`; if the struct derives `Default` and is built via `..Default::default()`, no change is needed beyond the field).

- [ ] **Step 4: Populate the index in the builder**

In the index builder loop (the `Atom::Role(r, u, _)` arm at ~585-591):

```rust
                Atom::Role(r, u, _) => {
                    push(&mut ix.role_trigger, role_id_index(*r), ci);
                    if *u != X {
                        push(&mut ix.role_back_trigger, role_id_index(*r), ci);
                    }
                    // First-leg inverse role: fire at the edge TARGET.
                    if *u == X && r.is_inverse() {
                        push(&mut ix.inverse_first_trigger, role_id_index(*r), ci);
                    }
                }
```

- [ ] **Step 5: Fire at the target in `Event::Edge`**

In the `Event::Edge` arm, rename the unused binding `_tgt` to `tgt`, and after the `role_back_trigger` block add:

```rust
                // Inverse first-leg trigger: this edge `src—role→tgt`
                // gives `tgt` an `Inverse(role)`-successor (`src`). A clause
                // `Atom::Role(Inverse(role), X, y) → …` rooted at `tgt` can
                // now fire; `Event::Edge` otherwise only fires at `src`.
                let n_inv = self.indexes.inverse_first_trigger.get(key).map_or(0, Vec::len);
                for i in 0..n_inv {
                    let ci = self.indexes.inverse_first_trigger[key][i];
                    if matches!(self.fire_clause(ci, tgt), FireOutcome::Clash) {
                        return FireOutcome::Clash;
                    }
                }
```

- [ ] **Step 6: Run the motif tests; expect inverse positives now PASS**

Run: `cargo test -p owl-dl-reasoner --test inverse_symmetric_domain`
Expected: `inverse_domain_syntactic_inconsistent`, `inverse_range_syntactic_inconsistent`, `inverse_domain_declared_inconsistent` PASS; negative still PASS. (The symmetric tests are added in Task 4/5.)

- [ ] **Step 7: Clippy + fmt + commit**

Run: `cargo clippy -p owl-dl-tableau --all-targets -- -D warnings && cargo fmt --all`
Expected: clean.

```sh
git add crates/owl-dl-tableau/src/hyper.rs crates/owl-dl-reasoner/tests/inverse_symmetric_domain.rs
git commit -m "feat(wedge): fire inverse first-leg domain/range clauses at edge target (SP1 Part 1)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: Corpus FP gate after Part 1

Part 1 must not introduce any false positive. Run the closure-diff net.

**Files:** none (test run).

- [ ] **Step 1: Ensure corpus present**

Run: `ls ontologies/real/*.ofn >/dev/null 2>&1 || ./scripts/fetch-real-ontologies.sh`
Expected: fixtures present.

- [ ] **Step 2: Run the closure-diff net**

Run: `cargo test -p owl-dl-reasoner --test konclude_closure_diff -- --include-ignored`
Expected: FP=0 / MISSED=0 on every fixture that has an oracle (galen, notgalen, sio, wine, ore-10908, ore-15672, shoiq-knowledge, alehif, ro, sulo, pizza). If any FP appears → STOP, this is a soundness regression; investigate before proceeding (do not continue to Part 2).

- [ ] **Step 3: Record the result**

Append a one-line note to the plan's results section (Task 6) with the net outcome (e.g. "Part 1: FP=0/MISSED=0, all fixtures").

---

## Task 4: Variant R — `role_matches` symmetric-awareness (worktree)

**Worktree:** create an isolated worktree so Variant R and Variant M don't collide.

```sh
git worktree add ../rustdl-variantR feat/wedge-inv-sym-variantR feat/wedge-inverse-symmetric-sp1
```

Do all Variant-R work in `../rustdl-variantR`.

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` (`role_matches`, index builder)
- Test: `crates/owl-dl-reasoner/tests/inverse_symmetric_domain.rs` (symmetric cases — add in this worktree)

- [ ] **Step 1: Add failing symmetric motif tests**

Append to `inverse_symmetric_domain.rs`:

```rust
// H1a: explicit SymmetricObjectProperty + domain — INCONSISTENT
#[test]
fn symmetric_domain_inconsistent() {
    assert!(!consistent_engine_only(
        r"    Declaration(ObjectProperty(:p)) Declaration(Class(:C)) Declaration(Class(:D))
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
    SymmetricObjectProperty(:p)
    ObjectPropertyDomain(:p :C)
    DisjointClasses(:C :D)
    ObjectPropertyAssertion(:p :a :b)
    ClassAssertion(:D :b)"
    ));
}

// H1: self-inverse InverseObjectProperties(p,p) + domain — INCONSISTENT
#[test]
fn self_inverse_domain_inconsistent() {
    assert!(!consistent_engine_only(
        r"    Declaration(ObjectProperty(:p)) Declaration(Class(:C)) Declaration(Class(:D))
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
    InverseObjectProperties(:p :p)
    ObjectPropertyDomain(:p :C)
    DisjointClasses(:C :D)
    ObjectPropertyAssertion(:p :a :b)
    ClassAssertion(:D :b)"
    ));
}

// Family core (15-axiom ddmin) — INCONSISTENT (SP1 headline gate)
#[test]
fn family_core_inconsistent() {
    let body = std::fs::read_to_string("../docs/family-mech4-ddmin-core.ofn")
        .or_else(|_| std::fs::read_to_string("docs/family-mech4-ddmin-core.ofn"))
        .expect("family core fixture");
    // The fixture is a full ontology; parse + is_consistent with ABox check off.
    let _serial = ENV_MUTEX.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let _abox = SetEnvGuard::set("RUSTDL_ABOX_CHECK", "0");
    let mut reader = std::io::Cursor::new(body);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse family core");
    assert!(!is_consistent(&onto).expect("is_consistent"), "family core must be inconsistent");
}
```

- [ ] **Step 2: Run; expect symmetric + family-core to FAIL**

Run: `cargo test -p owl-dl-reasoner --test inverse_symmetric_domain`
Expected: `symmetric_domain_inconsistent`, `self_inverse_domain_inconsistent`, `family_core_inconsistent` FAIL; the inverse + negative tests still PASS.

- [ ] **Step 3: Make `role_matches` symmetric-aware**

In `hyper.rs`, change `role_matches`:

```rust
fn role_matches(edge: Role, wanted: Role, sub_roles: Option<&RoleHierarchy>) -> bool {
    // Symmetric role: `p ≡ p⁻`, so an edge labelled `p` (or `p⁻`) satisfies a
    // wanted `p` (or `p⁻`) regardless of polarity, when ids match.
    if let Some(h) = sub_roles {
        if edge.role_id() == wanted.role_id() && h.is_symmetric(wanted.role_id()) {
            return true;
        }
    }
    if edge.is_inverse() != wanted.is_inverse() {
        return false;
    }
    match sub_roles {
        Some(h) => h.is_sub_role(edge.role_id(), wanted.role_id()),
        None => edge.role_id() == wanted.role_id(),
    }
}
```

- [ ] **Step 4: Trigger symmetric first-legs at the target too**

In the index builder, broaden the inverse_first_trigger condition to include symmetric forward first-legs (so `domain(p,C)` with symmetric `p` fires at the target via the `preds`-flip match):

```rust
                    // First-leg fired at the edge TARGET when it can match an
                    // INCOMING edge: inverse roles always; symmetric roles too.
                    let r_id = RoleId::new(role_id_index(*r) as u32);
                    let symmetric = sym.map_or(false, |s| s.is_symmetric(r_id));
                    if *u == X && (r.is_inverse() || symmetric) {
                        push(&mut ix.inverse_first_trigger, role_id_index(*r), ci);
                    }
```

The index builder must receive the symmetric info. Thread the `Option<&RoleHierarchy>` (already available where indexes are built, since the engine holds `sub_roles`) into the builder as a parameter `sym: Option<&RoleHierarchy>`; if the builder is a free function, add the param at the call site (the `HyperEngine` construction path that already has `self.sub_roles`). Import `RoleId` if not already in scope.

- [ ] **Step 5: Run motif + family-core; expect all PASS**

Run: `cargo test -p owl-dl-reasoner --test inverse_symmetric_domain`
Expected: ALL tests PASS (inverse, symmetric, self-inverse, family-core, negative).

- [ ] **Step 6: Oracle cross-check the family core directly**

Run:
```sh
export PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
cargo build --release -p owl-dl-cli
RUSTDL_ABOX_CHECK=0 ./target/release/rustdl consistent docs/family-mech4-ddmin-core.ofn
```
Expected: `inconsistent`.

- [ ] **Step 7: Corpus FP net + clippy/fmt + commit**

Run: `cargo test -p owl-dl-reasoner --test konclude_closure_diff -- --include-ignored && cargo clippy -p owl-dl-tableau --all-targets -- -D warnings && cargo fmt --all`
Expected: FP=0 / MISSED=0; clippy clean. If any FP → STOP (soundness regression).

```sh
git add -A
git commit -m "feat(wedge): Variant R — role_matches symmetric-awareness; closes family core (SP1 Part 2R)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

- [ ] **Step 8: Record Variant-R perf**

Run: `RUSTDL_ABOX_CHECK=0 \time -v ./target/release/rustdl classify ontologies/real/sio.ofn 2>&1 | tail -3` (and galen). Note wall + any `role_matches` flamegraph delta in Task 6's results table.

---

## Task 5: Variant M — symmetric edge materialization (worktree)

**Worktree:**

```sh
git worktree add ../rustdl-variantM feat/wedge-inv-sym-variantM feat/wedge-inverse-symmetric-sp1
```

Do all Variant-M work in `../rustdl-variantM`. (This branches off the same Part-1 base as Variant R, NOT off Variant R.)

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` (the `Event::Edge` handler — materialize the reverse symmetric edge)
- Test: `crates/owl-dl-reasoner/tests/inverse_symmetric_domain.rs` (same symmetric + family-core tests as Task 4 Step 1 — add them here too)

- [ ] **Step 1: Add the same failing symmetric + family-core motif tests**

Add the `symmetric_domain_inconsistent`, `self_inverse_domain_inconsistent`, and `family_core_inconsistent` tests verbatim from Task 4 Step 1.

- [ ] **Step 2: Run; expect symmetric + family-core to FAIL**

Run: `cargo test -p owl-dl-reasoner --test inverse_symmetric_domain`
Expected: symmetric + family-core FAIL; inverse + negative PASS.

- [ ] **Step 3: Materialize the reverse edge for symmetric roles in `Event::Edge`**

In `hyper.rs` `Event::Edge(src, role, tgt)`, before the trigger firing, materialize the reverse edge when `role` is symmetric and not already present. Guard against self-loops and duplicates (the reverse of the reverse is the original, already present, so no loop):

```rust
                // Symmetric role: `p(src,tgt) ⟹ p(tgt,src)`. Materialize the
                // reverse edge once (idempotent: skip self-loops and dupes) so
                // forward domain/range/clause firing covers both endpoints.
                if src != tgt
                    && self
                        .sub_roles
                        .as_ref()
                        .is_some_and(|h| h.is_symmetric(RoleId::new(role_id_index(role) as u32)))
                    && !self.nodes[tgt.index()].edges.iter().any(|(er, t)| *er == role && *t == src)
                {
                    self.add_role_edge(tgt, role, src);
                }
```

Use the existing sanctioned edge-add that records on the trail and posts an `Event::Edge` (search for the helper used by `add_edge`/`derive_role_edge` — e.g. `add_role_edge` / the `from/to` push at ~2378 with trail logging). If no single helper exists, add a small private `fn add_role_edge(&mut self, from: HNode, role: Role, to: HNode)` that mirrors the trail-logged edge push at lines 2378-2379 and pushes `Event::Edge(from, role, to)`. The posted `Event::Edge(tgt, role, src)` will itself check symmetry, find the original `src→tgt` edge already present, and not re-materialize — terminating.

Import `RoleId` if needed.

- [ ] **Step 4: Run motif + family-core; expect all PASS**

Run: `cargo test -p owl-dl-reasoner --test inverse_symmetric_domain`
Expected: ALL PASS.

- [ ] **Step 5: Oracle cross-check the family core**

Run:
```sh
export PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
cargo build --release -p owl-dl-cli
RUSTDL_ABOX_CHECK=0 ./target/release/rustdl consistent docs/family-mech4-ddmin-core.ofn
```
Expected: `inconsistent`.

- [ ] **Step 6: Corpus FP net + clippy/fmt + commit**

Run: `cargo test -p owl-dl-reasoner --test konclude_closure_diff -- --include-ignored && cargo clippy -p owl-dl-tableau --all-targets -- -D warnings && cargo fmt --all`
Expected: FP=0 / MISSED=0; clippy clean. **Pay special attention to non-termination** (materialization loop): if any fixture hangs that previously finished → STOP, the dedup guard is wrong.

```sh
git add -A
git commit -m "feat(wedge): Variant M — symmetric reverse-edge materialization; closes family core (SP1 Part 2M)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

- [ ] **Step 7: Record Variant-M perf + graph growth**

Run the same classify timing as Task 4 Step 8 on sio + galen; with `--features counters` if helpful, note edge-count / graph-size delta vs baseline in Task 6's table.

---

## Task 6: Compare, choose, merge

**Files:** Append a "## Results" section to this plan; merge the winner into `feat/wedge-inverse-symmetric-sp1`.

- [ ] **Step 1: Fill the comparison table**

| Criterion | Variant R (role_matches) | Variant M (materialization) |
|---|---|---|
| Symmetric motif + self-inverse | (pass/fail) | (pass/fail) |
| Family core inconsistent | (pass/fail) | (pass/fail) |
| Corpus FP=0/MISSED=0 | (result) | (result) |
| GALEN classify wall | (s) | (s) |
| SIO classify wall | (s) | (s) |
| Graph growth | none | (edge delta) |
| Termination risk | none | (loop guard verified?) |
| Code complexity / hot-path | (note) | (note) |

- [ ] **Step 2: Choose the winner**

Decision order: **FP=0 first** (any FP disqualifies), then **perf** (no regression beyond ~5%), then **simplicity / SP2-alignment**. Tie → prefer **R** (no graph growth; aligned with SP2's scale goal). Record the rationale in Results.

- [ ] **Step 3: Merge the winner, drop the loser**

```sh
cd /data/dumontier/rustdl
git checkout feat/wedge-inverse-symmetric-sp1
git merge --no-ff feat/wedge-inv-sym-variant<R|M> -m "merge: SP1 Part 2 (<chosen> variant) — wedge symmetric domain/range firing"
git worktree remove ../rustdl-variantR && git branch -D feat/wedge-inv-sym-variantR  # if R lost; else remove M
git worktree remove ../rustdl-variantM && git branch -D feat/wedge-inv-sym-variantM
```

- [ ] **Step 4: Final full-suite gate on the merged branch**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo fmt --all -- --check`
Expected: green; clippy clean; fmt clean.

- [ ] **Step 5: Final corpus net on the merged branch**

Run: `cargo test -p owl-dl-reasoner --test konclude_closure_diff -- --include-ignored`
Expected: FP=0 / MISSED=0.

- [ ] **Step 6: Update docs + CLAUDE.md**

Add an SP1 entry to CLAUDE.md (owl-dl-tableau section) noting: wedge now fires domain/range through inverse + symmetric roles (inverse first-leg target-triggering + `<chosen>` symmetric handling); closes the family *calculus* gap (15-axiom core inconsistent); full family still a scale MISS (SP2). Reference the spec + this plan + `docs/family-mech4-ddmin-core.ofn`.

```sh
git add CLAUDE.md docs/superpowers/plans/2026-06-18-wedge-inverse-symmetric-sp1.md
git commit -m "docs(wedge): SP1 results + CLAUDE.md (inverse/symmetric domain-range firing)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Results (2026-06-18/19)

**Outcome: SP1 shipped via Variant R. Merge `377b301` on `feat/wedge-inverse-symmetric-sp1`.**

### Part 1 (inverse first-leg triggering) — commit `2ccb98a`
- Motif: tinv/trinv (syntactic inverse domain/range), H4 (declared inverse) → inconsistent; negative control consistent.
- Corpus FP net (Task 3): **FP=0/MISSED=0** all fixtures; family sentinel `#[ignore]d` (expected). 124 wedge tests pass.
- Code-quality (opus): soundness crux confirmed — an over-fire is a `match_body`-re-verified no-op, never a false clash.

### Bake-off comparison table

| Criterion | **Variant R (role_matches + canon fix)** | Variant M (materialization) |
|---|---|---|
| Symmetric motif + self-inverse | ✅ pass | ✅ pass |
| **Family core inconsistent (SP1 gate)** | ✅ **inconsistent** | ❌ **consistent — GATE FAIL** |
| Corpus FP=0/MISSED=0 | ✅ verified all fixtures (galen 27997, notgalen 32739, sio 8904, wine 653, ore-10908 6001, ore-15672 142, alehif 247, ro 158, pizza 499, bibtex 16) | moot (fails gate; lacks canon fix) |
| Graph growth | none | yes (materialized reverse edges) |
| Termination risk | none | guarded by `derive_role_edge` dedup |
| Complexity / hot-path | `role_matches` O(1) branch + narrow self-inverse canon skip | edge materialization in `Event::Edge` + trail/backjump; **and still needs the canon fix to pass the gate** |

### Decision
**Variant R wins** on the primary criterion (correctness — the only variant that closes the family-core gate), and also on FP=0, graph growth (none, aligns with SP2's scale goal), and simplicity. The decisive finding: closing the family core **requires** suppressing the degenerate self-inverse `canon` rewrite (it was shadowing the genuine `isAuntInLawOf = hasAuntInLaw⁻` pair via the first-wins guard); materialization alone does not address that. Variant M was discarded; its branch/worktree removed.

### Final validation (merged branch `377b301`)
- Family core (`docs/family-mech4-ddmin-core.ofn`) → **inconsistent** (Konclude oracle parity).
- `cargo test --workspace` → green (tableau 124, saturation 51, justification 32, wedge_consistency 14, inverse_symmetric_domain 8, …).
- `cargo fmt --all -- --check` clean; `cargo clippy --workspace --all-targets --all-features -- -D warnings` clean.
- Corpus closure-diff net → **FP=0/MISSED=0** on every fixture (480 s); only the `#[ignore]d` `family_inconsistency_detected` (full family at scale = SP2) "fails", as expected.

### Carried forward to SP2
- Full `family.ofn` / `family-stripped.ofn` remain sound MISSes — a **scale** stall (transitive-role closure explosion + disjunctive branching depth ~256), not a calculus gap.
- Minor (noted in Part-1 code review): multi-atom clauses with an inverse first leg only target-trigger on the first-leg edge; if a later leg's edge arrives after, the clause is not re-fired at the target. MISS-not-FP (`match_body` re-verifies), out of SP1 scope (domain/range are single-atom).
