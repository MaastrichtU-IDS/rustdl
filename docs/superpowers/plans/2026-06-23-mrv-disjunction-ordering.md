# MRV Disjunction Ordering Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add MRV (most-constrained-variable) ordering to the wedge's ⊔ rule — `find_open_disjunction` returns the open disjunctive clause with the fewest live disjuncts first — as a sound, gated, corpus-FP=0-validated feature that collapses wine's hard models 5–54× to the correct verdict.

**Architecture:** A `mrv_ordering` bool flag on `HyperEngine` (env `RUSTDL_MRV_ORDERING`, default OFF) switches `find_open_disjunction` from first-open to fewest-live-disjunct selection. Verdict-invariant by construction (reordering only; no drop/add/alter). Validated in the combination spike's MRV-only run (FP=0/MISSED=0 full wine closure; Alsatian⊓¬American 66683→1227 br; SweetWine 67459→12366). The corpus FP=0 gate is the proof that flips it default-ON.

**Tech Stack:** Rust (edition 2024), crate `owl-dl-tableau` (`HyperEngine`), crate `owl-dl-reasoner` (env gate + `konclude_closure_diff` corpus gate).

## Global Constraints

- Branch `feat/mrv-ordering` off `feat/build-once-redesign`.
- **FP=0 is sacred.** MRV is verdict-invariant by construction, so FP=0/MISSED=0 is expected — the corpus gate (Task 3) is the proof, not the assumption.
- Gated `RUSTDL_MRV_ORDERING`, **default OFF**; flag-OFF path byte-identical to current `feat/build-once-redesign`. Flip default-ON only on a clean corpus gate.
- `cargo fmt --all -- --check` clean; `cargo clippy --workspace --all-targets --all-features -- -D warnings` clean (pedantic); `cargo test --workspace` green (flag-OFF).
- Toolchain (prefix every cargo command): `export RUSTUP_HOME=/home/dumontier/.rustup CARGO_HOME=/home/dumontier/.cargo; export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$CARGO_HOME/bin:$PATH"`
- Commit only when the human asks. Messages end with a blank line then:
  `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`
  `Claude-Session: https://claude.ai/code/session_01HSzon7V2wkhrudxBNAJduh`
- Frame by correctness/soundness/performance, never time/effort.

---

## File Structure

- `crates/owl-dl-tableau/src/hyper.rs` — `mrv_ordering` field + `with_mrv_ordering` builder + `mrv_ordering_for_test` accessor; extract `head_atom_satisfied`; the MRV branch in `find_open_disjunction`; the unit selection test.
- `crates/owl-dl-reasoner/src/lib.rs` — `hyper_mrv_ordering_enabled()` env helper + wiring at the `with_precise_card_deps` sites.
- `docs/mrv-ordering-gate-results-2026-06-23.md` — corpus gate results (Task 3, controller-run).

---

### Task 1: `mrv_ordering` flag scaffolding + `head_atom_satisfied` extraction

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` (`HyperEngine` struct ~448–575; constructors `new` ~729, `new_with_prebuilt` ~768, `new_seeded` ~1630; builder near `with_precise_card_deps` ~781; `any_head_satisfied` ~1938)
- Modify: `crates/owl-dl-reasoner/src/lib.rs` (env helper near `hyper_precise_card_deps_enabled` ~1181; wiring ~1014)
- Test: `crates/owl-dl-tableau/src/hyper.rs` (`mod tests`)

**Interfaces:**
- Produces: `HyperEngine` field `mrv_ordering: bool`; `pub fn with_mrv_ordering(mut self) -> Self`; `fn head_atom_satisfied(&self, ci: usize, k: usize, xnode: HNode, binding: &Binding) -> bool`; `owl_dl_reasoner::hyper_mrv_ordering_enabled() -> bool` (default **false**).

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn mrv_ordering_builder_and_default() {
    let a = cls(0);
    let clauses = vec![DlClause { body: vec![Atom::Class(a, X)], head: vec![] }];
    assert!(!HyperEngine::new(&clauses, a).mrv_ordering_for_test());
    assert!(HyperEngine::new(&clauses, a).with_mrv_ordering().mrv_ordering_for_test());
}
```
Accessor next to the field:
```rust
#[cfg(test)]
pub(crate) fn mrv_ordering_for_test(&self) -> bool { self.mrv_ordering }
```

- [ ] **Step 2: Run it — expect compile failure.**

Run: `cargo test -p owl-dl-tableau --lib mrv_ordering_builder -- --nocapture`

