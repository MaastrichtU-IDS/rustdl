# Phase-0 Combination Spike Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Measure whether combining three search levers (deterministic-look-ahead pruning + cheap-MRV ⊔ ordering + the aggressive ≤n/⊔ backjump) collapses one hard wine model's search from its ~67k-branch thrash to a small, fast, correct (`Sat`) search — the rewrite's load-bearing premise.

**Architecture:** One combo env flag (`RUSTDL_COMBO_SPIKE`, default OFF) enables all three levers together at the wedge's ⊔ branch path in `HyperEngine` (`crates/owl-dl-tableau/src/hyper.rs`). Branch off `feat/precise-merge-deps` (inherits the aggressive ≤n backjump). Throwaway, unsound-for-timing; measures search shrinkage + verdict-sanity, not FP=0.

**Tech Stack:** Rust (edition 2024), crate `owl-dl-tableau` (`HyperEngine`), crate `owl-dl-reasoner` (env gate + `sat_class_probe`/`decide_pair_probe` harness).

## Global Constraints

- **Throwaway spike**: code does NOT merge. Only `docs/combination-spike-gate-results-2026-06-23.md` lands (on `feat/build-once-redesign`).
- Branch `spike/combo-rewrite-gate` off `feat/precise-merge-deps`.
- **Unsound-for-timing is allowed.** The gate measures search shrinkage + verdict-sanity, NOT FP=0. Soundness is the rewrite's later build-phase gate, not this one. Frame results by correctness/soundness/performance, never by time/effort.
- **Flag-OFF byte-identical**: `combo_spike == false` ⇒ `find_open_disjunction` and the ⊔ loop behave exactly as on `feat/precise-merge-deps`.
- `cargo fmt --all -- --check` clean; `cargo clippy --workspace --all-targets --all-features -- -D warnings` clean (pedantic on); `cargo test -p owl-dl-tableau` green (flag-OFF).
- Toolchain (prefix every cargo command): `export RUSTUP_HOME=/home/dumontier/.rustup CARGO_HOME=/home/dumontier/.cargo; export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$CARGO_HOME/bin:$PATH"`
- Commit only when the human asks. Commit messages end with a blank line then:
  `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`
  `Claude-Session: https://claude.ai/code/session_01HSzon7V2wkhrudxBNAJduh`
- GO/NO-GO (pre-committed): **GO** iff one hard wine model collapses ~67k branches → small (<1k, ideally ~tens) **AND** wall < ~30 s **AND** verdict = **Sat**. **NO-GO** if it still thrashes OR collapses only to spurious-Unsat.

---

## File Structure

- `crates/owl-dl-tableau/src/hyper.rs` — `combo_spike` field + `with_combo_spike` builder; extract `head_atom_satisfied`; cheap-MRV in `find_open_disjunction`; det-pruning at the chosen ⊔; force precise backjump under the combo flag; white-box unit test.
- `crates/owl-dl-reasoner/src/lib.rs` — `hyper_combo_spike_enabled()` env helper (default OFF) wired at the `with_precise_card_deps` sites.
- `crates/owl-dl-reasoner/tests/combo_spike_gate.rs` — throwaway 2-wine-probe harness.
- `docs/combination-spike-gate-results-2026-06-23.md` — durable verdict.

---

### Task 1: combo flag scaffolding + extract `head_atom_satisfied`

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` (`HyperEngine` struct; constructors `new`/`new_with_prebuilt`/`new_seeded`; builder near `with_precise_card_deps` ~781; `any_head_satisfied` ~1938)
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (env helper near `hyper_precise_card_deps_enabled`; wiring at the `with_precise_card_deps` sites)
- Test: `crates/owl-dl-tableau/src/hyper.rs` (`mod tests`)

**Interfaces:**
- Produces: `HyperEngine` field `combo_spike: bool`; `pub fn with_combo_spike(mut self) -> Self`; `fn head_atom_satisfied(&self, ci: usize, k: usize, xnode: HNode, binding: &Binding) -> bool`; `owl_dl_reasoner::hyper_combo_spike_enabled() -> bool` (default **false**).

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn combo_spike_builder_and_head_atom_satisfied() {
    let a = cls(0);
    let clauses = vec![DlClause { body: vec![Atom::Class(a, X)], head: vec![] }];
    let off = HyperEngine::new(&clauses, a);
    assert!(!off.combo_spike_for_test());
    let on = HyperEngine::new(&clauses, a).with_combo_spike();
    assert!(on.combo_spike_for_test());
}
```
Add accessor next to the field:
```rust
#[cfg(test)]
pub(crate) fn combo_spike_for_test(&self) -> bool { self.combo_spike }
```

