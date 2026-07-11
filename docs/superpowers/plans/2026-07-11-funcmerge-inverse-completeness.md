# Functional-Merge-Across-Inverse Completeness Fix — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make rustdl's hypertableau wedge count inverse-induced role successors when checking `≤n`/functional constraints, so the functional-merge-across-inverse subsumptions it currently misses (galen's 10) are derived — restoring the Horn completeness the engine already claims.

**Architecture:** One targeted change: `distinct_role_succ` (`crates/owl-dl-tableau/src/hyper.rs`) currently scans only a node's outgoing `edges`; extend it to also union incoming `preds` with the role flipped (mirroring the existing `enumerate_matches` pattern) and dedupe by `resolve()`. This makes `find_open_at_most` see the true successor set, so `≤1`/functional merges fire. Because a merge can now fold the root node, also make `root_labels()` `resolve()`-safe and audit other direct label/edge readers.

**Tech Stack:** Rust (edition 2024). Build/test with `RUSTUP_TOOLCHAIN=stable` (the pinned 1.95.0 toolchain lacks the `cargo` binary; a bare build fails or silently reuses a stale binary).

## Global Constraints

- Every cargo command is prefixed `RUSTUP_TOOLCHAIN=stable`.
- **Soundness gate (hard):** corpus-wide FP must stay 0 — the `konclude_closure_diff` suite must show FP=0 on every fixture, unchanged.
- **Completeness gate:** galen `MISSED 10 → 0` vs the Konclude oracle; no *new* MISSED introduced on any other corpus ontology.
- **Termination:** galen classification must still terminate (no blowup from the extra merges).
- Clean `cargo clippy --all-targets --all-features -- -D warnings` and `cargo fmt --check` for every touched crate.
- The fix is minimal and targeted; the general predecessor-aware merge (HF3) is explicitly deferred to a follow-up (Task 4).
- `ontologies/` is gitignored — the regression fixture is an inline OFN string constant in the test, not a committed file.

---

## Task 1: Failing regression test (RED)

**Files:**
- Create: `crates/owl-dl-reasoner/tests/funcmerge_inverse.rs`