- [ ] **Step 3: Add field + builder (mirror `precise_card_deps`)**

In `HyperEngine`, next to `precise_card_deps`:
```rust
    /// `RUSTDL_MRV_ORDERING` (default OFF): `find_open_disjunction` returns the open
    /// disjunctive clause with the fewest live disjuncts first (most-constrained-variable).
    /// Verdict-invariant (reordering only). See the MRV spec.
    mrv_ordering: bool,
```
Add `mrv_ordering: false,` to the struct literal in `new`, `new_with_prebuilt`, `new_seeded`. Add:
```rust
    #[must_use]
    pub fn with_mrv_ordering(mut self) -> Self {
        self.mrv_ordering = true;
        self
    }
```

- [ ] **Step 4: Extract `head_atom_satisfied` from `any_head_satisfied`**

Replace `any_head_satisfied` (hyper.rs:1938) with a per-head helper + delegating wrapper. **Copy each per-head arm EXACTLY** from the current body (Class/Exists/AtMost/other):
```rust
    fn head_atom_satisfied(&self, ci: usize, k: usize, xnode: HNode, binding: &Binding) -> bool {
        let resolve = |v: Var| resolve_var(v, xnode, binding);
        match &self.clauses[ci].head[k] {
            Atom::Class(c, v) => matches!(resolve(*v), Some(t) if self.nodes[t.index()].has(*c)),
            Atom::Exists(role, cls, v) => matches!(resolve(*v), Some(src) if
                self.nodes[src.index()].edges.iter().any(|(er, t)| {
                    role_matches(*er, *role, self.sub_roles.as_ref()) && self.nodes[t.index()].has(*cls)
                })),
            Atom::AtMost(role, qual, n, v) => matches!(resolve(*v), Some(src) if
                self.nodes[src.index()].at_most.contains(&(*role, *qual, *n))
                || self.distinct_role_succ(src, *role, *qual).len() <= *n as usize),
            Atom::AtLeast(..) | Atom::Equal(..) | Atom::Role(..) => false,
        }
    }

    fn any_head_satisfied(&self, ci: usize, xnode: HNode, binding: &Binding) -> bool {
        (0..self.clauses[ci].head.len()).any(|k| self.head_atom_satisfied(ci, k, xnode, binding))
    }
```
**Verify** the field/method names (`has`, `edges`, `at_most`, `distinct_role_succ`, `role_matches`, `resolve_var`, `sub_roles`) against the arms you are replacing — copy verbatim; this MUST be behaviour-preserving.

- [ ] **Step 5: Reasoner env helper + wiring**

In `crates/owl-dl-reasoner/src/lib.rs`, near `hyper_precise_card_deps_enabled`:
```rust
/// `RUSTDL_MRV_ORDERING` (default OFF until the corpus gate flips it). See the MRV spec.
pub fn hyper_mrv_ordering_enabled() -> bool {
    std::env::var_os("RUSTDL_MRV_ORDERING").is_some_and(|v| v != "0" && !v.is_empty())
}
```
At every site chaining `with_precise_card_deps` (grep `with_precise_card_deps(`):
```rust
            if hyper_mrv_ordering_enabled() {
                engine = engine.with_mrv_ordering();
            }
```
`mrv_ordering` is read by the test accessor (now) and `find_open_disjunction` (Task 2) — no `#[allow(dead_code)]` expected; if clippy flags it before Task 2, add one and remove in Task 2.

- [ ] **Step 6: Run + verify flag-OFF byte-identical**

Run: `cargo test -p owl-dl-tableau --lib mrv_ordering_builder -- --nocapture` → PASS.
Run: `cargo test -p owl-dl-tableau` → ALL existing tests PASS (the `any_head_satisfied` refactor is behaviour-preserving — this is the guard).
Run: `cargo build -p owl-dl-reasoner`.

- [ ] **Step 7: fmt + clippy + commit**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
git add crates/owl-dl-tableau/src/hyper.rs crates/owl-dl-reasoner/src/lib.rs
git commit  # "feat(wedge): RUSTDL_MRV_ORDERING scaffolding + head_atom_satisfied extraction (default OFF)" + trailers
```

---

### Task 2: MRV branch in `find_open_disjunction` + unit selection test

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` (`find_open_disjunction` ~1936)
- Test: `crates/owl-dl-tableau/src/hyper.rs` (`mod tests`)

