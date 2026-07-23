# Plan: index the ABox-saturation disjoint-clash checks (Rule 8 + Rule 7b)

**Status:** advisor-reviewed 2026-07-23 (APPROVE-WITH-CHANGES; both blocking items folded in) — ready for delegation to Fable.
**Author:** Claude, 2026-07-23. Session: issue-#35 realize/inconsistency perf arc (Phase 2 — sibling of the chain-index plan, now shipped in v0.3.36).
**Branch (to create):** `perf/abox-disjoint-index`.

> **Advisor outcome.** Judged **materially lower-risk than the chain fix**: Rules
> 7b/8 write **only** the monotone `result.clash` bool (no derived types/edges,
> no downstream consumer), so the transformation is a provable set-membership
> equivalence and the sole observable is the verdict — no closure-identity gate,
> no completeness cliff. The one real risk (symmetric-adjacency omission) is
> already mandated (§4.1) and caught by the §5.1 A/B (which includes 10
> inconsistent onts). Two blocking fixes applied: **B1** — guard Rule 8 so the
> disjoint-free path stays inert (the naive rewrite added per-iteration cost on
> ABox onts with no `DisjointClasses`); **B2** — the "second fixture" must be a
> structurally *distinct* ont (the old candidate was a dup of 9899), and it
> gates *perf-generalization only*, not correctness.

## 1. Problem (measured this session, not theorized)

After the Phase-1a chain-index fix (v0.3.36), the ABox-saturation pre-check is
still the dominant cost on ORE ABox ontologies with disjoint classes.
`sample`-profiling `ore_ont_9899` (`is_consistent`, 48.8 s wall) attributes
**~99.9 % of the sampled window to `saturate_abox_consistency`**, concentrated at
`abox_saturation.rs:947–949`. The disable-flag A/B splits the wall:

| phase | wall |
|---|---|
| `abox_saturation` pre-check (Rule 8) | **~27.5 s** |
| hybrid tableau consistency (`RUSTDL_ABOX_SATURATION=0`) | ~21.3 s |

The hot spot is **Rule 8, the disjoint-clash check** (`abox_saturation.rs:946–`):

```rust
for &(c1, c2) in &disjoint_pairs {      // O(|disjoint_pairs|)
    for (ind, ind_types) in &types {    // × O(|individuals|), EVERY fixpoint iteration
        if ind_types.contains(&c1) && ind_types.contains(&c2) { result.clash = true; }
    }
}
```

`ore_ont_9899`: 31 `DisjointClasses` axioms (→ hundreds of pairs after pairwise
expansion), ~3396 `ClassAssertion`s (→ thousands of individuals in `types`),
×~5 fixpoint iterations ⇒ the `O(D × I)` product is the ~27 s. Most (ind, pair)
probes are misses (the individual has neither class).

**Same-family latent cost — Rule 7b** (`abox_saturation.rs:917–944`, the
functional existential-marker clash): inside a `fillers × fillers` loop it does
`disjoint_pairs.iter().any(...)` — an O(|disjoint_pairs|) linear scan per filler
pair. Not the top sample on 9899 (it has no functional-marker-heavy individual),
but the identical anti-pattern; fix both in one change.

This is the **same anti-pattern** as the chain phase: a brute scan re-run every
fixpoint iteration. Unlike the chain fix (which helped only the family class),
this one is expected to help **broadly** — many ORE ABox ontologies declare
disjoint classes.

## 2. Goal & non-goals

**Goal:** detect the *same* clashes with class-indexed lookups instead of the
`O(D × I)` / linear-scan probes. Target: `ore_ont_9899` `abox_saturation` phase
~27.5 s → ≤ ~2 s (the ~21.3 s tableau half is out of scope, §8).

**Non-goals:**
- Do **not** change *which* clashes are detected. Rule 8 and Rule 7b only set
  `result.clash` (they derive no edges/types), so the sole observable is the
  `clash: bool` verdict — the gate is **verdict-identity**, not closure identity.
  Much simpler than the chain fix.
- Do **not** change `disjoint_pairs` construction (seeding), the fixpoint
  structure, or any other rule.
- Do **not** touch the hybrid tableau consistency path (the other ~21 s).

