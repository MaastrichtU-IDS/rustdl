# Plan: trigger-index the non-Horn clause fire loop (wedge match_body volume)

**Status:** proposed, for advisor review then delegation to Fable.
**Author:** Claude, 2026-07-22. Session: DL-tail perf arc.
**Branch (to create):** `perf/nonhorn-trigger-index`.

## 1. Problem (measured this session, not theorized)

The wedge (`crates/owl-dl-tableau/src/hyper.rs`) fires DL-clauses by, at each
node, iterating the disjunctive (non-Horn) clause list and calling
`match_body(clause, node)` to find body matches (the `find_open_disjunction` loop,
hyper.rs:2759). Profiling the volume-bound DL tail (flagship `ore_ont_3215`:
DL-approximated SNOMED, 54,973 classes, 18,323 disjunctive definitions of the form
`C ≡ (∃R.D ⊔ D) ⊓ E`) shows the hot path is `decide_with_deadline → match_body →
enumerate_matches`. The cost is **per-node-work = (non-Horn clauses considered) ×
(match cost)**, multiplied across a huge number of classify probes.

Established by measurement this session (see
`docs/2026-07-22-*` and the ore-perf memory):
- 3215 has **zero deep-stalling pairs** (single-class sat = 1 branch; genuine hard
  pairs = 0) — it is NOT a search-depth / CDCL problem; CDCL and CB-SROIQ are
  documented NO-GOs for this tail.
- The wall is **constant-factor × volume**: redundant clause-matching at scale.

### Current fire-loop weakness (code-grounded)

The non-Horn index (`ClauseIndexes::nonhorn`, hyper.rs:860) is a **flat
`Vec<(clause_id, Option<anchor_class>)>`** where `anchor` = the clause body's
**first** `Atom::Class(c, X)` (hyper.rs:940). The loop (hyper.rs:2759):
```rust
for &(ci, anchor) in &nonhorn.nonhorn {
    if let Some(a) = anchor && !self.nodes[node].has(a) { continue; }  // O(1) reject
    ... match_body(ci, node) ...
}
```
Three weaknesses, each acute on shared-conjunct disjunctive onts like 3215:
1. **`anchor = None` clauses are never rejected** — a body with no X-class atom
   (e.g. pure `∃R.D` — exactly the `(∃R.D ⊔ D)` shape) always calls `match_body`.
2. **A common-conjunct anchor rejects rarely** — 3215's ~18k defs share conjuncts
   (the CarbonAtom-analog); a node carrying the shared atom passes the reject for
   all of them (same weakness that makes the label heuristic prune 0 here).
3. **Linear scan of the whole list per node** — even the O(1) rejects cost an
   iteration over all N non-Horn clauses at every node, every fixpoint pass.

## 2. Goal & non-goals

**Goal:** cut redundant `match_body` calls by iterating, at each node, only the
clauses whose body could plausibly match that node's current labels — a proper
**trigger index (labels → candidate clauses)** — instead of scanning all non-Horn
clauses. **Verdict-preserving by construction:** the index only changes which
clauses are *considered*; `match_body` still verifies every candidate, so no clause
that would have fired is skipped and none fires that shouldn't. This is a
constant-factor win (target: materially cut 3215-class wall; realistic 2–5×, not
asymptotic), soundness-safe (no FP/MISSED surface), and the only DL-tail lever
consistent with every measurement.

**Non-goals (do NOT do these — documented dead-ends / parked):**
- NOT the build-once / global-model classification rewrite — **archived (#37,
  commit 2905e8d)**; its P0 gate found it partial/conditional. Do not re-propose.
- NOT CDCL / conflict learning (NO-GO: tail has no deep search).
- NOT CB-SROIQ / Sequoia (documented NO-GO).
- NOT changing the tableau/wedge *verdicts* or the disjunct *selection* strategy
  (MRV/semantic branching are separately measured). This is purely which clauses
  the fire loop *considers*.
- NOT the tier-walk-overhead lever (`find_direct_parents_top_down` traversal) —
  a separate, orthogonal lever; leave it.

## 3. Phase 0 — measurement GATE (do FIRST; ~1 day) — DECIDES THE PROJECT

Every prior rustdl perf lever's hard-won lesson: **measure, don't infer** (four
wrong inferences preceded the unsat-probe-via-label-cache win; the build-once P0
gate parked that rewrite). **CRITICAL (advisor): the "non-Horn fire loop is the
bottleneck" premise is currently an INFERENCE, not a measurement.** The only
normal-run profile this session had pointed more at `classify_labels` than
`match_body`, was parent-frame (not leaf/self-time) attribution, and the
"recomputed each call" quote is about the *parked CB engine's* `apply_hyper`, a
DIFFERENT code path — not the wedge's `hyper.rs::match_body`. So P0 must FIND the
target before validating the fix. Two sub-steps, in order:

