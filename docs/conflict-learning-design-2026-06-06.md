# Conflict-driven nogood learning for the hypertableau wedge — design

Project kickoff, 2026-06-06. Goal: attack the disjunction-branching interdependence
that makes wine (and covering-disjunction-heavy SROIQ) stall — measured in
`docs/tableau-perf-scoping-2026-06-06.md` (90/137 wine classes stall,
`restores = branches`, genuine interdependence so backjumping can't prune).

**Status: DESIGN ONLY. No code yet.** Soundness-critical (#1 failure mode: an
unsound prune → false subsumption). Implement only behind a default-OFF flag with
FP=0 corpus-wide gating at every increment.

## 1. Where it hooks in (grounded in `hyper.rs`)

`solve(depth)` (hyper.rs:1164) is a recursive DFS. Each open disjunction
(`find_open_disjunction`) is a decision at **level `d = init_depth - depth`**.
`clash_deps: DepSet` (a `u128` bitset of decision *levels* + `overflow`) drives
backjumping: a disjunct's clash skips the remaining disjuncts of decision `d`
iff `!clash_deps.contains(d)` (line 1200).

Backjumping is **intra-path**: it prunes siblings of a decision *on the current
path*. It cannot transfer a learned conflict to a *different* subtree — which is
exactly what `restores = branches` on wine needs.

## 2. The core problem: levels are path-relative; nogoods must be stable

`clash_deps` is a set of decision *levels*. Level `d` denotes "the d-th decision
on the current path" — it means **different disjunct-choices in different
subtrees**. So a level-set is not a transferable nogood.

A transferable nogood must be a set of **stable decision identities**:
`(clause_id, canonical_node, disjunct_index)`. "Asserting all of these disjunct
choices clashes" holds in *any* branch (the clash derivation used only those
choices + the constant TBox/horn-fixpoint).

**Soundness of the nogood (the load-bearing argument — rewritten).** Two facts:
1. **Monotonicity of the learnable clashes.** The clashes we learn from
   (`A ⊓ ¬A`, `⊥`-label, disjoint-pair-present) are produced by rules that only
   *add* labels, never retract. So a clash derivable from decision-set `N` +
   deterministic closure stays derivable when `N` recurs with *additional*
   context in another subtree. Pruning a state whose decisions ⊇ `N` is therefore
   sound.
2. **Overflow excludes the non-monotone cases automatically.** The non-monotone
   rules (`≤n` merge, `≠`-violation, NN) all set `clash_deps = DepSet::ALL`
   (overflow). Restricting learning to **non-overflow `clash_deps`** therefore
   excludes exactly the clashes where monotonicity could fail. (This is the
   *deeper* reason the conservative rule is sound — not "node identity"; a future
   increment must not relax the overflow guard.)

Over-counting in `clash_deps` (an irrelevant decision included) is **safe** — it
yields a weaker nogood (matches less often), never an unsound one. Never
"tighten" `clash_deps` for learning.

## 3. The stable-node-identity subtlety (the real risk)

A decision is at a node. Across subtrees, "the same node" must be a stable
logical entity, not an `HNode` index (indices can differ by creation order).
- **Root**: always stable (id = root).
- **Deterministic ∃/≥n successors**: created by the horn fixpoint, identifiable by
  their creation provenance `(parent_canonical_id, role, qualifier)` — stable iff
  the parent is stable and the ∃ is deterministic (not itself under a branch).
