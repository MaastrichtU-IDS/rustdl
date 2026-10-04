# Classify search on #139: 350 classes never get a label-cache model — diagnosis and plan

Status: **RESOLVED by Step 1.0 (2026-10-04) — the blocking hypothesis was wrong; the cause is a
ghost-node read in `head_atom_satisfied`, fixed on `fix/hyper-ghost-node-disjunction-139`.**
The plan below is kept as the record of how it was reached; Steps 1.1–5 are moot. Issue: #139
(PMD core ontology).

## OUTCOME — Step 1.0 found the mechanism, and it is not blocking

**1.0 as specified.** A per-class probe on the label-cache build (peak completion-graph nodes,
branches, restores, depth, blocks), run with `RUSTDL_HYPER_DOUBLE_BLOCK` = 1 and = 0. On the
312 classes that stall at the default and are `Sat` under `=0`:

| | pairwise ON | pairwise OFF |
|---|---|---|
| peak nodes (median / max) | **20 / 73** | 18 / 89 |
| branches (median) | **1,248** | 4 |
| restores (median) | 1,248 (= branches) | 0 |
| max branch depth | **256 on all 312** (the cap) | median 4 |
| divergence cut fired | 312 / 312 | — |

Small graph in both arms ⇒ by the pre-registered rule, **not** non-terminating generation; the
blocking-redesign branch (Step 3) is dead.

**But "the search tree explodes" (H4) was also wrong, and the depth figure said so.** Raising
the label-cache depth cap (a throwaway probe, `RUSTDL_LABEL_DEPTH`, not in the tree) to 512 / 1,024 / 2,048 left the
same 350 stalled, and at 2,048 every one of the 312 reached depth **2,048 on graphs of 4–73
nodes** — 2,048 nested decisions over as few as 4 nodes. That is not a large search; it is
**one disjunction re-opened at every level.**

**Mechanism (pick-log probe at the ⊔ decision site).** On every stalled class the search
re-picks one `(clause, node)` hundreds of times (e.g. class 863: 508 of 510 picks are clause
2890 at node 2). Dumping the graph at the third re-pick shows the node is a **merge ghost**:
a `≤1` merge (`RUSTDL_INVERSE_FUNC_MERGE`, default ON) has folded it into a survivor
(`representative[n] ≠ n`). Then:

* `find_open_disjunction` scans every node, ghosts included, and the clause body still matches
  the ghost's own labels;
* `head_atom_satisfied` read the disjunct on the **ghost**, so it is never satisfied;
* `apply_head_atom` → `add_label` **resolves through the union-find** and writes the disjunct
  onto the **survivor**.

So the ghost never gains the label, the `⊔` never closes, and it is re-picked until the depth
cap; `is_diverging` then returns `Stalled` on a satisfiable class. `fire_exists`'s witness test
already resolved on read; `head_atom_satisfied` did not.

