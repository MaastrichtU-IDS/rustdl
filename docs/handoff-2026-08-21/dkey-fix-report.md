# DKey id aliasing — fix report

**Branch:** `fix/dkey-id-aliasing` · **Commit:** `132dd21` (single commit)
**Status:** fixed, tests RED→GREEN, no existing expectation changed.

---

## 1. Did the bug reproduce? Yes — but NOT on `bench-corpus/mie.ofn`

The **mechanism** in `finding.md` is exactly right and I reproduced it with real
false positives. The **reproduction instructions were not**: on this branch
`bench-corpus/mie.ofn` does not exhibit the bug at all.

Measured on `c1f44d8`:

```
mie.ofn: num_classes = 101, reported classes = 84
DKey ids = [84, 85, 86, 87, 88, 89, 90, ... 100]   <- ALL 17 above ALL 84 user classes
classify() vs is_subclass_of() on all 241 reported positives: 0 disagreements
```

The finding says "a DKey lands at id 73 with 83 named classes". The tracked
`mie.ofn` has **84** named classes and its lowest DKey id is **84**. Either the
finding was produced against a different `mie.ofn` (a `/ontologies/` copy?) or
against the incremental `convert_ontology_seeded` path, which does not exist on
this branch (`grep convert_ontology_seeded` -> no hits anywhere in `crates/`).
`sulo.ofn` is not in `bench-corpus/` either.

### The actual trigger condition (this is the load-bearing finding)

`convert_ontology` does `components.sort()` before lowering, and horned-owl's
`Component` derives `Ord` with all `DeclareClass` variants ordering **before**
any axiom variant. So **every `Declaration`-ed class is interned before any
axiom can mint a DKey**. `mie.ofn` declares all 84 of its classes -> every DKey
lands on top -> report position == `ClassId` -> aliasing invisible. That is *the*
reason the curated corpus never caught this, and it is a sharper statement than
the finding's "ontologies whose data axioms are lowered last".

To reproduce you need a class that is **used but not declared**, first mentioned
in an axiom that sorts *after* a DKey-minting one. (`SubClassOf` derives `Ord`
over `(sup, sub)` — `sup` first, because horned-owl declares the struct in that
field order — so a `DataSomeValuesFrom` in `sup` position sorts *late*; putting
the DKey-bearing expression in `sub` position under an early-sorting named `sup`
makes it sort *first*.)

Minimal fixture, `DKey` at id **0**:

```
SubClassOf(ObjectSomeValuesFrom(:op DataSomeValuesFrom(:dp xsd:integer)) :Aaa)
SubClassOf(:Zzz :Yy)
SubClassOf(:Uu :Zzz)
```
```
ids: ["urn:rustdl-dkey:*:*", "http://t/Aaa", "http://t/Zzz", "http://t/Yy", "http://t/Uu"]
  MISMATCH Zzz <= Yy : classify=false direct=true  <-- MISSED
  MISMATCH Yy  <= Uu : classify=true  direct=false <-- FALSE POSITIVE
  MISMATCH Uu  <= Zzz: classify=false direct=true  <-- MISSED
  MISMATCH Uu  <= Yy : classify=false direct=true  <-- MISSED
```

### RED output (tests run against pre-fix `classify.rs`, fix reverted)

```
running 10 tests
test result: FAILED. 2 passed; 8 failed; 0 ignored

---- classify_agrees_with_direct_query_on_el_fixture ----
[classify/EL] classify() reported subsumptions that is_subclass_of() denies — FALSE POSITIVES:
[ ("http://t/Yy", "http://t/Uu") ]

---- classify_agrees_with_direct_query_on_hybrid_fixture ----
[classify/hybrid] ... FALSE POSITIVES:
[ ("http://t/Ww","http://t/Uu"), ("http://t/Zzz","http://t/Uu"),
  ("http://t/Zzz","http://t/Ppp"), ("http://t/Ppp","http://t/Uu") ]

---- classify_n2_agrees_with_direct_query ----          [n2/EL] FALSE POSITIVES: [(Yy, Uu)]
---- classify_top_down_agrees_with_direct_query ----    [td/EL] FALSE POSITIVES: [(Yy, Uu)]
---- classify_saturation_only_has_no_false_positives ---- (failed)

---- inert_declarations_do_not_change_the_hierarchy_hybrid ----
[hybrid] adding Declaration(Class(..)) axioms that entail NOTHING changed the reported
hierarchy — impossible for a correct classifier:
  Ppp <= Uu : without declarations = true,  with = false
  Uu  <= Ww : without declarations = false, with = true
  Uu  <= Zzz: without declarations = false, with = true
  Ww  <= Uu : without declarations = true,  with = false
  Yy  <= Ww : without declarations = false, with = true
  Zzz <= Ppp: without declarations = true,  with = false
  Zzz <= Uu : without declarations = true,  with = false
  Zzz <= Ww : without declarations = false, with = true

---- inert_declarations_do_not_change_the_hierarchy_el ---- (failed)
---- report_positions_are_never_cast_to_class_ids ---- (failed; listed all 29 raw sites)
```

The 2 that pass pre-fix are correct to pass: the non-vacuity guard (it tests the
fixture, not the classifier) and the `mie.ofn` corpus oracle (mie does not
exercise the hazard — see above).

