# MOMS disjunct ordering — implementation plan

Drafted 2026-05-26. Multi-session work; this file tracks the design
so the work survives session boundaries.

## Goal

Reduce pizza-shape search-tree explosion. The diagnosis in
[`perf-2026-05-24-new-server.md`](perf-2026-05-24-new-server.md) §6
shows pizza's pair-loop wall is bounded by 200 ms × 1172 timed-out
pairs / parallelism — every probe exhausts its budget exploring
binary-disjunction trees that never terminate within the cap.

The fix that has the right shape: **pick the disjunct that, if
chosen, simplifies the most *other* open disjunctions**. That is
the SAT-solver MOMS heuristic (Maximum Occurrences in
Minimum-size clauses) adapted for DL tableau.

## Why session §6's attempt failed

`8c3fefa` introduced a static 4-tier score, then this session
tried to split tier 1 into "atomic with downstream rules" vs
"atomic without." Reverted because:
- Pizza disjunctions are uniformly binary (no tier discrimination
  inside a 2-option pick).
- All atomic disjuncts on pizza have downstream rules (the
  topping-coverage closure populates them).
- A successful tableau call regressed (87 → 86) — the static
  signal made one specific pair worse.

The static refinement was barking up the wrong tree. The
information that matters is **what other open Or labels in the
graph would be satisfied if I pick `d`** — a dynamic, per-decision
quantity.

## Algorithm

At each `branch()` decision over options `[d1, d2, …, dn]`:

1. Walk the completion graph and tally, for each candidate `dj`,
   how many *other* open Or labels (`Or(opts)` where no member of
   `opts` is yet in the host node's label set) contain `dj` as one
   of their options.
2. Use the tally as a secondary key inside the existing
   4-tier score: higher tally = try first, ties broken by original
   index (preserves determinism — required for
   `literal_complements`).

The structural intuition: picking `dj` adds `dj` as a label, which
**resolves** every other Or in which `dj` appears (the disjunct
is now satisfied, no further branching needed there). On pizza
the residual GCIs propagate the same Or-shape to every successor,
so a well-chosen `dj` collapses dozens of pending decisions at
once.

## Cost model

Per `reorder_disjuncts` call:

- Existing 4-tier score: O(|options|) cheap pool/binary-search ops.
- New MOMS tally: O(|nodes| × avg_labels_per_node) per option.
  Pizza peak: 92 × ~50 = 4600 label probes per option, × 2 options
  per disjunction ≈ 9k probes per decision. ~3 µs per decision.
- Per 200 ms probe at ~250 decisions: ~750 µs total overhead.

Trivial against the 200 ms budget. Bigger ontologies (SIO, 1585
classes) need the same analysis — the tableau model per probe is
not 1585 nodes, it's the per-probe completion graph (typically
≤ 100 nodes), so the cost stays bounded.

A future micro-optimisation if this turns out hot: maintain a
`Vec<u32>` indexed by `ConceptId` that counts "is in an open Or"
incrementally on `add_label` / `rollback_to`. For now, scan on
demand — correct first, fast later.

## Integration points

- `crates/owl-dl-tableau/src/search.rs`:
  - Add `count_open_or_occurrences(ctx, d) -> usize` (mirrors the
    iteration pattern in `first_open_disjunction`).
  - Refactor `reorder_disjuncts` so the score tuple is
    `(tier_u8, -moms_count_i32, original_index)`.
- No public API change. Existing callers see only verdict
  differences (we hope: faster sat-finding on pizza-shaped inputs).

## Validation strategy

1. **Unit tests are pre-existing** — every existing tableau test
   exercises `branch()`. They must all keep passing (same
   verdicts).
2. **Cross-check vs. naive on the 87-fixture corpus** — same
   harness used for the soundness-fixes work. No verdict diffs.
3. **Real-corpus regression** — `tests/real_ontology_corpus.rs`
   under `--features real-corpus`: pizza, sio-stripped, family,
   RO unsat sets must match the HermiT-via-ROBOT reference.
4. **Perf measurement** — pizza wall and SIO wall with
   `--pair-timeout-ms 200`. Report counters
   (`tableau_subsumption_calls`, `timed_out_pairs`). The headline
   is "fewer pairs time out": pizza 1172 → ?; SIO 33394 → ?.

## Acceptance criteria

