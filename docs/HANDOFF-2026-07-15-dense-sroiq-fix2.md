# HANDOFF — dense-SROIQ deep-R&D, Fix #2 (wedge semantic branching)

**Date:** 2026-07-15. **Purpose:** resume this work on another machine. Self-contained
(the `.superpowers/sdd/progress.md` SDD ledger is gitignored scratch and does NOT travel — this
doc supersedes it).

---

## TL;DR — where we are

Closing rustdl's dense-SROIQ completeness tail, exemplar `ore_ont_10019` (47 classes: 14 Sat,
**33 stalled** in the wedge; Konclude decides all in 90 ms, HermiT 360 ms; rustdl stalls). Three
sub-projects are **DONE + on `main`**; a fourth (**Fix #2**) is **specced + Layer-A-planned but NOT
implemented** — that is the next work.

## Git state & how to resume

- **`origin/main` = `6a76220`** (SP0+SP1+SP2). **local `main` = `5882473`, 4 commits ahead, being
  pushed with this handoff** (the Phase-1 backjump diagnostic).
- **Branch `feat/wedge-semantic-branching`** (off `main` `5882473`) holds Fix #2's spec + Layer-A
  plan (+ this doc). Being pushed to `origin` with this handoff.
- **Resume:** on the other machine — `git fetch origin`; `git checkout feat/wedge-semantic-branching`
  (it carries the spec, both plans, and this handoff). Then execute the Layer-A plan (below).
- **Prerequisites on the other machine** (data is machine-local, NOT in git):
  - ORE inputs `~/data/ore-run/input/ore_ont_10019.ofn` + `ore_ont_13723.ofn`, and the oracle
    `~/data/ore-run/oracle/ore_ont_13723-classified.owx`. If absent, copy from this machine.
  - Curated corpus: `./scripts/fetch-real-ontologies.sh` (gitignored, pulled on demand).
  - **Toolchain gotcha (critical):** always `RUSTUP_TOOLCHAIN=stable cargo …` (pinned 1.95.0 lacks
    `cargo`; a bare build silently reuses a STALE binary). Rebuild BOTH `-p owl-dl-cli -p owl-dl-bench`
    before any CLI/matrix run, and confirm the binary actually rebuilt.

## What's DONE (merged to `main`; details in the cited docs, which travel with git)

- **SP0+SP1** — `RUSTDL_HYPER_INCREMENTAL_FIXPOINT` **default-ON**: incremental `horn_fixpoint`
  (drain per-branch worklist delta, not whole-graph reseed). Verdict-preserving (curated
  FP=0/MISSED=0 byte-identical; `ore_ont_13723` non-Horn oracle 0→0); ~56× fewer match-attempts;
  ~10% fewer stalled pairs on `ore_ont_10019`. Doc: `docs/2026-07-13-ore_ont_10019-stall-findings.md`.
- **SP2** — node-local UNSAT no-good caching: investigated, **DEAD**, engine code **reverted**
  (`main` engine byte-identical to pre-SP2). Kept: the Phase-A measurement (`shadow_measures::
  analyze_by_depth` + `tests/sp2_nogood_gate.rs`) + docs. 152-ont ORE sweep: benefits ZERO onts,
  net-negative (4 completion→timeout regressions); fired 51,748× all-net-new (refuted
  "backjumping-redundant"; extraction cost dominates). **Do NOT revive node-local no-goods.** Doc:
  `docs/2026-07-14-sp2-nogood-findings.md`.
- **Phase-1 backjump-precision probe** — **Fix #1 (backjump repair) RULED OUT**: `bjgap_real ≡
  bjgap_shadow` bit-identical incl. max on all stalled classes → precise deps give no backjump
  headroom. Read-only probe (`tests/backjump_precision_gate.rs`), zero engine change. Doc:
  `docs/2026-07-15-backjump-precision-findings.md`; design `docs/superpowers/specs/2026-07-15-
  wedge-backjump-precision-design.md`.

## Established diagnosis (do NOT re-litigate / re-measure)

The stall is **H2**: disjunctive-DFS thrash over a **blocking-bounded, redundantly re-explored**
state space. H1 (unbounded model / blocking failure) is ruled out (blocking gates the ⊔ rule;
"depth ~142" is the *disjunctive-decision stack*, not model depth; SP2 `revisit_frac≈1.0`).
**All quick levers are exhausted:** SP1 throughput (no verdict change), SP2 no-goods (DEAD), MRV
ordering (default-on, inert), `sat_lookahead` (inert; it's EL-consequence reasoning, wrong
fragment), backjump-precision (Fix #1, ruled out). **The remaining gap is in-search boolean
constraint propagation (semantic branching).** Konclude/HermiT decide in ms, so it is NOT
intractable — the gap is disjunctive-engine sophistication, a genuine build.

## NEXT WORK — Fix #2: wedge semantic branching / in-search BCP

**Spec:** `docs/superpowers/specs/2026-07-15-wedge-semantic-branching-design.md` (read it first).
**Layer-A plan:** `docs/superpowers/plans/2026-07-15-wedge-semantic-branching-layerA.md` (execute this).

**The gap, precisely:** `ore_ont_10019`'s 25 disjunctions are genuine covering axioms
`C ⊑ D₁⊔…⊔Dₙ` (irreducible — no absorption removes them). The 55 disjointness axioms are Horn
`⊥`-headed clauses — the *fuel*. Today disjointness is enforced only *reactively* (a dead disjunct
is found a full `horn_fixpoint` AFTER the branch commits) and only *positively* (nodes carry
positive `labels`; no "¬D asserted" state). **`build_disjoint_pairs` already exists and is built per
engine, but is consulted ONLY in the `≤n` path — never in the `⊔` path.** That is the lever.

**Design — two layers, all in `crates/owl-dl-tableau/src/hyper.rs` `solve` (`~:2259-2315`), behind
default-OFF `RUSTDL_SEMANTIC_BRANCHING`:**
- **Layer A (verdict-preserving — can't MISS/FP; ship + validate FIRST):** at the ⊔ decision, filter
  `live` disjuncts, dropping any `Class(c,X)` disjoint (via `disjoint_pairs`) with a current node
  label. `live` empty → `Unsat`; exactly one → assert it WITHOUT a decision level (unit force,
  recurse at same `depth`); else → branch over the filtered `live`. Expected to move `ore_ont_10019`
  little on its own (the reactive fixpoint already catches co-occurrence) — that's fine; it validates
  the mechanism + gate for Layer B.
- **Layer B (the real mover — SEPARATE plan after Layer A measures):** per-node `excluded:
  Vec<ClassId>` (rides the whole-node-clone `Snapshot` — NO `trail.rs` change); when a prior sibling
  disjunct returns a clean **`Unsat`**, exclude its class before the next branch → `D₁|D₂|D₃` becomes
  `D₁|¬D₁∧D₂|¬D₁∧¬D₂∧D₃`, and each `¬Dⱼ` propagates through the 55 disjointness axioms, collapsing
  downstream disjunctions to unit.

**⚠️ THE LOAD-BEARING SOUNDNESS INVARIANT (Layer B) — reuse-trap family:** exclude a sibling's class
**ONLY if that sibling returned `Unsat`, NEVER `Stalled`.** Under a deadline, branches stall
routinely; excluding a merely-*stalled* disjunct asserts an unproven `¬Dⱼ` → false clash →
**unsound → FP subsumption**. If any sibling returns `Stalled`, the frame's result is `Stalled` with
NO exclusion added. Atomic `Class` disjuncts only (compound `∃`/`Q` disjuncts stay live). This is the
same hazard as `reuse-trap-A1` / the snapshot-cache soundness fix.

**Gate (every shipped layer):** default-OFF flag → **FP=0** on curated **AND the non-Horn
`ore_ont_13723` oracle** (`konclude_closure_diff::ore_one_closure_matches_oracle`) + **MISSED=0 /
byte-identical curated closures** + (Layer B) a **canary that a `Stalled` sibling is never excluded**
→ flip default-ON only in a separate reviewed commit.

**GO/NO-GO (the whole Fix #2):** flag-ON `ore_ont_10019 classify` decides **≥ ~half of the 33** stalled
classes within the Konclude/HermiT budget (a few seconds), FP=0/MISSED=0 → corpus gate → default-ON.
**Else STOP → bound-the-tail:** make the `Stalled → NoVerdict → search.rs` fallthrough return
sound-incomplete FAST (some ORE onts hang; SP2 sweep found 4 timeout onts), and document "dense-SROIQ
disjunctive tail needs Konclude-class caching/learning, deferred." A legitimate, evidence-backed
outcome. **Advisor's candid probability Fix #2 closes the 33: ~40%** — worth building (cheap to
falsify; covering-axioms + pairwise-disjoint disjuncts + 55 disjointness are the ideal substrate).

## Layer-A execution checklist (from the plan)

1. **Task 1** — `RUSTDL_SEMANTIC_BRANCHING` flag scaffold (field + `with_semantic_branching` builder in
   `hyper.rs`; `semantic_branching_enabled()` in `reasoner/lib.rs` default-OFF, wired at the same
   classify `HyperEngine::new*` sites as `with_incremental_fixpoint`).
2. **Task 2** — Layer A filter + unit-force in the `solve` ⊔ block; TDD (a disjoint-dead-disjunct
   fixture; verdict identical + `semantic_prunes ≥ 1`); + the verdict-identity differential gate
   (classify OFF vs ON byte-identical on funcmerge-cyclic/pizza/27_eight_way_disjunction_sat/
   18_diamond_subsumption_unsat).
3. **Task 3** — curated byte-identical gate + `ore_ont_13723` FP oracle + measure `ore_ont_10019` →
   write `docs/2026-07-15-semantic-branching-findings.md` → decide Layer B (separate plan).

## Process notes (what worked this session — keep doing)

- **Subagent-driven development** (fresh implementer per task + a spec/quality review after each; a
  broad final review before merge). Ledger tracked resume state.
- **The advisor is invaluable.** Before every non-trivial build, dispatch an opus "advisor" subagent
  to read the actual engine + pressure-test the design against the code. It caught SP1's latent
  merge-coupling bug, SP2's compile/soundness errors (twice), and scoped Fix #2 correctly. Do NOT
  build a wedge/soundness change without an advisor pass.
- **Measure-first.** Every lever got a cheap spike/probe before a full build (SP0, SP2 Stage-0,
  backjump probe). SP2 taught the cost of building before the mechanism is nailed.
- **Soundness gates are non-negotiable:** FP=0 on curated **and** the non-Horn `ore_ont_13723` oracle
  (curated alone is EL/Horn where disjunction-FP can't manifest); MISSED=0 byte-identical closures.

## Open / carry-forward

- After this push: `main` = `5882473` on origin; `feat/wedge-semantic-branching` on origin. (Shared
  org repo `MaastrichtU-IDS/rustdl` — pushes are the user's call; this handoff push is per the
  machine-move request.)
- The stale `.superpowers/sdd/progress.md` ledger does not travel; THIS doc is the resume source.
- (Pre-existing, from earlier) correct the paper (`~/code/rustdl-paper`) framing to "sound;
  near-complete (hard dense-SROIQ tail Konclude clears)" — unrelated to Fix #2.
