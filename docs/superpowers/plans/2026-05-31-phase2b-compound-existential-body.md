# Phase 2b — Compound Existential-Body Lowering Fix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the saturator's compound existential-body lowering so the `JointStability`-shaped pattern (`X ≡ A ⊓ ∃R.(B ⊓ ∃S.C)` with `S' ⊑ S`, `C' ⊑ C`) closes correctly, recovering the estimated ~60 of GALEN's 109 MISSED (clusters A+B+E per `docs/phase2b-galen-diagnosis.md`) while holding FP=0 on the Phase 0 net.

**Architecture:** Single-crate change in `crates/owl-dl-saturation/src/lib.rs`. Phase 2b.0's analysis points at `introduce_existential_marker` (`lib.rs:810`) — markers introduced for nested-existential bodies are deliberately ONE-WAY (`∃R.B ⊑ F` but not `F ⊑ ∃R.B`), which means a synthetic class F representing `JointArticulationProcess ⊓ ∃actsSpecificallyOn.KneeJoint` lacks an existential fact about itself, blocking the CR5 + CR9 + sub-property propagation chain. The fix is to ALSO emit `F ⊑ ∃R.B` (equivalence-style marker) when F is used as a body in a Tseitin synthetic — but ONLY when the existential is positive-position (LHS or RHS of a SubClassOf), never inside a trigger LHS that needs the asymmetric semantics. T3's trace verifies this hypothesis before any fix lands.

**Tech Stack:** Rust (edition 2024), `owl-dl-saturation` crate; Phase 2b.0 fixtures (`crates/owl-dl-reasoner/tests/fixtures/phase2b/pair_NN.{ofn,hermit.owx}`); the Phase 0 closure-diff harness + corpus.

---

## Background the executor needs

- Phase 2b.0's diagnosis (`docs/phase2b-galen-diagnosis.md`) found 6 of 8 sampled pairs need this fix, extrapolating to ~60 of 109 GALEN MISSED. The diagnosis is explicit: this is an implementation gap, NOT a missing calculus rule.
- pair_08's actual shape (the smallest, single-hop repro) — from `crates/owl-dl-reasoner/tests/fixtures/phase2b/pair_08.ofn`:
  ```
  JointStability      ≡ Scope ⊓ ∃isScopeOf.(JointArticulationProcess ⊓ ∃actsOn.Joint)
  KneeJointStability  ≡ Scope ⊓ ∃isScopeOf.(JointArticulationProcess ⊓ ∃actsSpecificallyOn.KneeJoint)
  actsSpecificallyOn  ⊑ actsOn          # sub-property
  KneeJoint           ⊑ Joint           # sub-class
  ```
  Plus thousands of unrelated axioms in the module. The minimal closure chain is: `KneeJointStability` → (witness via `isScopeOf` is `JointArticulationProcess ⊓ ∃actsSpecificallyOn.KneeJoint`) → witness needs `∃actsOn.Joint` via CR9 (sub-property) + CR5 (sub-class on body) → witness becomes `JointArticulationProcess ⊓ ∃actsOn.Joint` = JointStability's body shape → JointStability fires.