**Why `RUSTDL_HYPER_DOUBLE_BLOCK=0` looked like a fix.** Under anywhere blocking a ghost's
labels are a subset of its survivor's, so the ghost is *blocked* and `find_open_disjunction`
skips it. Pairwise blocking cannot block it when ghost and survivor arrived by different roles
(as on #139). The ⊔-gating confound the review flagged was real — it was masking this bug. And
`RUSTDL_INVERSE_FUNC_MERGE=0` (no merge ⇒ no ghost) gives **0 stalls, 1,375 / 1,375 `Sat`**.

**Fix.** A disjunct now counts as satisfied if it holds on the raw node **or** on its
representative (identity when `inverse_func_merge` is off). Both halves are needed, because the
writers disagree: `add_label` writes the survivor, while `fire_exists` and the `≤n` head write the
raw node. A resolve-only read (the first version) fixed #139, but by inspection it could leave an
`∃` disjunct asserted at a ghost open — `fire_exists` puts the witness on the ghost, which a
survivor-only read cannot see. That gap was argued, not observed: a canary with `∃` disjuncts
passes under resolve-only too, because the survivor inherits the ghost's labels at merge and its
own copy of the disjunction closes the ghost's. The raw half is kept anyway because it IS main's
read, so the OR can never close fewer disjunctions than main does; it is not test-guarded. The OR is a superset of both reads, so it can only
*remove* `⊔` branch points, never add a clash: FP-safe by construction (a spurious `Sat` is a
MISS, never an FP). Unflagged — it makes existing reads agree with existing writes.

**Effect on #139** (pinned binaries, default flags):

| | main `5fa16ec` | fix |
|---|---:|---:|
| label-cache NoVerdict | 350 | **0** |
| label misses | 72,044 | **0** |
| timed-out pairs | 76,338 | 4,986 |
| user CPU | 973 s | **61 s** (16×) |
| wall | 75 s | **10.3 s** |
| rows | 1,564 | 1,569 |
| FP / MISSED vs HermiT | 0 / 23 | **0 / 3** |

With the fix, pairwise ON and OFF agree (0 / 3 both).

**Guard.** `hyper::tests::disjunction_on_a_merged_ghost_closes_on_the_survivor`: ghost and
survivor reached by different roles (`s ⊑ r` via a role-head clause) under pairwise blocking,
with a precondition assert that a node was actually folded. **Sabotage verified for the resolved half only**: reverting
only the resolve makes it fail on its 5 s deadline (without a deadline it hangs — the re-picked
`⊔` multiplies with its sibling). Two earlier versions of the test were VACUOUS and sabotage
exposed both: one used the default (anywhere) blocking, the other a shared incoming role, and in
each the ghost was blocked, i.e. the masking described above reproduced inside the test.

**The #182 reproducer was the same bug.** With both #182 safety nets off
(`RUSTDL_CONSISTENCY_FULL_EFFORT=0`, `RUSTDL_CONSISTENCY_COMPONENTS=0`) main reports `unknown`
and the fix reports `inconsistent` (HermiT, Konclude and JFact agree); main with
`RUSTDL_INVERSE_FUNC_MERGE=0` also answers `inconsistent`, which attributes the give-up to the
merge ghost. Default-path consistency on that file goes 2.08 → 0.18 s. The test that pinned the
wrong answer (`full_effort_off_reverts_…`) is flipped, not deleted.

**Gates.** FP=0 soundness net: 20 / 2, matching this host's baseline (the 2 are the absent
alehif / notgalen fixtures); all 11 verified closures exact. Tests: tableau 157 lib + integration
green; reasoner 266 lib + 145 integration binaries / 1,233 tests green; remaining crates 42
binaries / 662 green (`owl-dl-py` cannot link here — no libpython). Clippy `-D warnings` clean.

**1,920-ontology two-arm sweep (v1, resolve-only), 60 s cap, 6 workers, arm order alternated:**
1,732 IDENTICAL / 178 BOTH_FAIL / 4 DIFFER / 4 "REGRESSED" / 2 "RECOVERED"; wall over the
both-completed set 8837 → 8754 s (0.991). Every flagged row re-run sequentially on an idle host,
3 alternating repeats per arm at a 180 s cap:

| row | verdict |
|---|---|
| `14459`, `16008`, `3914`, `3524` | cap-boundary noise — complete in both arms, answers identical 6/6 |
| `12698`, `1509`, `16457`, `778` | budget truncation — `incomplete: true`, and MAIN alone yields 2–3 distinct answer hashes over 3 runs, overlapping the fix's |
| `16372` | **real recovery** — main DNF at 180 s 3/3, fix 16 s 3/3 (an inconsistent KB) |
| `10109` | **real wall regression** — 37 s → 97 s 3/3, answers identical |

