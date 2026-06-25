# SP-0 Shadow Precise-Dependency Probe — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Measure, read-only, whether wine's dense clash-dependency chains (bjgap≈1) are an artifact of imprecise merge-causation tracking (→ GO build the per-fact dep graph) or genuine semantic structure (→ NO-GO), deciding the deep nominal rearch's mechanism before building it.

**Architecture:** Add a *shadow* precise-dependency layer to the wedge (`hyper.rs`) that recovers the merge causation the live engine discards (`at_most_tainted`/`nn_tainted` → `DepSet::ALL`). The shadow is computed alongside the real search but **never consulted for any decision or verdict** — so flag-off (and the live decision path with the flag on) is byte-identical. At each clash, record `(clash_deps_real, clash_deps_shadow)`; from those compute bjgap real-vs-shadow, reusable-nogood fraction, and revisited-state context-sharing. A controller-run harness runs it on wine's hard classes and writes the GO/NO-GO verdict.

**Tech Stack:** Rust (edition 2024), `owl-dl-tableau` (HyperEngine wedge), `fixedbitset`/`u128` `DepSet`, the `sat_class_probe`/`decide_pair_probe` public probe APIs + `SearchStats`.

## Global Constraints

- **Read-only:** the shadow layer must NEVER influence a search decision, branch choice, merge, or verdict. The flag-off path AND the live-decision path with the flag on are byte-identical to the integration branch base. This is the soundness guarantee (no closure-diff gate needed — but confirm wine closure unchanged flag-on and flag-off).
- `RUSTDL_SHADOW_DEP_PROBE` default OFF; reader idiom `std::env::var_os("RUSTDL_SHADOW_DEP_PROBE").is_some_and(|v| v != "0" && !v.is_empty())`.
- Spike: only the verdict doc is durable; shadow code stays on `feat/nominal-rearch-sp0` (off `feat/build-once-redesign`), unmerged. The shadow layer is the rearch foundation, retained for GO.
- `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets --all-features -- -D warnings` (pedantic) clean; `cargo test --workspace` green (flag-off).
- Toolchain: `export RUSTUP_HOME=/home/dumontier/.rustup CARGO_HOME=/home/dumontier/.cargo; export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$CARGO_HOME/bin:$PATH"`.
- Commit only when the controller says so; trailers on every commit:
  `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`
  `Claude-Session: https://claude.ai/code/session_01HSzon7V2wkhrudxBNAJduh`.

## Key grounding (read before editing `hyper.rs`)

- `DepSet` (`hyper.rs` ~70): `pub(crate) struct DepSet { bits: u128 }` with `EMPTY`, `ALL`, `singleton(level)`, `union`, `insert`, `remove`. It is a **bitset of decision levels**. Wine `max_branch_depth ≈ 30 ≪ 128`, so precise sets never overflow to `ALL` from capacity.
- Per-node (`HNode`) dep fields: `label_deps: Vec<DepSet>` (parallel to `labels`), `at_most_dep`, `at_most_tainted: bool`, `birth_deps`, `nn_tainted: bool`. The two `*_tainted` flags are the imprecision sources — they force `card_clash_deps`/merge to return `DepSet::ALL`.
- `card_clash_deps(parent, succs) -> DepSet` (~890): when `precise_card_deps` and NOT tainted/neq/foreign-succ, it already computes the precise over-set `at_most_dep ∪ ⋃(birth_deps ∪ label_deps)`. The shadow computes this **always**, replacing the taint→`ALL` short-circuit with the recovered merge causation.
- `merge_with_cause` (~2121) and the `≤n` `merge` (~2103) / NN-merge (~2735): where merge causation is joined (or dropped to `EMPTY`/tainted). Trace these to find every site that sets `at_most_tainted`/`nn_tainted`.
- The ⊔ backjump: in `solve` (~1788+), a clash returns `HyperResult::Unsat` carrying `self.clash_deps`; the branch loop backjumps using `clash_deps.contains(decision_level)`. This is where `bjgap` is observable.
- Probe entry points (public): `owl_dl_reasoner::sat_class_probe(&ont, iri, depth, timeout)` and `decide_pair_probe(&ont, sub, sup, depth, timeout)` return `(HyperResult, SearchStats, wall_ms)`.

---

### Task 0: Branch setup

**Files:** none (git only).

- [ ] **Step 1: Create the branch**

