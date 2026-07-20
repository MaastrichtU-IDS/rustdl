# Spec: bounded DKey-disjointness seeding (conversion-DNF fix)

**Status:** ready to implement. Diagnosis + oracle done; three prior attempts
failed and are recorded below so they are not repeated. Handoff target: an
implementer (human or agent) working in an **isolated git worktree**.

**Owner context:** memory `conversion-dnf-dkey-disjoint-oksquared`,
`km-headtohead-rustdl-fp` (FP-adjudication method: unsat- AND top-normalization).

---

## 1. Problem

~24 ORE ontologies **DNF during conversion** (before any reasoning), and the
larger ones stay DNF even after the shipped O(1) `told` fix (v0.3.27). Profile:
huge nominal/data-flood ABoxes with few classes (e.g. `ore_ont_10425`: 2.1 MB,
18 classes, 8 227 `DataPropertyAssertion`s, **5 261 distinct data values**).

**Root cause (gdb multi-sample + `RUSTDL_DATA_PROPERTIES=0` → converts in 1 s):**
`convert.rs::seed_dkey_subsumptions` → `seed_disjoint_bucket` (Phase D11b) emits
`DisjointClasses(DKey(ra), DKey(rb))` for **every provably-disjoint pair** within
a datatype bucket. Distinct point-values are all pairwise disjoint ⟹ **O(k²)**
axioms in the number of distinct values k (5 261 → ~14 M). Those axioms then feed
`build_told_tables`, the saturator's `disjoint_pairs`, and tableau absorption —
so k² work is incurred several times, and k also inflates the saturator's dense
`num_total_classes²` matrix.

The O(1) `told::add_disjoint_pair` (HashSet, shipped v0.3.27) drops the *told*
build from O(k³)→O(k²) but does not remove the O(k²) axiom volume. This spec
removes the volume.

---

## 2. The fix: seed disjointness only within a *merge-aware role component*

### 2.1 Key soundness/completeness fact

A `DKey(v1)`/`DKey(v2)` disjointness axiom is **only ever consumed** when both
DKeys land in **one node's label**, which happens only if they are reached via:

- the **same data role** `r` (two `∃r.DKey` fillers, merged by a functional/`≤1`
  restriction on `r`, or one `∃r.DKey(v)` clashing with a `∀r.DKey(range)`); or
- **two roles connected in the property hierarchy by a merge-inducing super**:
  `r1 ⊑ f`, `r2 ⊑ f` where `f` is functional / inverse-functional / appears in a
  `≤n` restriction / appears in a `∀f.DKey`. Then an `f`-successor is shared and
  the merge unites the `r1`- and `r2`-fillers.

Two DKeys reachable only via **unrelated roles**, or via roles whose only common
super is **non-merge-inducing** (e.g. `⊑ owl:topDataProperty`), can **never**
co-occur in one label — their mutual disjointness is dead weight.

**Therefore:** seeding `DisjointClasses(DKey,DKey)` only for pairs whose roles lie
in the same *merge-aware role component* **drops zero consumable clash** (no
silent MISS) while cutting the axiom count from O(k²) to O(Σ_component values²).

This is NOT co-occurrence guessing — it is exact about which pairs *can* be
consumed. (Co-occurrence — "values actually reachable together at a subject" — is
strictly tighter but requires reasoning-derived facts and *does* risk silent
MISS; do **not** go there.)

### 2.2 Algorithm

In `seed_dkey_subsumptions`, replace the seven unconditional `seed_disjoint_bucket`
calls with:

1. **Merge-aware role union-find** over role ids (`out.vocabulary.num_roles()`):
   - Determine the set of **merge-inducing roles** `M`:
     - `Axiom::FunctionalRole(r)` and `Axiom::InverseFunctionalRole(r)` → `r ∈ M`;
     - any `ConceptExpr::Max(_, r, _)` occurring in any axiom concept → `r ∈ M`;
     - any `ConceptExpr::All(r, f)` where `f` is a DKey filler → `r ∈ M`
       (the ∀-over-DKey case).
   - For each `Axiom::SubObjectPropertyOf { sub, sup }`: union `sub`'s role id(s)
     with `sup`'s role id **iff `sup ∈ M`** (a non-merge super never merges
     successors, so its sub-roles stay separate). For a `Chain` sub, union each
     part with `sup` under the same condition. Also: a role is trivially in its
     own component.
   - NOTE the load-bearing subtlety that killed attempt #3: **unioning on *every*
     `SubObjectPropertyOf` collapses all data properties under a shared
     `topDataProperty`-style root into one component → O(k²) again.** Union ONLY
     via merge-inducing supers.
   - Use `Role::role_id().index()` as the union-find key (ignore inverse polarity;
     data roles are not inverse in practice, and `role_id` normalizes).

2. **role → DKey-set** map: walk every axiom's class expressions
   (`SubClassOf` sub+sup, `EquivalentClasses`, `DisjointClasses`, `DisjointUnion`
   members, `ClassAssertion` class), and for each `∃r.f` / `∀r.f` where `f` is
   `Atomic(c)` with `is_dkey_iri(vocabulary.class_iri(c))`, record component
   `find(r.role_id().index())` for DKey `c`. A DKey may occur under several
   components; keep the set.