**`ore_ont_10109`, characterised.** All of it is `label_cache_build` (6.2 → 66.6 s). The same 73
classes are NoVerdict in both arms; on main they were cut at a median 47 ms each, on the fix all
73 run their full 910 ms budget. A pick-log probe shows they are NOT a ghost loop (≈0 ghost
picks, depth 39–144 below the 256 cap, `restores = branches`) — a genuine search that thrashes at
bounded depth. `is_diverging` requires a saturated depth, so it never cuts them; on main the ghost
loop saturated the depth and tripped the cut *by accident*. That is a blind spot in the adaptive
budget (no cut for all-failing search below the cap), a separate lever — recorded, not fixed here.
Its answer is unchanged, so the cost is wall only; it crosses the 60 s sweep cap.

**v2 (raw-OR-resolved), the shipped form**: #139 unchanged (0 label misses, 7.5 s), `10109`
unchanged (98 s), guard sabotage fails as expected. v2 swept over all 1,920 (60 s cap, 6 workers)
and compared against main's outputs recorded in the v1 sweep (a separate parallel run, hence the
sequential re-runs of every flagged row below): **1,732 identical / 176 both-fail / 12
flagged.** Nine flagged rows are the v1 set with the same verdicts. The three new ones —
`4911` (rows differ), `11005` and `3562` (main ~60 s, v2 55–56 s) — re-run sequentially, 3
alternating repeats per arm at 180 s: **answers identical 6/6 on all three**, walls overlapping
(`11005` straddles the 60 s cap in both arms). Net ship delta vs main over 1,920: **0 answer
changes, 1 real recovery (`16372`), 1 real wall regression (`10109`), aggregate wall flat** —
plus #139 (75 → ~8–10 s, MISSED 23 → 3) and the #182 reproducer (`unknown` → `inconsistent`
with the safety nets off).

## Residual: the 3 MISSED on #139 (investigated, NOT fixed)

All three share one sub: `PMD_0010100` (elemental semiconductor) ⊑ `PMD_0020210` (elemental
crystal), plus that class's two told supers `PMD_0020003` and `BFO_0000030`. HermiT's
justification is two axioms:

    PMD_0010100 ⊑ =1 has_member.PMD_0020140
    PMD_0020210 ≡ =1 has_member.PMD_0020140

That core alone classifies correctly (both binaries). The miss needs context, and it is
**budget-insensitive** (missed at `--pair-timeout-ms` 5 / 100 / 1000 / 2000) while `subclass`
and `explain` on the pair run past 120 s. Reproduced on a 482-axiom ⊥-module (`m100.ofn`,
deterministic 5/5 at 1 and 4 threads). No existing flag recovers it (`TRUST_SAT=0`,
`LABEL_HEURISTIC=0`, `INVERSE_FUNC_MERGE=0`, `HYPER_DOUBLE_BLOCK=0`, `SAME_TIER=1`,
`ROLE_HIERARCHY=1`).

Because `A`'s only route to `BFO_0000030` is through `PMD_0020210`, the top-down tier walk must
prove `A ⊑ BFO_0000030` before it ever compares `A` with `PMD_0020210`.

**Three distinct wedge gaps, found with a per-pair pick-log probe; each was prototyped in a
throwaway worktree, and all three together still leave the pair missed:**

1. **`≠`-only cardinality clash carries `DepSet::ALL`.** `card_clash_deps` returns `ALL` when
   two successors are distinct via `≠` rather than disjoint labels (guard 2), and the `≠` clash
   in `merge_with_cause` is hard-coded `ALL`. With 13 `PMD_00200xx ≡ PMD_0020140 ⊓ ∀has_part.X`
   definitions, every `PMD_0020140` successor carries 13 binary disjunctions, and backjumping
   cannot skip them: the wedge enumerated all 2¹³ (pick counts halving 5351 / 2676 / 1338 …,
   `restores = branches`). The main tableau shows the same blow-up in isolation — synthetic
   `A ⊑ =1 r.C`, `B ≡ =1 r.C` + k such definitions: `subclass` 1.4 s at k=12, 3.8 s at 14,
   15 s at 16, >120 s at 20 — while classify's wedge stays ~0.3 s there. A prototype (`≠`
   between unmerged endpoints justified by birth deps, which is how `generate_at_least`, the
   only `add_neq` caller, creates it; and only qualifier-label deps when distinctness is all
   `≠`) fixed a 20-axiom ddmin reduction but not the module. **Soundness-critical** — an
   under-reported dep set is an unsound backjump, i.e. an FP.