- [ ] **Step 2: Run it — expect compile failure** (`combo_spike` absent).

Run: `cargo test -p owl-dl-tableau --lib combo_spike_builder -- --nocapture`

- [ ] **Step 3: Add field + builder (mirror `precise_merge_deps`)**

In `HyperEngine`, next to `precise_merge_deps`:
```rust
    /// Throwaway Phase-0 combination spike (`RUSTDL_COMBO_SPIKE`): det-pruning +
    /// cheap-MRV + forced precise backjump at the ⊔ path. Default OFF.
    combo_spike: bool,
```
Add `combo_spike: false,` to the struct literal in `new`, `new_with_prebuilt`, `new_seeded`. Add:
```rust
    #[must_use]
    pub fn with_combo_spike(mut self) -> Self {
        self.combo_spike = true;
        self
    }
```

- [ ] **Step 4: Extract `head_atom_satisfied` from `any_head_satisfied`**

Replace `any_head_satisfied` (hyper.rs:1938–1981) with a per-head helper + a delegating wrapper (behaviour-preserving — same per-head arms, now indexed):
```rust
    fn head_atom_satisfied(&self, ci: usize, k: usize, xnode: HNode, binding: &Binding) -> bool {
        let resolve = |v: Var| resolve_var(v, xnode, binding);
        match &self.clauses[ci].head[k] {
            Atom::Class(c, v) => {
                matches!(resolve(*v), Some(t) if self.nodes[t.index()].has(*c))
            }
            Atom::Exists(role, cls, v) => {
                matches!(resolve(*v), Some(src) if self.nodes[src.index()].edges.iter().any(|(er, t)| {
                    role_matches(*er, *role, self.sub_roles.as_ref()) && self.nodes[t.index()].has(*cls)
                }))
            }
            Atom::AtMost(role, qual, n, v) => {
                matches!(resolve(*v), Some(src) if
                    self.nodes[src.index()].at_most.contains(&(*role, *qual, *n))
                    || self.distinct_role_succ(src, *role, *qual).len() <= *n as usize)
            }
            Atom::AtLeast(..) | Atom::Equal(..) | Atom::Role(..) => false,
        }
    }

    fn any_head_satisfied(&self, ci: usize, xnode: HNode, binding: &Binding) -> bool {
        (0..self.clauses[ci].head.len()).any(|k| self.head_atom_satisfied(ci, k, xnode, binding))
    }
```
(Verify the field/method names — `has`, `edges`, `at_most`, `distinct_role_succ`, `role_matches`, `resolve_var` — against the original arms you are replacing; copy them exactly.)

- [ ] **Step 5: Add the reasoner env helper + wiring**

In `crates/owl-dl-reasoner/src/lib.rs`, near `hyper_precise_card_deps_enabled`:
```rust
/// `RUSTDL_COMBO_SPIKE` (default OFF — throwaway Phase-0 rewrite gate).
pub fn hyper_combo_spike_enabled() -> bool {
    std::env::var_os("RUSTDL_COMBO_SPIKE").is_some_and(|v| v != "0" && !v.is_empty())
}
```
At every site chaining `with_precise_card_deps` (grep `with_precise_card_deps(`):
```rust
            if hyper_combo_spike_enabled() {
                engine = engine.with_combo_spike();
            }
```
`combo_spike` is read by the test accessor (Task 1) and the ⊔ path (Tasks 2–3), so no `#[allow(dead_code)]` should be needed; if clippy flags it before Task 2 lands, add it and remove in Task 2.

