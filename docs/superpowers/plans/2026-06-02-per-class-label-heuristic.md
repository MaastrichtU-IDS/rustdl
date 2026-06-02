# Per-class label heuristic + per-pair verify — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Cut per-pair tableau calls by ≥50% on non-Horn workloads (ORE-10908/15672, pizza) by adding a HermiT-style sound non-subsumption pruner: each named class's wedge satisfiability runs ONCE, the root node's labels seed a cache, the orchestrator skips per-pair `subsumes_via_tableau` when `D ∉ labels(C)` (sound counterexample) and verifies when `D ∈ labels(C)` (sound by existing per-pair path). FP=0 invariant preserved.

**Architecture:** Three layers. (1) `HyperEngine::satisfiability_labels(seed)` exposes the root-node's labels after a Horn-fixpoint + branching search. (2) `HyperCache::classify_labels(c, deadline) -> LabelOracle` wraps it, constructing the seed `q ⊑ c` clause analogous to `HyperCache::decide`'s `q ⊑ sub` setup. (3) `classify_top_down_internal` builds a `Vec<LabelOracle>` per-class cache in parallel (rayon), and `find_direct_parents_top_down` consults the cache before each per-pair test.

**Tech Stack:** Rust (edition 2024), `owl-dl-tableau` + `owl-dl-reasoner` crates, existing `HyperEngine` / `HyperCache` / `PreparedOntology` / `ClassificationStats` infrastructure. Existing rayon parallel pattern in the per-class unsat probe loop is reused for label-cache construction.

---

## File structure

- **Modify** `crates/owl-dl-tableau/src/hyper.rs` — add `HyperEngine::satisfiability_labels(seed_class) -> Option<Vec<ClassId>>` accessor + a `pub(crate)` `HyperEngine::seed_class()` getter for the seed class. Net ~30 lines.
- **Modify** `crates/owl-dl-reasoner/src/lib.rs` — add `LabelOracle` enum (Sat/Unsat/NoVerdict), add `HyperCache::classify_labels(c, deadline) -> LabelOracle`, add `PreparedOntology::classify_labels(c, deadline) -> LabelOracle` wrapper. Net ~70 lines.
- **Modify** `crates/owl-dl-reasoner/src/classify.rs` — add 3 `ClassificationStats` counters, build label cache as part of the unsat-probe loop, gate the per-pair check in `find_direct_parents_top_down`. Net ~50 lines.
- **Modify** `crates/owl-dl-cli/src/main.rs` — display the three new counters in `write_classification`. Net ~10 lines.
- **Create** `crates/owl-dl-reasoner/tests/label_heuristic_canary.rs` — structural test: minimal ontology where the label cache definitively prunes a non-subsumption pair. Net ~80 lines.

---

## Background the executor needs

- **HyperCache lives at `crates/owl-dl-reasoner/src/lib.rs:722-804`.** It owns the clausified ontology + the "fresh_q" sentinel class. `HyperCache::decide(sub, sup, deadline)` constructs Q-clauses `q ⊑ sub` + `q ⊑ ¬sup`, then runs `HyperEngine::decide_with_deadline`. Returns `HyperVerdict::{Subsumed, NotSubsumed, Unknown}`. The new `classify_labels(c, deadline)` follows the same pattern but with only the `q ⊑ c` clause (no negated sup).
- **HyperEngine lives at `crates/owl-dl-tableau/src/hyper.rs:265`.** Each `HyperNode` has a `labels: Vec<ClassId>` field (line 138). The seed class (`fresh_q`) passed at construction starts labeled at node 0. After the search returns `HyperResult::Sat`, the seeded node's `labels` vector contains every atomic class derivable from `q` via the Horn fixpoint + the model branches taken. That set IS the label heuristic for `q ≡ c` (i.e., for class `c` since `q ⊑ c` and `c` propagates labels onto `q`'s node).
- **ClassificationStats lives at `crates/owl-dl-reasoner/src/classify.rs:52`.** Fields are flat `usize` counters. Add three new fields: `label_cache_pruned`, `label_cache_pass_through`, `label_cache_misses`.
- **The unsat probe loop lives at `crates/owl-dl-reasoner/src/classify.rs:689-727`.** Currently calls `prepared.decide_with_deadline(...)` per class. The label-cache build can run AS A SECOND PARALLEL PASS over the same `(0..n)` range OR fuse into the unsat probe (Step 5 of this plan picks the fused option).
- **find_direct_parents_top_down lives at `crates/owl-dl-reasoner/src/classify.rs:1091`.** Phase 6 added the visited bitset. The label-cache check inserts BEFORE the existing `closure.contains` check.
- **CLAUDE.md soundness contract:** FP=0 throughout. The verify-positives path (existing `subsumes_via_tableau`) preserves the contract; the new prune-negatives path is sound by the counterexample-model argument (per design spec).

**ENV NOTE**: cargo and rustc are NOT on the default shell PATH:
```bash
export PATH=/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH
```

---

