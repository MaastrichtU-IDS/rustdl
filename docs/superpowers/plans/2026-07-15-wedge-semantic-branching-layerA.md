# Wedge Semantic Branching — Layer A (dead-disjunct pruning + unit forcing) — Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Give the wedge in-search disjointness pruning at the `⊔` decision point (Fix #2, Layer A): before branching, drop disjuncts that would immediately clash on `disjoint_pairs`, force a lone survivor without a branch, and fail fast if none survive. **Verdict-preserving — cannot MISS or FP.** Behind default-OFF `RUSTDL_SEMANTIC_BRANCHING`. Layer B (semantic branching via exclusion) is a SEPARATE plan, gated on Layer A's `ore_ont_10019` measurement.

**Architecture:** All changes in `crates/owl-dl-tableau/src/hyper.rs` `solve` (`~:2259-2315`), reusing the existing `live: Vec<usize>` slot and the already-built `self.disjoint_pairs`. Env flag mirrors `incremental_fixpoint` (field + builder + reasoner env-fn + classify-site wiring).

**Tech Stack:** Rust (`owl-dl-tableau`, `owl-dl-reasoner`, bin `rustdl`). Tests via `cargo test`.

## Global Constraints

- **always** `RUSTUP_TOOLCHAIN=stable cargo …`; rebuild `-p owl-dl-cli` before probe/matrix runs (stale-binary trap).
- Branch: `feat/wedge-semantic-branching` (off `main`; spec committed). Do NOT work on `main`.
- **Layer A is verdict-preserving.** Gate: FP=0 AND **byte-identical** curated closures (MISSED=0) OFF vs ON — a stronger bar than "no regression". Include the non-Horn `ore_ont_13723` oracle in the FP gate.
- Default-OFF flag first; flip default-ON only in a separate reviewed commit after gates green (and only if Layer A alone warrants it — likely it won't move `ore_ont_10019` much on its own; that's expected, Layer B is the mover).
- `clippy -D warnings` + `fmt --check` clean every commit. Test modules may carry `#![allow(clippy::unwrap_used)]`.

## Ground truth (verified 2026-07-15; re-confirm line numbers, they drift)

- `solve` disjunction block `hyper.rs:~2259-2315`: `find_open_disjunction()` → `(ci, node, binding)`; `d = init_depth - depth`; `head_len = self.clauses[ci].head.len()`; `let live: Vec<usize> = if sat_lookahead { lookahead_live_disjuncts } else { (0..head_len).collect() }`; `if live.is_empty() { clash_deps = body_deps; return Unsat }`; `for k in live { head_atom = clauses[ci].head[k]; save; apply_head_atom(head_atom, node, &binding, decision_deps); match solve(depth-1) {…} }`.
- `self.disjoint_pairs: Arc<HashSet<(u32,u32)>>` — keyed `(lo,hi)` by `ClassId::index()` (`build_disjoint_pairs:890`). Already consulted in the `≤n` path (`labels_disjoint`/`must_be_distinct` `~:2704`) — reuse that check style.
- `Atom` (owl-dl-core `clause.rs`): `Atom::Class(ClassId, Var)` is the atomic-disjunct shape. `HyperNode.labels: Vec<ClassId>` (`hyper.rs:176`); `self.nodes[node.index()].has(c)` / `.labels`.
- `apply_head_atom(head_atom, node, &binding, deps)` asserts a head atom; `solve` runs `horn_fixpoint` at entry then finds the next open disjunction.
- Flag-scaffold precedent: `incremental_fixpoint` field/builder (`with_incremental_fixpoint`) + `incremental_fixpoint_enabled()` in `reasoner/lib.rs` wired at the classify `HyperEngine::new*` sites.

---

### Task 1: `RUSTDL_SEMANTIC_BRANCHING` flag scaffold (default-OFF, inert)

**Files:** `crates/owl-dl-tableau/src/hyper.rs` (field + builder + constructors); `crates/owl-dl-reasoner/src/lib.rs` (env fn + classify-site wiring).

**Interfaces:** produces `HyperEngine::with_semantic_branching(self) -> Self` + field `semantic_branching: bool` (default false in all 3 constructors); `semantic_branching_enabled() -> bool` reading `RUSTDL_SEMANTIC_BRANCHING` (default OFF: `is_some_and(|v| v != "0" && !v.is_empty())`), applied at the same classify builder sites as `incremental_fixpoint`.

