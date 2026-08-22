# `probe_says_inconsistent` DKey aliasing — hotfix report

**Branch:** `fix/classify-consistency-probe-aliasing` (from `origin/main` `b796bec`)

## Headline: the reported bug does not reproduce, and the proposed fix creates it

I could not reproduce a false positive from `classify.rs`'s
`unsatisfiable_idxs.contains(&(c.index() as usize))`. I built the fixture the brief
asked for — DKey interned below a reported class, an ABox `ClassAssertion`, an
unsatisfiable class, probe reached — and stock `main` answers **correctly**.

I then applied the proposed minimal fix (thread `index` in, look the class up by
report position) to that same fixture. It reports the KB **inconsistent**, with all
four classes unsatisfiable, while `is_consistent` says consistent. **The proposed fix
is the false positive.**

So I did not apply it. What landed is doc-only plus a regression canary.

## Why the premise is inverted

The brief says `unsatisfiable_idxs` "holds REPORT POSITIONS". Its *consumers* read it
that way. Its **producers do not** — all three insert `i` after deciding the
satisfiability of `ClassId::new(i)`:

| site | producer | what `i` indexes |
|---|---|---|
| `classify.rs:1323` | `let class_id = ClassId::new(i); … decide_classify(pool.atomic(class_id))` | raw `ClassId` |
| `classify.rs:1511` | `closure.unsatisfiable_bitset()` — a `ClassId`-indexed bitset, iterated `0..n` | raw `ClassId` |
| `classify.rs:3656` | `let class_id = ClassId::new(i); …` (same shape as 1323) | raw `ClassId` |

There are no other mutations — `grep 'unsatisfiable_idxs\.\(insert\|extend\|remove\|retain\)\|unsatisfiable_idxs ='`
returns exactly those three inserts, no removals, no reassignment.

So the set is really `{ i ∈ 0..n : ClassId(i) is unsatisfiable }` — raw ids, clipped
to `< n`. Against *that*, the existing probe is exact:

```
c.index() ∈ unsatisfiable_idxs
  ⟺ c.index() < n ∧ ClassId(c.index()) unsat
  ⟺ c itself is unsat   (∧ c.index() < n)
```

Sound. The `< n` clip can only cause a **miss**, never a false positive.

The IRI lookup compares a *report position* against a set of *raw ids*. Those are
different spaces exactly when a DKey sits below a user class — which is the very
condition the brief identifies. It converts a sound check into an unsound one.

The real defect is upstream: the producers write raw ids into a field the consumers
read as positions. That is `fix/dkey-id-aliasing-on-main`'s `ReportedClasses` job, and
it must fix **both ends together**. Changing only the probe end is strictly worse than
changing neither.

## The reproducer

`crates/owl-dl-reasoner/tests/classify_dkey_alias_consistency.rs` (new, 2 tests).

Fixture (`:M`, `:N` used-but-undeclared so they intern after the DKey):

```
Declaration(Class(:A))
Declaration(Class(:B))
Declaration(DataProperty(:p))
Declaration(ObjectProperty(:r))
Declaration(NamedIndividual(:x))
SubClassOf(DataSomeValuesFrom(:p xsd:integer[0,5]) :A)   ; first axiom to sort → DKey at id 2
SubClassOf(:M :B)                                         ; :M id 3
SubClassOf(:N :B)                                         ; :N id 4
SubClassOf(:M ObjectComplementOf(:B))                     ; :M unsatisfiable, no instances
SubClassOf(:A ObjectMaxCardinality(1 :r))                 ; ─┬─ push out of EL so classify
SubClassOf(:B ObjectUnionOf(:A :N))                       ; ─┘  reaches probe_says_inconsistent
ClassAssertion(:N :x)                                     ; satisfiable class, ABox instance
```

Verified interning order and the collision:

```
id 0: http://ex.org/A      report position 0
id 1: http://ex.org/B      report position 1
id 2: urn:rustdl-dkey:0:5  (filtered from the reported list)
id 3: http://ex.org/M      report position 2   ← genuinely unsatisfiable
id 4: http://ex.org/N      report position 3   ← satisfiable, asserted; position == :M's raw id
```

Instrumenting the probe confirms it is entered with the firing shape:
`PROBE ENTER unsat={3} n=4` — non-empty set, ABox assertion present, probe reached.

### Stock `main` — CORRECT
```
unsatisfiable_classes() = ["http://ex.org/N"]
inconsistent = false
is_consistent = Ok(true)
```
Both tests pass.

### With the proposed fix applied — FALSE POSITIVE
```
PROBE ENTER unsat={3} n=4
unsatisfiable_classes() = ["A", "B", "M", "N"]   (all four)
inconsistent = true
is_consistent = Ok(true)                          ← sibling surface disagrees
```
```
test dkey_aliased_abox_assertion_is_not_an_inconsistency ... FAILED
  FALSE POSITIVE: classify declared a consistent KB inconsistent.
```

### The mutation that kills the canary
Precisely the change the brief requested — in `probe_says_inconsistent`, replace
`&& unsatisfiable_idxs.contains(&(c.index() as usize))` with
`&& let Some(&pos) = index.get(internal.vocabulary.class_iri(*c)) && unsatisfiable_idxs.contains(&pos)`,
threading `index: &HashMap<String, usize>` in from both call sites. **Constructed,
compiled and run** — output above. The RED→GREEN transition is inverted relative to
the brief: GREEN on `main`, RED under the proposed change.