```sh
git checkout feat/build-once-redesign
git checkout -b feat/nominal-rearch-sp0
git log --oneline -1   # expect 9d0b018 (SP-0 spec) at/near HEAD
```
No commit.

---

### Task 1: Shadow precise-dependency layer + per-clash recording

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` (shadow per-node dep fields; shadow merge-causation recording at the merge sites; `card_clash_deps_shadow`; the `shadow_dep_probe: bool` flag + builder; per-clash `(real, shadow)` recording into a probe accumulator on `SearchStats`).
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (env reader `hyper_shadow_dep_probe_enabled()`; pass `.with_shadow_dep_probe(...)` at the `sat_class_probe`/`decide_pair_probe` wedge-construction sites).
- Test: `crates/owl-dl-tableau/tests/shadow_dep_probe.rs` (new).

**Interfaces:**
- Consumes: `DepSet` (+ a new `DepSet::highest_level`/`count`/`iter_levels`), the per-node dep fields, `card_clash_deps`, the clash/backjump site in `solve`.
- Produces:
  - `DepSet::highest_level(self) -> Option<u32>` (highest set decision level, `None` if `EMPTY`; for `ALL` return `Some(127)`), `DepSet::count(self) -> u32` (popcount), `DepSet::iter_levels(self) -> impl Iterator<Item = u32>`.
  - `HyperEngine` field `shadow_dep_probe: bool` (default `false` in all 3 constructors) + `#[must_use] pub fn with_shadow_dep_probe(mut self, on: bool) -> Self`.
  - Per-node shadow fields: `shadow_label_deps: Vec<DepSet>` (parallel to `labels`), `shadow_at_most_dep: DepSet`, `shadow_birth_deps: DepSet`, `shadow_merge_cause: DepSet` (precise causation of all merges this node absorbed — the thing the taint discards).
  - `SearchStats` field `clash_records: Vec<ClashRecord>` where
    `pub struct ClashRecord { pub branch_depth: u32, pub real: DepSetSnapshot, pub shadow: DepSetSnapshot, pub clash_label_key: u64 }`, and
    `pub struct DepSetSnapshot { pub highest: Option<u32>, pub count: u32, pub levels: Vec<u32> }`.
  - `reasoner::hyper_shadow_dep_probe_enabled() -> bool`.

- [ ] **Step 1: Add `DepSet` accessors**

Add to `impl DepSet` (next to `union`/`insert`):

```rust
/// Highest decision level present, or `None` if empty. `ALL` ⇒ `Some(127)`.
pub(crate) fn highest_level(self) -> Option<u32> {
    if self.bits == 0 { None } else { Some(127 - self.bits.leading_zeros()) }
}
/// Number of decision levels present.
pub(crate) fn count(self) -> u32 { self.bits.count_ones() }
/// Iterate the decision levels present, ascending.
pub(crate) fn iter_levels(self) -> impl Iterator<Item = u32> {
    (0..128u32).filter(move |&i| self.bits & (1u128 << i) != 0)
}
```
(Confirm the field is named `bits: u128`; adjust if different.)

- [ ] **Step 2: Write the failing flag-off-identity + flag-on-records test**

```rust
// crates/owl-dl-tableau/tests/shadow_dep_probe.rs
// (1) flag-off: search verdict + branches identical to baseline on a small
//     nominal fixture; clash_records is empty.
// (2) flag-on: same verdict + same branches_taken (read-only invariant), but
//     clash_records is non-empty (the probe recorded something).
#[test]
fn shadow_probe_is_read_only_and_records() {
    let (internal, c_id) = build_small_nominal_clash_fixture(); // ≤n + nominal merge → clash
    let off = run_probe(&internal, c_id, /*probe=*/ false);
    let on  = run_probe(&internal, c_id, /*probe=*/ true);
    assert_eq!(on.verdict, off.verdict, "read-only: verdict invariant");
    assert_eq!(on.stats.branches_taken, off.stats.branches_taken, "read-only: branches invariant");
    assert_eq!(on.stats.restores, off.stats.restores, "read-only: restores invariant");
    assert!(off.stats.clash_records.is_empty(), "flag-off records nothing");
    assert!(!on.stats.clash_records.is_empty(), "flag-on records clashes");
}
```
`run_probe`/`build_small_nominal_clash_fixture` are thin wrappers over the in-crate sat-probe harness (`grep -rn "fn sat_class_probe\|_probe\|struct SearchStats" crates/owl-dl-tableau`); `run_probe` constructs the engine with/without `.with_shadow_dep_probe(true)` and returns `{verdict, stats}`. The fixture is a minimal `≤1` role over two nominal-typed successors that must merge then clash (mirror the existing nominal/≤n canaries in `crates/owl-dl-tableau/tests/`).

