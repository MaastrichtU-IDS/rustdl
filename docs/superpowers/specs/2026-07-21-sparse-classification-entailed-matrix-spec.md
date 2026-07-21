# Spec: sparse the `Classification.entailed` matrix (D4 residual — giant-ontology memory + print wall)

**Status:** ready to implement (delegated build).
**Author:** Claude (root-caused + gated), 2026-07-21.
**Session:** ef9e3672 (D4 arc).
**Gate flag:** none — this is a representation change, always on. (A test-only
`RUSTDL_CLASSIFY_DENSE_MAX` env override is added purely to force the sparse path
under the oracle gate; see §6.)

---

## 1. Problem (measured, not theorized)

`crates/owl-dl-reasoner/src/classify.rs` stores the class-subsumption result as

```rust
pub struct Classification {
    ...
    entailed: Vec<FixedBitSet>,   // entailed[i].contains(j)  ⟺  classes[i] ⊑ classes[j]
    unsatisfiable_idxs: HashSet<usize>,
    ...
}
```

All four builders allocate this as `(0..n).map(|_| FixedBitSet::with_capacity(n)).collect()`
— **n rows × n bits allocated up front, regardless of content**. For a giant EL
ontology this is the whole cost:

- `ore_ont_868` (170 MB file, **981,151 classes**, pure-EL): the dense matrix is
  981151² / 8 = **112 GB**. Measured peak RSS on real `classify` = **116 GB**.
- Corpus-wide the ORE memcap onts (10689/9674/868 ≈981k→112GB, 8486 904k→95GB,
  14459 848k→84GB, 16008 733k→63GB, 14042/11395 517k→31GB) are all this matrix.

**Two walls, one root (measured on 868, real `classify`, 150GB/20min gate,
2026-07-21 — `gate868.out` on the share drive):**

1. **Memory.** Saturation + matrix build reaches 116 GB by t=90s (OOMs at a normal
   budget; the box has 251 GB so it *builds*).
2. **Time.** RSS then sits **flat at 116 GB from t=90s→1200s** (pure CPU, no
   allocation) while the CLI hierarchy print streams output — **372,948 lines
   emitted before the 20-min kill, still not done.** The print calls
   `equivalent_classes(c)` and `direct_subsumers(c)` for every class
   (`crates/owl-dl-cli/src/main.rs:658,668`), and each of those **scans `0..n`**
   → **O(n²)** total. At n=981k the print alone does not finish in 20 minutes even
   on the fast dense bitset.