- The saturator's lowering chain for `X ⊑ ∃R.(B ⊓ ∃S.C)`-shaped axioms flows through: `lower_sub_class_of` → `atomic_existential_rhs` (RHS case) OR the LHS-And handler at `:1263` (LHS conjunction case) → `existential_body_alternatives` (`:1422`) → `atomic_or_tseitin_body_with_extras` (`:1447`) → `atomic_classes_with_existential_markers` (`:1502`) → `introduce_existential_marker` (`:810`). Read those functions before T2/T3.
- `introduce_existential_marker`'s docstring (`lib.rs:806-809`) explicitly says it does NOT emit the reverse `F ⊑ ∃R.B`. That's the diagnostic suspect — markers are one-way (correct for LHS-trigger semantics), but when reused INSIDE a Tseitin synthetic body that needs full equivalence, this one-wayness breaks the chain. T3 verifies via tracing.
- Dead-end discipline (#11 / #12 / #14): verify-before-build with HermiT cross-check + a real trace of the saturator's behavior on the canary, NOT just code reading. The trace must show WHICH derivation step doesn't fire, in concrete terms.
- The Phase 0 soundness net is `scripts/run-soundness-diff.sh` (galen, notgalen, alehif, ore-10908-sroiq, ore-15672-shoin, and the pizza/ro/sulo/sio corpus tests). The success measurable: Phase 0 net FP=0 still holds; GALEN MISSED drops from 109 toward ~49 (109 − 60 estimated). Wall regression should stay within 2-3× per fixture.

---

## Task 1: Build the minimal synthetic canary

**Files:**
- Modify: `crates/owl-dl-saturation/src/lib.rs` (add a `#[test]` in `mod tests`).

The canary mirrors pair_08's actual axiom shape, abstracted to 4 classes and 3 roles. It's tiny (10 declarations + 4 axioms) so the executor can reason about it by hand AND so an `eprintln` trace produces readable output. It asserts the CURRENT broken state — Task 5 inverts it after the fix.

- [ ] **Step 1: Locate the mod tests block**

Run: `grep -nE "^#\\[cfg\\(test\\)\\]|^mod tests" crates/owl-dl-saturation/src/lib.rs | head -3`
Note the line where `mod tests` lives.

- [ ] **Step 2: Add the canary test**

Add inside `mod tests`, placed adjacent to the existing Phase 2a `functional_role_merge_*` canaries:

```rust
/// Phase 2b canary: minimal repro of GALEN's
/// `KneeJointStability ⊑ JointStability` pattern (pair_08 in the
/// Phase 2b.0 fixture set). The axiom shape:
///
///   T ≡ A ⊓ ∃R.(B ⊓ ∃S.C)
///   X ≡ A ⊓ ∃R.(B ⊓ ∃S'.C')   where S' ⊑ S, C' ⊑ C
///
/// Expected entailment: X ⊑ T. Derivation: X's R-witness is in
/// (B ⊓ ∃S'.C'); via sub-property S' ⊑ S, the witness is also in
/// ∃S.C' (CR9); via sub-class C' ⊑ C, the witness has subsumer
/// `∃S.C` (CR5); so the witness is in B ⊓ ∃S.C = T's R-body;
/// closing the conjunctive trigger that defines T.
///
/// Phase 2b.0's analysis (docs/phase2b-galen-diagnosis.md) traced
/// the bug to `introduce_existential_marker`'s one-way semantics
/// being inadequate when the marker is reused inside a Tseitin
/// synthetic that needs full equivalence. This canary ASSERTS THE
/// GAP — the closure does NOT contain X ⊑ T pre-fix. Task 5 of
/// Phase 2b inverts the assertion after the fix.
#[test]
fn compound_existential_body_canary_documents_the_gap() {
    use horned_owl::io::ParserConfiguration;
    use horned_owl::io::ofn::reader::read;
    use horned_owl::model::RcStr;
    use horned_owl::ontology::set::SetOntology;
    use owl_dl_core::convert::convert_ontology;
    use std::io::Cursor;

    let src = "\
Prefix(:=<http://rustdl.test/p2b/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://rustdl.test/p2b/test>
    Declaration(Class(:T))
    Declaration(Class(:X))
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    Declaration(Class(:C_sub))
    Declaration(ObjectProperty(:R))
    Declaration(ObjectProperty(:S))
    Declaration(ObjectProperty(:S_sub))
    SubObjectPropertyOf(:S_sub :S)
    SubClassOf(:C_sub :C)
    EquivalentClasses(:T ObjectIntersectionOf(:A ObjectSomeValuesFrom(:R ObjectIntersectionOf(:B ObjectSomeValuesFrom(:S :C)))))
    EquivalentClasses(:X ObjectIntersectionOf(:A ObjectSomeValuesFrom(:R ObjectIntersectionOf(:B ObjectSomeValuesFrom(:S_sub :C_sub)))))
)
";
    let mut reader = Cursor::new(src);
    let (set_onto, _): (SetOntology<RcStr>, _) =
        read(&mut reader, ParserConfiguration::default()).expect("canary parses");
    let internal = convert_ontology(&set_onto).expect("canary lowers");
    let subsumers = crate::saturate(&internal);
    let x = internal.vocabulary.class_id("http://rustdl.test/p2b/X").expect("X declared");
    let t = internal.vocabulary.class_id("http://rustdl.test/p2b/T").expect("T declared");

    // ASSERT THE GAP — Task 5 inverts after the fix.
    assert!(
        !subsumers.contains(x, t),
        "Phase 2b canary unexpectedly passed: compound existential-body \
         lowering appears to be fixed (or the canary is wrong). If the \
         fix landed, invert this assertion."
    );
}
```

- [ ] **Step 3: Run — expect pass (asserts the gap)**

```bash
cargo test -p owl-dl-saturation compound_existential_body_canary -- --test-threads=1 2>&1 | tail -10
```
Expected: `test ... compound_existential_body_canary_documents_the_gap ... ok` — the gap holds.

If the canary unexpectedly FAILS (i.e. `subsumers.contains(x, t)` is already true), the bug shape isn't what we thought. Investigate immediately:
- Does the assertion check the right direction? (`x ⊑ t`, not `t ⊑ x`)
- Did Phase 2a's functional-role rule accidentally close this pattern? (Trace by setting `RUSTDL_HYPER_TRUST_SAT_MIN_MS=0` — but this is the saturator, env var doesn't apply.)
- Is the closure carrying it via some other path? Print all subsumers of X: `eprintln!("{:?}", subsumers.subsumers_of(x))` and look for T.

Report findings; stop work pending controller decision.

- [ ] **Step 4: Run all saturation tests — no regression**

```bash
cargo test -p owl-dl-saturation -- --test-threads=1 2>&1 | tail -5
```
Expected: 32 + 1 = 33 tests pass.

- [ ] **Step 5: CI strictness compile**

