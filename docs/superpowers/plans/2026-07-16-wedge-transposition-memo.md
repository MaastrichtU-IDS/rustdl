# Wedge within-search transposition memo (Phase 1b) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Make the `ore_ont_10019` residual-stall classes (`AcylGroup`, `KetoneGroup`) terminate by memoizing solve-frame verdicts within a single `decide()`, keyed on an EXACT graph state — collapsing the no-progress transposition (AcylGroup re-derives 2 distinct states 768×). Tractability-only; must not trade soundness for speed.

**Architecture:** At `solve` entry (after `horn_fixpoint` returns `Sat`, before `find_open_disjunction`), compute an EXACT, index-dependent canonical-node-state key (verdict-affecting state only; deps excluded). A per-`decide` `HashMap` memoizes the frame's terminal verdict. On a hit (verified by exact key equality — no hash-collision risk), `Sat` short-circuits and `Unsat` is reused with `DepSet::ALL`. Behind default-OFF `RUSTDL_WEDGE_TRANSPOSITION`.

**Tech Stack:** Rust (`owl-dl-tableau`, `owl-dl-reasoner`, bin `rustdl`).

## Global Constraints

- **always** `RUSTUP_TOOLCHAIN=stable cargo …` via `$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin` on PATH; rebuild `-p owl-dl-cli -p owl-dl-bench` before matrix/measure (stale-binary trap).
- Branch `feat/hard-antecedent-surrogate-absorption` (or a fresh child); do NOT work on main.
- **This is a TRACTABILITY fix over classes that already yield correct subsumptions (zero MISSED). Soundness is non-negotiable:** a memo hit that reuses a wrong verdict is a false `Unsat` → wrongly-not-subsumed, INVISIBLE to FP=0-vs-oracle unless the pair is exercised. Gate: FP=0 on the non-Horn `ore_ont_13723` oracle + curated MISSED=0 byte-identical + a dedicated adversarial canary that a memo hit cannot turn a real clash into a skip.
- **Soundness of the key (load-bearing):** the key must EXACTLY capture all verdict-affecting state, and hits must be verified by exact equality (NOT hash-only — a collision = wrong reuse = FP). Verdict-affecting = per canonical node: sorted `labels`, sorted `at_most`, sorted outgoing `edges` (role, target-index), and `≠` membership. `DepSet`/`label_deps`/`birth_deps` are EXCLUDED (they affect backjumping, not the Sat/Unsat verdict). Index-DEPENDENT is sound (exact state → exact verdict) and catches the observed transposition (AcylGroup's 4 physical nodes recur with different label combos); it may miss cross-numbering transpositions — that only loses benefit, never soundness.
- **Worklist/incremental — RESOLVED, no gate needed:** the advisor flagged the `worklist` as part of saved/restored state under incremental. But the memo point is *after* `horn_fixpoint` returns `Sat`, and `horn_fixpoint` returns `Sat` ONLY when the worklist is drained empty (`while let Some(ev)=worklist.pop(){…}; Sat` — verified `hyper.rs:1637-1646`; `Unsat`/`Stalled` return early, `Sat` iff empty). So at every memo point the worklist is empty ⟹ the graph state alone fully determines the subtree, under BOTH incremental settings. The memo therefore engages in the default classify path (incremental ON) — no `!incremental_fixpoint` gate, no worklist in the key. (State the empty-worklist invariant in a code comment at the memo site.)
- `clippy -D warnings` + `fmt --check` clean every commit.

## Ground truth (verified 2026-07-16)

- `solve` (`hyper.rs`): after `match self.horn_fixpoint(FIXPOINT_ITERS) { … Sat => {} }` (~:2384) and before `find_open_disjunction` — the memo-key/lookup site. The frame returns `Sat`/`Unsat`/`Stalled`; memoize only the terminal `Sat`/`Unsat`.
- `HyperNode` fields (`hyper.rs:173+`): `labels: Vec<ClassId>` (sorted), `at_most: Vec<(Role,Option<ClassId>,u32)>`, `edges: Vec<(Role,HNode)>`. `neq: Vec<(HNode,HNode)>` on the engine. `representative` (union-find) for canonicity.
- `decide_with_deadline` (`hyper.rs:~1915`): resets `stats`; the per-`decide` memo is reset here.
- Flag precedent: `incremental_fixpoint` field + `with_incremental_fixpoint` builder + `incremental_fixpoint_enabled()` (`reasoner/lib.rs:1680`), wired at classify `HyperEngine::new*` sites.
- B0.5: AcylGroup repeat_frac 0.997 (2 distinct states), KetoneGroup 0.816 (3770). `n_nodes` pinned at 4 for AcylGroup ⟹ index-dependent exact key catches it.

---

### Task 1: `RUSTDL_WEDGE_TRANSPOSITION` flag scaffold (default-OFF, inert)

**Files:** `crates/owl-dl-tableau/src/hyper.rs` (field + builder + 3 constructors); `crates/owl-dl-reasoner/src/lib.rs` (env fn + classify wiring).

- [ ] **Step 1:** Add `transposition: bool` beside `incremental_fixpoint` in `HyperEngine`; init `false` in all 3 constructors; add `#[must_use] pub fn with_transposition(mut self)->Self{self.transposition=true;self}`. Add `transposition` to the `struct_excessive_bools` allow-reason. `#[allow(dead_code)]` + `// consumed in Task 3` on the field.
- [ ] **Step 2:** Add `transposition_enabled()` in `reasoner/lib.rs` (default OFF: `is_some_and(|v| v!="0" && !v.is_empty())`); apply `if crate::transposition_enabled(){engine=engine.with_transposition();}` at every classify site that applies `with_incremental_fixpoint` (grep it).
- [ ] **Step 3:** `RUSTUP_TOOLCHAIN=stable cargo build -p owl-dl-tableau -p owl-dl-reasoner`; clippy `-D warnings` + fmt clean. Commit: `feat(hyper): default-OFF RUSTDL_WEDGE_TRANSPOSITION flag scaffold (Phase 1b)`.

### Task 2: exact canonical-state key + per-decide memo table

**Files:** `crates/owl-dl-tableau/src/hyper.rs`; test inline `#[cfg(test)]`.

**Interfaces:** produces `fn state_key(&self) -> Vec<NodeKey>` where `NodeKey = (Vec<u32> labels, Vec<(u32,Option<u32>,u32)> at_most, Vec<(u32,u32)> edges, Vec<u32> neq_partners)` (all sorted, canonical nodes only, deps excluded); an engine field `transposition_memo: HashMap<u64, Vec<(Vec<NodeKey>, HyperResult)>>` (hash → bucket of (exact-key, verdict) for collision-safe verify); reset in `decide_with_deadline`.

- [ ] **Step 1 (RED):** inline test `transposition_key_ignores_deps_and_node_order_stability`: build a tiny engine, assert `state_key()` is stable across a `save`/`restore` round-trip and changes when a label is added. Run → FAIL (no `state_key`).
- [ ] **Step 2:** implement `state_key()`: iterate canonical nodes (`resolve(n)==n`), per node collect sorted `labels` (as `u32`), sorted `at_most` (role-id, qual-index, n), sorted `edges` (`role_id_index`, `resolve(target).index()`), and sorted `≠`-partners (resolved). Push per-node tuples in node-index order. Deps excluded.
- [ ] **Step 3:** add the `transposition_memo` field (3 constructors init empty) + reset in `decide_with_deadline` (only meaningful when `transposition`). Hash helper `hash_state_key(&[NodeKey]) -> u64` (DefaultHasher).
- [ ] **Step 4 (GREEN):** test passes. clippy/fmt clean. Commit: `feat(hyper): exact canonical state key + per-decide transposition memo table`.

### Task 3: memo lookup/store in `solve` + soundness canary (TDD)

**Files:** `hyper.rs` `solve`; tests `crates/owl-dl-tableau/tests/transposition_memo.rs` (create).

**The change (inside `solve`, when `self.transposition`), right after the `horn_fixpoint … Sat => {}` block (where the worklist is provably empty — see Global Constraints):**
- Compute `key = self.state_key()`, `h = hash_state_key(&key)`.
- Lookup: if the memo bucket for `h` contains an entry whose stored key EQUALS `key` (exact `Vec<NodeKey>` comparison — collision-safe), reuse its verdict: `Sat` → `return HyperResult::Sat`; `Unsat` → `self.clash_deps = DepSet::ALL; return HyperResult::Unsat`.
- Else: recurse the rest of `solve` normally; capture its result `r`; if `r` is `Sat` or `Unsat` (NOT `Stalled` — stall is budget-dependent, not a true verdict), insert `(key, r)` into the bucket. Return `r`.
  (Refactor: split the post-`horn_fixpoint` body into an inner call so the result can be intercepted, or set a local and fall through to a single return.)

- [ ] **Step 1 (RED — the mover):** hand-built clauses forming a transposition (two independent covering disjunctions `X→A∨B`, `X→C∨D`, whose 4 choice-orders reach overlapping states), such that flag-ON revisits a state and reuses it. Assert flag-ON verdict == flag-OFF verdict AND a memo-hit counter ≥ 1. (Add `SearchStats::transposition_hits: u64`.)
- [ ] **Step 2 (RED — the FP tripwire, the load-bearing canary):** two states that share a hash but differ in an `at_most`/`≠`/`edge` field where one is `Sat` and the other `Unsat`; assert the flag-ON verdict is NOT corrupted (the exact-key verify must reject the false hit). Prove discriminating: temporarily key on the hash ONLY (skip the exact verify) → the canary flips to a wrong verdict; restore exact verify → correct.
- [ ] **Step 3:** implement lookup/store + `transposition_hits`.
- [ ] **Step 4 (GREEN):** both canaries pass. Full tableau suite green (OFF path unchanged). clippy/fmt clean. Commit: `feat(hyper): within-search transposition memo in solve under RUSTDL_WEDGE_TRANSPOSITION (Phase 1b)`.

### Task 4: gate + measure; GO/NO-GO (bound-the-probes fallback)

**Files:** `docs/2026-07-16-transposition-memo-findings.md` (create).

- [ ] **Step 1 (soundness gate — non-negotiable):** rebuild release `-p owl-dl-cli -p owl-dl-bench`. FP=0 on the non-Horn `ore_ont_13723` oracle (OFF vs ON). Curated MISSED=0 byte-identical (galen/notgalen/sio/wine/ore-15672/ore-10908/alehif/pizza), OFF vs ON. **Any FP or MISSED regression → STOP; the key is missing a verdict-affecting field — fix `state_key` (audit for any state a rule reads that isn't in the key).**
- [ ] **Step 2 (measure ore_ont_10019):** `hyper-sat` OFF vs ON — do AcylGroup/KetoneGroup stalls clear? memo hit-rate (`transposition_hits`)? The memo engages under the default (incremental ON) since the worklist is empty at the memo point. Record whether the defined-sweep (`RUSTDL_CLASSIFY_DEFINED_SWEEP=1`) then becomes affordable (was 263 s).
- [ ] **Step 3 (GO/NO-GO):**
  - **GO:** the memo makes the 2 probes terminate AND the soundness gate is green AND the defined-sweep becomes affordable → proceed to Track A (enable/refine the sweep to recover the 2/3 MISSED).
  - **NO-GO / not-clean:** the memo doesn't cleanly terminate the probes, or the incremental interaction is messy → **FALLBACK: bound the two probes.** They lose ZERO completeness; make their satisfiability probe return `Stalled`→treated-satisfiable fast (or cap those specific pairs), and document that `ore_ont_10019`'s 2 residual stalls are bounded (wall-only) and the 3 MISSED await Track A via a separately-scoped affordable sweep. A legitimate, evidence-backed outcome.
- [ ] **Step 4:** write findings; advisor pass on the shipped soundness (key completeness) before any default-ON flip.

---

## Self-review notes

- **Spec coverage:** transposition memo (residual-plan Track B / B1) → Tasks 1-3; soundness gate + adversarial canary + fallback → Task 4. The count-based `AtLeast` shortcut is deliberately NOT in the plan (advisor: unsound).
- **No placeholders:** the key contents (labels+at_most+edges+≠, deps excluded), the exact-verify-on-hit, Sat-short-circuit / Unsat-`DepSet::ALL`, and the `!incremental_fixpoint` gate are all concrete.
- **Load-bearing soundness:** Step 2 canary in Task 3 (exact-verify rejects a hash collision) + Task 4 Step 1 (FP oracle + curated MISSED=0) are the FP net; the fallback removes the risk entirely if the memo isn't clean.
