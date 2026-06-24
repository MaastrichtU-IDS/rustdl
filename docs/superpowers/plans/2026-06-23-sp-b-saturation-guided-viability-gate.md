# SP-B Saturation-Guided Viability Gate Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a throwaway, env-flag-gated measurement that feeds the B1–B2c saturation forcing into the wedge's ⊔ choice (live-disjunct filtering against the derived-subsumer closure × told-disjointness) and reports whether wine's branch count collapses to Konclude's regime — yielding a decisive GO/NO-GO on the multi-month build-once core.

**Architecture:** A `SatGuide` value (per-class derived subsumers + per-class told-disjoints) is computed from the saturation closure and told tables inside `HyperCache::build` when `RUSTDL_SAT_GUIDE=1`, and threaded into the tableau solver. At the existing ⊔ branch point in `Solver::solve`, head disjuncts that are named classes proven incompatible with the node's label (some label-class has a derived subsumer told-disjoint with the disjunct) are pruned before the branch loop. A throwaway harness runs the four matched wine probes under flag OFF then ON via the existing `decide_pair_probe` / `sat_class_probe`, dumps a branch-count table, and asserts verdict preservation.

**Tech Stack:** Rust (edition 2024), workspace crates `owl-dl-tableau` (the wedge), `owl-dl-reasoner` (HyperCache::build + probes), `owl-dl-core` (`Subsumers`, `ToldTables`), `owl-dl-saturation` (`saturate`).

## Global Constraints

- This is a **throwaway research gate**: the code does NOT merge. Only the verdict doc `docs/sp-b-saturation-guided-gate-results-2026-06-23.md` lands.
- Work on a throwaway sub-branch `spike/sat-guided-disjunction` branched off `feat/build-once-redesign`.
- Env flag `RUSTDL_SAT_GUIDE`: default OFF. The flag-OFF path MUST be byte-identical to current behavior (no field touched when the guide is `None`).
- `cargo fmt --all -- --check` clean; `cargo clippy --workspace --all-targets --all-features -- -D warnings` clean (pedantic on, `unwrap_used` warn) — CI runs on the branch, so even throwaway code must pass.
- `cargo test --workspace` green for the flag-OFF path.
- FP=0 is sacred. The gate's correctness guard is **verdict preservation** (flag-ON Sat/Unsat == flag-OFF) on the measured pairs — NOT corpus FP=0 (the code doesn't ship).
- Toolchain: `export RUSTUP_HOME=/home/dumontier/.rustup CARGO_HOME=/home/dumontier/.cargo; export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$CARGO_HOME/bin:$PATH"`.
- Commit only when the human asks. Commit messages end with:
  `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`
  `Claude-Session: https://claude.ai/code/session_01HSzon7V2wkhrudxBNAJduh`
- GO/NO-GO (pre-committed, STRICT): **GO** iff near-total Konclude-class collapse — total branch count drops to the hundreds regime (not tens of thousands) on **≥2/3 of the matched pairs**, AND a real wall drop (DNF/minutes → single-digit seconds), AND verdict-preserved. Else **NO-GO**.

---

## File Structure

- `crates/owl-dl-tableau/src/hyper.rs` — add `pub struct SatGuide`, a `sat_guide: Option<SatGuide>` field on the solver struct (the one owning `solve`/`find_open_disjunction`/`nodes`/`clauses`/`sub_roles`), a `with_sat_guide` setter mirroring `with_sub_roles`, the live-disjunct filter in `solve`'s ⊔ branch path, and three new `HyperStats` counters.
- `crates/owl-dl-reasoner/src/lib.rs` — in `HyperCache::build`, when `RUSTDL_SAT_GUIDE=1`, compute the `SatGuide` from `saturate(internal)` + `build_told_tables(internal)` and attach it to the constructed cache/solver.
- `crates/owl-dl-reasoner/tests/sat_guide_gate.rs` — throwaway harness: the four wine probes × {OFF, ON}, stats table, verdict-preservation assertions.
- `docs/sp-b-saturation-guided-gate-results-2026-06-23.md` — the durable verdict doc.

---

