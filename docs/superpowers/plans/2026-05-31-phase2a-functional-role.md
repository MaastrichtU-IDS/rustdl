# Phase 2a — Functional-Role Inference Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the EL++ functional-role witness-merge rule to the saturation engine, recovering GALEN's `<Region>Pathology ⊑ PathologicalCondition` / `PathologicalCondition` cluster (~50–80 of the 109 MISSED) and any analogous functional-role patterns in the broader corpus, while holding FP=0.

**Architecture:** Single rule added to `crates/owl-dl-saturation/src/lib.rs`. The rule fires inside the existing worklist fixpoint on new `ExistentialFact` arrivals; runtime Tseitin allocation produces synthetic class IDs for the conjoined bodies and incrementally updates the engine's per-class trigger indexes so CR5 propagation picks them up naturally. The empirical verify-before-build discipline (per spec §"Cross-cutting discipline" and dead-end #7) is enforced as a HermiT cross-check on the synthetic canary BEFORE implementation, ensuring the rule we build closes the entailment Konclude/HermiT actually derives.

**Architectural risk worth naming up front:** the rule fires per new fact with an inner loop over (sub's existing facts) × (sub's functional super-roles). On a class with many existential facts and a dense functional-role hierarchy, this can fire combinatorially and either (a) explode runtime, (b) flood the closure with synthetics, or (c) trigger dead-end #7's "sound rule → search-blowup → MISSED" pattern (a Phase 1 baseline regression). Task 6 measures this empirically on alehif (smallest) and GALEN (target); if either shows >5× wall regression, the rule needs a guard (e.g. cap synthetics per `sub` or skip when `target == existing.target`).

**Tech Stack:** Rust (edition 2024), existing `owl-dl-saturation` worklist, `owl-dl-core` IR including pre-existing `Axiom::FunctionalRole(Role)` + `RoleHierarchy::is_sub_role()`, ROBOT+HermiT oracle from Phase 0 (`docker/robot/classify-oracle.sh`), Phase 0 corpus-diff harness.

---

## The rule, stated precisely

**Premise:** the closure contains existential facts `(X, R_i, A)` and `(X, R_j, B)` (i.e., `X ⊑ ∃R_i.A` and `X ⊑ ∃R_j.B`), where `(R_i, A) ≠ (R_j, B)`, and there exists a role `R_f` that is functional and satisfies `R_i ⊑ R_f` and `R_j ⊑ R_f`.

**Conclusion:** `X ⊑ ∃R_f.(A ⊓ B)`. Lowered into the saturator's vocabulary: enqueue a new fact `(X, R_f, F)` where `F` is a Tseitin synthetic for the conjunction `A ⊓ B`, paired with the standard `F ⊑ A`, `F ⊑ B`, and `(A ⊓ B) ⊑ F` (conjunctive-trigger) clauses.

**Soundness sketch:** R_f functional means every X has at most one R_f-successor. R_i ⊑ R_f means every R_i-successor is also an R_f-successor; similarly for R_j. So if X has an R_i-witness `w_i` ∈ A and an R_j-witness `w_j` ∈ B, both `w_i` and `w_j` are R_f-successors of X, and functionality forces `w_i = w_j`. That single witness is in both A and B, hence in A ⊓ B, witnessing X ⊑ ∃R_f.(A ⊓ B). The rule is a textbook EL++ extension (Baader/Brandt/Lutz 2005), introduced specifically to handle role hierarchies with functional super-roles.

**Why this matters:** the GALEN cluster `<Region>Pathology ⊑ PathologicalCondition` derives via this exact pattern with `R_f = StatusAttribute` (functional), `R_i = hasIntrinsicPathologicalStatus`, `R_j = hasPathologicalStatus`, all sibling sub-properties (`docs/handoff-2026-05-30.md` GALEN section). Today rustdl misses the entailment because its EL fragment has no functional-role rule.

---

## Background the executor needs

- The saturator (`crates/owl-dl-saturation/src/lib.rs`, 2058 lines) is one file with a `WorklistEngine` running a fixed-point loop. Existing rules: told subsumption, conjunctive triggers, CR5 existential propagation, CR9 role hierarchy, length-2 role chains, domain/range, Tseitin lowering for static compound `∃` bodies.
- The `ElRules` struct (`lib.rs:680-716`) aggregates per-rule data. The worklist precomputes per-class indexes (`conjunctive_by_body`, `existential_triggers_by_body`, etc.) at engine construction time.
- `TseitinAllocator` (`lib.rs:765-829`) currently runs at COLLECTION TIME (statically), inside `collect_el_rules`. Phase 2a needs RUNTIME allocation; the allocator's `introduce(body, rules)` API can be reused if we plumb it onto `WorklistEngine` and incrementally update the indexes (`conjunctive_by_body`, `subsumed_by` bitsets, `num_total_classes`) when a new synthetic appears.
- `Axiom::FunctionalRole(Role)` already exists in IR (`crates/owl-dl-core/src/convert.rs:362`). `Axiom::InverseFunctionalRole` also exists but is **out of scope** for Phase 2a (potential Phase 2a.1).
- `RoleHierarchy` (`crates/owl-dl-core/src/role_hierarchy.rs`) provides `is_sub_role(sub, sup)` and `sub_roles(r)`, both transitively closed at build. The saturator builds `role_super: HashMap<RoleId, HashSet<RoleId>>` separately (`lib.rs:1303 build_role_super`); the new rule's index `functional_supers_of[r]` reuses that map.
- Workspace lints set `unsafe_code = "warn"` which CI promotes to deny via `RUSTFLAGS="-D warnings"`. No `unsafe` is needed for this plan.
- CI fmt check fails on pre-existing fmt debt in `crates/owl-dl-saturation/src/lib.rs` (lines 1030, 1195, 1204, 1214, 1253, 1264 from Phase 0 final review). **Do not** run `cargo fmt --all` — only fmt files this plan touches AFTER fmt confirmed the touched lines themselves are diff-free.
- The corpus-diff harness is `crates/owl-dl-reasoner/tests/konclude_closure_diff.rs`. GALEN fixture: `ontologies/external/galen.{ofn,-classified.owx}` already on disk. Baseline MISSED: 109 (`docs/handoff-2026-05-30.md`). Recent measurement on this hardware exceeded 30 min wall even at `RUSTDL_HYPER_TRUST_SAT_MIN_MS=0` (`docs/phase1-results.md`) — be prepared for measurement to be wall-bound.

