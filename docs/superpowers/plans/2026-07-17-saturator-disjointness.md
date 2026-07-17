# Saturator Disjointness in the Complete Fragment — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Admit `DisjointClasses` / `DisjointUnion` into the saturator's complete fragment when the ontology has no functional / inverse-functional roles, so disjoint-using Horn ontologies take the one-pass CB fast path instead of DNF-ing on the O(n²) per-pair tableau — soundly and completely.

**Architecture:** An allowlist-gate change, not an engine change. The saturator already builds `disjoint_pairs`, fires `ElRule::DisjointnessClash`, and propagates unsat via `process_unsat` (to subclasses and back through ∃-facts) — complete on the EL+disjoint-no-functional Horn fragment by construction. This increment gates `is_saturator_axiom` to accept the disjoint fragment when no functional/inverse-functional roles are present (the "disjoint×functional-merge unproven" case — 4/39 onts — stays on the hybrid path).

**Tech Stack:** Rust (edition 2024), the rustdl workspace (`owl-dl-reasoner::classify`), horned-owl OFN parsing.

## Global Constraints

- Build/test with the stable toolchain: prefix every cargo command — `export PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"; RUSTUP_TOOLCHAIN=stable cargo …`.
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all -- --check` must pass (pedantic on; warnings are errors).
- **FP=0 / MISSED=0 is the non-negotiable soundness contract.** The failure mode to prevent is routing a disjoint ontology to an *incomplete* saturator that silently MISSES an inconsistency (the D10 unsound-completeness bug class).
- This is a **FOUNDATION** increment: standalone DNF-recovery is expected to be ~0 (31/39 disjoint onts also use symmetric, the next increment). Success = soundness + completeness + the gate firing, NOT ontology recovery.
- Gate only on `functional_roles.is_empty() && inverse_functional_roles.is_empty()`; disjoint+functional onts must keep falling to the hybrid path.

---

### Task 1: Gate `DisjointClasses` / `DisjointUnion` into the complete fragment (no-functional)

**Files:**
- Modify: `crates/owl-dl-reasoner/src/classify.rs` — `saturator_complete_fragment` (~line 970), `is_saturator_axiom` (~line 1023), and the unit test `saturator_fragment_rejects_disjoint_classes` (~line 3633).

**Interfaces:**
- Consumes: `saturator_complete_fragment(internal: &InternalOntology) -> bool` (pub(crate)); `internal_of(body: &str) -> InternalOntology` (test helper in the `#[cfg(test)] mod tests`); `Axiom::{DisjointClasses(Vec<ConceptId>), DisjointUnion{class,members}, FunctionalRole(Role), InverseFunctionalRole(Role)}`.
- Produces: `is_saturator_axiom` gains a `disjoint_ok: bool` parameter; the fragment accepts the disjoint fragment iff no functional/inverse-functional roles.

- [ ] **Step 1: Write/replace the failing unit tests.** In `crates/owl-dl-reasoner/src/classify.rs`, REPLACE the existing `saturator_fragment_rejects_disjoint_classes` test with these two:

```rust
    #[test]
    fn saturator_fragment_accepts_disjoint_without_functional() {
        // No functional/inverse-functional roles ⇒ the disjoint×functional
        // interaction is absent, so DisjointClasses is now in the complete
        // fragment (one-pass fast path). The DisjointnessClash rule + unsat
        // back-prop are complete on EL+disjoint-no-functional by construction.
        let i = internal_of(
            "    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    DisjointClasses(:A :B)\n",
        );
        assert!(
            saturator_complete_fragment(&i),
            "DisjointClasses with no functional roles must be in the complete fragment"
        );
    }

    #[test]
    fn saturator_fragment_rejects_disjoint_with_functional() {
        // Functional role present ⇒ the disjoint×functional-merge interaction
        // is unproven, so the ontology conservatively falls to the hybrid path.
        let i = internal_of(
            "    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(ObjectProperty(:r))\n\
    FunctionalObjectProperty(:r)\n\
    DisjointClasses(:A :B)\n",
        );
        assert!(
            !saturator_complete_fragment(&i),
            "DisjointClasses + a functional role must fall back to the hybrid path"
        );
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --lib saturator_fragment_accepts_disjoint_without_functional saturator_fragment_rejects_disjoint_with_functional`
Expected: `accepts_disjoint_without_functional` FAILS (currently `DisjointClasses` always → `false`); `rejects_disjoint_with_functional` PASSES already (still rejected today).