**Interfaces:**
- Consumes: `mrv_ordering`, `head_atom_satisfied` (Task 1), `is_blocked`, `is_horn`, `match_body`, `any_head_satisfied`.
- Produces: MRV-ordered selection under the flag; first-open unchanged when off.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn mrv_ordering_picks_fewest_live_disjunct_clause() {
    // Root node labelled A. Two open disjunctive clauses:
    //   clause0: A -> d1 ⊔ d2 ⊔ d3   (3 live disjuncts)
    //   clause1: A -> e1 ⊔ e2        (2 live disjuncts)
    // MRV-OFF: find_open_disjunction returns clause0 (first). MRV-ON: returns clause1 (2<3).
    let (a, d1, d2, d3, e1, e2) = (cls(0), cls(1), cls(2), cls(3), cls(4), cls(5));
    let clauses = vec![
        DlClause { body: vec![Atom::Class(a, X)], head: vec![Atom::Class(d1, X), Atom::Class(d2, X), Atom::Class(d3, X)] },
        DlClause { body: vec![Atom::Class(a, X)], head: vec![Atom::Class(e1, X), Atom::Class(e2, X)] },
    ];
    // OFF: first-open = clause index 0
    let mut off = HyperEngine::new(&clauses, a);
    off.horn_fixpoint(FIXPOINT_ITERS);
    assert_eq!(off.find_open_disjunction_for_test().map(|(ci, _, _)| ci), Some(0));
    // ON: MRV = clause index 1 (fewer live disjuncts)
    let mut on = HyperEngine::new(&clauses, a).with_mrv_ordering();
    on.horn_fixpoint(FIXPOINT_ITERS);
    assert_eq!(on.find_open_disjunction_for_test().map(|(ci, _, _)| ci), Some(1));
}
```
Add a test-only accessor (find_open_disjunction is `&mut self`):
```rust
#[cfg(test)]
pub(crate) fn find_open_disjunction_for_test(&mut self) -> Option<(usize, HNode, Binding)> {
    self.find_open_disjunction()
}
```
(If seeding the root's `A` label needs an explicit step before `horn_fixpoint` — check how `new` seeds the root class; `new(&clauses, a)` roots at `a`, so `A` is on the root after the initial fixpoint. If `find_open_disjunction_for_test` returns `None`, the root isn't labelled `A` yet — add the label via the same path `new` uses, or call the engine's seed step. Adapt against the real seeding; the assertion is the ci selection.)

- [ ] **Step 2: Run — expect FAIL** (MRV not implemented; ON returns 0 not 1).

Run: `cargo test -p owl-dl-tableau --lib mrv_ordering_picks -- --nocapture`

- [ ] **Step 3: Implement the MRV branch**

At the TOP of `find_open_disjunction` (before the existing first-open body), add the gated MRV branch reusing the same candidate enumeration:
```rust
        if self.mrv_ordering {
            let mut best: Option<(usize, (usize, HNode, Binding))> = None; // (live_count, candidate)
            for idx in 0..self.nodes.len() {
                let node = HNode(u32::try_from(idx).expect("fits u32"));
                if self.is_blocked(node) {
                    continue;
                }
                for ci in 0..self.clauses.len() {
                    if self.clauses[ci].is_horn() {
                        continue;
                    }
                    let Some(bindings) = self.match_body(ci, node) else {
                        continue;
                    };
                    for binding in bindings {
                        if self.any_head_satisfied(ci, node, &binding) {
                            continue;
                        }
                        let live = (0..self.clauses[ci].head.len())
                            .filter(|&k| !self.head_atom_satisfied(ci, k, node, &binding))
                            .count();
                        let better = match &best {
                            None => true,
                            Some((b, _)) => live < *b,
                        };
                        if better {
                            best = Some((live, (ci, node, binding)));
                        }
                    }
                }
            }
            return best.map(|(_, cand)| cand);
        }
        // existing first-open body UNCHANGED below
```
Remove any `#[allow(dead_code)]` on `mrv_ordering`.

- [ ] **Step 4: Run the test + flag-OFF suite**

Run: `cargo test -p owl-dl-tableau --lib mrv_ordering_picks -- --nocapture` → PASS (OFF→0, ON→1).
Run: `cargo test -p owl-dl-tableau` → ALL pass (flag-OFF: the MRV branch is skipped, first-open body unchanged).