```bash
RUSTFLAGS="-D warnings" cargo test -p owl-dl-saturation --no-run 2>&1 | tail -3
```
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/owl-dl-saturation/src/lib.rs
git commit -m "test(saturation): Phase 2b canary documenting compound existential-body gap"
```

---

## Task 2: HermiT cross-check on the canary

**Files:**
- Create: `crates/owl-dl-saturation/tests/fixtures/phase2b_compound_existential_canary.ofn` (the canary's ontology, identical to the inline string).

Same verify-before-build discipline as Phase 2a Task 2: confirm HermiT derives X ⊑ T on the canary BEFORE building a fix. If HermiT also misses, the canary shape isn't what HermiT solves either, and the fix would be wrong.

- [ ] **Step 1: Write the fixture file (byte-identical to T1's inline string)**

Create `crates/owl-dl-saturation/tests/fixtures/phase2b_compound_existential_canary.ofn` with the same content as the `let src = "..."` in T1's test (the 14 lines starting with `Prefix(:=...)` through the closing `)`).

The file is tracked (the `tests/fixtures/` tree is not under the gitignored `ontologies/`). Verify: `git check-ignore -v crates/owl-dl-saturation/tests/fixtures/phase2b_compound_existential_canary.ofn` should print nothing.

- [ ] **Step 2: Run HermiT via Phase 0 oracle**

```bash
docker/robot/classify-oracle.sh \
    crates/owl-dl-saturation/tests/fixtures/phase2b_compound_existential_canary.ofn \
    /tmp/p2b-canary-hermit.owx
```

Expected: stderr ends `wrote /tmp/p2b-canary-hermit.owx`. Image is cached from Phase 0/2a.

- [ ] **Step 3: Verify HermiT derives X ⊑ T**

```bash
python3 <<'EOF'
import xml.etree.ElementTree as ET
NS = '{http://www.w3.org/2002/07/owl#}'
tree = ET.parse('/tmp/p2b-canary-hermit.owx')
x = 'http://rustdl.test/p2b/X'
t = 'http://rustdl.test/p2b/T'
found = False
for sc in tree.iter(NS + 'SubClassOf'):
    classes = [c.get('IRI') for c in sc.findall(NS + 'Class')]
    if len(classes) == 2 and classes[0] == x and classes[1] == t:
        found = True; break
for ec in tree.iter(NS + 'EquivalentClasses'):
    classes = [c.get('IRI') for c in ec.findall(NS + 'Class')]
    if x in classes and t in classes:
        found = True; break
print('FOUND' if found else 'NOT FOUND')
EOF
```

Expected: `FOUND`. HermiT derives the entailment on the synthetic.

If `NOT FOUND`, the canary doesn't exercise the actual HermiT-derivable pattern. STOP — re-examine pair_08 vs the canary and adjust. The canary must mirror pair_08's shape closely enough that HermiT derives the same shape of entailment.

- [ ] **Step 4: Commit the fixture**

```bash
git add crates/owl-dl-saturation/tests/fixtures/phase2b_compound_existential_canary.ofn
git commit -m "fixture(saturation): Phase 2b canary OFN for HermiT cross-check

HermiT confirms X ⊑ T on the synthetic via classify-oracle.sh. This
is the verify-before-build gate: the canary exercises a real EL+
pattern that a sound+complete reasoner derives, so the fix has a
sound target to recover."
```

---

## Task 3: Trace the bail-out — empirical diagnosis

**Files:**
- Modify (temporarily — reverted before commit): `crates/owl-dl-saturation/src/lib.rs` (add `eprintln!` traces in the lowering chain).
- Create: `docs/phase2b-trace.md` (committed — records what the trace showed and what the proposed fix is).

This is the analytical core. The hypothesis is that `introduce_existential_marker`'s one-way semantics breaks the chain. The trace VERIFIES this BEFORE the fix lands.

- [ ] **Step 1: Add temporary tracing to the lowering chain**

Add `eprintln!` at the following sites in `crates/owl-dl-saturation/src/lib.rs`:

- Top of `introduce_existential_marker` (around line 810): log `(role, body)` and the returned marker id.
- Top of `atomic_classes_with_existential_markers` (around line 1502): log the input ids.
- Top of `atomic_or_tseitin_body_with_extras` (around line 1447): log the body id and extras.
- After `tseitin.introduce(combined, rules)` calls: log the resulting synthetic + body.

Use a distinguishing prefix so the output is greppable, e.g. `eprintln!("P2B_TRACE introduce_existential_marker: role={role:?} body={body:?} marker={marker:?}");`.

Don't agonize about tracing every site — the goal is to see WHICH synthetics are allocated and WHICH triggers/facts get emitted when the canary saturates.

- [ ] **Step 2: Run the canary with trace output**

```bash
cargo test -p owl-dl-saturation compound_existential_body_canary -- --test-threads=1 --nocapture 2>&1 | grep "P2B_TRACE" > /tmp/p2b-trace.log
wc -l /tmp/p2b-trace.log
head -40 /tmp/p2b-trace.log
```

Expected: 10-30 lines of trace output showing the lowering for X and T's definitions. Look for:
- Are TWO markers introduced for the inner existentials — one for `∃S.C` (in T's body) and one for `∃S_sub.C_sub` (in X's body)?
- Are the two markers DIFFERENT class ids?
- Are facts emitted FOR the synthetics representing the bodies?

Build up a mental model of which classes/markers/facts exist after lowering.

- [ ] **Step 3: Trace the missing derivation**

Add MORE eprintln in the worklist's fact-processing function (search for `fn process_fact` or `todo_fact`) — specifically log when a fact `(sub, role, target)` is dequeued and processed. Also log when an existential-trigger fires (the trigger's body matched a class's subsumers).

Re-run, grep `P2B_TRACE`, examine:
- After lowering, what facts exist about the body synthetic F_X (representing `B ⊓ ∃S_sub.C_sub`)?
- Specifically: does F_X have any fact `(F_X, S, ?)` in the closure?
- Does the trigger for `∃S.C ⊑ marker_S_C` (the marker in T's body) ever fire on F_X?

If the answer to the last two is "no facts about F_X" / "trigger never fires on F_X" — that confirms the diagnostic hypothesis.

- [ ] **Step 4: Revert the tracing**

```bash
git checkout crates/owl-dl-saturation/src/lib.rs
git diff crates/owl-dl-saturation/src/lib.rs
```
Expected: empty diff (eprintln gone).

- [ ] **Step 5: Write `docs/phase2b-trace.md`**

Create the file with EXACTLY this structure (fill from real trace data):

```markdown
# Phase 2b — saturator trace for the compound existential-body canary

