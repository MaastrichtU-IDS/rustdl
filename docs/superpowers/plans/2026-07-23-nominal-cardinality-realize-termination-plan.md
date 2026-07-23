# Nominal + Number-Restriction Realize-Termination Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `realize` / `materialize_inferred_class_assertions` terminate (matching HermiT) on the issue-#35 `ObjectMinCardinality` + `ObjectOneOf` + property-domain pattern, via TBox-aware nominals-first scheduling, with a deterministic node cap that degrades to a sound MISS (never a hang or error).

**Architecture:** Two independent changes in the **main tableau** (`owl-dl-tableau`), which realize's `decide`/`decide_with_deadline` path uses (confirmed: `crates/owl-dl-reasoner/src/lib.rs:5296/5302` run `owl_dl_tableau::search`, not the wedge). (A) A **TBox-aware** generation guard defers `apply_exists`/`apply_min` while a node has a *pending* nominal-covering disjunction (materialized `Or`-with-`Nominal`, OR an atomic label whose absorbed concept-rule conclusion is such an `Or` not yet satisfied), and the search driver resolves those disjunctions first — so every `A`-node merges into one of the ≤k canonical nominal nodes (existing `apply_nominal_assignment`) before it can generate. (B) A deterministic live-node cap returns a distinct `NodeCap` verdict that `decide` maps to `Ok(None)` — the same sound under-approximation a spent deadline yields.

**Tech Stack:** Rust 1.88 / edition 2024; crates `owl-dl-tableau` (rules/search/saturate/lib), `owl-dl-reasoner` (decide mapping + canaries). Build with `RUSTUP_TOOLCHAIN=stable cargo …`.

## Global Constraints

- Build/test with `RUSTUP_TOOLCHAIN=stable cargo …`; confirm `target/release/rustdl` is freshly built before any benchmark (stale-binary gotcha).
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` clean (pedantic on; CI sets `RUSTFLAGS: -D warnings`). `unwrap_used`/`dbg_macro` are warn-level — avoid.
- `cargo fmt --all -- --check` must pass (max_width 100).
- **Soundness is non-negotiable:** FP=0 on every corpus ontology. Every engine change is gated by the full-corpus byte-identical-closure bake-off (Task 7).
- New env flags: `RUSTDL_NOMINAL_FIRST` (fix A, default ON, `=0` reverts), `RUSTDL_MAX_NODES` (safety net B, default 50000, `=0` disables). Read once and cache (`OnceLock`), mirroring `anywhere_blocking_enabled()` (`lib.rs:2063`) — never read env in the per-node hot loop.
- All concepts passed to `add_label` must be NNF (debug-asserted).
- **Verified real APIs to use** (the first draft fabricated several — do not reintroduce): `pool.atomic(ClassId)` / `pool.nominal(IndividualId)` / `pool.or(Vec<ConceptId>)` / `pool.min(n, Role, ConceptId)` (`ir.rs:322/327/…`); `AbsorbedTBox::default()` (NOT `::empty()`); `RoleHierarchy` via its builder (NOT `::empty()`); `TableauContext::with_tbox_and_hierarchy`; `ctx.tbox() -> Option<&AbsorbedTBox>`; `AbsorbedTBox::concept_rules_by_trigger: HashMap<ClassId, Vec<ConceptId>>` + linear `concept_rules: Vec<ConceptRule>`; reasoner `parse(&format!("{HEADER}…"))` + `realize(&onto)` + `Realization::{entailed_types, most_specific_types}(iri) -> &[String]` (`realize.rs:434/442/464`, pattern per `realize.rs:1239`).
- Commit after every green step.

---

### Task 1: Safety net B — deterministic node cap → `NodeCap` verdict → `Ok(None)`

Ship B first so every later termination test is hang-proof AND error-proof.

**Files:**
- Modify: `crates/owl-dl-tableau/src/lib.rs` (cached `max_nodes()`; `TableauContext::node_cap_exceeded`)
- Modify: `crates/owl-dl-tableau/src/saturate.rs` (`SaturationResult::NodeCapped`; check cap in the `step!` macro + sweep)
- Modify: `crates/owl-dl-tableau/src/search.rs` (`SearchVerdict::NodeCap`; propagate)
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (`decide` maps `NodeCap => Ok(None)`; make the two `expect("no deadline ⇒ Some")` at `:4572`/`:4605` graceful → `LabelOracle::NoVerdict`)
- Test: `crates/owl-dl-reasoner/tests/node_cap.rs`

**Interfaces:**
- Produces: `owl_dl_tableau::max_nodes() -> Option<usize>`; `TableauContext::node_cap_exceeded(&self) -> bool`; `SaturationResult::NodeCapped`; `SearchVerdict::NodeCap`.
- Consumes: existing `ctx.graph().len()`, `decide`'s `DepthLimit` arm (`lib.rs:5312-5313`).

- [ ] **Step 1: Write the failing test**

`crates/owl-dl-reasoner/tests/node_cap.rs` — a nominal-free unbounded generator so the test isolates B (not A). `⊤ ⊑ ∃r.C`, `C ⊑ ∃r.C`, blocking off, low cap; assert `realize`/`is_consistent` returns `Ok` (sound MISS/consistent), never `Err`, never hang:

```rust
const HEADER: &str = "Prefix(:=<http://ex/#>)\nOntology(";
#[test]
fn node_cap_degrades_to_ok_not_error() {
    // SAFETY: single-threaded test.
    unsafe { std::env::set_var("RUSTDL_MAX_NODES", "300"); }
    unsafe { std::env::set_var("RUSTDL_ANYWHERE_BLOCKING", "0"); } // force the blowup
    let onto = owl_dl_reasoner::parse(&format!(
        "{HEADER}\n\
         Declaration(Class(:C)) Declaration(ObjectProperty(:r))\n\
         Declaration(NamedIndividual(:a))\n\
         SubClassOf(owl:Thing ObjectSomeValuesFrom(:r :C))\n\
         ClassAssertion(:C :a)\n)"
    )).expect("parse");
    let r = owl_dl_reasoner::realize(&onto);
    assert!(r.is_ok(), "cap trip must be Ok(sound MISS), got {r:?}");
    unsafe { std::env::remove_var("RUSTDL_MAX_NODES"); }
    unsafe { std::env::remove_var("RUSTDL_ANYWHERE_BLOCKING"); }
}
```

Align the ontology so it genuinely grows unbounded with blocking off — the
implementer confirms the shape blows past 300 nodes pre-fix. The **contract**:
cap trip → `Ok`, not `Err(NoVerdict)`, not panic, not hang.

- [ ] **Step 2: Run test to verify it fails**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner node_cap_degrades_to_ok_not_error`
Expected: FAIL — either hang (no cap) or `Err(NoVerdict)`/panic (cap→DepthLimit→Err, per `lib.rs:5313`).

