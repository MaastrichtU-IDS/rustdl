# RESOLVED: DKey id aliasing — two false positives in `classify()`, both fixed

**Date:** 2026-08-20
**Branch:** `fix/dkey-id-aliasing` (based on `feat/complex-class-expression-queries-48` @ `c1f44d8`)
**Commits:** `132dd21` (29 sites) → `954bc7e` (the 30th, and the severe one) → `822a8d5` (CI canary + foot-gun)
**Severity:** was FP ≠ 0 in the public `classify()` — the failure mode this project treats as
never acceptable.
**Status:** fixed and verified. Corpus FP=0 / MISSED=0 re-validation is still the owner's gate.

Supersedes the reproduction instructions in
`docs/known-limitations/dkey-id-aliasing-classify-fp.md` (written on the P1 branch, **wrong** —
see §1). That file should be updated or replaced when the branches reconcile.

## 1. CORRECTION: the trigger is a used-but-undeclared class, not `mie.ofn`

The original write-up said `bench-corpus/mie.ofn` exhibits the bug. **It does not.** Measured:
mie declares all 84 of its classes, so all 17 DKeys land at ids 84–100 — above every user class —
and `classify` agrees with `is_subclass_of` on all 241 positives.

The actual trigger: **a class that is used but not declared.** Every `DeclareClass` component sorts
before every axiom in `convert_ontology`, so a declared class is always interned before any DKey.
An *undeclared* class is interned when first used — which can be after a DKey.

This reconciles the earlier report, which claimed FPs on "a half of `mie.ofn`": **splitting** an
ontology drops `Declaration` axioms for classes the retained axioms still use, manufacturing the
undeclared-but-used condition mechanically. It also means the trigger is not exotic — plenty of
real ontologies do not declare every class they use, and any tool that subsets one creates it.

The correct non-vacuity predicate for a fixture is `first_dkey_id < last_user_class_id`.

## 2. Two defects, not one

**(a) 29 report-position ↔ `ClassId` conflations in `classify.rs`**, in *both* directions —
`ClassId::new(i)` over a report index (21 sites) and `id.index() as usize` guarded by `if i < n`
(8 sites, including the entailment-matrix closure seed). Fixed in `132dd21` by a `ReportedClasses`
type owning the bijection both ways.

**(b) A 30th site, and the severe one: `classify_pure_el` Pass 1** probed the ClassId-indexed
`Subsumers::unsatisfiable_bitset()` with a report position. Fixed in `954bc7e`
(`closure.is_unsatisfiable(reported.class_id(i))`).

(b) is worse than (a) because of amplification: `Classification::entails` short-circuits on
`unsatisfiable_idxs` to supply `⊥ ⊑ *`, so **one** mis-indexed bit makes the wrong class subsume
**everything**. Reproduced on all four entry points (`classify`, `classify_n2`,
`classify_saturation_only`, `classify_top_down_with_timeout`), plus a corrupted `realize()` and a
wrong `unsatisfiable_classes()`.

Verified by independent build-and-measure across three separately compiled trees:

| tree | EL fixture | UNSAT fixture |
|---|---|---|
| `c1f44d8` | FP `Yy ⊑ Uu` | FP `Ccc ⊑ Aaa`, unsat=[Ccc] |
| `132dd21` | correct | **FP still present** |
| `954bc7e` | correct | correct, unsat=[Aaa] |

## 3. What the owner should expect from the corpus re-validation

**No delta on `bench-corpus`.** Measured three ways: the class/unsat/entailed-pair dump over
`mie.ofn`, `paper5.ofn` and `pizza.ofn` is byte-identical from `c1f44d8` through `132dd21` to
`954bc7e`, from three independently built binaries, with a sentinel that *does* diff.

`bench-corpus/pizza.ofn` **is** hazardous by layout (first DKey id 87, last user class id 95) but
the consequence is nil: all five aliased report positions have empty supers and subs, the rows they
misread were also empty, and its unsat set is empty on every commit. An intermediate review claimed
a "12 pairs pre-fix" delta on pizza — **that delta does not exist; disregard it.**