Diagnostic trace from running the Phase 2b canary
(`compound_existential_body_canary_documents_the_gap` in
`crates/owl-dl-saturation/src/lib.rs`) with temporary `eprintln!`
instrumentation. The tracing was REVERTED before commit.

## Setup

Canary axioms (from the test source):

```
SubObjectPropertyOf(:S_sub :S)
SubClassOf(:C_sub :C)
EquivalentClasses(:T   A ⊓ ∃R.(B ⊓ ∃S.C))
EquivalentClasses(:X   A ⊓ ∃R.(B ⊓ ∃S_sub.C_sub))
```

Expected entailment: X ⊑ T. HermiT confirms (Phase 2b Task 2).

## What the trace showed

<paste 5-15 lines of P2B_TRACE output, with annotations explaining
what each line means in the closure-construction story>

## Derivation that should fire but doesn't

<step-by-step narrative: which CR5 / CR9 / conjunctive-trigger
firings the closure would need to derive X ⊑ T, and which one
DOESN'T happen because of the bug>

## Diagnostic conclusion

<one paragraph: the actual bug, in concrete terms. Examples of what
this might say:
- "introduce_existential_marker emits the trigger `∃R.B ⊑ F` but no
  fact `(F, R, B)`, so when F is used as a body operand inside
  another Tseitin synthetic, the inner CR5/CR9 chain has nothing
  to fire on."
- "or some other concrete diagnostic — pick from what the trace
  actually shows.">

## Proposed fix

<one paragraph: the minimal code change. Likely shape:
- Add a new function `introduce_equivalent_existential_marker` that
  emits BOTH `∃R.B ⊑ F` AND `F ⊑ ∃R.B` (as a fact (F, R, B)), called
  from `atomic_classes_with_existential_markers` only (where the
  marker is used inside a Tseitin synthetic body that needs full
  equivalence semantics).
- The LHS-trigger-side call from the `:1263` LHS-And handler keeps
  using the existing one-way `introduce_existential_marker` because
  trigger semantics ARE one-way.
