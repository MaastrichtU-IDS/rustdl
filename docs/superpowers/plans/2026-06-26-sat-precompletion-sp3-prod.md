# SP3 Phase-2 production ∃-seed — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire the probe-validated derived-∃-fact seed into the classify path (label-cache build + per-pair tier walk), alongside SP2's named seed, and prove it sound at corpus scale (FP=0/MISSED=0) with a net wine wall improvement.

**Architecture:** `HyperCache::build` computes an `exists_seed` table once (per-class derived ∃-facts from `saturate_with_exists_facts`, translated to wedge-native targets — reusing one saturation call shared with `sat_seed`); `classify_labels` and `decide_with_stats` seed `Q → ∃R.target` clauses from it, alongside the named `sat_seed`, under `RUSTDL_SAT_SEED` (now named+∃). Sound by construction (monotone entailed ∃-facts); the corpus oracle is the proof.

**Tech Stack:** Rust (edition 2024), `owl-dl-reasoner` (`HyperCache`/`classify_labels`/`decide_with_stats`), `owl-dl-saturation` (`saturate_with_exists_facts`), the `konclude_closure_diff` oracle harness.

## Global Constraints

- FP=0 sacred — the **full-corpus** `konclude_closure_diff` (FP=0 **and** MISSED=0, byte-identical, flag ON) is the gate; **wine is the critical fixture** (653=653, unsat:rustdl=0).
- `RUSTDL_SAT_SEED` default OFF; flag-off path byte-identical to the integration branch base.
- Branch `feat/sat-precompletion-sp3-prod` (already created, off `feat/sat-precompletion-probe`); `main` untouched; no default flip without the gate passing.
- `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets --all-features -- -D warnings` (pedantic) clean; `cargo test --workspace` green (flag-off).
- Toolchain: `export RUSTUP_HOME=/home/dumontier/.rustup CARGO_HOME=/home/dumontier/.cargo; export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$CARGO_HOME/bin:$PATH"`.
- Commit only when the controller says so; trailers:
  `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`
  `Claude-Session: https://claude.ai/code/session_01HSzon7V2wkhrudxBNAJduh`.

## Grounding (read before editing — all in `crates/owl-dl-reasoner/src/lib.rs` unless noted)