- All ≥267 in-tree unit tests pass.
- 87-fixture differential corpus: zero verdict diff vs. baseline.
- Real-corpus tests: zero unsat-set diff vs. ROBOT-HermiT.
- Pizza wall: improvement (not "no change")
- SIO wall: improvement (not "no change")

If the perf numbers don't move, revert per the §6 lesson —
shipping a MOMS implementation that doesn't reduce timeouts is
the same mistake as last time, with more code.

## Open questions

- **Negative occurrences.** Should `Not(dj)` appearing in another
  Or count as a *positive* MOMS signal for `dj` (picking `dj`
  doesn't satisfy that Or, but does it harm)? Initial pass: no —
  count only direct occurrences. Iterate if pizza wall hasn't moved.
- **Soft-tier collapse.** If MOMS ranks a tier-3 (compound) disjunct
  far higher than a tier-1 (cheap atomic) sibling, do we still
  honour the tier ordering? Initial pass: yes — tier is the primary
  key, MOMS is only the secondary within a tier. Compounds are
  genuinely more expensive to commit to even if they resolve more
  Or's.
- **Incremental maintenance.** If the simple scan is hot, switch to
  a per-`ConceptId` open-Or-occurrence counter maintained
  alongside the trail. Don't pre-build — measure first.

## §A — 2026-05-26 implementation attempt: zero wall change

Built the algorithm described above. All 251 tests passed.
Measured:

| Workload | Before MOMS | After MOMS |
|---|---|---|
| pizza wall | 28.89 s | 28.87 s |
| pizza timed_out_pairs | 1172 | 1173 |
| pizza tableau_subsumption_calls | 87 | 86 |
| sio-stripped wall | 266.02 s | 269.78 s |
| sio-stripped timed_out_pairs | 33 394 | 33 394 |

Reverted per the acceptance criterion. The implementation lives
in this session's history (see git reflog if needed) and can be
resurrected — but as written it does not move the headline number.

### Why it didn't work

**MOMS assumes cross-clause interaction.** In classical SAT a
literal `d` appearing in clauses `C1, C2, …` means setting `d=true`
satisfies all of them. Resolving 10 clauses with one decision is
the structural win.

**In DL tableau, disjunctions are local to nodes.** Picking
disjunct `d` at node `N` adds `d` to `L(N)`, which satisfies the
Or label on `N`. It does **not** automatically satisfy `Or(d, e)`
appearing as a label on node `M ≠ N`. The Or on `M` is its own
constraint and must be branched independently.

The only mechanism by which a choice at `N` reaches `M` is rule
propagation:
- `apply_forall` if `N` has `∀R.d` and an `R`-edge to `M` — narrow
  conditions; doesn't fire on the pizza/SIO shape.
- `apply_concept_rules` — local to the node.
- `apply_residual_gcis` — *adds* new disjunctions to every node,
  doesn't resolve them.
- `apply_exists` — creates fresh successors with new labels;
  doesn't satisfy existing Or's.

For pizza/SIO the dominant disjunction source is residual GCIs
that propagate to every node *as new Or labels*, not as resolved
ones. Choosing a disjunct never satisfies another node's Or, so
MOMS counts what — by construction — doesn't influence the
outcome.

### What this rules out

- Static disjunct-ordering tweaks of any flavour (verified across
  three independent attempts: tier-split, depth bump, full MOMS).
  Per-decision information cannot shrink a search tree dominated
  by residual GCIs.

### What this points to

The lever has to act on the **number or shape of Or labels**, not
the choice between disjuncts inside an Or. Concrete next bets:

1. **Lazy unfolding of residual GCIs.** Don't materialise
   `⊤ ⊑ Or(d, e)` on every node — only on nodes that exercise
   one of the disjunction's downstream consequences. Pizza wins
   if most successors never trigger the residual.
2. **Smarter absorption.** Some residual GCIs can be rewritten as
   `concept_rule { trigger: A, conclusion: B }` if their structure
   permits. Fewer residuals = fewer per-node disjunctions.
3. **Model caching.** Even if every probe still hits the same
   residual GCI explosion, caching the satisfying model of `A`
   means `is_subclass(A, _)` queries reuse it.
4. **Module extraction.** Separately: cut `is_subclass(A, B)`
   probes whose ⊥-modules don't intersect.

(1) and (2) are the closest to a real perf fix; (3) and (4) are
orthogonal wins on the orchestrator side. None are session-scoped.
