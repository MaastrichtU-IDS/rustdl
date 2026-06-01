# Phase 2c — Functional-Role + Covering EL+ Approximation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an EL+ approximation rule to the saturator that closes the cluster-C/D residual MISSED (24-44 pairs of the post-Phase-2b/2b.5 + Phase-3 baseline) via pattern-matching the functional-role + covering triangle in absorbed-TBox shape, without negation / case-splitting / hypertableau extension. Hold FP=0 + the post-Phase-3 wall baseline.

**Architecture:** Single-crate change to `crates/owl-dl-saturation/src/lib.rs`. The triangle pattern (per Phase 2c.0 diagnosis + Phase 2b.0 pair-06 analysis) is: a GCI of shape `LHS_body ⊓ ∃R_i.A ⊑ ∃R_j.B` where (i) `R_i ⊑ R_f`, `R_j ⊑ R_f`, `R_f` functional; (ii) there's a covering or disjointness forcing `A` and `B` mutually exclusive on `R_f`'s domain. The materialization: `LHS_body ⊑ ∃R_j.B` — drop the conditional `∃R_i.A` operand. T3's design step finalizes the exact detection algorithm and soundness conditions based on reading the absorbed-TBox surface; T4 implements; T5 measures.

**Tech Stack:** Rust (edition 2024), `owl-dl-saturation` crate, existing Phase 2a functional-role infrastructure (`ElRules::functional_roles`, `functional_supers_of`), Phase 0 corpus-diff harness.

---

## Background the executor needs

- Phase 2c.0 diagnosis (`docs/phase2c-galen-diagnosis.md`, committed 78413b9) confirmed the 17 GALEN + 27 notgalen residual MISSED predominantly match Phase 2b.0's cluster C (functional-role + covering / sibling-collapse). Scope estimate: 24-pair confident floor (pure cluster C), 39 most-likely (with notgalen anonymous super-classes), 44 upper bound.
- Phase 2b.0 pair-06 analysis (`docs/phase2b-galen-pair-analysis.md` §"Pair 06") IS the canonical pair for this rule. The relevant GALEN axioms (excerpted):
  ```
  EquivalentClasses(:IneffectiveCardiacFunction
    ObjectIntersectionOf(:CardiacFunction
      ObjectSomeValuesFrom(:hasEffectiveness
        ObjectIntersectionOf(:Effectiveness ObjectSomeValuesFrom(:hasState :ineffective)))))
  EquivalentClasses(:IntrinsicallyPathologicalBodyProcess
    ObjectIntersectionOf(:BodyProcess
      ObjectSomeValuesFrom(:hasIntrinsicPathologicalStatus :pathological)))
  FunctionalObjectProperty(:StatusAttribute)
  SubObjectPropertyOf(:hasIntrinsicPathologicalStatus :StatusAttribute)
  SubObjectPropertyOf(:hasPathologicalStatus :StatusAttribute)
  SubClassOf(:pathological :PathologicalOrPhysiologicalStatus)
  # The conditional GCI:
  SubClassOf(
    ObjectIntersectionOf(:BodyProcess
      ObjectSomeValuesFrom(:hasEffectiveness
        ObjectIntersectionOf(:Effectiveness ObjectSomeValuesFrom(:hasState :ineffective)))
      ObjectSomeValuesFrom(:hasIntrinsicPathologicalStatus :physiological))
    ObjectSomeValuesFrom(:hasPathologicalStatus :pathological))
  ```