---

## Task 1: Canary — synthetic ontology mimicking the GALEN pattern

**Files:**
- Modify: `crates/owl-dl-saturation/src/lib.rs` (add a new `#[test]` in the existing `#[cfg(test)] mod tests` at the bottom of the file)

The test asserts the *current* gap: the closure does NOT contain the functional-role-merged entailment. The test will be FLIPPED in Task 5 to assert the entailment IS in the closure. This is the "verify-before-build" canary that documents what the rule must close.

- [ ] **Step 1: Locate the test module**

Run: `grep -nE "^#\\[cfg\\(test\\)\\]|^mod tests" crates/owl-dl-saturation/src/lib.rs | head -5`
Confirm the location of the existing `mod tests` block (it ends the file).

- [ ] **Step 2: Add the canary test inside `mod tests`**

The synthetic ontology mirrors the GALEN pattern: a functional super-role `R` with two sibling sub-properties `Ri` and `Rj`, a class `X` that has both `∃Ri.A` and `∃Rj.B` (forcing the witness-merge), a class `T` defined by `∃R.(A ⊓ B) ⊑ T` (the conjunctive-witness consumer). The expected entailment is `X ⊑ T`.

Add inside `mod tests`:

```rust
/// Phase 2a canary: synthetic mimicking GALEN's
/// <Region>Pathology / PathologicalCondition pattern. A functional
/// super-role `r_func` has two sibling sub-properties `r_i` and `r_j`.
/// Class `subject` has existential edges via both sub-properties;
/// class `target` is the conjunctive consumer through `r_func`.
///
/// The expected entailment `subject ⊑ target` requires the EL++
/// functional-role witness-merge rule. This test ASSERTS THE GAP
/// (the entailment is missed) until Phase 2a lands the rule, at
/// which point Task 5 flips the assertion. Do not delete; this
/// canary is the regression test for the rule.
#[test]
fn functional_role_merge_canary_documents_the_gap() {
    use horned_owl::io::ParserConfiguration;
    use horned_owl::io::ofn::reader::read;
    use horned_owl::model::RcStr;
    use horned_owl::ontology::set::SetOntology;
    use owl_dl_core::convert::convert_ontology;
    use std::io::Cursor;

    let src = "\
Prefix(:=<http://rustdl.test/p2a/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://rustdl.test/p2a/test>
    Declaration(Class(:Subject))
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:Target))
    Declaration(ObjectProperty(:r_func))
    Declaration(ObjectProperty(:r_i))
    Declaration(ObjectProperty(:r_j))
    FunctionalObjectProperty(:r_func)
    SubObjectPropertyOf(:r_i :r_func)
    SubObjectPropertyOf(:r_j :r_func)
    SubClassOf(:Subject ObjectSomeValuesFrom(:r_i :A))
    SubClassOf(:Subject ObjectSomeValuesFrom(:r_j :B))
    SubClassOf(ObjectSomeValuesFrom(:r_func ObjectIntersectionOf(:A :B)) :Target)
)
";
    let mut reader = Cursor::new(src);
    let (set_onto, _prefixes): (SetOntology<RcStr>, _) =
        read(&mut reader, ParserConfiguration::default()).expect("canary ontology parses");
    let internal = convert_ontology(&set_onto).expect("canary lowers to IR");
    let subsumers = crate::saturate(&internal);

    // Resolve the IRIs to class ids.
    let subject = internal
        .vocabulary
        .class_id("http://rustdl.test/p2a/Subject")
        .expect("Subject declared");
    let target = internal
        .vocabulary
        .class_id("http://rustdl.test/p2a/Target")
        .expect("Target declared");

    // ASSERT THE GAP: subject ⊑ target is NOT in the closure.
    // When Phase 2a ships, Task 5 inverts this to `assert!(contains, ...)`.
    assert!(
        !subsumers.contains(subject, target),
        "Phase 2a canary unexpectedly passed: the functional-role merge \
         rule appears to be implemented (or the synthetic is wrong). \
         If the rule is in place, invert this assertion. If not, the \
         synthetic doesn't exercise the intended pattern — investigate."
    );
}
```

- [ ] **Step 3: Confirm `vocabulary::class_id` accessor exists**

Run: `grep -nE "fn class_id|pub fn class_id" crates/owl-dl-core/src/vocab.rs crates/owl-dl-core/src/ir.rs | head -5`
Expected: a fn returning `Option<ClassId>` indexed by IRI string. If the accessor is named differently (e.g. `lookup_class`, `class_by_iri`), update the test's two calls accordingly. If no such accessor exists, fall back to walking `internal.vocabulary.classes()` (or its equivalent) and matching by IRI — but check first; this kind of accessor usually exists in well-organized IRs.

- [ ] **Step 4: Run — expect PASS (test asserts the gap)**

Run: `cargo test -p owl-dl-saturation functional_role_merge_canary 2>&1 | tail -10`
Expected: `test functional_role_merge_canary_documents_the_gap ... ok` — the assertion `!subsumers.contains(subject, target)` holds because the rule isn't implemented yet.

If the test FAILS (i.e. the entailment IS in the closure), that's a surprise — the existing CR9 + Tseitin pipeline somehow already covers this pattern. Investigate: print `subsumers.subsumers_of(subject)` and look for `target`. If present, Phase 2a may already be implicitly handled; revisit the plan. If absent, the assertion's negation is wrong — fix.

- [ ] **Step 5: Commit**

```bash
git add crates/owl-dl-saturation/src/lib.rs
git commit -m "test(saturation): Phase 2a canary documenting the functional-role gap"
```

---

## Task 2: HermiT cross-check on the canary synthetic