2. **`≥n` over an INVERSE role is never generated.** `has_member` is canonicalised to
   `member_of⁻` by `InverseObjectProperties`, so `PMD_0020210`'s ⊒ clause is
   `⊤ → PMD_0020210 ∨ ≥2 member_of⁻.C ∨ ≤0 member_of⁻.C`. `generate_at_least` returns
   `NoChange` for any inverse role (HF3a scope), so choosing that disjunct does nothing and the
   clause is re-picked ~494 times until the depth cap — the same re-pick-loop *shape* as the
   ghost bug, different cause. (The 20-axiom ddmin reduction had dropped the
   `InverseObjectProperties` axiom, which is why it showed only gap 1.)
3. **An `≥n` disjunct is never counted satisfied.** `head_atom_satisfied` returns `false` for
   `AtLeast`, but `generate_at_least` declines when the count guard or fire-once already holds,
   so a picked `≥n` disjunct can be a no-op that leaves its clause open — a third re-pick loop.

With all three prototypes on, what remains on `A ⊑ BFO_0000030` is a genuine search (a 5-way
disjunction over ~8 nodes, depth 89, no loop). These are wedge completeness work on inverse
cardinality, each needing a design + FP gate; they should be their own issue, not a #139
follow-up patch. Reproducers in the session scratchpad: `m100.ofn` (module), the 20-axiom
ddmin reduction (gap 1 only), the synthetic `k`-sweep (gap 1 in the main tableau).

## TL;DR

On the #139 ontology, 76,159 of 77,328 classify pairs (98.5%) hit the 5 ms per-pair deadline,
and the wall is set by that deadline, not by engine speed. The cause is upstream of the pair
loop: **350 of 1,375 classes never get a label-cache model**, budget-insensitively, and every
pair with one of them as the sub falls through to a per-pair search that also stalls. Of the
seven wedge options tested, only one moves this: **`RUSTDL_HYPER_DOUBLE_BLOCK=0` takes the
NoVerdict count 350 → 38, CPU 973 → 111 s, MISSED vs HermiT 23 → 7, FP 0.** That flag is a
diagnostic, not a fix (the ontology uses inverse roles, where single blocking is not a valid
model-construction rule).

**What the flag result does NOT yet establish** (review, §3a): the flag changes two things,
not one. `is_blocked` gates both `∃`-generation *and* the ⊔-rule (`find_open_disjunction`
skips blocked nodes, `hyper.rs:3623`, `:3694`), so `=0` may win by making most nodes never
branch rather than by making generation terminate. The plan therefore starts with a
one-command measurement that decides between those readings before any blocking redesign is
costed.

## Evidence (all on the coder host, v0.4.34 release binary, `ontology.owl` from #139)

### 1. The search is deadline-bound

Default run banner:

```
# label heuristic: pruned=194809 pass_through=5284 misses=72044
# wall breakdown ms: label_cache_build=10462 tier_walk=26869 sweeps=37332
# timed-out pairs: 76159 (defaulted to not-subsumed)
# fallthrough (wedge-stall→tableau): ran=76159 rescued=0 ... noverdict=76159
# wedge-cost-histogram ms (0|1|2-4|5-9|...): 193 | 34 | 844 | 76179 | 76 | 2 | 0 ...
```

At `--pair-timeout-ms 100`: `tier_walk` 26.9 → 470.0 s (17.5×), `sweeps` 37.3 → 626.5 s
(16.8×), for a 20× budget, with byte-identical subsumptions (1,564 rows). Per CLAUDE.md's
"check `wall/budget` across two budgets" rule, this is deadline-bound: a faster engine spends
its time inside the same budget and the wall does not move. **Do not optimise the hot loop.**