### Task 1: `SatGuide` view + `HyperStats` counters (tableau crate)

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` (add `SatGuide` struct near the other pub types ~line 360–445; add three counters to `HyperStats` struct, fields at ~365–382)
- Test: `crates/owl-dl-tableau/src/hyper.rs` (unit test in the existing `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `owl_dl_core::ClassId` (already imported).
- Produces:
  - `pub struct SatGuide { pub subsumers: Vec<Vec<ClassId>>, pub disjoint: Vec<Vec<ClassId>> }` — both indexed by `ClassId::index()`; `subsumers[c]` = derived subsumers of class `c` (including `c` itself is fine); `disjoint[c]` = sorted-by-index list of classes told-disjoint with `c`.
  - `impl SatGuide { pub fn is_dead(&self, disjunct: ClassId, label: &[ClassId]) -> bool }` — true iff some `C` in `label` has a subsumer `G` (in `subsumers[C]`) that is told-disjoint with `disjunct` (i.e. `self.disjoint[disjunct].binary_search_by_key(&G.index(), |g| g.index()).is_ok()`). Guards against out-of-range indices (return `false` if `disjunct.index() >= self.disjoint.len()`).
  - New `HyperStats` fields (all `u64`, `#[derive(Default)]` already on the struct): `pub disj_points_seen`, `pub disj_disjuncts_pruned`, `pub disj_forced_single`.

- [ ] **Step 1: Write the failing unit test**

Add to `mod tests` in `crates/owl-dl-tableau/src/hyper.rs`:

```rust
#[test]
fn sat_guide_is_dead_detects_disjoint_via_derived_subsumer() {
    // classes 0..4. Node label = {0}. 0's derived subsumers = {0, 2}.
    // 2 is told-disjoint with 3. So disjunct 3 is dead at label {0};
    // disjunct 1 (no disjointness) is live.
    let c = |i: u32| ClassId::from_index(i as usize);
    let guide = SatGuide {
        subsumers: vec![
            vec![c(0), c(2)], // subsumers[0]
            vec![c(1)],       // subsumers[1]
            vec![c(2)],       // subsumers[2]
            vec![c(3)],       // subsumers[3]
        ],
        disjoint: vec![
            vec![],     // disjoint[0]
            vec![],     // disjoint[1]
            vec![c(3)], // disjoint[2]  (2 ⟂ 3)
            vec![c(2)], // disjoint[3]  (3 ⟂ 2)
        ],
    };
    let label = [c(0)];
    assert!(guide.is_dead(c(3), &label), "3 dead: 0's subsumer 2 ⟂ 3");
    assert!(!guide.is_dead(c(1), &label), "1 live: no disjointness");
    // out-of-range disjunct is conservatively live
    assert!(!guide.is_dead(c(99), &label));
}
```

(If `ClassId::from_index` is not the constructor, use the actual one — check `owl_dl_core`’s `ClassId`; it is an index-newtype. `ClassId::index()` exists per `told.rs` usage `l.index()`.)

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p owl-dl-tableau --lib sat_guide_is_dead -- --nocapture`
Expected: FAIL to compile (`SatGuide` not defined).

- [ ] **Step 3: Add the `SatGuide` struct + counters**

Near the `HyperStats` struct in `hyper.rs`, add the three counter fields:

```rust
    /// Gate instrumentation (RUSTDL_SAT_GUIDE): ⊔ branch points the
    /// solver reached; disjuncts pruned dead by the saturation guide;
    /// ⊔ points the guide collapsed to a single live disjunct.
    pub disj_points_seen: u64,
    pub disj_disjuncts_pruned: u64,
    pub disj_forced_single: u64,
```

Add the struct + impl (place it after `HyperStats`, before the solver struct):

```rust
/// Throwaway SP-B viability-gate guide: per-class derived subsumers and
/// per-class told-disjoints, both indexed by `ClassId::index()`. Used to
/// prune ⊔ disjuncts incompatible with a node's label. NOT a production
/// type — gated by `RUSTDL_SAT_GUIDE`.
pub struct SatGuide {
    pub subsumers: Vec<Vec<ClassId>>,
    pub disjoint: Vec<Vec<ClassId>>,
}