### P0a — find the target (self-time + call-site attribution)

- **Leaf/self-time profile of a NORMAL bounded 3215 run** (not post-deadline; the
  gdb-sampler from `docs/2026-07-22`, but aggregate by *innermost* app frame, not
  parent scaffolding like `in_worker`/`catch_unwind`/`join_context`). Establish
  the actual top self-time consumers: `match_body`/`enumerate_matches` vs
  `classify_labels` (label-cache build) vs saturation/prep vs tier-walk traversal.
- **Attribute `match_body` calls BY CALL SITE.** `match_body` has four callers
  (hyper.rs:2768, 2824, 3517, + the non-Horn loop this plan targets at 2759).
  **Line 3517 is a SEPARATE caller** (plausibly the deterministic/Horn application
  that runs at every node every fixpoint pass). If 3517 (or 2768/2824) dominates
  the match_body volume, **trigger-indexing the non-Horn loop (2759) misses
  entirely** → NO-GO for this plan (a different lever). P0a must show the non-Horn
  loop (2759) is the dominant match_body chunk before P1 is justified.
- **Attribute by PHASE (label-cache-build vs tier-walk).** The label-cache A/B this
  session showed disabling it is NET-NEGATIVE — its per-class sat work (flowing
  through the same fire loop → match_body) is PRODUCTIVE, not redundant. So calls
  made during the label-cache build must be tagged separately and NOT counted as
  reclaimable headroom (they populate the oracle regardless of head-satisfied
  state, so the `match_body_useful` heuristic below misclassifies them). Only
  tier-walk-phase fire-loop calls are index-reclaimable.

### P0b — validate the index opportunity (only the tier-walk-phase, 2759-site calls)

Instrument the non-Horn fire loop (read-only `SearchStats` counters, env flag
`RUSTDL_NONHORN_PROBE`, default off, byte-identical when off), scoped to the
tier-walk phase:
- `nonhorn_considered`, `nonhorn_anchor_rejected`, `nonhorn_none_anchor`.
- `match_body_calls`, `match_body_empty` (None/empty binding = definitely wasted),
  `match_body_useful` (binding + head-not-already-satisfied).

**Run on:** `ore_ont_3215` (flagship), plus 2 volume-tail onts (one `has:D`/`has:DC`
from the MIXED bucket, one large EL-ish for contrast), bounded
(`--pair-timeout-ms 1500 --global-timeout-ms 120000`).