- [ ] **Step 3: Add cached accessor + context helper (`lib.rs`)**

Near `anywhere_blocking_enabled()` (`lib.rs:2063`):

```rust
/// Deterministic live-node cap for the deadline-free saturate/search path.
/// `RUSTDL_MAX_NODES` (default 50000; `0` disables). Cached once (#35 v4).
#[must_use]
pub fn max_nodes() -> Option<usize> {
    use std::sync::OnceLock;
    static CAP: OnceLock<Option<usize>> = OnceLock::new();
    *CAP.get_or_init(|| match std::env::var("RUSTDL_MAX_NODES") {
        Ok(v) => match v.trim().parse::<usize>() {
            Ok(0) => None,
            Ok(n) => Some(n),
            Err(_) => Some(50_000),
        },
        Err(_) => Some(50_000),
    })
}
```

In `impl TableauContext` (near `check_deadline`):

```rust
/// True once live node count exceeds [`crate::max_nodes`]. Callers treat a
/// resulting NodeCap verdict as a clean "no verdict" (sound under-approx).
#[must_use]
pub fn node_cap_exceeded(&self) -> bool {
    crate::max_nodes().is_some_and(|cap| self.graph().len() > cap)
}
```

- [ ] **Step 4: Add `NodeCapped`/`NodeCap` variants + enforce in saturate**

In `saturate.rs`, add `NodeCapped` to `SaturationResult` (doc: "live node cap hit; sound no-verdict"). Extend the `step!` macro guard (lines ~109-118) and the sweep deadline check (~151):

```rust
macro_rules! step {
    ($apply:expr) => {{
        if ctx.node_cap_exceeded() {
            return SaturationResult::NodeCapped;
        }
        if ctx.check_deadline() {
            return SaturationResult::Stalled;
        }
        if $apply == RuleOutcome::Applied {
            changed = true;
        }
    }};
}
```

In `search.rs`, add `NodeCap` to `SearchVerdict`. Then handle it at **every**
`SearchVerdict` match site (no `_` wildcards exist, so each is a forced compile
error — good; do NOT paper over any with a wildcard):

1. **`search` (`search.rs:103`, `match saturate(...)`):** map
   `SaturationResult::NodeCapped => SearchVerdict::NodeCap`. Keep `Stalled`
   (deadline) → `DepthLimit` unchanged.
