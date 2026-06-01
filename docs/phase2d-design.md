# Phase 2d — design

> **Task 1 of `docs/superpowers/plans/2026-06-01-phase2d-plus-2c-redux.md`.**
> Design-only (no production-code changes). Specifies the propagation
> mechanism that materializes inherited existential facts on subclasses,
> enabling Phase 2c-redux (Pass 2) to fire on classes that currently
> only inherit a single fact directly (e.g. pair_06's `IneffectiveCardiacFunction`).

## Goal

Close the §15 architectural gap: the ELK saturator already exploits
subsumer-driven existential propagation *semantically* in
`process_subsumer`'s sub-side trigger-firing block (lines 521-542 in
`crates/owl-dl-saturation/src/lib.rs`), but does NOT materialize the
inherited fact on `facts_by_sub[c]`. Phase 2c relied on a populated
`facts_by_sub[X]` to find sibling sub-role witnesses; without explicit
materialization the rule fires only on classes with two or more facts
directly attached. Phase 2d materializes; Phase 2c-redux then applies.

## Propagation points

Two locations in `crates/owl-dl-saturation/src/lib.rs`, both inside
`impl WorklistEngine`:

### Point A — `process_subsumer(c, d)` at lines 451-591

When a freshly-derived `(c, d)` edge enters the closure, the existing
sub-side trigger-firing block at lines 521-557 already snapshots
`self.facts_by_sub[d.index()]` and walks each fact's
`target_subsumers × trigger_idxs × role_supers`. Phase 2d adds a
parallel block immediately after (or fused into) this loop: for every
fact `(d, role, target)` in `facts_by_sub[d]`, also push the inherited
fact `(c, role, target)` into the engine via `push_fact`.

### Point B — `push_fact(fact)` at lines 437-447

When a new fact `(D, role, target)` is inserted, propagate to every
subclass `c ∈ subs_of_class(D)`. The propagation re-enters `push_fact`
for the inherited `(c, role, target)`.

Without Point B, a fact that becomes attached to `D` *after* the
subsumer edge `c ⊑ D` was already recorded would never reach `c`
(`process_subsumer` runs once per edge insert; it doesn't re-fire when
a new fact arrives on `D` later). Both points are needed for
completeness of the materialization.

## Strategy choice

**Strategy A (all-facts), with dedup.** Copy every fact from
`facts_by_sub[d]` to `facts_by_sub[c]`; symmetric propagation in
`push_fact`. Rejected Strategy B (selective propagation gated on a
filter predicate) on three grounds:

1. **Forward dependency on a not-yet-built rule.** Strategy B's filter
   would have to predict which inherited facts a future Phase 2c-redux
   call would consult on `facts_by_sub[X]`. The merge-side rule
   iterates ALL of `facts_by_sub[fact.sub]` and filters by
   `functional_supers_of(other.role).contains(&rf)` — a runtime
   predicate over the role-hierarchy that isn't known until merge
   time. Encoding it as a static filter is fragile.
2. **Memory absolute is small.** Baseline measurements (Step E) show
   `facts.len()` ≤ 3.4K on the largest in-corpus ontology (notgalen).
   Even a pessimistic 50× blowup is ~170K facts × ~24 bytes ≈ 4 MB.
   Strategy B's complexity savings buy nothing meaningful here.
3. **Soundness surface is smaller for A.** A copies model-theoretically
   equivalent facts; B introduces a filter whose correctness depends
   on knowing every downstream rule that consumes `facts_by_sub` is
   conservative w.r.t. the filter.

## Dedup invariant

The saturator already maintains `seen_facts: HashSet<(ClassId, RoleId,
ClassId)>` (lib.rs:106) which `push_fact` consults at line 438. **Phase
2d reuses this set unchanged.** Every inherited fact is offered to
`push_fact`; duplicates are silently dropped. No new dedup mechanism
is needed.

Confirmed: Phase 2c's reverted implementation at commit b83fcd6 used
the same `seen_facts.insert(...)` pattern inline (b83fcd6 diff lines
+794..+811). The Phase 2d insertion path goes through `push_fact`
rather than re-implementing the seen check inline.

## Termination

The propagation copies existing facts; it does **not** introduce new
*types* of facts. Concretely:

- The set of all reachable `(sub, role, target)` triples in any
  saturation run is bounded by `|class_universe| × |role_universe| ×
  |class_universe|`. With Tseitin synthetics included, the class
  universe is bounded by Phase 2a's atom-set argument
  (`merged_atom_sets` grows monotonically inside a flat
  power-of-atomic-vocabulary lattice; bounded by `2^|atomic_vocab|`
  but in practice much smaller — GALEN: 1811, notgalen: 2634).
- `seen_facts.insert` guards every push. Phase 2d emits each
  `(sub, role, target)` triple at most once.
- Termination of the overall worklist is preserved because `push_fact`
  drains via the same `todo_fact` queue; the queue is bounded by the
  number of distinct facts, which is finite by the above.

### Recursive propagation chains

Point B's propagation re-enters `push_fact`, which re-enters the
"propagate to subs of `inherited.sub`" loop. Since `subs_of(c) ⊆
subs_of(d)` whenever `c ⊑ d`, the recursive paths visit a subset of
already-iterated subclasses; dedup short-circuits every redundant
emission. The recursion depth is bounded by the subsumption depth of
the class hierarchy (≤ `num_total_classes`), and each level pushes at
most one new fact per dedup-fresh `(sub, role, target)`.

Cycles in the subsumption closure (e.g. `A ⊑ B ⊑ A`) collapse to a
single equivalence class — both `subs_of(A)` and `subs_of(B)` include
each other plus all their shared subs; the dedup catches every loop
because the triple `(equiv_member, role, target)` is offered exactly
once across all entries in the class.

## Memory estimate

| Corpus | Baseline `facts.len()` | Naive projection (× avg supers) | Notes |
|---|---|---|---|
| ORE-10908-sroiq | 31 | ~250 (10×) | Small SROIQ fixture. |
| alehif-test | 120 | ~1K (10×) | Small ALEHIF fixture. |
| GALEN | 2,625 | ~26K (10×) | 2748 user classes / 27,980 closure pairs ⇒ avg ~10 supers per class. |
| notgalen | 3,377 | ~34K (10×) | 3087 user classes; ratio similar to GALEN. |

The "naive projection" treats every fact as inherited by every
subclass of its definition class. With Strategy A's dedup, the true
ratio is generally smaller: many subclasses share inheritance paths,
and many `(role, target)` pairs collapse across classes. **Pessimistic
ceiling** (no dedup savings beyond the trivial triple-identity check):
~50× baseline, ≈ 170K facts on notgalen ≈ 4 MB at 24 bytes per fact.
Absolutely small; no memory concern.

The bigger amplifier is the **Phase 2a witness-merge** cascade:
inherited facts on `(C, R_k, _)` may grow `merged_atom_sets[(C, R_f)]`
for the first time, triggering new synthetic emissions which feed back
through `process_fact`. The GALEN `merged_atom_sets.len()` of 1811
(itself bounded) will grow but stays bounded by the atom-set argument.

**No Strategy B fallback required.**

## Soundness

Standard subsumer-driven existential propagation. For every inherited
fact `(C, role, target)`:

- Hypothesis: `C ⊑ D` is in the closure (we're only emitting at
  process_subsumer / push_fact when `C` has `D` as a subsumer or `D`
  has `C` as a subclass).
- Hypothesis: `(D, role, target)` is a sound existential fact
  (saturator invariant: every fact represents an axiomatized
  `D ⊑ ∃role.target` or a soundly-derived equivalent).
- For any model M of the ontology and any `c ∈ M(C)`: since `C ⊑ D`,
  we have `c ∈ M(D)`. By the existential semantics of `D ⊑ ∃role.target`,
  there exists `y ∈ M(target)` with `(c, y) ∈ M(role)`. So
  `(C, role, target)` holds in M.

This is the same model-theoretic content the saturator already
exploits *implicitly* in `process_subsumer` lines 521-557 (it fires
downstream triggers as if the inherited fact were present). Phase 2d
just makes the fact *explicit* on `facts_by_sub[C]` so that rules
operating on `facts_by_sub` directly — Phase 2a witness-merge, Phase
2c-redux sub-role propagation, chain rule joins — can see it.