impl SatGuide {
    /// True iff `disjunct` is incompatible with `label`: some class `C` in
    /// `label` has a derived subsumer `G` told-disjoint with `disjunct`.
    pub fn is_dead(&self, disjunct: ClassId, label: &[ClassId]) -> bool {
        let di = disjunct.index();
        if di >= self.disjoint.len() {
            return false;
        }
        let dj = &self.disjoint[di];
        for c in label {
            let ci = c.index();
            if ci >= self.subsumers.len() {
                continue;
            }
            for g in &self.subsumers[ci] {
                if dj.binary_search_by_key(&g.index(), |x| x.index()).is_ok() {
                    return true;
                }
            }
        }
        false
    }
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p owl-dl-tableau --lib sat_guide_is_dead -- --nocapture`
Expected: PASS.

- [ ] **Step 5: fmt + clippy + commit**

```bash
cargo fmt --all -- --check
cargo clippy -p owl-dl-tableau --all-targets --all-features -- -D warnings
git add crates/owl-dl-tableau/src/hyper.rs
git commit -m "spike(sp-b): SatGuide view + ⊔ gate counters" # + trailer lines
```

---

### Task 2: Thread `SatGuide` into the solver + `with_sat_guide` setter

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` (the solver struct that owns `solve`; add field + setter mirroring `with_sub_roles` at ~901/917)

**Interfaces:**
- Consumes: `SatGuide` (Task 1).
- Produces:
  - field `sat_guide: Option<SatGuide>` on the solver struct (default `None` in every constructor — `new`, `new_with_prebuilt`, `new_seeded`).
  - `pub fn with_sat_guide(mut self, guide: SatGuide) -> Self { self.sat_guide = Some(guide); self }`.

- [ ] **Step 1: Add the field to the solver struct**

Locate the solver struct (the one with `nodes`, `clauses`, `sub_roles: Option<RoleHierarchy>`, `stats`). Add:

```rust
    /// Throwaway SP-B gate guide (RUSTDL_SAT_GUIDE). `None` ⟹ flag-OFF,
    /// behavior byte-identical to production.
    sat_guide: Option<SatGuide>,
```

- [ ] **Step 2: Initialize `sat_guide: None` in every constructor**

In each of `new`, `new_with_prebuilt`, `new_seeded` (and any other struct-literal constructor of the solver), add `sat_guide: None,` to the struct literal.

- [ ] **Step 3: Add the setter**

Next to `with_sub_roles` (~line 901):

```rust
    #[must_use]
    pub fn with_sat_guide(mut self, guide: SatGuide) -> Self {
        self.sat_guide = Some(guide);
        self
    }
```

- [ ] **Step 4: Verify it compiles (flag-OFF byte-identical)**

Run: `cargo build -p owl-dl-tableau`
Expected: compiles; `sat_guide` is unused-but-present (an `#[allow(dead_code)]` on the field may be needed to satisfy clippy until Task 3 reads it — add it, remove in Task 3).

Run: `cargo test -p owl-dl-tableau`
Expected: all existing tests PASS (nothing reads the field yet).

- [ ] **Step 5: fmt + clippy + commit**

```bash
cargo fmt --all -- --check
cargo clippy -p owl-dl-tableau --all-targets --all-features -- -D warnings
git add crates/owl-dl-tableau/src/hyper.rs
git commit -m "spike(sp-b): thread sat_guide into solver + with_sat_guide setter" # + trailers
```

---

### Task 3: Live-disjunct filter in the ⊔ branch path

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` (the ⊔ branch block in `solve`, currently `hyper.rs:1707–1752`)
- Test: `crates/owl-dl-tableau/src/hyper.rs` (a unit test asserting a forced single-disjunct collapses branching while preserving the Sat verdict)

**Interfaces:**
- Consumes: `sat_guide` field (Task 2), `SatGuide::is_dead` (Task 1), the head atoms `self.clauses[ci].head[k]` (variant `Atom::Class(ClassId, Var)`), the node label `self.nodes[node.index()].labels` (a `Vec<ClassId>`), `resolve_var(v, node, binding)`.
- Produces: the filtered branch loop (no new public surface).

- [ ] **Step 1: Write the failing test**

Add to `mod tests`. Build a tiny clause set where a node carries label class `A`, a disjunctive clause head is `{B, D}`, the guide marks `D` dead at `{A}` (A's subsumer told-disjoint with D), and `B` is satisfiable. With a guide attached, the solver must reach `Sat` and report `disj_forced_single >= 1` and `disj_disjuncts_pruned >= 1`; without the guide it reaches the same `Sat` verdict. Model this on the existing `disjunction_sat_takes_first_branch` test (hyper.rs:3484) for clause/node construction idiom, adding `.with_sat_guide(...)`.

```rust
#[test]
fn sat_guide_forces_single_live_disjunct() {
    // Reuse the construction idiom of `disjunction_sat_takes_first_branch`.
    // Clause: body matches a node labelled A; head = {Class(B), Class(D)}.
    // Guide: subsumers[A] = {A}; disjoint[D] = {A} (so D dead at {A}); B live.
    // Expect: Sat, disj_forced_single >= 1, disj_disjuncts_pruned >= 1.
    // (Construct via the same helpers the sibling test uses; assert verdict
    //  equals the no-guide run.)
    // ... concrete construction mirrors disjunction_sat_takes_first_branch ...
}
```

(The implementer fills the construction by copying `disjunction_sat_takes_first_branch`'s setup verbatim and adding the guide + a dead disjunct `D`. The assertions are the three lines above.)

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p owl-dl-tableau --lib sat_guide_forces_single -- --nocapture`
Expected: FAIL (filter not implemented; either both branches taken or counters zero).

- [ ] **Step 3: Implement the filter**

Replace the head-iteration in the ⊔ block (currently `let head_len = self.clauses[ci].head.len(); ... for k in 0..head_len { ... }`) with a filtered index list. Insert immediately after `let decision_deps = body_deps.insert(d);`:

```rust
            let head_len = self.clauses[ci].head.len();
            // SP-B gate: prune head disjuncts incompatible with the node's
            // label per the saturation guide. Only named-class disjuncts are
            // filterable; ∃/≤n disjuncts stay live. Pruning is sound (a pruned
            // disjunct's class is told-disjoint with a derived subsumer of a
            // label class — a real clash), so an empty live set is a genuine
            // local clash.
            let live: Vec<usize> = if let Some(guide) = self.sat_guide.as_ref() {
                self.stats.disj_points_seen += 1;
                let label = self.nodes[node.index()].labels.clone();
                let mut keep = Vec::with_capacity(head_len);
                for k in 0..head_len {
                    let dead = match self.clauses[ci].head[k] {
                        Atom::Class(dk, v) => {
                            // resolve to the target node; filter on THAT node's label
                            match resolve_var(v, node, &binding) {
                                Some(t) => guide.is_dead(dk, &self.nodes[t.index()].labels),
                                None => false,
                            }
                        }
                        _ => false,
                    };
                    if dead {
                        self.stats.disj_disjuncts_pruned += 1;
                    } else {
                        keep.push(k);
                    }
                }
                if keep.len() == 1 {
                    self.stats.disj_forced_single += 1;
                }
                keep
            } else {
                (0..head_len).collect()
            };
            if live.is_empty() {
                // every disjunct pruned ⟹ this binding is unsatisfiable.
                // Conservative deps: the clause body's dep-set (sound — an
                // over-approx only weakens backjumping, never the verdict).
                self.clash_deps = body_deps;
                return HyperResult::Unsat;
            }
            let mut any_stalled = false;
            let mut combined = DepSet::EMPTY;
            for &k in &live {
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
```

Remove the `#[allow(dead_code)]` added in Task 2 (the field is now read).

NOTE: keep the `if depth == 0 { return HyperResult::Stalled; }`, `self.track_depth(depth);`, `let d = ...`, `let body_deps = ...`, `let decision_deps = ...` lines exactly as they are above this block.

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p owl-dl-tableau --lib sat_guide_forces_single -- --nocapture`
Expected: PASS (Sat verdict, `disj_forced_single >= 1`, `disj_disjuncts_pruned >= 1`).

- [ ] **Step 5: Verify flag-OFF is byte-identical**

Run: `cargo test -p owl-dl-tableau`
Expected: ALL existing tests PASS (the `else { (0..head_len).collect() }` arm reproduces today's behavior exactly — same `k` order, same dep logic).

- [ ] **Step 6: fmt + clippy + commit**

```bash
cargo fmt --all -- --check
cargo clippy -p owl-dl-tableau --all-targets --all-features -- -D warnings
git add crates/owl-dl-tableau/src/hyper.rs
git commit -m "spike(sp-b): live-disjunct filter in ⊔ branch path" # + trailers
```

---

### Task 4: Compute + attach `SatGuide` in `HyperCache::build` (reasoner)

**Files:**
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (`HyperCache::build`, struct around 1523–1591; build fn the place that constructs the solver/indexes)

**Interfaces:**
- Consumes: `owl_dl_saturation::saturate(internal) -> Subsumers`; `owl_dl_core::told::build_told_tables(internal) -> ToldTables`; `Subsumers::subsumers_of(c) -> Vec<ClassId>`; `ToldTables::disjoints_of(c) -> &[ClassId]`; `owl_dl_tableau::hyper::SatGuide`; the solver `with_sat_guide`.
- Produces: a `SatGuide`-attached cache when `RUSTDL_SAT_GUIDE=1`.

- [ ] **Step 1: Add a guide-builder helper in `HyperCache::build`**

Read `HyperCache::build` first to see where the solver/base indexes are constructed. When the flag is set, build the guide from the same `internal` already in scope:

```rust
        let sat_guide = if std::env::var("RUSTDL_SAT_GUIDE").as_deref() == Ok("1") {
            let closure = owl_dl_saturation::saturate(internal);
            let told = owl_dl_core::told::build_told_tables(internal);
            let n = internal.vocabulary.classes().count();
            let mut subsumers = vec![Vec::new(); n];
            let mut disjoint = vec![Vec::new(); n];
            for (id, _) in internal.vocabulary.classes() {
                subsumers[id.index()] = closure.subsumers_of(id);
                let mut dj: Vec<ClassId> = told.disjoints_of(id).to_vec();
                dj.sort_by_key(|c| c.index());
                disjoint[id.index()] = dj;
            }
            Some(owl_dl_tableau::hyper::SatGuide { subsumers, disjoint })
        } else {
            None
        };
```

(Confirm the exact names: `internal.vocabulary.classes()` is used elsewhere in this file (see `decide_pair_probe`); `ClassId` is in scope. If `HyperCache` stores prebuilt `ClauseIndexes` and constructs solvers per-query, store `sat_guide` on `HyperCache` (as `Option<SatGuide>` — note `SatGuide` is not `Clone`; either make it `Clone` by deriving it in Task 1, or rebuild per query). **Derive `Clone` on `SatGuide` in Task 1** to allow per-query attachment.)

- [ ] **Step 2: Attach the guide where the solver is created per query**

Wherever `HyperCache` builds a solver for a `decide`/`decide_with_stats` query (the per-pair path), chain `.with_sat_guide(guide.clone())` when `self.sat_guide` is `Some`. Add a `sat_guide: Option<SatGuide>` field to `HyperCache` and populate it in `build`.

- [ ] **Step 3: Update Task 1 — derive `Clone`**

Go back to `SatGuide` in `hyper.rs` and add `#[derive(Clone)]`.

- [ ] **Step 4: Verify both flag states build + flag-OFF tests pass**

```bash
cargo build -p owl-dl-reasoner
cargo test -p owl-dl-reasoner --lib    # flag OFF by default → unchanged
```
Expected: builds; flag-OFF tests pass.

- [ ] **Step 5: fmt + clippy + commit**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
git add crates/owl-dl-reasoner/src/lib.rs crates/owl-dl-tableau/src/hyper.rs
git commit -m "spike(sp-b): build+attach SatGuide in HyperCache::build under RUSTDL_SAT_GUIDE" # + trailers
```

---

### Task 5: Measurement harness (4 wine probes × {OFF, ON}) + verdict preservation

**Files:**
- Create: `crates/owl-dl-reasoner/tests/sat_guide_gate.rs`

**Interfaces:**
- Consumes: `owl_dl_reasoner::{decide_pair_probe, sat_class_probe}` (already public, lib.rs:~1076/1112), `HyperResult`, `SearchStats` (fields: `branches_taken`, `disj_branches`, `restores`, `node_clones`, `max_branch_depth`, plus the Task-1 gate counters `disj_points_seen`/`disj_disjuncts_pruned`/`disj_forced_single` — confirm `SearchStats` exposes them; if `SearchStats` is a re-export of `HyperStats` the fields are present, else add the three to `SearchStats` too).
- Produces: a `#[ignore]` test that prints the table (run explicitly).

- [ ] **Step 1: Write the harness test**

```rust
//! Throwaway SP-B viability gate. Run explicitly:
//!   RUSTDL_SAT_GUIDE=1 cargo test -p owl-dl-reasoner --test sat_guide_gate -- --ignored --nocapture
//! and again with RUSTDL_SAT_GUIDE unset for the OFF baseline.
use owl_dl_reasoner::{decide_pair_probe, sat_class_probe};
use std::time::Duration;

const WINE: &str = "ontologies/real/wine.ofn";
const NS: &str = "http://www.w3.org/TR/2003/PR-owl-guide-20031209/wine#";

fn load() -> impl horned_owl::ontology::set::SetOntology<std::sync::Arc<str>> { /* parse WINE via the same loader other reasoner tests use; see an existing tests/*.rs for the parse helper */ unimplemented!() }

#[test]
#[ignore = "throwaway SP-B viability gate; run explicitly with/without RUSTDL_SAT_GUIDE"]
fn sat_guide_wine_branch_collapse() {
    let onto = load();
    let depth = 256;
    let dl = Some(Duration::from_secs(120));
    let pairs: &[(&str, Option<&str>)] = &[
        ("AlsatianWine", Some("AmericanWine")), // sat(Alsatian ⊓ ¬American)
        ("SweetWine", None),
        ("Zinfandel", None),
        ("RedWine", None),
    ];
    println!("flag_on={}", std::env::var("RUSTDL_SAT_GUIDE").as_deref() == Ok("1"));
    for (sub, sup) in pairs {
        let s = format!("{NS}{sub}");
        let out = match sup {
            Some(p) => decide_pair_probe(&onto, &s, &format!("{NS}{p}"), depth, dl),
            None => sat_class_probe(&onto, &s, depth, dl),
        }
        .expect("probe ok");
        match out {
            Some((res, st, ms)) => println!(
                "{sub:14} verdict={res:?} branches={} disj_branches={} restores={} \
                 points_seen={} pruned={} forced_single={} wall_ms={ms:.0}",
                st.branches_taken, st.disj_branches, st.restores,
                st.disj_points_seen, st.disj_disjuncts_pruned, st.disj_forced_single,
            ),
            None => println!("{sub:14} NOT A NAMED CLASS"),
        }
    }
}
```

(The implementer wires `load()` using whatever parse helper the existing reasoner integration tests use — grep `crates/owl-dl-reasoner/tests/` for a `.ofn` loader. Confirm the exact wine IRIs by grepping `ontologies/real/wine.ofn` for `AlsatianWine`/`SweetWine`/`Zinfandel`/`RedWine`/`AmericanWine` — fix the local-name/namespace if they differ.)

- [ ] **Step 2: Run OFF baseline**

Run: `cargo test -p owl-dl-reasoner --test sat_guide_gate -- --ignored --nocapture`
Expected: prints `flag_on=false` and a row per pair with the OFF branch counts (wine pairs likely tens of thousands of branches / DNF-at-deadline).

- [ ] **Step 3: Run ON**

Run: `RUSTDL_SAT_GUIDE=1 cargo test -p owl-dl-reasoner --test sat_guide_gate -- --ignored --nocapture`
Expected: prints `flag_on=true` with branch counts and the gate counters (`points_seen`/`pruned`/`forced_single`).

- [ ] **Step 4: Verdict-preservation assertion**

Confirm by eye (and optionally assert in the test) that each pair's `verdict` is identical between OFF and ON (for pairs that DNF at OFF, compare ON's verdict to the known oracle: AmericanWine/Alsatian etc. from the wine corpus closure). If any verdict flips, the filter is buggy — STOP and fix Task 3 before trusting any branch count.

