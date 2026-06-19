# Within-search state caching (Lever #2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Memoize the wedge's `solve` verdict per **full graph-state** within a single search, so the disjunctive search stops re-exploring the ~48 states it currently revisits ~278k times — collapsing ore-15672/wine (and maybe making `family` terminate) with **byte-identical verdicts**.

**Architecture:** A per-search `HashMap<u64, CachedVerdict>` keyed on a canonical full-state hash. Lookup at the post-`horn_fixpoint`-`Sat` quiescent point in `solve`; on a cached **decisive** verdict (Unsat/Sat) return it (Unsat ⇒ conservative `clash_deps = DepSet::ALL`). Insert the branching phase's decisive verdict under the same key. **Sound by construction** (whole-graph save/restore ⇒ `solve` deterministic in the full state); the only obligation is **key-completeness**.

**Tech Stack:** Rust; `owl-dl-tableau/src/hyper.rs` (engine + `solve` + the key); `owl-dl-reasoner/src/lib.rs` (wedge wiring); `konclude_closure_diff` corpus net.

**Spec:** `docs/superpowers/specs/2026-06-19-within-search-caching-design.md`

**Soundness law:** FP=0. Soundness reduces ENTIRELY to **key-completeness** — the key must hash every field `solve`/`horn_fixpoint`/`apply_head_atom` read that affects the verdict; a missed field = a wrong hit = FP. **Task 3 is a mandatory adversarial key-completeness review + the corpus closure-IDENTITY net (byte-identical closures, not just FP=0).** Cache only decisive verdicts (never `Stalled`); never cache the dep-set. Exclude dep-sets (`label_deps`/`at_most_dep`) from the key — they're backjumping bookkeeping, not verdict-determining.

---

## Conventions
- Toolchain: `export PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"`.
- Build: `cargo build --release -p owl-dl-cli`.
- Branch is `feat/within-search-caching` (spec committed `ca96759`). Do NOT touch main.

---

## Task 1: Full-state key + per-search cache + lookup/insert (opt-in)

**Files:** Modify `crates/owl-dl-tableau/src/hyper.rs`; test in its `#[cfg(test)]`.

- [ ] **Step 1: `CachedVerdict` + engine fields + opt-in**

```rust
/// A DECISIVE wedge verdict memoized per full graph-state. Never `Stalled`
/// (depth-relative). `Unsat`/`Sat` are absolute, so valid at any depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CachedVerdict { Unsat, Sat }
```
Add to `struct HyperEngine`: `wedge_cache: bool,` and
`cache: std::collections::HashMap<u64, CachedVerdict>,`. Initialize
`wedge_cache: false, cache: std::collections::HashMap::new(),` in EVERY constructor
(grep the `Self {` literals — `new`, `new_with_prebuilt`, `new_seeded`, `from_snapshot*`).
In `decide_with_deadline`, clear it alongside the stats reset: add `self.cache.clear();`
next to `self.stats = SearchStats::default();`. Add:
```rust
/// Opt into within-search state memoization (Lever #2). Off by default.
#[must_use]
pub fn with_wedge_cache(mut self) -> Self {
    self.wedge_cache = true;
    self
}
```

- [ ] **Step 2: `full_state_key` (the soundness-critical hash)**