**Both walls are fixed by the same change**: store each row sparsely (only the
classes it actually subsumes) and make the accessors iterate the sparse row
(O(k), k = #subsumers per class ≈ 16 on 868) instead of scanning `0..n` (O(n)).
868's real closure is ~15.8M entries ≈ a few hundred MB, vs 112 GB dense.

## 2. Non-goals / scope

- **In scope:** the `entailed` field representation + the three accessors
  (`is_subclass`, `equivalent_classes`, `direct_subsumers`) + the four builder
  sites, all inside `classify.rs`. The `.entailed` field is accessed **only**
  inside `classify.rs` (verified workspace-wide) — no external consumer touches
  it; every external caller uses the three accessors, whose **signatures must not
  change**.
- **Out of scope (leave as-is):**
  - The naive n²-pairwise builder (`classify`, site ~727) has its own O(n²) `work`
    Vec that sparse-`entailed` does *not* fix — **but no giant enters this path**
    (giants are routed to `classify_pure_el` / the fast paths), so it is correct
    to leave it. Do NOT "fix" it and do NOT flag its untouched O(n²) as a
    regression. Its `entailed` still becomes the new type (all four sites share
    the field), but its per-pair loop is unchanged.
  - Python `subclasses_of`/`superclasses_of` (`__init__.py`) and `realize.rs`
    most-specific-type loops call `is_subclass` in O(n)/O(n²) patterns of their
    own — separate concern, not this arc.

## 3. Target path (what actually needs the fix)

- `ore_ont_868` uses **`classify_pure_el`** (site ~843) — the primary target.
- `classify_top_down_internal` (site ~2094, SROIQ/hybrid) also builds the dense
  matrix — fix it too (a giant SROIQ input would hit it).
- `classify_inconsistent` (site ~887) fills every row dense (`insert_range(..n)`)
  — the unsat-row elision (§5) removes this dense fill for free.
- `classify` n²-path (site ~727) — field type changes, per-pair loop unchanged.

## 4. Representation

Introduce a private adaptive matrix type in `classify.rs` (mirrors the shipped
`owl-dl-saturation::IdMatrix` Dense/Sparse precedent — do **not** try to promote
that private type cross-crate; a local type is simpler):

```rust
/// Row-major subsumption matrix. `Dense` is the current byte-identical
/// `Vec<FixedBitSet>` (O(1) contains, keeps the EL niche fast). `Sparse` stores
/// each row as an ASCENDING-sorted `Vec<u32>` of subsumer ids (O(log k) contains),
/// used only for ontologies larger than `DENSE_MAX` where the dense n×n bitset is
/// intractable (868: 112 GB dense vs a few hundred MB sparse).
enum EntailmentMatrix {
    Dense(Vec<FixedBitSet>),
    Sparse(Vec<Vec<u32>>),   // rows[i] sorted ascending; UNSAT rows are EMPTY (elided)
}

const DENSE_MAX: usize = 60_000; // largest curated fixture is go-basic ~52k classes;
                                 // 60k keeps every curated fixture on the byte-identical
                                 // dense path (≤450 MB) while every ORE giant (≫100k) is sparse.
```

Builder chooses the arm once from `n` (test-overridable, §6). Provide a small
builder API so the four sites stay readable, e.g.:

```rust
impl EntailmentMatrix {
    fn new(n: usize) -> Self;                 // Dense if n <= dense_max(), else Sparse
    fn set_row_from_sorted(&mut self, i: usize, sorted_subsumers: &[u32]); // Sparse: move; Dense: set bits
    // Dense builders may keep using FixedBitSet directly if cleaner; the point
    // is the field type + the read path, not forcing a uniform write API.
    fn row_contains(&self, i: usize, j: usize) -> bool;  // Dense: bit; Sparse: binary_search
    fn row_ascending(&self, i: usize) -> impl Iterator<Item = usize> + '_; // members of row i, ascending
}
```

**UNSAT rows are never materialized in `Sparse`** (they are `insert_range(..n)`
today — n bits each; on 868 a single unsat class would be 122 MB). They are
recovered via the choke-point in §5. In `Dense` you MAY keep materializing them
(byte-identical) or also elide — either is fine as long as §5 holds; simplest is
to elide in both arms and let §5 do the work.

## 5. Correctness: single `entails` choke-point + unsat elision (advisor Landmine 2)

**Invariant (state it in a code comment):** *a satisfiable class's row contains
only satisfiable supers.* Every builder already skips unsat `j`
(`classify_pure_el:859`, n²-path:737, top-down seeds from the closure which
excludes unsat). Therefore `C ⊑ D` with `C` satisfiable never has `D` unsat, and
the only place "unsat ⊑ everything" must be synthesized is when the **subject** is
unsat.

**Every row read MUST go through one private method — no accessor may touch a raw
row:**

```rust
impl Classification {
    /// True iff classes[i] ⊑ classes[j]. An unsatisfiable i subsumes everything
    /// (⊥ ⊑ *) — its row is NOT materialized in the sparse rep, so this is the
    /// ONLY place that fact is reintroduced. All accessors route through here.
    fn entails(&self, i: usize, j: usize) -> bool {
        self.unsatisfiable_idxs.contains(&i) || self.entailed.row_contains(i, j)
    }
}
```

Accessor rewrites (all must call `entails`, never `entailed[..].contains`):

- **`is_subclass(sub, sup)`** → `self.entails(i, j)`.

- **`equivalent_classes(c)`** — return `{j : i⊑j ∧ j⊑i}` (incl. `i`), ASCENDING:
  - If `i` unsat: return **all unsat classes** (from `unsatisfiable_idxs`, sorted
    ascending) — they are mutually ≡⊥. (Do NOT scan rows.)
  - Else: candidates are `entailed.row_ascending(i)` (i's supers, all satisfiable
    by the invariant) plus reflexive `i`; keep `j` iff `entails(j, i)`. Merge the
    reflexive `i` in sorted position so output stays ascending.

- **`direct_subsumers(c)`** — Hasse-direct strict supers, ASCENDING:
  - If `i` unsat: degenerate case — keep a **`0..n` scan** (rare; only an
    unsat subject; correctness over speed here). Matches today's semantics.
  - Else: `strict = [ j in row_ascending(i) : j != i && !entails(j, i) ]`
    (row is satisfiable-only, so this is O(k)); then prune `j` if
    `∃ k in strict : k != j && entails(k, j) && !entails(j, k)` (O(strict²),
    strict is small). Preserve ascending order.

**Ordering (advisor Landmine 3):** today's accessors return `(0..n).filter(..)`
= ascending. Sparse rows are sorted ascending, so `direct_subsumers` is ascending
for free; `equivalent_classes` must insert reflexive `i` in sorted position (not
prepend). The identity test (§6) compares **ordered Vecs, not sets**.

## 6. Correctness gates (in priority order)

**Gate 1 — dense-vs-sparse self-diff on real fixtures (HIGHEST VALUE; the default
fixtures run the UNCHANGED dense path, so without this the sparse accessors are
validated only by a toy test).** Add a test-only env override so `dense_max()`
reads `RUSTDL_CLASSIFY_DENSE_MAX` when set (default 60_000). Then, for **galen**
(2748) and one mid-size fixture (**sio** 1585): run `rustdl classify` twice —
once at the default threshold (Dense) and once with `RUSTDL_CLASSIFY_DENSE_MAX=0`
(all-Sparse) — and assert the two hierarchy outputs are **byte-identical** (sort
both if edge-emission order could differ, but per §5 ordering it should not).
Because the Dense path is already Konclude-validated (FP=0/MISSED=0 corpus-wide),
byte-identical Sparse output is transitively Konclude-validated — with **no
Konclude dependency**. (Optional bonus: also run the in-repo Konclude
`oracle_diff` all-sparse if Konclude is set up, but the self-diff is the required
gate.) Correctness-only; wall-irrelevant.

**Gate 2 — dense-vs-sparse semantic identity unit test** (mirrors the saturator's
`id_matrix_dense_and_sparse_are_semantically_identical`). Build ONE small
`Classification` twice — once forced Dense, once forced Sparse (via the override)
— from a fixture that MUST contain: **(a) an unsatisfiable class, (b) an
equivalence pair (A≡B), (c) a 3-level chain A⊑B⊑C.** Assert, for the ordered
outputs:
  - `is_subclass(x,y)` agrees for **every** ordered pair (incl. unsat subjects),
  - `equivalent_classes(x)` agrees (ordered Vec) for every x (incl. the unsat
    class → all-unsat, and each member of the equiv pair),
  - `direct_subsumers(x)` agrees (ordered Vec) for every x (incl. unsat + the
    chain, so the Hasse prune is exercised).
A trivial 3-class chain with no unsat / no equiv would pass while every risky path
ships untested — the fixture contents above are mandatory.

**Gate 3 — default-path perf non-regression.** galen classify wall at the DEFAULT
threshold (galen 2748 ≪ 60k → Dense → byte-identical code) must be unchanged
(±noise). This is the EL-niche guard; keep it SEPARATE from Gate 1 (different
threshold, different purpose).

**Gate 4 — full curated corpus FP=0/MISSED=0** at default threshold
(`scripts/bench-rustdl-modes.sh` / the oracle net: galen, notgalen, sio, wine,
ore-10908, ore-15672, alehif, ro, pizza, bibtex). Byte-identical closures — this
is the crown-jewel invariant.

**Gate 5 — full workspace test suite green** (`cargo test --workspace`) +
`cargo clippy --workspace --all-targets --all-features -- -D warnings` +
`cargo fmt --all -- --check`. CI is `-D warnings`; a single warning fails it.

## 7. Payoff proof (distinct from correctness — the number that justifies the arc)

Re-run the **868 real `classify`** gate (`gate868.sh`, 150GB/20min watchdog).
Required outcome: **completes** (no TIMEOUT), **peak RSS sub-GB** (was 116 GB),
and a **sane direct-edge count** (~15.8M subsumption entries → millions of
`direct` edges; non-empty, plausible). The unit test proves dense==sparse; only
this re-run proves the wall collapses. Expected: 16 avg subsumers/class → O(k)
accessors finish in seconds, converting the TIMEOUT into a clean completion at a
normal memory budget. Record the before/after in a results doc.

## 8. Build discipline

- TDD: write Gate 2's identity test first, watch it fail (accessors not yet sparse
  / type not introduced), then implement.
- One representation change; no "while I'm here" refactors of unrelated classify
  code.
- Commit trailers (mandatory):
  `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`
  `Claude-Session: https://claude.ai/code/session_01BPU4DH5DXn2jmpuXdfijF7`
- Do NOT commit or push unless the human asks; leave the branch for review.

## 9. Files

- `crates/owl-dl-reasoner/src/classify.rs` — field type, `EntailmentMatrix`,
  `dense_max()` (env override), `entails`, three accessors, four builder sites.
- New test: `crates/owl-dl-reasoner/tests/sparse_classification_identity.rs`
  (Gate 2) + the fixture (inline `.ofn` string or `tests/fixtures/...`).
- Results doc: `docs/2026-07-21-sparse-classification-results.md` (868
  before/after, gate outcomes).
