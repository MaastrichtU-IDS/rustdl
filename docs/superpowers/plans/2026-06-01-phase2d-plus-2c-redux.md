# Phase 2d + 2c-redux Combined Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Recover the IPBP-cluster MISSED on GALEN (17 pairs) and notgalen (27 pairs) by layering two architectural changes: (Phase 2d) materialize inherited existential facts on subclasses at `process_subsumer` time, and (Phase 2c-redux) re-apply the previously-reverted sub-role witness-propagation rule (commit b83fcd6, reverted at cc2019e) on top of 2d's now-populated `facts_by_sub`. Hold FP=0 throughout.

**Architecture:** Two-layered change to `crates/owl-dl-saturation/src/lib.rs`. Phase 2d adds fact-copy logic at two propagation points: (a) `process_subsumer(c, d)` copies all `(D, role, target)` facts to `(C, role, target)`, and (b) `push_fact(D, role, target)` propagates the new fact to every subclass of D. Phase 2c-redux re-applies the Phase 2c rule unchanged, which now fires because subclasses have inherited facts. Both layers are gated by measurement gates: Phase 2d ships only if FP=0 + GALEN wall regression < 10% + memory blowup < 2×; Phase 2c-redux ships only if combined GALEN MISSED drops AND combined wall regression vs pre-2d baseline < 15%. Either layer can be reverted independently.

**Tech Stack:** Rust (edition 2024), `owl-dl-saturation` crate. Reuses Phase 2a `WorklistEngine`, `facts_by_sub` / `facts_by_target` indices, `record_subsumer` / `enqueue_subsumer` propagation, `existential_triggers_by_body`, and the Phase 2c rule code preserved in commit b83fcd6.

---

## Background the executor needs

### The two-layer architecture

**Phase 2d** addresses the gap identified in the Phase 2c T4 finding (`docs/phase2c-fix-target.md` §"Predicted walkthrough… (and what actually happened)") and dead-end §15:
- ELK saturator stores `ExistentialFact { sub, role, target }` on the class that DEFINES the existential, not on subclasses.
- Subclass C inheriting `C ⊑ D` semantically inherits D's existentials, but `facts_by_sub[C]` does NOT contain them.
- `process_subsumer` at `lib.rs:451-550` already does sub-side existential-TRIGGER firing (lines 521-542) — checks D's facts and fires triggers whose body matches the fact's target-subsumers. But the trigger's role-hierarchy check (`fact_role_supers.contains(&trigger.role)`) requires the trigger's role to be a SUPER of the fact's role.
- The pair_06 case fails because the IPBP trigger needs `hasIntrinsicPathologicalStatus` but available facts are on `hasPathologicalStatus` — sibling sub-properties of `StatusAttribute`, neither a super of the other.

**Phase 2c-redux** re-applies the Phase 2c rule from commit b83fcd6 (reverted at cc2019e):
- Phase 2a's functional-role witness-merge accumulates atom-set over a functional super R_f.
- Phase 2c rule: at emission of `(X, R_f, synthetic)`, also emit `(X, R_k, synthetic)` for every sub-role R_k on which X has a fact.
- After Phase 2d populates `facts_by_sub[ICF]` with inherited facts, Phase 2c-redux's rule has the preconditions to fire on ICF.

### Why this needs Phase 2d to fire

`facts_by_sub[ICF]` BEFORE Phase 2d: contains 1 fact (per Phase 2c T4 trace: `(ICF, RoleId(40), ClassId(195))`).
`facts_by_sub[ICF]` AFTER Phase 2d: should contain inherited facts including `(ICF, hasIntrinsicPathologicalStatus, physiological)` (from NAMEDPhysiologicalProcess super) and `(ICF, hasPathologicalStatus, pathological)` (from PathologicalBodyProcess super).

With both inherited, Phase 2a's merge fires on (ICF, StatusAttribute) → merged synthetic; Phase 2c-redux's rule then propagates synthetic to (ICF, hasIntrinsicPathologicalStatus, synthetic); existential trigger for IPBP matches via target-subsumer propagation (synthetic ⊑ pathological). ICF ⊑ IPBP closes.

### Risk surface (advisor's flags)