- [ ] **Step 6: Run + verify flag-OFF byte-identical**

Run: `cargo test -p owl-dl-tableau --lib combo_spike_builder -- --nocapture` → PASS.
Run: `cargo test -p owl-dl-tableau` → ALL pass (the `any_head_satisfied` refactor is behaviour-preserving; nothing else changed).
Run: `cargo build -p owl-dl-reasoner`.

- [ ] **Step 7: fmt + clippy + commit**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
git add crates/owl-dl-tableau/src/hyper.rs crates/owl-dl-reasoner/src/lib.rs
git commit  # "spike(combo): RUSTDL_COMBO_SPIKE scaffolding + head_atom_satisfied extraction" + trailers
```

---

### Task 2: det-look-ahead-as-pruning at the chosen ⊔

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` (the ⊔ block in `solve`, `if let Some((ci, node, binding)) = self.find_open_disjunction()` ~1777; `horn_fixpoint(FIXPOINT_ITERS)` const at hyper.rs:50)
- Test: `crates/owl-dl-tableau/src/hyper.rs` (`mod tests`)

**Interfaces:**
- Consumes: `combo_spike`, `head_atom_satisfied` (Task 1), `save`/`restore`, `apply_head_atom`, `horn_fixpoint(FIXPOINT_ITERS)`, `resolve_var`.
- Produces: the gated det-pruning that computes the live-disjunct index set used by the branch loop.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn combo_det_pruning_drops_killed_disjunct_preserving_sat() {
    // A ⊑ D1⊔D2 ; A⊓D1 ⊑ ⊥ (Horn). At the ⊔ on a node labelled A, the look-ahead
    // kills D1 (horn_fixpoint clashes), leaving D2 → A is Sat via D2, with fewer
    // branches than the un-pruned path (which would also try D1 and backtrack).
    let (a, d1, d2) = (cls(0), cls(1), cls(2));
    let clauses = vec![
        DlClause { body: vec![Atom::Class(a, X)], head: vec![Atom::Class(d1, X), Atom::Class(d2, X)] },
        DlClause { body: vec![Atom::Class(a, X), Atom::Class(d1, X)], head: vec![] },
    ];
    let off = HyperEngine::new(&clauses, a).decide(64);
    let mut on_eng = HyperEngine::new(&clauses, a).with_combo_spike();
    let on = on_eng.decide(64);
    assert_eq!(on, off, "combo changed the verdict on a controlled case");
    assert_eq!(off, HyperResult::Sat);
    // det-pruning means D1 is never branched: the on-run takes <= the off-run branches.
    let s_on = on_eng.stats();
    let s_off = { let mut e = HyperEngine::new(&clauses, a); let _ = e.decide(64); e.stats() };
    assert!(s_on.branches_taken <= s_off.branches_taken);
}
```
(Confirm `decide`/`stats()` accessors against the engine API, as on prior spikes; adapt if `decide` consumes `self`.)

- [ ] **Step 2: Run — expect FAIL** (no pruning yet; D1 branched).

Run: `cargo test -p owl-dl-tableau --lib combo_det_pruning -- --nocapture`

- [ ] **Step 3: Implement det-pruning in the ⊔ block**

In the ⊔ block, after `find_open_disjunction` returns `(ci, node, binding)` and after the existing `depth==0`/`track_depth`/`d`/`body_deps`/`decision_deps` preamble, compute the live-disjunct index list (gated); the real branch loop then iterates `live` instead of `0..head_len`:
```rust
            let head_len = self.clauses[ci].head.len();
            let live: Vec<usize> = if self.combo_spike {
                let saved_clash = self.clash_deps;
                let mut keep = Vec::with_capacity(head_len);
                for k in 0..head_len {
                    if self.head_atom_satisfied(ci, k, node, &binding) {
                        // already satisfied ⟹ this clause is not actually open here; keep
                        // (defensive — find_open_disjunction only returns open clauses).
                        keep.push(k);
                        continue;
                    }
                    let head_atom = self.clauses[ci].head[k];
                    let saved = self.save();
                    let _ = self.apply_head_atom(head_atom, node, &binding, DepSet::EMPTY);
                    let killed = matches!(self.horn_fixpoint(FIXPOINT_ITERS), HyperResult::Unsat);
                    self.restore(saved);
                    if !killed {
                        keep.push(k);
                    }
                }
                self.clash_deps = saved_clash;
                keep
            } else {
                (0..head_len).collect()
            };
            if self.combo_spike && live.is_empty() {
                // every disjunct deterministically clashes ⟹ this binding is unsat.
                // Conservative deps (spike is unsound-for-timing anyway).
                self.clash_deps = decision_deps;
                return HyperResult::Unsat;
            }
            // real branch loop: iterate `&live` instead of `0..head_len`
            for &k in &live {
                let head_atom = self.clauses[ci].head[k];
                // ... existing save / branches_taken+=1 / apply_head_atom / solve(depth-1)
                //     / restore / backjump-on-!contains(d) / combined.union body, UNCHANGED ...
            }