- [ ] **Step 3: Collect inverse-functional roles + compute the gate in `saturator_complete_fragment`.** Replace the `functional_roles` collection and the final `.all(...)` call (~lines 991-1002) with:

```rust
    let functional_roles: HashSet<Role> = internal
        .axioms
        .iter()
        .filter_map(|ax| match ax {
            Axiom::FunctionalRole(r) => Some(*r),
            _ => None,
        })
        .collect();
    // Disjointness is admitted only when there is no functional / inverse-
    // functional role: the disjoint×functional-merge interaction is unproven
    // (a later increment), so disjoint+functional falls to the hybrid path.
    let has_cardinality_role = functional_roles.iter().next().is_some()
        || internal
            .axioms
            .iter()
            .any(|ax| matches!(ax, Axiom::InverseFunctionalRole(_)));
    let disjoint_ok = !has_cardinality_role;
    internal
        .axioms
        .iter()
        .all(|ax| is_saturator_axiom(ax, &internal.concepts, &functional_roles, disjoint_ok))
}
```

- [ ] **Step 4: Add the `disjoint_ok` param + the disjoint arms to `is_saturator_axiom`.** Change the signature (~line 1023) to:

```rust
fn is_saturator_axiom(
    ax: &Axiom,
    pool: &ConceptPool,
    functional_roles: &HashSet<Role>,
    disjoint_ok: bool,
) -> bool {
```

and, inside the `match ax {`, add these arms immediately before the final `_ => false,`:

```rust
        // Disjointness is complete in the saturator (DisjointnessClash +
        // process_unsat back-prop) on the EL+disjoint-no-functional Horn
        // fragment by construction. Admitted only when no functional /
        // inverse-functional role is present (see saturator_complete_fragment).
        Axiom::DisjointClasses(_) | Axiom::DisjointUnion { .. } => disjoint_ok,
```

- [ ] **Step 5: Run the unit tests to verify they pass**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --lib saturator_fragment`
Expected: PASS — including the two new tests and the pre-existing `saturator_fragment_*` tests (accepts_el_plus_functional, rejects_forall, rejects_max_cardinality, rejects_user_unqualified_max_without_functional, accepts_derived_functional_max_gci).

- [ ] **Step 6: Implementation-order check (confirm the disjoint form reaches the gate).** Confirm the two new tests exercise the intended path: `internal_of` runs `convert_ontology` (the same pipeline `saturator_complete_fragment` sees), and `saturator_fragment_accepts_disjoint_without_functional` passing at Step 5 proves `Axiom::DisjointClasses` is present in `internal.axioms` at the check point (not already rewritten to `SubClassOf(A,¬B)`). If instead that test still fails after Steps 3-4, the disjoint fragment is reaching the gate as `SubClassOf(A, Not(Atomic))` — in that case ALSO extend `is_saturator_concept` to accept `Not(Atomic)` on a SubClassOf RHS gated by `disjoint_ok`, and note it in the commit. (Expected: not needed — the existing `_ => false` arm comment lists `DisjointClasses` as a live variant.)

- [ ] **Step 7: fmt + clippy + commit**

```bash
RUSTUP_TOOLCHAIN=stable cargo fmt --all
RUSTUP_TOOLCHAIN=stable cargo clippy -p owl-dl-reasoner --all-targets -- -D warnings
git add crates/owl-dl-reasoner/src/classify.rs
git commit -m "feat(reasoner): admit DisjointClasses/DisjointUnion to the saturator complete fragment (no-functional gate)"
```

---

### Task 2: Disjointness fast-path soundness canaries

**Files:**
- Create: `crates/owl-dl-reasoner/tests/saturator_disjointness.rs`

**Interfaces:**
- Consumes: `owl_dl_reasoner::classify(&SetOntology) -> Result<Classification, _>`; `Classification::unsatisfiable_classes() -> Vec<&str>`; `owl_dl_reasoner::classify` dispatches via the Horn-shortcircuit (env `RUSTDL_HORN_SHORTCIRCUIT`, default ON), so a disjoint-no-functional ontology now takes the saturator fast path.
- Produces: end-to-end proof that the fast path detects disjointness clashes soundly + completely, and matches the hybrid path.

- [ ] **Step 1: Write the failing canaries.** Create `crates/owl-dl-reasoner/tests/saturator_disjointness.rs`:

```rust
//! Disjointness on the saturator fast path (no-functional): clash detection,
//! ∃-fact back-propagation, a satisfiable control, and fast-vs-hybrid identity.
use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::classify;
use std::io::Cursor;
use std::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn onto(body: &str) -> SetOntology<RcStr> {
    let src = format!("Prefix(:=<http://e#>)\nOntology(\n{body}\n)");
    let mut r = Cursor::new(src);
    read_ofn(&mut r, ParserConfiguration::default()).expect("parse ofn").0
}