- **Branch-created nodes**: NOT stable — a nogood mentioning one must not be
  recorded (it can't be matched soundly elsewhere).

**Conservative rule:** only record a nogood if *every* decision in it sits on a
canonically-identifiable node (root, or a deterministic-successor chain from
root). Otherwise skip recording. This keeps learning a sound *optimization*:
fewer nogoods = less pruning = never unsound, only less effective.

## 3b. Go/no-go measurement (2026-06-06, temp-instrumented `solve` on wine)

Per-class `hyper-sat` on wine, instrumented at the disjunction decision + clash:

| Metric | Value | Meaning |
|---|---|---|
| clash `d ∈ deps` vs `d ∉ deps` (backjumps) | **1 071 030 vs 0** | backjumping **never fires** — every clash depends on the current decision. Confirms (measured, not inferred) that learning is the only lever. |
| disj decisions: root vs successor | **17.9 k vs 476 k** | **96 %** of decisions are on successors, not the root. |
| of successor decisions, deterministic (`birth_deps=EMPTY`) | **68.6 k** (~14 % of succ) | only ~17.5 % of *all* decisions (root + deterministic-succ) sit on nodes identifiable without creation provenance. |

**Consequence — the plan below is reordered.** There is **no de-risked
on-ramp**: root-only (Inc 1) touches ~4 % of decisions, and even
root+deterministic-successors reaches only ~17.5 %. The other ~82.5 % are
branch-created successors. Covering them requires **canonical-by-creation-
provenance node identity** (`root`, or `(parent_canonical_id, role, qualifier,
creating-decision)`), so a nogood transfers whenever the same decision *prefix*
is replayed. That is the hard, soundness-critical core, and it is unavoidable
from the first useful increment.

## 4. Increment plan (each behind `RUSTDL_HYPER_LEARNING`, default OFF; FP=0 gate)

**Reordered after §3b: provenance-based node identity is increment 0, not a
late extension — root-only is not a useful milestone.**

- **Inc 0 — canonical node identity + decision stack (no behavior change).** Intern
  a `canonical_node_id` for every node by creation provenance: `root → 0`;
  successor → `intern((parent_cid, role, qualifier, creating_decision))`. Maintain
  `Vec<Decision>` indexed by level, `Decision = {clause_id, canonical_node_id,
  disjunct_index}`. **Gate: corpus closure-diff byte-identical** (pure bookkeeping).
  This is the load-bearing, risky piece (provenance must be a true invariant
  across subtrees) and there is no smaller useful version — confront it first.
- **Inc 0.5 — recurrence probe (GATES the multi-week commitment).** On top of
  Inc 0's canonicalization, at each clash with non-overflow `clash_deps`,
  canonicalize its decision-set and record it; count distinct nogoods and the
  **recurrence rate** (clashes whose decision-set repeats / is a superset of a
  prior nogood). **Zero pruning, zero soundness risk** — pure measurement on the
  current engine. This is a *lower bound on Inc 1's pruning power*: high
  recurrence → Inc 1 pays off, commit with evidence; near-zero → Inc 1 prunes
  nothing, **stop before building it**. (Per review: canonicalize exactly as Inc 0
  does — a coarser key inflates the rate into false optimism.)
- **Inc 1 — record + match nogoods (non-overflow clashes only).** At a clash with
  non-overflow `clash_deps`, map its levels → decisions → store the nogood. Before
  recursing a disjunct, if the current decision-set ⊇ a stored nogood, prune
  (return Unsat with the nogood's deps). **Gate: FP=0 corpus-wide + differential
  fuzz (learning-ON ≡ learning-OFF on random SROIQ).** Measure wine
  `restores/branches` drop and how many of the 34 recover within budget.
- **Inc 2 — watched-literal indexing** for the nogood-match check, only if Inc 1
  shows the check is hot. Pure perf, same verdicts.

(There is no separate "root-only" milestone — §3b shows it would touch ~4 % of
decisions and validate almost nothing.)

## 5. Soundness gates (non-negotiable)

- Default OFF; every increment FP=0 across all corpus fixtures (closure-diff) +
  verdicts byte-identical to learning-OFF on a sample.
- A dedicated adversarial test: random small SROIQ ontologies, learning-ON vs
  learning-OFF must agree on every pair (differential fuzz).
- Termination: the nogood store is monotone and bounded by the number of distinct
  decision-sets; learning only prunes (never adds branches), so it can't
  non-terminate. The store is cleared per top-level `decide` call (nogoods are
  TBox-relative, valid for the whole pair-loop — but start per-call to be safe;
  cross-pair reuse is a later optimization).

## 6. Expected payoff + honest caveat

Targets wine's 34 + covering-disjunction-heavy ontologies. The corpus already
converges, so **no corpus speedup expected** — the metric is wine
`restores/branches` reduction and recovering some of the 34 within budget.

## RECURRENCE PROBE RESULT (2026-06-06) — decisive GO

Built the recurrence probe (Inc 0 canonicalization + per-decide nogood store,
zero pruning, thread-local; reverted after measuring). On wine (`hyper-sat`,
1 s/class):

```
learnable_clashes = 159 908   distinct_nogoods = 1 017   recur_hits = 158 891
overflow_clashes  = 0
```

- **99.4 % recurrence** — only **1 017 distinct conflicts**, re-derived **~160 k
  times**. Conflict-driven learning records the 1 017 and prunes the ~158.9 k
  recurrences ⟹ ~99 % of the disjunction clashes collapse.
- **0 overflow clashes** — *every* clash is learnable; no `≤n`/`≠`/NN
  non-monotone case on wine's hot path (pure disjunction). The
  overflow-exclusion guard costs nothing here.
- The probe canon is **finer-than-needed** (includes the full decision prefix),
  which only *deflates* recurrence — so **99.4 % is a lower bound**.

**Verdict: GO, evidence-backed.** Payoff is no longer "uncertain until built" —
the lower bound is ~99 % clash reduction. Proceed to production Inc 1.

## Honest assessment (now superseded by the probe — kept for the record)

Two facts had made this look high-risk before the probe:
1. **No de-risked increment.** The provenance-based canonical node identity
   (Inc 0) is the load-bearing, soundness-critical piece and must come first;
   there's no "root-only" warm-up that validates anything (~4 % coverage).
2. **Payoff is uncertain even if Inc 0/1 land.** A nogood transfers only when its
   whole decision *prefix* is replayed; on a tree dominated by branch-created
   successors (82.5 %), how often that happens is unknown until measured. It may
   prune a lot, or little.

**Recommendation: this is a dedicated, fresh, focused project — not a tail-end
continuation.** It is multi-week, soundness-critical (a single under-reported-dep
nogood → false subsumption), and targets an outlier (the corpus already
converges). Start it from Inc 0 with the full gate suite (closure-diff +
differential fuzz) in a session devoted to it, and re-evaluate payoff after Inc 1
measures wine's `restores/branches` reduction. If that measurement is weak, stop
— the wine-34 are then a genuine architectural limit of dependency-directed
search, documented as such.