- Or some alternative fix — whatever the trace data justifies.>
```

- [ ] **Step 6: Commit the trace doc**

```bash
git add docs/phase2b-trace.md
git commit -m "docs(phase2b): saturator trace + diagnostic for compound-body canary"
```

- [ ] **Step 7: Confirm reverted tracing didn't sneak through**

```bash
git diff HEAD~1 HEAD -- crates/owl-dl-saturation/src/lib.rs | head -5
```
Expected: empty (only `docs/phase2b-trace.md` was added in the previous commit).

---

## Task 4: Implement the fix

**Files:**
- Modify: `crates/owl-dl-saturation/src/lib.rs` (the specific change from Task 3's "Proposed fix" section).

The exact code is what T3's analysis proposes. The most likely shape, based on the diagnostic hypothesis, is:

- Add a new method on `TseitinAllocator` named `introduce_equivalent_existential_marker(role, body, rules)` that emits BOTH the existing trigger `∃R.B ⊑ F` AND a new fact `(F, R, B)` in `rules.existential_facts`.
- In `atomic_classes_with_existential_markers` (`lib.rs:1502`), change the calls to `tseitin.introduce_existential_marker(...)` (lines 1514-1518 and 1523-1527) to use the new equivalent variant.
- Leave the one-way `introduce_existential_marker` AND its caller in the LHS-And handler at `:1263` UNCHANGED (those are for LHS-trigger semantics which need to stay asymmetric).

But if Task 3's trace points at a different bug, follow what the trace says, not this guess.

- [ ] **Step 1: Implement the fix per Task 3's proposal**

Make the code change in `crates/owl-dl-saturation/src/lib.rs`. The change should be small (one new method + 2-4 call-site swaps), bounded by what T3 documents.

If T3 proposed adding `introduce_equivalent_existential_marker`, place it on `impl TseitinAllocator` right after `introduce_existential_marker` (which lives around `lib.rs:810`). The body of the new method:

```rust
/// Like `introduce_existential_marker`, but ALSO emits the
/// existential fact `(marker, role, body)` so the marker behaves
/// equivalent to `∃R.B` in the closure — not just one-way. Used
/// when the marker is consumed inside a Tseitin synthetic body
/// that requires full ≡ semantics (`atomic_classes_with_existential_markers`),
/// where the outer synthetic's closure needs to derive that its
/// witness has the existential as a subsumer.
fn introduce_equivalent_existential_marker(
    &mut self,
    role: RoleId,
    body: ClassId,
    rules: &mut ElRules,
) -> ClassId {
    let marker = self.introduce_existential_marker(role, body, rules);
    // Emit the fact (marker, role, body) so CR5/CR9 propagation
    // can fire on the marker as if it had an explicit existential
    // in its definition.
    rules.existential_facts.push(ExistentialFact {
        sub: marker,
        role,
        target: body,
    });
    marker
}
```

Then in `atomic_classes_with_existential_markers` (`lib.rs:1502`), change:
```rust
let marker = tseitin.introduce_existential_marker(role.role_id(), inner_id, rules);
```
to:
```rust
let marker = tseitin.introduce_equivalent_existential_marker(role.role_id(), inner_id, rules);
```
in BOTH the `Some` arm (line ~1514) and the `Min` arm (line ~1523).

- [ ] **Step 2: Run the canary — expect it to FAIL (the gap-assertion no longer holds)**

```bash
cargo test -p owl-dl-saturation compound_existential_body_canary -- --test-threads=1 2>&1 | tail -10
```

Expected: the canary FAILS with the "Phase 2b canary unexpectedly passed: compound existential-body lowering appears to be fixed..." message. THAT'S CORRECT — the assertion was `!contains`, and now the closure DOES contain X ⊑ T. Task 5 will flip the assertion in the next commit.

If the canary STILL PASSES (i.e. `!contains` still true → fix didn't fire), the proposed code change doesn't close the gap. Re-trace with T3's eprintln approach and investigate. Common possibilities:
- The fact was added to `rules.existential_facts` but didn't make it into the worklist's initial fact set — check `WorklistEngine::new` for where `rules.existential_facts` is consumed.
- The marker is allocated BEFORE the closure starts, but the new fact's existence at construction-time isn't enough — it may need to be seeded into `todo_fact` explicitly.

- [ ] **Step 3: Run all saturation tests — no regression**

```bash
cargo test -p owl-dl-saturation -- --test-threads=1 2>&1 | tail -10
```

Expected: 33 tests run; 32 pass; 1 fails (the canary, intentionally). NO OTHER tests should fail.

If a non-canary test fails, the fix introduced a regression. Most likely culprits:
- The new fact `(marker, role, body)` makes the marker appear in more triggers than before, possibly firing them in cases where they shouldn't. Look for tests that assert specific subsumption-count expectations.
- The marker now has an existential fact AND a trigger, so CR5 might fire in a loop. Check `seen_facts` dedup behavior.

If a regression, stop and consult — don't ship.

- [ ] **Step 4: CI strictness + clippy clean**

```bash
RUSTFLAGS="-D warnings" cargo test -p owl-dl-saturation --no-run 2>&1 | tail -3
cargo clippy -p owl-dl-saturation -- -D warnings 2>&1 | grep -E "warning|error" | grep -v "(too_many_lines|map_unwrap_or|doc-markdown)" | head -10
```
Expected: clean compile; no new clippy on touched code.

- [ ] **Step 5: Commit (canary intentionally failing)**

```bash
git add crates/owl-dl-saturation/src/lib.rs
git commit -m "fix(saturation): emit existential fact for equivalent markers (Phase 2b)

The existential markers introduced by introduce_existential_marker
for nested ∃R.B bodies inside Tseitin synthetics were ONE-WAY
(∃R.B ⊑ F but no F ⊑ ∃R.B), which prevented the outer Tseitin
synthetic's closure from picking up sub-property / sub-class
propagation through the inner existential. The new
introduce_equivalent_existential_marker emits both the trigger and
the fact (F, R, B), used only in atomic_classes_with_existential_markers
where full ≡ semantics are needed; LHS-trigger call sites keep the
asymmetric semantics.

The compound_existential_body_canary_documents_the_gap test is
INTENTIONALLY FAILING after this commit — its assertion is
!contains(X, T), and the closure now does contain X ⊑ T. Task 5
inverts the assertion."
```

---

## Task 5: Flip the canary + extended coverage canaries

**Files:**
- Modify: `crates/owl-dl-saturation/src/lib.rs` (flip the canary's assertion + add 2 more canaries).

- [ ] **Step 1: Flip the canary's assertion**

In the canary test from Task 1, find:

```rust
    assert!(
        !subsumers.contains(x, t),
        "Phase 2b canary unexpectedly passed: ..."
    );
```

Change to:

```rust
    assert!(
        subsumers.contains(x, t),
        "Phase 2b regression: the compound existential-body fix \
         failed to derive X ⊑ T. introduce_equivalent_existential_marker \
         likely regressed."
    );