// SetEnvGuard: set/unset an env var for the duration of a test, restore on drop.
struct SetEnvGuard { key: &'static str, prior: Option<std::ffi::OsString> }
impl SetEnvGuard {
    #[allow(unsafe_code)]
    fn set(key: &'static str, value: &str) -> Self {
        let prior = std::env::var_os(key);
        unsafe { std::env::set_var(key, value) };
        Self { key, prior }
    }
}
impl Drop for SetEnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        unsafe {
            match &self.prior {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

// A) basic disjointness clash: X ⊑ A, X ⊑ B, Disjoint(A,B) ⇒ X unsatisfiable.
#[test]
fn disjoint_clash_makes_class_unsatisfiable() {
    let o = onto(
        "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:X))\n\
         DisjointClasses(:A :B)\n\
         SubClassOf(:X :A) SubClassOf(:X :B)",
    );
    let c = classify(&o).expect("classify");
    assert!(c.unsatisfiable_classes().contains(&"http://e#X"), "X⊑A⊓B (disjoint) must be unsatisfiable");
}

// B) unsat back-propagates through an ∃-fact: Y ⊑ ∃r.X, X ⊑ A⊓B disjoint ⇒ X and Y unsatisfiable.
#[test]
fn disjoint_clash_backpropagates_through_existential() {
    let o = onto(
        "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:X)) Declaration(Class(:Y))\n\
         Declaration(ObjectProperty(:r))\n\
         DisjointClasses(:A :B)\n\
         SubClassOf(:X :A) SubClassOf(:X :B)\n\
         SubClassOf(:Y ObjectSomeValuesFrom(:r :X))",
    );
    let c = classify(&o).expect("classify");
    let u = c.unsatisfiable_classes();
    assert!(u.contains(&"http://e#X"), "X must be unsatisfiable");
    assert!(u.contains(&"http://e#Y"), "Y⊑∃r.X with X unsat must be unsatisfiable (∃-fact back-prop)");
}

// C) satisfiable control: disjoint classes with DISTINCT subclasses ⇒ NO spurious unsat.
#[test]
fn disjoint_without_shared_subclass_is_satisfiable() {
    let o = onto(
        "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:X)) Declaration(Class(:Z))\n\
         DisjointClasses(:A :B)\n\
         SubClassOf(:X :A) SubClassOf(:Z :B)",
    );
    let c = classify(&o).expect("classify");
    assert!(c.unsatisfiable_classes().is_empty(), "no class is ⊑ both A and B; nothing must be unsat");
}

// D) fast (shortcircuit ON) vs hybrid (shortcircuit OFF) produce the SAME unsatisfiable set.
#[test]
fn disjoint_fastpath_matches_hybrid() {
    let body =
        "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:X)) Declaration(Class(:Y))\n\
         Declaration(ObjectProperty(:r))\n\
         DisjointClasses(:A :B)\n\
         SubClassOf(:X :A) SubClassOf(:X :B)\n\
         SubClassOf(:Y ObjectSomeValuesFrom(:r :X))";
    let _lock = ENV_LOCK.lock().unwrap();
    let fast = {
        let _g = SetEnvGuard::set("RUSTDL_HORN_SHORTCIRCUIT", "1");
        let mut u = classify(&onto(body)).expect("classify fast").unsatisfiable_classes()
            .into_iter().map(str::to_owned).collect::<Vec<_>>();
        u.sort();
        u
    };
    let hybrid = {
        let _g = SetEnvGuard::set("RUSTDL_HORN_SHORTCIRCUIT", "0");
        let mut u = classify(&onto(body)).expect("classify hybrid").unsatisfiable_classes()
            .into_iter().map(str::to_owned).collect::<Vec<_>>();
        u.sort();
        u
    };
    assert_eq!(fast, hybrid, "fast-path (saturator) unsat set must equal hybrid-path unsat set");
}
```

- [ ] **Step 2: Run the canaries to verify they pass**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test saturator_disjointness -- --test-threads=1`
Expected: all 4 PASS. (`--test-threads=1` because test D mutates a process-global env var under `ENV_LOCK`; if A–C are flaky under parallelism due to the env mutation in D, the single-thread run removes the interference.) If A/B FAIL, the fast path is NOT detecting/propagating the clash — that is a genuine completeness gap in the saturator's disjointness handling for this fragment; investigate `process_unsat` / the `DisjointnessClash` firing, fix at the source, do NOT weaken the assertion.

- [ ] **Step 3: fmt + clippy + commit**

```bash
RUSTUP_TOOLCHAIN=stable cargo fmt --all
RUSTUP_TOOLCHAIN=stable cargo clippy -p owl-dl-reasoner --all-targets -- -D warnings
git add crates/owl-dl-reasoner/tests/saturator_disjointness.rs
git commit -m "test(reasoner): disjointness fast-path soundness canaries (clash, ∃-back-prop, control, fast==hybrid)"
```

---

### Task 3: Acceptance — non-regression + MISSED=0 on the oracle-classifiable disjoint subset

**Files:**
- None (verification task). Optionally append a note to `docs/superpowers/specs/2026-07-17-saturator-disjointness-design.md` or a new results file.

**Interfaces:**
- Consumes: the full workspace test suite; the ORE `pool_sample` corpus at `/data/dumontier/ore-run/pool_sample/files`; a fresh `target/release/rustdl`; the pilot oracle outputs under `/data/dumontier/ore-run/pilot/<ont>.owl/kon.owx` where present.

- [ ] **Step 1: Curated non-regression — full workspace tests green**

Run: `RUSTUP_TOOLCHAIN=stable cargo test --workspace`
Expected: green except the known pre-existing, unrelated failure `incremental_matches_baseline_on_fixtures` (missing fixture `ontologies/regression/funcmerge-cyclic.ofn`). No NEW failures. The curated closure-diff / oracle tests (`konclude_closure_diff.rs`, `completeness_contract.rs`, …) must still pass — most curated fixtures are disjoint-free or functional (so unaffected); any curated disjoint-no-functional fixture must be byte-identical fast-vs-hybrid.

- [ ] **Step 2: Confirm the gate fires — a disjoint-no-functional ORE ont now takes the fast path.** Build fresh (`RUSTUP_TOOLCHAIN=stable cargo build --release -p owl-dl-cli`), then on a small disjoint-no-functional ORE ont, confirm the `# fragment:` banner / classify behaviour reflects the fast path (it no longer routes through the per-pair hybrid path for the disjointness reason). Pick one from the disjoint Horn-ish set, e.g. verify it classifies via saturation without the disjointness axiom kicking it to hybrid.

- [ ] **Step 3: MISSED=0 / FP=0 evidence (three sources, in priority order).** The soundness of this increment rests on: (a) **by-construction** — disjointness on EL+disjoint-no-functional yields only unsat, which `process_unsat` propagates completely (the spec's tier-iii argument); (b) **the Task 2 canaries** — tiny-fixture clash / ∃-back-prop / satisfiable-control / fast==hybrid, which are the empirical completeness+soundness check; (c) **the curated closure-diff** in the workspace suite (Step 1), which is the FP=0/MISSED=0 gate on the curated fixtures. Those three carry the gate. As an additional confidence spot-check against a real disjoint ORE ont: pick a *small* disjoint-no-functional ont that has a pilot Konclude oracle (`ls /data/dumontier/ore-run/pilot/*/kon.owx` — cross-reference with the disjoint Horn set), classify it with the fast path (default), and confirm its **unsatisfiable-class set** and reportable subsumptions are consistent with the oracle's (`kon.owx`). If no small disjoint-no-functional ont has a pilot oracle, record that (a)+(b)+(c) are the gate and the ORE spot-check was not separately runnable — do not fabricate a diff.

- [ ] **Step 4: Record the result + commit.** Append a short "increment 1 shipped" note (fragment tests updated, canaries green, curated non-regression green, MISSED=0 on the oracle subset, gate fires; standalone DNF-recovery ~0 as expected — foundation for the symmetric increment) to a results doc and commit.

---

## Notes for the implementer

- This is a **gate change**, not an engine change: the `DisjointnessClash` rule and `process_unsat` back-prop already exist. If Task 2's canaries fail, that reveals a real completeness gap in the existing disjointness handling — fix it at the source (`crates/owl-dl-saturation/src/lib.rs` / `proof.rs`), do not weaken the tests.
- Do NOT touch the disjoint+functional path — those 4 onts stay on the hybrid path by design (the no-functional gate). A later increment may prove that interaction.
- Recovery is not the goal here (foundation increment). Do not treat "the giant disjoint onts still DNF" as a failure — 31/39 also need the symmetric increment.
