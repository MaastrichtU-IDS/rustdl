# SP1: Semantic branching + disjunct reordering in the wedge — design

**Status:** design (brainstormed 2026-06-19), pre-plan.
**Sub-project 1 of 4** in the "close the wine wall to Konclude" program (see Roadmap below).
**Branch:** `feat/wedge-semantic-branching-sp1`.

## Problem

The hypertableau **wedge** (`crates/owl-dl-tableau/src/hyper.rs`) — the default
classify accelerator — has a catastrophically inefficient disjunctive search on
nominal+disjunction SROIQ. Measured root cause (this session, probes
`decide_pair_probe`/`sat_class_probe`):

- wine's varietal classes (`X ≡ Wine ⊓ ∃madeFromGrape.{XGrape} ⊓ ≤1 madeFromGrape`)
  are **satisfiable** (oracle has 0 unsatisfiable classes), yet `sat(CabernetFranc)`
  reaches `Sat` only at **90 s / 1.49 M disjunctive branches**, `restores == branches`.
- It is a **find-the-model** failure, not exhaustion: the model exists; the wedge's
  blind depth-first disjunct selection re-explores ~1.5 M doomed combinations first.
- **Backjumping cannot help** (`backjumps = 0 / 76 k decisions`, structural bjgap≈1:
  every nominal-merge clash genuinely depends on its disjunction decision — confirmed
  by experiment, not a defect). The within-search cache is inert (branches are
  distinct, not re-derived). See memory `wine-wall-bjgap1-genuine`.

The wedge's `solve` disjunction loop is **purely syntactic**: it tries each clause
head atom via save/restore in clause order, with no complement carried forward and
no reordering. The **main tableau** (`search.rs::branch`) already implements the two
standard fixes — restricted semantic branching and disjunct reordering — and runs
them soundly in production. The wedge simply lacks them.

## Roadmap (the program this is SP1 of)

Konclude's sub-second wine = optimized tableau: dependency-directed backtracking
(we have it) + **semantic branching + branching heuristics + (un)sat caching +
nominal/cardinality optimizations**. Decomposed, each FP=0-gated and measured on wine:

| SP | Technique | Leverage | Risk |
|----|-----------|----------|------|
| **SP1 (this)** | Semantic branching + disjunct reordering (port `search.rs`→wedge) | Biggest single win; stops the 1.5 M re-exploration | low–med |
| SP2 | Branching heuristics (constraint-informed disjunct selection) | reach the model faster | med |
| SP3 | Nominal-aware unit propagation (BCP) | force the grape nominal directly | med–high |
| SP4 | (Un)satisfiability label caching | reuse sat status across n² pairs | med |

## Goal

Port `search.rs`'s restricted semantic branching + disjunct reordering into the
wedge's `solve` disjunction loop, **flag-gated, FP=0/byte-identical-corpus
preserved**, collapsing wine's disjunctive search by ≥10×.

## Design

### A. Algorithm (in `hyper.rs::solve`, the `find_open_disjunction` branch)

The current loop (≈ lines 1696–1740) iterates `for k in 0..head_len` over the
clause's disjunctive head atoms, each via `save()` / `apply_head_atom` /
`solve(depth-1)` / `restore`. Two additions, mirroring `search.rs::branch`:

1. **Disjunct reordering.** Before the loop, compute an order over the head-atom
   indices via `score_disjunct(atom, node) -> u8` (lower tried first):
   - `0` — leaf/inert: an atom whose assertion adds only a non-triggering label
     (no concept-rule trigger, no ∃ to expand, no merge).
   - `1` — plain `Class` atom that does not obviously clash.
   - `2` — ∃-generating / compound atom (creates a successor or fires a merge).
   - `3` — obvious clash: the atom's complement is already in `node`'s label
     (assert ⇒ immediate `(C, ¬C)` clash). Tried last.
   Reordering is **verdict-neutral by construction**: the same set of disjuncts is
   explored to the same completeness; only the order changes. Stable secondary key
   on original index for determinism.