The fallthrough tableau ran on every timed-out pair and **rescued 0 of 76,159** — about half
the CPU (973 s / 77k pairs ≈ 12.6 ms per pair ≈ 5 ms wedge + 5 ms tableau + overhead).

### 2. The miss pool is the label-cache NoVerdict set

`RUSTDL_DUMP_LABELS` verdicts: **1,025 `sat`, 350 `noverdict`, 0 `unsat`**. 350 subs ×
~200 candidate sups ≈ the 72,044 label misses. Budget-insensitive:

| per-class label budget | NoVerdict | `label_cache_build` |
|---|---:|---:|
| default (adaptive, 1375 × 5 ms ≈ 6.9 s) | 350 | 10.5 s |
| `RUSTDL_LABEL_CACHE_TIMEOUT_MS=30000` | 350 | 6.4 s |
| `=10000` + `RUSTDL_ADAPTIVE_BUDGET=0` | 350 | 210 s |

With the adaptive early-cut on, they are abandoned fast as diverging; with it off, each burns
the full 10 s and still fails. These classes do not converge at any budget.

`hyper-sat` (which runs *anywhere* blocking, never pairwise) still stalls on 6 classes, and
**the three worst block 109,691 / 140,696 / 253,597 times and stall anyway**. Its `depth`
column is the ⊔-search depth (`max_branch_depth`), not the length of a generating chain, and
the `depth=256, restores≈branches` signature also appears on a class that ends `Sat`
(`PMD_0000142`: 2,012 branches / 2,007 restores). So that output does not show a blocking
failure; if anything it shows that the hardest classes are **search-bound** (the 2026-08-02
nominal-blocking root cause had the same shape: the search tree explodes, not the model).
`blk=x/0`'s second field is always 0 outside pairwise mode (`block_eligible` is only
incremented there, `hyper.rs:2181`).

### 3. One flag discriminates

Each arm sets one flag to `0`, everything else default:

| arm | NoVerdict | timed-out pairs | user CPU | rows |
|---|---:|---:|---:|---:|
| default | 350 | 76,159 | 973 s | 1,564 |
| **`RUSTDL_HYPER_DOUBLE_BLOCK=0`** | **38** | **12,430** | **111 s** | **1,567** |
| `RUSTDL_SAT_SEED=0` | 350 | 76,660 | 719 s | 1,564 |
| `RUSTDL_ITERATIVE_DEEPENING=0` | 350 | 76,612 | 725 s | 1,563 |
| `RUSTDL_MRV_ORDERING=0` | 350 | 75,915 | 752 s | 1,565 |
| `RUSTDL_SAT_LOOKAHEAD=0` | 350 | 76,635 | 744 s | 1,564 |
| `RUSTDL_PRECISE_CARD_DEPS=0` | 350 | 76,643 | 752 s | 1,564 |

Closure diff vs HermiT (BFS closure, equivalences expanded, unsat excluded): default
**FP 0 / MISSED 23**; `RUSTDL_HYPER_DOUBLE_BLOCK=0` **FP 0 / MISSED 7**, still
`incomplete: true`.

**Caveats:**
- Four arms ran concurrently on 16 cores, so their walls mean nothing. The CPU totals and the
  NoVerdict counts are what count, and the NoVerdict count is shown above not to depend on
  the budget.
- The rows column counts Hasse rows, not the closure; use the closure diff, not the row count.
- Class counts: classify reports 1,375 named classes; `hyper-sat` iterates 1,377
  vocabulary ids (it includes `owl:Thing`/`owl:Nothing`).
- The `hyper-sat` probe, which never enables pairwise blocking, decides 1,371 of 1,377
  classes `Sat`. The label build stalls on 350. The probe's construction differs in other
  ways too (no seeds, no Q-root), so Step 1 must confirm the attribution on a single class.

### 3a. What the CPU drop is made of