- `HyperCache` struct (~1603/1707): already has `sat_seed: Option<Vec<Vec<ClassId>>>` (SP2). `HyperCache::build` (~1727) computes `sat_seed` when `hyper_sat_seed_enabled()` by calling `owl_dl_saturation::saturate(&internal)` — **switch that to `saturate_with_exists_facts`** and build BOTH `sat_seed` and the new `exists_seed` from the one call (no double saturation).
- The translation to reuse verbatim is in `precompletion_probe` (~1094): for `(s, r, tgt)`, named target (`tgt.index() < n_named`) → `(r, tgt)`; NomKey (`nom_to_ind.get(&tgt) = ind`) → `(r, ClassId::new(n_named as u32 + ind.index()))`; else drop.
- `decide_with_stats` (~1764): pushes `Q → sub`, then the `sat_seed` loop (`if let Some(tbl) = &self.sat_seed …`). Add the `exists_seed` loop right after.
- `classify_labels` (~2035): pushes `Q → c`, then the SP2.1 `sat_seed` loop, then builds the engine with `HyperEngine::new` (full rebuild) **when any seed is present** (the SP2.1 index lesson). Add the `exists_seed` loop alongside `sat_seed`, and ensure the "rebuild index when seeded" condition includes `exists_seed.is_some()`.
- `owl_dl_saturation::saturate_with_exists_facts(&internal) -> (Subsumers, Vec<(ClassId,RoleId,ClassId)>, HashMap<ClassId,IndividualId>)` (Phase-1, on this branch's ancestor).
- `Atom::Exists(Role, ClassId, Var)`; construct the role via the same `Role::named(RoleId)` form `precompletion_probe` uses (grep it to confirm the exact constructor).

---

### Task 1: `exists_seed` table + seed in both wedge sites + flag wiring

**Files:**
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (HyperCache `exists_seed` field; build it in `HyperCache::build` from `saturate_with_exists_facts`; seed it in `classify_labels` + `decide_with_stats`; switch the `sat_seed` build to the shared call).
- Test: inline `#[cfg(test)]` in `lib.rs` (mirror the SP2 `sat_seed_wiring_tests` env-guard pattern).

**Interfaces:**
- Consumes: `owl_dl_saturation::saturate_with_exists_facts`, `precompletion_probe`'s translation, `HyperCache`'s existing `sat_seed` build + seed loops, `Atom::Exists`/`Role::named`.
- Produces:
  - `HyperCache` field `exists_seed: Option<Vec<Vec<(owl_dl_tableau::hyper::Role /* or owl_dl_core Role — match decide_with_stats' Atom::Exists role type */, owl_dl_core::ir::ClassId)>>>` indexed by class id.
  - `#[cfg(test)] pub(crate) fn exists_seed_for_test(&self) -> Option<&Vec<Vec<(Role, ClassId)>>>`.
  - Behaviour: with the flag on, `classify_labels(c)` and `decide_with_stats(sub, …)` push `Q → ∃R.target` for each `(R, target)` in `exists_seed[c.index()]` / `exists_seed[sub.index()]`; flag off ⇒ `None` ⇒ no change.

- [ ] **Step 1: Switch the `sat_seed` build to the shared saturation call + build `exists_seed`**

In `HyperCache::build`, replace the existing `sat_seed` block's `owl_dl_saturation::saturate(&internal)` with a single `saturate_with_exists_facts` call, build `sat_seed` from `subs` (unchanged logic) and `exists_seed` from `facts`+`nom_to_ind`:

```rust
let (sat_seed, exists_seed) = if hyper_sat_seed_enabled() {
    use owl_dl_core::ir::ClassId;
    let n_named = internal.vocabulary.num_classes();
    let (subs, facts, nom_to_ind) = owl_dl_saturation::saturate_with_exists_facts(&internal);
    // named seed (unchanged from SP2)
    let mut named: Vec<Vec<ClassId>> = vec![Vec::new(); n_named];
    for ci in 0..n_named {
        let c = ClassId::new(u32::try_from(ci).unwrap_or(u32::MAX));
        named[ci] = subs.subsumers_of(c).into_iter()
            .filter(|&d| d != c && (d.index() as usize) < n_named).collect();
    }
    // ∃ seed: translate derived ∃-facts (named direct; NomKey → wedge nominal; drop else)
    let mut exists: Vec<Vec<(Role, ClassId)>> = vec![Vec::new(); n_named];
    for (s, r, tgt) in facts {
        let si = s.index() as usize;
        if si >= n_named { continue; } // ∃-facts of synthetic subjects not seeded
        let translated = if (tgt.index() as usize) < n_named {
            Some(tgt)
        } else if let Some(&ind) = nom_to_ind.get(&tgt) {
            Some(ClassId::new(n_named as u32 + ind.index()))
        } else { None };
        if let Some(t) = translated {
            exists[si].push((Role::named(r), t));
        }
    }
    (Some(named), Some(exists))
} else {
    (None, None)
};
```
Confirm the `Role` type matches what `Atom::Exists` expects (the same `Role::named(RoleId)` `precompletion_probe` uses). Add `sat_seed,` and `exists_seed,` to the `Self { … }` constructor. (Remove the old standalone `sat_seed` block this replaces.)

- [ ] **Step 2: Add the `exists_seed` field + test accessor to `HyperCache`**

Add `exists_seed: Option<Vec<Vec<(Role, ClassId)>>>` beside `sat_seed`, and:
```rust
#[cfg(test)]
pub(crate) fn exists_seed_for_test(&self) -> Option<&Vec<Vec<(Role, owl_dl_core::ir::ClassId)>>> {
    self.exists_seed.as_ref()
}
```

- [ ] **Step 3: Write the failing wiring test (inline)**

```rust
// Fixture: C ⊑ ∃r.{a}. Flag-off ⇒ exists_seed None. Flag-on ⇒ exists_seed[C]
// contains (r, wedge_nominal) with wedge_nominal = num_classes + a.index().
#[test]
fn exists_seed_table_built_only_when_flagged() {
    let internal = build_c_exists_nominal_a(); // reuse the precompletion translation-test fixture
    let off = build_cache_with_sat_seed_flag(&internal, false);
    assert!(off.exists_seed_for_test().is_none(), "flag off ⇒ no ∃ table");
    let on = build_cache_with_sat_seed_flag(&internal, true);
    let tbl = on.exists_seed_for_test().expect("flag on ⇒ ∃ table");
    let c = class_id(&internal, "C");
    let n_named = internal.vocabulary.num_classes() as u32;
    let a = individual_id(&internal, "a"); // the {a} individual
    let want = (role_named(&internal, "r"), owl_dl_core::ir::ClassId::new(n_named + a.index()));
    assert!(tbl[c.index() as usize].iter().any(|&(ref rr, t)| *rr == want.0 && t == want.1),
            "C seeds ∃r.{{a}} translated to the wedge nominal id");
}
```
Reuse the Phase-1 `build_c_exists_nominal_a` fixture + `class_id`/`individual_id`/`role_named` helpers (grep the precompletion translation test). `build_cache_with_sat_seed_flag` = the SP2 env-guard helper (sets `RUSTDL_SAT_SEED`, calls `HyperCache::build`).

- [ ] **Step 4: Run — confirm fail.** `cargo test -p owl-dl-reasoner exists_seed_table` → FAIL (field/accessor absent).

- [ ] **Step 5: Seed `exists_seed` in `decide_with_stats`**

Right after the existing `sat_seed` loop in `decide_with_stats`:
```rust
if let Some(tbl) = &self.exists_seed
    && let Some(seeds) = tbl.get(sub.index() as usize)
{
    for &(role, target) in seeds {
        clauses.push(DlClause {
            body: vec![Atom::Class(self.fresh_q, X)],
            head: vec![Atom::Exists(role, target, X)],
        });
    }
}
```
(`decide_with_stats` already uses `HyperEngine::new` — full rebuild — so the ∃-clauses are indexed.)

- [ ] **Step 6: Seed `exists_seed` in `classify_labels`**

Right after the SP2.1 `sat_seed` loop in `classify_labels`, the same ∃-loop indexed by `c`. Then ensure the "rebuild index when seeded" branch fires for ∃-seeds too: change the existing `if self.sat_seed.is_some()` engine-construction guard to `if self.sat_seed.is_some() || self.exists_seed.is_some()` (so `HyperEngine::new` is used whenever either seed is present — appended ∃-clauses must be indexed).

- [ ] **Step 7: Run the test — pass.** `cargo test -p owl-dl-reasoner exists_seed_table` → PASS.

- [ ] **Step 8: Flag-off byte-identical + clippy**

```sh
cargo test -p owl-dl-reasoner
cargo test -p owl-dl-tableau
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
```
Expected: green with the flag unset.

- [ ] **Step 9: Commit**

```sh
git add crates/owl-dl-reasoner/src/lib.rs
git commit -m "feat(sat-precompletion): SP3 Phase-2 — production ∃-seed in classify path

<trailers>"
```

---

### Task 2: Corpus FP gate + net wine wall + verdict (controller-run)

**Files:**
- Create (durable): `docs/sat-precompletion-sp3-prod-gate-results-2026-06-26.md`.

**Interfaces:**
- Consumes: `konclude_closure_diff` (`#[ignore]` oracle tests), `RUSTDL_SAT_SEED`, `RUSTDL_TEST_PAIR_MS`, the `rustdl classify` CLI banner (label-cache/tier-walk breakdown).

- [ ] **Step 1: Corpus FP=0/MISSED=0, flag ON, byte-identical**

```sh
cargo build -p owl-dl-reasoner owl-dl-cli --release --tests
RUSTDL_TEST_PAIR_MS=25 RUSTDL_SAT_SEED=1 cargo test -p owl-dl-reasoner --release \
  --test konclude_closure_diff wine_closure_matches_konclude -- --ignored --nocapture   # CRITICAL: 653=653 FP=0 MISSED=0 unsat:0
RUSTDL_TEST_PAIR_MS=25 RUSTDL_SAT_SEED=1 cargo test -p owl-dl-reasoner --release \
  --test konclude_closure_diff -- --ignored --nocapture                                  # full corpus
```
Record each fixture's `rustdl_closure / konclude_closure / FP / MISSED / unsat`. **Require FP=0 AND MISSED=0 on every fixture, unsat counts equal.** Any FP/MISSED ⇒ NO-GO (classify-scale ∃-coupling hole) — record the failing fixture.

- [ ] **Step 2: Flag-off byte-identical baseline** — same wine command without `RUSTDL_SAT_SEED`; confirm identical (653=653/FP=0/MISSED=0/unsat:0).

- [ ] **Step 3: Net wine classify wall, flag ON vs OFF**

```sh
./target/release/rustdl classify --pair-timeout-ms 25 ontologies/real/wine.ofn 2>&1 | grep -E "label heuristic|timed-out|wall breakdown"   # OFF
RUSTDL_SAT_SEED=1 ./target/release/rustdl classify --pair-timeout-ms 25 ontologies/real/wine.ofn 2>&1 | grep -E "label heuristic|timed-out|wall breakdown"  # ON
```
Wrap each with `/usr/bin/time -v` for wall. Record misses, timed-out pairs, label_cache_build, tier_walk, total wall — compare ON vs OFF and vs SP2.1's named-only ~7.5%.

- [ ] **Step 4: Write the verdict doc**

`docs/sat-precompletion-sp3-prod-gate-results-2026-06-26.md`: the per-fixture FP/MISSED table, the flag-off identity, the net wine wall (ON vs OFF vs SP2.1 named-only), and the **VERDICT**:
- **GO** iff FP=0/MISSED=0 byte-identical corpus-wide AND net wine wall improves beyond SP2.1's ~7.5% → ship (flip default-ON or opt-in).
- **NO-GO (FP/MISSED)** → classify-scale hole; record repro; default OFF.
- **Net-negative wall** → add selectivity (named-only-first, ∃-on-timeout — the spec's deferred fallback) and re-gate.

- [ ] **Step 5: Commit verdict + report to controller.** No default flip / merge without the controller's call.

---

## Notes for the implementer

- The ∃-seed is **sound by construction** (monotone: adding `sub`'s entailed ∃-facts can't flip Sat↔Unsat) — but Task 2's corpus oracle is the proof, not this argument.
- Saturation is computed **once** in `build` (now via `saturate_with_exists_facts`, feeding BOTH `sat_seed` and `exists_seed`) — never per pair, never twice.
- The `HyperEngine::new` (full index rebuild) when seeded is load-bearing (SP2.1 lesson): appended ∃-clauses must be indexed to fire. `decide_with_stats` already rebuilds; `classify_labels`' rebuild guard must include `exists_seed.is_some()`.
- Drop untranslatable ∃-targets (Tseitin/DKey) — sound under-approximation. Named + NomKey-nominal cover wine's value-assignment facts.