GREEN after the fix: `10 passed; 0 failed` in 0.13 s.

---

## 2. Which fix, and why

**Option (a) — thread the real id through.** Implemented as a `ReportedClasses`
type that owns the bijection in **both** directions:

```rust
struct ReportedClasses {
    iris: Vec<String>,                 // report position -> IRI
    ids: Vec<owl_dl_core::ClassId>,    // report position -> ClassId
    pos_of_id: Vec<Option<u32>>,       // ClassId index   -> report position (None for DKey)
}
fn class_id(&self, report_pos: usize) -> ClassId
fn report_pos(&self, id: ClassId) -> Option<usize>
fn beyond_vocabulary(&self, id: ClassId) -> bool   // id-space vs id-space only
```

Option (b) was rejected on the numbers: keeping report index == `ClassId` means
the `n x n` entailment matrix and every report-space vector grow to
`num_classes()`. DKeys are minted **per distinct literal/facet range**, so an
ABox-heavy ontology with 100 classes and 10 000 data values would allocate a
10 100^2 matrix instead of 100^2. On top of the unmeasured pair-loop cost and the
`ClassificationStats` churn the brief already flags, that is not acceptable.

I also considered and rejected a third option — interning every named class in a
pre-pass so the "DKeys on top" invariant holds by construction. It would make the
filter-then-index *valid*, but it changes `ClassId` assignment for **every**
ontology, hence saturation/tableau ordering, hence verdicts and timings
corpus-wide. Far larger blast radius than the projection bug it fixes.

### Note on the bug's real shape: it is BIDIRECTIONAL