Add a method computing a canonical, order-independent, union-find-resolved hash of the
verdict-determining state. Use `std::hash::{Hash, Hasher}` with `DefaultHasher`; combine
per-node hashes order-INDEPENDENTLY (sum into a `u64` so node ordering doesn't matter):
```rust
fn full_state_key(&self) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut acc: u64 = 0;
    for i in 0..self.nodes.len() {
        let rep = self.resolve(HNode(i as u32));
        if rep.index() as usize != i {
            continue; // only canonical representatives contribute
        }
        let node = &self.nodes[i];
        let mut h = std::collections::hash_map::DefaultHasher::new();
        // labels (already sorted-insert? — sort a copy to be safe)
        let mut labels = node.labels.clone();
        labels.sort_unstable();
        labels.hash(&mut h);
        // edges/preds: resolve endpoints through union-find, sort, hash
        let mut edges: Vec<(Role, u32)> =
            node.edges.iter().map(|&(r, t)| (r, self.resolve(t).index())).collect();
        edges.sort_unstable();
        edges.hash(&mut h);
        let mut preds: Vec<(Role, u32)> =
            node.preds.iter().map(|&(r, s)| (r, self.resolve(s).index())).collect();
        preds.sort_unstable();
        preds.hash(&mut h);
        // open ≤n obligations on this node
        let mut am = node.at_most.clone();
        am.sort_unstable();
        am.hash(&mut h);
        // fold node-key into acc order-independently, salted by the rep id so
        // two structurally-identical nodes at different reps still both count
        acc = acc.wrapping_add(h.finish() ^ (i as u64).wrapping_mul(0x9E3779B97F4A7C15));
    }
    // engine-level state
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let mut neq: Vec<(u32, u32)> = self
        .neq
        .iter()
        .map(|&(a, b)| {
            let (x, y) = (self.resolve(a).index(), self.resolve(b).index());
            if x <= y { (x, y) } else { (y, x) }
        })
        .collect();
    neq.sort_unstable();
    neq.dedup();
    neq.hash(&mut h);
    self.nominals.hash(&mut h);
    self.snapshot_backprop_aborted.hash(&mut h);
    acc.wrapping_add(h.finish())
}
```
NOTE on the rep-salt: salting each node-hash by its representative id `i` means two
structurally-identical nodes don't cancel under the order-independent sum, AND the key is
stable across re-derivations that land the same structure on the same reps (which is what
the probe measured — 48 stable states). If a future merge can relabel reps between
equivalent states, the salt could lower the hit rate (a perf cost, never soundness). Start
here; revisit only if the hit rate is poor. (Dep-sets `label_deps`/`at_most_dep` are
DELIBERATELY excluded — they don't affect the Sat/Unsat verdict.)

- [ ] **Step 3: Lookup at the post-`horn_fixpoint`-`Sat` point + insert at decisive returns**

In `solve` (`hyper.rs:1657`-ish), the structure is:
```rust
match self.horn_fixpoint(FIXPOINT_ITERS) {
    HyperResult::Unsat => return HyperResult::Unsat,
    HyperResult::Stalled => return HyperResult::Stalled,
    HyperResult::Sat => {}
}
// <-- HERE: graph is closure-saturated + worklist-quiescent.
```
Immediately after the `match` (graph quiescent), add the lookup + capture the key:
```rust
let cache_key: Option<u64> = if self.wedge_cache {
    let k = self.full_state_key();
    match self.cache.get(&k) {
        Some(CachedVerdict::Unsat) => {
            self.clash_deps = DepSet::ALL; // conservative — sound, forfeits on-hit pruning
            return HyperResult::Unsat;
        }
        Some(CachedVerdict::Sat) => return HyperResult::Sat,
        None => Some(k),
    }
} else {
    None
};
```
Then the branching phase (`find_open_disjunction` / `find_open_at_most` / `solve_at_most`)
runs. Every place it returns a **decisive** verdict, insert under `cache_key` first.
Cleanest: introduce a tiny helper used at each decisive return:
```rust
fn memoize(&mut self, key: Option<u64>, v: HyperResult) -> HyperResult {
    if let (Some(k), HyperResult::Unsat | HyperResult::Sat) = (key, v) {
        let cv = if matches!(v, HyperResult::Unsat) { CachedVerdict::Unsat } else { CachedVerdict::Sat };
        self.cache.insert(k, cv);
    }
    v
}
```
Replace the decisive `return HyperResult::Sat;` / `return HyperResult::Unsat;` returns in
the branching phase of `solve` (the disjunct-Sat early return, the backjump Unsat return,
the disjunction-exhausted `self.clash_deps = combined.remove(d); return HyperResult::Unsat;`,
and the `≤n` Unsat returns) with `return self.memoize(cache_key, HyperResult::Sat);` etc.
Leave `Stalled` returns un-memoized. CAUTION: the `≤n` path returns `self.solve_at_most(...)`
— wrap that too (`return self.memoize(cache_key, self.solve_at_most(...))`). Do NOT memoize
the early pre-branch returns ABOVE the lookup point (e.g. the `forced_distinct_exceeds`
Unsat is before the lookup — leave it; or move the lookup above it — keep it simple: lookup
right after horn_fixpoint, memoize only returns that occur AFTER it).

- [ ] **Step 4: Verdict-identity unit tests (cache ON == OFF)**

In `hyper.rs` tests: build several disjunctive `DlClause` sets (reuse the existing test
construction style, e.g. `horn_chain_derives_transitive_subsumers`), each decided twice —
once `HyperEngine::new(...)`, once `.with_wedge_cache()` — assert verdicts equal. Include
an `Unsat` case, a `Sat` case, a multi-level-branch case, and (important for key-completeness)
a case with inverse edges + a `≤n` + a `neq`/merge.
```rust
#[test]
fn wedge_cache_matches_uncached_verdict() {
    for clauses in cache_fixtures() {
        let v_off = HyperEngine::new(&clauses, root()).decide(256);
        let v_on = HyperEngine::new(&clauses, root()).with_wedge_cache().decide(256);
        assert_eq!(v_off, v_on);
    }
}
```

- [ ] **Step 5: Gates + commit**

Run: `cargo build --release -p owl-dl-cli && cargo test -p owl-dl-tableau` (ALL pass — cache off by default ⇒ existing behavior unchanged; the new test passes ⇒ cache-on matches). Clippy `-p owl-dl-tableau --all-targets -- -D warnings`; fmt.
```sh
git add crates/owl-dl-tableau/src/hyper.rs
git commit -m "feat(wedge): within-search state memoization (Lever #2, flag off) + verdict-identity test

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: Wire into reasoner wedge paths (env-gated) + headline wall smoke

**Files:** Modify `crates/owl-dl-reasoner/src/lib.rs`; test `crates/owl-dl-reasoner/tests/wedge_cache.rs` (new).

- [ ] **Step 1: Flag helper** (mirror `adaptive_budget_enabled`):
```rust
/// Lever #2: within-search state memoization. Default OFF until the key-completeness
/// review + corpus closure-identity gate pass. Set `RUSTDL_WEDGE_CACHE=1`.
pub(crate) fn wedge_cache_enabled() -> bool {
    std::env::var("RUSTDL_WEDGE_CACHE").is_ok_and(|v| v == "1")
}
```

- [ ] **Step 2: Apply `.with_wedge_cache()` (gated) at the wedge sites** — same sites as
adaptive-budget (`HyperCache::decide`, `HyperCache::classify_labels`, `ConsistencyCache`, the
per-class unsat-probe), next to the `if crate::adaptive_budget_enabled()` chain:
```rust
        if crate::wedge_cache_enabled() {
            engine = engine.with_wedge_cache();
        }
```

- [ ] **Step 3: Verdict-identity reasoner test** (`wedge_cache.rs`): classify a small ontology
with a real subsumption, flag OFF vs ON (env-guard pattern from `inverse_symmetric_domain.rs`),
assert identical subsumption sets. (Real diverging-case behavior is the Task-3 corpus gate.)

- [ ] **Step 4: Headline wall smoke (report numbers, don't tune here):**
```sh
echo -n "ore-15672 cache ON: "; RUSTDL_WEDGE_CACHE=1 /usr/bin/time -p ./target/release/rustdl classify ontologies/external/ore-15672-shoin.ofn >/dev/null 2>/tmp/c.t; grep real /tmp/c.t
```
Expected (the prize): ore-15672 ≪ 138s (target seconds) if caching collapses the search. Report it. Also smoke `family` consistency: `RUSTDL_WEDGE_CACHE=1 timeout 60 ./target/release/rustdl consistent ontologies/real/family.ofn` — report whether it now returns a verdict (vs prior stall).

- [ ] **Step 5: Gates + commit** (`cargo test -p owl-dl-reasoner --test wedge_cache --test classify_inverse_domain --test inverse_symmetric_domain` green; clippy; fmt).
```sh
git add crates/owl-dl-reasoner/src/lib.rs crates/owl-dl-reasoner/tests/wedge_cache.rs
git commit -m "feat(wedge): wire within-search cache into reasoner paths (RUSTDL_WEDGE_CACHE, off)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: Key-completeness adversarial review + corpus closure-identity (THE sacred gate)

- [ ] **Step 1: Adversarial key-completeness review.** Dispatch a reviewer (opus) to audit
`full_state_key` against EVERY field `solve`/`horn_fixpoint`/`apply_head_atom`/`apply_head_atom`'s
callees read from `self`. The question: *is there a field X such that two graph states with the
same key but different X produce different verdicts?* Candidates to scrutinize: un-drained
worklist events (is the state truly quiescent at the lookup point — does `horn_fixpoint`
returning `Sat` guarantee the worklist is empty?); `at_most` obligations; nominal range;
`representative` partition (is it fully captured by hashing only reps + resolved endpoints?);
`block_index`/blocking state if it affects `Sat`; any `RUSTDL_*`-gated rule state. Output: either
"complete — here's why each verdict-affecting field is in the key" OR "field X is missing →
FP risk". A missing field MUST be added before Step 2.

- [ ] **Step 2: Corpus closure-IDENTITY net (sacred):**
```sh
RUSTDL_WEDGE_CACHE=1 cargo test --release -p owl-dl-reasoner --test konclude_closure_diff -- --include-ignored --nocapture 2>&1 | grep -iE 'rustdl_closure=|FP=|MISSED=|test result'
```
Expected: FP=0 AND **every closure byte-identical to baseline** (galen 27997, notgalen 32739,
sio 8904, wine 653, ore-10908 6001, ore-15672 142, alehif 247, ro 158, pizza 499, bibtex 16).
**ANY closure change (shrink OR grow) → a key-completeness bug → STOP, fix the key, re-run.**
(`family_inconsistency_detected` failing under `--include-ignored` is the known scale sentinel —
UNLESS caching made family terminate, in which case it may now PASS: record that as a bonus.)

- [ ] **Step 3: Record results** in the plan's Results section: ore-15672/wine/family walls
cache ON vs OFF; closure-identity confirmation; the review's verdict.

---

## Task 4: Cache cost / fast-corpus non-regression + flip default

- [ ] **Step 1: Fast-corpus non-regression:** galen/sio classify walls cache ON vs OFF — must
not regress (the full-state hash per lookup must cost ≪ the saved sub-search; on fast fixtures
the cache rarely hits so the hash is pure overhead — confirm it's negligible). If galen/sio
regress materially, implement incremental key maintenance (update a running hash on each
mutation) OR gate the cache to only engage after a branch-count threshold.

- [ ] **Step 2: Flip default** per the gate: if closures byte-identical + ore-15672 collapses +
no fast-corpus regression, flip `wedge_cache_enabled` to default ON (`map_or(true, v != "0")`).
Re-run the corpus net at the new default. Else keep opt-in + document.

- [ ] **Step 3: Full suite + CLAUDE.md + Results + commit.** `cargo test --workspace`,
`cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --all -- --check`.
CLAUDE.md entry (owl-dl-tableau): within-search memoization, sound-by-construction (whole-graph
determinism), key-completeness reviewed, ore-15672 `<baseline>→<new>`, closures byte-identical;
reference spec + plan. Note if family now terminates.

---

## Results

(Filled during execution: key-completeness review verdict; ore-15672/wine/family walls ON vs OFF; corpus closure-identity confirmation; fast-corpus non-regression; default ON or opt-in; whether family terminated.)