The **witness identity** (which individual `y` is named) is *not*
preserved across inheritance: `D`-instances and `C`-instances may use
the same witness or distinct witnesses. The saturator does not track
witness identity in any rule, including Phase 2c-redux's
witness-coincidence argument (which relies on functionality of `R_f`
to force coincidence regardless of which individual is named). The
inherited fact carries the *existence* of a witness, which is all the
saturator's rules consume.

### Why no rule that currently consumes `facts_by_sub` becomes unsound

| Consumer | Existing reading | Phase 2d impact |
|---|---|---|
| `process_subsumer` lines 521-557 (sub-side trigger firing) | Walks `facts_by_sub[d]` to enqueue `c ⊑ trigger.head` | After 2d, also walks materialized inherited facts on subclasses — equivalent to what it already does implicitly when iterating supers-side; no new entailment. |
| `process_subsumer` lines 569-590 (chain rule tail-fact lookup) | Iterates `facts_by_sub[d]` | Inherited facts represent the same witnesses; chain joins on them produce the same `(sub, sup, target)` derived facts that the existing supers-side fact would. |
| `process_fact` lines 673 (chain rule tail-side join) | Iterates `facts_by_sub[sub]` for sub ∈ target_subsumers | Same argument. |
| Phase 2a witness-merge `process_fact` lines 707-760 | Reads `fact` directly, NOT `facts_by_sub` | No direct impact, BUT inherited facts now enter `todo_fact` and re-trigger this rule on the inherited subclass — sound by the same Phase 2a soundness argument (the inherited fact represents a real witness; the merge is justified by functionality of R_f). |
| Phase 2c-redux (Pass 2) `process_fact` inner loop | Iterates `facts_by_sub[fact.sub]` | The intended consumer; sees the populated set. Soundness re-argued in `docs/phase2c-fix-target.md` §"Rule design" (witness-coincidence under functionality); unchanged by Phase 2d. |

The cross-rule audit shows no rule treats `facts_by_sub[X]` as a
witness-identity-tracking structure. All consumers read it as
"existential commitments of X" — exactly what inheritance produces.

## Code-change surface

### `crates/owl-dl-saturation/src/lib.rs`

**Three additions, est. ~50 LoC plus the structural canary:**

1. **`push_fact` augmentation (~12 LoC).** After the new fact is
   inserted (line 446), iterate `self.subs_of_class(fact.sub)` and
   re-enter `push_fact` for each `(c, fact.role, fact.target)`.
   The recursive call's dedup short-circuits previously-seen triples.

   ```rust
   // Phase 2d: propagate the new fact to every subclass of fact.sub.
   // Inherited fact (c, role, target) represents the same model-
   // theoretic witness (c ⊑ fact.sub ⇒ every c-instance is a
   // fact.sub-instance with the same role-witness). The recursive
   // push_fact call's `seen_facts.insert` guard catches every
   // already-known triple; recursion depth is bounded by the
   // subsumption-depth of the class hierarchy.
   let subs = self.subs_of_class(fact.sub);
   for c in subs {
       if c == fact.sub { continue; }
       self.push_fact(ExistentialFact { sub: c, role: fact.role, target: fact.target });
   }
   ```

2. **`process_subsumer` augmentation (~10 LoC).** Inside or after the
   existing sub-side trigger block at lines 525-557, snapshot
   `self.facts_by_sub[d.index()]` (already snapshotted in the existing
   block — fuse to avoid a redundant clone) and push each fact as
   `(c, role, target)`:

   ```rust
   // Phase 2d: materialize d's existential facts on c. Sound by the
   // standard subsumer-driven existential propagation (c ⊑ d ⇒
   // d's witnesses are c's witnesses). The existing sub-side
   // trigger-firing above (lines 525-557) already exploits this
   // semantically; Phase 2d makes the fact explicit so fact-time
   // rules (Phase 2a, Phase 2c-redux, chain rule) can see it.
   for fidx in fact_idxs_snapshot {
       let fact = self.facts[fidx];
       self.push_fact(ExistentialFact { sub: c, role: fact.role, target: fact.target });
   }
   ```

