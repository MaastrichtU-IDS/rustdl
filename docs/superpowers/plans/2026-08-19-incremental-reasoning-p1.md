# Incremental Reasoning P1 — Foundation + Addition-Only Session

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a working `IncrementalSession` that reuses saturation state across axiom **additions** and answers classification queries IRI-identically to a from-scratch run, falling back to a full rebuild on any deletion.

**Architecture:** Make the internal id space stable across edits (reserved headroom above the user vocabulary), add an incremental lowering path that never re-sorts, keep derived axioms correct via a recompute-and-diff overlay, and hold the EL saturation engine alive across revisions behind a new `SaturationState`. Deletion, tableau retention, and the external surfaces are P2–P4.

**Tech Stack:** Rust (edition 2024, toolchain pinned 1.95.0 but **build with `RUSTUP_TOOLCHAIN=stable`**), `horned-owl` 1.4 (pinned git rev), `fixedbitset`, `hashbrown`, rayon.

**Spec:** `docs/superpowers/specs/2026-08-18-incremental-reasoning-design.md` (v2.1)
**Supporting measurement:** `docs/2026-08-19-incremental-lowering-floor-findings.md`

## Global Constraints

- **Build/test command:** `RUSTUP_TOOLCHAIN=stable cargo test --workspace`. A bare `cargo` fails — `rust-toolchain.toml` pins 1.95.0, often installed without the `cargo` binary.
- **Lint gate:** `RUSTUP_TOOLCHAIN=stable cargo clippy --workspace --all-targets --all-features -- -D warnings`. Warnings are errors.
- **Format:** `cargo fmt --all -- --check`, `max_width = 100`.
- **Soundness rule:** a false positive (reporting a subsumption or unsat class that does not hold) is never acceptable. A missed entailment is a documented limitation. When in doubt, rebuild.
- **`slack = 0` must be byte-identical to today.** Every existing test must pass unchanged on the non-session path. This is the merge gate for the whole phase.
- **Never compare internal ids across a session boundary.** Session ids differ from from-scratch ids by construction (`convert.rs:2095`, `:2203` both sort). All gates compare IRIs.
- **P1 is addition-only.** Any delta containing a removal, or any object/data-property axiom delta, triggers a full rebuild. That is correct behaviour in P1, not a bug.

---

### Task 1: Axiom liveness (tombstoning) on `InternalOntology`

Axiom indices are load-bearing — `ProofTrace`'s provenance vectors (`crates/owl-dl-saturation/src/proof.rs:146-163`) are parallel to `axioms`, and `justify`/`repair` key on indices. Removal must therefore never shift indices.

**Files:**
- Modify: `crates/owl-dl-core/src/ontology.rs:102-118`
- Test: `crates/owl-dl-core/tests/axiom_liveness.rs` (create)

**Interfaces:**
- Consumes: nothing.
- Produces: `InternalOntology::live: FixedBitSet`, `fn live_axiom_indices(&self) -> impl Iterator<Item = usize> + '_`, `fn live_axioms(&self) -> impl Iterator<Item = (usize, &Axiom)> + '_`, `fn kill_axiom(&mut self, idx: usize) -> bool`, `fn push_live_axiom(&mut self, ax: Axiom) -> usize`, `fn num_live_axioms(&self) -> usize`.

- [ ] **Step 1: Write the failing test**

```rust
// crates/owl-dl-core/tests/axiom_liveness.rs
use owl_dl_core::ontology::InternalOntology;
use owl_dl_core::Axiom;

fn top_bot_sub(o: &mut InternalOntology) -> Axiom {
    let t = o.concepts.top();
    let b = o.concepts.bot();
    Axiom::SubClassOf { sub: b, sup: t }
}

#[test]
fn killing_an_axiom_preserves_indices_of_survivors() {
    let mut o = InternalOntology::new();
    let a0 = top_bot_sub(&mut o);
    let a1 = top_bot_sub(&mut o);
    let i0 = o.push_live_axiom(a0);
    let i1 = o.push_live_axiom(a1);
    assert_eq!((i0, i1), (0, 1));
    assert_eq!(o.num_live_axioms(), 2);

    assert!(o.kill_axiom(i0));
    // Index of the survivor is unchanged - this is the whole point.
    assert_eq!(o.live_axiom_indices().collect::<Vec<_>>(), vec![i1]);
    assert_eq!(o.num_live_axioms(), 1);
    // The dead slot is still addressable so parallel provenance vectors stay valid.
    assert_eq!(o.axioms.len(), 2);
    // Killing twice is a no-op, not a panic or a double-decrement.
    assert!(!o.kill_axiom(i0));
    assert_eq!(o.num_live_axioms(), 1);
}

#[test]
fn axioms_pushed_by_convert_are_live_by_default() {
    let mut o = InternalOntology::new();
    let a = top_bot_sub(&mut o);
    o.axioms.push(a); // legacy direct push, as convert_ontology does today
    o.sync_liveness();
    assert_eq!(o.num_live_axioms(), 1);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-core --test axiom_liveness`
Expected: FAIL — `no method named push_live_axiom`.

- [ ] **Step 3: Write minimal implementation**

```rust
// crates/owl-dl-core/src/ontology.rs — add to the struct and impl
pub struct InternalOntology {
    pub vocabulary: Vocabulary,
    pub concepts: ConceptPool,
    pub axioms: Vec<Axiom>,
    /// Bit `i` set iff `axioms[i]` is active. NEVER shrink `axioms` —
    /// `ProofTrace`'s provenance vectors and `justify`/`repair` key on
    /// these indices. Removal clears a bit; the slot stays addressable.
    pub live: fixedbitset::FixedBitSet,
}

impl InternalOntology {
    /// Bring `live` up to `axioms.len()`, marking any un-tracked tail live.
    /// Call after code paths that push straight into `axioms`.
    pub fn sync_liveness(&mut self) {
        let n = self.axioms.len();
        if self.live.len() < n {
            self.live.grow(n);
            for i in 0..n {
                self.live.insert(i);
            }
        }
    }

    pub fn push_live_axiom(&mut self, ax: Axiom) -> usize {
        let idx = self.axioms.len();
        self.axioms.push(ax);
        self.live.grow(idx + 1);
        self.live.insert(idx);
        idx
    }

    /// Returns true iff this call transitioned the axiom live -> dead.
    pub fn kill_axiom(&mut self, idx: usize) -> bool {
        if idx < self.live.len() && self.live.contains(idx) {
            self.live.set(idx, false);
            true
        } else {
            false
        }
    }

    pub fn live_axiom_indices(&self) -> impl Iterator<Item = usize> + '_ {
        self.live.ones()
    }

    pub fn live_axioms(&self) -> impl Iterator<Item = (usize, &Axiom)> + '_ {
        self.live.ones().map(move |i| (i, &self.axioms[i]))
    }

    #[must_use]
    pub fn num_live_axioms(&self) -> usize {
        self.live.count_ones(..)
    }
}
```