**Interfaces:**
- Consumes: `owl_dl_reasoner::classify` (`&SetOntology<RcStr>) -> Result<Classification>`), `Classification::is_subclass(&str, &str) -> bool`; `horned_owl::io::ofn::reader::read`.
- Produces: test `funcmerge_cyclic_derives_a_sub_y` (later tasks don't depend on it).

- [ ] **Step 1: Write the failing test**

Create `crates/owl-dl-reasoner/tests/funcmerge_inverse.rs`:

```rust
//! Regression: functional (≤1) role merge across an inverse-induced edge, in a
//! cyclic model. Konclude derives A ⊑ Y (⊑ Z); rustdl missed it because the
//! wedge's ≤n-successor count ignored inverse-induced successors. See
//! docs/superpowers/specs/2026-07-11-funcmerge-inverse-completeness-design.md.

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::classify;

const FUNCMERGE_CYCLIC: &str = r#"Prefix(:=<http://t/#>)
Ontology(
Declaration(Class(:A))
Declaration(Class(:N))
Declaration(Class(:Y))
Declaration(Class(:Z))
Declaration(Class(:LFC))
Declaration(ObjectProperty(:f))
Declaration(ObjectProperty(:g))
Declaration(ObjectProperty(:h))
SubClassOf(:A ObjectSomeValuesFrom(:f :N))
InverseObjectProperties(:f :g)
FunctionalObjectProperty(:g)
EquivalentClasses(:N ObjectSomeValuesFrom(:g ObjectIntersectionOf(:Y ObjectSomeValuesFrom(:h :LFC))))
SubClassOf(:Y :Z)
EquivalentClasses(:LFC ObjectSomeValuesFrom(:g :A))
)
"#;

fn load(src: &str) -> SetOntology<RcStr> {
    let mut cur = std::io::Cursor::new(src.to_string());
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut cur, ParserConfiguration::default()).expect("parse OFN");
    onto
}

#[test]
fn funcmerge_cyclic_derives_a_sub_y() {
    let onto = load(FUNCMERGE_CYCLIC);
    let c = classify(&onto).expect("classify");
    // A ⊑ Y by the functional merge across the inverse edge; A ⊑ Z since Y ⊑ Z.
    assert!(
        c.is_subclass("http://t/#A", "http://t/#Y"),
        "expected A ⊑ Y (functional-merge-across-inverse)"
    );
    assert!(
        c.is_subclass("http://t/#A", "http://t/#Z"),
        "expected A ⊑ Z (A ⊑ Y ⊑ Z)"
    );
}
```

- [ ] **Step 2: Run the test to verify it FAILS**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test funcmerge_inverse -- --nocapture`
Expected: FAIL — `assertion failed: c.is_subclass("http://t/#A", "http://t/#Y")` (rustdl does not currently derive it).

If instead it errors on the `classify`/`is_subclass` signature, check the real signature in `crates/owl-dl-reasoner/src/lib.rs` (`pub fn classify`) and `Classification::is_subclass`, and adjust the calls to match — do not change the assertions.

- [ ] **Step 3: Commit the failing test**

```bash
git add crates/owl-dl-reasoner/tests/funcmerge_inverse.rs
git commit -m "test(hyper): failing regression for functional-merge-across-inverse (RED)"
```

(Committing a RED test is intentional here — the next task turns it GREEN and the two commits document the fix.)

---

## Task 2: Inverse-aware `distinct_role_succ` + resolve-safe root readers (GREEN)

**Files:**
- Modify: `crates/owl-dl-tableau/src/hyper.rs` — `distinct_role_succ` (~line 2411), `root_labels` (~line 3336)
- Test: `crates/owl-dl-tableau/src/hyper.rs` (`#[cfg(test)]` unit test), and the Task 1 integration test

**Interfaces:**
- Consumes: `role_matches(Role, Role, Option<&RoleHierarchy>) -> bool`, `Role::flip(&self) -> Role`, `self.resolve(HNode) -> HNode`, `HNode::index(&self) -> usize`, `HNode(0)` (root), `self.sub_roles: Option<RoleHierarchy>`, node fields `.edges: Vec<(Role, HNode)>` and `.preds: Vec<(Role, HNode)>`.
- Produces: `distinct_role_succ` now returns forward + inverse-induced successors (deduped by resolve); `root_labels` resolves the root first. Signatures unchanged.

- [ ] **Step 1: Make `distinct_role_succ` inverse-aware**

In `crates/owl-dl-tableau/src/hyper.rs`, replace the body of `distinct_role_succ` (current code scans only `edges`):

```rust
    /// The *distinct* (representative-resolved) `role`-successors of
    /// `node`, filtered by the optional class qualifier. Counts BOTH
    /// outgoing `edges` matching `role` AND incoming `preds` whose role
    /// flips to `role` (an incoming `s —er→ node` asserts `er⁻(node, s)`,
    /// so `s` is a genuine `role`-successor of `node` when `er.flip()`
    /// matches — e.g. a declared inverse `f ≡ g⁻`). Mirrors the
    /// inverse-role handling in `enumerate_matches`. Deduped by
    /// `resolve()` so a node reachable both ways is counted once.
    fn distinct_role_succ(&self, node: HNode, role: Role, qual: Option<ClassId>) -> Vec<HNode> {
        let hier = self.sub_roles.as_ref();
        let mut seen: Vec<HNode> = Vec::new();
        let mut consider = |seen: &mut Vec<HNode>, target: HNode| {
            let rt = self.resolve(target);
            if let Some(q) = qual
                && !self.nodes[rt.index()].has(q)
            {
                return;
            }
            if !seen.contains(&rt) {
                seen.push(rt);
            }
        };
        for (er, t) in &self.nodes[node.index()].edges {
            if role_matches(*er, role, hier) {
                consider(&mut seen, *t);
            }
        }
        for (er, s) in &self.nodes[node.index()].preds {
            if role_matches(er.flip(), role, hier) {
                consider(&mut seen, *s);
            }
        }
        seen
    }
```

Note: the closure borrows `self` immutably (via `self.resolve`/`self.nodes`) while the loops also borrow `self.nodes[..].edges`/`.preds` immutably — all shared borrows, which the current code already relies on. If the borrow checker rejects the closure form, inline the `consider` logic as two explicit loop bodies (identical logic) instead.

- [ ] **Step 2: Run the Task 1 test — expect PASS (or a precise diagnosis)**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test funcmerge_inverse -- --nocapture`
Expected: PASS.

If it still FAILS (merge now fires but `A`'s label isn't visible), the root was folded away and a label reader isn't resolving — proceed to Step 3 (root-reader audit), which is required regardless; then re-run.

- [ ] **Step 3: Make `root_labels` resolve-safe and audit other direct root/label readers**

Replace `root_labels` (a merge can now fold the root, node 0, into another representative):

```rust
    /// Class labels of the root node — the derived subsumers of the root
    /// concept, for EL-closure cross-checks. Resolves the root through the
    /// merge union-find first: a `≤n` merge can fold node 0 into another
    /// representative, so reading `self.nodes[0].labels` directly would be
    /// stale after such a merge.
    #[must_use]
    pub fn root_labels(&self) -> &[ClassId] {
        &self.nodes[self.resolve(HNode(0)).index()].labels
    }
```

Then audit for other unresolved root/label reads:

Run: `grep -n 'nodes\[0\]\|\.labels\b' crates/owl-dl-tableau/src/hyper.rs`
For each hit that reads a node's `.labels` or `nodes[0]` on a correctness path (a returned subsumption / satisfiability verdict) without a preceding `self.resolve(...)`, change it to read through `self.resolve(...)`. Do **not** change reads that are already inside a `resolve`d context or are pure diagnostics/counters. In your report, list every hit and whether you changed it and why.

- [ ] **Step 4: Add a focused unit test for `distinct_role_succ`**

Add to the existing `#[cfg(test)] mod tests` in `crates/owl-dl-tableau/src/hyper.rs` (if none exists, create one with `#![allow(clippy::unwrap_used)]` as its first line, matching repo convention). The test builds a graph where `node` has one forward `edges` successor and one inverse `preds` successor of the same (inverse-related) role and asserts the count is 2:

```rust
    #[test]
    fn distinct_role_succ_counts_inverse_predecessors() {
        // Build a minimal engine graph: node n with a forward g-edge to m,
        // and an incoming f-edge from a (f ≡ g⁻ via canonicalization → the
        // incoming edge's role flips to g). Both m and a are g-successors of n.
        let mut eng = HyperEngine::for_test_empty();
        let n = eng.push_test_node();
        let m = eng.push_test_node();
        let a = eng.push_test_node();
        let g = eng.test_role_g(); // Role keyed on the g/f-inverse role-id
        eng.add_test_edge(n, g, m); // n —g→ m  (n.edges)
        eng.add_test_pred(n, g.flip(), a); // incoming a —g⁻→ n  ⇒ n —g→ a (n.preds)
        let succ = eng.distinct_role_succ(n, g, None);
        assert_eq!(succ.len(), 2, "n has two g-successors: m (edge) and a (inverse pred)");
    }
```

If the engine lacks these `for_test_*`/`push_test_*`/`add_test_*` helpers, use whatever minimal in-crate constructors the existing `hyper.rs` tests already use to build a graph (check the current `#[cfg(test)]` block first and follow its pattern); the assertion — count == 2 for one forward + one inverse successor — is the fixed requirement. If building an engine graph directly is impractical from outside the struct's private API, drop this unit test and rely on the Task 1 integration test plus the corpus gates (Task 3) — note that choice in your report.

- [ ] **Step 5: Run tableau unit tests + lints**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-tableau`
Expected: all pass (including the new unit test if added).

Run: `RUSTUP_TOOLCHAIN=stable cargo clippy -p owl-dl-tableau --all-targets --all-features -- -D warnings`
Run: `RUSTUP_TOOLCHAIN=stable cargo fmt -p owl-dl-tableau -- --check`
Expected: both clean. (If fmt flags only your new lines, run `cargo fmt -p owl-dl-tableau` and re-verify; do not reformat unrelated code.)

- [ ] **Step 6: Commit**

```bash
git add crates/owl-dl-tableau/src/hyper.rs
git commit -m "fix(hyper): count inverse-induced successors in distinct_role_succ so functional/≤1 merges fire (GREEN)"
```

---

## Task 3: Corpus regression + galen completeness gates

**Files:** none modified (verification task). Uses the built binaries and the eval harness.

**Interfaces:** Consumes the fix from Task 2; consumes `owl-dl-bench matrix` (built earlier in this repo) and `~/eval-tools`.

- [ ] **Step 1: Soundness gate — closure-diff suite FP=0**

Run: `RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test konclude_closure_diff --release -- --ignored --nocapture`
Expected: same pass count as before the fix (21/21), every fixture reports `FP=0`, and no fixture's `MISSED` increases relative to its pre-fix value. Record the FP/MISSED line for each fixture in your report. If any fixture gains an FP, STOP — the fix is unsound; report immediately (do not proceed).

(Fixtures under `ontologies/external/` are gitignored and may be absent on this host; those cases print `SKIP` and are not failures. Report which fixtures actually ran.)

- [ ] **Step 2: Completeness + termination gate — galen via the matrix harness**

Rebuild fresh binaries (the freshness guard requires it):
`RUSTUP_TOOLCHAIN=stable cargo build --release -p owl-dl-cli -p owl-dl-bench`

Run the curated matrix (regenerates the authoritative curated results):
`RUSTUP_TOOLCHAIN=stable ./target/release/owl-dl-bench matrix --tier curated --out /tmp/matrix-postfix --pair-timeout-ms 5000 --global-timeout-s 120`

Then verify galen and the soundness of the whole curated tier:
```bash
python3 - <<'PY'
import json
rows=[json.loads(l) for l in open("/tmp/matrix-postfix/results.jsonl")]
g=[c for c in rows if c["ont"]=="galen" and c["reasoner"]=="rustdl"][0]
print("galen rustdl:", g["status"], "FP", g["fp"], "MISSED", g["missed"], "wall_ms", g["wall_ms"])
bad=[(c["ont"],c["fp"]) for c in rows if c["reasoner"]=="rustdl" and c["fp"] not in (0,None)]
print("rustdl FP!=0 cells:", bad)
PY
```
Expected: `galen rustdl: ok FP 0 MISSED 0` (was MISSED 10) with a finite wall (galen terminates); `rustdl FP!=0 cells: []` (soundness preserved across the whole curated tier).

If galen `MISSED` is >0 but <10, report the residual pairs (rerun the earlier `dump`-style diff if needed) — a partial improvement means the fix is correct but incomplete for some pattern; report before proceeding.

- [ ] **Step 3: Record the gate results in the report**

No commit (verification only). Record in the task report: the closure-diff per-fixture FP/MISSED, the galen cell line, and the whole-tier FP check. These results feed the doc corrections in the separate "finish the matrix" work.

---

## Task 4: File the HF3 follow-up (deferred general merge)

**Files:**
- Create: `docs/known-limitations/hf3-general-predecessor-aware-merge.md`

**Interfaces:** none.

- [ ] **Step 1: Write the follow-up note**

Create `docs/known-limitations/hf3-general-predecessor-aware-merge.md`:

```markdown
# HF3 — general predecessor-aware merge (deferred)

**Status:** deferred follow-up (not scheduled).
**Context:** `docs/superpowers/specs/2026-07-11-funcmerge-inverse-completeness-design.md`.

## What is done
The functional/`≤1` merge now counts inverse-induced successors
(`distinct_role_succ` unions `edges` + `preds`/flip), so merges across a
declared-inverse edge fire — closing galen's 10 missed subsumptions
(`funcmerge-cyclic` regression test).

## What remains (HF3)
`merge_with_cause` still does not redirect a folded node's **incoming** edges
(`preds`) to the survivor (see the docstring at `crates/owl-dl-tableau/src/hyper.rs`
`merge_with_cause`, and the `enumerate_matches` comment "merges are
root-successor-only"). This is sound today because a stale predecessor edge
remains a valid R-relationship and label reads go through `resolve()`, but a
*general* predecessor-aware merge (redirecting in-edges) would be needed for
correctness if merges over arbitrary non-root-successor nodes are ever relied
upon beyond the current cases. Revisit if new inverse/merge incompleteness
surfaces on other ontologies.
```

- [ ] **Step 2: Commit**

```bash
git add docs/known-limitations/hf3-general-predecessor-aware-merge.md
git commit -m "docs: file HF3 (general predecessor-aware merge) follow-up"
```

---

## Self-Review

**Spec coverage:**
- Inverse-aware `distinct_role_succ` (edges + preds/flip + dedupe) → Task 2 Step 1. ✓
- Root-folded-merge correctness (`root_labels` resolve-safe + reader audit) → Task 2 Steps 3. ✓
- TDD anchor (inline `funcmerge-cyclic` → A⊑Y/A⊑Z) → Task 1. ✓
- Unit test for `distinct_role_succ` count=2 → Task 2 Step 4. ✓
- Soundness gate corpus FP=0 → Task 3 Step 1. ✓
- galen MISSED 10→0 + terminates → Task 3 Step 2. ✓
- clippy/fmt clean → Task 2 Step 5 (+ constraints). ✓
- HF3 deferred + filed → Task 4. ✓
- gitignored `ontologies/` → fixture inline (Task 1) per Global Constraints. ✓

**Placeholder scan:** No TBD/TODO. Task 2 Step 3's audit is a bounded grep with an explicit change criterion (not a vague "handle edge cases"); Task 2 Step 4 gives a concrete fallback if private-API test construction is impractical. All code steps carry real code.

**Type consistency:** `distinct_role_succ(HNode, Role, Option<ClassId>) -> Vec<HNode>`, `root_labels(&self) -> &[ClassId]`, `resolve(HNode) -> HNode`, `Role::flip`, `role_matches` used consistently and match the code quoted from `hyper.rs`. Test uses `classify` + `Classification::is_subclass(&str,&str)` consistent with the reasoner API.
