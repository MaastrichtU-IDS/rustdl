# Dependency-directed undo for the main tableau (#288) — design

Status: **spec only, for review.** No engine code until the review is addressed.
Issue: #288. Evidence: the issue comments of 2026-10-23.
Revision 3: addresses the first Fable review (B1–B5, S1–S7) and the second (N1–N2, S1–S5, nits).

## 1. Problem

`rustdl sat` on PMD class `PMD_0010100` reaches no answer in 300 CPU-s; HermiT says sat.
The main tableau (`crates/owl-dl-tableau/src/search.rs`) spends almost all of its time
rebuilding work that a backjump threw away but that never depended on the decision being
flipped.

Measured (single-thread CPU, one core):

| case | what the trace shows |
|---|---|
| core3 (15-axiom cut of PMD) | 55 backjumps of ≥ 50 levels undo 11,334 levels; **99.5%** of the undone ⊔ decisions have preconditions independent of the flipped decision. ~95% are then re-made identically. |
| full `PMD_0010100`, 150 s | 31.6k decisions; **4** backjumps undo **21,180** levels (~5,000 each); **100%** independent. |
| full `PMD_0020140` | only 7.8k decisions and 210 clashes; the cost is ~3 ms per decision, not search. |

"Independent" means: the decision's `⊔` label deps and its node's attaching-edge deps
(`in_edge_deps`) are contained in {decisions below the backjump target} ∪ {earlier
independent undone decisions}. That is a precondition test only, and it under-states
dependence: it ignores node-existence closure (see B2 / §4.3). §4's go/no-go re-measures
with the correct test before anything is built.

Already tried and rejected (issue comments): clash-activity ordering (2× slower everywhere),
dependency-recency ordering (core3 38× faster, but full-ontology classes 2.5–4.6× slower and
`PMD_0010100` still unanswered). Decision replay alone saves nothing: the picks are re-made
for free; the cost is the state rebuild.

## 2. Goal and non-goals

Goal: on a backjump to decision `T`, undo only the state that depends on `T` (or on a
decision above `T` that is itself undone), keep everything else, and continue.

Non-goals: learning (nogoods / CDBL lookup), model caching, ordering heuristics, the
hypertableau wedge. The wedge is untouched.

## 3. Plan, in separately mergeable steps

| step | content | merges on its own? |
|---|---|---|
| 1 | dependency under-recording fixes, behaviour-preserving today | yes |
| 2 | go/no-go measurement, diagnostic branch only | no code to merge |
| 3 | explicit decision stack, chronological undo, trace-identical | only if step 2 says go |
| 4 | selective undo behind `RUSTDL_SELECTIVE_UNDO` (default OFF) | only if step 2 says go |
| 5 | measurement and default decision | — |

### Step 1 — dependency under-recording fixes

Each fix only widens or adds a dep set. Under chronological backtracking that can only
make backjumping more conservative, so it cannot introduce a false `Unsat`; it can cost
backjumping precision, so step 1 is gated on CPU as well as verdicts (§3.1 gate).

**1a. The `≤n` choice point (`first_open_max` / `branch_merge`).** `apply_max` already
unions every counted witness's deps into the merge (`witness_deps`, `rules.rs:1100-1105`,
used at `:1123-1126` and in the `⊥` arm). The gap is the #293 choice point:
`first_open_max` computes partner deps pairwise (`search.rs:263-272`) and `branch_merge`
merges with `pair_deps ∪ {my_id}` (`search.rs:330-335`). The witness that made the `≤n`
over-full is not in the merge's deps; today `combined` starting from all active branches
(`search.rs:300-301`) masks it. Fix: compute `witness_deps` once in `first_open_max` over
`witnesses` (the same per-witness `compute_max_merge_deps(w, w)` union `apply_max` uses),
store it on `OpenMax`, and union it into every merge option's deps. It is also the merge
frame's precondition set (§4.2).

**1b. Failed-disjunct complements** (`search.rs`, the `literal_complements` loop). `¬d_j`
is added to later siblings with `combined_deps` (parent `⊔` deps ∪ `my_id`). Its real
justification is `d_j`'s failure, i.e. `clash_deps(d_j)`. Since the `d_j` label itself
carries `combined_deps` and earlier complements already carry their own tags,
`clash_deps(d_j) ⊇ combined_deps` whenever `my_id ∈ clash_deps(d_j)`, so the tag is simply
`clash_deps(d_j)`. Keeping `my_id` in it is required: the complement must leave with the
frame. Note `add_label_with_deps` keeps the first writer's deps (`lib.rs:1620`), so the
complement must be removed before it can be re-asserted under different deps.