## 3. Phase 0 — measurement GATE (largely done, formalize)

- **GO (held on 9899):** `sample` shows ≥ 80 % of the pre-check in Rule-8 lines
  947–949 (measured ~99.9 % of the sampled window); the `RUSTDL_ABOX_SATURATION=0`
  A/B shows the pre-check is ~27.5 s of the 48.8 s (so removing it is a real win).
- **Second fixture — PERF-GENERALIZATION only, not correctness (advisor B2).**
  Re-measure on a **structurally distinct** ORE ABox ont (NOT byte-identical to
  9899 — `ore_ont_6132` is a dup of 9899 and gives zero independent evidence).
  Selection criterion against the fetched 79-ont ABox set: `DisjointClasses ≥ 1`,
  `ClassAssertion ≥ 1000`, **ideally consistent** (so Rule 8 runs to fixpoint
  rather than short-circuiting on clash). Confirm Rule-8 dominance is not
  9899-specific; if not dominant there, still GO on 9899's strength, note it.
  This gate is about *perf generalization* — the **correctness net is the §5.1
  verdict-identity A/B across family + all 79 ORE ABox onts (incl. the 10
  inconsistent ones)**, which is strong and independent of this fixture choice.
- **NO-GO if:** on re-measure the pre-check cost is actually the functional-merge
  `fillers_by_subj` rebuild or the type/edge queue (it is not, per the sample).

## 4. Phase 1 — class-indexed disjointness

Build **once, before the fixpoint** (from the already-seeded `disjoint_pairs`,
which is static after seeding — do NOT rebuild per iteration):

```rust
// symmetric adjacency: for each class, the classes told-disjoint from it
let mut disjoint_of: HashMap<ClassId, Vec<ClassId>> = HashMap::new();
for &(c1, c2) in &disjoint_pairs {
    disjoint_of.entry(c1).or_default().push(c2);
    disjoint_of.entry(c2).or_default().push(c1);   // BOTH directions
}
```

### 4.1 Rule 8 — type-driven clash detection

Replace the `O(D × I)` pair×individual scan with a per-individual, per-type
lookup. **Guard the whole phase on `!disjoint_of.is_empty()` (advisor B1)** — the
old `for (c1,c2) in disjoint_pairs {}` was zero-work when there are no disjoint
pairs, but the rewrite would otherwise scan every individual×type every
iteration on disjoint-free ABox ontologies (a gratuitous new cost). With the
guard, the disjoint-free path stays exactly as free as today.

```rust
if !disjoint_of.is_empty() {
'outer: for (ind, ind_types) in &types {
    for &c1 in ind_types {                       // iterate the individual's OWN types
        if let Some(partners) = disjoint_of.get(&c1) {
            for &c2 in partners {
                if ind_types.contains(&c2) {     // c1 and its disjoint partner c2 both present
                    // (trace: same message as today)
                    result.clash = true;
                    break 'outer;                // one clash is enough; matches today's early break-on-clash
                }
            }
        }
    }
}
}
```

Complexity `O(Σ_ind |types(ind)| × avg-disjoint-partners)` — bounded by the
individuals that actually carry a disjoint-participating class, vs. the old
`D × I` that probed every pair against every individual. Correct: finds a clash
iff some individual's type set contains a told-disjoint pair — identical
predicate, both directions covered by the symmetric `disjoint_of`.