## Task 1: `HyperEngine` label-readback API

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` (add public method near existing `decide` at line 718).
- Modify: `crates/owl-dl-tableau/src/hyper.rs` (`mod tests` near the bottom — search `mod tests`).

- [ ] **Step 1: Locate the `HyperEngine` impl block + existing tests**

```bash
export PATH=/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH
grep -nE "^impl[^a-zA-Z].*HyperEngine|^pub fn decide|^    pub fn decide" crates/owl-dl-tableau/src/hyper.rs | head -5
grep -nE "^#\[cfg\(test\)\]|^mod tests" crates/owl-dl-tableau/src/hyper.rs | head -3
```

Confirm the impl block contains `decide` at line ~718 and `mod tests` exists near the bottom.

- [ ] **Step 2: Write a failing test in `mod tests`**

Find the `mod tests` block. Add a test that constructs a minimal Horn-only clause set `{q ⊑ A, A ⊑ B}`, runs `decide_with_deadline`, then asserts the new accessor returns both `A` and `B` in the seed-node's labels.

```rust
#[test]
fn satisfiability_labels_returns_horn_consequences_at_seed_node() {
    use owl_dl_core::clause::{Atom, DlClause, X};
    use owl_dl_core::ir::ClassId;

    let q = ClassId::new(100);
    let a = ClassId::new(101);
    let b = ClassId::new(102);

    let clauses = vec![
        // q ⊑ a (q's seed label triggers a)
        DlClause {
            body: vec![Atom::Class(q, X)],
            head: vec![Atom::Class(a, X)],
        },
        // a ⊑ b (then b)
        DlClause {
            body: vec![Atom::Class(a, X)],
            head: vec![Atom::Class(b, X)],
        },
    ];
    let mut engine = HyperEngine::new(&clauses, q);
    let result = engine.decide(8);
    assert_eq!(result, HyperResult::Sat, "Horn fixpoint should be Sat");

    let labels = engine
        .satisfiability_labels(q)
        .expect("Sat result must expose seed-node labels");
    assert!(labels.contains(&a), "labels must contain A (q ⊑ a): {labels:?}");
    assert!(labels.contains(&b), "labels must contain B (Horn-derived): {labels:?}");
    assert!(labels.contains(&q), "labels include the seed class itself: {labels:?}");
}
```

Place this test inside the existing `#[cfg(test)] mod tests { ... }` block. If `HyperEngine::new` / `HyperResult::Sat` / `Atom::Class` / `X` aren't already in scope of `mod tests`, mirror the imports used by neighboring tests in the same module.

- [ ] **Step 3: Run the test — expect compile failure (`satisfiability_labels` not defined)**

```bash
cargo test -p owl-dl-tableau --lib satisfiability_labels 2>&1 | tail -10
```

Expected: error[E0599]: no method named `satisfiability_labels` found for struct `HyperEngine`.

- [ ] **Step 4: Implement `HyperEngine::satisfiability_labels`**

Add this method to the `HyperEngine` impl block, immediately after the existing `decide_with_deadline` at line ~725. The method walks the engine's `nodes` and returns the labels of the seed-class node:

```rust
    /// On a successful satisfiability search, return the labels of the
    /// node seeded with `seed`. Returns `None` if the search hasn't
    /// returned Sat OR if no node is labeled with `seed` (shouldn't
    /// happen for a well-formed Q-clause setup — Q's seed is always
    /// asserted at node 0 by `new`).
    ///
    /// The returned set is the basis for the per-class label heuristic
    /// in `owl-dl-reasoner::classify_top_down_internal`: any atomic
    /// class D ∈ this set is a candidate subsumer of `seed`; any
    /// D ∉ this set is a sound non-subsumer (this completion graph IS
    /// a counterexample model). See
    /// `docs/superpowers/specs/2026-06-02-per-class-label-heuristic-design.md`.
    #[must_use]
    pub fn satisfiability_labels(&self, seed: ClassId) -> Option<Vec<ClassId>> {
        // Walk every node; the one labeled with `seed` is the root.
        // (HyperCache::new seeds `q` at node 0; merges may relocate.
        // Linear scan is fine — node count is small at typical inputs.)
        for node in &self.nodes {
            if node.labels.contains(&seed) {
                return Some(node.labels.clone());
            }
        }
        None
    }
```

If `HyperNode.labels` isn't accessible from this impl (private to the struct), check the existing `decide`'s reads. The struct is defined at line 135 of the same file with `labels: Vec<ClassId>` — same-module access is fine.

- [ ] **Step 5: Run the test — expect pass**

```bash
cargo test -p owl-dl-tableau --lib satisfiability_labels 2>&1 | tail -10
```

Expected: `test result: ok. 1 passed`.

- [ ] **Step 6: Full tableau regression sweep + CI strict**

```bash
cargo test -p owl-dl-tableau -- --test-threads=1 2>&1 | tail -5
RUSTFLAGS="-D warnings" cargo test -p owl-dl-tableau --no-run 2>&1 | tail -3
cargo clippy -p owl-dl-tableau --all-targets -- -D warnings 2>&1 | grep -E "warning|error" | head -5
```

