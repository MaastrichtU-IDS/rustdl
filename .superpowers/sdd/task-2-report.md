### Task 2 Report: `RUSTDL_SAT_LOOKAHEAD` ⊔ failed-literal drop

**Branch:** `feat/marker-saturator-lookahead-gate`
**Status:** DONE

---

#### Summary

Wired the `SeedSaturator` (Task 1) into the hypertableau wedge as a failed-literal
propagator at ⊔ branch points, behind env flag `RUSTDL_SAT_LOOKAHEAD` (default OFF).

---

#### Files modified

- `crates/owl-dl-tableau/Cargo.toml` — added `owl-dl-saturation.workspace = true` dep
- `crates/owl-dl-tableau/src/hyper.rs` — field + builder + counters + drop logic
- `crates/owl-dl-tableau/tests/sat_lookahead_drop.rs` — new integration test
- `crates/owl-dl-reasoner/src/lib.rs` — env reader, `HyperCache` + `ConsistencyCache` wiring

---

#### hyper.rs changes

**`SearchStats` counters added** (after `block_compares`):
```rust
pub lookahead_calls: u64,
pub lookahead_dropped: u64,
pub lookahead_forced_single: u64,
```

**`HyperEngine` field** (after `div_checkpoint`):
```rust
sat_lookahead: Option<std::sync::Arc<owl_dl_saturation::seed_sat::SeedSaturator>>,
```
Initialised to `None` in all 3 constructors (`new`, `new_with_prebuilt`, `new_seeded`).

**Builder method** `with_sat_lookahead(self, Arc<SeedSaturator>) -> Self` follows the
`with_mrv_ordering` pattern exactly.

**`lookahead_live_disjuncts` helper** — private method called before the ⊔ branch loop
when `sat_lookahead.is_some()`. Builds an atomic seed from the resolved node's current
labels and an existential seed from its non-inverse outgoing edges, then for each
disjunct `Dₖ` calls `sat.seed_unsat(&atomic_k, &exists_k)`. Live survivors drive the
branch loop; if `0` survivors → treat as clash (return `Unsat`); if `1` survivor →
bump `lookahead_forced_single`.

**⊔ branch loop** — gated on a `live: Vec<usize>` (indices into the head). Flag-OFF
path uses `(0..head_len).collect()` (byte-identical to pre-implementation).

---

#### Fixture design

Fixture uses functional-role-merge reasoning to create a disjunct that the clausifier
cannot pre-derive but the EL saturator (Phase 2a) can kill:

```
C ⊑ ∃R.A
B1 ⊑ ∃R.D
C ⊑ (B1 ⊔ B2)
DisjointClasses(A, D)
FunctionalObjectProperty(R)
```

At the ⊔ for `C`: branch B1 adds `∃R.D` alongside `C`'s existing `∃R.A`; with R
functional the saturator merges the two successors → `A ⊓ D` → clash (told-disjoint).
`seed_unsat({C,B1},{(R,A),(R,D)}) = true`. Branch B2 is clean. The clausifier does NOT
derive `C ⊑ B2` directly (no known told-subsumer bridge); the disjunction remains open
in the Horn fixpoint, exercising the lookahead gate.

---

#### Test output

```
test lookahead_drops_dead_disjunct_off_branches_it ... ok
stats_off: branches=2
stats_on: branches=1, lookahead_calls=2, lookahead_dropped=1, lookahead_forced_single=1
```

---

#### reasoner/src/lib.rs wiring

Added `hyper_sat_lookahead_enabled()` (default OFF). At every wedge-construction site
that calls `with_mrv_ordering`, added a symmetric `with_sat_lookahead` call when the
flag is on. Wired to 4 sites:
- `HyperCache::decide_with_stats`
- `HyperCache::sat_only_with_stats`
- `HyperCache::classify_labels`
- `ConsistencyCache::decide`

The `SeedSaturator` is built **once per `HyperCache`/`ConsistencyCache` construction**
(not per pair) via `Arc`; the per-pair wedge calls clone the `Arc`.

Standalone `hyper_subsumption_probe` function also wired (builds the saturator once
before the pair loop).

The `wine_wedge_construct_vs_solve_probe` `#[ignore]`d test-internal engine also wired
(uses the already-built `cache.sat_lookahead`).

---

#### fmt + clippy

`cargo fmt --all -- --check`: clean
`cargo clippy --workspace --all-targets --all-features -- -D warnings`: clean

Fixed 3 clippy issues: unnecessary raw-string hashes on `FIXTURE_SRC`, two
`doc_markdown` backtick complaints in the test module doc, and one unused
`sat_lookahead_for_test` method (removed — not referenced by the integration test).

---

#### Full suite results

`cargo test -p owl-dl-tableau`: all 128 tests pass, 0 failed
`cargo test -p owl-dl-reasoner`: all tests pass (14 pass + 1 ignored), 0 failed

---

#### Soundness

The drop is sound by under-approximation: `seed_unsat` can only return `true` when the
added atoms (the disjunct body) provably entail unsatisfiability — the saturator is
conservative. A false `true` return would be unsound (FP), but Task 1's
`SeedSaturator` is restricted to the EL fragment where it is complete, so no phantom
kills. A false `false` return (the typical case) merely skips the optimisation — a
MISS at worst, never an FP.

Flag-OFF path is structurally identical to the pre-implementation state.

---

#### Concerns

None.