Today's behaviour is sound: exhaustion unions every child's clash deps into the returned
set. A differential fuzz (complements on vs off, 2,100 cases, HermiT adjudication; §8)
found no divergence. Under selective undo, a kept `¬d_j` whose refutation needed `T`
would be wrong, so 1b is a prerequisite for step 4, not a bug fix.

**1c. Deps for mutations that record none.** Give each the deps listed:

| mutation | deps |
|---|---|
| `DistinctMarked` (store a parallel `inequality_deps`; `inequalities` is a bare `Vec<NodeId>`, `lib.rs:1695`) | `apply_min`: `min_deps`; `branch_merge` distinct arm: `{my_id}` ∪ `max_deps` ∪ partner deps (`search.rs:343-345`); merge step 4 (`lib.rs:1930-1935`): merge deps ∪ the original mark's deps. First writer wins, as for labels. |
| merge (`MergedRedirect`, `ParentRewritten`, the root-breaking case `lib.rs:1969-1978`, and the `EdgeRemoved`/`EdgeAdded`/label moves it performs) | recorded as one **merge unit** (begin/end markers) carrying the merge deps. Everything a merge moves already carries deps ⊇ the merge deps (`lib.rs:1823-1826`). `EdgeRemoved` is tagged with the merge deps, not `prior_edge_deps` (which is often `∅`). |
| `NominalAssigned` | deps of the `Nominal` label **∪ the node's existence deps** (§4.3). |
| `NodeCreated` | the node's **existence deps** (§4.3), not just the creating edge's deps. |