- [ ] **Step 1:** Add `semantic_branching: bool` beside `incremental_fixpoint` in `HyperEngine`; init `false` in all 3 constructors; add `#[must_use] pub fn with_semantic_branching(mut self)->Self { self.semantic_branching = true; self }`. Add `semantic_branching` to the `struct_excessive_bools` allow-reason list.
- [ ] **Step 2:** Add `semantic_branching_enabled()` in `reasoner/lib.rs` (mirror `incremental_fixpoint_enabled`, default OFF). Apply `if crate::semantic_branching_enabled() { engine = engine.with_semantic_branching(); }` at every classify `HyperEngine::new*` site where `incremental_fixpoint` is applied (grep `with_incremental_fixpoint`).
- [ ] **Step 3:** Field is read nowhere yet (Task 2 reads it). If `dead_code` trips under clippy, add scoped `#[allow(dead_code)]` + `// consumed in Task 2` on the field.
- [ ] **Step 4:** `RUSTUP_TOOLCHAIN=stable cargo build -p owl-dl-tableau -p owl-dl-reasoner`; fmt + clippy `-D warnings` clean. Commit: `feat(hyper): default-OFF RUSTDL_SEMANTIC_BRANCHING flag scaffold (Fix#2 Layer A)`.

### Task 2: Layer A — disjoint-prune + unit-force at the `⊔` decision

**Files:** `crates/owl-dl-tableau/src/hyper.rs` (`solve` disjunction block); test `crates/owl-dl-tableau/tests/semantic_branching.rs` (create).

**Interfaces:** consumes `self.disjoint_pairs`, `self.clauses`, `HyperNode.labels`, `apply_head_atom`. Produces the flag-gated pruning; a `SearchStats` counter `semantic_prunes: u64` + `semantic_unit_forces: u64` (optional, for measurement).

**The change (inside `solve`, only when `self.semantic_branching`), after `live` is computed and before the `if live.is_empty()` check:**
- Filter `live` to drop each `k` where `self.clauses[ci].head[k]` is `Atom::Class(c, _)` **and** the node (resolved) carries some label `e` with `(min(c.index(),e.index()), max(..)) ∈ self.disjoint_pairs`. (Non-`Class` / compound head atoms are never dropped — conservative.) Increment `semantic_prunes` per dropped `k`.
- Then:
  - `live` empty → the existing `clash_deps = body_deps; return Unsat` (now also reached when disjointness killed all disjuncts — sound: each was an immediate clash).
  - `live.len() == 1` → **unit-force, no branch/decision level:** let `k` be the survivor; `self.apply_head_atom(self.clauses[ci].head[k], node, &binding, body_deps)` (deps = `body_deps`, NOT `decision_deps` — no decision was made); `self.semantic_unit_forces += 1`; then `return self.solve(depth)` (SAME `depth` — no decrement; the forced assertion is monotone, and `solve` re-runs `horn_fixpoint` + finds the next disjunction; the forced disjunct is now head-satisfied so it is not re-picked → terminating). Do NOT `track_depth`.
  - else → branch over the filtered `live` exactly as today (save/apply/`solve(depth-1)`/backjump).

**Soundness note for the implementer:** a disjunct dropped here is one the reactive `horn_fixpoint` would have clashed on the next pass anyway (both its class and a told-disjoint class are/would-be on the node) — so pruning it changes no verdict. Unit-forcing asserts a disjunct the search was forced to take (all siblings are immediate clashes) — also verdict-preserving. **No negative/exclusion state is introduced (that is Layer B).**