Under `=0` the per-pair search is barely better: pass_through 5,580 + misses 7,805 = 13,385
pairs reached the per-pair search and **12,430 timed out (92.9%, vs 98.5% at default)**.
Almost the whole 973 → 111 s drop is pairs no longer *attempted*, because the label cache now
covers 1,337 classes and prunes them. That bounds what any label-cache fix can buy and is
why MISSED moves only 23 → 7: the per-pair search on the remaining pairs is still stalling.

### 4. Why the flag is not the fix

The ontology has 40 `owl:inverseOf` and 14 property chains. With inverses, anywhere
(single) blocking can accept a pre-model that is not a model. A wrong `Sat` is MISS-direction
under every consumer the flag reaches (`lib.rs` 1136, 1279, 1441, 4481, 4547, 4734, 5063):
- `trust_sat` refutation and the label prune: "not subsumed";
- the pseudo-model realize prune: a type not reported;
- consistency: a false `consistent`;
- the class-satisfiability / unsat probe: an unsat class reported satisfiable (which *adds*
  Hasse rows, so compare closures, not row counts).

None can create an FP, but each can hide an entailment silently. That is exactly the failure this project keeps finding (D10 shape).
The `=0` arm's 7 remaining MISSED should not be read as "the rest are fine".

### 5. Scale-dependence (from the earlier #139 investigation)

A 286-axiom robot BOT module around `PMD_0020227`/`BFO_0000017` classifies that missed pair
correctly. `rustdl subclass` proves 20 of the 23 missed pairs individually (11 in < 2 s,
9 in 16–33 s); the 3 involving `PMD_0010100` do not resolve in 60–300 s. `PMD_0010100` is
also one of the `hyper-sat` stalls (5 s, 23,407 branches, depth 256).

## Plan

### Step 1: pin the mechanism (≈ 1–2 days)

**1.0 — the deciding measurement, first (≈ 1 hour).** Pick one of the 312 classes that are
NoVerdict at default and `Sat` under `=0`. Record the **completion-graph node count at the
stall** and the branch count, with pairwise blocking on vs off.
- **Graph large under pairwise, small under anywhere** ⇒ generation does not terminate;
  continue with the blocking hypotheses below.
- **Graph small (< ~200 nodes) in both arms** ⇒ the search tree explodes (H4), and the `=0`
  win is mostly the ⊔-gating side effect. Step 3-as-blocking is then dead; redirect to the
  ⊔-search (branch ordering, dependency-directed backjumping coverage, or the ⊔-gating
  itself).

The cheapest instrument is a `--double-blocking` switch on `hyper-sat` (it never enables
pairwise blocking, `lib.rs:734`); that gives a per-class stall predicate without the classify
harness, and is also the ddmin predicate in 1.4.

1. Make `dump_label_cache` print class IRIs so the 350 can be named.
2. Add three counters to the `double_blocking` branch of `HyperEngine::is_blocked`
   (`owl-dl-tableau/src/hyper.rs`, around line 2166). Each counts one way a check fails:
   - no *earlier* candidate in the incoming-role bucket (the bucket itself is never empty —
     the node is pushed at `:4917`);
   - no candidate passed `labels(n) ⊆ labels(m)`;
   - a candidate passed that, but failed `labels(parent n) ⊆ labels(parent m)`.
3. At the stall, dump the generating chain: for each depth, the incoming role, `|labels|`,
   and the labels added relative to the nearest same-role earlier node.
