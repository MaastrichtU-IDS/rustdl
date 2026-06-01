# Phase 3b — first fix target

Based on Phase 3 SIO flamegraph (`docs/flamegraphs/sio-classify-2026-06-01.svg`)
hot frame `are_declared_inverses` at 25.76% of `apply_max`'s inclusive cost.

## Inverse-pair counts

| Ontology | InverseObjectProperties axioms (grep) | Vec entries after `declare_inverse_pair` (2×) |
|---|---|---|
| sio-stripped.ofn | 84 | ~168 |
| galen.ofn | 0 | 0 |
| notgalen.ofn | 0 | 0 |
| alehif-test.ofn | 2 | 4 |
| ore-10908-sroiq.ofn | 24 | 48 |
| ore-15672-shoin.ofn | 15 | 30 |

The existing comment at `lib.rs:485-488` assumed "0-3 pairs" (pizza-era). SIO has
84 declared pairs; `declare_inverse_pair` stores each direction separately
(both `(r,s)` and `(s,r)`), so the runtime Vec has **~168 entries**. The linear
scan in `are_declared_inverses` is O(168) per call — ~50× what the pizza-era
comment assumed justified. This directly explains why the flamegraph attributes
25.76% of `apply_max`'s inclusive cost to the scan on SIO.

## Chosen fix

**Option A — `hashbrown::HashSet<(RoleId, RoleId)>`.**

Replace the O(N) `Vec::iter().any()` scan with an O(1) hashset lookup. The
`hashbrown` crate (AHash hasher) is already a direct dependency of the tableau
crate (see `Cargo.toml`); no new dependencies required. AHash is fast on small
`(u32, u32)` keys and avoids the `make_hash`/`find_inner` overhead that would
matter only at much higher entry counts.

The existing `inverse_pairs: Vec<(RoleId, RoleId)>` field is retained for
declaration-order iteration (used by the `decide` function in the reasoner which
passes the slice to `ctx.declare_inverse_pair` in order). A parallel field
`inverse_pairs_set: hashbrown::HashSet<(RoleId, RoleId)>` is added and kept in
sync at all mutation points.

`rustc_hash` / `FxHashSet` are not used anywhere in the workspace; there is no
reason to add that dependency when `hashbrown` is already present and equally
fast for this key size.

Option B (per-role `HashMap<RoleId, Vec<RoleId>>`) would marginally improve
lookup when a role has many inverses, but SIO's inverse declarations are
essentially 1:1 per role, so the extra indirection buys nothing. Option C (sorted
Vec + binary_search) avoids a new data structure but adds build-time complexity
and log(N) lookup vs O(1) — not worth it given `hashbrown` is already a dep.

## Implementation surface

- `crates/owl-dl-tableau/src/lib.rs`
  - Line 120: add `inverse_pairs_set: hashbrown::HashSet<(RoleId, RoleId)>` field.
  - Lines 211, 239, 272: three constructors that init `inverse_pairs: Vec::new()` —
    each needs a matching `inverse_pairs_set: hashbrown::HashSet::new()`.
  - Lines 469-478 (`declare_inverse_pair`): mirror both `push` calls with
    corresponding `insert` calls into `inverse_pairs_set`.
  - Lines 484-493 (`are_declared_inverses`): replace `iter().any()` with
    `inverse_pairs_set.contains(&(r, s))`.
- `crates/owl-dl-tableau/src/counters.rs`: new counter
  `inverse_pair_fast_hits: Cell<u64>` (bumped on every consulted `contains`
  call) + matching `dump()` entry.

No changes to `crates/owl-dl-reasoner/src/lib.rs` (the `collect_inverse_pairs`
function and `declare_inverse_pair` call site are unchanged; the Vec is still
populated in the same order and passed as a slice to the tableau builder).

## Expected impact

SIO flame: `are_declared_inverses` 25.76% → ~0–2% (hash lookup is O(1), well
below the flamegraph noise floor). `apply_max` overall: 27.93% → ~18–20%
(the 25.76% scan is the dominant component, so eliminating it drops the frame
proportionally).

SIO wall baseline ~68s; post-fix target ~50–55s (~20–26% reduction). The
reduction derives from the 25.76% of CPU time shifted from O(168) linear scans
to O(1) hashset lookups.

> **Actual outcome:** 3.44% (the residual is hashbrown's own foldhash +
> contains cost). Above the predicted 0-2% but well within the goal of
> eliminating the linear scan. See `phase3b-results.md`.

GALEN wall: GALEN has 0 inverse pairs so `are_declared_inverses` returns
immediately on the `is_empty()` fast-path — no change expected for GALEN.

ORE-SROIQ (24 axioms → 48 Vec entries): minor improvement; not a primary target.

## Soundness considerations

The fix changes ONLY the data structure for inverse-pair lookup. Logic unchanged:
same boolean returned for same inputs on any input. `hashbrown::HashSet::contains`
is a pure deterministic lookup with no side effects on the graph or trail.
Verdicts preserved on all 87 tableau + 78 reasoner-lib tests + Phase 0 net +
GALEN. The `is_empty()` fast-path guard is removed (the set's `contains` method
handles the empty case correctly and in O(1) time).