Expected: all 88+ tests pass; CI strict clean (any pre-existing clippy errors are unrelated and acceptable for this task — verify they're unchanged from `main` HEAD).

- [ ] **Step 7: Commit**

```bash
git add crates/owl-dl-tableau/src/hyper.rs
git commit -m "feat(tableau): HyperEngine::satisfiability_labels accessor

After a Sat verdict, expose the seed-class node's label set so
per-class label heuristics can read off Horn-derived + branch-asserted
consequences. Basis for the per-class label cache in
docs/superpowers/specs/2026-06-02-per-class-label-heuristic-design.md."
```

---

## Task 2: `LabelOracle` enum + `HyperCache::classify_labels`

**Files:**
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (add enum near `HyperVerdict`, add method on `HyperCache` near line 770).

- [ ] **Step 1: Locate `HyperVerdict` and the existing `HyperCache::decide`**

```bash
grep -nE "^pub.*enum HyperVerdict|impl HyperCache|pub.*fn decide\(" crates/owl-dl-reasoner/src/lib.rs | head -5
```

Confirm `HyperVerdict` is a `pub(crate)` enum and `HyperCache::decide` is `pub(crate) fn decide(...)` at line ~770.

- [ ] **Step 2: Add the `LabelOracle` enum next to `HyperVerdict`**

Find the `HyperVerdict` enum definition (search for `enum HyperVerdict`). Add this enum below it:

```rust
/// Per-class label heuristic oracle. Built by `HyperCache::classify_labels`
/// once per named class at classify-time; consulted by the orchestrator
/// to prune `subsumes_via_tableau` calls. See
/// `docs/superpowers/specs/2026-06-02-per-class-label-heuristic-design.md`.
#[derive(Debug, Clone)]
pub(crate) enum LabelOracle {
    /// C is satisfiable; root-node labels are the candidate subsumer
    /// set. `D ∈ labels` → verify via per-pair test; `D ∉ labels` →
    /// sound non-subsumption (this completion graph is a counterexample).
    Sat(std::collections::HashSet<owl_dl_core::ir::ClassId>),
    /// C is unsatisfiable (every model omits C). Orchestrator returns
    /// `true` for every (C, D) — unsat classes vacuously subsume all.
    Unsat,
    /// Deadline elapsed; no labels recorded. Orchestrator falls through
    /// to the existing per-pair path (sound by existing contract).
    NoVerdict,
}
```

- [ ] **Step 3: Add `HyperCache::classify_labels`**

Add this method to the `impl HyperCache` block, immediately after the existing `decide` method (around line 803):

```rust
    /// Run wedge satisfiability of `c` alone (no negated sup) and return
    /// a [`LabelOracle`] capturing the seed-node's labels. Sound basis
    /// for the per-class label heuristic — see
    /// `docs/superpowers/specs/2026-06-02-per-class-label-heuristic-design.md`.
    pub(crate) fn classify_labels(
        &self,
        c: owl_dl_core::ir::ClassId,
        deadline: Option<std::time::Instant>,
    ) -> LabelOracle {
        use owl_dl_core::clause::{Atom, DlClause, X};
        use owl_dl_tableau::hyper::{HyperEngine, HyperResult};
        let mut clauses = self.clauses.clone();
        // Single Q-clause: q ⊑ c. No negated sup (unlike `decide`).
        clauses.push(DlClause {
            body: vec![Atom::Class(self.fresh_q, X)],
            head: vec![Atom::Class(c, X)],
        });
        let mut engine = HyperEngine::new(&clauses, self.fresh_q);
        if hyper_double_block_enabled() {
            engine = engine.with_double_blocking();
        }
        match engine.decide_with_deadline(HYPER_WEDGE_DEPTH, deadline) {
            HyperResult::Unsat => LabelOracle::Unsat,
            HyperResult::Sat => engine
                .satisfiability_labels(self.fresh_q)
                .map(|v| LabelOracle::Sat(v.into_iter().collect()))
                .unwrap_or(LabelOracle::NoVerdict),
            HyperResult::Stalled => LabelOracle::NoVerdict,
        }
    }
```

- [ ] **Step 4: Add a test on HyperCache::classify_labels**

Find the test module in `crates/owl-dl-reasoner/src/lib.rs` (search for `mod tests` near the bottom). Add this test mirroring the existing `HyperCache` tests' style (look around line 2170 for the existing `HyperCache::build(&internal).decide(...)` pattern):

```rust
#[test]
fn hypercache_classify_labels_returns_atomic_supers_on_horn_chain() {
    use owl_dl_core::convert::convert_ontology;
    use horned_owl::io::ParserConfiguration;
    use horned_owl::io::ofn::reader::read;
    use horned_owl::model::RcStr;
    use horned_owl::ontology::set::SetOntology;
    use std::io::Cursor;

    let src = "\
Prefix(:=<http://test/label/>)
Ontology(<http://test/label>
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    SubClassOf(:A :B)
    SubClassOf(:B :C)
)
";
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read(&mut reader, ParserConfiguration::default()).expect("parse");
    let internal = convert_ontology(&onto).expect("convert");
    let a = internal
        .vocabulary
        .class_id("http://test/label/A")
        .expect("A declared");
    let b = internal
        .vocabulary
        .class_id("http://test/label/B")
        .expect("B declared");
    let c = internal
        .vocabulary
        .class_id("http://test/label/C")
        .expect("C declared");
    let cache = HyperCache::build(&internal);
    let oracle = cache.classify_labels(a, None);
    match oracle {
        LabelOracle::Sat(labels) => {
            assert!(labels.contains(&b), "A's labels must contain B: {labels:?}");
            assert!(labels.contains(&c), "A's labels must contain C: {labels:?}");
        }
        other => panic!("expected Sat, got {other:?}"),
    }
}
```

- [ ] **Step 5: Run the test — expect pass**

```bash
export PATH=/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH
cargo test -p owl-dl-reasoner --lib hypercache_classify_labels 2>&1 | tail -10
```

Expected: `test result: ok. 1 passed`.

- [ ] **Step 6: Reasoner-lib regression sweep**

```bash
cargo test -p owl-dl-reasoner --lib -- --test-threads=1 2>&1 | tail -5
RUSTFLAGS="-D warnings" cargo test -p owl-dl-reasoner --no-run 2>&1 | tail -3
```

Expected: 84 tests pass (the 83 pre-existing + 1 new); CI strict clean.

- [ ] **Step 7: Commit**

```bash
git add crates/owl-dl-reasoner/src/lib.rs
git commit -m "feat(reasoner): LabelOracle + HyperCache::classify_labels

Per-class wedge satisfiability that returns the seed-node's label
set (HermiT-style heuristic basis). Three-valued: Sat(labels) /
Unsat / NoVerdict. Used by orchestrator's per-class cache in a
follow-up commit to prune subsumes_via_tableau calls.

See docs/superpowers/specs/2026-06-02-per-class-label-heuristic-design.md."
```

---

## Task 3: `PreparedOntology::classify_labels` wrapper

**Files:**
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (impl block on `PreparedOntology` starting at line 1348).

- [ ] **Step 1: Locate `PreparedOntology` impl block**

```bash
grep -nE "impl PreparedOntology|fn hyper_decide" crates/owl-dl-reasoner/src/lib.rs | head -3
```

Confirm `impl PreparedOntology` starts at line 1348 and `hyper_decide` at line ~1391.

- [ ] **Step 2: Add `PreparedOntology::classify_labels`**

Add this method immediately after `hyper_decide` (around line 1400):

```rust
    /// Per-class label heuristic: run wedge satisfiability of `c` and
    /// return a [`LabelOracle`]. Returns
    /// [`LabelOracle::NoVerdict`] when the hyper wedge is disabled.
    /// See `docs/superpowers/specs/2026-06-02-per-class-label-heuristic-design.md`.
    pub(crate) fn classify_labels(
        &self,
        c: owl_dl_core::ir::ClassId,
        deadline: Option<std::time::Instant>,
    ) -> LabelOracle {
        match self.hyper.as_ref() {
            Some(hc) => hc.classify_labels(c, deadline),
            None => LabelOracle::NoVerdict,
        }
    }
```

- [ ] **Step 3: Run the existing test from Task 2 to confirm no regression**

```bash
cargo test -p owl-dl-reasoner --lib hypercache_classify_labels 2>&1 | tail -5
RUSTFLAGS="-D warnings" cargo test -p owl-dl-reasoner --no-run 2>&1 | tail -3
```

Both must pass.

- [ ] **Step 4: Commit**

```bash
git add crates/owl-dl-reasoner/src/lib.rs
git commit -m "feat(reasoner): PreparedOntology::classify_labels wrapper

Thin wrapper that delegates to HyperCache::classify_labels when the
wedge is enabled, returns NoVerdict otherwise. The classify pipeline
will consume this in a follow-up."
```

---

## Task 4: Add `ClassificationStats` counters

**Files:**
- Modify: `crates/owl-dl-reasoner/src/classify.rs` (`pub struct ClassificationStats` at line 52).
- Modify: `crates/owl-dl-cli/src/main.rs` (`write_classification` near line 246).

- [ ] **Step 1: Locate ClassificationStats**

```bash
grep -nE "pub struct ClassificationStats|pub.*usize" crates/owl-dl-reasoner/src/classify.rs | head -15
```

Identify the struct's field layout (it has 8-10 `usize` fields, all `pub`).

- [ ] **Step 2: Add three fields**

Add these fields to the struct, alphabetically near the existing ones (look for `pub timed_out_pairs: usize` and similar):

```rust
    /// Per-class label heuristic (Phase 7): pairs where the orchestrator
    /// skipped `subsumes_via_tableau` because D was absent from C's
    /// label cache (sound non-subsumption via counterexample-model).
    pub label_cache_pruned: usize,
    /// Per-class label heuristic: pairs where D was present in C's
    /// label cache and the orchestrator fell through to the existing
    /// per-pair verification (might be coincidence of model).
    pub label_cache_pass_through: usize,
    /// Per-class label heuristic: pairs where the cache was missing
    /// (NoVerdict or hyper disabled) and the orchestrator fell through.
    pub label_cache_misses: usize,
```

- [ ] **Step 3: Display fields in CLI banner**

Open `crates/owl-dl-cli/src/main.rs`, find `write_classification` (around line 246). Find the existing block that prints `# subsumption: saturation=X tableau=Y`. Add a new line after the existing `# satisfiability probes` line:

```rust
    writeln!(
        out,
        "# label heuristic: pruned={} pass_through={} misses={}",
        stats.label_cache_pruned,
        stats.label_cache_pass_through,
        stats.label_cache_misses,
    )?;
```

- [ ] **Step 4: Build + CI strict**

```bash
cargo build --release -p owl-dl-cli 2>&1 | tail -3
RUSTFLAGS="-D warnings" cargo test -p owl-dl-reasoner --no-run 2>&1 | tail -3
```

Expected: clean build.

- [ ] **Step 5: Run existing tests — no behaviour change**

```bash
cargo test -p owl-dl-reasoner --lib -- --test-threads=1 2>&1 | tail -5
```

Expected: all tests pass (zero values for the new counters; no regression).

- [ ] **Step 6: Commit**

```bash
git add crates/owl-dl-reasoner/src/classify.rs crates/owl-dl-cli/src/main.rs
git commit -m "feat(reasoner): ClassificationStats label_cache counters

Add three pub usize counters for the per-class label heuristic:
pruned (sound non-sub), pass_through (verified positive), misses
(NoVerdict/disabled). Surface as a new # label heuristic: banner
line in the CLI's write_classification output. Counters all default
to 0; behaviour unchanged until the cache is wired up in Task 5/6."
```

---

## Task 5: Build the label cache in parallel during classify

**Files:**
- Modify: `crates/owl-dl-reasoner/src/classify.rs` — extend the unsat probe loop at lines 689-727 to also build `Vec<LabelOracle>`.

- [ ] **Step 1: Read the existing unsat probe loop**

```bash
sed -n '688,730p' crates/owl-dl-reasoner/src/classify.rs
```

Confirm the structure: `(0..n).into_par_iter().map(|i| { ... })` returns `(usize, bool, bool)` (idx, is_sat, used_saturation). Each iteration may call `prepared.decide_with_deadline(...)` for a non-EL satisfiable test.

- [ ] **Step 2: Add the parallel label-cache build BEFORE the unsat probe**

Find the line `let unsat_probe_results: Result<Vec<(usize, bool, bool)>, ReasonError> = (0..n)` (around line 689). IMMEDIATELY BEFORE that block, add:

```rust
    // Phase 7: per-class label heuristic. Run wedge satisfiability per
    // named class ONCE; cache the root-node labels as a sound
    // non-subsumption pruner. Parallel via rayon — independent calls,
    // ~0.5-2 ms each (Horn case) + occasional slower disjunctive
    // cases. See
    // docs/superpowers/specs/2026-06-02-per-class-label-heuristic-design.md.
    let label_cache: Vec<crate::LabelOracle> = (0..n)
        .into_par_iter()
        .map(|i| {
            let class_id = owl_dl_core::ClassId::new(
                u32::try_from(i).expect("class index fits in u32"),
            );
            let deadline = per_pair_timeout.map(|t| Instant::now() + t);
            prepared.classify_labels(class_id, deadline)
        })
        .collect();
```

The cache is `Vec<LabelOracle>` indexed by class-index 0..n; matches the existing `(0..n)` index pattern of unsat_probe_results. The deadline is the per-class budget, same as the unsat probe.

- [ ] **Step 3: Build + CI strict (no behavioural change yet)**

```bash
cargo build --release -p owl-dl-cli 2>&1 | tail -3
RUSTFLAGS="-D warnings" cargo test -p owl-dl-reasoner --no-run 2>&1 | tail -3
```

Expected: clean — `label_cache` is built but not yet consulted, so behaviour is unchanged.

- [ ] **Step 4: Run a quick smoke test (alehif) to confirm no regression**

```bash
cargo test -p owl-dl-reasoner --test konclude_closure_diff --release -- --ignored --nocapture alehif_closure_matches_konclude 2>&1 | grep -E "FP=|MISSED=|test result"
```

Expected: FP=0 / MISSED=0. (Cache is built but not consulted; classify behaviour identical.)

- [ ] **Step 5: Commit**

```bash
git add crates/owl-dl-reasoner/src/classify.rs
git commit -m "feat(reasoner): build per-class label cache in classify_top_down

Parallel rayon pass over (0..n) producing Vec<LabelOracle> immediately
before the existing unsat probe loop. No behavioural change yet — the
cache is built but not consulted; Task 6 wires it into
find_direct_parents_top_down. This separation lets us measure the
build cost in isolation if it ever becomes a concern."
```

---

## Task 6: Integrate cache into `find_direct_parents_top_down`

**Files:**
- Modify: `crates/owl-dl-reasoner/src/classify.rs` (the find_direct_parents_top_down function around line 1091 + its call site).

- [ ] **Step 1: Read the function signature + call site**

```bash
sed -n '1090,1125p' crates/owl-dl-reasoner/src/classify.rs
grep -nE "find_direct_parents_top_down\(" crates/owl-dl-reasoner/src/classify.rs | head -5
```

Confirm: the function takes `(c, closure, prepared, direct_supers, direct_children, top_level, per_pair_timeout, stats)`. Call site is in the tier-walk parallel block (around line 788).

- [ ] **Step 2: Extend the signature with the label cache**

Change the function signature to add the new parameter:

```rust
#[allow(clippy::too_many_arguments)]
fn find_direct_parents_top_down(
    c: usize,
    closure: &owl_dl_saturation::Subsumers,
    prepared: &PreparedOntology,
    direct_supers: &[Vec<usize>],
    direct_children: &[Vec<usize>],
    top_level: &[usize],
    per_pair_timeout: Option<std::time::Duration>,
    label_cache: &[crate::LabelOracle],
    stats: &mut ClassificationStats,
) -> Result<Vec<usize>, ReasonError> {
```

- [ ] **Step 3: Insert the cache check inside the walk loop**

Find the line `let subsumed = if closure.contains(c_id, d_id) {` (around line 1109). REPLACE the `let subsumed = ...` block (including the `else` branch that calls `subsumes_via_tableau`) with:

```rust
        let subsumed = if closure.contains(c_id, d_id) {
            stats.saturation_subsumption_hits += 1;
            true
        } else {
            // Phase 7: per-class label heuristic — check the cache
            // before paying for `subsumes_via_tableau`.
            match label_cache.get(c) {
                Some(crate::LabelOracle::Sat(labels)) => {
                    if labels.contains(&d_id) {
                        // D ∈ C's labels: might be coincidence-of-model;
                        // verify via the existing per-pair path.
                        stats.label_cache_pass_through += 1;
                        subsumes_via_tableau(
                            prepared, c_id, d_id, per_pair_timeout, true, stats,
                        )?
                        .unwrap_or_default()
                    } else {
                        // D ∉ C's labels: this completion graph is a
                        // counterexample model. Sound non-subsumption.
                        stats.label_cache_pruned += 1;
                        false
                    }
                }
                Some(crate::LabelOracle::Unsat) => {
                    // C is unsatisfiable: vacuously subsumes every D.
                    true
                }
                Some(crate::LabelOracle::NoVerdict) | None => {
                    // Cache missing — fall through to per-pair.
                    stats.label_cache_misses += 1;
                    subsumes_via_tableau(
                        prepared, c_id, d_id, per_pair_timeout, true, stats,
                    )?
                    .unwrap_or_default()
                }
            }
        };
```

- [ ] **Step 4: Update the call site to pass `label_cache`**

Find the call to `find_direct_parents_top_down` in the tier-walk block (search `find_direct_parents_top_down(`). Update the argument list to pass `&label_cache` between `per_pair_timeout` and `&mut local_stats`:

```rust
                let parents = find_direct_parents_top_down(
                    c,
                    &closure,
                    &prepared,
                    &direct_supers,
                    &direct_children,
                    &top_level,
                    per_pair_timeout,
                    &label_cache,                // <-- new arg
                    &mut local_stats,
                )?;
```

- [ ] **Step 5: Add the per-tier stats merge for the new counters**

Find the block that merges `sd.*` deltas into the global `stats` (around line 803). Add the three new counter merges:

```rust
        for (c, parents, sd) in tier_results {
            stats.saturation_subsumption_hits += sd.saturation_subsumption_hits;
            stats.tableau_subsumption_calls += sd.tableau_subsumption_calls;
            stats.timed_out_pairs += sd.timed_out_pairs;
            stats.hyper_proven_pairs += sd.hyper_proven_pairs;
            stats.hyper_refuted_pairs += sd.hyper_refuted_pairs;
            stats.hyper_refuted_fast_pairs += sd.hyper_refuted_fast_pairs;
            stats.hyper_refuted_fast_flipped_pairs += sd.hyper_refuted_fast_flipped_pairs;
            stats.label_cache_pruned += sd.label_cache_pruned;            // <-- new
            stats.label_cache_pass_through += sd.label_cache_pass_through;// <-- new
            stats.label_cache_misses += sd.label_cache_misses;            // <-- new
            // ... rest unchanged
```

- [ ] **Step 6: Build**

```bash
cargo build --release -p owl-dl-cli 2>&1 | tail -3
RUSTFLAGS="-D warnings" cargo test -p owl-dl-reasoner --no-run 2>&1 | tail -3
```

Expected: clean build.

- [ ] **Step 7: Run Phase 0 soundness gate**

```bash
cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    -- --ignored --nocapture \
    alehif_closure_matches_konclude ore_10908_sroiq ore_15672_shoin \
    2>&1 | grep -E "^---|FP=|MISSED=|test result"
```

Expected: **FP=0 / MISSED=0** across all 3 fixtures. If any FP > 0, the heuristic is unsound — STOP and investigate (the most likely cause: label cache returns Sat with labels missing a D that's actually a logical-consequence subsumer; possible if the wedge engine's label propagation is incomplete on some construct).

- [ ] **Step 8: Quick smoke on alehif via rustdl CLI to inspect counters**

```bash
./target/release/rustdl classify --pair-timeout-ms 200 ontologies/external/alehif-test.ofn 2>&1 | grep -E "# classes|# fragment|# label heuristic"
```

Expected: the `# label heuristic:` line shows `pruned > 0` for any non-trivial classification. If `pruned == 0 && pass_through == 0`, the cache isn't firing — investigate.

- [ ] **Step 9: Reasoner-lib regression sweep**

```bash
cargo test -p owl-dl-reasoner --lib -- --test-threads=1 2>&1 | tail -5
```

Expected: 84 tests pass (all pre-existing tests must continue passing).

- [ ] **Step 10: Commit**

```bash
git add crates/owl-dl-reasoner/src/classify.rs
git commit -m "feat(reasoner): wire per-class label heuristic into top-down walk

find_direct_parents_top_down now consults label_cache[c] before each
non-closure-hit pair: prune if D ∉ labels(C) (sound non-subsumption),
verify if D ∈ labels(C) (existing per-pair path), fall through on
NoVerdict / Unsat handled as vacuous subsumption.

Phase 0 net (alehif + ORE-10908 + ORE-15672): FP=0/MISSED=0 unchanged.
Label heuristic banner shows pruned/pass_through/misses counts.

See docs/superpowers/specs/2026-06-02-per-class-label-heuristic-design.md
for the soundness contract."
```

---

## Task 7: Structural canary test (assert prune > 0 on a synthetic)

**Files:**
- Create: `crates/owl-dl-reasoner/tests/label_heuristic_canary.rs`.

- [ ] **Step 1: Create the test file**

```rust
//! Structural canary for the per-class label heuristic.
//!
//! Constructs a tiny synthetic ontology where many `C ⊑ D` queries
//! must succeed via the wedge closure AND many `C ⊑ D'` non-queries
//! must be pruned by `D' ∉ labels(C)`. Asserts that
//! `ClassificationStats::label_cache_pruned > 0`.
//!
//! Failure mode: the heuristic isn't firing (cache wiring broken,
//! or wedge labels are missing the disjoint-class atoms).

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::classify_top_down_with_timeout;
use std::io::Cursor;
use std::time::Duration;

#[test]
fn label_heuristic_prunes_disjoint_pairs() {
    // 4 classes: A, B disjoint chains. Many pair queries
    // (Ai ⊑ Bj) will have D ∉ labels(C) and should be pruned.
    let src = "\
Prefix(:=<http://test/lh/>)
Ontology(<http://test/lh>
    Declaration(Class(:A1))
    Declaration(Class(:A2))
    Declaration(Class(:A3))
    Declaration(Class(:B1))
    Declaration(Class(:B2))
    Declaration(Class(:B3))
    SubClassOf(:A2 :A1)
    SubClassOf(:A3 :A2)
    SubClassOf(:B2 :B1)
    SubClassOf(:B3 :B2)
    DisjointClasses(:A1 :B1)
)
";
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read(&mut reader, ParserConfiguration::default()).expect("parse");
    let result = classify_top_down_with_timeout(&onto, Duration::from_millis(200))
        .expect("classify");
    let stats = result.stats();

    // Sanity checks: classification correctness.
    assert!(
        result.is_subclass("http://test/lh/A3", "http://test/lh/A1"),
        "A3 ⊑ A1 via chain"
    );
    assert!(
        !result.is_subclass("http://test/lh/A3", "http://test/lh/B1"),
        "A3 ⋢ B1 (disjoint)"
    );

    // Label heuristic must have fired at least once. The disjoint
    // class atoms ensure pruning is exercised: querying Ai ⊑ Bj
    // sees D=Bj absent from labels(Ai) and prunes.
    assert!(
        stats.label_cache_pruned > 0,
        "Phase 7 label heuristic must prune at least one pair on this synthetic. \
         Got pruned={} pass_through={} misses={}",
        stats.label_cache_pruned,
        stats.label_cache_pass_through,
        stats.label_cache_misses,
    );
}
```

- [ ] **Step 2: Run the test — expect pass**

```bash
export PATH=/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH
cargo test -p owl-dl-reasoner --test label_heuristic_canary 2>&1 | tail -10
```

Expected: `test result: ok. 1 passed`. If it fails on the `pruned > 0` assertion, the wiring from Task 6 isn't firing — investigate.

- [ ] **Step 3: CI strict on the new test file**

```bash
RUSTFLAGS="-D warnings" cargo test -p owl-dl-reasoner --test label_heuristic_canary --no-run 2>&1 | tail -3
```

Expected: clean build.

- [ ] **Step 4: Commit**

```bash
git add crates/owl-dl-reasoner/tests/label_heuristic_canary.rs
git commit -m "test(reasoner): structural canary asserting label heuristic fires

Tiny synthetic with disjoint A/B chains. Querying Ai ⊑ Bj triggers
the heuristic prune (Bj ∉ labels(Ai)); asserts
ClassificationStats::label_cache_pruned > 0. Catches future regressions
of the cache wiring or the wedge's label propagation."
```

---

## Task 8: Soundness gate + corpus measurement

No file changes — measurement only. Captures the wall + completeness delta.

- [ ] **Step 1: Phase 0 net soundness gate (FP=0)**

```bash
export PATH=/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH
cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    -- --ignored --nocapture \
    alehif_closure_matches_konclude ore_10908_sroiq ore_15672_shoin \
    2>&1 | tee /tmp/p7-net.log | grep -E "^---|FP=|MISSED=|test result"
```

Expected: **FP=0 / MISSED=0** across all 3 fixtures. If any FP > 0, the heuristic is unsound; REVERT all wiring (Tasks 5-7) and STOP.

- [ ] **Step 2: ORE-10908 + ORE-15672 wall measurements**

```bash
cargo build --release -p owl-dl-cli 2>&1 | tail -3
for f in ore-10908-sroiq.ofn ore-15672-shoin.ofn; do
    echo "=== $f ==="
    /usr/bin/time -f "wall=%e s" timeout 300 ./target/release/rustdl classify \
        --pair-timeout-ms 200 ontologies/external/$f 2>&1 | \
        grep -E "# classes|# fragment|# subsumption|# label heuristic|wall=" | tee -a /tmp/p7-ore.log
done
```

Compare wall numbers to the baseline in
`docs/perf-2026-06-02-konclude-vs-rustdl.md`:
- ORE-10908: was 27.37s; target ≤ 14s.
- ORE-15672: was 29.55s; target ≤ 15s.

Also note `# label heuristic: pruned=X pass_through=Y misses=Z` — compute prune rate `X / (X+Y+Z)`. Target ≥ 50%.

- [ ] **Step 3: pizza + GALEN regression check**

```bash
echo "=== pizza.ofn ==="
/usr/bin/time -f "wall=%e s" timeout 300 ./target/release/rustdl classify \
    --pair-timeout-ms 200 ontologies/real/pizza.ofn 2>&1 | \
    grep -E "# classes|# fragment|# subsumption|# label heuristic|wall="
```

Pizza baseline: 4.39s. Target: ≤ 2s (or no worse than baseline).

For GALEN, skip the full classify-diff (~13min). Instead run the
unmodified standalone `anon349_diagnostic` (it runs full GALEN classify)
to confirm wall isn't catastrophically regressed:

```bash
timeout 1200 cargo test -p owl-dl-reasoner --test anon349_diagnostic \
    --release -- --ignored --nocapture 2>&1 | \
    grep -E "classify wall|test result"
```

Expected: wall within ±10% of post-Phase-6 baseline 684s (i.e., ≤752s).
If GALEN regresses >10%, the per-class cache build cost is outweighing
non-Horn benefit on Horn workloads — flag for Task 9's results doc as
an open issue requiring workload-adaptive dispatch (skip cache for
fragment=Horn). Don't revert yet; the ORE wins may justify the GALEN
cost.

- [ ] **Step 4: Sanity-check the small wins**

```bash
for f in ontologies/external/alehif-test.ofn ontologies/real/sulo-stripped.ofn \
         ontologies/real/ro-stripped.ofn ontologies/real/sio-fp2-module.ofn; do
    echo "=== $(basename $f) ==="
    /usr/bin/time -f "wall=%e s" timeout 60 ./target/release/rustdl classify \
        --pair-timeout-ms 200 $f 2>&1 | \
        grep -E "# classes|# label heuristic|wall="
done
```

These should all classify ≤2 s with the new heuristic; minor regression
is acceptable (small ontologies pay the cache-build cost without
proportional savings).

- [ ] **Step 5: Triage**

Decision tree:
- All gates met (Phase 0 FP=0; ORE ≥2× speedup; GALEN ±10%): proceed to Task 9, ship.
- FP > 0 anywhere: REVERT Tasks 5-7 immediately.
- ORE wall unchanged AND prune rate < 30%: the heuristic isn't helpful for SROIQ workloads in practice. Pivot to investigate why (perhaps the wedge's labels are sparse on these ontologies). Document in Task 9 as a dead-end-shape; consider partial revert.
- GALEN wall regression >20%: workload-adaptive dispatch needed. Add `fragment == Horn` short-circuit in Task 5's cache builder (skip building cache when Phase 4c says PureEl/Horn). Re-measure.