4. Hypotheses, each with the evidence that would confirm it:
   - **H4 — the search tree explodes, not the model.** Confirmed or refuted by 1.0;
     currently the best-supported reading (§2).
   - **H1 — labels keep growing down the chain.** Chain clauses `R(x,y) ∧ S(y,z) → T(x,z)`
     add shortcut edges into deep nodes, and range/`∀` on `T` then adds labels there, so no
     earlier node is ever a superset. Expect counter (b) to dominate.
   - **H2 — incoming roles alternate.** Expect counter (a) to dominate.
   - **H3 — parent-label failure at the top.** The root carries the synthetic Q label and the
     seed labels, which no other node can have. Expect counter (c) to dominate, failing only
     at the depth-1 → depth-2 comparisons.
   - **H5 — static blocking hides constraints (bears on the residual 7 MISSED, not the
     stall).** `is_blocked` is evaluated once at `apply_exists` time (`:4904`) against the
     node's *partial* label. Under the default incremental fixpoint there is no re-seed, and
     there is no unblock/re-fire machinery. A node blocked early and then grown by Horn
     propagation keeps its skipped `∃`s skipped, which gives a spurious `Sat`. MISS-direction,
     but a completeness hole in *both* blocking modes.
5. Shrink to a reproducer with ddmin over the axioms, using **"class X stalls with pairwise
   blocking and is decided without it"** (via the `hyper-sat` switch) as the predicate. It is
   cheap: the adaptive cut detects a stall in about 200 ms per class. (A ddmin on the missed
   *entailment* was judged too costly earlier because it needs a full classify per probe.)
6. **Exit criterion:** the 1.0 verdict, plus a fixture of 30 axioms or fewer that reproduces
   it, with Konclude and HermiT verdicts recorded.

### Step 2: size the population (≈ half a day, in parallel with Step 1)

Two-arm sweep over the 1,920 ORE pool (`~/data/ore-run/pool_sample/files`). The arms are
`RUSTDL_HYPER_DOUBLE_BLOCK` at its default vs `=0`, run sequentially with the arm order
alternated and a 60 s cap. Record:
- label NoVerdict (from the dump) and label misses;
- timed-out pairs and the fallthrough `rescued` count;
- user CPU and the `ok/dnf` outcome.

This decides whether the fix is #139-specific or a class, and supplies the Step 4 census.
**Do not read the `=0` arm's answers as an oracle.** The arm measures the *flag*, which also
changes ⊔-gating and consistency, so report its gains as an **upper bound** on any blocking
fix, not as that fix's value.

### Step 3: fix pairwise blocking (shaped by Step 1; only if 1.0 says generation)

- **If H3, or another defect in the condition:** correct the condition, e.g. exclude
  synthetic/seed labels from the comparison. A small change, but it touches what blocking may
  conclude, so it needs the full gates.
- **If H1, i.e. the growth is genuine:** implement **core blocking with validation** (Glimm,
  Horrocks & Motik, *Optimized Description Logic Reasoning via Core Blocking*, IJCAR 2010;
  available in HermiT as the `SIMPLE_CORE`/`COMPLEX_CORE` options, whose default is
  anywhere pairwise *equality* blocking). Block on the *core* labels that caused the node's
  creation, then validate blocks once the graph is complete and unblock any whose blocked
  node's non-core constraints fail. Sound and complete for SHOIQ.
  **Effort: ≈ 1–2 weeks, not days.** This engine has no validate → unblock → re-expand
  machinery at all (H5), and core blocking requires it. Building that machinery first also
  closes H5 in the current blocking mode, so it is worth doing on its own.
- **If H2, or a doubt about the condition itself:** the implemented condition is **not** the
  one the code cites. `hyper.rs:2170` cites Motik et al. §3.4 (label *equality*);
  `:2206-2212` implements *subset* on node and parent labels with the same incoming role and
  an earlier blocker. Subset pairwise blocking (Horrocks & Sattler 2002) carries an extra
  `∀`-propagation condition that is absent here, and
  `docs/hypertableau-hf2-doubleblocking-scoping.md:238-241` justifies `==`→`⊆` by citing
  Motik 2009, which is equality. The discrepancy points toward spurious `Sat` (MISS), not FP.
  Any change must name the exact published condition it implements. The `:2164` comment
  ("no inverse roles enter the blocking condition here") is stale.