Step 1 only records these. Nothing reads them yet, so verdicts are unchanged by
construction. 1a and 1b both widen deps that backjumping reads (complement tags gain the
failed disjunct's ancestor ids), which is why §3.1 gates on CPU.

Not a prerequisite: the choose rule's empty `DepSet` (`search.rs`, `first_open_choose`).
The split `C ⊔ ¬C` is a tautology, so its labels' only dep is the branch's own `my_id`.
The choose *frame* still needs a precondition set for step 4 (§4.2).

#### 3.1 Step 1 gate

- Unit tests asserting the dep content at each fixed site.
- Verdict identity on the curated corpus, core3 and the six PMD classes.
- **CPU**: core3 and the five finishing PMD classes, interleaved, ≥ 3 reps; no class
  slower than 1.1× `main` (#312 showed PMD performance hinges on precise deps).
- Clippy, workspace tests with `--no-fail-fast`, `run-soundness-diff.sh`.

### Step 2 — go/no-go (§5)

### Step 3 — explicit decision stack (behaviour-identical)

Replace the recursive `search`/`branch`/`branch_merge` DFS with an explicit stack of
decision frames. A frame holds: kind (`⊔`, choose, merge), node / label / pivot, ordered
options, next option index, `my_id`, checkpoint, the precondition dep set (§4.2), and one
**failure record per refuted option**: `(option, clash_deps)`. `combined` is no longer a
running accumulator; it is computed from the surviving failure records (§4.4).

`push_branch`/`pop_branch` go away: `active_branches` is maintained by the stack as a
**sorted set** (insert and remove by id), not LIFO (`lib.rs:690-712`). Step 4 reorders
frames, after which push order is no longer ascending, and two sites feed
`active_branches()` straight into the sorted merge-walk `deps::union` (`deps.rs:20-46`):
`branch_merge`'s `combined` (`search.rs:300-301`, then `binary_search` at `:361`) and
`apply_max`'s `any_distinct` fallback (`rules.rs:1113`). An unsorted set makes
`clash_deps.binary_search(&my_id)` (`search.rs:356`, `:495`) miss a present id, which is a
spurious backjump and a false `Unsat`. Add a debug assertion that every `DepSet` passed to
`union` or `binary_search` is sorted.

Undo stays chronological. Acceptance: verdicts and decision sequences identical to
`main` (same `(node, disj, pick)` sequence on core3 and the five finishing PMD classes),
and CPU within 1.05×.

### Step 4 — selective undo behind `RUSTDL_SELECTIVE_UNDO=1`

See §4 for the rules. Default OFF until §6's bar is met.

### Step 5 — measurement and rollout

Two-arm sweep, MISSED net, FP=0 net, ORE sweep, as §6 requires.

## 4. Selective undo: the rules

### 4.1 The backjump

On a backjump to `T` with clash deps `D`, let `U` = `{T}` ∪ {every decision above `T`}:
the decisions chronological undo would remove. Build the kept set `K` **bottom-up from
`T`**: a frame `k` above `T` is kept iff its precondition set (§4.2) is disjoint from
`U ∖ K`. Walking bottom-up is what catches transitive dependence through a candidate
that is itself undone.

An entry on the trail above `T`'s checkpoint is **dependent** iff its deps intersect
`U ∖ K`, **or it references a dead node** (§4.3). Undo every dependent entry, in strict
reverse-chronological order among the undone entries.

### 4.2 Frame preconditions

| frame | precondition set |
|---|---|
| `⊔` | the `⊔` label's deps ∪ the node's existence deps |
| choose | the `≤n` label's deps ∪ the existence deps of the node and the neighbour ∪ the edge's deps (the `DepSet::new()` at `search.rs:174` is fine for the asserted labels, not for keeping the frame) |
| merge | `OpenMax.witness_deps` (1a) ∪ every witness's existence deps |

### 4.3 Node existence is a closure

A node's existence deps are its creating edge's deps **∪ its creator's existence deps**,
transitively. Residual GCIs fire with empty deps at any node, so a child `c` of a
`T`-dependent node `z` can be created with `∅` edge deps (`apply_exists` passes only
`exists_deps`, `rules.rs:764-767`; likewise `apply_min`, `rules.rs:879-884`). Without the
closure, `c` would survive `z`'s death with a dangling in-edge and a `blocking.parent`
pointing at a dead node (`lib.rs:1416` walks it), and a kept `⊔` frame on `c` could
exhaust and return a false `Unsat`.

Equivalently: tombstone the parent-forest subtree of every dead node. The rule "any entry
that references a dead node is undone regardless of its deps" makes this robust to a
missed dep anywhere else.

### 4.4 Failure records and the flip (dynamic backtracking)

Branch ids are per frame, not per option (`push_branch` once, `search.rs:398`;
`record_decision` overwrites, `lib.rs:765`). A kept frame `k` may hold failure records
whose clash deps mention a decision in `U ∖ K` — refutations that were valid only under
`T`'s old option. Ginsberg's rule, adopted here: **when `T` flips, delete every failure
record (in any kept frame) whose clash deps intersect `U ∖ K`**. Those options become live
again and their `¬d_j` complements have already been undone by §4.1, because 1b tags them
with exactly those clash deps. A frame's `combined` is recomputed from its surviving
records. Without this, a kept frame could exhaust on options never tried under `T`'s new
option and return a false `Unsat`.

Then `T` moves to its next option on top of the kept state; kept frames are re-ordered
below it.

**Option iteration is record-driven.** A frame's next option is the first option with no
surviving failure record, not a stored index: after a deletion, an option with an index
below the old "next" must be retried, or the frame can exhaust on the rest and return a
false `Unsat`. The complements asserted before an option are exactly the surviving
records' complements (re-adding a present label is a no-op).

**Complement labels and records may diverge, harmlessly.** Because labels keep the first
writer's deps (`lib.rs:1620`), a complement label can carry deps different from its
record's. If the label is undone while the record survives, the frame loses that pruning
only. If the label survives while the record is deleted, the label was derived
independently and is valid. Neither path is unsound; do not "fix" it by forcing the
label's deps to the record's.

### 4.4a Kept frames act on `resolve(node)`

A kept frame's node can be merged away by a kept (independent) merge unit. `saturate`
skips redirected nodes (`saturate.rs:135-144`), so an option or complement added at the
original node never propagates: the disjunction is unsatisfied at the survivor (a false
`Sat`), or a later complement at the survivor contradicts a choice the frame never sees.
Today this cannot happen, because rollback undoes the merge. Rule: frames, and complement
re-assertion, act on `resolve(node)`; if the survivor already satisfies the frame's `⊔`,
the frame is retired, not exhausted.

### 4.5 State that is not on the trail

After a selective undo:
- force `saturate_full`: the incremental path reads the trail tail
  (`trail.rs:170-186`, `saturate.rs:73-95`) and would see holes;
- `mark_all_dirty`;
- clear `residuals_saturated` on every node an undone entry touched (`trail.rs:259` does
  it only for `LabelAdded`);
- bump the graph `generation`, so the one-entry `blocked_memo` (`lib.rs:1387-1400`)
  invalidates; blocking is lazy, so nothing else needs recomputing;
- recompute `graph.nominals` from live `{a}` labels: `NominalAssigned`,
  `ParentRewritten` and `MergedRedirect` undo by prior-value restore (`trail.rs:332-359`),
  which can be stale with holes. Two live, unredirected nodes may carry the same `{a}`
  after an undone merge: take the lowest id as canonical, dirty both, and let
  `apply_nominal_assignment` re-merge them in the forced `saturate_full`;
- skip dead ids in `block_index` buckets and in every `0..graph.len()` scan
  (`first_open_*`, `resolve`, the rules).

Not graph state, safe as is: `decision_labels`, `learned_nogoods` (never recorded),
deadline and early-abandon counters.

### 4.6 Engine changes this needs

- **Position-independent undo.** `EdgeAdded` and `DistinctMarked` undo by `pop()`, and
  `EdgeRemoved` stores absolute positions; with holes these are wrong. Undo by key. An
  `EdgeRemoved` undo then appends the edge rather than restoring its old position, which
  is harmless: edge order carries no meaning beyond neighbour enumeration order.
- **Node tombstones.** `truncate_nodes` cannot remove a dead node that sits between two
  kept ones. Add an `alive` flag and never reuse ids.
- **Merge units** (1c) so a merge is undone or kept whole.

## 5. Go/no-go before step 3

The explicit decision stack is the expensive part. Before building it, measure on the
diagnostic branch (`diag/288-search`, which already prints `or_deps`, `cdeps` and `ret`
lines; `main` has no `ret` line) what selective undo could save, with the correct
dependence test:

1. At each backjump of ≥ 50 levels, compute `K` with §4.1–4.3 (existence closure
   included) and the fraction of undone trail **entries** that are not dependent. The
   existence closure needs each node's creator and creating-edge deps, which the decision
   trace does not print: compute `K` in-process from `graph` at the jump (a diagnostic hook
   on the branch), which also yields per-entry deps once 1c lands.
2. Charge CPU from the jump until the trail length regains its pre-jump value (a monotone
   CPU counter; no id matching needed). If it never does within the window, charge to the
   end of the window.
3. Ceiling = total CPU / (total CPU − Σ kept-fraction × rebuild CPU).

**Go** if the ceiling is ≥ 2.5× on `PMD_0010100` (150 s window) **and** ≥ 3× on core3.
**No-go** otherwise: steps 3–5 stop, step 1 still merges on its own merits.

Why 2.5×: on `PMD_0010100` the four long jumps undo 21,180 of the window's 31,600
decisions, so by decision count the ceiling is about 3×, and the window undercounts an
unfinished search. A bar above what the data can show would be a decision made in
advance.

The ceiling is computed over undone trail entries on a 150 s window of an unfinished
run, so a pass is weak evidence and a fail is strong evidence.

Quote the result as a ceiling, never as the expected value: re-saturation and cache
invalidation after a selective undo (§4.5) cost something the trace cannot see.

## 6. Bar for turning step 4 on by default

- `PMD_0010100` answers sat (HermiT agrees) within 300 CPU-s.
- core3 and the five finishing PMD classes: no class more than 1.1× slower than `main`.
- The differential fuzz of §7 with the flag on: 0 verdict differences vs HermiT.
- The debug oracle of §7: 0 mismatches.
- FP=0 net: all closures exact. MISSED net: ΔMISSED ≤ 0.
- Full two-arm ORE sweep: 0 answer changes, 0 genuine `ok → dnf` after serial recheck.

## 7. Testing

- **Differential fuzz, extended to ALCHIQ+O.** The current generator is ALC (literal
  disjunctions); it cannot exercise ≤n merges, nominals, inverse or functional roles,
  which are exactly the 1a/1c hazards and the shapes of core3. Add functional roles, ≤n
  and ≥n, inverse roles and `{a}`. Run with the flag on and off; adjudicate by HermiT.
  Treat a 0-byte HermiT output as an error, never as sat (§8).
- **Debug oracle.** In debug builds, after every selective undo, rebuild chronologically
  from `T`'s checkpoint by replaying the kept frames' picks, and assert identity of labels
  with deps, edges, inequalities and the nominal map. (`replay.rs` is the wedge snapshot
  path and is not reusable.) Run it in the fuzz, not in the hot path of release builds.
- **Sabotage.** For each of 1a, 1b, 1c, §4.3, §4.4 (deletion and record-driven
  iteration), §4.4a and the sorted `active_branches` of step 3, revert it with the flag on and
  confirm the fuzz or the debug oracle reports a false `Unsat` or a mismatch. A rule
  whose removal nothing notices is not guarded.
- **Trace identity** for step 3.
- Gates: clippy, workspace tests with `--no-fail-fast`, `run-soundness-diff.sh`.

## 8. Evidence appendix

- Keepable-fraction measurement: #288 issue comments of 2026-10-23.
- Complement-tagging fuzz (§3 step 1b): 600 + 1,500 ALC cases. Verdicts identical with
  complements on and off; complements changed the search in 44 cases. HermiT agrees on
  all 386 cases adjudicated: 201 from the first set (121 unsat, 80 sat) and 185 from the
  second (the 35 where complements changed the search, 100 unsat, 50 sat). One case first
  read as a disagreement: HermiT had written an empty output file, which the check scored
  as sat. Rerun, HermiT says unsat, as rustdl does.