Non-vacuity is also guarded inside the tests:
- `fixture_actually_aliases` pins the full interning order, asserts no DKey reaches
  the reported list, and asserts `report_position(:N) == 3 == raw_id(:M)`. If
  component sorting or the data lowering drifts, the collision dissolves and this
  fails loudly instead of the canary passing for free.
- `dkey_aliased_abox_assertion_is_not_an_inconsistency` asserts
  `!unsatisfiable_classes().is_empty()` before the payload, because an empty set makes
  the probe return at its first gate and the asserted-instance rule is never reached.

The canary is **fix-agnostic**: it asserts only the observable (KB consistent, not all
classes unsat, no vacuous entailment), cross-checked against `is_consistent`. A
correct two-ended `ReportedClasses` fix keeps it green; only a one-ended probe "fix"
breaks it. It deliberately does *not* pin `unsatisfiable_classes()`, which is
separately wrong here.

## What actually changed

`crates/owl-dl-reasoner/src/classify.rs` — **documentation only, no behaviour change**:

1. **The stale doc, corrected (in scope).** Line 1564 said
   `RUSTDL_CLASSIFY_CONSISTENCY_PROBE`, "default OFF". `lib.rs:2469` is
   `is_none_or(|v| v != "0")` — default **ON**, and `lib.rs:2463` says so. Now reads
   "**default ON** — `=0` reverts", with the reason and a dated note that it used to
   be wrong.
2. **An orphaned doc block, re-attached.** `classify_inconsistent`'s doc comment had
   drifted onto `probe_says_inconsistent` (stacked above the probe's own doc at
   1558-1562), leaving `classify_inconsistent` at 1691 undocumented. Moved to its
   owner. Adjacent to the in-scope edit and part of why this area misleads readers.
3. **A load-bearing comment at the probe check**, recording the producer/consumer
   index-space split, why `c.index()` is correct against it, that the IRI lookup
   introduces the maximal false positive, and that the real fix must change both ends.
   Names the canary file.

`crates/owl-dl-reasoner/tests/classify_dkey_alias_consistency.rs` — new.

No production logic was touched.

## Both call sites confirmed

`classify.rs:1405` and `classify.rs:4266` are the only two calls
(`grep -n 'probe_says_inconsistent'` → 1405, 1584 def, 4266). Both reach a
`unsatisfiable_idxs` built by a raw-`ClassId` producer (1323 and 3656 respectively),
so both are consistent with the `c.index()` probe and both are sound today. They would
break **identically** under the proposed fix — it is one defect, not two treatments.

The third producer (1511, `classify_pure_el`) never calls the probe; it assembles its
`Classification` directly at 1552.

## Pre-existing tests whose verdict changed

**None.** The diff is doc-only. Full crate suite: see "Verification" below.

## Concerns

1. **The real bug is still live, and it is a reporting false positive.** On this same
   fixture, stock `main` reports `unsatisfiable_classes() == ["http://ex.org/N"]`.
   `:N` is **satisfiable**; the unsatisfiable class is `:M`. classify names the wrong
   class as unsatisfiable whenever a DKey is interned below a reported class. Worse,
   because the unsat probe loops `0..n` over raw `ClassId`s while `n` counts *reported*
   classes, the highest-id user classes are **never probed at all** — with one DKey
   present, `SubClassOf(:Unsat :B) SubClassOf(:Unsat ObjectComplementOf(:B))` yields
   `unsatisfiable_classes() == []`. Every `Classification` consumer indexing
   `unsatisfiable_idxs` into `classes` (`is_subclass` at :543, `equivalent_classes`
   :566, `direct_subsumers` :615/:628, :681, :714, :735) inherits this. That is the
   `ReportedClasses` work, and it is bigger than a hotfix.
2. **This branch has no fix to land.** If the intent was to ship something ahead of
   `fix/dkey-id-aliasing-on-main`, there is nothing here to ship but the doc and the
   canary. That may be worth landing anyway — the canary blocks a change that is
   currently sitting in a brief as "the minimal fix".
3. **~~`fix/dkey-id-aliasing-on-main` (`14db978`) needs re-checking against this.~~
   CHECKED (2026-08-21) — it is clean.** The concern was that if its `ReportedClasses`
   conversion changed `probe_says_inconsistent` to a report position without
   simultaneously converting all three producers, it would ship this false positive. It
   converts both. At `9caf7d8` (parent of `14db978`) the producers already read
   `reported.class_id(i)` over `0..n` at `:1404`, `:1647`, `:3701` — converted by
   `c8ca8b9`/`94e49c0` — and `14db978` then moves the consumer to
   `reported.report_pos(*c)` at `:1754`. Same index space on both sides, so that branch
   carries no regression from this site. Cherry-picking the canary onto it is still
   worthwhile as a permanent guard, but is no longer needed to answer the question.
4. **Fixture fragility.** The reproducer depends on horned-owl's derived component
   `Ord` (empirically the `SubClassOf` superclass expression dominates the ordering,
   with `Class` sorting before `DataSomeValuesFrom`). A horned-owl bump could reshuffle
   it. `fixture_actually_aliases` is there to make that a loud failure, not a silent
   one — but it means the canary needs a real fix to the fixture if it ever fires,
   not a relaxed assertion.