**Gates, all required:**
- the soundness net (`scripts/run-soundness-diff.sh`, host baseline 20 pass / 2 fail);
- `per_pair_fp_gate.rs`;
- the double-blocking canaries and gates in `docs/hypertableau-hf2-doubleblocking-scoping.md`;
- the #139 targets, **directional only** (they come from the `=0` arm, which is not an
  oracle): NoVerdict and MISSED should fall substantially toward 38 / 7, with FP 0;
- a full 1,920-ontology two-arm sweep, sequential and alternated, comparing triples (rows,
  unsat, equivalence groups) and adjudicating every DIFFER at ≥ 3 runs per arm;
- the corpus MISSED net. **Prerequisite:** regenerate its Konclude ∪ HermiT oracle set,
  which was removed from g1 on 2026-10-03.

**Direction of risk:** blocking earlier can only *suppress* generation, so an unsound block
yields a spurious `Sat`, which is a MISS, not an FP, under every consumer
(`trust_sat`, label prune, pseudo-model). The MISSED net is therefore the binding gate, not
the FP net.

### Step 4: stop the futile fallthrough (≈ 1–2 days; conditional)

Only if Step 2 shows the #139 pattern corpus-wide: `fallthrough ran=N rescued≈0` whenever the
wedge stalled on a class whose label build diverged. Then skip the per-pair tableau
fallthrough for subs whose label build was cut as diverging, and record those pairs as
undecided (`incomplete` stays honest). This is subtractive: it can only lose pairs the
fallthrough would have rescued, and the census says how many. Likely moot if Step 3 works.

### Step 5: per-class ⊥-module retry for residual undecided pairs (≈ 1–2 weeks; only if MISSED remains after Step 3)

Syntactic ⊥-locality modules for `{C}` preserve every named subsumer of `C`. The 286-axiom
BOT module recovered a pair that the full ontology misses. `locality.rs` is a co-occurrence
*component* analysis, measured as one component on every real workload
(`docs/module-extraction-plan.md` §A); it is not ⊥-locality, so this is new code. The cost is
a `PreparedOntology` per module, so restrict it to classes that still have timed-out pairs.

## What not to do

- **Optimise the matcher or the search hot loop.** The wall is deadline-bound (§1).
- **Raise `--pair-timeout-ms` or the label-cache budget.** Measured 15× slower with identical
  output; the label NoVerdict count is budget-insensitive.
- **Tune `RUSTDL_ADAPTIVE_BUDGET` / `DIV_WINDOW`.** Turning it off left the NoVerdict count
  unchanged and cost 21× label-build wall.
- **Default `RUSTDL_HYPER_DOUBLE_BLOCK=0`.** That trades a visible timeout for a possible
  silent miss on inverse-bearing ontologies (§4).

## Reproduce

```sh
B=./target/release/rustdl; O=<#139 ontology.owl>
RUSTDL_DUMP_LABELS=labels.txt $B classify $O > out.txt     # banner + label verdicts
grep -c ' noverdict' labels.txt                            # 350
RUSTDL_HYPER_DOUBLE_BLOCK=0 RUSTDL_DUMP_LABELS=l0.txt $B classify $O > out0.txt
grep -c ' noverdict' l0.txt                                # 38
$B hyper-sat $O | sed -n '/top classes/,$p'                # depth=256, restores==branches
```

HermiT reference: `robot reason --reasoner hermit` (JDK 17, `~/bench/jdk-17.0.20.1+1`).

## Review log

2026-10-04, technical review. Every evidence number re-derived from the run outputs except
one (rows 1,564 → 1,563 for `ITERATIVE_DEEPENING=0`, fixed). Accepted and folded in:
- the ⊔-gating confound and H4, with Step 1.0 as the deciding measurement;
- `depth` is search depth, not chain length;
- the per-pair search still times out 92.9% under `=0` (§3a);
- the implemented subset condition differs from the cited equality condition;
- H5 (static blocking, no unblock machinery) and the re-estimated core-blocking effort;
- all five MISS-direction consumers listed;
- the HermiT core-blocking options stated correctly;
- the Step 2 arm reported as an upper bound;
- the #139 targets made directional;
- the counter (a) wording.