3. **Group-and-seed** (this is where attempt #2 went wrong — do NOT iterate all
   bucket pairs and filter; that leaves the O(k²) *iteration*). For each datatype
   bucket, group its `(ClassId, range)` entries by component (a DKey with several
   components joins each group), then iterate **within-group** pairs only,
   emitting `DisjointClasses([DKey(a), DKey(b)])` when `disjoint(range_a, range_b)`.
   Dedup pairs across groups (a DKey shared by two components reaches a pair
   twice). Cost = O(Σ_component (values in component)²).

### 2.3 Invariants (do not violate)

- **FP-safety:** keep the existing per-pair `disjoint(range_a, range_b)` range
  check. Only ever emit a pair proven range-disjoint. (Seeding is a *subset* of
  the current sound all-pairs set ⇒ FP-safe by construction.)
- **Completeness:** the component bound must be **merge-aware** (§2.2 step 1). If
  in doubt, over-union (coarser components) — that only reduces the perf win, it
  never causes a MISS. Under-unioning (missing a real merge-inducing super) is a
  MISS bug.
- **`RUSTDL_DATA_PROPERTIES=0`** path must be byte-identical (no DKeys exist
  there).

---

## 3. Dead-ends (do not repeat)

1. **`∀p.DKey`-only gate** (shipped v0.3.27, reverted v0.3.28): skipped seeding
   unless a `∀p.DKey` existed. WRONG — the functional/`≤1` *merge* clash
   (`∃p.DKey(v1) ⊓ ∃p.DKey(v2)`) consumes the disjointness with no `∀`. Dropped it
   → `data_properties.rs` POC tests failed on all platforms. A completeness
   regression (FP-safe, silent — only the POC tests caught it).
2. **Per-pair component *guard*** over the O(k²) bucket walk: correct set, but the
   27 M-iteration × HashMap lookup itself stalled → *more* DNFs than the gate.
   Must group first, not filter.
3. **Naive (non-merge-aware) role components**: `ore_ont_10425`'s 45
   `SubProperty` axioms union all its data properties into ONE component (shared
   super) → one group of 5 261 → O(k²) again. Hence §2.2's merge-aware union.

---

## 4. Files

- `crates/owl-dl-core/src/convert.rs` — `seed_dkey_subsumptions` (build the
  components + role→DKey map) and `seed_disjoint_bucket` (group-and-seed;
  currently takes `(out, keys, disjoint)` — add the component map). The `*_dkeys`
  bucket vectors and `is_dkey_iri` / `DKEY_IRI_PREFIX` already exist.
- `crates/owl-dl-core/src/told.rs` — `add_disjoint_pair` already O(1) (HashSet),
  shipped; leave as is.
- No consumer changes (the output is still native `DisjointClasses` axioms, just
  fewer).

---

## 5. Acceptance gates (ALL required before claiming done)

Run locally with the stable toolchain (`RUSTUP_TOOLCHAIN=stable`, PATH per
CLAUDE.md). Do **not** push on a subset — the v0.3.27 regression slipped exactly
because only the datatype canaries were run, not the functional-merge POCs.

1. `cargo test -p owl-dl-reasoner --test data_properties` → **9/9** (functional/`≤1`
   merge-clash POCs — the anti-regression gate).
2. `cargo test -p owl-dl-reasoner --test datatype_value_membership` → **66/66**
   (∀-DKey membership clashes).
3. `cargo test -p owl-dl-core told` → **11/11** (told-disjoint correctness).
4. `cargo test --workspace --all-targets --exclude owl-dl-py` → green.
5. `cargo fmt --all -- --check` (modulo the pre-existing `explain.rs`) + `cargo
   clippy --workspace --all-targets --all-features -- -D warnings` clean.
6. **Curated MISSED=0 / byte-identical**: `sio` classify = 1617 lines; the
   datatype-bearing curated fixtures unchanged.
7. **THE ORACLE (the gate that catches the silent MISS):**
   `/mnt/um-share-drive/dumontier/rustdl-scratch/oracle_dkey.py`
   ```
   python3 oracle_dkey.py \
     --candidate  <fixed release rustdl> \
     --baseline   <v0.3.28 release rustdl> \
     --manifest   /mnt/um-share-drive/dumontier/rustdl-scratch/manifest_dkey.txt
   ```
   Must print `ACCEPTANCE: PASS`, i.e. over every ontology the candidate
   classifies: **FP == 0** (vs Konclude, unsat+top-normalized) **and REGR == 0**
   (candidate loses nothing the v0.3.28 baseline had). RECOVER-tagged onts should
   flip baseline-DNF → candidate-classified with FP=0/MISS=0. `MISS` that is not a
   `REGR` (baseline also missed it, or a per-pair-budget cut) is acceptable and is
   reported separately — it is a pre-existing gap, not caused by this change.
   Konclude DNFs ~4/24 recovery targets (no oracle for those — note, don't fail).

## 6. Risk & discipline

- Silent-MISS is the whole danger. Gate #7 is mandatory and non-substitutable by
  the curated/unit gates (curated has almost no functional-DKey/∀-DKey structure).
- If the implementation does not converge in ~2 attempts, or the oracle shows any
  MISS/REGR that can't be quickly explained, **revert** to v0.3.28 (correct, O(k²)
  slow, no regression) and report — do not keep patching. This tail is
  low-value (Konclude itself DNFs 4/24); correctness ≫ recovery.
- Gate behind a default-ON env flag (`RUSTDL_BOUNDED_DKEY_DISJOINT`?) per project
  convention, with `=0` restoring the unconditional seeding, so the change is
  A/B-reversible.
