# Deterministic-closure ⊔-resolution viability gate Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Measure whether a full deterministic closure (Horn-fixpoint look-ahead) resolves wine's free ⊔ branch points — a read-only probe at ⊔ points counting how many collapse to ≤1 surviving disjunct — to decide GO/NO-GO on building the build-once deterministic-expansion cache (Konclude edge 2).

**Architecture:** Read-only look-ahead hooked at `HyperEngine`'s ⊔ branch point: for each free disjunct, `save()` → apply it → `horn_fixpoint()` → record clash → `restore()`. Counts only; the real ⊔ branching is unchanged. Gated `RUSTDL_DET_LOOKAHEAD_PROBE` (default OFF). Near-clone of the already-built SP-B saturation-guided gate with a deterministic-look-ahead oracle instead of told-disjointness.

**Tech Stack:** Rust (edition 2024), crate `owl-dl-tableau` (`HyperEngine`), crate `owl-dl-reasoner` (env gate + `sat_class_probe` harness).

## Global Constraints

- **Throwaway research gate**: code does NOT merge. Only `docs/deterministic-disjunction-resolution-gate-results-2026-06-23.md` lands.
- Branch `spike/det-lookahead` off `feat/build-once-redesign`.
- **Read-only / verdict-preserving**: probe ON must not change any verdict vs probe OFF (it only counts). Default OFF; flag-OFF path byte-identical to current branch.
- `cargo fmt --all -- --check` clean; `cargo clippy --workspace --all-targets --all-features -- -D warnings` clean (pedantic on); `cargo test -p owl-dl-tableau` green.
- Toolchain (prefix every cargo command): `export RUSTUP_HOME=/home/dumontier/.rustup CARGO_HOME=/home/dumontier/.cargo; export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$CARGO_HOME/bin:$PATH"`
- Commit only when the human asks. Commit messages end with a blank line then:
  `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`
  `Claude-Session: https://claude.ai/code/session_01HSzon7V2wkhrudxBNAJduh`
- GO/NO-GO (pre-committed): **GO** iff the look-ahead collapses **≥70%** of wine's sampled free ⊔ points to ≤1 survivor. **NO-GO** (wine exhaustively closed) iff it collapses ~none (comparable to SP-B's pruned=0).

---

## File Structure

- `crates/owl-dl-tableau/src/hyper.rs` — `det_lookahead_probe` field + `with_det_lookahead_probe` builder + 3 `SearchStats` counters + the read-only look-ahead loop in the ⊔ block + a verdict-preservation unit test.
- `crates/owl-dl-reasoner/src/lib.rs` — `hyper_det_lookahead_probe_enabled()` env helper (default OFF) + wiring at the `with_precise_card_deps` sites.
- `crates/owl-dl-reasoner/tests/det_lookahead_gate.rs` — throwaway wine harness (4 sat-probes × OFF/ON, stats dump).
- `docs/deterministic-disjunction-resolution-gate-results-2026-06-23.md` — durable verdict.

---