```
Adapt to the exact existing loop body on this branch (it already does the ⊔ backjump). The ONLY change to the loop is iterating `&live` rather than `0..head_len`. The `live` computation + `live.is_empty()` early-clash are the new gated code. REMOVE any `#[allow(dead_code)]` on `combo_spike`.

NOTE (read-only safety of the look-ahead): per-`k` `save()` is paired with `restore()` on every path; `self.clash_deps` is saved before the loop and restored after; `horn_fixpoint` self-re-seeds the worklist. This is the pattern the det-lookahead gate's review verified.

- [ ] **Step 4: Run the test + flag-OFF suite**

Run: `cargo test -p owl-dl-tableau --lib combo_det_pruning -- --nocapture` → PASS.
Run: `cargo test -p owl-dl-tableau` → ALL pass (flag-OFF: `live == (0..head_len)`, loop unchanged).

- [ ] **Step 5: fmt + clippy + commit**

```bash
cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings
git add crates/owl-dl-tableau/src/hyper.rs
git commit  # "spike(combo): det-look-ahead pruning at the chosen ⊔" + trailers
```

---

### Task 3: cheap-MRV ⊔ ordering + force precise backjump under combo

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` (`find_open_disjunction` ~1891; the combo-gated precise-backjump force)

**Interfaces:**
- Consumes: `combo_spike`, `head_atom_satisfied`, `precise_merge_deps`.
- Produces: MRV-ordered ⊔ selection under the combo flag; `precise_merge_deps` effectively on under combo.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn combo_mrv_picks_fewest_live_disjunct_clause() {
    // Two open ⊔ clauses on the same node-context: one with 3 not-satisfied
    // disjuncts, one with 2. With combo on, find_open_disjunction must return the
    // 2-disjunct clause first (MRV). Assert via a verdict-preserving + branch check,
    // or expose the chosen clause index via a test hook. (Construct two clauses with
    // distinct head arities both open at the root; assert the on-run resolves with
    // <= off-run branches and same verdict.)
    // ... concrete construction; the assertion is verdict-preserved + branches_on <= branches_off ...
}
```
(If a direct "which clause was chosen" assertion is awkward, assert verdict-preservation + `branches_on <= branches_off` on a shape where MRV demonstrably helps; document the shape. Do NOT fake the assertion.)

- [ ] **Step 2: Run — expect FAIL or no-op** (MRV not implemented).

Run: `cargo test -p owl-dl-tableau --lib combo_mrv -- --nocapture`

- [ ] **Step 3: Implement cheap-MRV in `find_open_disjunction`**