2. **`branch` (`search.rs:238-294`) — CRITICAL, do NOT treat like `DepthLimit`.**
   `DepthLimit` here is *soft*: it sets `depth_limited = true` and the final
   fallback returns `SearchVerdict::DepthLimit`. If `NodeCap` collapses into that
   path, a cap-tripped branch returns `DepthLimit`, which `decide` maps on the
   deadline-free path to `Err(NoVerdict)` — re-creating exactly the bug B fixes.
   Add a **distinct** `node_capped` flag mirroring `depth_limited`:
   ```rust
   // in the inner `match search(ctx, max_depth - 1)`:
   SearchVerdict::NodeCap => {
       ctx.rollback_to(cp);
       node_capped = true;
   }
   ```
   and in the final return, check it FIRST (a cap trip is a hard no-verdict, it
   should not be masked by a sibling's soft depth-limit):
   ```rust
   if let Some(v) = early_return {
       v
   } else if node_capped {
       SearchVerdict::NodeCap
   } else if depth_limited {
       SearchVerdict::DepthLimit
   } else {
       SearchVerdict::Unsat(combined)
   }
   ```
   Declare `let mut node_capped = false;` next to `depth_limited`.
3. **`to_option` (`search.rs:67-78`):** add `Self::NodeCap => None` (don't-know,
   same as `DepthLimit`).

- [ ] **Step 5: Map `NodeCap => Ok(None)` in `decide` + fix the expect sites**

In `crates/owl-dl-reasoner/src/lib.rs`, in the `decide` verdict match (near
`:5312-5313`) add, BEFORE the `DepthLimit` arms:

```rust
        owl_dl_tableau::SearchVerdict::NodeCap => Ok(None),
```

At `lib.rs:4572` and `:4605`, replace `.expect("no deadline ⇒ search always returns Some(_)")` with a graceful map (a cap trip can now legitimately yield `None`):

```rust
        .map(|opt| opt.map_or(LabelOracle::NoVerdict, |v| /* existing Some-handling */ ))
```

Adjust to the exact closure body at each site; the contract is: `None` → `LabelOracle::NoVerdict`, never a panic.

- [ ] **Step 6: Run test to verify it passes**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner node_cap_degrades_to_ok_not_error`
Expected: PASS (`Ok`).

- [ ] **Step 7: fmt + clippy (workspace, both crates) + commit**

```bash
RUSTUP_TOOLCHAIN=stable cargo fmt --all
RUSTUP_TOOLCHAIN=stable cargo clippy -p owl-dl-tableau -p owl-dl-reasoner --all-targets -- -D warnings
git add crates/owl-dl-tableau crates/owl-dl-reasoner
git commit -m "feat: RUSTDL_MAX_NODES cap -> NodeCap verdict -> Ok(None) sound MISS (#35 safety net)"
```

---

### Task 2: TBox-aware `has_pending_nominal_disjunction` + `nominal_first_enabled`

**Files:**
- Modify: `crates/owl-dl-tableau/src/lib.rs` (cached `nominal_first_enabled()`; predicate on `TableauContext`)
- Test: `crates/owl-dl-tableau/src/lib.rs` `#[cfg(test)]` module

**Interfaces:**
- Produces: `owl_dl_tableau::nominal_first_enabled() -> bool`; `TableauContext::has_pending_nominal_disjunction(&self, NodeId) -> bool`.
- Consumes: `ctx.pool()`, `ctx.tbox()`, `graph.node(node).labels()`, `ConceptExpr::{Atomic, Or, Nominal}`, `AbsorbedTBox::{concept_rules_by_trigger, concept_rules}`.

- [ ] **Step 1: Write the failing test**

Two cases: the **pending** (TBox concept-rule) case — the real bug shape — and the **materialized-`Or`** case.

```rust
#[test]
fn pending_nominal_disjunction_tbox_and_materialized() {
    use owl_dl_core::ir::{ClassId, IndividualId};
    let mut pool = owl_dl_core::ConceptPool::new();
    let a = pool.atomic(ClassId::new(0));
    let x = pool.nominal(IndividualId::new(0));
    let y = pool.nominal(IndividualId::new(1));
    let one_of = pool.or(vec![x, y]);
    // TBox: A ⊑ {x,y}  as a concept-rule (trigger A, conclusion one_of).
    let mut tbox = owl_dl_core::AbsorbedTBox::default();
    tbox.concept_rules.push(owl_dl_core::ConceptRule { trigger: ClassId::new(0), conclusion: one_of });
    tbox.finalize(); // builds concept_rules_by_trigger (absorb.rs:110)
    let hier = /* empty RoleHierarchy via its builder */;
    let mut ctx = TableauContext::with_tbox_and_hierarchy(&pool, &tbox, &hier);

    // Pending case: node has only atomic A -> Or not materialized -> still pending.
    let n = ctx.new_node();
    ctx.add_label(n, a);
    assert!(ctx.has_pending_nominal_disjunction(n), "pending via TBox concept-rule");

    // Materialized-open case.
    let n2 = ctx.new_node();
    ctx.add_label(n2, one_of);
    assert!(ctx.has_pending_nominal_disjunction(n2), "materialized open Or");

    // Resolved: a disjunct present -> not pending.
    ctx.add_label(n2, x);
    assert!(!ctx.has_pending_nominal_disjunction(n2), "resolved");
}
```

Align `ConceptRule` field names, `finalize`, and the empty-`RoleHierarchy`
construction to the real APIs (`absorb.rs:145` `ConceptRule`, `absorb.rs:110`
finalize, `role_hierarchy.rs` builder). The **contract** is the three assertions.

- [ ] **Step 2: Run test to verify it fails**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-tableau pending_nominal_disjunction_tbox_and_materialized`
Expected: FAIL — `has_pending_nominal_disjunction`/`nominal_first_enabled` undefined.

- [ ] **Step 3: Implement flag + predicate**

Near `max_nodes()`:

```rust
/// Nominals-first scheduling (#35 v4). `RUSTDL_NOMINAL_FIRST` default ON; `=0`
/// reverts to unconditional ∃/≥ generation. Cached once.
#[must_use]
pub fn nominal_first_enabled() -> bool {
    use std::sync::OnceLock;
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| !matches!(std::env::var("RUSTDL_NOMINAL_FIRST").as_deref(), Ok("0")))
}
```

In `impl TableauContext` — helper to test one `Or` conclusion, then the predicate:

```rust
/// True iff `or_id` is `Or(args)` with a `Nominal(_)` disjunct and no disjunct
/// already present in `labels` (sorted).
fn is_open_nominal_or(&self, or_id: ConceptId, labels: &[ConceptId]) -> bool {
    if let owl_dl_core::ConceptExpr::Or(args) = self.pool().get(or_id) {
        let open = !args.iter().any(|d| labels.binary_search(d).is_ok());
        open && args.iter().any(|&d|
            matches!(self.pool().get(d), owl_dl_core::ConceptExpr::Nominal(_)))
    } else {
        false
    }
}