Then in `crates/owl-dl-core/src/convert.rs`, immediately before `Ok(out)` at the end of `convert_ontology` (after the existing `out.axioms.sort()` at `:2203`), add `out.sync_liveness();`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-core --test axiom_liveness`
Expected: PASS (2 tests).

- [ ] **Step 5: Verify no regression across the workspace**

Run: `RUSTUP_TOOLCHAIN=stable cargo test --workspace`
Expected: PASS. Nothing reads `live` yet, so this must be a pure addition.

- [ ] **Step 6: Commit**

```bash
git add crates/owl-dl-core/src/ontology.rs crates/owl-dl-core/src/convert.rs crates/owl-dl-core/tests/axiom_liveness.rs
git commit -m "feat(core): axiom liveness bitset with index-stable tombstoning"
```

---

### Task 2: Live-signature reporting (replaces phantom classes on delete)

Spec §4a requires that an entity stop being reported once no live axiom mentions it. The spec words this as per-entity refcounts.

**Intentional implementation deviation — flag for review:** this task instead **recomputes the live signature from the live axiom set** at commit time. Same observable contract, no incrementally-maintained counter that can drift out of sync, and the cost is a single O(live axioms) pass, which the floor measurement already shows is affordable (`docs/2026-08-19-incremental-lowering-floor-findings.md`: the whole lowering pass is 7.6 % of a saturation-only classify on galen). If a reviewer prefers true refcounts, this is the task to object to.

**Files:**
- Create: `crates/owl-dl-core/src/signature.rs`
- Modify: `crates/owl-dl-core/src/lib.rs` (add `pub mod signature;`)
- Test: `crates/owl-dl-core/tests/live_signature.rs` (create)

**Interfaces:**
- Consumes: `InternalOntology::live_axioms` (Task 1).
- Produces: `signature::LiveSignature { classes: FixedBitSet, roles: FixedBitSet, individuals: FixedBitSet }`, `signature::compute(&InternalOntology) -> LiveSignature`, `LiveSignature::has_class(&self, ClassId) -> bool`.

- [ ] **Step 1: Write the failing test**

```rust
// crates/owl-dl-core/tests/live_signature.rs
use horned_owl::model::{Build, RcStr};
use horned_owl::ontology::set::SetOntology;
use horned_owl::model::MutableOntology;
use owl_dl_core::convert::convert_ontology;
use owl_dl_core::signature;