```

Rename the test from `compound_existential_body_canary_documents_the_gap` to `compound_existential_body_canary_recovers_entailment`. Update the doc comment accordingly (replace "ASSERTS THE GAP" wording with "ASSERTS THE FIX (Phase 2b rule active)").

- [ ] **Step 2: Add a cluster-A shape canary**

Cluster A pairs (FemoralHead, HeadOfHumerus, MeniscusOfKneeJoint → ExactlyPairedBodyStructure / MirrorImagedBodyStructure) involve a DIFFERENT axiom shape than pair_08 — the structure is `X ⊑ Y` where Y has an `≡ A ⊓ ∃R.X` style definition. Add a synthetic for it:

```rust
/// Phase 2b — cluster A shape canary: paired-anatomy pattern.
/// `Paired ≡ Body ⊓ ∃isPaired.Paired_self` style (the actual GALEN
/// shape) — verifies the fix carries through more complex nested
/// shapes than the simple pair_08 single-hop case.
#[test]
fn compound_existential_body_cluster_a_paired_anatomy_canary() {
    use horned_owl::io::ParserConfiguration;
    use horned_owl::io::ofn::reader::read;
    use horned_owl::model::RcStr;
    use horned_owl::ontology::set::SetOntology;
    use owl_dl_core::convert::convert_ontology;
    use std::io::Cursor;

    let src = "\
Prefix(:=<http://rustdl.test/p2bA/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://rustdl.test/p2bA/test>
    Declaration(Class(:Paired))
    Declaration(Class(:Body))
    Declaration(Class(:Limb))
    Declaration(Class(:Femur))
    Declaration(ObjectProperty(:isPaired))
    Declaration(ObjectProperty(:isLimbDivision))
    Declaration(ObjectProperty(:isBodyDivision))
    SubObjectPropertyOf(:isLimbDivision :isBodyDivision)
    SubClassOf(:Limb :Body)
    EquivalentClasses(:Paired ObjectIntersectionOf(:Body ObjectSomeValuesFrom(:isBodyDivision :Body)))
    SubClassOf(:Femur ObjectIntersectionOf(:Body ObjectSomeValuesFrom(:isLimbDivision :Limb)))
)
";
    let mut reader = Cursor::new(src);
    let (set_onto, _): (SetOntology<RcStr>, _) =
        read(&mut reader, ParserConfiguration::default()).expect("parses");
    let internal = convert_ontology(&set_onto).expect("lowers");
    let subsumers = crate::saturate(&internal);
    let femur = internal.vocabulary.class_id("http://rustdl.test/p2bA/Femur").expect("Femur declared");
    let paired = internal.vocabulary.class_id("http://rustdl.test/p2bA/Paired").expect("Paired declared");

    assert!(
        subsumers.contains(femur, paired),
        "Phase 2b cluster-A canary: Femur ⊑ Paired should derive via \
         (Femur ⊑ ∃isLimbDivision.Limb) + (isLimbDivision ⊑ isBodyDivision) + (Limb ⊑ Body)."
    );
}
```

Wait — this canary is at the OUTER existential level (NO nested-in-body case). Pair 08's bug is specifically the NESTED-existential-in-body case. The cluster-A canary might pass WITHOUT the fix (and that's fine — it'd assert the existing CR5+CR9 path already works). The test still serves to gate against regression. If you want a tighter test, add nesting to the body to match pair_08's shape more closely. Use your judgment based on T3's diagnosis.

- [ ] **Step 3: Add a deeper-nesting canary**

```rust
/// Phase 2b — deeper nesting: A ⊓ ∃R.(B ⊓ ∃S.(C ⊓ ∃U.D)). Two
/// levels of nesting, verifying the equivalent-marker fix is
/// transitive through chains.
#[test]
fn compound_existential_body_deeper_nesting_canary() {
    use horned_owl::io::ParserConfiguration;
    use horned_owl::io::ofn::reader::read;
    use horned_owl::model::RcStr;
    use horned_owl::ontology::set::SetOntology;
    use owl_dl_core::convert::convert_ontology;
    use std::io::Cursor;

    let src = "\
Prefix(:=<http://rustdl.test/p2bD/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://rustdl.test/p2bD/test>
    Declaration(Class(:T))
    Declaration(Class(:X))
    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(Class(:D_sub))
    Declaration(ObjectProperty(:R))
    Declaration(ObjectProperty(:S))
    Declaration(ObjectProperty(:U))
    Declaration(ObjectProperty(:U_sub))
    SubObjectPropertyOf(:U_sub :U)
    SubClassOf(:D_sub :D)
    EquivalentClasses(:T ObjectIntersectionOf(:A ObjectSomeValuesFrom(:R ObjectIntersectionOf(:B ObjectSomeValuesFrom(:S ObjectIntersectionOf(:C ObjectSomeValuesFrom(:U :D)))))))
    EquivalentClasses(:X ObjectIntersectionOf(:A ObjectSomeValuesFrom(:R ObjectIntersectionOf(:B ObjectSomeValuesFrom(:S ObjectIntersectionOf(:C ObjectSomeValuesFrom(:U_sub :D_sub)))))))
)
";
    let mut reader = Cursor::new(src);
    let (set_onto, _): (SetOntology<RcStr>, _) =
        read(&mut reader, ParserConfiguration::default()).expect("parses");
    let internal = convert_ontology(&set_onto).expect("lowers");
    let subsumers = crate::saturate(&internal);
    let x = internal.vocabulary.class_id("http://rustdl.test/p2bD/X").expect("X declared");
    let t = internal.vocabulary.class_id("http://rustdl.test/p2bD/T").expect("T declared");

    assert!(
        subsumers.contains(x, t),
        "Phase 2b deeper nesting canary: 2-level nested existential lowering should work."
    );
}
```

- [ ] **Step 4: Run all 3 canaries**

```bash
cargo test -p owl-dl-saturation compound_existential_body -- --test-threads=1 2>&1 | tail -15
```

Expected: all 3 pass.

If the deeper-nesting canary FAILS but the basic canary passes, the fix doesn't transitively recurse — investigate (the fact's body might need to be processed through `atomic_classes_with_existential_markers` itself).

- [ ] **Step 5: Full saturation + reasoner-lib regression sweep**

```bash
cargo test -p owl-dl-saturation -- --test-threads=1 2>&1 | tail -10
cargo test -p owl-dl-reasoner --lib -- --test-threads=1 2>&1 | tail -10
RUSTFLAGS="-D warnings" cargo test -p owl-dl-saturation --no-run 2>&1 | tail -3
```

Expected: all saturation tests pass (34 now: 32 baseline + 3 P2b canaries, the gap-asserter renamed); all 78 reasoner-lib tests pass; CI strictness compile clean.

- [ ] **Step 6: Commit**

```bash
git add crates/owl-dl-saturation/src/lib.rs
git commit -m "test(saturation): flip P2b canary + add cluster-A and deeper-nesting canaries