If the sweep *does* show a `bench-corpus` delta, trust the delta and treat this analysis as broken.
A delta on untracked `ontologies/` is possible and would need per-class analysis.

## 4. Why both defects shipped, and what now guards them

The source-level guard (`report_positions_are_never_cast_to_class_ids`) catches two spellings —
`ClassId::new(i)` and `.index() as usize`. Defect (b) has **no cast at all**: it is an implicit
report-position-as-bitset-index. So the guard was green on a file with a live FP path. Its doc now
carries an explicit blind-spot section quoting the bitset bug.

**Only behavioural oracles caught (b).** The load-bearing ones now:
- `assert_inert_declarations_are_inert` — adding axioms that entail nothing must not change the
  hierarchy. Parameterised over the classifier, with `classify_saturation_only` variants that run
  in **debug** in 0.04 s through `classify_pure_el` (the function defect (b) lived in).
- Non-vacuity measured across revisions: 16 pass at `954bc7e`, **8 FAILED at `132dd21`**,
  **13 FAILED at `c1f44d8`** — the fixture-based saturation-only oracle fails on both buggy
  revisions.

`Subsumers::unsatisfiable_bitset()` was **deleted** (zero callers; a `pub`, ClassId-indexed
foot-gun that had already caused one shipped FP). This also removes `fixedbitset` from that crate's
public API.

## 5. Open decision for the owner

**The deletion in §4 is this branch's only semver-visible change.** `owl-dl-saturation` has no
`publish = false`, so it defaults to publishable. At 0.4.x a breaking change is semver-legal in a
minor bump but not a patch. If you would rather keep the symbol, renaming it to
`unsatisfiable_bitset_by_class_id` is a one-line revert.

## 6. Follow-ups (filed, not started)

1. **A tracked corpus fixture with the hazard shape** — used-but-undeclared class, a DKey below it,
   and an unsatisfiable class. Rated **above** the newtype below in value, because it would put the
   bench/ORE sweeps on this bug class permanently. Today the entire net is 4–6-class hand fixtures,
   and **no tracked corpus fixture can fail on this bug class.**
2. **A `ReportIdx` newtype.** Declined in round 1 on risk grounds, then recommended by the same
   implementer after defect (b) — a newtype would have made `unsat_bs.contains(i)` a type error,
   which is exactly the shape the source guard cannot see. Does not block. Two design constraints
   or it buys nothing: iterate `(0..n).map(ReportIdx)` rather than `0..n`, and give it **no**
   `Deref` and no `From<ReportIdx> for usize`. It is one-directional: it will not catch a `ClassId`
   indexing a report-space `Vec` unless those containers are keyed by `ReportIdx` too.
3. **`hyper.rs:3677`** — a `debug_assert!` fires on a bare `classify(bench-corpus/pizza.ofn)` in any
   debug build, on `c1f44d8` too. Pre-existing, not a regression, but it is a soundness-adjacent
   tableau invariant violated on a curated corpus file, and it is what forces the release-only gate
   on the `classify`-based pizza oracle.
4. **`realize` has no DKey exclusion anywhere.** `realize.rs:686`'s comment claimed one; the comment
   is now corrected to match the code. `realize_saturation_only_internal` and `realize_internal`'s
   `class_iris` build the full id space with no filter. Latent — no reproducer was constructible,
   since a DKey lives on a value node rather than as a nominal subsumer.
5. **`pairs_per_sub`** (`classify.rs:463`) stays ClassId-keyed while every other pair stat is
   report-space. Verified safe today (its only consumer reads `.values()`), but it will mis-attribute
   for any consumer that indexes `classes()` with it.

## 7. Method notes worth keeping

Two ways a cross-revision comparison manufactures a false IDENTICAL, both encountered here:

1. **A shared `CARGO_TARGET_DIR`** — cargo silently reuses the other tree's binary ("Finished in
   0.12s", no `Compiling` lines).
2. **`cargo test -q`** — suppresses `Compiling` lines, so a genuine rebuild looks like a cache hit.

They are indistinguishable from the outside and both invert the conclusion. The protocol is
per-tree target dirs **and** positive per-comparison confirmation that a rebuild occurred — not
the absence of a cache-hit signal.