1. **Memory blowup**: every class inherits every super's existentials. Worst case O(classes × max_facts_per_class). GALEN has ~27,000 classes; potentially 100K+ existential facts × multiple subsumer paths. Memory could 2-10×. Mitigation: dedup; selective propagation (only inherit facts whose target's subsumer set contains some trigger body that the subclass could reach).
2. **Termination**: more facts → more rule firings → potentially more facts. Phase 2a's atom-set merge IS bounded (per Phase 2a results); Phase 2c's rule is bounded by `|atomic_vocab|² × |sibling_roles|`. The compound termination needs re-verification — especially that Phase 2c's "inner loop iterates facts_by_sub[X]" doesn't unbounded-cascade when X's fact set keeps growing via Phase 2d copies.
3. **Soundness re-argument**: Phase 2c's witness-coincidence invariant assumed facts represent ground-truth told existentials. With Phase 2d, inherited facts share witnesses with the parent class. Need to verify: the model-theoretic semantics ARE preserved by inheritance (if `(D, role, target)` is true in model M and `C ⊑ D`, then every C-instance is a D-instance with the same role witness). The "same individual" semantics that functional super-role merge requires should still hold — but verify against pair_06's specific axiom set.
4. **Wall cost compounding**: Phase 2d adds fact-copy work at every subsumer-add; Phase 2c-redux iterates the larger `facts_by_sub[X]` per emission. Combined wall regression on GALEN could be substantial.

### Measurement gates (hard)

- **After Phase 2d alone (T4)**: FP=0 (gate); GALEN wall regression < 10% (cap); GALEN MISSED unchanged (expected — 2c not on yet); memory peak < 2× pre-2d baseline (informational). If gate fails: REVERT 2d, abandon plan.
- **After Phase 2c-redux on top (T6)**: FP=0 (gate); GALEN wall regression vs pre-2d baseline < 15% (cap); GALEN MISSED dropped from 17 (success signal). If gate fails: REVERT 2c-redux only (keep 2d as foundation); if 2c-redux gives < 6 MISSED reduction, also revert 2d (no net benefit).

### Branch + revert state

- Current HEAD: `8c38b5b` (Phase 3f §17 entry).
- Phase 2c code lives at commit `b83fcd6` (reverted at `cc2019e`); restore by `git show b83fcd6 -- crates/owl-dl-saturation/src/lib.rs` or by manual re-port.
- Branch: `plan/soundness-completeness-perf`, 96 commits since main, all green, FP=0 throughout.

**ENV NOTE for executor**: cargo and rustc are NOT on the default shell PATH:
```bash
export PATH=/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH
```

---

## Pass 1: Phase 2d (fact-on-subclass propagation)

### Task 1: Design — propagation mechanism + soundness + termination