No commits in Task 8; Task 9 captures.

---

## Task 9: Results doc + envelope updates

**Files:**
- Create: `docs/phase7-results.md`.
- Modify: `CLAUDE.md` — append a Phase 7 paragraph to the `crates/owl-dl-reasoner` bullet.
- Modify: `docs/superpowers/specs/2026-05-31-soundness-completeness-perf-design.md` — append Phase 7 close-out.

- [ ] **Step 1: Write `docs/phase7-results.md`**

Mirror the Phase 6 results doc shape. Fill in actual numbers from Task 8.

```markdown
# Phase 7 — per-class label heuristic results

Run 2026-06-0N. Per-class wedge satisfiability builds a label cache;
orchestrator skips `subsumes_via_tableau` when D ∉ labels(C) (sound
counterexample-model) and verifies when D ∈ labels(C) (existing
per-pair contract). See
`docs/superpowers/specs/2026-06-02-per-class-label-heuristic-design.md`
for the design and `docs/superpowers/plans/2026-06-02-per-class-label-heuristic.md`
for the implementation plan.

## Headline

<one paragraph: ORE wall delta, GALEN wall delta, FP=0 status, prune rate>

## Soundness gate (Phase 0 net)

<table: alehif / ORE-10908 / ORE-15672 FP/MISSED unchanged>

## ORE / SROIQ wall delta

| Fixture | Pre-Phase-7 wall | Post-Phase-7 wall | Δ | Prune rate |
|---|---|---|---|---|
| ORE-10908-sroiq | 27.37 s | <X> s | <Y>% | <Z>% |
| ORE-15672-shoin | 29.55 s | <X> s | <Y>% | <Z>% |
| pizza | 4.39 s | <X> s | <Y>% | <Z>% |

## GALEN regression check

<wall comparison vs post-Phase-6 684s baseline; flag if >±10%>

## Konclude comparison update

<updated row of the head-to-head table reflecting the new ORE wall>

## What this DOES / DOESN'T change

<what shipped vs what's still queued; whether the Konclude-class
≤5× ratio was achieved or remains an aspiration>

## Cross-references

- Design: `docs/superpowers/specs/2026-06-02-per-class-label-heuristic-design.md`
- Plan: `docs/superpowers/plans/2026-06-02-per-class-label-heuristic.md`
- Head-to-head baseline: `docs/perf-2026-06-02-konclude-vs-rustdl.md`
- Phase 6 (prior perf win): `docs/phase6-results.md`
```