- [ ] **Step 3: Run it — confirm fail**

```sh
cargo test -p owl-dl-tableau --test shadow_dep_probe
```
Expected: FAIL (field/method/flag not defined).

- [ ] **Step 4: Add the flag, builder, shadow fields, and probe accumulator**

Add `shadow_dep_probe: bool` to `HyperEngine`, default `false` in `new`/`new_with_prebuilt`/`new_seeded`. Add `with_shadow_dep_probe`. Add the four shadow per-node fields to `HNode`, initialised to `DepSet::EMPTY` wherever the real `label_deps`/`birth_deps`/`at_most_dep` are initialised (keep them parallel — push `DepSet::EMPTY` to `shadow_label_deps` wherever a label is pushed to `labels`/`label_deps`). Add `clash_records: Vec<ClashRecord>` + the two `pub struct`s to `SearchStats` (init empty). **Guard every shadow write and the recording with `if self.shadow_dep_probe`** so flag-off does zero work and stays byte-identical. Build.

- [ ] **Step 5: Thread precise merge causation into the shadow fields**

At each site that sets `at_most_tainted = true` or `nn_tainted = true` (trace `merge_with_cause` ~2121, `merge` ~2103, NN-merge ~2735), under `if self.shadow_dep_probe`, instead of discarding the causation, union the **precise decision-level cause of that merge** into the surviving node's `shadow_merge_cause` and into the relevant `shadow_at_most_dep`/`shadow_label_deps`. The precise cause is the decision level(s) the live code already has in scope at the merge (the `cause`/`decision_deps` argument the real merge folds or drops). Mirror the real `label_deps`/`birth_deps`/`at_most_dep` propagation into the `shadow_*` counterparts at the ordinary (non-tainting) derivation sites too, so the shadow set is a complete precise mirror — differing from the real set ONLY in that it never collapses to `ALL`.

Add a shadow twin of the clash-dep computation:

```rust
/// Shadow of `card_clash_deps`: the precise over-set computed ALWAYS — it
/// recovers the merge causation that `at_most_tainted` discards in the real
/// path. Read-only; only used to populate ClashRecord.
fn card_clash_deps_shadow(&self, parent: HNode, succs: &[HNode]) -> DepSet {
    let p = self.resolve(parent);
    let mut over = self.nodes[p.index()].shadow_at_most_dep
        .union(self.nodes[p.index()].shadow_merge_cause);
    for node in std::iter::once(p).chain(succs.iter().copied()) {
        let hn = &self.nodes[self.resolve(node).index()];
        over = over.union(hn.shadow_birth_deps).union(hn.shadow_merge_cause);
        for &ld in &hn.shadow_label_deps { over = over.union(ld); }
    }
    over
}
```

- [ ] **Step 6: Record `(real, shadow)` at every clash**