**GO criteria (ALL must hold — a projected-speedup threshold, not just "waste
exists"):**
1. **Target confirmed (P0a):** the non-Horn fire loop (site 2759) is the dominant
   `match_body` self-time chunk, above label-cache-build and tier-walk-traversal.
2. **Reclaimable fraction:** the trigger index would eliminate a large fraction of
   the tier-walk-phase fire-loop calls (wasted `match_body_empty` + anchor-passed-
   but-empty attributable to weak/absent anchors — `nonhorn_none_anchor` high or
   common-conjunct anchors).
3. **Projected wall win clears ~30%:** `(fraction of fire-loop match_body calls the
   index eliminates) × (fire-loop self-time share of total wall) ≥ ~0.30`. This is
   the guard against a correct-but-inert build — it forces the "would it actually
   move the wall" question BEFORE the multi-week work, not after (§6.6).

**NO-GO if:** the dominant cost is a different match_body call site (3517/2768/2824
→ different lever), or `classify_labels`/saturation/prep/tier-walk-traversal (→
neither phase of this plan helps; e.g. 12128's ∀-prep), or the anchor reject is
already effective, or the projected win is < ~30%. Record all numbers; a NO-GO
here saves the multi-week build and is a valid, valuable outcome.

## 4. Phase 1 — trigger index (only if P0 = GO)

Replace the flat `nonhorn: Vec<(ci, Option<anchor>)>` scan with a
**label-keyed candidate lookup**:
- Build `nonhorn_by_trigger: Vec<Vec<usize>>` (indexed by `ClassId`) — for each
  clause, its trigger class(es). For a clause with X-class body atoms, index it
  under **all** of them (not just the first) so the intersection is tighter; a
  node need only consider clauses ALL of whose X-class triggers it carries — but
  the conservative, obviously-sound version indexes under each and dedups
  candidates, then lets `match_body` verify.
- **`anchor = None` clauses** (pure role-body, no X-class trigger): these cannot
  be label-keyed. Handle by a role-trigger index — key them by the body's
  existential filler/role so a node only considers them when it has a matching
  edge — OR, conservatively for v1, keep them in a small always-considered
  residual list (measure how many there are in P0; if few, residual is fine).
- At a node, gather candidate clause-ids = ⋃ over the node's labels of
  `nonhorn_by_trigger[label]`, dedup (a `visited`/generation stamp or a sorted
  merge), plus the always-considered residual, then run the existing
  `match_body`-verify + head-satisfied + MRV-best-selection logic **unchanged**.

**Ordering caveat (MRV/first-found determinism):** the current loop is
ascending-clause-id, and the MRV tie-break relies on it (hyper.rs:2756 comment).
The candidate gather MUST present clauses in ascending clause-id order (sorted
merge or sort the deduped set) so the selection is identical → byte-identical
verdicts. Pin this.

**Amortization:** the trigger index is built once at `ClauseIndexes` construction
(alongside `match_plans`/`nonhorn`), shared via the existing `Arc<ClauseIndexes>`
(hyper.rs:2758) — zero per-node/per-probe rebuild.

## 5. Phase 2 — enumerate_matches join (CONDITIONAL, only if P0 says cost is
inside match_body, not call volume)

If P0 shows most `match_body` calls are *useful* but individually expensive (the
join over successors is the cost), optimize `enumerate_matches` (hyper.rs:3609):
successor-by-role indexing on the node (avoid the per-atom `edges.iter().filter`
linear scan), and/or a better variable evaluation order (most-constrained role
atom first). Same verdict-preserving property (it changes *how* matches are found,
not *which*). Scope this only if warranted; do not build speculatively.

## 6. Correctness gates (mandatory, in order)

1. **Byte-identical closures corpus-wide** — before-vs-after `classify` output
   (sorted direct/equiv/unsat) on the curated corpus (galen, notgalen, sio, wine,
   ore-10908, ore-15672, alehif, ro, pizza, bibtex) + a sample of ORE volume-tail
   onts. **0 diffs.** This is the crown-jewel FP=0/MISSED=0 gate — the index is
   verdict-preserving by construction, so ANY diff is a bug.
2. **Non-Horn oracle FP gate** — `ore_ont_13723` (the non-Horn FP canary) vs the
   Konclude∩HermiT oracle: FP stays 0.
3. **Full workspace suite green** — `cargo test --workspace` (excl. owl-dl-py per
   CI), + the existing wedge canaries (shadow_dep, backjump_precision, etc.).
4. **fmt + clippy -D warnings clean** (CI is -D warnings; edition-2024 let-chains).
5. **Perf: EL non-regression** — galen classify wall unchanged (the index must not
   tax the Horn/EL path; non-Horn list is empty there, so it should be inert —
   verify).
6. **Perf: the win** — 3215-class wall before-vs-after at a fixed budget (more
   direct edges decided / lower wall). Report the number; this is the payoff proof.

## 7. Delegation notes (Fable)

- Branch `perf/nonhorn-trigger-index` off current `main`. Do NOT push, do NOT merge.
- **Toolchain gotcha:** build/test with
  `export PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"; export RUSTUP_TOOLCHAIN=stable`.
  Release builds ~1 min; confirm `target/release/rustdl` fresh before benchmarking.
- **TDD:** Phase 0 counters first (assert they populate on a small non-Horn
  fixture, byte-identical when the flag is off). Phase 1: write the
  dense-vs-indexed identity check (a small non-Horn fixture classified with the
  index forced on vs a scan-all fallback → identical) before wiring.
- Tooling on the share drive for benchmarking: the `bjgap_dl_tail.rs` harness
  (hard-pair finder) and the ORE ont pool at
  `/data/dumontier/ore-run/pool_sample/files/` (convert to `.ofn` via ROBOT).
- Commit trailers (exact):
  `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`
  `Claude-Session: https://claude.ai/code/session_01BPU4DH5DXn2jmpuXdfijF7`
- **STOP and report at the Phase 0 gate** — do not build Phase 1 until the human
  confirms P0 = GO. A NO-GO is a valid, valuable outcome.

## 8. Risks

- **P0 = NO-GO** (the honest primary risk): the "non-Horn fire loop is the
  bottleneck" premise is unproven — the cost may be a DIFFERENT `match_body` call
  site (3517/2768/2824 → different lever), inside `enumerate_matches`' join (→
  Phase 2), in the label-cache build (→ parked build-once territory, not this
  plan), or in saturation/prep (e.g. 12128's ∀-prep → neither phase helps). P0a
  exists to find the real target cheaply before the build; do not skip it.
- **Ordering drift** breaking byte-identity (§4 caveat) — the most likely
  correctness bug; the identity gate (§6.1) catches it.
- **Constant-factor only** — this will not close the Konclude gap (partly a decade
  of C++ tuning); it cuts the volume-tail wall, no more. Frame honestly.