### Task 1: `det_lookahead_probe` gate scaffolding + counters (flag-OFF byte-identical)

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` (`HyperEngine` struct ~448–575; constructors `new` ~729, `new_with_prebuilt` ~768, `new_seeded` ~1630; builder near `with_precise_card_deps` ~781; `SearchStats` struct ~363)
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (env helper near `hyper_precise_card_deps_enabled` ~1181; wiring ~1014)
- Test: `crates/owl-dl-tableau/src/hyper.rs` (`mod tests`)

**Interfaces:**
- Produces: `HyperEngine` field `det_lookahead_probe: bool`; `pub fn with_det_lookahead_probe(mut self) -> Self`; `SearchStats` fields `det_or_points: u64`, `det_disjuncts_killed: u64`, `det_or_points_collapsed: u64`; `owl_dl_reasoner::hyper_det_lookahead_probe_enabled() -> bool` (default **false**).

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn det_lookahead_probe_builder_sets_flag_and_off_is_default() {
    let a = cls(0);
    let clauses = vec![DlClause { body: vec![Atom::Class(a, X)], head: vec![] }];
    let off = HyperEngine::new(&clauses, a);
    assert!(!off.det_lookahead_probe_for_test());
    let on = HyperEngine::new(&clauses, a).with_det_lookahead_probe();
    assert!(on.det_lookahead_probe_for_test());
}
```
Add the accessor next to the field:
```rust
#[cfg(test)]
pub(crate) fn det_lookahead_probe_for_test(&self) -> bool { self.det_lookahead_probe }
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `cargo test -p owl-dl-tableau --lib det_lookahead_probe_builder -- --nocapture`
Expected: FAIL to compile (field/builder absent).

- [ ] **Step 3: Add the field, counters, builder, env helper**

In `SearchStats` (~363), add:
```rust
    /// Deterministic-look-ahead probe (RUSTDL_DET_LOOKAHEAD_PROBE): ⊔ points
    /// reached; free disjuncts killed by a Horn-fixpoint look-ahead; ⊔ points
    /// the look-ahead collapses to ≤1 surviving disjunct.
    pub det_or_points: u64,
    pub det_disjuncts_killed: u64,
    pub det_or_points_collapsed: u64,
```
In `HyperEngine`, next to `precise_card_deps: bool`:
```rust
    /// Opt-in (`RUSTDL_DET_LOOKAHEAD_PROBE`): read-only deterministic-closure
    /// look-ahead at ⊔ points (counts only; does not change the search). Default OFF.
    det_lookahead_probe: bool,
```
Add `det_lookahead_probe: false,` to the struct literal in `new`, `new_with_prebuilt`, `new_seeded`.
Builder next to `with_precise_card_deps`:
```rust
    #[must_use]
    pub fn with_det_lookahead_probe(mut self) -> Self {
        self.det_lookahead_probe = true;
        self
    }
