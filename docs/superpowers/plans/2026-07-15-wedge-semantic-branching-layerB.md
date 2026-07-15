# Wedge Semantic Branching — Layer B (per-node exclusion set) — Plan

**Goal:** Give the wedge true semantic branching: when a prior sibling disjunct `Dⱼ`
returns a **clean `Unsat`**, assert `¬Dⱼ` (exclude its class) before trying later
siblings, converting the syntactic branch `D₁|D₂|D₃` into the sound partition
`D₁ | ¬D₁∧D₂ | ¬D₁∧¬D₂∧D₃`. Each asserted `¬Dⱼ` propagates through the disjointness
axioms (via Layer A's prune/force), collapsing downstream disjunctions to unit. This
is the intended mover for `ore_ont_10019`. Behind the SAME default-OFF flag
`RUSTDL_SEMANTIC_BRANCHING` (Layer A + B ship under one flag).

**Spec:** `docs/superpowers/specs/2026-07-15-wedge-semantic-branching-design.md` §Layer B
+ §Soundness invariant. **Builds on Layer A** (committed: `9fe0fa9`).

## ⚠️ THE LOAD-BEARING SOUNDNESS INVARIANT (reuse-trap family)

Exclude a sibling's class **ONLY if that sibling returned `Unsat`, NEVER `Stalled`.**
Under a deadline / `is_diverging`, branches stall routinely; excluding a merely-
*stalled* disjunct asserts an unproven `¬Dⱼ` → false clash → **unsound → FP
subsumption** (the same hazard as `reuse-trap-A1` / the snapshot-cache soundness fix).
If any sibling returns `Stalled`, the frame's result is `Stalled` with **NO exclusion
added from it**. Atomic `Class` disjuncts only (compound `∃`/`Q` disjuncts have no
single class to exclude — stay live).

**Dep discipline (superset, one level worse than Layer A):** the exclusion of `Dⱼ`
carries `Dⱼ`'s Unsat clash dep-set `child_depsⱼ`. When a later branch re-derives `Dⱼ`
(adds its class to the excluded node), the manufactured clash's dep-set must be
`deps(the re-derived label) ∪ child_depsⱼ` — a SUPERSET, so backjumping accounts for
every decision the exclusion's validity rests on. A subset → unsound backjump → FP.

## Architecture / ground truth (verified 2026-07-15)

- `HyperNode` (`hyper.rs:173`): add `excluded: Vec<(ClassId, DepSet)>` (sorted by
  `ClassId::index()` for O(log n) membership; carries the exclusion's dep-set). Rides
  the whole-node-clone `Snapshot { nodes: self.nodes.clone() }` (`save`/`restore`) — **NO
  `trail.rs` change**. Default empty; inert when the flag is off (never written).
- Clash chokepoint: `process_event(Event::Label(n, c))` (`hyper.rs:1657`) fires when a
  label is added; a `FireOutcome::Clash` return propagates to `horn_fixpoint` → `Unsat`.
  **Layer B clash hook:** at the top of that arm, if `c` (resolved node) is in
  `excluded`, set `self.clash_deps = self.nodes[rn].deps_of(c).union(excl_dep)` and
  return `FireOutcome::Clash`. (Also guard `apply_nn_rule`/merge paths that add labels
  without an `Event::Label` — audit `add_label` callers; `add_label` is the single
  label chokepoint per its docstring, and every add emits `Event::Label`, so the
  process_event hook covers them. Confirm no label add bypasses `add_label`.)
- Exclusion is consulted in the `⊔` decision (Layer A's filter, `hyper.rs` `solve`):
  extend the drop test — a disjunct `Class(c,_)` is dead if disjoint with a label
  **OR** `c ∈ excluded(landing node)`; fold `excl_dep(c)` into `prune_deps` on that
  drop (same superset accounting as the disjoint case).
- The branch loop (`for k in live`, `hyper.rs` `solve`): after a sibling's clean
  `Unsat` + `restore`, add the exclusion BEFORE the next iteration's `save`.

## Global Constraints

- **always** `RUSTUP_TOOLCHAIN=stable cargo …` via `$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin` on PATH; rebuild `-p owl-dl-cli -p owl-dl-bench` fresh before matrix/measure (stale-binary trap).
- Branch `feat/wedge-semantic-branching` (do NOT work on main).
- **Gate (SOUNDNESS — non-negotiable):** default-OFF flag → **FP=0** on curated AND the non-Horn `ore_ont_13723` oracle + a **dedicated `Stalled`-sibling-never-excluded canary** (the FP tripwire) + a **superset-dep canary** (excluded-class re-derivation carries `child_depsⱼ`). Unlike Layer A, Layer B is NOT verdict-preserving — it can DECIDE more (recover MISSED), so the curated gate is **FP=0 + MISSED=0** (byte-identical is NOT required — new sound subsumptions are allowed, but curated is already MISSED=0 so in practice expect byte-identical there; the win shows on `ore_ont_10019`).
- `clippy -D warnings` + `fmt --check` clean every commit.

## Tasks

### Task 1: `excluded` node state + the clash hook (inert until Task 2 writes it)

**Files:** `crates/owl-dl-tableau/src/hyper.rs`.

- [ ] Add `excluded: Vec<(ClassId, DepSet)>` to `HyperNode` (doc: Layer B semantic-branching exclusion set; empty default via `#[derive(Default)]`/manual). Add helpers: `is_excluded(&self, c) -> Option<DepSet>` (binary-search), `exclude(&mut self, c, deps)` (insert sorted, keep-first or widen — decide: keep-first is fine, the first exclusion's dep is the tightest sound one).
- [ ] Clash hook in `process_event(Event::Label(n, c))` at the top (after resolve): `if self.semantic_branching { if let Some(xd) = self.nodes[rn].is_excluded(c) { self.clash_deps = self.nodes[rn].deps_of(c).union(xd); return FireOutcome::Clash; } }`. Confirm `rn = resolve(n) iff inverse_func_merge` (mirror `add_label`).
- [ ] `excluded` participates in `save`/`restore` automatically (whole-node clone) — verify `Snapshot` clones `nodes` (it does). No trail change.
- [ ] Guard: field read only under `self.semantic_branching`; flag-OFF path byte-identical. Build + clippy + fmt clean. Commit.

### Task 2: exclude-on-clean-Unsat in the branch loop + liveness integration (TDD)

**Files:** `hyper.rs` `solve`; tests `tests/semantic_branching.rs`.

- [ ] **RED canary A (the mover works):** hand-built clauses where excluding a clean-`Unsat` sibling collapses a downstream disjunction to unit and DECIDES a pair the flag-OFF search leaves stalled (or needs more branches). Assert flag-ON decides it; flag-OFF (Layer A only or plain) does not within the same small depth. (Model on `ore_ont_10019`'s covering+disjointness shape.)
- [ ] **RED canary B (the `Stalled` FP tripwire):** a disjunct that only *stalls* (force a tiny `depth`/deadline so a sibling returns `Stalled`), and asserting its negation WOULD clash. Assert flag-ON verdict is NOT flipped to a false `Unsat` (must stay `Sat`/`Stalled`) — i.e. a `Stalled` sibling is never excluded. Prove discriminating (inject "exclude on Stalled too" → canary flips to Unsat).
- [ ] **RED canary C (superset dep):** excluding `Dⱼ` then re-deriving `Dⱼ`'s class in a later branch must carry `child_depsⱼ` so an ancestor decision is not backjumped past. Model on the Layer A `survivors_remain` / `ancestor` structure; prove discriminating (subset dep → false Unsat).
- [ ] **Implement:** in the branch loop, capture per-sibling result; on `Unsat` (clean, `head[k]` atomic `Class(c,_)`, target resolved), `self.nodes[t].exclude(c, child_deps)` AFTER `restore(saved)` and BEFORE the next `save`. On `Stalled`: set `any_stalled`, add NO exclusion. Extend Layer A's `live` filter to also drop `c ∈ excluded(t)`, folding `excl_dep` into `prune_deps`. Ensure the exclusions added this frame are scoped to it (parent's restore clears them — verify).
- [ ] **GREEN:** all three canaries pass; the Layer A canaries still pass; full tableau suite green; fmt/clippy clean. Commit.

### Task 3: gate + measure `ore_ont_10019`; GO/NO-GO

**Files:** `docs/2026-07-15-semantic-branching-layerB-findings.md`.

- [ ] Build release `-p owl-dl-cli -p owl-dl-bench` fresh.
- [ ] **FP gate:** non-Horn `ore_ont_13723` oracle FP=0 (OFF + ON); curated byte-identity/MISSED=0 OFF vs ON on galen/notgalen/sio/wine/ore-15672/ore-10908/alehif/pizza (both `INVERSE_FUNC_MERGE` settings). **Any FP → STOP, fix.**
- [ ] **Measure `ore_ont_10019`** OFF vs ON: incomplete-pair count + decided classes + wall. **GO/NO-GO:** flag-ON decides **≥ ~half of the 33** stalled classes within the Konclude/HermiT budget (a few seconds), FP=0/MISSED=0.
  - **GO** → corpus FP=0/MISSED=0 → recommend default-ON in a separate reviewed commit.
  - **NO-GO** → STOP → **bound-the-tail**: make the `Stalled → NoVerdict → search.rs` fallthrough return sound-incomplete FAST, document "dense-SROIQ disjunctive tail needs Konclude-class caching/learning, deferred." A legitimate, evidence-backed outcome.
- [ ] Write findings; decide.

## Self-review notes

- Spec coverage: §Layer B (exclusion set) → Tasks 1-2; §Soundness invariant (Unsat-only, atomic-only, superset dep) → Task 2 canaries B+C + the clash hook; §Gate/§go-no-go → Task 3.
- The three canaries are the entire safety net for the reuse-trap FP surface — each must be proven discriminating (RED under the corresponding bug).