/// True iff `node` carries a pending nominal-covering disjunction: either a
/// materialized open `Or`-with-`Nominal`, OR an `Atomic(class)` whose absorbed
/// concept-rule conclusion is such an `Or` not yet satisfied. The TBox branch is
/// what fires on the #35 bug — the nominal `Or` is a deferred concept-rule
/// conclusion, unmaterialized at generation time (rules.rs:204/217; saturate.rs:158).
#[must_use]
pub fn has_pending_nominal_disjunction(&self, node: NodeId) -> bool {
    let labels: Vec<ConceptId> = self.graph().node(node).labels().to_vec();
    // (1) materialized open Or on the node.
    if labels.iter().any(|&c| self.is_open_nominal_or(c, &labels)) {
        return true;
    }
    // (2) pending via TBox concept-rule keyed by an atomic label's class.
    let Some(tbox) = self.tbox() else { return false; };
    for &c in &labels {
        if let owl_dl_core::ConceptExpr::Atomic(class) = self.pool().get(c) {
            if let Some(concls) = tbox.concept_rules_by_trigger.get(class) {
                if concls.iter().any(|&o| self.is_open_nominal_or(o, &labels)) {
                    return true;
                }
            } else {
                // linear fallback when the index is empty
                if tbox.concept_rules.iter().any(|r|
                    r.trigger == *class && self.is_open_nominal_or(r.conclusion, &labels))
                {
                    return true;
                }
            }
        }
    }
    false
}
```

Confirm the exact `ConceptExpr::Atomic` shape and `concept_rules_by_trigger` key type (`ClassId`) against `ir.rs`/`absorb.rs`; adjust the pattern binding as needed.

- [ ] **Step 4: Run test to verify it passes**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-tableau pending_nominal_disjunction_tbox_and_materialized`
Expected: PASS.

- [ ] **Step 5: fmt + clippy + commit**

```bash
RUSTUP_TOOLCHAIN=stable cargo fmt --all
RUSTUP_TOOLCHAIN=stable cargo clippy -p owl-dl-tableau --all-targets -- -D warnings
git add crates/owl-dl-tableau
git commit -m "feat(tableau): TBox-aware has_pending_nominal_disjunction + RUSTDL_NOMINAL_FIRST (#35)"
```

---

### Task 3: Generation guard in `apply_exists` / `apply_min`

**Files:**
- Modify: `crates/owl-dl-tableau/src/rules.rs` (`apply_exists` ~711, `apply_min` ~786)
- Test: covered by the real-driver gate in Task 5 (a direct-call unit test is misleading here — it cannot reproduce the materialization ordering, per advisor concern 1). Add only a minimal direct assertion that the guard returns `NoChange` when the predicate holds.

**Interfaces:**
- Consumes: `ctx.has_pending_nominal_disjunction(node)`, `crate::nominal_first_enabled()`, `RuleOutcome::NoChange`.

- [ ] **Step 1: Write the minimal failing test**

In `rules.rs` `#[cfg(test)]`: pre-seed a node with `≥2 r.C` AND atomic `A` where the TBox has `A ⊑ {x,y}`; assert `apply_min` returns `NoChange` and adds no successor. (This validates the guard mechanically; Task 5 validates it end-to-end.)