**Files:**
- Create: `crates/owl-dl-saturation/tests/fixtures/phase2a_functional_role_canary.ofn`
- Reference: `docker/robot/classify-oracle.sh` (Phase 0 Task 1)

The synthetic in Task 1 is hand-built; before implementing the rule, confirm a sound+complete reference reasoner (HermiT via ROBOT) does derive `Subject ⊑ Target` on this exact pattern. If HermiT doesn't, the synthetic doesn't faithfully represent the GALEN pattern and the rule we'd build would close a synthetic but miss the corpus target — exactly the dead-end-#12 failure mode in miniature.

- [ ] **Step 1: Write the same canary ontology to disk in OFN form**

Create `crates/owl-dl-saturation/tests/fixtures/phase2a_functional_role_canary.ofn` with the EXACT same content as the inline string in Task 1, Step 2 (no changes — they must be byte-identical so Task 2's HermiT result IS evidence about Task 1's synthetic):

```
Prefix(:=<http://rustdl.test/p2a/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://rustdl.test/p2a/test>
    Declaration(Class(:Subject))
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:Target))
    Declaration(ObjectProperty(:r_func))
    Declaration(ObjectProperty(:r_i))
    Declaration(ObjectProperty(:r_j))
    FunctionalObjectProperty(:r_func)
    SubObjectPropertyOf(:r_i :r_func)
    SubObjectPropertyOf(:r_j :r_func)
    SubClassOf(:Subject ObjectSomeValuesFrom(:r_i :A))
    SubClassOf(:Subject ObjectSomeValuesFrom(:r_j :B))
    SubClassOf(ObjectSomeValuesFrom(:r_func ObjectIntersectionOf(:A :B)) :Target)
)
```

- [ ] **Step 2: Confirm the fixture path isn't gitignored**

Run: `git check-ignore -v crates/owl-dl-saturation/tests/fixtures/phase2a_functional_role_canary.ofn`
Expected: empty output (file is tracked). If gitignored (unexpected — `ontologies/` is the gitignored tree, not `crates/*/tests/`), report and stop.

- [ ] **Step 3: Run HermiT via the Phase 0 oracle**

Run:
```bash
docker/robot/classify-oracle.sh \
  crates/owl-dl-saturation/tests/fixtures/phase2a_functional_role_canary.ofn \
  /tmp/phase2a-canary-hermit.owx
```
Expected: stderr ends `wrote /tmp/phase2a-canary-hermit.owx`. The first invocation may pull the obolibrary/robot:v1.9.6 image (~600 MB) if not cached.

- [ ] **Step 4: Verify HermiT derives Subject ⊑ Target**

Run:
```bash
grep -E "(SubClassOf|EquivalentClasses)" /tmp/phase2a-canary-hermit.owx \
  | grep -E "Subject|Target" | head -10
```
Expected: at least one line containing both `#Subject` and `#Target` in a `SubClassOf` axiom — the inferred entailment HermiT derived.

**If absent:** the synthetic does NOT exercise the rule HermiT applies. This is a verify-before-build failure. STOP. Two diagnostic moves:
1. Add a redundant explicit existential to give HermiT the same handle: `SubClassOf(:Subject ObjectSomeValuesFrom(:r_func :A))` and re-run. If HermiT now derives it, the merge rule needs the actual functional-super existential to be present, not just sub-property existentials.
2. If still absent, the GALEN pattern is more subtle than the handoff captured — re-read `docs/handoff-2026-05-30.md` GALEN trace, examine the actual GALEN axioms via `grep -E "StatusAttribute|hasIntrinsic|PathologicalCondition" ontologies/external/galen.ofn | head -30`, and refine the synthetic. Do not proceed to Task 3 until HermiT confirms the entailment on the canary.

- [ ] **Step 5: Commit the fixture**

```bash
git add crates/owl-dl-saturation/tests/fixtures/phase2a_functional_role_canary.ofn
git commit -m "fixture(saturation): Phase 2a canary OFN for HermiT cross-check"
```

(Only commit the fixture file. The `/tmp/phase2a-canary-hermit.owx` is throwaway. The HermiT-pass confirmation is recorded in the commit message body — paste the grep output from Step 4 there.)

---

## Task 3: Collect functional-role data into ElRules

**Files:**
- Modify: `crates/owl-dl-saturation/src/lib.rs`

Extend `ElRules` with two fields: the set of functional roles, and per-role precomputed list of its functional super-roles (the index the runtime rule will consult on every new existential fact).

- [ ] **Step 1: Write the failing unit test**

Inside `mod tests`, add:

```rust
#[test]
fn collect_el_rules_records_functional_roles_and_their_supers() {
    use horned_owl::io::ParserConfiguration;
    use horned_owl::io::ofn::reader::read;
    use horned_owl::model::RcStr;
    use horned_owl::ontology::set::SetOntology;
    use owl_dl_core::convert::convert_ontology;
    use std::io::Cursor;

    let src = "\
Prefix(:=<http://rustdl.test/p2a/>)
Ontology(<http://rustdl.test/p2a/funcrole>
    Declaration(ObjectProperty(:r_func))
    Declaration(ObjectProperty(:r_i))
    Declaration(ObjectProperty(:r_j))
    Declaration(ObjectProperty(:r_unrelated))
    FunctionalObjectProperty(:r_func)
    SubObjectPropertyOf(:r_i :r_func)
    SubObjectPropertyOf(:r_j :r_func)
)
";
    let mut reader = Cursor::new(src);
    let (set_onto, _): (SetOntology<RcStr>, _) =
        read(&mut reader, ParserConfiguration::default()).expect("parses");
    let internal = convert_ontology(&set_onto).expect("lowers");
    let role_super = crate::build_role_super(&internal);
    let (rules, _num_total) = crate::collect_el_rules(&internal, &role_super);

    let id = |iri: &str| {
        internal
            .vocabulary
            .role_id(iri)
            .expect("role declared")
    };
    let rf = id("http://rustdl.test/p2a/r_func");
    let ri = id("http://rustdl.test/p2a/r_i");
    let rj = id("http://rustdl.test/p2a/r_j");
    let ru = id("http://rustdl.test/p2a/r_unrelated");

    assert!(rules.is_functional(rf), "r_func is declared functional");
    assert!(!rules.is_functional(ri));
    assert!(!rules.is_functional(rj));
    assert!(!rules.is_functional(ru));

    let supers = |r| rules.functional_supers_of(r).to_vec();
    assert_eq!(supers(ri), vec![rf], "r_i ⊑ r_func and r_func is functional");
    assert_eq!(supers(rj), vec![rf], "r_j ⊑ r_func");
    assert_eq!(supers(rf), vec![rf], "r_func is its own super (reflexive)");
    assert!(supers(ru).is_empty(), "r_unrelated has no functional super");
}
```