At each site that sets `self.clash_deps` and returns `HyperResult::Unsat` (the `≤n` card clash via `card_clash_deps`, the label/disjoint clash, the NN clash), under `if self.shadow_dep_probe`, push a `ClashRecord` capturing: `branch_depth` (the current decision depth — derive from the same counter feeding `max_branch_depth`), `real` = snapshot of the just-computed real `clash_deps`, `shadow` = snapshot of the shadow twin (`card_clash_deps_shadow` for the card site; the union of the two clashing labels' `shadow_label_deps` for the label/disjoint site), and `clash_label_key` = a stable hash of the resolved clashing node's label-set (for the reusability/revisit measures in Task 2). A `DepSetSnapshot` is `{ highest: d.highest_level(), count: d.count(), levels: d.iter_levels().collect() }`.

- [ ] **Step 7: Run the test — confirm pass**

```sh
cargo test -p owl-dl-tableau --test shadow_dep_probe
```
Expected: PASS (verdict/branches/restores invariant flag-on vs off; records empty off, non-empty on).

- [ ] **Step 8: Wire the env flag (reasoner)**

In `crates/owl-dl-reasoner/src/lib.rs` add `hyper_shadow_dep_probe_enabled()` (default OFF idiom). At the `sat_class_probe` and `decide_pair_probe` wedge-construction sites, when enabled, call `.with_shadow_dep_probe(true)`. (These probe paths are the only callers that need it; do NOT wire it into the classify per-pair loop.)

- [ ] **Step 9: Flag-off byte-identical + clippy**

```sh
cargo test -p owl-dl-tableau
cargo test -p owl-dl-reasoner --lib
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
```
Expected: green with the flag unset.

- [ ] **Step 10: Commit**

```sh
git add crates/owl-dl-tableau crates/owl-dl-reasoner
git commit -m "feat(sp0): read-only shadow precise-dep layer + per-clash recording

<trailers>"
```

---

### Task 2: The three measures (read-only, from `clash_records`)

**Files:**
- Create: `crates/owl-dl-tableau/src/shadow_measures.rs` (pure functions over `&[ClashRecord]`; `pub mod shadow_measures;` in `hyper.rs` or `lib.rs`).
- Test: inline `#[cfg(test)]` in `shadow_measures.rs`.

**Interfaces:**
- Consumes: `ClashRecord`, `DepSetSnapshot` (Task 1).
- Produces:
  - `pub struct ShadowReport { pub n_clashes: usize, pub bjgap_real: Histogram, pub bjgap_shadow: Histogram, pub reusable_nogood_frac: f64, pub distinct_nogoods: usize, pub revisit_frac: f64, pub revisit_context_shared_frac: f64 }`.
  - `pub fn analyze(records: &[ClashRecord]) -> ShadowReport`.
  - `pub struct Histogram { pub min: u32, pub median: u32, pub p90: u32, pub max: u32, pub mean: f64 }` + `fn from_samples(&[u32]) -> Histogram`.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    fn rec(depth: u32, real_hi: Option<u32>, shadow_hi: Option<u32>, key: u64) -> ClashRecord {
        ClashRecord {
            branch_depth: depth,
            real: DepSetSnapshot { highest: real_hi, count: real_hi.map_or(0,|_|1), levels: real_hi.into_iter().collect() },
            shadow: DepSetSnapshot { highest: shadow_hi, count: shadow_hi.map_or(0,|_|1), levels: shadow_hi.into_iter().collect() },
            clash_label_key: key,
        }
    }
    #[test]
    fn bjgap_and_reuse_are_computed() {
        // Two clashes at depth 10: real highest=10 (bjgap 1 = useless),
        // shadow highest=2 (bjgap 9 = precise backjump). Same nogood key twice → reusable.
        let recs = vec![rec(10, Some(10), Some(2), 7), rec(10, Some(10), Some(2), 7)];
        let r = analyze(&recs);
        assert_eq!(r.n_clashes, 2);
        assert_eq!(r.bjgap_real.median, 1);     // 10 - 10 + 1
        assert_eq!(r.bjgap_shadow.median, 9);    // 10 - 2 + 1
        assert!(r.reusable_nogood_frac > 0.0);   // key 7 recurs
        assert_eq!(r.distinct_nogoods, 1);
    }
}
```

- [ ] **Step 2: Run — confirm fail.** `cargo test -p owl-dl-tableau shadow_measures` → FAIL (module absent).

- [ ] **Step 3: Implement `analyze`**

```rust
//! Read-only measures over the shadow-dep probe's clash records.
use crate::hyper::{ClashRecord, DepSetSnapshot};
use std::collections::HashMap;