The write-up describes report-index -> `ClassId`. There is an equally live reverse
direction that `sites.md` did not list: `id.index() as usize` followed by an
`if i < n` guard, at `classify.rs:1881, 1906, 2137, 2142, 2167, 2239, 2335`.
Those `< n` guards *are* the aliasing assumption ("ids below `n` are the reported
ones") and they were wrong in exactly the same cases. `report_pos` replaces all of
them. A fix that only did the forward direction would have left false positives in
place (the closure-seed at 2239 feeds the entailment matrix directly).

### Is recurrence a compile error?

**Honest answer: no — it is a test failure, not a compile error.** I owe you the
reasoning, since the brief asked for the compile error:

- A `ReportIdx(u32)` newtype threaded through every report-space collection
  (`unsatisfiable_idxs`, `direct_supers`/`direct_children`, `order`/`tiers`,
  `already_known`, `candidates`, `visited_gen`, `subsumer_counts`, `label_cache`,
  `EntailmentMatrix`, `HashMap<String, usize>`, the rayon result tuples, four
  helper signatures, and `Classification`'s public fields) *would* be a compile
  error — `ClassId::new(ridx)` would not typecheck. It is also a 400-500 line
  mechanical rename through the single most soundness-critical file in the tree,
  in a session whose only verification is the crate suite. I judged the risk of
  that rename higher than the risk it removes. **If you want it, it is a clean
  follow-up and the `ReportedClasses` boundary is the right seam to do it from.**
  A partial newtype (boundary only, collections left as `usize`) buys friction
  without the guarantee, so I did not do that either.
- A `const _: () = assert!(!contains(include_str!("classify.rs"), ...))` *is* a
  compile error and I considered it, but a ~180 KB const-eval substring scan risks
  tripping the long-running-const-eval limit. Rejected as too cute for a soundness
  fix.

What ships instead: the struct + impl is fenced by literal sentinel comments,

```rust
// --- BEGIN report-position <-> ClassId conversion boundary ---
... struct ReportedClasses / impl ...
// --- END   report-position <-> ClassId conversion boundary ---
```

and `report_positions_are_never_cast_to_class_ids` scans `classify.rs`'s own
source (via `include_str!`, `#[cfg(test)] mod tests` truncated off) and fails if
`ClassId::new(` or `.index() as usize` appears anywhere outside the fence. It runs
on every `cargo test -p owl-dl-reasoner`, names the offending file:line, and its
failure message points at the two accessors. **Mutation evidence: it fails on the
pre-fix file and lists all 29 original sites** (output above). After the fix there
are exactly three sanctioned occurrences, all inside the fence (`collect`,
`report_pos`, `beyond_vocabulary`).

---

## 3. Any existing test whose expectation would have to change?

**None.** Nothing was edited and nothing needed editing.

- `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --release` -> 76 suites,
  all `ok`, 0 failed.
- `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner` (debug) -> 76 suites,
  all `ok`, 0 failed. The pizza `hyper.rs:3677` `debug_assert!` did not fire.
- `cargo clippy -p owl-dl-reasoner --lib --tests -- -D warnings` -> clean.
- `cargo fmt --all` applied.

That is expected and is itself a datum: **every curated fixture has all DKeys
above all user classes**, so on the whole curated corpus the new code is
verdict-identical to the old by construction. The change is a no-op on the corpus
and a fix off it. Full FP=0/MISSED=0 revalidation remains your gate; I did not
attempt an ORE sweep.

Two byte-level behaviour deltas worth your eye, both no-ops on all-DKeys-on-top
inputs:

1. `classify_pure_el`'s subsumer walk used to `break` at the first id `>= n`
   (which, DKeys-on-top, *is* the first DKey). It now `break`s at
   `beyond_vocabulary` (first Tseitin) and `continue`s past DKeys individually.
   Same rows, same `saturation_subsumption_hits`, a handful more loop iterations.
2. `defined_sups` / the defined-SUB sweep now drop an axiom whose named operand or
   a union disjunct resolves to a DKey (previously it got a huge index and was
   dropped by `>= n`). Same outcome; the sweep only ever *adds* oracle-confirmed
   edges, so a drop is a MISS at worst.

---

## 4. The inert-declaration regression test

`inert_declarations_do_not_change_the_hierarchy_{el,hybrid}`.

`Declaration(Class(:C))` for a `C` the ontology already mentions entails nothing —
OWL 2 treats a used class as declared, so the two ontologies have identical models
and a correct classifier must report an identical hierarchy. The test classifies
`O` and `Declarations + O` and diffs `is_subclass` over every pair. It also asserts
the reported class *set* is unchanged, so a projection that silently dropped a
class would be caught too.

Why it is the right generalisation: adding the declarations changes nothing
semantically but *does* change `ClassId` assignment — the declared classes are
interned first and every DKey moves above them, i.e. into the configuration where
the aliasing is invisible. So under the bug the declared variant is correct and the
bare one is broken, and the two differ. The property holds for any future
id-assignment-sensitive defect, not just this one.

**Mutation evidence:** fails on the pre-fix file with 8 concrete flips on the
hybrid fixture, listed above — including two directions changing in *opposite*
ways, which no legitimate monotone effect could produce.

**Non-vacuity guard:** `fixtures_really_put_a_dkey_below_a_user_class` asserts each
fixture actually mints a DKey *and* that its last DKey id is below its last
user-class id. If a future `convert_ontology` change pushes DKeys back to the top,
this guard fails loudly instead of leaving the oracles quietly green. This is the
one test I would look at first if these ever go green for a bad reason.

**Coverage across entry points:** the same hazardous fixtures are run through
`classify`, `classify_n2`, `classify_saturation_only` and
`classify_top_down_with_timeout`, in EL and out-of-fragment variants, so both
`classify_pure_el` and the tier-walk/sweep path are exercised. The saturation-only
path asserts the FP direction only (documented sound under-approximation).

---

## 5. Siblings (Step 4)

**Fixed as part of this change (same shape, found by review not by the brief):**

- `subsumes_via_tableau` pushed `(sub.index(), sup.index())` — raw `ClassId`
  indices — into `ClassificationStats::timed_out_pair_ids`, which
  `Classification::undecided_pairs()` reads back as **report positions** to index
  `self.classes[i]`. Once the two spaces differ this mislabels the undecided pair
  or panics out of bounds. Now goes through `push_undecided_pair`, which converts
  via `report_pos` and `expect`s (rather than skips) so the asserted anytime
  invariant `undecided_pairs().len() == timed_out_pairs` stays exact. Had I fixed
  only the main bug, this would have become a live panic.

**Checked, not siblings:**

- `DKEY_IRI_PREFIX` is the only filtered class prefix in the reasoner
  (`grep DKEY_IRI_PREFIX|rustdl-dkey` across `crates/`), and `classify.rs` was the
  only file with the filter-then-index shape.
- `owl-dl-bench/src/main.rs:448,533` also excludes `DKEY_IRI_PREFIX` and
  `urn:rustdl-` — but it filters **inside** the `for i in 0..n` loop over
  `ClassId`s and keys its output by IRI string, so no index is ever remapped. This
  is the correct pattern and a useful contrast.
- `owl-dl-saturation/src/proof.rs:783,813` compares `index() >= num_classes()` to
  detect Tseitin synthetics. Id-space vs id-space, no report projection.
- `class_expr_query.rs:223` (`retain` on `urn:rustdl-ce-probe`) and
  `lib.rs:304,410` (`retain` on `ANON_IRI_PREFIX`) filter `Vec<String>` / tuple
  vectors that are never index-mapped afterwards.
- `ClassificationStats::pairs_per_sub` is `ClassId`-keyed **by documentation**, and
  its only consumer (`owl-dl-cli/src/main.rs:717`) reads `.values()` only — keys
  are never resolved to class names. Left alone deliberately.
- `convert_ontology_seeded` (the incremental path the finding says makes this
  worse) does not exist on this branch — no hits in `crates/`. Whoever lands it
  should re-run `tests/dkey_id_aliasing.rs` against the seeded ids, because seeded
  interning breaks the "declarations first" ordering that keeps the curated corpus
  safe.

**Latent, unproven, worth a note (NOT fixed):**

- `realize.rs:693` filters entailed types with
  `.filter(|c| (c.index() as usize) < num_user_classes)` under a comment claiming
  "restricted to declared user classes". That predicate excludes Tseitin/nominal
  synthetics but **does not exclude DKeys**, so a DKey subsumer of an individual's
  nominal class would be reported as an entailed type with a `urn:rustdl-dkey:`
  IRI. I probed two fixtures (`DataPropertyAssertion` + `SubClassOf(:A E dp.int)`,
  and a `ClassAssertion(ObjectIntersectionOf(:A, E dp.int))`) and got clean output
  — the DKey lives on the *value* node, reached through the role, so it is not a
  subsumer of the nominal. I could not construct a reproducer and believe it is
  currently unreachable, but the filter does not enforce what its comment claims.
  A one-line `&& !iri.starts_with(DKEY_IRI_PREFIX)` would make it defensive. I
  left it alone rather than touch `realize` in a soundness commit.

---

## 6. Files changed

- `crates/owl-dl-reasoner/src/classify.rs` — +275/-105. `ReportedClasses` +
  sentinels; 29 raw conversions replaced; `classify_pure_el`,
  `inject_backfold_derived_sups`, `find_direct_parents_top_down` and
  `subsumes_via_tableau` signatures now carry `&ReportedClasses`;
  `push_undecided_pair` added; `timed_out_pair_ids` doc now states its index
  space; 3 inline unit tests updated for the new `inject_backfold` signature
  (behaviour unchanged — those fixtures are declaration-only, so report position
  genuinely equals `ClassId` there, which the added comment says).
- `crates/owl-dl-reasoner/tests/dkey_id_aliasing.rs` — new, 10 tests.

Untouched, as instructed: `crates/owl-dl-py/examples/`, `docs/benchmarks/`.

## 7. Self-review findings and concerns

- **`class_id` panics on an out-of-range report position** where the old raw cast
  would have silently produced a wrong-but-in-range id. That is the right trade
  (loud over silent) but it is a new panic surface. All call sites index from
  `0..n` where `n == reported.len()`, or from `direct_supers.len()` which is sized
  `n`, so it is unreachable by construction.
- **`inject_backfold_derived_sups` now derives `n` from `direct_supers.len()`**
  instead of taking it. Same value in production and in all three unit tests; it
  removes a parameter that could disagree with the vectors it bounds.
- **Memory:** `ReportedClasses` adds two vectors of `num_classes()` entries
  (`Vec<ClassId>` + `Vec<Option<u32>>`) ~= 12 MB on the 981 k-class ORE giant, on
  top of the `Vec<String>` that already existed. `report_pos` is an O(1) Vec
  lookup — no asymptotic change anywhere.
- **The mechanical guard is whitespace/spelling-sensitive.** It matches the two
  raw spellings that exist today; someone determined could write
  `ClassId::new({let x = i as u32; x})` and slip past. It fails *closed* (a new
  raw cast fails the test), which is the direction that matters, but it is a
  guard, not a proof. The full `ReportIdx` newtype is the proof, and is a
  follow-up.
- **What I did not do:** no ORE sweep, no `--workspace` run, no
  `--all-targets --all-features`. `owl-dl-bench` and `owl-dl-cli` compile against
  unchanged public APIs (`ClassificationStats` field *types* are identical; only
  `timed_out_pair_ids`' documented index space is now correct rather than
  accidentally correct), but I did not build them.
- **The finding's `mie.ofn` claim should be corrected in the source document**
  before someone else tries to reproduce from it. The real trigger is "a
  used-but-undeclared class interned after a DKey", not "data axioms lowered
  last".

---
---

# Round 2 — fix-round-1 response

**Commit:** `954bc7e` · **Status:** the surviving FP is fixed; RED shown before, GREEN after.
Two of the review's own claims did not survive measurement — details in §R2.4.

## R2.1 CRITICAL — the surviving FP at `classify_pure_el` Pass 1: CONFIRMED and fixed

The reviewer is right, and it is worse than a wrong subsumption.

```rust
let unsat_bs = closure.unsatisfiable_bitset();
for i in 0..n {                                    // i is a REPORT POSITION
    if i < unsat_bs.len() && unsat_bs.contains(i)   // bit i is a CLASS ID
```

`Subsumers::unsatisfiable_bitset` (`owl-dl-saturation/src/lib.rs:2188-2192`):
"Bit `i` set iff `class_i ⊑ ⊥`". Verified it is the **only** caller in the tree.

The amplification is what makes this severe: `Classification::entails`
short-circuits on `unsatisfiable_idxs` to reintroduce `⊥ ⊑ *`. So one
mis-indexed bit does not produce one bad pair — it makes the mis-flagged class
subsume **every** class in the ontology, and simultaneously hides the genuinely
unsatisfiable class.

**Reproducer, run against `132dd21` (the round-1 tip):**

```
ids = ["urn:rustdl-dkey:*:*", :Aaa, :Ccc, :Ddd, :Eee]
SubClassOf(ObjectSomeValuesFrom(:op DataSomeValuesFrom(:dp xsd:integer)) :Aaa)
SubClassOf(:Aaa owl:Nothing)
SubClassOf(:Ccc :Ddd)   SubClassOf(:Ddd :Eee)

--- classify:                 unsat = ["http://t/Ccc"]    FP: Ccc <= Aaa
--- classify_n2:              unsat = ["http://t/Ccc"]    FP: Ccc <= Aaa
--- classify_saturation_only: unsat = ["http://t/Ccc"]    FP: Ccc <= Aaa
--- classify_top_down:        unsat = ["http://t/Ccc"]    FP: Ccc <= Aaa
realize entailed_types(:i1) = ["http://t/Ddd", "http://t/Eee"]     <- Ccc deleted
```

Ground truth is `unsat = ["Aaa"]`. Bit 1 (Aaa's *class id*) probed at report
position 1 (which is *Ccc*) — the shift is exactly the one DKey at id 0.

**Fix** (`classify.rs`, one line + a comment explaining the trap):

```rust
for i in 0..n {
    if closure.is_unsatisfiable(reported.class_id(i)) {
```

**RED, on `132dd21`, with the new tests in place:**

```
running 15 tests
test result: FAILED. 8 passed; 7 failed; 0 ignored; finished in 0.43s

failures:
    classify_agrees_with_direct_query_on_unsat_fixture      [classify/unsat] FP: (Ccc, Aaa)
    classify_n2_agrees_with_direct_query                    [n2/unsat]       FP: (Ccc, Aaa)
    classify_saturation_only_has_no_false_positives         [sat/unsat]      FP: (Ccc, Aaa)
    classify_top_down_agrees_with_direct_query              [td/unsat]       FP: (Ccc, Aaa)
    unsatisfiable_set_names_the_right_class    left: ["Ccc"]  right: ["Aaa"]
    inert_declarations_do_not_change_the_hierarchy_unsat
                        [unsat] inert declarations changed the UNSATISFIABLE set
                        left: ["Ccc"]  right: ["Aaa"]
    realize_types_survive_the_unsat_projection
                        realize() lost the entailed type http://t/Ccc for :i1
                        — got ["http://t/Ddd", "http://t/Eee"]
```

**GREEN, after the one-line fix:**

```
running 15 tests
test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.43s
```

## R2.2 Why the source guard missed it — and its doc now says so

The reviewer's diagnosis is exactly right and it is the most useful thing to
come out of this round. The conflation here is spelled with **no cast at all**:
a report position handed to something that is `ClassId`-indexed underneath. My
guard matches two textual spellings, so it was green on a file with a live FP
path — and my round-1 report's confident framing of it was worse than my own
"a guard, not a proof" caveat.

The guard's doc comment now carries a `# This guard is NOT a proof — know
exactly what it misses` section that quotes the bitset bug verbatim as the
worked example, enumerates the other invisible shapes (indexing any
`Vec`/bitset/slice the saturator or tableau sized by `ClassId`; passing a report
position to a `usize` parameter that means a class id; arithmetic that
reconstructs an id), and states plainly that the behavioural oracles — not the
lint — are the safety net, because only they caught this.

## R2.3 Tests: the unsat gap is closed

Added `UNSAT_BODY` as a third fixture rather than mutating the existing two
(mutating them would have diluted their assertions: an unsat class subsumes
everything, so many pairs become genuine positives). It is threaded through
**both** oracle families and all four entry points, plus two new dedicated
oracles:

- `unsatisfiable_set_names_the_right_class` — pins the exact unsat set on
  `classify` / `classify_n2` / `classify_saturation_only` /
  `classify_top_down_with_timeout`. This is the highest-value assertion added
  this round: it catches the bug at its source instead of downstream of the
  `⊥ ⊑ *` fan-out.
- `realize_types_survive_the_unsat_projection` — pins the downstream corruption
  (`realize.rs:669` builds its unsat filter from
  `classify_saturation_only_internal`), and also asserts no `urn:rustdl-dkey:`
  IRI leaks into the types, which turns the round-1 §5 "latent, unproven" note
  into a live guard at no cost.
- `assert_inert_declarations_are_inert` now also diffs the **unsatisfiable set**,
  which is how the reviewer's observation ("it *would* have caught this") is
  made true rather than merely plausible.

**I also found my own round-1 test wrong.** The non-vacuity guard used
`last_dkey < last_user`. That is the wrong predicate: a run of DKeys that
*straddles* the top of the user range is hazardous but reports `false`. Pizza is
exactly that shape (first DKey 87, last user class 95, last DKey 103) — my
predicate called it hazard-free, the reviewer's `first_dkey < last_user` calls it
hazardous, and the reviewer is right. Corrected, with the reasoning in the
helper's doc so it is not silently re-broken.

## R2.4 Two review claims that did not survive measurement

I checked both rather than restating them, which is the lesson round 1 taught.

**(a) "Use pizza — it CAN fail on this bug class." Structurally true, practically
not.** Pizza has the hazardous id layout: five user classes
(`pro/ContainedRole`, `pro/DevelopmentRole`, `pro/OnTopPositionRole`,
`pro/PersistingRole`, `sulo/Collection`) interned above its first DKey at 87. But
all five are **hierarchy-isolated** — zero reported subsumers, zero reported
subclasses — and pizza's unsatisfiable set is empty. So the rows they misread
were empty too. The pizza inert-declaration oracle **passes on `c1f44d8`, on
`132dd21`, and on the tip.** It is a *latent-hazard canary*, not a reproducer:
same practical status as mie, for the opposite reason (hazard present but inert,
vs hazard absent). Kept anyway — it is ~0.4 s, and it becomes a live oracle on a
real ontology the day pizza gains an edge on those classes or any unsat class.
Its doc comment says all of this so nobody mistakes a passing pizza test for
proof. **The claimed "12 pairs, pre-fix" delta does not exist.**

**(b) "Your fix DID change results on `bench-corpus/pizza.ofn`; the owner's
re-validation will show a delta." It does not.** I dumped the full reported
hierarchy + unsat set for `pizza.ofn`, `mie.ofn` and `paper5.ofn` on `c1f44d8`
and on the tip and diffed:

```
377 lines each — IDENTICAL
```

And I mutation-tested the measurement instead of trusting it: adding the
`UNSAT_BODY` sentinel to the same harness produces a diff on the same two runs
(`unsat=["Ccc"]` → `["Aaa"]`, plus three pair changes), so the harness can see a
delta and there is none on the corpus.

**So: round 1's claim-4 CONCLUSION was right and its REASON was wrong.** Correct
statement for the owner:

> The change is a no-op on every tracked corpus fixture — **measured**, not
> inferred. `mie.ofn` and `paper5.ofn` have all DKeys above all user classes.
> `pizza.ofn` *does* have DKeys below five user classes, so its report positions
> genuinely were aliased pre-fix — but those five classes are
> hierarchy-isolated and its unsat set is empty, so the aliasing had no
> observable consequence. **Expect FP=0/MISSED=0 re-validation to show NO delta
> on `bench-corpus`.** If it shows one, something in this analysis is wrong and
> the delta is the more trustworthy signal.

I did not scan `ontologies/` — it is gitignored, so any finding there is not
reproducible from the repo. If the owner's sweep covers it, a delta there is
possible and would need the same per-class analysis.

## R2.5 `realize.rs` — comments corrected, behaviour untouched

As instructed, no behaviour change. Three sites now state what they actually
guarantee:

- `realize_via_saturation_internal:686` — the comment claimed "restricted to
  declared user classes". `num_user_classes` is `vocabulary.num_classes()`,
  which **counts** DKeys (they live *inside* the class vocabulary, not above it),
  so `< num_user_classes` excludes only Tseitin/nominal synthetics. The comment
  now says that, says a DKey subsumer would pass straight through, says why no
  reproducer exists (a DKey subsumes a *value* node reached through a role, not
  an individual's nominal class), and names the one-line fix and the two other
  sites to apply it at if one ever surfaces.
- `realize_saturation_only_internal:524` and `realize_internal:778` — their
  `class_iris` enumerate the **full** id space, which is *correct* and worth
  saying out loud: the filter is applied after `enumerate()` captures `(i, iri)`,
  so `i` stays a true `ClassId`. Applying a DKey filter *before* the `enumerate`
  is precisely the classify.rs bug, so these must not be "tidied" that way. The
  comments now record both the invariant and the consequence (DKey IRIs are in
  `class_iris`).

New `realize_types_survive_the_unsat_projection` asserts no DKey IRI reaches the
output, so if the hole ever becomes reachable a test says so.

## R2.6 Verification

- `cargo test -p owl-dl-reasoner --release` → 76 suites, all ok, 0 failed.
- `cargo test -p owl-dl-reasoner` (debug) → 76 suites, all ok, 0 failed.
- `cargo clippy -p owl-dl-reasoner --lib --tests -- -D warnings` → clean in
  **both** profiles.
- `cargo fmt --all` applied.
- Still **no existing test changed verdict**, and none was edited.
- `dkey_id_aliasing`: 15 tests, 15 pass in release; 14 pass + 1 ignored in
  debug (the pizza gate).

**One new gate, disclosed:** `inert_declarations_do_not_change_the_hierarchy_pizza`
is `#[cfg_attr(debug_assertions, ignore = "…")]`. `classify(pizza.ofn)` trips a
pre-existing `debug_assert!` at `owl-dl-tableau/src/hyper.rs:3677` ("≤1 violation
reached find_open_at_most under inverse_func_merge"). I verified pre-existence
rather than trusting the brief: a bare `classify(pizza.ofn)` panics there on
`c1f44d8` with none of this branch's code involved. Fixing an unrelated tableau
invariant is out of scope; `catch_unwind` would hide a real defect. `ignore`
keeps it visible in debug output and it runs for real in release. Worth the
owner knowing it exists, since it is a soundness-adjacent assert failing on a
curated corpus file.

## R2.7 Files changed (round 2)

- `crates/owl-dl-reasoner/src/classify.rs` — the one-line unsat fix + a comment
  block naming the trap and why the source guard cannot see it.
- `crates/owl-dl-reasoner/src/realize.rs` — comments only, three sites.
- `crates/owl-dl-reasoner/tests/dkey_id_aliasing.rs` — `UNSAT_BODY`; two new
  oracles; unsat-set diff in the inert-declaration oracle; corrected
  non-vacuity predicate; programmatic `with_all_classes_declared` (works on a
  corpus file, replaces hand-listed declaration text); pizza corpus oracle
  replacing the mie cross-check; guard-doc blind-spot section. 11 → 15 tests.

Skipped as instructed: the three `Vec<String>` copies on the pure-EL path, the
`pairs_per_sub` ClassId keying, the dead `if c >= n` guard.

## R2.8 Remaining concerns

- **The `ReportIdx` newtype is now clearly worth doing, and for a reason round 1
  did not have.** I argued against it on diff risk. The bitset bug changes the
  calculus: a real newtype would have made `unsat_bs.contains(i)` a type error,
  because `FixedBitSet::contains` takes `usize` and `i` would have been
  `ReportIdx`. That is the one class of defect the sentinel guard structurally
  cannot catch and the newtype structurally can. I still would not fold it into
  this commit — it is a separate, mechanical, compiler-verified change — but I no
  longer think it is optional.
- **Two rounds, two live FPs in the same function family.** Both were found by
  cross-checking against an independent oracle, neither by reading. Before the
  owner's gate I would want the `is_subclass_of` cross-check run over a handful
  of *hazardous* real ontologies (undeclared classes + data axioms + at least one
  unsat class), not just hand-built fixtures. My fixtures are 4-6 classes; they
  cannot exercise the tier walk's sweeps at depth.
- **No corpus fixture can currently fail on this bug class** (§R2.4). That is a
  gap in the corpus, not in the fix, and it is the reason both defects reached
  shipped code. A tracked fixture with a used-but-undeclared class, a DKey below
  it, and an unsatisfiable class would close it permanently — my `UNSAT_BODY` is
  that fixture in miniature, but it is a test-local string, not a corpus file the
  bench and ORE sweeps would pick up.

---
---

# Round 3 — final round

**Commit:** `822a8d5` · Both items done. Full branch: `132dd21` → `954bc7e` → `822a8d5`.

## R3.1 The corpus canary now runs in CI — with its limits stated

The coordinator is right and this was a real hole of my own making. I gated the
pizza oracle on `debug_assertions` to dodge the pre-existing `hyper.rs:3677`
assert, and `.github/workflows/ci.yml` runs
`cargo test --workspace --all-targets --exclude owl-dl-py` with **no release
job**. So the gate did not degrade the test to "release-only" — it deleted it
from CI outright. My round-2 report claimed it "becomes a live oracle on a real
ontology the day pizza gains an edge, with no test change needed." As
configured, that was false. Worth naming plainly: I introduced a gate and then
described the gated test as if it still ran.

Fix taken (the re-reviewer's, which is better than gating): make the oracle take
the classifier as a parameter and add `classify_saturation_only` variants.
`classify_saturation_only` never reaches the hypertableau, so it does not trip
that assert, and it routes straight through `classify_pure_el` — the function
the round-2 unsat-projection FP lived in.

- `inert_declarations_do_not_change_the_hierarchy_pizza_saturation_only`
  — 92-class pizza, **runs in debug in 0.04 s**.
- `inert_declarations_do_not_change_the_saturation_only_hierarchy`
  — EL / hybrid / unsat fixtures through the same entry point.

The release-profile `classify` pizza variant is kept alongside it.

On soundness of the comparison: `classify_saturation_only` is a documented
under-approximation, so equality between two runs is not a completeness claim.
The asserted property is "adding semantically inert axioms does not change
whatever this entry point reports" — an under-approximation must still be
*stable* under an inert change. That holds regardless of completeness.

### Non-vacuity, measured — and the shared-target-dir trap, met firsthand

I ran the new tests against `classify.rs` from all three revisions in one tree:

```
classify.rs @ 954bc7e  ->  16 passed, 0 failed, 1 ignored
classify.rs @ 132dd21  ->   8 passed, 8 FAILED   (round-2 bitset bug live)
classify.rs @ c1f44d8  ->   3 passed, 13 FAILED  (round-1 projection bug live)
```

`inert_declarations_do_not_change_the_saturation_only_hierarchy` is in the
failure list on **both** bad revisions, so the new debug-runnable oracle
genuinely catches the round-2 bug — it is not a decoration.

**I walked into the warned-about trap and it caught me.** My first pass grepped
the run output for `Compiling owl-dl-reasoner` as proof of a rebuild and got
**zero on all three revisions** — the same "no `Compiling` lines" signature the
re-reviewer saw. Cause was different and more mundane: `cargo test -q`
suppresses them. The results differing across revisions already proved distinct
binaries ran, but I re-ran without `-q` for positive confirmation rather than
lean on inference:

```
##### classify.rs @ 132dd21
   Compiling owl-dl-reasoner v0.4.2 (...)
   test result: FAILED. 8 passed; 8 failed; 1 ignored
##### classify.rs @ 954bc7e
   Compiling owl-dl-reasoner v0.4.2 (...)
   test result: ok. 16 passed; 0 failed; 1 ignored
```

Carrying the lesson forward, generalised: **a "no rebuild happened" signal has
at least two causes — a shared target dir silently reusing a binary, and a
quiet flag hiding the evidence.** Both look identical from the outside and both
manufacture a false IDENTICAL. Positive confirmation of the rebuild, per
comparison, is the only safe protocol; per-tree target dirs alone would not have
saved me here.

(For the record, the round-2 corpus dump comparison this validates was done by
in-place file swaps in a single tree with a single target dir — not two
checkouts — and its per-revision results differed, which is why it was sound.
The independent three-tree re-measurement confirming byte-identity is the
stronger evidence and I defer to it.)

### Scope, stated honestly

The **pizza** saturation-only canary **passes on all three revisions**, including
both buggy ones. Pizza's unsat set is empty and its five aliased classes are
hierarchy-isolated, so it stays a latent canary — exactly the round-2 finding,
unchanged. What it buys is a corpus-level guard that *executes* in the profile CI
builds, so the latency between "pizza gains an edge" and "CI notices" is zero
instead of infinite. The load-bearing new coverage is the fixture-based variant.
Both test doc comments say this, so a passing pizza test is not mistaken for
proof.

## R3.2 `unsatisfiable_bitset` deleted

Deleted rather than renamed. Reasoning: a rename is *equally* semver-visible
(both break an out-of-workspace caller identically), so the tie-break is which
leaves less loaded gun behind — and a rename leaves the method, still handing out
a `FixedBitSet` whose `contains(usize)` accepts any index in scope. Deletion also
removes `fixedbitset` from that crate's public API, where this was its only
occurrence.

Migration is already what every in-tree caller uses: `is_unsatisfiable(ClassId)`
for a membership test, `unsatisfiable_classes() -> Vec<ClassId>` for enumeration.
Both are typed, so the report-position mistake cannot be spelled.

A replacement block comment at the removal site records the incident (what the
mis-indexing did, and the `⊥ ⊑ *` amplification that made one bit unbounded), so
the next person to want a raw bitset finds the reason it is gone.

**~~The one semver-visible change in this branch, flagged for the owner:~~ RESOLVED
2026-08-22 — the deletion stands, and the semver framing below was overstated.**
`owl-dl-saturation` has no `publish = false`, so it is *publishable*, and the
concern was that an out-of-workspace consumer pinned at 0.4.x would break. **No
such consumer can exist.** The sparse index shows `owl-dl-saturation` published at
0.1.0, 0.2.0, 0.2.1, 0.2.2, **0.3.0 and no further** — as are `owl-dl-core`,
`owl-dl-reasoner` and `owl-dl-tableau` — while the workspace is at 0.4.21. No
0.4.x was ever published and no workflow runs `cargo publish` (`release-python.yml`
goes to PyPI via maturin; `release-cli.yml` ships binaries). Even a future 0.4.x
publish compares against 0.3.0, where a `0.3 -> 0.4` minor bump is precisely where
Cargo's 0.x rules permit a breaking removal.

So the tie-break is decided on its merits alone, and deletion wins them: it removes
a `FixedBitSet` whose `contains(usize)` accepts any index in scope, and drops
`fixedbitset` from the crate's public API where it was the only occurrence. Nothing
in-tree references it (source, tests, docs, `owl-dl-py`, `owl-dl-bench`), and
`owl-dl-cli`, `owl-dl-tableau` and `owl-dl-core` still compile.

Residual exposure is a git-dependency consumer only, which semver does not govern.
A note for the record: **a rename would NOT have been the safer option** — it
removes the old symbol identically, so it breaks exactly the same callers while
keeping the footgun. `unsatisfiable_bitset_by_class_id` remains a one-line revert if
a git consumer ever turns up needing it.

## R3.3 Verification (round 3)

Crate-scoped only, as instructed — no workspace suite, no all-targets clippy.

| | debug | release |
|---|---|---|
| `cargo test -p owl-dl-reasoner` | 76 suites ok | 76 suites ok |
| `cargo test -p owl-dl-saturation` | 2 suites ok | 2 suites ok |
| `cargo clippy -p owl-dl-reasoner --lib --tests -D warnings` | clean | clean |
| `cargo clippy -p owl-dl-saturation --lib --tests -D warnings` | clean | clean |

`cargo check -p owl-dl-cli` / `-p owl-dl-tableau` / `-p owl-dl-core`: compile.
`cargo fmt --all` applied. `dkey_id_aliasing`: 17 tests — 17 pass in release,
16 pass + 1 ignored in debug (the release-only `classify` pizza variant).
**Still no existing test changed verdict, and none was edited.**

## R3.4 Files changed (round 3)

- `crates/owl-dl-reasoner/tests/dkey_id_aliasing.rs` — classifier-parameterised
  oracle; two new `classify_saturation_only` oracles (one corpus, one fixture);
  doc comments stating the CI-profile reasoning and the pizza canary's latent
  status. 15 → 17 tests.
- `crates/owl-dl-saturation/src/lib.rs` — `unsatisfiable_bitset` deleted,
  replaced by an incident/migration comment.

No production behaviour changed in round 3.

## R3.5 Closing state

The FP path is closed and the follow-ups are the coordinator's to file. Two
things I would want the owner to carry out of this, both of which are about the
*process* rather than the code:

- **Both false positives were found by an independent oracle, neither by
  reading.** Round 1's was found by cross-checking `classify` against
  `is_subclass_of`; round 2's by a reviewer building a fixture with an
  unsatisfiable class. Neither was found by the source-level lint, and round 2's
  *could not* have been. On this bug class, the differential oracle is the
  instrument; static review is the hypothesis generator.
- **My own two errors this branch were both overclaims, not miscodings**: a
  wrong hazard predicate (`last_dkey` for `first_dkey`) that called pizza safe,
  and describing a `debug_assertions`-gated test as if it still ran in CI. The
  code was checked; the claims about the code were not. The corrective in both
  cases was measurement, and in both cases someone else had to ask for it.

Follow-ups, as filed by the coordinator and not started here: the tracked
hazard-shape corpus fixture (rated above `ReportIdx`, and I agree — it puts the
bench/ORE sweeps on this bug class permanently, where today the whole net is
4-6-class hand fixtures); `ReportIdx` (`(0..n).map(ReportIdx)`, no `Deref`, no
`From<ReportIdx> for usize`, and one-directional unless the report-space
containers are keyed by it too); and `hyper.rs:3677`.