- [ ] **Step 2: Update CLAUDE.md saturator/reasoner bullet**

Append a Phase 7 paragraph to the `crates/owl-dl-reasoner` bullet
mirroring the Phase 4c/Phase 6 entries.

- [ ] **Step 3: Update design spec**

Append after the most-recent prior-phase paragraph in
`docs/superpowers/specs/2026-05-31-soundness-completeness-perf-design.md`:

```
Phase 7 landed: `docs/phase7-results.md`. Per-class label heuristic
(HermiT-style) cut ORE-10908 wall from 27 s to <X> s (<Y>× speedup);
ORE-15672 from 30 s to <X> s. FP=0 + MISSED=0 unchanged across the
Phase 0 net. The Konclude gap on SROIQ workloads closed from 17× to
<Z>×.
```

- [ ] **Step 4: Single docs commit**

```bash
git add docs/phase7-results.md \
        CLAUDE.md \
        docs/superpowers/specs/2026-05-31-soundness-completeness-perf-design.md
git commit -m "docs(phase7): results doc + envelope updates

<one-paragraph summary of headline numbers + whether Konclude-class
≤5× ratio was met>"
```

---

## Definition of done

- Structural canary `label_heuristic_prunes_disjoint_pairs` passes.
- All reasoner + tableau tests pass; CI strict clean.
- Phase 0 net FP=0 / MISSED=0 unchanged.
- ORE-10908 wall reduced ≥2× (target: ≤14 s).
- ORE-15672 wall reduced ≥2× (target: ≤15 s).
- pizza wall reduced (target: ≤2 s).
- GALEN wall within ±10% of post-Phase-6 baseline (684 s ± 68 s).
- Label-cache prune rate ≥ 50% on at least one SROIQ workload.
- Results doc + CLAUDE.md + design-spec updates committed.

## What this plan does NOT do

- Does NOT change `trust_sat` behaviour.
- Does NOT add a `Cell`-style cache invalidation (the cache is per-classify-run, one-shot).
- Does NOT touch the saturator or wedge core algorithms.
- Does NOT add multi-model enumeration (the first-found model's labels are the heuristic; insufficient prune rate on a workload means the heuristic doesn't fit, not that we should chase more models).
- Does NOT implement workload-adaptive dispatch (skip cache for fragment=Horn) UNLESS Task 8 measurements show GALEN regression > 20% — in which case a Task 5 amendment adds the `fragment == Horn` short-circuit.