```rust
#[test]
fn min_defers_under_pending_nominal_tbox() {
    // build pool + tbox with concept-rule A ⊑ Or([{x},{y}]); node with A and ≥2 r.C
    // (reuse the Task 2 construction helpers)
    unsafe { std::env::set_var("RUSTDL_NOMINAL_FIRST", "1"); }
    let before = ctx.graph().len();
    let out = apply_min(&mut ctx, n);
    assert_eq!(out, RuleOutcome::NoChange);
    assert_eq!(ctx.graph().len(), before, "no successor generated");
    unsafe { std::env::remove_var("RUSTDL_NOMINAL_FIRST"); }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-tableau min_defers_under_pending_nominal_tbox`
Expected: FAIL — `apply_min` generates (`Applied`, node count grows).

- [ ] **Step 3: Add the guard to both generators**

In `apply_exists` and `apply_min`, immediately after the existing
`if ctx.is_blocked(node) { return RuleOutcome::NoChange; }`:

```rust
    // Nominals-first (#35 v4): defer generation while the node still carries a
    // pending nominal-covering disjunction (materialized OR TBox concept-rule).
    // The search driver resolves it first (search::first_open_disjunction
    // priority), apply_nominal_assignment merges this node into its canonical
    // nominal, and generation then fires once on the bounded canonical node.
    // Deferral only — the node is re-dirtied on add_label/merge, so generation
    // resumes on the survivor (completeness-preserving).
    if crate::nominal_first_enabled() && ctx.has_pending_nominal_disjunction(node) {
        return RuleOutcome::NoChange;
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-tableau min_defers_under_pending_nominal_tbox`
Expected: PASS.

- [ ] **Step 5: fmt + clippy + commit**

```bash
RUSTUP_TOOLCHAIN=stable cargo fmt --all
RUSTUP_TOOLCHAIN=stable cargo clippy -p owl-dl-tableau --all-targets -- -D warnings
git add crates/owl-dl-tableau
git commit -m "feat(tableau): defer ∃/≥ generation under pending nominal disjunction (#35)"
```

---

### Task 4: Search-driver nominal-disjunction priority

**Files:**
- Modify: `crates/owl-dl-tableau/src/search.rs:427` (`first_open_disjunction`)
- Test: `crates/owl-dl-tableau/src/search.rs` `#[cfg(test)]`