Inverts compound_existential_body_canary_documents_the_gap to assert
the fix recovered the entailment (renamed: _recovers_entailment).
Adds two coverage canaries:
- cluster_a_paired_anatomy: paired-anatomy-shaped pattern
- deeper_nesting: 2-level nested existential body
All three pass; no other test regresses."
```

---

## Task 6: Corpus-diff measurement on GALEN + Phase 0 net

**Files:**
- No new files. Uses the Phase 0 closure-diff harness + corpus.

The empirical question: does the fix recover ~60 of GALEN's 109 MISSED while holding FP=0 on the Phase 0 net?

- [ ] **Step 1: Phase 0 net soundness gate**

```bash
cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    alehif_closure_matches_konclude ore_10908_sroiq ore_15672_shoin \
    -- --ignored --nocapture 2>&1 | tee /tmp/p2b-net.log | grep -E "^---|^test "
```

Hard cap: 30 minutes total. Expected per fixture: `FP=0` (soundness held). MISSED counts should be ≤ pre-Phase-2b baseline (Phase 2a measured all three at MISSED=0). Wall regressions ≤ 2× acceptable.

If ANY fixture has FP > 0, that's a soundness regression. STOP. Run `rustdl classify --saturation-only ontologies/external/<failing>.ofn` per dead-end #12: if FP persists, the bug is in the saturation fix; if not, in the wedge/tableau. Report; do NOT proceed.

- [ ] **Step 2: GALEN measurement (the lever's payoff)**

```bash
timeout 2400 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    galen_closure_matches_konclude \
    -- --ignored --nocapture 2>&1 | tee /tmp/p2b-galen.log | grep -E "^--- galen|^test galen"
```

Hard cap 40 min. Expected: FP=0; MISSED drops substantially from the 109 baseline. Spec target (per `phase2b-galen-diagnosis.md`): ~60 reduction (109 → ~49 MISSED). Wall: GALEN's Phase 2a wall was 12.5 min; expect similar (the fix is a small additional emission, not a new rule).

If GALEN times out at 40 min, record the timeout. If the harness line printed even partial data, capture what's there.

- [ ] **Step 3: notgalen (secondary fixture)**

```bash
timeout 2400 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    notgalen_closure_matches_konclude \
    -- --ignored --nocapture 2>&1 | tee /tmp/p2b-notgalen.log | grep -E "^--- notgalen|^test notgalen"
```

Baseline: 27 MISSED. The Phase 2b.0 diagnosis didn't characterize notgalen — its MISSED may share the same patterns as GALEN, or may not. Either way: FP=0 must hold.

- [ ] **Step 4: Triage**

Compute and record:
- Phase 0 net FP-gate: held N/N? (Any FP > 0 = block.)
- GALEN MISSED: 109 → ?. Did the spec target (109 → ≤ 49) land?
- notgalen MISSED: 27 → ?.
- Wall regressions on small fixtures: any > 2× growth?

Capture for Task 7's results doc. Do NOT commit yet.

---

## Task 7: Results doc + close-out

**Files:**
- Create: `docs/phase2b-results.md`.
- Modify: `docs/fragment-completeness.md` (extend the "Provably complete fragment" section).
- Modify: `CLAUDE.md` (saturator description).
- Modify: `docs/superpowers/specs/2026-05-31-soundness-completeness-perf-design.md` (Phase 2 section close-out).

- [ ] **Step 1: Write `docs/phase2b-results.md`**

Use the Phase 2a results doc as a structural template. Honest about what landed:

```markdown
# Phase 2b — Compound existential-body lowering fix results