pub struct Histogram { pub min: u32, pub median: u32, pub p90: u32, pub max: u32, pub mean: f64 }
impl Histogram {
    fn from_samples(xs: &[u32]) -> Histogram {
        if xs.is_empty() { return Histogram { min:0, median:0, p90:0, max:0, mean:0.0 }; }
        let mut v = xs.to_vec(); v.sort_unstable();
        let pick = |q: f64| v[((v.len() as f64 - 1.0) * q).round() as usize];
        Histogram {
            min: v[0], median: pick(0.5), p90: pick(0.9), max: *v.last().unwrap(),
            mean: xs.iter().map(|&x| f64::from(x)).sum::<f64>() / xs.len() as f64,
        }
    }
}
pub struct ShadowReport {
    pub n_clashes: usize,
    pub bjgap_real: Histogram, pub bjgap_shadow: Histogram,
    pub reusable_nogood_frac: f64, pub distinct_nogoods: usize,
    pub revisit_frac: f64, pub revisit_context_shared_frac: f64,
}
// bjgap = branch_depth - highest + 1 (levels skipped; 1 = useless). No highest
// (EMPTY deps) ⇒ the clash is context-free ⇒ gap = branch_depth (jump to root).
fn bjgap(depth: u32, snap: &DepSetSnapshot) -> u32 {
    match snap.highest { Some(h) => depth.saturating_sub(h).saturating_add(1), None => depth.saturating_add(1) }
}
pub fn analyze(records: &[ClashRecord]) -> ShadowReport {
    let real: Vec<u32>   = records.iter().map(|r| bjgap(r.branch_depth, &r.real)).collect();
    let shadow: Vec<u32> = records.iter().map(|r| bjgap(r.branch_depth, &r.shadow)).collect();
    // reusable NOGOOD = the precise shadow dep-SET (the actual nogood) recurs across
    // ≥2 records. Keyed on the shadow levels, NOT the state — this is the
    // caching/CDCL signal (a context-independent nogood reusable across branches).
    let nogood_key = |r: &ClashRecord| -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        use std::hash::{Hash, Hasher};
        r.shadow.levels.hash(&mut h);
        h.finish()
    };
    let mut ngc: HashMap<u64, usize> = HashMap::new();
    for r in records { *ngc.entry(nogood_key(r)).or_default() += 1; }
    let distinct_nogoods = ngc.len();
    let reusable: usize = ngc.values().filter(|&&c| c >= 2).map(|&c| c).sum();
    let reusable_nogood_frac = if records.is_empty() {0.0} else { reusable as f64 / records.len() as f64 };
    // revisited STATE = the clash node's label-set (clash_label_key) recurs. Distinct
    // from nogood reuse: a state can recur under different nominal contexts.
    let mut counts: HashMap<u64, usize> = HashMap::new();
    for r in records { *counts.entry(r.clash_label_key).or_default() += 1; }
    let revisited: usize = counts.values().filter(|&&c| c >= 2).map(|&c| c).sum();
    let revisit_frac = if records.is_empty() {0.0} else { revisited as f64 / records.len() as f64 };
    // context-sharing: of revisited keys, fraction whose shadow dep-set highest matches
    // across occurrences (same nominal context ⇒ cacheable; differing ⇒ reuse-trap).
    let mut by_key: HashMap<u64, Vec<Option<u32>>> = HashMap::new();
    for r in records { by_key.entry(r.clash_label_key).or_default().push(r.shadow.highest); }
    let (mut shared, mut total) = (0usize, 0usize);
    for (_k, hs) in by_key.iter().filter(|(_,h)| h.len() >= 2) {
        total += hs.len();
        let first = hs[0];
        shared += hs.iter().filter(|&&h| h == first).count();
    }
    let revisit_context_shared_frac = if total == 0 {0.0} else { shared as f64 / total as f64 };
    ShadowReport {
        n_clashes: records.len(),
        bjgap_real: Histogram::from_samples(&real),
        bjgap_shadow: Histogram::from_samples(&shadow),
        reusable_nogood_frac, distinct_nogoods, revisit_frac, revisit_context_shared_frac,
    }
}
```
Make `ClashRecord`/`DepSetSnapshot` fields `pub` and the structs reachable (re-export from `hyper` if needed).

- [ ] **Step 4: Run — confirm pass.** `cargo test -p owl-dl-tableau shadow_measures` → PASS. Then `cargo fmt`/`clippy` for the crate.

- [ ] **Step 5: Commit**

```sh
git add crates/owl-dl-tableau/src/shadow_measures.rs crates/owl-dl-tableau/src/hyper.rs
git commit -m "feat(sp0): bjgap/reuse/revisit measures over clash records

<trailers>"
```

---

### Task 3: Gate harness, measurement, verdict (controller-run)

**Files:**
- Create: `crates/owl-dl-reasoner/tests/shadow_dep_gate.rs` (`#[ignore]`d probe over wine hard classes; prints the `ShadowReport`).
- Create (durable): `docs/nominal-rearch-sp0-shadow-dep-probe-results-2026-06-25.md`.

**Interfaces:**
- Consumes: `sat_class_probe`/`decide_pair_probe` (which now carry `clash_records` in `SearchStats` when the flag is on); `owl_dl_tableau::shadow_measures::analyze`; `RUSTDL_SHADOW_DEP_PROBE`.

- [ ] **Step 1: Harness over wine hard classes**