- [ ] **Step 5: Commit the harness (throwaway)**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
git add crates/owl-dl-reasoner/tests/sat_guide_gate.rs
git commit -m "spike(sp-b): wine branch-collapse measurement harness" # + trailers
```

---

### Task 6: Verdict doc + GO/NO-GO call (the only durable artifact)

**Files:**
- Create: `docs/sp-b-saturation-guided-gate-results-2026-06-23.md`

- [ ] **Step 1: Record the measurement table**

Write the OFF-vs-ON table (pair × {branches, disj_branches, restores, wall, verdict}) plus the gate counters (`points_seen`/`pruned`/`forced_single`) for the ON run. Include the **named-vs-successor diagnosis**: if `points_seen` is high but `pruned`/`forced_single` are ~0, the hint rarely fired (explosion is on successor/Tseitin nodes — true NO-GO); if `pruned`/`forced_single` are high but branches stay in the tens of thousands, the hint fired but didn't collapse (also NO-GO).

- [ ] **Step 2: State verdict preservation**

Explicitly record that every measured pair's verdict matched OFF (or the oracle). If not, record the bug and that the gate is INVALID until fixed.

- [ ] **Step 3: GO/NO-GO call against the pre-committed bar**

Apply the Global-Constraints bar: GO iff Konclude-class collapse (hundreds, not tens of thousands) on ≥2/3 pairs AND single-digit-second wall AND verdict-preserved. State GO or NO-GO and the one-line consequence (GO → write the production SP-B spec; NO-GO → bank B1–B2c, wine stays accepted perf gap).

- [ ] **Step 4: Commit the verdict doc (durable)**

```bash
git add docs/sp-b-saturation-guided-gate-results-2026-06-23.md
git commit -m "docs(sp-b): saturation-guided viability gate verdict + GO/NO-GO" # + trailers
```

- [ ] **Step 5: Discard the throwaway code**

The code branch `spike/sat-guided-disjunction` is NOT merged. After the verdict doc is on `feat/build-once-redesign` (cherry-pick just the doc commit), the spike branch is left for reference or deleted. Do NOT merge the gate code into `feat/build-once-redesign` or `main`.

---

## Self-Review

**1. Spec coverage:** mechanism (Task 3) ✓; SatGuide from closure+told (Tasks 1,4) ✓; RUSTDL_SAT_GUIDE flag + OFF byte-identical (Tasks 2,3,4) ✓; 4 matched wine probes via existing probes (Task 5) ✓; HyperStats branch counters (Task 1) ✓; verdict-preservation check (Task 5,6) ✓; named-vs-successor diagnosis (Task 1 counters + Task 6) ✓; GO/NO-GO bar (Global Constraints + Task 6) ✓; throwaway/verdict-doc-only (Global Constraints + Task 6) ✓.

**2. Placeholder scan:** `load()` and the synthetic-clause construction in Task 3/5 reference existing idioms (sibling tests) the implementer copies — flagged explicitly with the source test to copy (`disjunction_sat_takes_first_branch`) and the grep to find the loader. These are construction-by-analogy, not unspecified logic. No "TBD"/"handle edge cases".

**3. Type consistency:** `SatGuide { subsumers: Vec<Vec<ClassId>>, disjoint: Vec<Vec<ClassId>> }`, `is_dead(disjunct, label)`, `with_sat_guide`, counters `disj_points_seen`/`disj_disjuncts_pruned`/`disj_forced_single` — consistent across Tasks 1–6. `Clone` derive on `SatGuide` reconciled (Task 4 Step 3 amends Task 1). One open verification handed to the implementer: whether `SearchStats` (returned by `decide_with_stats`) is the same struct as `HyperStats` or a wrapper — Task 5 Step 1 flags it and the fix (add the three fields to `SearchStats` too if it's a wrapper).