3. **Counter field on `WorklistEngine` (~3 LoC).** Add
   `phase2d_facts_inherited: u64`, initialized to 0 in the constructor
   (lib.rs:197-217), bumped at each successful `push_fact` for an
   inherited triple (i.e. in the helper that wraps the inheritance
   push, NOT in plain `push_fact` itself — distinguish original from
   inherited so the counter is a clean diagnostic).

4. **Structural canary (~30 LoC) in `mod tests`.** Minimal ontology:
   `Declaration(Class(:A) Class(:B) Class(:T) ObjectProperty(:R))`,
   `SubClassOf(:A :B)`, `SubClassOf(:B ObjectSomeValuesFrom(:R :T))`.
   After `saturate`, assert: `A ⊑ B` is in closure AND
   `phase2d_facts_inherited > 0` AND a fact `(A, R, T)` (or its
   internal id form) exists in `facts_by_sub[A]`. Use the existing
   test scaffolding pattern (parse_internal + WorklistEngine
   construction + run).

**Total estimated LoC for T3 implementation:** ~55 (40 rule lines + 15
test setup), measured by the surface above. Phase 2c's reverted
b83fcd6 added ~60 LoC for a comparable surface, so this estimate is
within the right order of magnitude.

### Optional: avoid the recursive push_fact's subs-iteration overhead

If T4's wall measurement shows excess overhead from the recursive
re-iteration (`push_fact` for inherited fact `(c, R, T)` re-walks
`subs_of(c)` ⊆ already-walked `subs_of(D)`), split into two
functions:

```rust
fn push_fact(&mut self, fact) -> Option<usize> {
    let idx = self.push_fact_no_propagate(fact)?;
    let subs = self.subs_of_class(fact.sub);
    for c in subs {
        if c == fact.sub { continue; }
        self.push_fact_no_propagate(ExistentialFact { sub: c, role: fact.role, target: fact.target });
    }
    Some(idx)
}
```

This walks `subs_of(D)` once per genuine new fact and never re-walks
on inherited propagations. Trade-off: explicit two-function API
slightly increases surface; cleaner termination story. **Recommend
defaulting to the simpler recursive variant in T3; switch to the
split variant only if T4's wall regression exceeds the 10% cap.**

## What this design does NOT do

- Does **not** re-introduce the Phase 2c sub-role witness propagation
  rule (that's Phase 2c-redux in Pass 2 of this plan).
- Does **not** change EL+ rule semantics — only adds fact-copy on
  subsumer edges and fact insertions.
- Does **not** introduce per-fact dependency tracking (the saturator
  has no per-fact deps today; inherited facts share the same
  zero-dep contract as original facts).
- Does **not** change `Subsumers`, `subsumed_by`, or any consumer of
  the closure; only the per-class fact indices grow.

## Cross-references

- Plan: `docs/superpowers/plans/2026-06-01-phase2d-plus-2c-redux.md` (T1).
- Prior diagnosis: `docs/phase2c-fix-target.md` §"Predicted walkthrough
  on pair_06 (and what actually happened)" — the empirical
  reckoning that motivated Phase 2d.
- Dead-end ledger: `docs/hypertableau-dead-ends.md` §15 — the
  architectural prerequisite this design addresses.
- Reverted Phase 2c rule (for Pass 2 reference): commit `b83fcd6`,
  reverted at `cc2019e`. The Phase 2c rule code at b83fcd6 lines
  +794..+811 plugs into `process_fact` after Phase 2a's emission;
  uses the same `seen_facts.insert(...)` dedup that Phase 2d reuses.

## Measurement gates (Pass 1 of the plan, T4)

Phase 2d alone ships only if T4 confirms:
- Phase 0 net FP=0 / MISSED=0 (soundness).
- GALEN FP=0 (soundness).
- GALEN MISSED unchanged at 17 (expected — Phase 2c-redux is the
  recovery layer; Phase 2d alone shouldn't reduce MISSED).
- GALEN wall regression < 10% on the 12.33-min baseline.
- `facts.len()` growth < 5× (informational; design ceiling is ~50×
  pessimistic but realistic estimate is 5-10×).

If any gate fails, REVERT Phase 2d and abandon the combined plan;
Phase 2c-redux cannot proceed without Phase 2d's foundation.