The accessors `vocabulary::role_id`, `ElRules::is_functional`, and `ElRules::functional_supers_of` don't exist yet. The test will fail to compile, which is the first red.

- [ ] **Step 2: Run — fail to compile**

Run: `cargo test -p owl-dl-saturation collect_el_rules_records_functional 2>&1 | tail -15`
Expected: compile errors — `no method named 'is_functional'`, `no method named 'functional_supers_of'`. Also possibly `no method named 'role_id'` on vocabulary if it's named differently — if so, look at how Task 1 resolves class IRIs (`class_id`) and use the parallel role accessor. Update the test to use the correct name.

- [ ] **Step 3: Add the fields and accessors to `ElRules`**

In `crates/owl-dl-saturation/src/lib.rs`, find `struct ElRules` (around line 680). Add two fields immediately after the existing ones (before the closing `}`):

```rust
/// Roles declared `FunctionalObjectProperty(...)`. Indexed by role
/// id (dense bitset for O(1) lookup). Phase 2a EL++ rule input.
functional_roles: FixedBitSet,
/// Per-role precomputed list of FUNCTIONAL super-roles in the
/// transitive closure: `functional_supers_of[r]` lists every
/// functional role `R_f` such that `r ⊑ R_f` (reflexive: r itself
/// if functional). Precomputed once at collection time so the
/// runtime worklist rule doesn't re-walk role_super on every new
/// existential fact. Empty for roles with no functional ancestor.
functional_supers_of: Vec<Vec<RoleId>>,
```

Note: `FixedBitSet` is already used elsewhere in `WorklistEngine` (`lib.rs:160`); `use fixedbitset::FixedBitSet;` may already be imported at the top — check with `grep -n "use fixedbitset" crates/owl-dl-saturation/src/lib.rs`.