- HermiT's derivation (per the per-pair analysis): non-Horn case-splits on `IneffectiveCardiacFunction`'s `hasIntrinsicPathologicalStatus` value. The `physiological` branch contradicts via functional-StatusAttribute sibling-collapse + covering on `PathologicalOrPhysiologicalStatus`; only the `pathological` branch survives, hence `IneffectiveCardiacFunction ⊑ ∃hasIntrinsicPathologicalStatus.pathological`.
- Phase 2a's functional-role infrastructure already exists in the saturator: `ElRules::functional_roles: FixedBitSet` and `functional_supers_of: Vec<Vec<RoleId>>` (Phase 2a T3, commit de196c9). The witness-merge rule itself (Phase 2a T4/T4.5) operates on existential facts; the Phase 2c rule operates differently — on absorbed-TBox GCI shape, not facts.
- The minimal HermiT-verified pair_06 module (`crates/owl-dl-reasoner/tests/fixtures/phase2b/pair_06.ofn` + `.hermit.owx`, committed 654a8de) IS the verify-before-build artifact. No separate HermiT cross-check needed for the canary if it mirrors pair_06's shape.
- Phase 2c.0 scope: 24-44 of 44 residual recovery. **The plan does not promise 44.** The Phase 2c implementation outline (per the diagnosis):
  1. Build synthetic canary mirroring pair-06.
  2. Pattern detection in absorption.
  3. TDD canary for the rule firing.
  4. Implement the lowering.
  5. Corpus measurement.
  6. Results doc.

This plan follows that outline exactly.

---

## Task 1: Synthetic canary documenting the gap

**Files:**
- Modify: `crates/owl-dl-saturation/src/lib.rs` (add a `#[test]` in `mod tests`).

The synthetic ontology mirrors pair_06's structure abstractly:
- A functional super-role `R_f` with two sibling sub-properties `R_i` and `R_j`.
- A class `X` defined to imply `∃R_i.A` (via a conjunction or sub-class chain).
- A class `T` defined as `... ⊓ ∃R_j.B`.
- A covering: `A ⊓ B ⊑ ⊥` (or `Z ⊑ A ⊔ B` plus disjointness).
- A conditional GCI: `LHS_body ⊓ ∃R_i.A ⊑ ∃R_j.B`.