**Break semantics (advisor-confirmed verdict-equivalent).** Today's loop does NOT
`break` on clash (it sets `result.clash = true` and keeps scanning, then the
outer `while changed` breaks after the phase via `if result.clash { break; }`).
`break 'outer` on first hit is verdict-equivalent (clash is monotone; the
fixpoint exits either way). Confirmed: Rule 8 is the **last** rule before
`if result.clash { break }`, and nothing after it in the iteration reads the scan
result. The only observable difference is `RUSTDL_TRACE` output: `break 'outer`
prints at most one `[abox-sat] CLASH` line per iteration instead of one per
clashing (ind, pair) — diagnostic-only, no verdict impact (noted here so a future
trace-diff isn't surprising).

### 4.2 Rule 7b — HashSet membership

Build a normalized membership set once:

```rust
let disjoint_set: HashSet<(ClassId, ClassId)> =
    disjoint_pairs.iter().map(|&(a, b)| if a <= b { (a, b) } else { (b, a) }).collect();
```

Replace `disjoint_pairs.iter().any(|&(d1,d2)| (d1==f1&&d2==f2)||(d1==f2&&d2==f1))`
with `disjoint_set.contains(&if f1 <= f2 { (f1, f2) } else { (f2, f1) })`.

### 4.3 Escape hatch

`RUSTDL_ABOX_DISJOINT_BRUTE=1` keeps the old Rule-8 pair×individual scan and the
old Rule-7b linear scan, for one release of A/B validation.

## 5. Correctness gates (mandatory, in order)

The gate is **verdict-identity** (Rule 8/7b produce only `clash: bool`).

1. **A/B verdict-identity, family + all 79 ORE ABox onts:** `is_consistent`
   verdict byte-identical `RUSTDL_ABOX_DISJOINT_BRUTE=1` vs default. (Reuse the
   79-ont ABox sweep harness from the chain-index validation.)
2. **Corpus consistency:** family/pizza/ro/sulo/wine/galen verdicts unchanged.
3. **Full suite** `cargo test -p owl-dl-reasoner -p owl-dl-tableau` green;
   `abox_saturation` unit tests unchanged; add a unit test with an individual
   carrying a told-disjoint pair (asserts `clash`), one carrying a
   non-disjoint pair (asserts no clash from this rule), and a functional-marker
   disjoint case for Rule 7b.
4. **fmt + clippy -D warnings.**
5. **Perf:** `ore_ont_9899` — the (default − `RUSTDL_ABOX_SATURATION=0`) gap
   (i.e. the pre-check cost) drops from ~27.5 s to ≤ ~2 s; verdict unchanged.

## 6. Risks

- **Symmetric-adjacency omission.** If `disjoint_of` stores only one direction, a
  clash is missed when the individual is iterated from the "other" class →
  **completeness-unsound** (MISS a real inconsistency). Mitigation: store BOTH
  directions (explicit in §4.1); the §5.1 A/B gate on 79 onts (which include 10
  inconsistent ones) is the direct check.
- **Self-disjoint edge case.** `DisjointClasses(C, C)` (degenerate) would make
  `C` unsatisfiable; `disjoint_of[C]` would contain `C`, and `ind_types.contains(C)`
  fires on a single membership. Confirm the brute path treats `(C,C)` the same
  (it checks `contains(c1) && contains(c2)` with `c1==c2` ⇒ single membership) —
  so behavior is preserved. Note in a unit test.
- **`disjoint_pairs` with duplicates** (same pair from overlapping
  `DisjointClasses` axioms) — harmless for a HashMap/HashSet; the brute path also
  double-counts harmlessly (clash is idempotent). `disjoint_of` Vec values may
  hold duplicate partners (redundant inner probes) — harmless; dedup optional.
- **Empty / disjoint-free path (advisor B1):** NOT naturally inert in the rewrite
  — guarded to inert via `if !disjoint_of.is_empty()` (§4.1). Without the guard
  the new Rule 8 would scan every individual×type each iteration on ABox onts
  with no `DisjointClasses`.
- **`DisjointUnion` is not handled** (no match arm → `disjoint_pairs` never gets
  its pairwise disjointness) — a **pre-existing** sound under-approximation. The
  reindex faithfully inherits it: it neither widens nor narrows disjointness
  coverage. Called out so no one assumes indexing changed the semantics.
- Low overall risk: no closure/edge output, verdict-only, static index built once.

## 7. Delegation notes (Fable)

- Single file: `crates/owl-dl-reasoner/src/abox_saturation.rs`. No public API
  change.
- Build `disjoint_of` + `disjoint_set` once, right after `disjoint_pairs` is
  finalized (before the `while changed` loop). Keep the diff to Rules 8 and 7b +
  the two index builds + the escape hatch.
- Commit order: single commit (it's small); gate on the §5.1 A/B sweep.

## 8. Follow-ups (out of scope)

- The ~21.3 s **hybrid tableau consistency** path on 9899 (3396-assertion ABox) —
  the larger remaining frontier; separate profiling + plan.
- Index the functional-merge `fillers_by_subj` rebuild (noted in the chain plan §8).