`find_open_disjunction` currently returns the first open `(ci, node, binding)`. Under the combo flag, collect all open candidates and return the one minimizing the count of not-already-satisfied disjuncts (cheap — uses `head_atom_satisfied`, NO `horn_fixpoint`):
```rust
    fn find_open_disjunction(&mut self) -> Option<(usize, HNode, Binding)> {
        if self.combo_spike {
            let mut best: Option<(usize, (usize, HNode, Binding))> = None; // (live_count, cand)
            // ... iterate the SAME candidate enumeration the existing body uses
            //     (node × clause × binding where the clause is open),
            //     compute live = #{k : !head_atom_satisfied(ci,k,node,&binding)},
            //     keep the candidate with the smallest live (ties: first) ...
            return best.map(|(_, cand)| cand);
        }
        // ... existing first-open body UNCHANGED ...
    }
```
Implement the combo branch by reusing the existing open-clause enumeration (lift it so both paths share it, or duplicate the scan inside the `if`). The MRV `live` count uses `head_atom_satisfied` only.

Force the precise ≤n backjump on under combo: at engine construction OR at the start of `decide`/`solve`, set `self.precise_merge_deps |= self.combo_spike;` (simplest: in `with_combo_spike`, also set `self.precise_merge_deps = true;`). Confirm the ≤n precise backjump path reads `precise_merge_deps`.

- [ ] **Step 4: Run the test + flag-OFF suite**

Run: `cargo test -p owl-dl-tableau --lib combo_mrv -- --nocapture` → PASS.
Run: `cargo test -p owl-dl-tableau` → ALL pass (flag-OFF: `find_open_disjunction` returns the first open clause as before).

- [ ] **Step 5: fmt + clippy + commit**

```bash
cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings
git add crates/owl-dl-tableau/src/hyper.rs
git commit  # "spike(combo): cheap-MRV ⊔ ordering + force precise backjump under combo" + trailers
```

---

### Task 4: throwaway 2-wine-probe harness

**Files:**
- Create: `crates/owl-dl-reasoner/tests/combo_spike_gate.rs`

**Interfaces:** Consumes `owl_dl_reasoner::{sat_class_probe, decide_pair_probe}` + `SearchStats`. Produces a `#[ignore]` harness.

- [ ] **Step 1: Write the harness** (clone the `det_lookahead_gate.rs` shape: 2 GiB stack, `RUSTDL_ADAPTIVE_BUDGET=0`, reads `RUSTDL_COMBO_SPIKE` from env, depth 256, 60 s deadline)

```rust
//! THROWAWAY Phase-0 combination spike gate (does NOT merge). Run TWICE (OFF then ON):
//!   cargo test -p owl-dl-reasoner --test combo_spike_gate -- --ignored --nocapture
//!   RUSTDL_COMBO_SPIKE=1 cargo test -p owl-dl-reasoner --test combo_spike_gate -- --ignored --nocapture
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
    read_ofn(&mut Cursor::new(src.into_bytes()), ParserConfiguration::default()).expect("parse").0
}

#[test]
#[ignore = "throwaway combo-spike gate; run with/without RUSTDL_COMBO_SPIKE"]
fn combo_spike_wine_collapse() {
    unsafe { std::env::set_var("RUSTDL_ADAPTIVE_BUDGET", "0"); }
    let child = std::thread::Builder::new().stack_size(2 * 1024 * 1024 * 1024).spawn(|| {
        let ont = load();
        let on = std::env::var("RUSTDL_COMBO_SPIKE").as_deref() == Ok("1");
        let dl = Some(Duration::from_secs(60));
        println!("##### COMBO-SPIKE GATE combo_on={on} #####");
        // sat(SweetWine)
        if let Some((res, s, ms)) = owl_dl_reasoner::sat_class_probe(&ont, &format!("{NS}SweetWine"), 256, dl).expect("probe") {
            println!("sat(SweetWine)            verdict={res:?} wall_ms={ms:.0} branches={} restores={} disj={} merge={}",
                s.branches_taken, s.restores, s.disj_branches, s.merge_branches);
        }
        // sat(Alsatian ⊓ ¬American)
        if let Some((res, s, ms)) = owl_dl_reasoner::decide_pair_probe(&ont, &format!("{NS}AlsatianWine"), &format!("{NS}AmericanWine"), 256, dl).expect("probe") {
            println!("sat(Alsatian⊓¬American)   verdict={res:?} wall_ms={ms:.0} branches={} restores={} disj={} merge={}",
                s.branches_taken, s.restores, s.disj_branches, s.merge_branches);
        }
        println!("##### END combo_on={on} #####");
    }).expect("spawn");
    child.join().expect("thread");
}
```
(Confirm `SearchStats` field names + the probe signatures against `decide_pair_probe`/`sat_class_probe` in `reasoner/src/lib.rs`.)