**Files:**
- Read: `crates/owl-dl-saturation/src/lib.rs:451-760` (process_subsumer + push_fact + Phase 2a witness-merge block).
- Read: `docs/phase2c-fix-target.md` (the existing analysis of pair_06's fact state).
- Read: `docs/hypertableau-dead-ends.md` §15 (Phase 2c's revert rationale).
- Create: `docs/phase2d-design.md` (committed).

T1's deliverable: a design doc specifying EXACTLY (a) where the propagation happens in code, (b) which facts get propagated (all vs selective), (c) how deps are tracked on inherited facts, (d) termination argument, (e) memory estimate, (f) soundness argument.

- [ ] **Step 1: Read the saturator's subsumer / fact propagation**

```bash
grep -nE "fn process_subsumer|fn push_fact|fn record_subsumer|fn enqueue_subsumer|facts_by_sub\b|subs_of_class" crates/owl-dl-saturation/src/lib.rs | head -30
```

Read `process_subsumer` (lines 451-594) thoroughly. Identify:
- WHERE `facts_by_sub[d]` is iterated currently (lines 521-542 for trigger-firing).
- WHERE `push_fact` is called and what propagation it does today.
- HOW deps would be tracked on an inherited fact (Phase 2c's saturator doesn't appear to carry per-fact deps explicitly — verify).
- WHAT the subs_of_class / supers_of_class APIs look like for the propagation chain.

- [ ] **Step 2: Decide propagation strategy**

Two candidate strategies:
- **Strategy A (all-facts)**: at `process_subsumer(c, d)`, copy every fact from `facts_by_sub[d]` into `facts_by_sub[c]` (with dedup). At `push_fact(D, role, target)`, also push into `facts_by_sub[s]` for every s ∈ subs_of_class(D). Simple, complete; worst case memory ~quadratic.
- **Strategy B (selective)**: only inherit facts where there's a downstream existential trigger whose body is in the target's subsumer set (or could be). Reduces memory; complicates correctness.

Recommend Strategy A unless GALEN's memory estimate from Step 5 is prohibitive (>4× baseline). T1's design picks one.

- [ ] **Step 3: Specify the dedup invariant**

`(C, role, target)` already in `facts_by_sub[C]` ⇒ skip. The fact's identity is `(sub, role, target)` — three integer fields, easy to dedup via a HashSet or a `BTreeSet`-keyed seen check.

Identify: does the saturator already have a `seen_facts` mechanism? (Phase 2c's reverted code used `seen_facts.insert((sub, role, target))` — confirm by reading b83fcd6's lib.rs.)

- [ ] **Step 4: Termination argument**

Per-class bounded: `facts_by_sub[C]` is bounded by the total number of distinct `(role, target)` pairs across all `facts_by_sub[D]` for D in supers_of_class(C). This is bounded by total facts in the system, which is bounded by `|atomic_vocab| × |roles|` plus synthetics.

The synthetics are introduced by Phase 2a/2b and bounded by atom-set cardinality.

Phase 2d's propagation doesn't introduce new TYPES of facts — only copies. So termination holds by the existing bounded total-fact-count argument.

- [ ] **Step 5: Memory estimate**

Estimate: on GALEN, current facts count ≈ ? (read counter or instrument briefly). Pessimistic bound: total_facts × avg_subsumer_depth = ?. Document the estimate; if it's > 5× baseline, flag as concern for T4 to measure carefully.

- [ ] **Step 6: Soundness argument**

For each inherited fact `(C, role, target)` to be sound: if `C ⊑ D` and `(D, role, target)` is sound, then every model M with C-instance c has c ∈ M(D), so there exists a `(role, target)` witness for c — the inherited fact represents EXACTLY the same model-theoretic content. This is the same argument that makes existential propagation via subsumers sound (which the saturator already does — see `process_subsumer` lines 521-542).

Phase 2c's "witness exists" precondition: with Phase 2d, the inherited fact represents a witness in models, even if the saturator doesn't track which individual. The witness-coincidence argument (Phase 2c-redux) extends because the model-theoretic semantics are preserved.

- [ ] **Step 7: Write `docs/phase2d-design.md`**

Structure:

```markdown
# Phase 2d — design

## Propagation points

<list the code locations: process_subsumer + push_fact; what each does>

## Strategy choice

<A or B; rationale>

## Dedup invariant

<the (sub, role, target) seen-set; where it lives>

## Termination

<the bounded-fact-count argument>

## Memory estimate

<for GALEN; flag if > 5× baseline>

## Soundness

<inherited fact represents same model-theoretic witness as parent's fact>

## Code-change surface

<which functions in crates/owl-dl-saturation/src/lib.rs; estimated LoC>

## What this design does NOT do

- Does NOT re-introduce the Phase 2c sub-role witness propagation rule
  (that's Phase 2c-redux in Pass 2 of this plan).
- Does NOT change EL+ rule semantics; only adds fact-copy.
```

- [ ] **Step 8: Commit**

```bash
git add docs/phase2d-design.md
git commit -m "perf(phase2d): design — fact-on-subclass propagation"
```

If memory estimate exceeds 5× baseline and Strategy A is infeasible, document Strategy B with selective-propagation rules. If neither is feasible (memory bounds even with selectivity), report DONE_WITH_CONCERNS — Phase 2d isn't workable.

---

### Task 2: Lock baseline

```bash
export PATH=/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH
timeout 1500 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    galen_closure_matches_konclude \
    -- --exact --ignored --nocapture 2>&1 | tee /tmp/p2d-baseline-galen.log | grep -E "^--- galen|MISSED=|FP=|finished"
```

Expected: FP=0, MISSED=17, wall ~12-13 min.

Also capture rough fact-count baseline by instrumenting `WorklistEngine::run` to print `self.facts.len()` at the end. Add a temporary `eprintln!("facts.len()={}", self.facts.len());` before the function returns, run the test once, capture the count, then REVERT the eprintln. This is needed for T4's memory comparison.

No commit (eprintln is temporary; the cargo test result is the baseline).

---

### Task 3: Implement Phase 2d

**Files:**
- Modify: `crates/owl-dl-saturation/src/lib.rs::WorklistEngine` (process_subsumer + push_fact + possibly a new method).
- Modify: `crates/owl-dl-saturation/src/lib.rs::WorklistEngine` (add `phase2d_facts_inherited: u64` counter; gate `cfg(feature = "counters")` or stats struct per existing pattern).
- Modify: `crates/owl-dl-saturation/src/lib.rs::mod tests` (synthetic canary: minimal ontology with A ⊑ B + (B, R, T) fact; assert (A, R, T) materializes after saturate).

Implementation per T1 design. Surgical: add inherit-on-subsumer + inherit-on-push-fact paths; reuse seen_facts dedup; do NOT change other rule semantics.

- [ ] **Step 1: Add inherit logic in `process_subsumer`**

After line 525 (existing sub-side trigger-firing), add a block that materializes inherited facts:

```rust
// Phase 2d: materialize D's existential facts on C in facts_by_sub[c].
// When C newly has D as subsumer, every existential fact on D
// represents a witness that C-instances also have (model-theoretically:
// C ⊑ D ⇒ every C-instance is a D-instance with the same witness).
// Sound by the standard ELK existential-propagation argument; the
// existing sub-side trigger-firing at lines 521-542 already exploits
// this semantically — Phase 2d materializes the fact explicitly so
// fact-time rules (Phase 2a witness-merge, future Phase 2c-redux)
// can see it.
//
// See docs/phase2d-design.md for the termination + soundness arguments.
for fidx in self.facts_by_sub[d.index() as usize].clone() {
    let fact = self.facts[fidx];
    let inherited = ExistentialFact { sub: c, role: fact.role, target: fact.target };
    if self.seen_facts.insert((inherited.sub, inherited.role, inherited.target)) {
        let new_idx = self.facts.len();
        self.facts.push(inherited);
        self.facts_by_sub[inherited.sub.index() as usize].push(new_idx);
        self.facts_by_target[inherited.target.index() as usize].push(new_idx);
        self.todo_fact.push(new_idx);   // enqueue for processing
        self.phase2d_facts_inherited += 1;
    }
}
```

Adapt to actual struct names + field types (seen_facts may not exist by that name; the Phase 2c reverted code at b83fcd6 used one — restore the pattern). Verify the right way to push into the worklist (todo_fact? or a different mechanism).

- [ ] **Step 2: Add inherit logic in `push_fact`**

When `push_fact(D, role, target)` adds a new fact on D, also propagate to every subclass of D:

```rust
// At the end of push_fact, after the new fact is inserted, propagate
// to every subclass of D. (See process_subsumer's symmetric path.)
let new_fact_idx = idx;  // the index just inserted
let original_sub = fact.sub;
let subs_of_original = self.subs_of_class(original_sub);
for c in subs_of_original {
    let inherited = ExistentialFact { sub: c, role: fact.role, target: fact.target };
    if self.seen_facts.insert((inherited.sub, inherited.role, inherited.target)) {
        let inherited_idx = self.facts.len();
        self.facts.push(inherited);
        self.facts_by_sub[inherited.sub.index() as usize].push(inherited_idx);
        self.facts_by_target[inherited.target.index() as usize].push(inherited_idx);
        self.todo_fact.push(inherited_idx);
        self.phase2d_facts_inherited += 1;
    }
}
```

Adapt similarly. Verify `subs_of_class` is the right API.

- [ ] **Step 3: Add the counter**

```rust
// In WorklistEngine struct:
phase2d_facts_inherited: u64,
```

Initialize to 0 in the constructor.

- [ ] **Step 4: Add a structural canary**

In `mod tests`, add a test that constructs a minimal ontology:
```
A ⊑ B
B ⊑ ∃R.T
```

After saturate, assert: `subsumers.contains(A, ∃R.T_introduced_atom)` AND the counter `phase2d_facts_inherited > 0`.

(Adapt: the existential atom may need an explicit IRI / synthetic introduction; mirror existing Phase 2a/2b canaries.)

- [ ] **Step 5: Regression sweep**

```bash
export PATH=/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH
cargo test -p owl-dl-saturation -- --test-threads=1 2>&1 | tail -10
cargo test -p owl-dl-reasoner --lib -- --test-threads=1 2>&1 | tail -10
RUSTFLAGS="-D warnings" cargo test -p owl-dl-saturation --no-run 2>&1 | tail -3
cargo clippy -p owl-dl-saturation --all-targets -- -D warnings 2>&1 | grep -E "warning|error" | head -10
```

All green; CI strict clean. If any pre-existing test fails (esp. Phase 2a/2b canaries), STOP — the inheritance logic broke a semantic invariant.

- [ ] **Step 6: Commit**

```bash
git add crates/owl-dl-saturation/src/lib.rs
git commit -m "feat(saturation): fact-on-subclass propagation (Phase 2d)

When C newly has D as subsumer, materialize every existential fact
(D, role, target) as (C, role, target) on facts_by_sub[c]. Symmetric
path in push_fact: when a new fact lands on D, propagate to every
subclass of D.

Soundness: inherited fact represents the same model-theoretic witness
as parent's fact (C ⊑ D ⇒ every C-instance is a D-instance with the
same witness). Termination: bounded by existing total-fact-count
invariant. See docs/phase2d-design.md.

This is the architectural prerequisite for Phase 2c-redux (dead-end
§15). Phase 2c-redux re-applies in a follow-up commit; Phase 2d
alone is expected to leave GALEN MISSED=17 unchanged."
```

---

### Task 4: Measure Phase 2d alone — Phase 0 net + GALEN + fact count

**Files:**
- Capture: `/tmp/p2d-net.log`, `/tmp/p2d-galen.log`.

**Expected**: FP=0 + MISSED=0 unchanged on Phase 0 net; FP=0 + MISSED=17 unchanged on GALEN; wall regression < 10%; fact count grows but bounded.

- [ ] **Step 1: Phase 0 net (30 min cap)**

```bash
export PATH=/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH
timeout 1800 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    -- --ignored --nocapture alehif_closure_matches_konclude ore_10908_sroiq ore_15672_shoin \
    2>&1 | tee /tmp/p2d-net.log | grep -E "^---|FP=|MISSED=|test result"
```

FP > 0 → REVERT 2d immediately.

- [ ] **Step 2: GALEN (25 min cap, clean — no concurrent load)**

```bash
timeout 1500 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    galen_closure_matches_konclude \
    -- --exact --ignored --nocapture 2>&1 | tee /tmp/p2d-galen.log | grep -E "^--- galen|MISSED=|FP=|finished"
```

Expected: FP=0, MISSED=17 (unchanged), wall ≤ 14 min (10% cap on 12.33 baseline).

- [ ] **Step 3: Capture fact count + counter value**

Add temporary `eprintln!("facts.len()={} phase2d_inherited={}", self.facts.len(), self.phase2d_facts_inherited)` in saturate's end; re-run any small ontology test; capture; revert.

- [ ] **Step 4: Triage gate**

| Criterion | Pass | Fail action |
|---|---|---|
| Phase 0 net FP=0 / MISSED=0 | Pass | REVERT 2d, abandon plan |
| GALEN FP=0 | Pass | REVERT 2d, abandon plan |
| GALEN MISSED unchanged at 17 | Pass | Investigate — unexpected change |
| GALEN wall regression < 10% | Pass | Continue to Pass 2 |
| GALEN wall regression 10-30% | Investigate — Phase 2c-redux will worsen | Likely abandon |
| GALEN wall regression > 30% | Fail | REVERT 2d, abandon plan |
| Fact count growth < 5× | Continue | Continue (informational) |
| Fact count growth > 10× | Investigate | Tighten propagation |

No commit; T5 captures intermediate results.

---

### Task 5: Phase 2d intermediate results doc

**Files:**
- Create: `docs/phase2d-intermediate-results.md` (committed).

A brief intermediate-state doc capturing T4's measurements. Pass 2 either proceeds (if T4 gates passed) or this doc becomes the final write-up (if gates failed and the plan ends here).

```markdown
# Phase 2d intermediate results

Phase 2d (fact-on-subclass propagation) measurement after layer 1
landed at commit <SHA>, before Phase 2c-redux is applied.

## Soundness gate (Phase 0 net)

<table>

## Wall + memory lever (GALEN)

| Metric | Baseline | Post-2d | Δ |
|---|---|---|---|
| FP | 0 | <X> | — |
| MISSED | 17 | <X> | — |
| Wall | <baseline_min> | <post_min> | <pct>% |
| `facts.len()` | <X> | <Y> | <ratio>× |
| `phase2d_facts_inherited` counter | — | <Z> | — |

## Triage decision

<continue to Pass 2 OR abandon; rationale>

## Cross-references

- Phase 2d design: `docs/phase2d-design.md`
- Phase 2d implementation commit: <SHA>
- Phase 2c original (reverted): commit b83fcd6 → reverted at cc2019e
- Dead-end §15: the prerequisite this addresses
```

Commit. Then either proceed to Pass 2 (if gates passed) or close out + add dead-end §18 (if gates failed).

---

## Pass 2: Phase 2c-redux (only if Pass 1 gates passed)

### Task 6: Restore Phase 2c rule on top of Phase 2d

**Files:**
- Modify: `crates/owl-dl-saturation/src/lib.rs::WorklistEngine::process_fact` (the Phase 2a witness-merge block).

Phase 2c's code was at commit b83fcd6 (reverted at cc2019e). The 59-line diff added an inner loop in the Phase 2a witness-merge emission block + a counter `phase2c_sub_role_propagations`. Restore.

- [ ] **Step 1: Inspect the original Phase 2c diff**

```bash
git show b83fcd6 -- crates/owl-dl-saturation/src/lib.rs | head -120
```

Identify the inner loop block + the counter field.

- [ ] **Step 2: Re-apply manually**

Don't `git cherry-pick b83fcd6` — that would also re-introduce the structural canary which tests the now-reverted state. Instead:
- Manually re-add the inner loop (the +59 lines of rule code).
- Re-add the counter field on `WorklistEngine`.
- Re-add the structural canary (it tested a 4-fan-in synthetic; the canary semantics are still valid).

Per the original Phase 2c implementation (per `docs/phase2c-fix-target.md` §"Rule design"):

```rust
// In process_fact, after Phase 2a's emission of (X, R_f, synthetic):
for &other_idx in &facts_by_sub[fact.sub.index() as usize].clone() {
    let other = self.facts[other_idx];
    if other.role == fact.role {
        continue;  // R_arr's emission covered by Phase 2a's R_f path
    }
    if !self.rules.functional_supers_of(other.role).contains(&rf) {
        continue;
    }
    let new_fact = ExistentialFact { sub: x, role: other.role, target: synthetic };
    if self.seen_facts.insert((new_fact.sub, new_fact.role, new_fact.target)) {
        // ... same push pattern as Phase 2a ...
        self.phase2c_sub_role_propagations += 1;
    }
}
```

- [ ] **Step 3: Restore the structural canary**

The canary from b83fcd6 (`phase2c_sub_role_propagation_counter_bumps_on_4_fan_in`) asserted that on a 4-sub-property fan-in synthetic, the counter bumps. Restore it.

- [ ] **Step 4: Update the pair_06 canary**

The pair_06 canary at `crates/owl-dl-reasoner/tests/phase2c_pair_06_canary.rs` is currently gap-asserting (`!subsumers.contains(ccf, ipbp)`). With Phase 2d + 2c-redux landed, it SHOULD pass the entailment. Flip:
- Rename: `phase2c_pair_06_saturator_still_misses_target_subsumption_known_limitation` → `phase2c_pair_06_saturator_recovers_target_subsumption_via_phase2d`.
- Invert: `!subsumers.contains(ccf, ipbp)` → `subsumers.contains(ccf, ipbp)`.
- Update doc comment to explain the recovery path.

- [ ] **Step 5: Regression sweep**

```bash
cargo test -p owl-dl-saturation --features counters -- --test-threads=1 2>&1 | tail -10
cargo test -p owl-dl-saturation -- --test-threads=1 2>&1 | tail -10
cargo test -p owl-dl-reasoner --lib -- --test-threads=1 2>&1 | tail -5
cargo test -p owl-dl-reasoner --tests -- --test-threads=1 2>&1 | tail -10
RUSTFLAGS="-D warnings" cargo test -p owl-dl-saturation --no-run 2>&1 | tail -3
```

Expected: all green. The pair_06 canary's recovery is the key signal — if it doesn't flip, Phase 2c-redux's rule isn't firing on ICF even with inherited facts. Investigate.

- [ ] **Step 6: Commit**

```bash
git add crates/owl-dl-saturation/src/lib.rs \
        crates/owl-dl-reasoner/tests/phase2c_pair_06_canary.rs
git commit -m "feat(saturation): Phase 2c-redux on top of Phase 2d

Re-apply the previously-reverted (cc2019e) Phase 2c sub-role witness
propagation rule. Now fires because Phase 2d populates facts_by_sub[X]
with inherited facts from X's super-classes, so the merge has the
preconditions to extend back to sub-roles where downstream triggers
live.

The pair_06 canary flips from gap-asserting to entailment-recovering:
ICF now has both (ICF, hasIntrinsicPathologicalStatus, physiological)
[inherited from NAMEDPhysiologicalProcess] and (ICF, hasPathologicalStatus,
pathological) [inherited from PathologicalBodyProcess]; Phase 2a's
StatusAttribute-merge then triggers Phase 2c-redux's sub-role
propagation; existential trigger for IntrinsicallyPathologicalBodyProcess
matches via target-subsumer propagation.

See docs/phase2c-fix-target.md for the original soundness argument
(unchanged); docs/phase2d-design.md for the inheritance argument that
gives Phase 2c-redux the witness-existence precondition."
```

---

### Task 7: Combined corpus measurement

Mirror Phase 2c T5 measurement pattern. Run ALL of:
- Phase 0 net (FP=0 gate)
- GALEN (FP=0 + MISSED + wall)
- notgalen (FP=0 + MISSED + wall)
- Optional: pizza / ro / sulo / sio sanity checks if quick

Expected (per Phase 2c.0 diagnosis): GALEN MISSED reduction in the 12-20 range (the IPBP-cluster); notgalen MISSED reduction in the 12-20 range. Combined wall regression vs pre-2d baseline < 15%.

- [ ] **Step 1: Build the test binary**

```bash
cargo test -p owl-dl-reasoner --test konclude_closure_diff --release --no-run 2>&1 | tail -3
```

- [ ] **Step 2: Phase 0 net soundness gate (30 min cap)**

```bash
timeout 1800 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    -- --ignored --nocapture alehif_closure_matches_konclude ore_10908_sroiq ore_15672_shoin \
    2>&1 | tee /tmp/p2d-final-net.log | grep -E "^---|FP=|MISSED=|test result"
```

FP > 0 → REVERT both layers, abandon.

- [ ] **Step 3: GALEN clean (40 min cap)**

```bash
timeout 2400 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    galen_closure_matches_konclude \
    -- --exact --ignored --nocapture 2>&1 | tee /tmp/p2d-final-galen.log | grep -E "^--- galen|MISSED=|FP=|finished"
```

Expected: FP=0; MISSED ≤ 5 (reduction of 12+ pairs); wall ≤ 14.5 min (15% cap on 12.33 baseline).

If GALEN wall regresses >15%: investigate. The combined cost may not be worth the recovery. Consider reverting 2c-redux (keep 2d as foundation) and re-measuring whether 2d alone gives any benefit elsewhere.

- [ ] **Step 4: notgalen clean (40 min cap)**

```bash
timeout 2400 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    notgalen_closure_matches_konclude \
    -- --ignored --nocapture 2>&1 | tee /tmp/p2d-final-notgalen.log | grep -E "^--- notgalen|MISSED=|FP=|finished"
```

Expected: FP=0; MISSED reduction; wall regression < 15% vs prior notgalen baseline.

- [ ] **Step 5: Triage**

| Criterion | Pass | Fail action |
|---|---|---|
| Phase 0 net FP=0 | Pass | REVERT both |
| GALEN FP=0 | Pass | REVERT both |
| GALEN MISSED dropped ≥ 6 | Real win | Continue |
| GALEN MISSED dropped 1-5 | Marginal — re-think | Consider revert 2c-redux only |
| GALEN MISSED unchanged | Phase 2c-redux didn't fire | Investigate; likely REVERT both |
| GALEN wall regression < 15% | Acceptable cost | Continue |
| GALEN wall regression 15-30% | High cost — weigh against MISSED gain | Per-case decision |
| GALEN wall regression > 30% | Unacceptable | REVERT 2c-redux only (try 2d-alone results) |
| notgalen MISSED dropped ≥ 6 | Real win | Continue |
| pair_06 canary passes | Expected | Continue |

No commit; T8 captures results.

---

### Task 8: Final results doc + envelope updates

If T7 verdict is SHIP both layers, mirror Phase 2c-results-doc-shape but flip the verdict:

**Files:**
- Create: `docs/phase2d-2c-redux-results.md`.
- Update: `CLAUDE.md` (saturator paragraph).
- Update: `docs/superpowers/specs/2026-05-31-soundness-completeness-perf-design.md`.
- Update: `docs/hypertableau-dead-ends.md` §15 (mark "resolved by Phase 2d + 2c-redux at commit <SHA>").

```markdown
# Phase 2d + 2c-redux combined results

Run 2026-06-0N. Phase 2d (fact-on-subclass propagation, commit <SHA>)
+ Phase 2c-redux (sub-role witness propagation re-applied on top,
commit <SHA>). See `docs/phase2d-design.md` for the propagation
mechanism and `docs/phase2c-fix-target.md` for the witness-coincidence
rule (unchanged from original Phase 2c).

## Headline

<one paragraph: GALEN MISSED Δ, notgalen MISSED Δ, wall costs, FP gate>

## Soundness gate (Phase 0 net)

<table>

## Completeness lever (GALEN + notgalen)

<table with pre-2d / post-2d-only / post-2d+2c-redux>

## Wall cost (the price of recovery)

<table>

## Memory cost

<facts.len() growth>

## Cross-references

- §15 (the prerequisite this addresses): mark resolved
- Phase 2c original implementation: b83fcd6 → cc2019e (reverted)
- Phase 2d design: phase2d-design.md
- Phase 2d intermediate results: phase2d-intermediate-results.md
- Final implementation: <commit SHAs>
```

If verdict is partial-revert (revert 2c-redux only, keep 2d) or full-revert, document as dead-end ledger entry §18.

Commit per existing Phase pattern (single docs commit).

---

## Definition of done

- Phase 2d ships sound; either alone (gate passed but 2c-redux didn't fire) or with 2c-redux on top (the full recovery).
- OR plan fully reverted at any gate, with dead-end §18 documenting findings.
- Either way: Phase 0 net FP=0 + MISSED=0 held.
- pair_06 canary flipped to recovery-asserting (if both layers ship) OR remains gap-asserting (if any layer reverts).

## What this plan does NOT do

- Does NOT touch the hypertableau wedge or tableau engines.
- Does NOT change env-flag defaults.
- Does NOT add new rules beyond fact-inheritance (2d) + restoring Phase 2c (redux).
- Does NOT promise corpus recovery — that's what T7 measures.

## Estimated session count

Realistically multi-session:
- Pass 1 (2d): 1-2 sessions (T1 design + T3 implement are non-trivial; T4 measurement is ~1 hour of bench wall).
- Pass 2 (2c-redux): 1 session if Pass 1 succeeds (T6 is mostly cherry-pick + flip the canary).
- Total: 2-3 sessions.

If memory/wall costs are prohibitive in Pass 1, the plan ends after T5 with a dead-end ledger entry — no Pass 2.