- [ ] **Step 1 (RED):** In `semantic_branching.rs`, build a tiny clause set: `Disjoint(B,C)` (⊥-headed `B⊓C→⊥`), a node seeded with `{A, B}`, and a covering clause `A → B ∨ C`. With the flag ON, `solve` must (a) drop disjunct `C`? no — wait: node has `A,B`; disjunct `C` is disjoint with `B` (on node) → drop `C`; disjunct `B` is already satisfied. Construct so exactly one disjunct survives and is forced, OR all-die → Unsat. Assert: flag-ON verdict == flag-OFF verdict (Sat/Unsat identical), and `semantic_prunes >= 1` (non-vacuous: pruning actually happened). Run → FAIL (flag/counter absent).
- [ ] **Step 2:** Implement the filter + unit-force + counters per the design above.
- [ ] **Step 3 (GREEN):** `cargo test -p owl-dl-tableau --test semantic_branching` → PASS (verdict identical, prune fired).
- [ ] **Step 4 (verdict-identity gate):** rebuild CLI; add/reuse a differential test (mirror `incremental_fixpoint_identity.rs`) comparing `classify` OFF vs ON on `funcmerge-cyclic`, `pizza`, `27_eight_way_disjunction_sat`, `18_diamond_subsumption_unsat` — **byte-identical** (Layer A is verdict-preserving). Full tableau suite green (OFF path unchanged). fmt/clippy clean.
- [ ] **Step 5:** Commit: `feat(hyper): Layer A disjoint-prune + unit-force at ⊔ under RUSTDL_SEMANTIC_BRANCHING (Fix#2)`.

### Task 3: Gate + measure `ore_ont_10019`; decide Layer B

**Files:** `docs/2026-07-15-semantic-branching-findings.md` (create).

- [ ] **Step 1 (curated byte-identical gate):** `RUSTUP_TOOLCHAIN=stable cargo build --release -p owl-dl-cli -p owl-dl-bench`; `RUSTDL_SEMANTIC_BRANCHING=1 owl-dl-bench matrix --tier curated --pair-timeout-ms 1000 --global-timeout-s 120`; confirm every rustdl `fp=0` and `missed=0` and closures byte-identical to OFF (Layer A must not change any verdict). **If any differ, STOP** — Layer A is not verdict-preserving as built; fix Task 2.
- [ ] **Step 2 (non-Horn FP oracle):** `ore_ont_13723` via `konclude_closure_diff::ore_one_closure_matches_oracle`, OFF vs ON, FP=0 both.
- [ ] **Step 3 (measure `ore_ont_10019`):** `for f in 0 1; do gtimeout -s KILL 120 env RUSTDL_SEMANTIC_BRANCHING=$f RUSTDL_AGGREGATE_DEADLINE_MS=60000 ./target/release/rustdl classify ~/data/ore-run/input/ore_ont_10019.ofn --pair-timeout-ms 250 2>&1 | grep -iE 'incomplete|# classes|direct|real|semantic'; done`. Record incomplete-pair count + decided classes OFF vs ON + `semantic_prunes`/`semantic_unit_forces`.
- [ ] **Step 4 (findings + Layer-B decision):** write `docs/2026-07-15-semantic-branching-findings.md`. Layer A alone is **expected to move little** (the reactive fixpoint already caught these; the win is fewer save/restores + unit-forcing) — that is NOT a failure signal; it validates the mechanism + gate for Layer B. Decision: proceed to **Layer B** (separate plan — the per-node exclusion set with the `Unsat`-only-exclusion invariant, the real mover) unless Layer A already shows a material `ore_ont_10019` win (then re-measure appetite). Commit findings.

---

## Self-review notes

- **Spec coverage:** Layer A (spec §Layer A) → Tasks 1-3. Verdict-preserving gate (byte-identical curated + non-Horn oracle) → Task 3 Steps 1-2. `ore_ont_10019` measurement + Layer-B gate → Task 3 Steps 3-4. Layer B + the `Unsat`-only-exclusion invariant are a SEPARATE plan (spec §Layer B / §Soundness invariant), written after this measures.
- **No placeholders:** the `solve` hook is specified concretely (filter `live` by `disjoint_pairs`, empty→Unsat, single→unit-force at same depth, else→branch); the unit-force's same-depth recursion + termination argument is stated.
- **Reuse-first:** `disjoint_pairs`, the `live` slot, `apply_head_atom` all exist; new code is the filter + unit-force + counters + tests.
- **Open confirmations the implementer resolves in-task:** exact `Atom::Class` destructuring + the `labels_disjoint`-style disjoint check to reuse (Task 2); whether unit-force `return self.solve(depth)` needs the forced disjunct marked head-satisfied to avoid re-pick (it is, once asserted — confirm via `any_head_satisfied`); the classify `with_incremental_fixpoint` sites to mirror (Task 1 Step 2).