Run on <date> against the Phase 0 soundness net + GALEN + notgalen.
Fix: `introduce_equivalent_existential_marker` emits both trigger
and fact for markers used inside Tseitin synthetic bodies; one-way
markers preserved at LHS-trigger call sites. See commits in the
arc <START>..<END>.

## Headline finding

<one paragraph: did the spec target (~60 GALEN MISSED recovery) land?
If yes, by how much; if not, what landed instead. Same honesty
discipline as Phase 1's wall-time-as-filter finding and Phase 2a's
0-recovery: own the result without spin.>

## Soundness gate (Phase 0 net)

| Fixture | Pre-2b MISSED | Phase 2b MISSED | FP | Wall (Phase 2b) | Wall vs pre |
|---|---|---|---|---|---|
| alehif | 0 | <m> | 0 | <s> | <ratio>× |
| ore-10908-sroiq | 0 | <m> | 0 | <s> | <ratio>× |
| ore-15672-shoin | 0 | <m> | 0 | <s> | <ratio>× |

FP=0 held across N/N fixtures. <Note any wall regression.>

## Completeness lever (GALEN, notgalen)

| Fixture | Baseline MISSED | Phase 2b MISSED | Wall | Outcome |
|---|---|---|---|---|
| galen | 109 | <m> | <s> | <PASS / partial / FP-filed> |
| notgalen | 27 | <m> | <s> | <PASS / partial / FP-filed> |

<paragraph interpreting the GALEN and notgalen numbers vs the
~60-pair spec target.>

## What's left after Phase 2b

- Cluster F's ~25 unsampled MISSED — re-cluster against the new
  reduced MISSED set in a follow-on diagnosis.
- Cluster C+D's ~24 pairs needing functional-role + covering /
  sibling-collapse — the Phase 2b extension plan.
- Phase 3 (saturator perf) and Phase 4 (auto-gating) still queued
  per design spec.

## How to re-run

```bash
# Canaries:
cargo test -p owl-dl-saturation compound_existential_body -- --test-threads=1

# Soundness net:
cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    alehif_closure_matches_konclude ore_10908_sroiq ore_15672_shoin \
    -- --ignored --nocapture

# GALEN:
timeout 2400 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    galen_closure_matches_konclude \
    -- --ignored --nocapture
```
```

- [ ] **Step 2: Update `docs/fragment-completeness.md`**

Find the "Provably complete fragment" section. Add an item under the supported EL+ constructs list:

```
- **Compound existential-body lowering (Phase 2b, equivalent markers):**
  bodies of the shape `B ⊓ ∃S.C ⊓ …` inside a Tseitin synthetic now
  emit both the existential trigger AND the corresponding fact, so
  CR5/CR9 propagation can fire on the marker as if it had an explicit
  existential in its definition. Fixes the GALEN `JointStability`-shape
  pattern and ~N of 109 MISSED (see `phase2b-results.md`).
```

- [ ] **Step 3: Update CLAUDE.md saturator description**

Find the `crates/owl-dl-saturation` bullet in the "Workspace architecture" section. Append:

```
Phase 2b (commit <head SHA>) added equivalent-marker semantics for
nested existentials in Tseitin synthetic bodies, fixing the
`JointStability`-shape miss pattern. See `docs/phase2b-results.md`
for the corpus measurement.
```

- [ ] **Step 4: Close-out in the design spec**

In `docs/superpowers/specs/2026-05-31-soundness-completeness-perf-design.md`, find the Phase 2 section. Under the revised `2b — fix compound existential-body lowering` bullet (added by Phase 2b.0 Task 5), append:

```
Landed: `docs/phase2b-results.md`. <One-sentence summary: e.g.
"Recovered N of 109 GALEN MISSED, FP=0 held; remaining gaps split
into cluster F (unsampled tail, re-diagnosis needed) and clusters
C+D (24 pairs, separate functional-role + covering extension plan)."
>
```

- [ ] **Step 5: Commit**

```bash
git add docs/phase2b-results.md \
        docs/fragment-completeness.md \
        CLAUDE.md \
        docs/superpowers/specs/2026-05-31-soundness-completeness-perf-design.md
git commit -m "docs(phase2b): results doc + envelope updates for compound-body fix"
```

---

## Definition of done (Phase 2b)

- `introduce_equivalent_existential_marker` (or the equivalent shape T3's trace justified) is wired into `atomic_classes_with_existential_markers` (Task 4).
- Three canaries in `mod tests` exercise the basic, cluster-A, and deeper-nesting shapes; all pass (Tasks 1, 5).
- HermiT cross-check on the basic canary (Task 2) confirmed the entailment is reachable from a sound+complete reasoner.
- T3's trace doc (`docs/phase2b-trace.md`) records what the actual bug was AND the proposed fix — honest engineering trail.
- Phase 0 net FP=0 held under the fix (Task 6).
- GALEN MISSED measurement recorded (Task 6); spec target was ~60 reduction — outcome doc'd in `docs/phase2b-results.md` (Task 7) regardless of whether target landed exactly.
- `docs/fragment-completeness.md`, `CLAUDE.md`, design spec all reflect Phase 2b's landed state (Task 7).

This closes Phase 2b's main scope. Phase 2b extension (functional-role + covering for cluster C+D, ~24 pairs) is a separate plan written after this one's measurement informs its scope.