#[test]
fn dropping_the_last_axiom_mentioning_a_class_drops_it_from_the_live_signature() {
    let b = Build::new_rc();
    let mut o: SetOntology<RcStr> = SetOntology::new_rc();
    let a = b.class("http://x/A");
    let c = b.class("http://x/C");
    o.insert(horned_owl::model::SubClassOf {
        sub: a.clone().into(),
        sup: c.clone().into(),
        ann: Default::default(),
    });

    let mut internal = convert_ontology(&o).expect("convert");
    let a_id = internal.vocabulary.class_id("http://x/A").expect("A interned");

    let sig = signature::compute(&internal);
    assert!(sig.has_class(a_id), "A is mentioned by a live axiom");

    // Kill every live axiom; A must drop out of the signature but keep its id.
    for i in internal.live_axiom_indices().collect::<Vec<_>>() {
        internal.kill_axiom(i);
    }
    let sig = signature::compute(&internal);
    assert!(!sig.has_class(a_id), "A is no longer mentioned by any live axiom");
    // Id still resolves - ids are never recycled, only hidden.
    assert_eq!(internal.vocabulary.class_iri(a_id), "http://x/A");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-core --test live_signature`
Expected: FAIL — unresolved module `signature`.

- [ ] **Step 3: Write minimal implementation**

```rust
// crates/owl-dl-core/src/signature.rs
//! Live signature: which named entities are still mentioned by a LIVE axiom.
//!
//! Ids are append-only and never recycled (`vocab.rs:24-33`), so a session that
//! deletes the last axiom mentioning a class would keep reporting that class
//! forever unless reporting is filtered through this set. See spec §4a.

use fixedbitset::FixedBitSet;

use crate::ir::{ClassId, ConceptExpr, ConceptId, IndividualId, RoleId};
use crate::{Axiom, ConceptPool, InternalOntology};

#[derive(Debug, Clone, Default)]
pub struct LiveSignature {
    pub classes: FixedBitSet,
    pub roles: FixedBitSet,
    pub individuals: FixedBitSet,
}

impl LiveSignature {
    #[must_use]
    pub fn has_class(&self, c: ClassId) -> bool {
        let i = c.index() as usize;
        i < self.classes.len() && self.classes.contains(i)
    }
    #[must_use]
    pub fn has_role(&self, r: RoleId) -> bool {
        let i = r.index() as usize;
        i < self.roles.len() && self.roles.contains(i)
    }
    #[must_use]
    pub fn has_individual(&self, i0: IndividualId) -> bool {
        let i = i0.index() as usize;
        i < self.individuals.len() && self.individuals.contains(i)
    }
}

/// Walk every LIVE axiom and mark the entities it mentions.
#[must_use]
pub fn compute(o: &InternalOntology) -> LiveSignature {
    let mut sig = LiveSignature {
        classes: FixedBitSet::with_capacity(o.vocabulary.num_classes()),
        roles: FixedBitSet::with_capacity(o.vocabulary.num_roles()),
        individuals: FixedBitSet::with_capacity(o.vocabulary.num_individuals()),
    };
    for (_idx, ax) in o.live_axioms() {
        mark_axiom(ax, &o.concepts, &mut sig);
    }
    sig
}

fn mark_concept(c: ConceptId, pool: &ConceptPool, sig: &mut LiveSignature) {
    // Iterative walk; the pool is a DAG and concepts can be deeply nested.
    let mut stack = vec![c];
    while let Some(cur) = stack.pop() {
        match pool.get(cur) {
            ConceptExpr::Atomic(cid) => {
                let i = cid.index() as usize;
                if i < sig.classes.len() {
                    sig.classes.insert(i);
                }
            }
            ConceptExpr::Top | ConceptExpr::Bot => {}
            other => {
                for child in other.child_concepts() {
                    stack.push(child);
                }
                for r in other.child_roles() {
                    let i = r.index() as usize;
                    if i < sig.roles.len() {
                        sig.roles.insert(i);
                    }
                }
                for ind in other.child_individuals() {
                    let i = ind.index() as usize;
                    if i < sig.individuals.len() {
                        sig.individuals.insert(i);
                    }
                }
            }
        }
    }
}
```

**Implementation note for the engineer:** `ConceptExpr` has no `child_concepts` / `child_roles` / `child_individuals` helpers today. Add them to `crates/owl-dl-core/src/ir.rs` as small `impl ConceptExpr` methods returning `SmallVec`/`Vec`, one match arm per variant — do **not** hand-roll the traversal separately in `signature.rs`, and do **not** use a wildcard arm, so that adding a future `ConceptExpr` variant is a compile error here rather than a silent signature miss. Write `mark_axiom` the same way: an exhaustive `match` over `Axiom` with no `_` arm.

- [ ] **Step 4: Run test to verify it passes**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-core --test live_signature`
Expected: PASS.

- [ ] **Step 5: Prove exhaustiveness is enforced**

Temporarily add a dummy variant to `ConceptExpr`, run `cargo check -p owl-dl-core`, and confirm the compiler errors inside `child_concepts`. Remove the dummy variant afterward. This step exists because a silent signature miss is a phantom-class bug that the identity gate would only catch much later.

- [ ] **Step 6: Commit**

```bash
git add crates/owl-dl-core/src/signature.rs crates/owl-dl-core/src/lib.rs crates/owl-dl-core/src/ir.rs crates/owl-dl-core/tests/live_signature.rs
git commit -m "feat(core): live-signature computation over live axioms"
```

---

### Task 3: Id-space headroom (slack) in the saturator

Spec §4 + §F2. Synthetics are based at `num_classes()` across **eight** allocator maps (`crates/owl-dl-saturation/src/lib.rs:2377-2434`) plus the nominal region (`:119`). Reserving slack lets new named classes appear without disturbing retained state.

**Files:**
- Modify: `crates/owl-dl-saturation/src/lib.rs` (`SaturateConfig` ~`:83`, `saturate_with_config` `:269`, `TseitinAllocator::new`, `WorklistEngine::new` `:470`, `seed()` `:686-698`)
- Test: `crates/owl-dl-saturation/tests/slack_identity.rs` (create)

**Interfaces:**
- Consumes: nothing.
- Produces: `SaturateConfig { record_proofs: bool, slack: usize }` (`Default` gives `slack: 0`); `pub fn saturate_with_slack(&InternalOntology, usize) -> Subsumers`.

- [ ] **Step 1: Write the failing test**

```rust
// crates/owl-dl-saturation/tests/slack_identity.rs
//! Slack must be semantically invisible: the IRI-level closure with slack N
//! is identical to the closure with slack 0, for every N. Slack only moves
//! synthetic ids further up the id space.
#![allow(clippy::unwrap_used)]

use owl_dl_saturation::{saturate, saturate_with_slack};

mod common;
use common::load_fixture; // parses an .ofn fixture into InternalOntology

fn closure_as_iri_pairs(
    internal: &owl_dl_core::InternalOntology,
    subs: &owl_dl_saturation::Subsumers,
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for i in 0..internal.vocabulary.num_classes() {
        let c = owl_dl_core::ClassId::new(u32::try_from(i).unwrap());
        for s in subs.subsumers_of(c) {
            // Synthetics live above the user vocabulary and have no IRI - skip them.
            if (s.index() as usize) < internal.vocabulary.num_classes() {
                out.push((
                    internal.vocabulary.class_iri(c).to_string(),
                    internal.vocabulary.class_iri(s).to_string(),
                ));
            }
        }
    }
    out.sort();
    out
}

#[test]
fn slack_does_not_change_the_closure() {
    for fixture in ["sulo.ofn", "pizza.ofn", "mie.ofn"] {
        let internal = load_fixture(fixture);
        let base = closure_as_iri_pairs(&internal, &saturate(&internal));
        for slack in [1usize, 64, 1000] {
            let with = closure_as_iri_pairs(&internal, &saturate_with_slack(&internal, slack));
            assert_eq!(base, with, "fixture {fixture} diverged at slack {slack}");
        }
    }
}

#[test]
fn slack_zero_is_the_default_path() {
    let internal = load_fixture("pizza.ofn");
    let a = closure_as_iri_pairs(&internal, &saturate(&internal));
    let b = closure_as_iri_pairs(&internal, &saturate_with_slack(&internal, 0));
    assert_eq!(a, b);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-saturation --test slack_identity`
Expected: FAIL — `saturate_with_slack` not found. (Write `tests/common/mod.rs` with `load_fixture` in this step too; model it on the fixture-path helper in `crates/owl-dl-cli/tests/incremental_fixpoint_identity.rs:20-26`, which resolves fixtures off `CARGO_MANIFEST_DIR` because integration-test cwd is the crate dir, not the workspace root.)

- [ ] **Step 3: Write minimal implementation**

1. `SaturateConfig` gains `pub slack: usize`, defaulting to `0`.
2. `TseitinAllocator::new(num_original_classes)` becomes `TseitinAllocator::new(synth_base)`; every construction site passes `num_classes + slack`.
3. `WorklistEngine::new` receives `num_user_classes` (unchanged, = `num_classes`) and `synth_base`; `num_total_classes` becomes `synth_base + allocated_synthetics`.
4. In `seed()` (`:686-698`), the reflexive `C ⊑ C` loop must iterate `0..num_user_classes` and then only over **allocated** synthetic ids — never across the slack gap, or the gap gets phantom rows.

```rust
pub fn saturate_with_slack(internal: &InternalOntology, slack: usize) -> Subsumers {
    saturate_with_config(
        internal,
        &SaturateConfig { slack, ..SaturateConfig::default() },
    )
    .0
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-saturation`
Expected: PASS.

- [ ] **Step 5: Run the workspace no-regression gate**

Run: `RUSTUP_TOOLCHAIN=stable cargo test --workspace`
Expected: PASS, unchanged. `slack = 0` is the default on every existing call site, so this must be byte-identical. **If anything fails here, stop** — the retarget missed one of the eight maps.

- [ ] **Step 6: Commit**

```bash
git add crates/owl-dl-saturation
git commit -m "feat(saturation): reserved id headroom (slack) above the user vocabulary"
```

---

### Task 4: `convert_delta` + derived-axiom overlay

Spec §3. This is the **soundness-critical** task: `convert_ontology` runs four whole-ontology derivation passes (`convert.rs:2124`, `:2346`, `derive_disjunction_existentials`, `:2237`) and an append-only delta would leave stale derived axioms live, producing false positives.

**Files:**
- Modify: `crates/owl-dl-core/src/convert.rs` (make `seed_dkey_subsumptions` and `derive_functional_max_cardinality` `pub(crate)`; extract the derivation block into `run_derivation_passes`)
- Create: `crates/owl-dl-core/src/delta.rs`
- Modify: `crates/owl-dl-core/src/lib.rs` (`pub mod delta;`)
- Test: `crates/owl-dl-core/tests/convert_delta_equivalence.rs` (create)

**Interfaces:**
- Consumes: Task 1 liveness.
- Produces: `delta::convert_delta<A: ForIRI>(&mut InternalOntology, &SetOntology<A>, &[AnnotatedComponent<A>]) -> Result<Vec<usize>, ConversionError>`; `delta::refresh_derived<A: ForIRI>(&mut InternalOntology, &SetOntology<A>) -> DerivedDiff`; `struct DerivedDiff { added: Vec<usize>, killed: Vec<usize> }`; `Axiom` carries `derived: bool` provenance via a new `InternalOntology::derived: FixedBitSet`.

- [ ] **Step 1: Write the failing test**

```rust
// crates/owl-dl-core/tests/convert_delta_equivalence.rs
//! convert_delta(convert(O), d) must be IRI-equivalent to convert(O + d).
//! Ids WILL differ (convert_ontology sorts at convert.rs:2095 and :2203) -
//! only the IRI-level axiom multiset is comparable.
#![allow(clippy::unwrap_used)]

use horned_owl::model::{Build, MutableOntology, RcStr};
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::convert::convert_ontology;
use owl_dl_core::delta;

fn axiom_strings(o: &owl_dl_core::InternalOntology) -> Vec<String> {
    let mut v: Vec<String> = o
        .live_axioms()
        .map(|(_, ax)| owl_dl_core::debug_render_axiom(ax, &o.vocabulary, &o.concepts))
        .collect();
    v.sort();
    v
}

#[test]
fn delta_addition_matches_from_scratch() {
    let b = Build::new_rc();
    let mut base: SetOntology<RcStr> = SetOntology::new_rc();
    base.insert(horned_owl::model::SubClassOf {
        sub: b.class("http://x/A").into(),
        sup: b.class("http://x/B").into(),
        ann: Default::default(),
    });

    let new_ax = horned_owl::model::SubClassOf {
        sub: b.class("http://x/B").into(),
        sup: b.class("http://x/C").into(),
        ann: Default::default(),
    };

    let mut union = base.clone();
    union.insert(new_ax.clone());
    let from_scratch = convert_ontology(&union).unwrap();

    let mut incremental = convert_ontology(&base).unwrap();
    let mut mirror = base.clone();
    mirror.insert(new_ax.clone());
    delta::convert_delta(&mut incremental, &mirror, &[new_ax.into()]).unwrap();
    delta::refresh_derived(&mut incremental, &mirror);

    assert_eq!(axiom_strings(&from_scratch), axiom_strings(&incremental));
}

#[test]
fn refresh_derived_retracts_a_stale_derived_axiom() {
    // Functional(dp) + DataMin(2, dp) derives an unsat axiom via derive_data_axioms.
    // Removing Functional(dp) must retract it, or the session reports a
    // false-positive unsatisfiable class. This is the exact B1 failure mode.
    let b = Build::new_rc();
    let dp = b.data_property("http://x/dp");
    let functional = horned_owl::model::FunctionalDataProperty {
        dp: dp.clone().into(),
        ann: Default::default(),
    };

    let mut with: SetOntology<RcStr> = SetOntology::new_rc();
    with.insert(functional.clone());
    with.insert(horned_owl::model::SubClassOf {
        sub: b.class("http://x/C").into(),
        sup: horned_owl::model::ClassExpression::DataMinCardinality {
            n: 2,
            dp: dp.clone(),
            dr: b.datatype("http://www.w3.org/2001/XMLSchema#integer").into(),
        },
        ann: Default::default(),
    });

    let mut internal = convert_ontology(&with).unwrap();
    let derived_before = internal.derived.count_ones(..);
    assert!(derived_before > 0, "fixture must actually derive something");

    // Remove Functional(dp) from the mirror, kill its lowered axiom, refresh.
    let mut without = with.clone();
    without.take(&functional.clone().into());
    let diff = delta::refresh_derived(&mut internal, &without);

    assert!(!diff.killed.is_empty(), "the stale derived axiom must be retracted");
    let from_scratch = convert_ontology(&without).unwrap();
    assert_eq!(axiom_strings(&from_scratch), axiom_strings(&internal));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-core --test convert_delta_equivalence`
Expected: FAIL — `delta` module missing. (`debug_render_axiom` may also not exist; add it in Step 3 as a small deterministic renderer — it is test infrastructure that later tasks reuse for IRI-level comparison.)

- [ ] **Step 3: Write minimal implementation**

```rust
// crates/owl-dl-core/src/delta.rs
//! Incremental lowering. See spec §3.
//!
//! `convert_ontology` cannot be reused for a delta: it sorts components before
//! interning (`convert.rs:2095`) and sorts the axiom list again (`:2203`), so
//! ids and indices are a function of the WHOLE axiom set. `convert_delta`
//! interns into the existing vocabulary and appends without sorting.

use horned_owl::model::{AnnotatedComponent, ForIRI};
use horned_owl::ontology::set::SetOntology;

use crate::convert::{convert_component, ConversionError, run_derivation_passes};
use crate::InternalOntology;

/// Lower `added` into `internal`, interning into the EXISTING vocabulary.
/// Returns the new axiom indices. Does not touch derived axioms - call
/// [`refresh_derived`] afterwards, in the same commit.
pub fn convert_delta<A: ForIRI>(
    internal: &mut InternalOntology,
    _mirror: &SetOntology<A>,
    added: &[AnnotatedComponent<A>],
) -> Result<Vec<usize>, ConversionError> {
    let mut out = Vec::new();
    for ac in added {
        if let Some(axiom) =
            convert_component(&ac.component, &mut internal.vocabulary, &mut internal.concepts)?
        {
            out.push(internal.push_live_axiom(axiom));
        }
    }
    Ok(out)
}

#[derive(Debug, Default)]
pub struct DerivedDiff {
    pub added: Vec<usize>,
    pub killed: Vec<usize>,
}

/// Recompute ALL derived axioms over the live user axioms and reconcile.
///
/// SOUNDNESS: the four derivation passes are whole-ontology fixpoints whose
/// output depends on the entire axiom set, so a stale derived axiom retained
/// across a delete is a FALSE POSITIVE. Cost is ~7.6 % of a saturation-only
/// classify on galen - see docs/2026-08-19-incremental-lowering-floor-findings.md.
pub fn refresh_derived<A: ForIRI>(
    internal: &mut InternalOntology,
    mirror: &SetOntology<A>,
) -> DerivedDiff {
    // 1. Kill every currently-live DERIVED axiom (user axioms untouched).
    let stale: Vec<usize> = internal
        .live
        .ones()
        .filter(|i| internal.derived.contains(*i))
        .collect();

    // 2. Re-run the passes over the live user axioms to get the new derived set.
    let fresh = run_derivation_passes(internal, mirror);

    // 3. Reconcile by value so unchanged derived axioms keep their indices
    //    (proof provenance and rule indices stay valid across the commit).
    let mut diff = DerivedDiff::default();
    let mut keep = std::collections::HashSet::new();
    for ax in fresh {
        if let Some(&existing) = internal.derived_index.get(&ax) {
            keep.insert(existing);
        } else {
            let idx = internal.push_live_axiom(ax.clone());
            internal.derived.grow(idx + 1);
            internal.derived.insert(idx);
            internal.derived_index.insert(ax, idx);
            diff.added.push(idx);
        }
    }
    for i in stale {
        if !keep.contains(&i) {
            internal.kill_axiom(i);
            diff.killed.push(i);
        }
    }
    diff
}
```

Supporting changes in `convert.rs`: extract the block from `:2118` (the `let bot_id = ...` line) through `derive_functional_max_cardinality(&mut out)` at `:2237` into

```rust
pub(crate) fn run_derivation_passes<A: ForIRI>(
    out: &mut InternalOntology,
    src: &SetOntology<A>,
) -> Vec<Axiom>
```

returning the derived axioms instead of pushing them, and have `convert_ontology` call it and extend `out.axioms` with the result (preserving today's behaviour exactly, then `sync_liveness()` and mark those indices in `out.derived`).

- [ ] **Step 4: Run tests to verify they pass**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-core`
Expected: PASS.

- [ ] **Step 5: Workspace no-regression gate**

Run: `RUSTUP_TOOLCHAIN=stable cargo test --workspace`
Expected: PASS. `convert_ontology`'s observable output is unchanged — only refactored.

- [ ] **Step 6: Commit**

```bash
git add crates/owl-dl-core
git commit -m "feat(core): convert_delta + derived-axiom overlay (fixes stale-derived FP on delete)"
```

---

### Task 5: Always-on rule→axiom index

Spec §5a. Needed so a later phase can remove a dead axiom's compiled rules. Today the only rule→axiom provenance is `ProofTrace`'s vectors (`proof.rs:146-163`), populated **only** under `record_proofs`.

**Files:**
- Modify: `crates/owl-dl-saturation/src/lib.rs` (`ElRules`, `collect_el_rules`)
- Test: `crates/owl-dl-saturation/tests/rule_axiom_index.rs` (create)

**Interfaces:**
- Consumes: Task 1 liveness.
- Produces: `ElRules::axiom_of_atomic_sub: Vec<u32>`, `axiom_of_conjunctive_trigger: Vec<u32>`, `axiom_of_existential_fact: Vec<u32>`, `axiom_of_existential_trigger: Vec<u32>`, `axiom_of_disjoint_pair: Vec<u32>`, each parallel to its rule vector; sentinel `u32::MAX` = "synthetic, no source axiom".

- [ ] **Step 1: Write the failing test**

```rust
// crates/owl-dl-saturation/tests/rule_axiom_index.rs
#![allow(clippy::unwrap_used)]
mod common;
use common::load_fixture;

#[test]
fn every_compiled_rule_maps_to_a_live_source_axiom_or_the_synthetic_sentinel() {
    let internal = load_fixture("pizza.ofn");
    let rules = owl_dl_saturation::collect_el_rules_for_test(&internal);

    assert_eq!(rules.atomic_subsumptions.len(), rules.axiom_of_atomic_sub.len());
    assert_eq!(rules.conjunctive_triggers.len(), rules.axiom_of_conjunctive_trigger.len());

    for &a in &rules.axiom_of_atomic_sub {
        if a != u32::MAX {
            let idx = a as usize;
            assert!(idx < internal.axioms.len(), "axiom index out of range");
            assert!(internal.live.contains(idx), "rule points at a dead axiom");
        }
    }
}

#[test]
fn index_is_populated_without_proof_recording() {
    // The whole point: this must NOT require RUSTDL_PROOF=1.
    assert!(std::env::var("RUSTDL_PROOF").is_err());
    let internal = load_fixture("sulo.ofn");
    let rules = owl_dl_saturation::collect_el_rules_for_test(&internal);
    assert!(
        rules.axiom_of_atomic_sub.iter().any(|&a| a != u32::MAX),
        "at least one rule must carry real axiom provenance"
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-saturation --test rule_axiom_index`
Expected: FAIL — fields and `collect_el_rules_for_test` do not exist.

- [ ] **Step 3: Write minimal implementation**

Add the five `Vec<u32>` fields to `ElRules`. In `collect_el_rules`, wherever a rule is pushed, push the source axiom index alongside it — unconditionally, not gated on `record_proofs`. Where a rule has no single source axiom (Tseitin-introduced), push `u32::MAX`. Export a test hook:

```rust
/// Test-only accessor for the compiled rule set.
#[doc(hidden)]
#[must_use]
pub fn collect_el_rules_for_test(internal: &InternalOntology) -> ElRules {
    collect_el_rules(internal, &SaturateConfig::default()).0
}
```

`ElRules` and its new fields must be `pub` for the test to read them.

- [ ] **Step 4: Run tests to verify they pass**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-saturation`
Expected: PASS.

- [ ] **Step 5: Confirm the index costs nothing measurable**

Run: `RUSTUP_TOOLCHAIN=stable cargo run --release -p owl-dl-bench -- classify ontologies/external/galen.ofn`
Compare wall-clock against the pre-task value (record both in the commit message). Expected: within noise — this adds one `Vec<u32>` push per compiled rule. If it is not within noise, say so rather than proceeding.

- [ ] **Step 6: Commit**

```bash
git add crates/owl-dl-saturation
git commit -m "feat(saturation): always-on rule->axiom provenance index"
```

---

### Task 6: `SaturationState` — a saturator that survives across revisions

Spec §1. Today `saturate()` builds a `WorklistEngine` and drops it.

**Files:**
- Create: `crates/owl-dl-saturation/src/state.rs`
- Modify: `crates/owl-dl-saturation/src/lib.rs` (`pub mod state;`, make `WorklistEngine` reachable)
- Test: `crates/owl-dl-saturation/tests/state_addition_identity.rs` (create)

**Interfaces:**
- Consumes: Tasks 3 (slack) and 5 (rule index).
- Produces: `state::SaturationState`, `SaturationState::build(&InternalOntology, slack: usize) -> Self`, `SaturationState::apply_additions(&mut self, &InternalOntology, &[usize]) -> DeltaOutcome`, `SaturationState::subsumers(&self) -> &Subsumers`, `struct DeltaOutcome { pub rebuilt: bool, pub marked_contexts: usize }`.

- [ ] **Step 1: Write the failing test**

```rust
// crates/owl-dl-saturation/tests/state_addition_identity.rs
//! Retained-state addition must equal from-scratch saturation, at IRI level.
#![allow(clippy::unwrap_used)]
mod common;
use common::{closure_as_iri_pairs, load_fixture_pair};
use owl_dl_saturation::{saturate, state::SaturationState};

#[test]
fn incremental_addition_equals_from_scratch() {
    // load_fixture_pair returns (base_internal, union_internal, added_axiom_indices)
    // where union == base + one SubClassOf axiom.
    for fixture in ["sulo.ofn", "pizza.ofn", "mie.ofn"] {
        let (base, union, added) = load_fixture_pair(fixture);

        let mut st = SaturationState::build(&base, 64);
        let outcome = st.apply_additions(&union, &added);
        assert!(!outcome.rebuilt, "a pure addition must not force a rebuild");

        let incremental = closure_as_iri_pairs(&union, st.subsumers());
        let from_scratch = closure_as_iri_pairs(&union, &saturate(&union));
        assert_eq!(from_scratch, incremental, "fixture {fixture}");
    }
}

#[test]
fn addition_introducing_a_new_class_fits_in_slack() {
    let (base, union, added) = load_fixture_pair("sulo-new-class.ofn");
    let mut st = SaturationState::build(&base, 64);
    let outcome = st.apply_additions(&union, &added);
    assert!(!outcome.rebuilt, "a new named class must fit in slack, not force a rebuild");
    assert_eq!(
        closure_as_iri_pairs(&union, &saturate(&union)),
        closure_as_iri_pairs(&union, st.subsumers())
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-saturation --test state_addition_identity`
Expected: FAIL — `state` module missing. Create the `sulo-new-class.ofn` fixture in `ontologies/regression/` in this step (sulo plus one axiom mentioning a class name absent from sulo).

- [ ] **Step 3: Write minimal implementation**

`SaturationState` owns the `WorklistEngine` plus the `slack` and `synth_base` it was built with. `apply_additions`:

1. If any added axiom is an object/data-property axiom (sub-property, equivalent-property, transitive, reflexive, chain, domain/range on a new role), set `rebuilt = true`, rebuild from scratch, return. Spec §9 — `role_super` is frozen at build and ELK does the same.
2. If the new named classes/individuals would exceed `synth_base`, set `rebuilt = true` and rebuild with doubled slack.
3. Otherwise: compile the new axioms into additional rules (reusing `collect_el_rules` restricted to the new indices), push them into the engine's rule tables and trigger indices, seed the new classes' reflexive rows, enqueue the new rules' consequences on the worklist, and run the fixpoint to quiescence. Monotone addition needs no invalidation.

- [ ] **Step 4: Run tests to verify they pass**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-saturation`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/owl-dl-saturation ontologies/regression/sulo-new-class.ofn
git commit -m "feat(saturation): SaturationState with monotone addition"
```

---

### Task 7: `IncrementalSession` public API

Spec §8, §7 (fail-closed), §2 (delta contract).

**Files:**
- Create: `crates/owl-dl-reasoner/src/incremental.rs`
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (`pub mod incremental;` + re-exports)
- Test: `crates/owl-dl-reasoner/tests/incremental_session.rs` (create)

**Interfaces:**
- Consumes: Tasks 1–6.
- Produces: `IncrementalSession::new<A: ForIRI>(&SetOntology<A>) -> Result<Self, ReasonError>`, `.apply<A: ForIRI>(&mut self, &AxiomDelta<A>) -> Result<Revision, ReasonError>`, `.classify(&mut self) -> Result<&Classification, ReasonError>`, `.is_subclass_of(&mut self, &str, &str) -> Result<bool, ReasonError>`, `.is_consistent(&mut self) -> Result<bool, ReasonError>`, `.revision(&self) -> Revision`, `.stats(&self) -> &SessionStats`; `struct AxiomDelta<A: ForIRI> { added: Vec<AnnotatedComponent<A>>, removed: Vec<AnnotatedComponent<A>> }`; `struct Revision(pub u64)`; `SessionStats { revisions: u64, rebuilds: u64, additions_reused: u64 }`.

- [ ] **Step 1: Write the failing test**

```rust
// crates/owl-dl-reasoner/tests/incremental_session.rs
#![allow(clippy::unwrap_used)]
use horned_owl::model::{Build, MutableOntology, RcStr};
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::incremental::{AxiomDelta, IncrementalSession};

fn hierarchy(c: &owl_dl_reasoner::Classification) -> Vec<(String, String)> {
    let mut v = Vec::new();
    for a in c.classes() {
        for b in c.classes() {
            if a != b && c.is_subclass(a, b) {
                v.push((a.clone(), b.clone()));
            }
        }
    }
    v.sort();
    v
}

#[test]
fn session_addition_matches_from_scratch() {
    let b = Build::new_rc();
    let mut base: SetOntology<RcStr> = SetOntology::new_rc();
    base.insert(horned_owl::model::SubClassOf {
        sub: b.class("http://x/A").into(),
        sup: b.class("http://x/B").into(),
        ann: Default::default(),
    });
    let added = horned_owl::model::SubClassOf {
        sub: b.class("http://x/B").into(),
        sup: b.class("http://x/C").into(),
        ann: Default::default(),
    };

    let mut session = IncrementalSession::new(&base).unwrap();
    assert_eq!(session.revision().0, 0);
    let rev = session
        .apply(&AxiomDelta { added: vec![added.clone().into()], removed: vec![] })
        .unwrap();
    assert_eq!(rev.0, 1);

    let mut union = base.clone();
    union.insert(added);
    let expected = owl_dl_reasoner::classify(&union).unwrap();

    assert_eq!(hierarchy(&expected), hierarchy(session.classify().unwrap()));
    // A ⊑ C must now be entailed transitively.
    assert!(session.is_subclass_of("http://x/A", "http://x/C").unwrap());
}

#[test]
fn consistency_verdict_is_retained_in_the_monotone_direction() {
    // Spec §10: `consistent` survives a delete; `inconsistent` survives an add.
    let b = Build::new_rc();
    let ax = horned_owl::model::SubClassOf {
        sub: b.class("http://x/A").into(),
        sup: b.class("http://x/B").into(),
        ann: Default::default(),
    };
    let mut base: SetOntology<RcStr> = SetOntology::new_rc();
    base.insert(ax.clone());

    let mut session = IncrementalSession::new(&base).unwrap();
    assert!(session.is_consistent().unwrap());
    // A pure delete cannot make a consistent KB inconsistent.
    session.apply(&AxiomDelta { added: vec![], removed: vec![ax.into()] }).unwrap();
    assert!(session.is_consistent().unwrap());
}

#[test]
fn removal_forces_a_rebuild_but_stays_correct_in_p1() {
    let b = Build::new_rc();
    let ax = horned_owl::model::SubClassOf {
        sub: b.class("http://x/A").into(),
        sup: b.class("http://x/B").into(),
        ann: Default::default(),
    };
    let mut base: SetOntology<RcStr> = SetOntology::new_rc();
    base.insert(ax.clone());

    let mut session = IncrementalSession::new(&base).unwrap();
    session
        .apply(&AxiomDelta { added: vec![], removed: vec![ax.into()] })
        .unwrap();

    assert_eq!(session.stats().rebuilds, 1, "P1 rebuilds on any delete");
    assert!(!session.is_subclass_of("http://x/A", "http://x/B").unwrap());
}

#[test]
fn a_rejected_delta_leaves_the_revision_untouched() {
    let b = Build::new_rc();
    let mut base: SetOntology<RcStr> = SetOntology::new_rc();
    base.insert(horned_owl::model::SubClassOf {
        sub: b.class("http://x/A").into(),
        sup: b.class("http://x/B").into(),
        ann: Default::default(),
    });
    let mut session = IncrementalSession::new(&base).unwrap();
    let before = hierarchy(session.classify().unwrap()); // owned Vec: borrow ends here
    let rev_before = session.revision();

    // A delta whose removal names an axiom that is not present.
    let bogus = horned_owl::model::SubClassOf {
        sub: b.class("http://x/NOPE").into(),
        sup: b.class("http://x/ALSO_NOPE").into(),
        ann: Default::default(),
    };
    let err = session.apply(&AxiomDelta { added: vec![], removed: vec![bogus.into()] });
    assert!(err.is_err(), "removing an absent axiom is rejected");
    assert_eq!(session.revision().0, rev_before.0, "revision must not advance");
    assert_eq!(before, hierarchy(session.classify().unwrap()));
}

#[test]
fn annotation_only_delta_is_logically_empty() {
    // Spec §10: annotation edits lower to zero logical axioms and must commit
    // a revision with zero invalidation.
    let b = Build::new_rc();
    let mut base: SetOntology<RcStr> = SetOntology::new_rc();
    base.insert(horned_owl::model::SubClassOf {
        sub: b.class("http://x/A").into(),
        sup: b.class("http://x/B").into(),
        ann: Default::default(),
    });
    let mut session = IncrementalSession::new(&base).unwrap();
    let rebuilds_before = session.stats().rebuilds;

    let anno = horned_owl::model::AnnotationAssertion {
        subject: b.iri("http://x/A").into(),
        ann: horned_owl::model::Annotation {
            ap: b.annotation_property("http://www.w3.org/2000/01/rdf-schema#comment"),
            av: horned_owl::model::AnnotationValue::Literal(
                horned_owl::model::Literal::Simple { literal: "hi".to_string() },
            ),
        },
    };
    session.apply(&AxiomDelta { added: vec![anno.into()], removed: vec![] }).unwrap();
    assert_eq!(session.stats().rebuilds, rebuilds_before, "no rebuild for an annotation");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test incremental_session`
Expected: FAIL — `incremental` module missing.

- [ ] **Step 3: Write minimal implementation**

`IncrementalSession` holds: the `SetOntology<A>` mirror, the `InternalOntology`, a `SaturationState`, the cached `Classification` (invalidated on commit), `Revision`, and `SessionStats`.

`apply` in commit order:
1. **Stage.** Resolve every `removed` component to a live axiom index; if any is absent, return `Err` **before mutating anything** (fail-closed, §7).
2. **Logically-empty check.** If the delta lowers to zero logical axioms (annotations, declarations of already-known entities), bump the revision and return with no invalidation.
3. **Route.** Non-empty removals, or any property-axiom addition, or slack exhaustion ⇒ full rebuild (`stats.rebuilds += 1`). Otherwise `SaturationState::apply_additions` (`stats.additions_reused += 1`).
4. **Derived overlay.** Call `delta::refresh_derived` before saturation runs, so no stale derived axiom reaches the engine.
5. **Commit.** Update the mirror, bump `Revision`, drop the cached `Classification`.

`classify` recomputes if the cache is empty and **sorts reported classes by IRI** (session ids differ from from-scratch ids — spec §F1), filtering through the Task 2 live signature.

**Consistency-verdict retention (spec §10).** Consistency is monotone in both directions:
`consistent` survives a delete, `inconsistent` survives an add. Cache the last verdict as
`Option<bool>` and short-circuit `is_consistent` when the transaction direction preserves it —
`Some(true)` + pure delete ⇒ still consistent; `Some(false)` + pure add ⇒ still inconsistent.
Any other combination recomputes. This answers `is_consistent` for free in roughly half of all
transactions, for a few lines of code.

- [ ] **Step 4: Run tests to verify they pass**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test incremental_session`
Expected: PASS (6 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/owl-dl-reasoner
git commit -m "feat(reasoner): IncrementalSession (addition-only, rebuild on delete)"
```

---

### Task 8: Identity gate over randomized edit scripts

Spec gate 1. **Runs budget-free** — with budgets on, `timed_out_pairs` (`classify.rs:278-284`) and the unsat probe's default-to-satisfiable on timeout (`:828`) are host-speed dependent, so from-scratch is not reproducible against itself and the gate would flake by construction.

**Files:**
- Test: `crates/owl-dl-reasoner/tests/incremental_identity_gate.rs` (create)

**Interfaces:**
- Consumes: Task 7.
- Produces: nothing (test-only).

- [ ] **Step 1: Write the failing test**

```rust
// crates/owl-dl-reasoner/tests/incremental_identity_gate.rs
//! Gate 1. A session reaching an axiom set via an edit script must produce an
//! IRI-identical hierarchy to a from-scratch run on that same set, at EVERY
//! revision - not just the last. Budget-free by construction.
#![allow(clippy::unwrap_used)]

use owl_dl_reasoner::incremental::{AxiomDelta, IncrementalSession};

mod common;
use common::{hierarchy_iris, load_ofn, split_axioms};

/// Deterministic LCG - no rand dependency, and a failing seed is reproducible.
fn lcg(state: &mut u64) -> u64 {
    *state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
    *state
}

#[test]
fn addition_script_matches_from_scratch_at_every_revision() {
    for fixture in ["sulo.ofn", "pizza.ofn", "mie.ofn"] {
        for seed in [1u64, 7, 42] {
            let full = load_ofn(fixture);
            let mut state = seed;
            // Start from a random half; add the rest in random order.
            let (mut current, rest) = split_axioms(&full, |_| lcg(&mut state) % 2 == 0);

            let mut session = IncrementalSession::new(&current).unwrap();
            for ax in rest {
                session
                    .apply(&AxiomDelta { added: vec![ax.clone()], removed: vec![] })
                    .unwrap();
                current.insert(ax.clone());

                let expected = owl_dl_reasoner::classify(&current).unwrap();
                assert_eq!(
                    hierarchy_iris(&expected),
                    hierarchy_iris(session.classify().unwrap()),
                    "fixture {fixture} seed {seed} diverged at revision {}",
                    session.revision().0
                );
            }
        }
    }
}

#[test]
fn round_trip_add_then_remove_returns_to_the_original() {
    // Gate 2. Catches over-retention: in P1 the removal rebuilds, so this
    // proves the ADD path left no state that survives into the rebuild.
    let full = load_ofn("pizza.ofn");
    let (base, rest) = split_axioms(&full, |i| i % 3 != 0);
    let mut session = IncrementalSession::new(&base).unwrap();
    let before = hierarchy_iris(session.classify().unwrap());

    for ax in &rest {
        session.apply(&AxiomDelta { added: vec![ax.clone()], removed: vec![] }).unwrap();
    }
    for ax in &rest {
        session.apply(&AxiomDelta { added: vec![], removed: vec![ax.clone()] }).unwrap();
    }
    assert_eq!(before, hierarchy_iris(session.classify().unwrap()));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test incremental_identity_gate`
Expected: FAIL — `common` helpers missing. Write them in this step: `load_ofn` (fixture path off `CARGO_MANIFEST_DIR`), `split_axioms`, `hierarchy_iris` (sorted `(sub, sup)` IRI pairs read through the public `Classification::is_subclass` (`classify.rs:427`), **never** raw matrix rows — `is_subclass` routes through the private `entails` choke-point (`:418`) which is the ONLY place the elided `⊥ ⊑ *` rows are reintroduced (`classify.rs:69-71`, invariant at `:411-419`)).

- [ ] **Step 3: Make it pass**

If Tasks 1–7 are correct this passes with no production change. If it fails, the failure is a real bug in one of them — **fix the cause, not the test.** The most likely culprits, in order: a missed allocator map in Task 3; a derived axiom not reconciled in Task 4; reported class order not IRI-sorted in Task 7.

- [ ] **Step 4: Run the full gate**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test incremental_identity_gate -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/owl-dl-reasoner/tests
git commit -m "test(reasoner): budget-free identity + round-trip gates for the session"
```

---

### Task 9: P1 exit criterion — measure against the floor

Spec *Phasing*, P1 row. Single-axiom addition on galen must complete in ≤ 2× the measured 5.8 ms lowering floor (≤ ~12 ms), i.e. within ~1.6× of the theoretical best. Below that bar the retained-state design is not paying off and must be re-examined **before** P2 builds deletion on it.

**Files:**
- Modify: `crates/owl-dl-bench/src/main.rs` (add an `incremental-latency` subcommand)
- Create: `docs/2026-08-19-incremental-p1-latency.md` (results)

**Interfaces:**
- Consumes: Task 7.
- Produces: `owl-dl-bench incremental-latency FILE --revisions N` printing p50/p95/max per-revision milliseconds and the rebuild count.

- [ ] **Step 1: Add the subcommand**

```rust
// crates/owl-dl-bench/src/main.rs - new Cmd variant
IncrementalLatency {
    file: PathBuf,
    #[arg(long, default_value_t = 100)]
    revisions: usize,
},
```

Handler: parse the ontology, build an `IncrementalSession`, then for each of `revisions` iterations add one fresh synthetic leaf axiom (`SubClassOf(<gen:i>, <an existing class>)`), timing each `apply` + `classify`. Print p50/p95/max and `stats().rebuilds`.

- [ ] **Step 2: Run it on galen**

Run: `RUSTUP_TOOLCHAIN=stable cargo run --release -p owl-dl-bench -- incremental-latency ontologies/external/galen.ofn --revisions 100`
Expected: p50 ≤ 12 ms, rebuilds ≪ revisions.

- [ ] **Step 3: Record the result honestly**

Write `docs/2026-08-19-incremental-p1-latency.md` with p50/p95/max, rebuild count, host, and the pass/fail verdict against the ≤12 ms bar. **If it fails the bar, write that down and stop** — escalate rather than proceeding to P2. A failed exit criterion is the signal the plan exists to produce.

- [ ] **Step 4: Full workspace gate**

Run:
```bash
RUSTUP_TOOLCHAIN=stable cargo test --workspace
RUSTUP_TOOLCHAIN=stable cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/owl-dl-bench docs/2026-08-19-incremental-p1-latency.md
git commit -m "feat(bench): incremental-latency + P1 exit-criterion measurement"
```

---

## Out of scope for this plan

Each gets its own plan, written after its predecessor lands:

- **P0** — edit-locality measurement over real GO history. Independent of P1; decides P2's algorithm.
- **P2** — ELK-style context invalidation for deletion (five mark channels, `seen_facts` eviction, fact tombstoning, cost-based bail-out, B2 `fired` reset).
- **P3** — tableau monotonicity retention + sticky incompleteness.
- **P4** — CLI JSONL protocol + Python `Session`.