- [ ] **Step 2: Build + fmt + clippy + commit** (do NOT run the slow wine probe — Task 5 does that)

```bash
cargo build -p owl-dl-reasoner --tests
cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings
git add crates/owl-dl-reasoner/tests/combo_spike_gate.rs
git commit  # "spike(combo): 2-wine-probe collapse harness" + trailers
```

---

### Task 5: run + verdict doc + GO/NO-GO (controller-run, not a subagent)

**Files:**
- Create: `docs/combination-spike-gate-results-2026-06-23.md`

- [ ] **Step 1: Combo-OFF baseline** — `cargo test -p owl-dl-reasoner --test combo_spike_gate -- --ignored --nocapture` → record branches/restores/wall/verdict (expect the ~67k thrash, DNF/Stalled).
- [ ] **Step 2: Combo-ON** — `RUSTDL_COMBO_SPIKE=1 cargo test ... combo_spike_gate ...` → record branches/restores/wall/verdict.
- [ ] **Step 3: Verdict-sanity** — wine is consistent, both classes satisfiable (oracle). Confirm combo-ON verdict = **Sat**. A spurious-**Unsat** collapse is NOT a win — flag it.
- [ ] **Step 4: Verdict doc + GO/NO-GO** — branch/wall/verdict table (OFF vs ON) for both models; apply the bar: GO iff one model collapses ~67k → small (<1k) AND wall < ~30 s AND verdict = Sat; else NO-GO (still thrashing, or only spurious-Unsat). State the call + consequence (GO → the rewrite's first real build phase is a *sound* single-model construction, corpus closure-diff/FP=0 gated; NO-GO → the levers do not compound, reconsider the approach).
- [ ] **Step 5: Commit verdict; discard spike code** — commit the verdict doc; then land ONLY the verdict doc on `feat/build-once-redesign` (cherry-pick the doc commit). The `spike/combo-rewrite-gate` code is NOT merged.

---

## Self-Review

**1. Spec coverage:** combo flag default-OFF + byte-identical (Task 1) ✓; det-pruning at chosen ⊔ (Task 2) ✓; cheap-MRV no-look-ahead-in-scan (Task 3) ✓; force precise backjump under combo (Task 3) ✓; `head_atom_satisfied` extraction (Task 1) ✓; 2-wine-probe harness + verdict (Task 4) ✓; GO/NO-GO branch<1k AND wall<30s AND Sat (Global Constraints + Task 5) ✓; verdict-sanity / spurious-Unsat flag (Task 5) ✓; throwaway/verdict-only (Task 5) ✓.

**2. Placeholder scan:** Task 3's MRV-enumeration and its test reuse the existing open-clause scan / a verdict-preserving+branch assertion with a "document the shape, don't fake" instruction — construction-by-analogy with a concrete fallback, not unspecified logic. `FIXPOINT_ITERS` is the real const (hyper.rs:50). No "TBD"/"handle errors".

**3. Type consistency:** `combo_spike: bool`, `with_combo_spike`, `hyper_combo_spike_enabled`, `head_atom_satisfied(ci,k,xnode,binding)`, `horn_fixpoint(FIXPOINT_ITERS)`, `find_open_disjunction -> Option<(usize, HNode, Binding)>`, the `live: Vec<usize>` branch-loop change — consistent across Tasks 1–5. `combo_spike` forces `precise_merge_deps` on (Task 3) — the field exists on `feat/precise-merge-deps`, verified.