2. **Restricted semantic branching.** Maintain `literal_complements: Vec<Atom>`
   (reset per disjunction). When a disjunct `d` (a `Class` atom) returns `Unsat`
   with `decision_d` *in* its clash deps (the "this decision mattered" arm, where
   `combined` is currently accumulated), look up `¬d` in the engine's complement
   map; if present (a literal complement), push `Atom::Class(¬d, X)` onto
   `literal_complements`. At the top of each subsequent sibling iteration, assert
   every accumulated complement on `node` tagged with `decision_deps` (the parent
   disjunction's body deps ∪ this decision id), before applying that sibling's head
   atom. Compound complements are **not** carried (label-set bloat without paying
   for itself — same restriction `search.rs` documents).

### B. Soundness (FP=0 is sacred)

- **Reordering**: no soundness surface — verdict-neutral (same disjuncts, same
  completeness, different order).
- **Semantic branching**: `¬d` is asserted only when `d`'s failure depended on this
  disjunction decision (`decision_d ∈ clash_deps`), so `¬d` is entailed in the
  sibling context; it is tagged with `decision_deps` so the existing
  dependency-directed backjumping stays correct. This is exactly the in-production
  `search.rs` logic. The risk direction is a *missed* subsumption or an FP from an
  unsound complement assertion — both caught by the byte-identical corpus gate.
- The existing wedge guards are untouched: the `nn_tainted → DepSet::ALL` clash-dep
  widening, double-blocking, precise-card-deps, adaptive budget all remain. Semantic
  branching only *adds* dep-tagged labels in the disjunction loop.

### C. Components / files

- `crates/owl-dl-tableau/src/hyper.rs`:
  - new `complements: Vec<Option<ClassId>>` engine field (indexed by class id;
    `None` = no literal complement) + `with_complements(map)` builder mirroring
    `with_sub_roles`.
  - new `score_disjunct(&self, atom, node) -> u8` and
    `reorder_disjunction_heads(&self, ci, node) -> Vec<usize>`.
  - modify `solve`'s disjunction branch: iterate the reordered indices; maintain
    `literal_complements`; assert them dep-tagged on siblings; push a complement on
    the "decision mattered" arm.
  - gate the whole behaviour on `self.semantic_branching` (set by builder from the
    env flag) so flag-off is byte-identical to today.
- `crates/owl-dl-reasoner/src/lib.rs`:
  - add `complements: HashMap<ClassId, ClassId>` to `HyperCache` (currently a dropped
    local in `build`); thread it via `with_complements` into every engine
    (`decide_with_stats`, `sat_only_with_stats`, `classify_labels`, the probes).
  - `semantic_branching_enabled()` flag helper (`RUSTDL_WEDGE_SEMANTIC_BRANCHING`,
    default off; flip to default-on after corpus validation).

### D. Flag / default

`RUSTDL_WEDGE_SEMANTIC_BRANCHING` **default off** for the build/validation phase.
Flip to default-on (and document like the other levers) only after the FP=0
byte-identical corpus gate passes and wine collapse is confirmed. Flag-off path is
byte-identical to current main.

## Validation

- **P0 (go/no-go, first task):** prototype reorder + semantic branching, measure
  `sat(CabernetFranc)` (`sat_class_probe`, adaptive off, big stack). Success =
  branches collapse ≥10× (1.49 M → ≤150 k, ideally Sat in seconds). If it does not
  bite, **stop and rethink** before investing in polish/threading.
- **FP=0 gate (mandatory):** `konclude_closure_diff` byte-identical to the
  Konclude∩HermiT oracle on every fixture (galen, notgalen, sio, ore-10908,
  ore-15672, wine, ro, sulo, bibtex, shoiq-knowledge), flag-on. FP=0 AND MISSED
  unchanged.
- **Completeness canary:** a `blocked_disjunction_soundness`-style test that a
  disjunctive-`Unsat` subsumption is still proven with semantic branching on
  (`disj_branches > 0`, verdict `Subsumed`).
- **Verdict-preservation unit tests:** small ontologies where flag-on and flag-off
  produce identical sat/unsat verdicts (reordering + semantic branching change only
  search order/pruning, never the answer).
- **Wine canary:** `ore-15672`-style — wine `timed_out_pairs` drops materially at a
  fixed per-pair budget, or `sat(CabernetFranc)` branch count collapses ≥10×.

## Out of scope (SP2–SP4)

Branching heuristics beyond the score-based reordering (SP2); nominal-aware unit
propagation / BCP (SP3); (un)satisfiability label caching (SP4). SP1 is the
foundation they build on.