Expected entailment: `X ⊑ T` (currently missed because the saturator can't do the case-split).

- [ ] **Step 1: Locate `mod tests`**

```bash
grep -nE "^#\\[cfg\\(test\\)\\]|^mod tests" crates/owl-dl-saturation/src/lib.rs | head -5
```
The existing Phase 2a + 2b canaries live in `mod tests` near the bottom of the file. Add the new canary adjacent.

- [ ] **Step 2: Write the canary test**

Place after the Phase 2a/2b canaries:

```rust
/// Phase 2c canary: synthetic mirroring GALEN pair_06's
/// `CongestiveCardiacFailure ⊑ IntrinsicallyPathologicalBodyProcess`
/// pattern (functional-role + covering / sibling-collapse).
///
/// Shape:
///   T ≡ A_super ⊓ ∃R_j.B
///   X ⊑ A_super ⊓ ∃R_i.A      (the LHS_body, without the conditional ∃R_f.X-side)
///   R_i ⊑ R_f                  (sub-property of functional super)
///   R_j ⊑ R_f
///   FunctionalObjectProperty(R_f)
///   A ⊓ B ⊑ ⊥                  (mutually exclusive — the covering)
///   LHS_body ⊓ ∃R_i.A ⊑ ∃R_j.B  (the conditional GCI; this is the case-split target)
///
/// HermiT derives X ⊑ T via case-split on X's R_i value: the case
/// where it's NOT in A contradicts via functionality + covering;
/// the case where it IS in A makes the conditional GCI fire,
/// yielding ∃R_j.B, which combined with the also-derived A_super
/// satisfies T's body.
///
/// The saturator's current EL closure can't do this case-split.
/// Phase 2c materializes the conclusion `LHS_body ⊑ ∃R_j.B`
/// directly via absorbed-TBox pattern-matching on the triangle.
///
/// ASSERTS THE GAP — Phase 2c flips after the rule lands.
#[test]
fn functional_role_covering_canary_documents_the_gap() {
    use horned_owl::io::ParserConfiguration;
    use horned_owl::io::ofn::reader::read;
    use horned_owl::model::RcStr;
    use horned_owl::ontology::set::SetOntology;
    use owl_dl_core::convert::convert_ontology;
    use std::io::Cursor;

    let src = "\
Prefix(:=<http://rustdl.test/p2c/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://rustdl.test/p2c/test>
    Declaration(Class(:T))
    Declaration(Class(:X))
    Declaration(Class(:A_super))
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:LHS_body))
    Declaration(ObjectProperty(:R_f))
    Declaration(ObjectProperty(:R_i))
    Declaration(ObjectProperty(:R_j))
    FunctionalObjectProperty(:R_f)
    SubObjectPropertyOf(:R_i :R_f)
    SubObjectPropertyOf(:R_j :R_f)
    DisjointClasses(:A :B)
    EquivalentClasses(:T ObjectIntersectionOf(:A_super ObjectSomeValuesFrom(:R_j :B)))
    SubClassOf(:X :A_super)
    SubClassOf(:X :LHS_body)
    SubClassOf(:X ObjectSomeValuesFrom(:R_i :A))
    SubClassOf(
      ObjectIntersectionOf(:LHS_body ObjectSomeValuesFrom(:R_i :A))
      ObjectSomeValuesFrom(:R_j :B))
)
";
    let mut reader = Cursor::new(src);
    let (set_onto, _): (SetOntology<RcStr>, _) =
        read(&mut reader, ParserConfiguration::default()).expect("canary parses");
    let internal = convert_ontology(&set_onto).expect("canary lowers");
    let subsumers = crate::saturate(&internal);
    let x = internal.vocabulary.class_id("http://rustdl.test/p2c/X").expect("X declared");
    let t = internal.vocabulary.class_id("http://rustdl.test/p2c/T").expect("T declared");

    assert!(
        !subsumers.contains(x, t),
        "Phase 2c canary unexpectedly passed: the functional-role + covering rule \
         appears to be implemented (or the synthetic is wrong). If the rule landed, \
         invert this assertion."
    );
}
```

NOTE: The canary asserts the GAP. This synthetic is SIMPLIFIED from pair_06 — the conditional GCI directly contains `∃R_i.A` rather than a deeper chain through `∃hasEffectiveness.(Effectiveness ⊓ ...)`. The simplification keeps the canary tractable for the executor's manual reasoning while preserving the triangle's essential structure.

CAVEAT: this synthetic may not be HermiT-derivable as easily as pair_06 — HermiT's derivation on pair_06 depends on specific GALEN axioms that aren't all present in this synthetic. Task 2 verifies via HermiT cross-check.

- [ ] **Step 3: Run, expect canary PASSES (gap holds)**

```bash
cargo test -p owl-dl-saturation functional_role_covering_canary -- --test-threads=1 2>&1 | tail -10
```

Expected: PASSES (the saturator misses X ⊑ T per current capability).

If FAILS (i.e. `subsumers.contains(x, t)` is unexpectedly true), the saturator IS deriving this already — either Phase 2a/2b inadvertently covered it, or the synthetic is wrong. Investigate via `eprintln!` on the closure's content.

- [ ] **Step 4: Run all saturation tests + CI strictness**

```bash
cargo test -p owl-dl-saturation -- --test-threads=1 2>&1 | tail -5
RUSTFLAGS="-D warnings" cargo test -p owl-dl-saturation --no-run 2>&1 | tail -3
```
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/owl-dl-saturation/src/lib.rs
git commit -m "test(saturation): Phase 2c canary documenting functional-role + covering gap"
```

---

## Task 2: HermiT cross-check on the canary synthetic

**Files:**
- Create: `crates/owl-dl-saturation/tests/fixtures/phase2c_functional_role_covering_canary.ofn`

Verify HermiT derives `X ⊑ T` on the simplified synthetic BEFORE implementing the rule. If HermiT can't derive it on the synthetic, the synthetic is too simplified — Task 3's design step then needs to use the FULL pair_06 fixture rather than the synthetic.

- [ ] **Step 1: Write the fixture (byte-identical to T1's inline string)**

Create `crates/owl-dl-saturation/tests/fixtures/phase2c_functional_role_covering_canary.ofn` with the exact same content as the `let src = "..."` literal in T1's test. Verify byte-identicality:

```bash
diff <(awk '/let src = "/,/";/' crates/owl-dl-saturation/src/lib.rs | sed -n 's/^\\\\$//;s/^"\(.*\)";$/\1/p;s/^"\(.*\)\\\\$/\1/p') crates/owl-dl-saturation/tests/fixtures/phase2c_functional_role_covering_canary.ofn 2>&1
```

(Or just visually compare the two; the ontology is 14 axiom lines + declarations.)

Confirm not gitignored: `git check-ignore -v crates/owl-dl-saturation/tests/fixtures/phase2c_functional_role_covering_canary.ofn` — empty output.

- [ ] **Step 2: Run HermiT via Phase 0 oracle**

```bash
docker/robot/classify-oracle.sh \
    crates/owl-dl-saturation/tests/fixtures/phase2c_functional_role_covering_canary.ofn \
    /tmp/p2c-canary-hermit.owx
```

- [ ] **Step 3: Verify HermiT derives X ⊑ T (transitively)**

```bash
python3 <<'EOF'
import xml.etree.ElementTree as ET
from collections import defaultdict
NS = '{http://www.w3.org/2002/07/owl#}'
tree = ET.parse('/tmp/p2c-canary-hermit.owx')
x = 'http://rustdl.test/p2c/X'
t = 'http://rustdl.test/p2c/T'
edges = defaultdict(set)
for sc in tree.iter(NS + 'SubClassOf'):
    classes = [c.get('IRI') for c in sc.findall(NS + 'Class')]
    if len(classes) == 2:
        edges[classes[0]].add(classes[1])
# BFS for transitive reachability
seen, queue = {x}, [x]
while queue:
    cur = queue.pop(0)
    if cur == t:
        print("FOUND"); break
    for nxt in edges.get(cur, ()):
        if nxt not in seen:
            seen.add(nxt); queue.append(nxt)
else:
    print("NOT FOUND")
EOF
```

Expected: `FOUND`. If `NOT FOUND`, the synthetic isn't a HermiT-derivable instance of the pattern. Two fallbacks:
- (a) Adjust the synthetic to add the missing GALEN-style axiom (e.g. covering on a third class `Status_super ≡ A ⊔ B`, or a chain through `R_f` that forces a witness). Update T1's inline string AND this fixture in lockstep.
- (b) Use pair_06 directly as the canary fixture for Phase 2c, and adjust T1 to load it rather than embedding a synthetic. The trade-off: pair_06 is 89 KB of GALEN axioms (less tractable for manual reasoning) but is guaranteed HermiT-verifiable.

If after one adjustment HermiT still misses, document the gap in Task 3's design doc and use pair_06.ofn as the canary instead.

- [ ] **Step 4: Commit (only if HermiT derived)**

```bash
git add crates/owl-dl-saturation/tests/fixtures/phase2c_functional_role_covering_canary.ofn
git commit -m "fixture(saturation): Phase 2c canary OFN for HermiT cross-check

HermiT confirms X ⊑ T on the synthetic via classify-oracle.sh: <paste
evidence>. This is the verify-before-build gate."
```

If HermiT didn't derive even with the adjustment, commit the (working) updated canary fixture with a comment that HermiT IS deriving it.

If HermiT never derives (even with adjustments), don't commit the fixture and report DONE_WITH_CONCERNS — the Phase 2c canary needs to revert to pair_06.ofn as the test artifact.

---

## Task 3: Design — read absorption code, identify the triangle pattern, design the rule

**Files:**
- Create: `docs/phase2c-fix-target.md` (committed).

T3 is the analytical heart. Same shape as Phase 2b's T3 (trace before extending). The rule's exact detection algorithm depends on what the absorbed-TBox shape looks like — `mod absorb` in `owl-dl-core` produces the post-NNF/absorb TBox the saturator consumes.

- [ ] **Step 1: Read the absorbed-TBox shape**

```bash
grep -nE "pub struct AbsorbedTBox|pub enum (Concept|Trigger)|ConceptRule\b" crates/owl-dl-core/src/absorb.rs crates/owl-dl-core/src/told.rs crates/owl-dl-core/src/clause.rs 2>/dev/null | head -30
```

Read the relevant struct definitions. The triangle pattern's source axiom is `LHS_body ⊓ ∃R_i.A ⊑ ∃R_j.B`; this is absorbed into one of:
- A `ConceptRule { trigger: <atomic>, conclusion: ... }` if absorption found a single-trigger atomic.
- A residual GCI if no absorption was possible.
- A conjunctive trigger if `LHS_body` is itself a conjunction of atomics.

Identify the exact absorbed representation. The Phase 2c rule's pattern detector needs to inspect THIS representation, not the raw axiom.

- [ ] **Step 2: Read the existing saturator rule structure**

```bash
grep -nE "fn collect_el_rules|struct ElRules" crates/owl-dl-saturation/src/lib.rs | head -10
```

The saturator's `collect_el_rules` (Phase 2a T3 added `functional_roles` + `functional_supers_of` infrastructure) is where Phase 2c's pattern detection lives. Read the existing collection loop. The Phase 2c rule adds a NEW kind of derived entry (not a fact, not a conjunctive trigger — a NEW `AtomicSubsumption` or `ExistentialFact` representing the materialized conclusion).

- [ ] **Step 3: Design the rule**

The pattern (per the diagnosis):

**Trigger condition** (detected at `collect_el_rules` time, scanning the absorbed TBox):
- Some axiom `GCI`: `LHS_body ⊑ Conclusion` where:
  - `LHS_body` is `And(L_1, L_2, ..., ∃R_i.A, ..., L_k)` (or absorbed equivalent).
  - `Conclusion` is `∃R_j.B`.
  - `R_i, R_j` share a common functional super-role `R_f` (per Phase 2a's `functional_supers_of`).
  - There's a `DisjointClasses(A, B)` axiom (or equivalent disjointness/covering — see soundness conditions below).
- AND some atomic class `X` (or class with told-subsumers) provably has every `L_i` EXCEPT `∃R_i.A` as a told subsumer.

**Materialized conclusion** (emitted at collection time):
- `X ⊑ ∃R_j.B` — i.e. emit an `ExistentialFact { sub: X, role: R_j, target: B }` directly, bypassing the case-split.

**Soundness conditions** (must be checked to avoid false positives):
- The covering must be axiom-strong: `DisjointClasses(A, B)` declared, OR a covering axiom `Z ⊑ A ⊔ B` plus a disjointness between A and B inferable from told relationships.
- The functional super-role's domain/range must be consistent with X having a witness (otherwise the case-split is on an empty set).
- Optionally: only emit when X also has `∃R_i.something` already — that ensures the case-split is on a real witness, not an empty role. Wait — this is the wrong direction. We want to emit even when X doesn't have ∃R_i.A directly. Re-think.

Actually the simplest sound form: if the GCI is `LHS_body ⊓ ∃R_i.A ⊑ ∃R_j.B` AND `A ⊓ B ⊑ ⊥`, then for any X with LHS_body that has SOMETHING via `R_f` (per functional-role reasoning), the case-split forces ∃R_j.B. But proving "X has something via R_f" requires its own chain.

T3's job is to nail down the EXACT precondition. Read pair_06's axiom set carefully; identify which axioms in the GALEN extract make `IneffectiveCardiacFunction` HAVE an R_f witness (not just have access to the conditional GCI). The Phase 2b.0 per-pair analysis hints that `BodyProcess` (or `CardiacFunction`) implies a `StatusAttribute` witness via the ontology's other axioms.

**Two candidate rule shapes** to consider in T3:

(i) **Strong (always materialize)**: if the GCI shape + covering + functional-super match, emit `LHS_body ⊑ ∃R_j.B` unconditionally. This is sound IF "X with LHS_body always has an R_f witness." Verify by tracing pair_06: does `IneffectiveCardiacFunction`'s closure (in the saturator) include `∃R_f.⊤` (some witness)? If yes, the strong form is sound.

(ii) **Conditional (require an R_f witness on X)**: only materialize when X is known to have `∃R_f.something` (via some other rule's output). This requires fact-time integration, not just collection-time.

T3 picks the form. The strong form is simpler if it's sound on pair_06.

- [ ] **Step 4: Write `docs/phase2c-fix-target.md`**

Structure:

```markdown
# Phase 2c — fix target

Per Phase 2c.0 diagnosis (`docs/phase2c-galen-diagnosis.md`), the
target rule is the EL+ approximation of functional-role + covering
case-split. This doc nails down the exact pattern detector + lowering.

## Pattern to detect (absorbed TBox)

<from T3's reading of the absorption shape: the exact ConceptRule /
residual GCI shape that represents `LHS_body ⊓ ∃R_i.A ⊑ ∃R_j.B`>

## Soundness conditions

<the precise conditions that must hold for the materialization to be
sound — covering / functional super-role / X has R_f witness, etc.>

## Lowering

<what to emit. Likely an `ExistentialFact { sub: X, role: R_j, target: B }`
seeded into the worklist at collection time. The fact then propagates
normally via CR5 / CR9.>

## Pattern detection algorithm

<concrete steps: iterate absorbed-TBox entries, look for the shape,
check the soundness preconditions, emit the fact>

## Expected impact

<24-44 pairs of GALEN+notgalen recovery, per Phase 2c.0 diagnosis>

## Soundness argument (for the canary)

<walk-through: for the Phase 2c canary's specific synthetic, why does
the materialization preserve soundness? FP=0 must hold>
```

- [ ] **Step 5: Commit the design doc**

```bash
git add docs/phase2c-fix-target.md
git commit -m "perf(phase2c): chosen fix target + soundness conditions"
```

---

## Task 4: Implement the rule + structural canary

**Files:**
- Modify: `crates/owl-dl-saturation/src/lib.rs` (the `collect_el_rules` function + a new method or block).
- Modify: `crates/owl-dl-saturation/src/lib.rs` (the existing Phase 2c canary from T1 — flip the assertion + add a structural counter test).

The exact code depends on T3's design. The plan provides the FRAMEWORK:

- [ ] **Step 1: Add a counter for the new rule's firings**

In `collect_el_rules` or a related struct, add a counter (mirror Phase 2a/2b/3 patterns):

```rust
    /// Phase 2c: count of functional-role + covering triangles
    /// detected during collection. Used by the structural canary
    /// to confirm the rule fires.
    pub functional_role_covering_materializations: u64,
```

(If counters live on `RuleCounters` in the tableau crate per prior phases, mirror there. If the saturator has its own per-collection counter struct, use that. Phase 2a's `functional_supers_of` precomputation didn't add a counter — it just populated the data; Phase 2c can do the same and use a return-value or stats-out-param to signal counts. Pick what fits.)

- [ ] **Step 2: Implement the pattern detector in `collect_el_rules`**

Per T3's design doc's "Pattern detection algorithm" section. Likely shape:

```rust
// In collect_el_rules, after Phase 2a's functional_supers_of is
// populated and after the main absorption-to-rule loop:
//
// Phase 2c: detect the functional-role + covering triangle and
// materialize the conclusion existential directly.
for axiom in internal.axioms.iter().filter_map(|a| a.as_sub_class_of()) {
    let Some((lhs_body, r_i, a_class, r_j, b_class)) = matches_triangle_shape(axiom, ...) else {
        continue;
    };
    let funcs_i = rules.functional_supers_of(r_i);
    let funcs_j = rules.functional_supers_of(r_j);
    let Some(r_f) = funcs_i.iter().find(|f| funcs_j.contains(f)).copied() else {
        continue;
    };
    if !rules.disjoint_pairs.contains(&(a_class, b_class)) {
        continue;  // covering not satisfied
    }
    // ... per T3 design ...
    rules.functional_role_covering_materializations += 1;
    // Emit: for each X with lhs_body as a told super, push the materialized fact.
    for x in classes_with_told_subsumers_including_lhs_body(lhs_body, &rules) {
        rules.existential_facts.push(ExistentialFact { sub: x, role: r_j, target: b_class });
    }
}
```

The `matches_triangle_shape` helper inspects the absorbed-TBox shape per T3's design. The `classes_with_told_subsumers_including_lhs_body` is the iteration to find subjects.

(This is a SKETCH. T3's actual design doc gives the precise algorithm; T4 implements it.)

- [ ] **Step 3: Flip the canary's assertion + rename**

In `crates/owl-dl-saturation/src/lib.rs`, find `functional_role_covering_canary_documents_the_gap` (added in T1). Change:
- Rename: `_documents_the_gap` → `_recovers_entailment`.
- Invert: `!subsumers.contains(x, t)` → `subsumers.contains(x, t)`.
- Update doc comment + message accordingly.

- [ ] **Step 4: Add the structural canary**

```rust
#[test]
fn functional_role_covering_canary_materialization_counter_bumped() {
    // Same synthetic as the verdict canary.
    let src = "..."; // re-use the inline string
    // ... parse + saturate ...
    // Assert the materialization counter > 0 (the rule fired).
    // Path depends on T1's design — if it's on rules, check rules.functional_role_covering_materializations.
}
```

If the structural assertion is awkward (the counter isn't easily accessible from the test), gate the structural canary with `#[cfg(feature = "counters")]` and rely on the corpus-diff result as the structural signal instead.

- [ ] **Step 5: Run, expect both canaries pass**

```bash
cargo test -p owl-dl-saturation functional_role_covering -- --test-threads=1 2>&1 | tail -10
```

Expected: both canaries pass.

- [ ] **Step 6: Full saturation + reasoner-lib regression sweep**

```bash
cargo test -p owl-dl-saturation -- --test-threads=1 2>&1 | tail -10
cargo test -p owl-dl-reasoner --lib -- --test-threads=1 2>&1 | tail -10
RUSTFLAGS="-D warnings" cargo test -p owl-dl-saturation --no-run 2>&1 | tail -3
cargo clippy -p owl-dl-saturation --all-targets -- -D warnings 2>&1 | grep -E "warning|error" | grep -v "(too_many_lines|map_unwrap_or|doc-markdown)" | head -5
```

Expected: all tests pass; CI strictness clean; no new clippy.

If any pre-existing test fails, the rule's materialization is changing verdicts on OTHER ontologies. STOP and investigate — likely the soundness conditions are too loose.

- [ ] **Step 7: Commit**

```bash
git add crates/owl-dl-saturation/src/lib.rs
git commit -m "feat(saturation): EL+ functional-role + covering materialization (Phase 2c)

Pattern-match the triangle `LHS_body ⊓ ∃R_i.A ⊑ ∃R_j.B` in absorbed-TBox
shape with R_i, R_j sub-properties of functional R_f and A ⊓ B ⊑ ⊥.
Materialize `LHS_body ⊑ ∃R_j.B` directly via an ExistentialFact,
bypassing the non-Horn case-split HermiT uses. See
docs/phase2c-fix-target.md for the soundness argument.

Phase 2c canary (functional_role_covering_canary_recovers_entailment)
now passes: rule materializes the existential; CR5 propagates through;
X ⊑ T closes. Structural canary asserts the counter bumps."
```

---

## Task 5: Corpus measurement on GALEN + notgalen + Phase 0 net

**Files:**
- Capture: `/tmp/p2c-net.log`, `/tmp/p2c-galen.log`, `/tmp/p2c-notgalen.log`.

- [ ] **Step 1: Build**

```bash
cargo test -p owl-dl-reasoner --test konclude_closure_diff --release --no-run 2>&1 | tail -3
```

- [ ] **Step 2: Phase 0 net soundness gate**

```bash
cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    alehif_closure_matches_konclude ore_10908_sroiq ore_15672_shoin \
    -- --ignored --nocapture 2>&1 | tee /tmp/p2c-net.log | grep -E "^---|FP=|MISSED="
```

Hard cap 30 min. Expected: FP=0 / MISSED=0 across all 3. If ANY FP > 0, the rule is UNSOUND on the corpus — STOP. Investigate which pair becomes spurious; trace through T3's soundness conditions; tighten or revert.

- [ ] **Step 3: GALEN measurement**

```bash
timeout 2400 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    galen_closure_matches_konclude \
    -- --exact --ignored --nocapture 2>&1 | tee /tmp/p2c-galen.log | grep -E "^--- galen|MISSED="
```

Hard cap 40 min. Expected:
- FP=0.
- MISSED drops from 17. Spec target (per Phase 2c.0): 24-pair confident floor of recovery across GALEN+notgalen. GALEN's confident share is 12 of 12 (pure cluster C); MISSED should drop 17 → ≤ 5.
- Wall ≤ 13 min (Phase 3c baseline 12.2 min + small overhead from new rule).

- [ ] **Step 4: notgalen measurement**

```bash
timeout 2400 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    notgalen_closure_matches_konclude \
    -- --ignored --nocapture 2>&1 | tee /tmp/p2c-notgalen.log | grep -E "^--- notgalen|MISSED="
```

Hard cap 40 min. Expected: FP=0; MISSED 27 → ≤ 15 (confident: 12 pairs are pure cluster C; the 15 anonymous-cluster-C-variant may or may not match).

- [ ] **Step 5: Triage**

Record per-fixture: FP, MISSED, wall. Compute MISSED reduction vs Phase 2c.0 baselines (17 GALEN, 27 notgalen). Compare to the diagnosis estimate (24-44 of 44).

If recovery is LESS than 24 (the confident floor): the rule fires on the synthetic but not on the corpus — investigate via the same trace-before-extend discipline as Phase 2b.5 (eprintln on the rule's match path; identify which corpus pair doesn't match).

If recovery is exactly 24 (the floor): predicted. Phase 2d could extend.

If recovery is > 24: bonus — some anonymous-cluster-C-variant pairs matched too.

DO NOT commit anything in T5. T6 captures the results.

---

## Task 6: Results doc + close-out

**Files:**
- Create: `docs/phase2c-results.md`
- Modify: `CLAUDE.md` (saturator description)
- Modify: `docs/superpowers/specs/2026-05-31-soundness-completeness-perf-design.md` (Phase 2 close-out continuation)

- [ ] **Step 1: Write `docs/phase2c-results.md`**

Mirror Phase 2b's results doc shape. Fill values from T5's measurement.

```markdown
# Phase 2c — Functional-role + covering EL+ approximation results

Run 2026-06-0N. Fix: pattern-match the functional-role + covering
triangle in absorbed-TBox shape; materialize the conclusion existential
directly. See `docs/phase2c-fix-target.md` for design and
`docs/phase2c-galen-diagnosis.md` for the underlying scope estimate.

## Headline finding

<one paragraph: GALEN MISSED reduction; notgalen MISSED reduction;
FP gate status. Honest about whether the 24-44 range was hit.>

## Soundness gate (Phase 0 net)

<table>

## Completeness lever (GALEN + notgalen)

<table>

## What's left

- Residual MISSED if any (likely the F-tail body-structure or
  anonymous-notgalen variants that didn't match the triangle).
- Phase 2d would target those if scope-justified by the residual count.

## Cross-references

- Phase 2c.0 diagnosis: `phase2c-galen-diagnosis.md`
- Phase 2b.0 per-pair analysis (pair 06 is the canonical canary):
  `phase2b-galen-pair-analysis.md`
```

- [ ] **Step 2: Update CLAUDE.md saturator description**

Append a one-paragraph Phase 2c note to the `crates/owl-dl-saturation` bullet.

- [ ] **Step 3: Update design spec Phase 2 close-out**

In `docs/superpowers/specs/2026-05-31-soundness-completeness-perf-design.md`, append after the Phase 2c.0 paragraph:

```
Phase 2c landed: `docs/phase2c-results.md`. <one-sentence headline:
e.g. "Recovered N of 44 residual MISSED via functional-role + covering
triangle materialization; FP=0 held. Remaining M pairs are <residual>.">
```

- [ ] **Step 4: Commit**

```bash
git add docs/phase2c-results.md \
        CLAUDE.md \
        docs/superpowers/specs/2026-05-31-soundness-completeness-perf-design.md
git commit -m "docs(phase2c): results doc + envelope updates"
```

---

## Definition of done (Phase 2c)

- Phase 2c canary (`functional_role_covering_canary_recovers_entailment`) passes after the fix.
- Structural canary asserts the materialization counter bumps.
- All saturation + reasoner-lib tests pass; CI strictness clean.
- Phase 0 net FP=0 + MISSED-unchanged held.
- GALEN + notgalen MISSED recorded; recovery in the 24-44 range per the diagnosis.
- Results doc + CLAUDE.md + design spec updated.

## What this plan does NOT do

- Does NOT extend to hypertableau (Option 1) or full classical disjointness propagation (Option 2). Those are months of work each.
- Does NOT promise 44/44 recovery. The diagnosis floor is 24; upper bound 44.
- Does NOT change verdicts on FP=0 fixtures (alehif, ORE-SROIQ, ORE-SHOIN, pizza, ro, sulo, sio).
- Does NOT touch the tableau or hypertableau wedge. Saturator-only.