Then add the accessors just above the `#[derive]`-d `ElRules` struct definition, OR inside an `impl ElRules` block (introduce one if it doesn't exist):

```rust
impl ElRules {
    /// True if `r` is declared `FunctionalObjectProperty`.
    fn is_functional(&self, r: RoleId) -> bool {
        let i = r.index() as usize;
        i < self.functional_roles.len() && self.functional_roles.contains(i)
    }

    /// Precomputed: every functional role `R_f` with `r ⊑ R_f`.
    /// Empty slice if `r` has no functional ancestor.
    fn functional_supers_of(&self, r: RoleId) -> &[RoleId] {
        let i = r.index() as usize;
        self.functional_supers_of.get(i).map(Vec::as_slice).unwrap_or(&[])
    }
}
```

(If `RoleId::index()` returns `u32` instead of `usize`, cast appropriately. Check the existing `id.index() as usize` pattern at `lib.rs:147`.)

- [ ] **Step 4: Populate the fields in `collect_el_rules`**

Find `fn collect_el_rules` (`lib.rs:831`). Add a NEW pass (after the existing axiom-walking pass that populates `disjoint_pairs`, `role_domains`, etc., but before the return) that:

1. Collects `FunctionalRole` axioms into `rules.functional_roles`.
2. Precomputes `rules.functional_supers_of` from `role_super` filtered by `functional_roles`.

Find the spot at the end of `collect_el_rules` just before the `return (rules, ...)`. Add:

```rust
    // Phase 2a: collect functional-role declarations and precompute
    // the per-role list of functional super-roles (the index the
    // runtime witness-merge rule consults on every new existential
    // fact arrival).
    let num_roles = internal.vocabulary.num_roles();
    rules.functional_roles = FixedBitSet::with_capacity(num_roles);
    for ax in &internal.axioms {
        if let Axiom::FunctionalRole(role) = ax
            && !role.is_inverse()
        {
            rules.functional_roles.insert(role.role_id().index() as usize);
        }
    }
    rules.functional_supers_of = vec![Vec::new(); num_roles];
    for r_idx in 0..num_roles {
        let r = RoleId::new(u32::try_from(r_idx).expect("role id fits in u32"));
        let mut supers: Vec<RoleId> = role_super
            .get(&r)
            .map(|s| s.iter().copied().collect())
            .unwrap_or_default();
        supers.retain(|s| rules.is_functional(*s));
        supers.sort_unstable_by_key(|s| s.index());
        rules.functional_supers_of[r_idx] = supers;
    }
```

Note: `role.is_inverse()` filter is conservative — Phase 2a handles only forward (non-inverse) functional roles; `Axiom::InverseFunctionalRole` and inverse-role functionality is explicitly out of scope (potential Phase 2a.1).

If `internal.vocabulary.num_roles()` doesn't exist, find the equivalent accessor; the saturator already needs role count for `build_role_super` (`lib.rs:1303`) — match that idiom.

- [ ] **Step 5: Initialize `functional_roles` in `ElRules::default`**

`ElRules` derives `Default` (`lib.rs:680`). `FixedBitSet::default()` returns an empty bitset, which is correct (no functional roles before population). `Vec::default()` is empty too. No code change needed for Default; just confirm.

- [ ] **Step 6: Run — pass**

Run: `cargo test -p owl-dl-saturation collect_el_rules_records_functional 2>&1 | tail -15`
Expected: test passes.

- [ ] **Step 7: Soundness regression check**

Run: `cargo test -p owl-dl-saturation 2>&1 | tail -10`
Expected: all existing saturation tests still pass. The new fields are populated but not yet consulted by any rule, so verdicts must be unchanged.

- [ ] **Step 8: Clippy + CI strictness**

```bash
RUSTFLAGS="-D warnings" cargo test -p owl-dl-saturation --no-run 2>&1 | tail -5
cargo clippy -p owl-dl-saturation -- -D warnings 2>&1 | grep -E "warning|error.*owl-dl-saturation" | head -10
```
Expected: clean compile; clippy on the new code clean. (Pre-existing `too_many_lines` warnings in this file are out of scope — don't touch them.)

- [ ] **Step 9: Commit**

```bash
git add crates/owl-dl-saturation/src/lib.rs
git commit -m "feat(saturation): collect functional-role declarations + per-role functional-super index"
```

---

## Task 4: Wire the functional-role merge rule into the worklist

**Files:**
- Modify: `crates/owl-dl-saturation/src/lib.rs` (the `WorklistEngine` step that processes a new `ExistentialFact`)

This is the load-bearing change. The rule fires when a fact `(sub, role, target)` enters the worklist; for each functional super-role `R_f` of `role`, scan the OTHER facts `(sub, role_j, target_j)` where `R_f` is also a functional super of `role_j` AND the new fact isn't equal to the existing one, allocate the Tseitin synthetic `F` for `{target, target_j}`, and enqueue `(sub, R_f, F)`.

- [ ] **Step 1: Locate the existential-fact processing loop**

Run: `grep -nE "fn process_fact|todo_fact|ExistentialFact" crates/owl-dl-saturation/src/lib.rs | head -20`
Expected: find the function/loop that dequeues from `todo_fact` and processes new facts (the spot where CR5 chain rule fires, role-hierarchy CR9 fires, range propagation, etc). The new rule fires alongside these.

- [ ] **Step 2: Plumb the runtime Tseitin allocator onto `WorklistEngine`**

In `struct WorklistEngine` (around `lib.rs:97`), add:

```rust
/// Runtime Tseitin allocator for synthetic class IDs introduced by
/// the Phase 2a functional-role witness-merge rule. Allocated
/// dynamically during the fixpoint (unlike the collection-time
/// allocator in `collect_el_rules` which only handles statically-
/// visible compound bodies). Pairs `(target_i, target_j)` are
/// deduplicated by sorted body.
tseitin_runtime: TseitinAllocator,
```

In `WorklistEngine::new` (around `lib.rs:137`), initialize:

```rust
tseitin_runtime: TseitinAllocator::new(num_total_classes),
```

Note: `TseitinAllocator::new(start)` (from `lib.rs:776`) takes the "first synthetic id" — pass `num_total_classes`, which is the post-collection-time total (includes any static synthetics). New runtime synthetics get IDs above that.

This means `num_total_classes` is no longer the true total — runtime synthetics push past it. The `subsumers` and `subsumed_by` bitsets are sized by `num_total_classes` at construction (`lib.rs:159-162`). We need to grow them when a runtime synthetic appears (Step 4 below).

- [ ] **Step 3: Add a runtime-synthetic introduction helper on `WorklistEngine`**

Add this method on `impl WorklistEngine` (just after `new` for proximity):

```rust
/// Introduce a runtime Tseitin synthetic for the conjunction
/// `body[0] ⊓ … ⊓ body[k-1]` (where body is a deduplicated sorted
/// list of atomic class ids). Returns the synthetic class id.
///
/// Beyond `TseitinAllocator::introduce` (which would only mutate
/// `self.rules`), this method also:
/// - Grows `self.subsumers` and `self.subsumed_by` bitsets to fit
///   the new id.
/// - Updates `self.conjunctive_by_body` to include the new
///   conjunctive trigger.
/// - Enqueues `synthetic ⊑ body[i]` subsumptions into
///   `self.todo_subsumer` for the F ⊑ Bi clauses so CR5 picks
///   them up in the next iteration of the fixpoint.
///
/// Deduplication via the embedded allocator: passing the same
/// body twice returns the same synthetic id without allocating.
fn introduce_runtime_synthetic(&mut self, body: Vec<ClassId>) -> ClassId {
    // Snapshot rule lengths so we can detect what introduce() added.
    let before_atomic = self.rules.atomic_subsumptions.len();
    let before_conjunctive = self.rules.conjunctive_triggers.len();
    let synthetic = self.tseitin_runtime.introduce(body.clone(), &mut self.rules);
    let s_idx = synthetic.index() as usize;
    let added_atomic = self.rules.atomic_subsumptions.len() - before_atomic;
    let added_conjunctive = self.rules.conjunctive_triggers.len() - before_conjunctive;
    if added_atomic == 0 && added_conjunctive == 0 {
        // Deduplicated — synthetic already exists, nothing to wire.
        return synthetic;
    }
    // Grow bitsets if the synthetic id exceeded the static capacity.
    let needed = s_idx + 1;
    if needed > self.num_total_classes {
        for bs in &mut self.subsumers.subsumers {
            bs.grow(needed);
        }
        for bs in &mut self.subsumed_by {
            bs.grow(needed);
        }
        // Add fresh bitsets/rows for the new id(s).
        while self.subsumers.subsumers.len() < needed {
            self.subsumers.subsumers.push(FixedBitSet::with_capacity(needed));
        }
        while self.subsumed_by.len() < needed {
            self.subsumed_by.push(FixedBitSet::with_capacity(needed));
        }
        while self.facts_by_sub.len() < needed {
            self.facts_by_sub.push(Vec::new());
        }
        while self.facts_by_target.len() < needed {
            self.facts_by_target.push(Vec::new());
        }
        while self.conjunctive_by_body.len() < needed {
            self.conjunctive_by_body.push(Vec::new());
        }
        while self.existential_triggers_by_body.len() < needed {
            self.existential_triggers_by_body.push(Vec::new());
        }
        while self.disjoints_by_class.len() < needed {
            self.disjoints_by_class.push(Vec::new());
        }
        self.num_total_classes = needed;
    }
    // Index the new conjunctive triggers (introduce adds at most one).
    for added_idx in before_conjunctive..self.rules.conjunctive_triggers.len() {
        let trigger = &self.rules.conjunctive_triggers[added_idx];
        for &b in &trigger.bodies {
            self.conjunctive_by_body[b.index() as usize].push(added_idx);
        }
    }
    // Enqueue the F ⊑ Bi subsumptions so CR5 propagates them.
    for added_idx in before_atomic..self.rules.atomic_subsumptions.len() {
        let sub_ax = self.rules.atomic_subsumptions[added_idx];
        self.todo_subsumer.push_back((sub_ax.sub, sub_ax.sup));
    }
    synthetic
}
```

This is the trickiest piece of Phase 2a. The pattern: the allocator mutates `self.rules`; we observe what it added; we incrementally update every index that's a function of `self.rules`. The bitset growth uses `FixedBitSet::grow` (in-place) for existing rows and `with_capacity` for new rows.

**Note on `Subsumers.subsumers`**: it's accessed as `self.subsumers.subsumers` (the inner `Vec<FixedBitSet>`) — confirm the public/private accessibility. If `subsumers` is private, add a small `pub(crate) fn grow_to(&mut self, n: usize)` method on `Subsumers` and call that. The existing access pattern at `lib.rs:189-196` shows `self.subsumers.subsumers.get(ci)` works, so the field is at least `pub(crate)`.

- [ ] **Step 4: Add the rule into the fact-processing loop**

In the body of the function that processes a new fact (found in Step 1; typically iterating over `todo_fact` or called per dequeue), add the functional-role merge rule AT THE END of the per-fact processing (so all existing rules — CR5, CR9, range — still get to fire first):

```rust
// Phase 2a EL++ functional-role witness-merge rule.
// If the new fact is (sub, role, target) and there exists a
// functional super-role R_f of `role`, then for every other fact
// (sub, role_j, target_j) where R_f is also a functional super of
// role_j AND (role_j, target_j) != (role, target), derive
// (sub, R_f, F) where F is the runtime Tseitin synthetic for
// {target, target_j}. Soundness: R_f functional => the R_i and R_j
// witnesses coincide; that single R_f-witness is in target ∩ target_j.
let funcs = self.rules.functional_supers_of(role).to_vec();
if !funcs.is_empty() {
    // Snapshot the existing facts on `sub` once; we don't iterate
    // a vector we may mutate (introduce_runtime_synthetic +
    // enqueue happen below).
    let sub_idx = sub.index() as usize;
    let existing_fact_idxs: Vec<usize> =
        self.facts_by_sub.get(sub_idx).cloned().unwrap_or_default();
    for &existing_idx in &existing_fact_idxs {
        let existing = self.facts[existing_idx];
        // Skip self-comparison.
        if existing.role == role && existing.target == target {
            continue;
        }
        // For each functional super-role they share, fire the rule.
        for &rf in &funcs {
            let other_funcs = self.rules.functional_supers_of(existing.role);
            if !other_funcs.contains(&rf) {
                continue;
            }
            // Allocate synthetic for {target, existing.target}.
            // (introduce_runtime_synthetic dedups via sorted body.)
            let body = vec![target, existing.target];
            let synthetic = self.introduce_runtime_synthetic(body);
            // Enqueue the derived fact (sub, rf, synthetic).
            let fact = ExistentialFact { sub, role: rf, target: synthetic };
            if self.seen_facts.insert((fact.sub, fact.role, fact.target)) {
                let new_idx = self.facts.len();
                self.facts.push(fact);
                self.facts_by_sub[fact.sub.index() as usize].push(new_idx);
                self.facts_by_target[fact.target.index() as usize].push(new_idx);
                self.todo_fact.push_back(new_idx);
            }
        }
    }
}
```

The triple-loop is `O(|facts_on_sub| × |functional_supers|²)` per new fact — acceptable for the GALEN scale where most classes have few facts and few functional roles.

**Important:** the snapshot at `existing_fact_idxs: Vec<usize>` (a `.cloned()` of the slice) is necessary because `introduce_runtime_synthetic` may push to `self.facts_by_sub[sub_idx]` (when the new derived fact has `fact.sub == sub`), invalidating an iterator. The `.cloned()` snapshots once.

- [ ] **Step 5: Run all saturation tests**

Run: `cargo test -p owl-dl-saturation 2>&1 | tail -15`
Expected: all tests pass INCLUDING the Task 1 canary — wait, the canary asserts the GAP. With the rule now implemented, the canary should now FAIL (the entailment IS in the closure). That's the trigger for Task 5.

If the canary STILL passes (`!subsumers.contains(subject, target)` still true), the rule didn't fire — investigate. Debug pointers: print `rules.functional_roles`, `rules.functional_supers_of(r_i)`, and the facts list after `saturate()`. Likely causes: (a) `FunctionalRole` axiom not lowered correctly in Step 3; (b) `role_super` doesn't include the sub-role declarations; (c) the rule's `existing.role == role` self-check is too strict.

If a non-canary test fails, that's a real soundness/completeness regression — DO NOT proceed; localize first.

- [ ] **Step 6: CI strictness**

```bash
RUSTFLAGS="-D warnings" cargo test -p owl-dl-saturation --no-run 2>&1 | tail -5
cargo clippy -p owl-dl-saturation -- -D warnings 2>&1 | grep -E "warning|error.*owl-dl-saturation" | head -10
```
Expected: clean. Pre-existing `too_many_lines` warnings may now be aggravated (this task adds ~50 lines to a method); if a NEW clippy warning fires on the new code, fix it (extract a helper). If clippy yells about ONLY the pre-existing issue, leave it.

- [ ] **Step 7: Commit (canary expected to fail at this commit — that's intentional)**

```bash
git add crates/owl-dl-saturation/src/lib.rs
git commit -m "feat(saturation): EL++ functional-role witness-merge rule (Phase 2a)

For roles R_i, R_j with a common functional super-role R_f, when a class
X has both ∃R_i.A and ∃R_j.B, derive X ⊑ ∃R_f.(A ⊓ B) via a runtime
Tseitin synthetic. Standard EL++ extension (Baader/Brandt/Lutz 2005).

The Phase 2a canary test (functional_role_merge_canary_documents_the_gap)
is INTENTIONALLY FAILING after this commit — it asserts the closure does
NOT contain the entailment. Task 5 inverts the assertion in the next
commit."
```

---

## Task 5: Flip the canary; soundness regression sweep

**Files:**
- Modify: `crates/owl-dl-saturation/src/lib.rs` (one assertion in the canary test)

- [ ] **Step 1: Invert the canary assertion**

Open the canary test from Task 1. Change:

```rust
    assert!(
        !subsumers.contains(subject, target),
        "Phase 2a canary unexpectedly passed: ..."
    );
```

To:

```rust
    assert!(
        subsumers.contains(subject, target),
        "Phase 2a regression: the functional-role witness-merge rule \
         failed to derive Subject ⊑ Target. The rule, the role-hierarchy \
         index, or the runtime Tseitin allocator likely regressed."
    );
```

Also update the doc comment on the test — remove the "ASSERTS THE GAP" sentence and replace with "ASSERTS THE FIX (Phase 2a rule active)."

Rename the test from `functional_role_merge_canary_documents_the_gap` to `functional_role_merge_canary_recovers_entailment` to reflect the new role. (The renaming is cosmetic but documents the lifecycle for future readers.)

- [ ] **Step 2: Run — pass**

Run: `cargo test -p owl-dl-saturation functional_role_merge_canary 2>&1 | tail -10`
Expected: `functional_role_merge_canary_recovers_entailment ... ok`.

- [ ] **Step 3: Run ALL saturation tests + lib tests + CI strictness**

```bash
RUSTFLAGS="-D warnings" cargo test -p owl-dl-saturation 2>&1 | tail -10
RUSTFLAGS="-D warnings" cargo test -p owl-dl-reasoner --lib 2>&1 | tail -10
```
Expected: every saturation test passes. Every reasoner lib test passes (78 from Phase 1).

A failure on a reasoner lib test is the dead-end-#4 / dead-end-#7 pattern: a sound rule may regress completeness via search blowup or label-set explosion. Localize: identify the failing test, examine its ontology, check whether the new rule fires on it inappropriately. Fix by tightening the rule's preconditions (e.g. require `target != existing.target` to avoid trivial X ⊑ ∃R_f.(A ⊓ A) = X ⊑ ∃R_f.A, which is already entailed via CR9).

- [ ] **Step 4: Commit**

```bash
git add crates/owl-dl-saturation/src/lib.rs
git commit -m "test(saturation): flip Phase 2a canary to assert rule recovers entailment"
```

---

## Task 6: Corpus-diff measurement on GALEN + Phase 0 net

**Files:**
- No new files. Uses `crates/owl-dl-reasoner/tests/konclude_closure_diff.rs` from Phase 0 + the existing fixtures.

The empirical question: how many of GALEN's 109 MISSED does Phase 2a recover? Spec target: 50–80. Soundness gate: FP must stay 0 across the broadened net (the ORE fixtures + alehif + pizza/ro/sulo).

- [ ] **Step 1: Baseline check — confirm pre-Phase-2a state is recorded**

Read `docs/phase1-results.md` for the most recent GALEN baseline (likely "not measured" — the Phase 1 sweep exceeded the wall budget). Confirm there's no fresher baseline.

Run a quick fast-fixture run to confirm Phase 0 net still holds FP=0 with the new code (NO env vars; default behaviour):
```bash
cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    alehif_closure_matches_konclude ore_10908_sroiq ore_15672_shoin \
    -- --ignored --nocapture 2>&1 | tee /tmp/phase2a-net.log | grep -E "^---|^test "
```
Expected: each fixture line ends `FP=0 MISSED=0` matching the Phase 0 baseline. If FP > 0 on any fixture — STOP. Phase 2a introduced a soundness bug. The dead-end-#12 frame-test is `rustdl classify --saturation-only ontologies/external/<bad>.ofn` — if the FP persists under saturation-only, it's in the new rule.

If alehif's wall blew up substantially (it's a SHOIQ ontology; the new rule fires whenever it sees two existentials on the same class via siblings of a functional role), record that as a perf concern but don't block. The Phase 0 baseline for alehif was 1.76 s; up to ~3–4 s is normal; >30 s suggests the new rule is firing combinatorially.

- [ ] **Step 2: GALEN measurement — hard wall cap**

GALEN's `_matches_konclude` test uses 200 ms per-pair timeout. The Phase 1 measurement showed >30 min wall on this hardware even at threshold=0. With Phase 2a's added rule firing, expect MORE work per pair. Run with a hard cap:

```bash
timeout 2400 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    galen_closure_matches_konclude \
    -- --ignored --nocapture 2>&1 | tee /tmp/phase2a-galen.log | grep -E "^--- galen|^test galen"
```

(2400 s = 40 min hard cap.) Expected line: `--- galen (<wall>) --- rustdl_closure=N konclude_closure=M FP=F MISSED=X (...)`.

- **Goal:** FP=0; MISSED drops substantially from the 109 baseline (target 50–80 reduction, leaving MISSED ≤ 60). Wall should stay in the minutes range; if it blows past 40 min the rule has a perf bug (combinatorial firing on a class with many sibling-role existentials).
- **If timeout:** record "TIMEOUT(>40 min)" and continue. The MISSED-reduction result is still measurable on a smaller fixture (Step 3), but the GALEN-specific spec target can't be verified.

- [ ] **Step 3: ALEHIF + ORE re-check at default**

Confirm one more time the soundness net holds with Phase 2a active (FP=0 across the smaller fixtures). Already done in Step 1, but if anything looks off, re-run:

```bash
cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    alehif_closure_matches_konclude ore_10908_sroiq ore_15672_shoin \
    -- --ignored --nocapture 2>&1 | tail -15
```

Expected: FP=0 on all three; MISSED unchanged or smaller. Any FP > 0 = block + investigate.

- [ ] **Step 4: Triage notes (no commit yet — Task 7 writes the results doc)**

Capture:
- Phase 2a GALEN result: FP, MISSED, wall.
- Phase 2a ALEHIF + ORE result: FP=0 confirmed, MISSED counts.
- Whether the spec target (GALEN MISSED 109 → 50–80 reduction) was hit.
- Any unexpected wall regressions on the small fixtures.

---

## Task 7: Results doc + cross-link

**Files:**
- Create: `docs/phase2a-results.md`
- Modify: `docs/fragment-completeness.md` (the "Provably complete fragment" section now needs the EL++ functional-role rule added).
- Modify: `CLAUDE.md` (the saturator description in "Workspace architecture" now includes the new rule).
- Modify: `docs/superpowers/specs/2026-05-31-soundness-completeness-perf-design.md` Phase 2 section: append a "Landed: ..." pointer to phase2a-results.md.

- [ ] **Step 1: Write `docs/phase2a-results.md`**

Use the Phase 1 results doc as a structural template; fill values from Task 6:

```markdown
# Phase 2a — Functional-role inference results

Run 2026-MM-DD against the Phase 0 soundness net + GALEN. Mechanism:
EL++ functional-role witness-merge rule added to the saturator
(`crates/owl-dl-saturation/src/lib.rs`, see commit history for
`feat(saturation): EL++ functional-role witness-merge rule`). See
`docs/superpowers/plans/2026-05-31-phase2a-functional-role.md`.

## Soundness gate (Phase 0 net)

| Fixture | FP | MISSED (pre-2a) | MISSED (Phase 2a) | Wall | Outcome |
|---|---|---|---|---|---|
| alehif | 0 | 0 | <m> | <s> | <PASS / regression>|
| ore-10908-sroiq | 0 | 0 | <m> | <s> | <pass>|
| ore-15672-shoin | 0 | 0 | <m> | <s> | <pass>|

FP=0 held across N/N fixtures.

## Completeness lever (GALEN)

| Threshold | FP | MISSED | Wall | Delta vs baseline (109) |
|---|---|---|---|---|
| Phase 2a default | <fp> | <missed> | <wall> | -<n> MISSED (<pct>%) |

**Interpretation:** <did the spec target (50-80 MISSED reduction) land?
If yes, by how much; if not, by how close and what's likely outstanding
(the spec also flagged "<Region>Pathology" as ~50-80 of 109, with
PairedBodyStructure as ~20-30 — Phase 2b's job).>

## Honesty paragraph

<If GALEN couldn't be measured due to wall cap, say so explicitly:
"GALEN measurement timed out at <cap> min wall on this hardware.
The synthetic canary confirms the rule fires correctly; MISSED-reduction
on GALEN remains to be measured on faster hardware or with a smaller
per-pair budget."

If MISSED-reduction was less than expected, own it: the rule fires
correctly on the canary but doesn't close as much of GALEN's MISSED
as the handoff estimated; investigate which GALEN pairs are NOT
covered.>

## How to re-run

```bash
# Canary (fast — confirms the rule is wired):
cargo test -p owl-dl-saturation functional_role_merge_canary -- --nocapture

# Soundness net (the FP=0 gate):
cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    alehif_closure_matches_konclude ore_10908_sroiq ore_15672_shoin \
    -- --ignored --nocapture

# GALEN MISSED measurement (slow):
cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    galen_closure_matches_konclude -- --ignored --nocapture
```
```

- [ ] **Step 2: Update `docs/fragment-completeness.md`**

Find the "## Provably complete fragment" section. Add to the list of supported EL constructs: "EL++ functional-role witness-merge (Phase 2a — see `phase2a-results.md`): if `R_i, R_j ⊑ R_f` and `R_f` is functional, `X ⊑ ∃R_i.A ⊓ ∃R_j.B` implies `X ⊑ ∃R_f.(A ⊓ B)`." Keep the addition brief; the proof is standard EL++ (cite Baader/Brandt/Lutz 2005).

- [ ] **Step 3: Update CLAUDE.md saturator description**

Find the `crates/owl-dl-saturation` bullet in the "Workspace architecture" section. Append to the list of supported features: "EL++ functional-role witness-merge (Phase 2a) for sibling sub-properties of a functional role."

- [ ] **Step 4: Cross-link from the design spec**

In `docs/superpowers/specs/2026-05-31-soundness-completeness-perf-design.md`, find the "### Phase 2 — Deep completeness calculus" section. Under the `2a` bullet, append: `Landed as `[`docs/phase2a-results.md`](../../phase2a-results.md)`.`

- [ ] **Step 5: Commit**

```bash
git add docs/phase2a-results.md \
        docs/fragment-completeness.md \
        CLAUDE.md \
        docs/superpowers/specs/2026-05-31-soundness-completeness-perf-design.md
git commit -m "docs(phase2a): results doc + envelope/CLAUDE.md updates for functional-role rule"
```

---

## Definition of done (Phase 2a)

- `Axiom::FunctionalRole` flows into `ElRules::functional_roles`; `functional_supers_of(r)` index is precomputed and unit-tested (Task 3).
- The EL++ witness-merge rule is wired into the worklist fact-processing loop with runtime Tseitin allocation; the synthetic canary recovers the entailment (Tasks 4–5).
- HermiT cross-check confirms the canary's pattern matches what a sound+complete reasoner derives (Task 2).
- Phase 0 soundness net holds FP=0 under Phase 2a's default behaviour (Task 6).
- GALEN measurement recorded — either MISSED reduction toward the 50–80 target OR an explicit "couldn't be measured on this hardware at this budget" honesty paragraph (Task 6 + Task 7).
- `docs/fragment-completeness.md` adds EL++ functional-role to the provably-complete fragment; `CLAUDE.md` describes the new rule; design spec cross-links the results doc (Task 7).

This unblocks Phase 2b (≥n + disjointness). Phase 2b's targeting depends on what Phase 2a closed — if Phase 2a hits the upper end of the 50–80 target, Phase 2b's `PairedBodyStructure` cluster is most of what remains; if Phase 2a lands lower, the cluster split may differ.