- [ ] **Step 5: fmt + clippy + commit**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
git add crates/owl-dl-tableau/src/hyper.rs
git commit  # "feat(wedge): MRV most-constrained-⊔-first ordering in find_open_disjunction" + trailers
```

---

### Task 3: Corpus FP=0 / no-regression gate + (on pass) flip default-ON — controller-run

**Files:**
- Create: `docs/mrv-ordering-gate-results-2026-06-23.md`
- Modify (only on pass): `crates/owl-dl-reasoner/src/lib.rs` (`hyper_mrv_ordering_enabled` default)

- [ ] **Step 1: Flag-OFF baseline byte-identical**

Run (flag unset): `cargo test --workspace` → green. Confirms the OFF path is unchanged.

- [ ] **Step 2: Corpus FP=0 gate, flag ON**

Run `konclude_closure_diff` with `RUSTDL_MRV_ORDERING=1` over all oracled fixtures. For the fast ones use the normal deadline; for the slow cardinality/disjunction fixtures (wine, sio, ore) use a **tight per-pair deadline** (`RUSTDL_TEST_PAIR_MS=25`) — the spike showed wine FP-signals reproduce in ~56 s there and MRV collapses the real subsumptions fast so MISSED=0 still holds:
```bash
export RUSTDL_MRV_ORDERING=1
# fast fixtures (normal deadline)
RUSTDL_TEST_PAIR_MS=1000 cargo test -p owl-dl-reasoner --test konclude_closure_diff -- --ignored --nocapture \
  bibtex_ pizza_ ro_ sulo_ galen_ notgalen_ ore_15672
# heavy fixtures (tight deadline)
RUSTDL_TEST_PAIR_MS=25 cargo test -p owl-dl-reasoner --test konclude_closure_diff -- --ignored --nocapture \
  sio_ ore_10908 wine_
```
Expected: every fixture **FP=0 / MISSED=0** byte-identical. **Any FP or MISS is a stop** — record which, do not flip default-ON, diagnose.

- [ ] **Step 3: Wall no-regression + wine improvement**

With `RUSTDL_MRV_ORDERING` OFF then ON, measure per-pair-probe wall on the hard wine models (`decide_pair_probe(AlsatianWine, AmericanWine)`, `sat_class_probe(SweetWine)`) and a couple of sio/ore classes (reuse the big-stack-thread / adaptive-budget-OFF probe shape). Confirm: wine collapses (matches the spike's ~1227 / ~12366 branches), and no measured fixture regresses materially.

- [ ] **Step 4: Verdict doc**

`docs/mrv-ordering-gate-results-2026-06-23.md`: per-fixture FP/MISSED table (flag ON), the wall OFF-vs-ON table, and the GO/NO-GO: flip default-ON iff FP=0/MISSED=0 corpus-wide AND no regression AND wine collapse confirmed.

- [ ] **Step 5: On pass — flip default-ON**

Change `hyper_mrv_ordering_enabled` to default-ON (mirror `hyper_precise_card_deps_enabled`'s `is_none_or(|v| v != "0" && !v.is_empty())`), re-run `cargo test --workspace` + the corpus gate with the new default, confirm still FP=0, commit. If any fixture FPs/regresses: leave default OFF, record, diagnose.

- [ ] **Step 6: Commit verdict (+ flip if pass)**

```bash
git add docs/mrv-ordering-gate-results-2026-06-23.md crates/owl-dl-reasoner/src/lib.rs
git commit  # "feat+docs(wedge): MRV corpus gate verdict + (pass) default-ON" + trailers
```

---

## Self-Review

**1. Spec coverage:** MRV in find_open_disjunction (Task 2) ✓; head_atom_satisfied extraction (Task 1) ✓; `RUSTDL_MRV_ORDERING` default OFF + byte-identical (Task 1) ✓; soundness = verdict-invariant (no code, argued in spec; gate proves) ✓; corpus FP=0/MISSED=0 gate + tight-deadline note (Task 3) ✓; wall no-regression + wine improvement (Task 3) ✓; default-decided-by-gate flip (Task 3) ✓; unit selection test (Task 2) ✓.

**2. Placeholder scan:** Task 2's test-seeding note ("if find_open_disjunction_for_test returns None, seed A via new's path") is a concrete verification against real seeding, not unspecified logic. `FIXPOINT_ITERS` is the real const (hyper.rs:50). No "TBD"/"handle errors".

**3. Type consistency:** `mrv_ordering: bool`, `with_mrv_ordering`, `hyper_mrv_ordering_enabled`, `head_atom_satisfied(ci,k,xnode,binding)`, `find_open_disjunction -> Option<(usize, HNode, Binding)>`, the `best: Option<(usize,(usize,HNode,Binding))>` MRV pattern — consistent across Tasks 1–3. The MRV branch + head_atom_satisfied are the exact validated forms from the combination spike.