```
In `crates/owl-dl-reasoner/src/lib.rs`, next to `hyper_precise_card_deps_enabled`:
```rust
/// `RUSTDL_DET_LOOKAHEAD_PROBE` (default OFF — throwaway research gate). Read-only
/// deterministic look-ahead at ⊔ points; counts only.
pub fn hyper_det_lookahead_probe_enabled() -> bool {
    std::env::var_os("RUSTDL_DET_LOOKAHEAD_PROBE").is_some_and(|v| v != "0" && !v.is_empty())
}
```
At every site chaining `with_precise_card_deps` (grep `with_precise_card_deps(` in lib.rs):
```rust
            if hyper_det_lookahead_probe_enabled() {
                engine = engine.with_det_lookahead_probe();
            }
```
The field is unused until Task 2 — add `#[allow(dead_code)]` on `det_lookahead_probe` ONLY if clippy requires it (the test accessor reads it, so likely not needed); remove any such allow in Task 2.

- [ ] **Step 4: Run the test + flag-OFF suite**

Run: `cargo test -p owl-dl-tableau --lib det_lookahead_probe_builder -- --nocapture` → PASS.
Run: `cargo test -p owl-dl-tableau` → all existing tests PASS (field inert).
Run: `cargo build -p owl-dl-reasoner` → builds.

- [ ] **Step 5: fmt + clippy + commit**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
git add crates/owl-dl-tableau/src/hyper.rs crates/owl-dl-reasoner/src/lib.rs
git commit  # "spike(det): RUSTDL_DET_LOOKAHEAD_PROBE gate scaffolding (default OFF, inert)" + trailers
```

---

### Task 2: Read-only deterministic look-ahead at the ⊔ point

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` (the ⊔ branch block in `solve` — `if let Some((ci, node, binding)) = self.find_open_disjunction() { ... }`, ~line 1761; `horn_fixpoint` const `FIXPOINT_ITERS` is the cap the real pre-branch call uses at ~1699)
- Test: `crates/owl-dl-tableau/src/hyper.rs` (`mod tests`)

**Interfaces:**
- Consumes: `det_lookahead_probe` (Task 1), `save`/`restore`, `apply_head_atom`, `horn_fixpoint(FIXPOINT_ITERS)`, `any_head_satisfied(ci, node, &binding)` (the existing satisfied-disjunct check), `resolve_var`.
- Produces: the read-only counting loop; no change to the real ⊔ branching.

- [ ] **Step 1: Write the failing verdict-preservation + kill test**

A tiny ⊔ ontology where one disjunct is deterministically killed by Horn propagation:
```rust
#[test]
fn det_lookahead_probe_counts_kill_and_preserves_verdict() {
    // A ⊑ ∃r.b ; A ⊑ ≤... not needed. Use: A ⊑ (D1 ⊔ D2); D1 ⊓ A ⊑ ⊥ (Horn:
    // body {D1,A} → empty head). At the ⊔ point on a node labelled A, asserting
    // D1 then horn_fixpoint clashes (D1 killed); D2 survives. Probe ON must count
    // det_disjuncts_killed>=1 and a collapse, and the verdict must equal OFF.
    let a = cls(0);
    let (d1, d2) = (cls(1), cls(2));
    let clauses = vec![
        DlClause { body: vec![Atom::Class(a, X)], head: vec![Atom::Class(d1, X), Atom::Class(d2, X)] },
        DlClause { body: vec![Atom::Class(a, X), Atom::Class(d1, X)], head: vec![] }, // A⊓D1 ⊑ ⊥
    ];
    let off = HyperEngine::new(&clauses, a).decide(64);
    let mut on_eng = HyperEngine::new(&clauses, a).with_det_lookahead_probe();
    let on = on_eng.decide(64);
    assert_eq!(on, off, "probe changed the verdict — NOT read-only");
    assert_eq!(off, HyperResult::Sat, "A is Sat via D2");
    let s = on_eng.stats();
    assert!(s.det_disjuncts_killed >= 1, "D1 must be killed by Horn look-ahead");
    assert!(s.det_or_points >= 1 && s.det_or_points_collapsed >= 1);
}
```
(Confirm `decide` / `stats()` accessors against the engine: `decide(depth) -> HyperResult` exists; `stats()` is public per the precise-merge canary. If `decide` consumes `self`, capture stats via a `&self` accessor after — read the engine API and adapt; the assertion is the three counter checks + verdict equality.)

- [ ] **Step 2: Run it to confirm it fails**

Run: `cargo test -p owl-dl-tableau --lib det_lookahead_probe_counts_kill -- --nocapture`
Expected: FAIL (counters stay 0 — look-ahead not implemented).

- [ ] **Step 3: Implement the read-only look-ahead**

In the ⊔ block, immediately after `if let Some((ci, node, binding)) = self.find_open_disjunction() {` and after the `depth == 0` / `track_depth` lines but BEFORE the real branching loop, insert (guarded):
```rust
            if self.det_lookahead_probe {
                self.stats.det_or_points += 1;
                let head_len = self.clauses[ci].head.len();
                let mut survivors = 0u32;
                for k in 0..head_len {
                    // Skip disjuncts already satisfied (not "free").
                    if self.head_atom_satisfied(ci, k, node, &binding) {
                        continue;
                    }
                    let head_atom = self.clauses[ci].head[k];
                    let saved = self.save();
                    let _ = self.apply_head_atom(head_atom, node, &binding, DepSet::EMPTY);
                    let killed = matches!(self.horn_fixpoint(FIXPOINT_ITERS), HyperResult::Unsat);
                    self.restore(saved);
                    if killed {
                        self.stats.det_disjuncts_killed += 1;
                    } else {
                        survivors += 1;
                    }
                }
                if survivors <= 1 {
                    self.stats.det_or_points_collapsed += 1;
                }
            }
            // ... UNCHANGED real ⊔ branching loop follows ...
```
NOTE: `any_head_satisfied(ci, node, &binding)` checks whether ANY head is satisfied (for the whole clause). For the per-disjunct "free" check you need a per-`k` test. If a `head_atom_satisfied(ci, k, node, binding)` helper does not exist, extract the single-head check out of `any_head_satisfied` into a `fn head_atom_satisfied(&self, ci: usize, k: usize, xnode: HNode, binding: &Binding) -> bool` and call it from both (DRY) — read `any_head_satisfied` (~the `fn any_head_satisfied` after `find_open_disjunction`) and lift its per-head arm. Counting a satisfied disjunct as a survivor would only inflate survivor count (bias toward NOT-collapsed = conservative/NO-GO), so if extracting is awkward, you MAY skip the satisfied-check and count all head atoms — note it in the report (it makes the collapse ratio a lower bound).

Remove any `#[allow(dead_code)]` on `det_lookahead_probe`.

- [ ] **Step 4: Run the test + flag-OFF suite**

Run: `cargo test -p owl-dl-tableau --lib det_lookahead_probe_counts_kill -- --nocapture` → PASS.
Run: `cargo test -p owl-dl-tableau` → ALL pass (flag-OFF byte-identical: the `if self.det_lookahead_probe` block is skipped, real branching unchanged).

- [ ] **Step 5: fmt + clippy + commit**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
git add crates/owl-dl-tableau/src/hyper.rs
git commit  # "spike(det): read-only deterministic look-ahead at ⊔ points (counts only)" + trailers
```

---

### Task 3: Throwaway wine harness (4 sat-probes × OFF/ON)

**Files:**
- Create: `crates/owl-dl-reasoner/tests/det_lookahead_gate.rs`

**Interfaces:** Consumes `owl_dl_reasoner::sat_class_probe` and the `SearchStats` counters (Task 1). Produces a `#[ignore]` harness.

- [ ] **Step 1: Write the harness** (clone the `precise_merge_fp_diag.rs` / `sat_guide_gate.rs` shape)

```rust
//! THROWAWAY det-lookahead viability gate (does NOT merge). Run TWICE:
//!   cargo test -p owl-dl-reasoner --test det_lookahead_gate -- --ignored --nocapture
//!   RUSTDL_DET_LOOKAHEAD_PROBE=1 cargo test -p owl-dl-reasoner --test det_lookahead_gate -- --ignored --nocapture
#![allow(clippy::unwrap_used, clippy::doc_markdown, unsafe_code)]
use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;
use std::time::Duration;

const WINE: &str = "../../ontologies/real/wine.ofn";
const NS: &str = "http://www.w3.org/TR/2003/PR-owl-guide-20031209/wine#";

fn load() -> SetOntology<RcStr> {
    let src = std::fs::read_to_string(WINE).unwrap_or_else(|e| panic!("read {WINE}: {e}"));
    let mut r = Cursor::new(src.into_bytes());
    read_ofn(&mut r, ParserConfiguration::default()).expect("parse wine.ofn").0
}

#[test]
#[ignore = "throwaway det-lookahead viability gate; run with/without RUSTDL_DET_LOOKAHEAD_PROBE"]
fn det_lookahead_wine_collapse_ratio() {
    unsafe { std::env::set_var("RUSTDL_ADAPTIVE_BUDGET", "0"); }
    let child = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024 * 1024)
        .spawn(|| {
            let ont = load();
            let on = std::env::var("RUSTDL_DET_LOOKAHEAD_PROBE").as_deref() == Ok("1");
            println!("##### DET-LOOKAHEAD GATE probe_on={on} #####");
            let dl = Some(Duration::from_secs(60));
            for c in ["Wine", "AlsatianWine", "SweetWine", "Zinfandel"] {
                let iri = format!("{NS}{c}");
                match owl_dl_reasoner::sat_class_probe(&ont, &iri, 256, dl).expect("probe") {
                    Some((res, s, ms)) => println!(
                        "{c:14} verdict={res:?} wall_ms={ms:.0} or_points={} killed={} collapsed={} ratio={:.2}",
                        s.det_or_points, s.det_disjuncts_killed, s.det_or_points_collapsed,
                        if s.det_or_points > 0 { s.det_or_points_collapsed as f64 / s.det_or_points as f64 } else { 0.0 },
                    ),
                    None => println!("{c:14} NOT A NAMED CLASS"),
                }
            }
            println!("##### END probe_on={on} #####");
        })
        .expect("spawn");
    child.join().expect("thread");
}
```
(Confirm `SearchStats` is the struct `sat_class_probe` returns and exposes the 3 new counters — it is the same `SearchStats` Task 1 extended.)

- [ ] **Step 2: Build + fmt + clippy + commit**

```bash
cargo build -p owl-dl-reasoner --tests
cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings
git add crates/owl-dl-reasoner/tests/det_lookahead_gate.rs
git commit  # "spike(det): wine collapse-ratio harness" + trailers
```

---

### Task 4: Run + verdict doc + GO/NO-GO (controller-run, not a subagent)

**Files:**
- Create: `docs/deterministic-disjunction-resolution-gate-results-2026-06-23.md`

- [ ] **Step 1: Run probe OFF (verdict-preservation baseline)**

`cargo test -p owl-dl-reasoner --test det_lookahead_gate -- --ignored --nocapture` → record each class's verdict + wall.

- [ ] **Step 2: Run probe ON**

`RUSTDL_DET_LOOKAHEAD_PROBE=1 cargo test ... det_lookahead_gate ...` → record verdict + or_points/killed/collapsed/ratio per class.

- [ ] **Step 3: Verdict-preservation check**

Confirm each class's ON verdict == OFF verdict. If any differ, the look-ahead leaked state — the measurement is INVALID; fix Task 2 before trusting ratios.

- [ ] **Step 4: Write the verdict doc + GO/NO-GO**

Per-class collapse-ratio table; the aggregate. Apply the bar: GO (build the build-once deterministic-expansion cache) iff ≥70% of sampled free ⊔ points collapse to ≤1 survivor; NO-GO (wine exhaustively closed — 4th convergent NO-GO) iff ~none. State the call + consequence. Note the deadline + #⊔-points-sampled (the ratio is over the sampled prefix).

- [ ] **Step 5: Commit the verdict (durable); discard the spike code**

```bash
git add docs/deterministic-disjunction-resolution-gate-results-2026-06-23.md
git commit  # "docs(det): deterministic ⊔-resolution gate verdict + GO/NO-GO" + trailers
```
Then land ONLY the verdict doc on `feat/build-once-redesign` (cherry-pick the doc commit); the `spike/det-lookahead` code is NOT merged.

---

## Self-Review

**1. Spec coverage:** read-only look-ahead at ⊔ (Task 2) ✓; save→apply→horn_fixpoint→restore ✓; 3 counters + collapse-to-≤1 (Tasks 1–2) ✓; RUSTDL_DET_LOOKAHEAD_PROBE default OFF + byte-identical (Task 1) ✓; verdict-preservation guard (Tasks 2 test + 4) ✓; wine 4-class harness (Task 3) ✓; GO/NO-GO ≥70% bar (Global Constraints + Task 4) ✓; throwaway/verdict-only (Task 4) ✓.

**2. Placeholder scan:** the `head_atom_satisfied` per-disjunct check is flagged with a concrete fallback (count all head atoms = conservative lower bound) if the helper extraction is awkward — not an unspecified gap. `FIXPOINT_ITERS` is the real const (verified at hyper.rs:1699). No "TBD".

**3. Type consistency:** `det_lookahead_probe: bool`, `with_det_lookahead_probe`, `hyper_det_lookahead_probe_enabled`, counters `det_or_points`/`det_disjuncts_killed`/`det_or_points_collapsed`, `horn_fixpoint(FIXPOINT_ITERS)` — consistent across Tasks 1–4. `SearchStats` is the struct `sat_class_probe` returns (verified — same struct extended in Task 1).