**Interfaces:**
- Consumes: `crate::nominal_first_enabled()`, `ConceptExpr::{Or, Nominal}`.
- Produces: `first_open_disjunction` returns a nominal-bearing open `Or` first when one exists (flag ON); identical to today otherwise. Return tuple `(NodeId, ConceptId, Vec<ConceptId>, DepSet)` unchanged.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn first_open_disjunction_prefers_nominal_bearing() {
    unsafe { std::env::set_var("RUSTDL_NOMINAL_FIRST", "1"); }
    // node with TWO open Ors: plain Or([P,Q]) added FIRST, Or([{x},{y}]) SECOND.
    // (build via Task 2 helpers)
    let (_, chosen, _, _) = super::first_open_disjunction(&ctx).expect("an open Or");
    assert!(matches!(ctx.pool().get(chosen), owl_dl_core::ConceptExpr::Or(args)
        if args.iter().any(|&d| matches!(ctx.pool().get(d), owl_dl_core::ConceptExpr::Nominal(_)))),
        "nominal-bearing Or must win despite being second");
    unsafe { std::env::remove_var("RUSTDL_NOMINAL_FIRST"); }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-tableau first_open_disjunction_prefers_nominal_bearing`
Expected: FAIL — returns the plain `Or` (first in iteration order).

- [ ] **Step 3: Add the priority pass**

Rewrite `first_open_disjunction` — single traversal, prefer a nominal-bearing open `Or` when `nominal_first_enabled()`, else the existing first-open. Preserve the doc-comment about returning the parent `Or` label id + `DepSet` (back-jump soundness):

```rust
fn first_open_disjunction(
    ctx: &TableauContext<'_, '_, '_>,
) -> Option<(NodeId, ConceptId, Vec<ConceptId>, DepSet)> {
    let pool = ctx.pool();
    let graph = ctx.graph();
    let prefer_nominal = crate::nominal_first_enabled();
    let mut first_any: Option<(NodeId, ConceptId, Vec<ConceptId>, DepSet)> = None;
    for idx in 0..graph.len() {
        let node_id = NodeId::new(u32::try_from(idx).expect("node count exceeds u32"));
        let node = graph.node(node_id);
        let labels = node.labels();
        for (pos, &c) in labels.iter().enumerate() {
            if let ConceptExpr::Or(args) = pool.get(c)
                && !args.iter().any(|d| labels.binary_search(d).is_ok())
            {
                let hit = (node_id, c, args.to_vec(), node.label_deps[pos].clone());
                let has_nominal = prefer_nominal
                    && args.iter().any(|&d| matches!(pool.get(d), ConceptExpr::Nominal(_)));
                if has_nominal {
                    return Some(hit);
                }
                if first_any.is_none() {
                    first_any = Some(hit);
                }
            }
        }
    }
    first_any
}
```

- [ ] **Step 4: Run test + full tableau regression suite**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-tableau`
Expected: new test PASS; all existing search/backjump/precise-card-deps tests PASS (order-only change; `branch_id`/`DepSet` bookkeeping unchanged — advisor confirmed sound).

- [ ] **Step 5: fmt + clippy + commit**

```bash
RUSTUP_TOOLCHAIN=stable cargo fmt --all
RUSTUP_TOOLCHAIN=stable cargo clippy -p owl-dl-tableau --all-targets -- -D warnings
git add crates/owl-dl-tableau
git commit -m "feat(tableau): prioritise nominal-bearing disjunctions in search (#35)"
```

---

### Task 5: Real-driver termination gate + reproducer realize canaries

This is the acceptance criterion for A (advisor concern 2). **The load-bearing gate is a cap-DISABLED, tableau-layer node-count assertion** — because a realize-level "does not trip a low cap" check is *cap-invariant*: a divergent run that trips the cap returns `NodeCap → Ok(None) →` "not an instance", which is the *same* answer as a genuinely-bounded run, so it cannot distinguish bounded from capped-divergent. The graph must be proven small with the cap OFF.

**Files:**
- Test: `crates/owl-dl-tableau/tests/nominal_first_bounded.rs` (load-bearing gate)
- Test: `crates/owl-dl-reasoner/tests/nominal_cardinality_realize.rs` (realize smoke + correctness)

**Interfaces:**
- Consumes: `owl_dl_tableau::{search, SearchVerdict, TableauContext}`, `owl-dl-core` convert+absorb to build the reproducer's `AbsorbedTBox`; `owl_dl_reasoner::{parse, realize}`, `Realization::entailed_types`.

- [ ] **Step 1 (LOAD-BEARING): tableau-layer bounded-node gate, cap disabled**

Build the reproducer's completion probe directly and assert the graph stays tiny with the cap OFF and the fix ON — this proves boundedness, not "capped".

```rust
// RUSTDL_MAX_NODES=0 (cap disabled) + RUSTDL_NOMINAL_FIRST=1: the fix alone must
// bound the graph. Build the {a}⊓¬C-style probe from the reproducer's absorbed
// TBox (via owl-dl-core convert+absorb over the OFN string), run the real search,
// assert graph().len() < 64 AND verdict is Sat/Unsat (NOT NodeCap/DepthLimit).
#[test]
fn issue35_v4_completion_graph_is_bounded() {
    unsafe { std::env::set_var("RUSTDL_MAX_NODES", "0"); }       // cap OFF
    unsafe { std::env::set_var("RUSTDL_NOMINAL_FIRST", "1"); }   // fix ON
    let (pool, tbox, hier, /* abox/probe seed */ ..) = build_reproducer_probe(); // helper
    let mut ctx = TableauContext::with_tbox_and_hierarchy(&pool, &tbox, &hier);
    // seed the probe node(s) as the reasoner's decide() does for a consistency check
    let verdict = owl_dl_tableau::search(&mut ctx, 1_000_000);
    assert!(matches!(verdict, SearchVerdict::Sat | SearchVerdict::Unsat(_)),
        "must decide, not stall: {verdict:?}");
    assert!(ctx.graph().len() < 64, "graph must stay bounded, got {}", ctx.graph().len());
    unsafe { std::env::remove_var("RUSTDL_MAX_NODES"); }
    unsafe { std::env::remove_var("RUSTDL_NOMINAL_FIRST"); }
}
```

`build_reproducer_probe` constructs the reproducer TBox exactly as the reasoner's
`PreparedOntology`/`decide` does — the implementer mirrors the seed the deadline-free
consistency probe uses (see `crates/owl-dl-reasoner/src/lib.rs` around `:5296-5302`
and `PreparedOntology::from_internal`). If wiring the seed at the tableau layer is
impractical, put this test in `crates/owl-dl-reasoner/tests/` where `convert_ontology`
+ the internal decide are reachable, and expose peak `graph().len()` via a
test-only hook or the counters feature — but the assertion (`len < 64` with cap OFF)
is mandatory and must not be replaced by a realize-level cap check.

- [ ] **Step 2: Run cap-disabled gate — fails pre-fix (real termination check)**

Run: `RUSTUP_TOOLCHAIN=stable RUSTDL_NOMINAL_FIRST=0 RUSTDL_MAX_NODES=0 cargo test -p owl-dl-tableau issue35_v4_completion_graph_is_bounded`
Expected: with the fix OFF and cap OFF, this **hangs / blows the graph** (that is the bug). Run it under a shell `timeout` to confirm non-termination, then move on — do NOT leave a hanging test enabled for the OFF case; the committed test runs only the ON case.

- [ ] **Step 3: Run cap-disabled gate — passes with the fix**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-tableau issue35_v4_completion_graph_is_bounded`
Expected: PASS — `graph().len() < 64`, verdict `Sat`/`Unsat`.

- [ ] **Step 3b: realize smoke + correctness test (secondary)**

```rust
const HEADER: &str = "Prefix(:=<http://example.org/card#>)\nOntology(<http://example.org/card>\n";
const CORE: &str = "\
  Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))\n\
  Declaration(ObjectProperty(:r))\n\
  Declaration(NamedIndividual(:x)) Declaration(NamedIndividual(:y)) Declaration(NamedIndividual(:z))\n\
  SubClassOf(:A ObjectOneOf(:x :y :z))\n\
  EquivalentClasses(:B ObjectIntersectionOf(:A ObjectMinCardinality(2 :r :C)))\n\
  ObjectPropertyDomain(:r :A)\n)";

#[test]
fn issue35_v4_realize_smoke_and_correct() {
    // SMOKE + CORRECTNESS (secondary to Step 1's bounded-node gate). This proves
    // realize returns cleanly (Ok, no hang/error) and reports the HermiT verdict.
    // It does NOT prove boundedness — a divergent-but-capped run returns the same
    // "x,y,z not in B/C" answer (NodeCap -> Ok(None) -> not-an-instance). Step 1
    // (cap OFF, graph().len() < 64) is the termination gate; this is the verdict check.
    let onto = owl_dl_reasoner::parse(&format!("{HEADER}{CORE}")).expect("parse");
    let r = owl_dl_reasoner::realize(&onto).expect("realize returns (no hang, no error)");
    for ind in ["http://example.org/card#x","http://example.org/card#y","http://example.org/card#z"] {
        let types = r.entailed_types(ind);
        assert!(!types.iter().any(|t| t.ends_with("#B")), "{ind} must not be B");
        assert!(!types.iter().any(|t| t.ends_with("#C")), "{ind} must not be C");
    }
}
```

- [ ] **Step 3c: Run the smoke test with the fix on (default)**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner issue35_v4_realize_smoke_and_correct`
Expected: PASS.

- [ ] **Step 3d: Positive-entailment canary (makes a divergence observable)**

Add a crafted fixture where the correct answer IS "instance of" (a subsumed/entailed
type), so that a divergence-capped run — which returns `Ok(None) →` not-an-instance —
gives the WRONG answer and FAILS. E.g. add a `ClassAssertion` and a defined class that
entails a named type for one individual under the same nominal+`≥n` shape; assert that
individual's `entailed_types` DOES contain the entailed class. This is the test a
cap-invariant negative assertion cannot provide. (Keep it independent of the cap: run
with `RUSTDL_MAX_NODES=0` so a divergence hangs rather than silently passes — guard the
CI run with a shell `timeout` if the shape risks non-termination pre-fix.)

- [ ] **Step 4: Add the minimality-variant sub-tests**

Two more `parse(&format!("{HEADER}…"))` cases confirming correct results:
(a) drop `ObjectPropertyDomain(:r :A)`; (b) replace `SubClassOf(:A ObjectOneOf(...))`
with `SubClassOf(:A :D)` (+ `Declaration(Class(:D))`). Guards against the guard
over-firing (must still give the same verdicts as pre-fix on these — check with
`RUSTDL_NOMINAL_FIRST=0` vs `=1`).

- [ ] **Step 5: commit**

```bash
git add crates/owl-dl-tableau/tests/nominal_first_bounded.rs \
        crates/owl-dl-reasoner/tests/nominal_cardinality_realize.rs
git commit -m "test: issue #35 v4 cap-disabled bounded-graph gate + realize correctness"
```

---

### Task 6: Full-workspace test + clippy

- [ ] **Step 1:** `RUSTUP_TOOLCHAIN=stable cargo test --workspace` — all green.
- [ ] **Step 2:** `RUSTUP_TOOLCHAIN=stable cargo clippy --workspace --all-targets --all-features -- -D warnings` — clean.
- [ ] **Step 3:** `RUSTUP_TOOLCHAIN=stable cargo fmt --all -- --check` — clean. Commit any fmt fixes.

---

### Task 7: Full-corpus bake-off gate (the soundness gate)

**Files:** Create `docs/2026-07-23-nominal-first-bakeoff-results.md`.

- [ ] **Step 1:** `RUSTUP_TOOLCHAIN=stable cargo build --workspace --release`; confirm `target/release/rustdl` timestamp is current.
- [ ] **Step 2:** `./scripts/fetch-real-ontologies.sh` if the corpus is absent.
- [ ] **Step 3:** For galen, notgalen, sio, wine, ore-10908, ore-15672, alehif, ro, pizza, bibtex: classify twice — `RUSTDL_NOMINAL_FIRST=0` vs `=1` — and diff sorted closures. Expected: **byte-identical**, FP=0, MISSED unchanged.
- [ ] **Step 4:** Walls via `./scripts/bench-rustdl-modes.sh`. Expected: no reproducible regression (±10%; watch wine's nominal cluster). Record.
- [ ] **Step 5:** Also confirm `RUSTDL_MAX_NODES` default (50k) is never approached on the corpus (log peak node counts if the counters feature is on).
- [ ] **Step 6:** Write the results doc; commit.

```bash
git add docs/2026-07-23-nominal-first-bakeoff-results.md
git commit -m "docs: nominal-first corpus bake-off — FP=0/MISSED unchanged, closures byte-identical"
```

**GATE:** any closure diff or new FP → STOP, return to Task 3/4; do not release.

---

### Task 8: Docs, CHANGELOG, issue reply

**Files:** `CHANGELOG.md`, `CLAUDE.md`, `Cargo.toml` (version bump if releasing).

- [ ] **Step 1: CHANGELOG** — #35 v4 nominal+cardinality realize hang; TBox-aware nominals-first scheduling (`RUSTDL_NOMINAL_FIRST`, default ON) + deterministic `RUSTDL_MAX_NODES` cap → sound MISS; link spec, plan, bake-off results.
- [ ] **Step 2: CLAUDE.md** — `owl-dl-tableau` section note: root cause (nominal exclusion in blocking + `≥`-generation residual-GCI cycle, generation outrunning the deferred nominal-`Or` materialization), fix (TBox-aware deferral + search priority → o-rule merge bounds the graph), safety net (`NodeCap → Ok(None)`), env flags, bake-off result.
- [ ] **Step 3: Commit.**

```bash
git add CHANGELOG.md CLAUDE.md Cargo.toml
git commit -m "docs: document nominal-first realize-termination fix (#35 v4)"
```

- [ ] **Step 4:** Draft the GitHub issue reply (do NOT post without user confirmation): fix summary + the workaround for users on released 0.3.38 — `RUSTDL_REALIZE_PAIR_TIMEOUT_MS=<ms>` bounds each per-individual probe and is honoured from Python (read from the environment).

---

## Self-Review

**Spec coverage:** §3 A.1 (TBox-aware guard) → Task 2 (predicate) + Task 3 (guard); §3 A.2 (search priority) → Task 4; §4 safety net B (`NodeCap → Ok(None)` + expect-site fix) → Task 1; §5.1 real-driver bounded gate + canaries → Task 5; §5.2 bake-off → Task 7; §8 docs → Task 8; full-suite gate → Task 6. §7 out-of-scope (nominal blocking / NN-rule / wedge) correctly unimplemented.

**Advisor concerns addressed:** (1) predicate now TBox-aware → fires at generation time (Task 2); direct-call test explicitly noted as insufficient, real-driver gate is the acceptance criterion (Task 5). (2) hard deterministic bounded-node gate through the real driver, not deferred to bake-off (Task 5 Steps 2-3). (3) `NodeCap` distinct verdict → `Ok(None)`, `expect` sites made graceful (Task 1 Steps 4-5). (4) fabricated symbols removed — real APIs enumerated in Global Constraints and used throughout. (5) scope confirmed main tableau (header). (6) `is_consistent`/realize `Ok(None)` semantics correct.

**Placeholder scan:** test bodies flag repo-specific construction (`RoleHierarchy` builder, `ConceptRule` fields, `finalize`, `build_reproducer_probe` seed, the exact `expect`-site closure) for the implementer to align to real signatures; the asserted **contract** is concrete in every test. No "TODO/handle edge cases".

**Advisor round-2 fixes applied:** concern 3 — `branch` propagates a distinct `NodeCap` (own flag + return arm), not folded into soft `DepthLimit`; `to_option` gets `NodeCap => None` (Task 1 Step 4). Concern 2 — load-bearing gate is now the cap-DISABLED tableau-layer `graph().len() < 64` + `Sat`/`Unsat` assertion (Task 5 Step 1); realize test demoted to smoke + a positive-entailment canary added (Step 3d); B's node-count-only limit documented (spec §4). Minor — `finalize_indexes` → `finalize`.

**Type consistency:** `RuleOutcome::{Applied,NoChange}`, `ConceptExpr::{Atomic,Or,Nominal}`, `SaturationResult::NodeCapped`, `SearchVerdict::NodeCap`, `max_nodes() -> Option<usize>`, `nominal_first_enabled() -> bool`, `has_pending_nominal_disjunction(&self, NodeId) -> bool`, `is_open_nominal_or(&self, ConceptId, &[ConceptId]) -> bool`, `entailed_types(iri) -> &[String]`, `first_open_disjunction -> Option<(NodeId, ConceptId, Vec<ConceptId>, DepSet)>` used consistently across tasks.

**Ordering:** B (Task 1) before the termination canaries so no regression can hang or hard-error CI; predicate (Task 2) before its consumers (Tasks 3-4); real-driver gate + bake-off last.