Mirror `crates/owl-dl-reasoner/tests/sat_lookahead_gate.rs` (absolute wine path `/data/dumontier/rustdl/ontologies/real/wine.ofn`, IRI prefix `http://www.w3.org/TR/2003/PR-owl-guide-20031209/wine#`, big-stack thread, depth 256, 60s timeout). For each of `sat(SweetWine)`, `sat(AlsatianWine ⊓ ¬AmericanWine)`, and ≥3 more hard wine classes (pick from the ~19 — e.g. `Zinfandel`, `WhiteNonSweetWine`, `RedTableWine`; verify each is a real class IRI first), call the probe with the flag set, take `stats.clash_records`, run `analyze`, and print the `ShadowReport` (n_clashes, bjgap_real vs bjgap_shadow histograms, reusable_nogood_frac, revisit fracs).

```sh
RUSTDL_ADAPTIVE_BUDGET=0 RUSTDL_SHADOW_DEP_PROBE=1 cargo test -p owl-dl-reasoner --release \
  --test shadow_dep_gate -- --ignored --nocapture
```

- [ ] **Step 2: Flag-off byte-identical wine closure spot-check**

```sh
RUSTDL_TEST_PAIR_MS=25 cargo test -p owl-dl-reasoner --release \
  --test konclude_closure_diff wine_closure_matches_konclude -- --ignored --nocapture
RUSTDL_TEST_PAIR_MS=25 RUSTDL_SHADOW_DEP_PROBE=1 cargo test -p owl-dl-reasoner --release \
  --test konclude_closure_diff wine_closure_matches_konclude -- --ignored --nocapture
```
Expected: BOTH `rustdl_closure=653 konclude=653 FP=0 MISSED=0 unsat:rustdl=0` — the probe is read-only, so flag-on must equal flag-off (this is the read-only proof at corpus scale; unlike SP-A, unsat must stay 0).

- [ ] **Step 3: Interpret + write the verdict**

Create `docs/nominal-rearch-sp0-shadow-dep-probe-results-2026-06-25.md` with the per-class `ShadowReport` table, the flag-off-identity confirmation, and the **VERDICT**:
- **GO** iff a regime change: `bjgap_shadow` shifts off 1 (median ≥ 3, or a substantial fraction with `bjgap_shadow ≥ 5`) **and** `reusable_nogood_frac` is non-trivial (≫ the ~0 baseline). Consequence: build the CMERGED* per-fact dep graph (the shadow layer becomes real); `revisit_context_shared_frac` names whether state-memoization is FP-safe (shared) or the reuse-trap (differing).
- **NO-GO** iff `bjgap_shadow` stays ≈1 / `reusable_nogood_frac ≈ 0`. Consequence: proven mechanism-floor — the per-fact-dep graph is the wrong mechanism; present the user's remaining fork (fully-integrated engine reimplemented vs stop), with the numbers as evidence.

- [ ] **Step 4: Commit verdict (+ harness)**

```sh
git add docs/nominal-rearch-sp0-shadow-dep-probe-results-2026-06-25.md \
        crates/owl-dl-reasoner/tests/shadow_dep_gate.rs
git commit -m "docs(sp0): shadow precise-dep probe verdict (<GO/NO-GO>)

<trailers>"
```

- [ ] **Step 5: Report to controller**

Surface the verdict + the bjgap/reuse numbers. Do NOT merge to `main`. On GO: next step is the CMERGED* foundation spec. On NO-GO: present the user's fork with the evidence.

---

## Notes for the implementer

- The single most important invariant is **read-only**: if any shadow write or recording can change `branches_taken`/`restores`/verdict, it is a bug. The Task-1 test asserts this on a fixture; the Task-3 wine closure spot-check asserts it at scale. If flag-on ≠ flag-off, stop and fix before measuring.
- Do not try to compute a true minimal (MUS) per clash — the shadow precise dep is the *per-fact provenance* set (what the CMERGED* graph would carry), which is the realistic measure of what the rearch achieves. That is `card_clash_deps_shadow` / the clashing labels' `shadow_label_deps`, never collapsed to `ALL`.
- If tracing the merge sites is ambiguous, the anchor is: wherever the real code sets `at_most_tainted`/`nn_tainted` or returns `DepSet::ALL` for merge causation, the shadow must instead union the in-scope decision-level `cause`. Everything else mirrors the real propagation 1:1.
- `bjgap` uses the decision depth at the clash. If the wedge tracks decision depth as `init_depth - depth + 1` (see the adaptive-budget comment ~1803), record the *level* consistently so `bjgap = level_at_clash - highest_in_deps + 1`.
