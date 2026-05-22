# rustdl performance plan: outperform HermiT

Drafted 2026-05-22. The strategy memory's bench table is the
ground truth for where we start; this plan lays out what would
have to land for "rustdl ≥ HermiT" to be a credible claim across
the workloads where we currently lose.

## Today's gap (16-core release vs single-threaded HermiT)

| Workload | rustdl | HermiT | Gap |
|---|---|---|---|
| anatomy.ofn (pure-EL, 31 classes) | 85 µs | unknown but ≥ | rustdl wins |
| Synthetic EL n=200 | 145 ms | 188 ms | rustdl wins |
| Synthetic EL n=400 | 1.35 s | 338 ms | **4×** |
| SULO classify `--pair-timeout-ms 200` | 466 ms | 43 ms | **11×** |
| SIO classify | does not finish in 90 s | minutes | **∞** |
| family.rdf.owl (length-3 chains) | rejected / timeout | inconsistent in 8 s | coverage gap |

We win on pure EL up to ~200 classes; we lose decisively on
SROIQ classify at any meaningful scale. Differential corpus
verdicts match HermiT on 87/87.

## Where the gap actually lives

HermiT is the reference for "how fast can a SROIQ tableau go" —
~15 years of accumulated optimization. The big architectural
levers HermiT pulls and rustdl currently doesn't:

1. **Hypertableau** (Motik 2008) — the actual algorithm HermiT
   runs. Resolution-based rule clauses instead of NNF
   propagation; smaller branching factor by construction.
2. **Lazy unfolding** — concepts that appear in axioms are not
   added to every node; expansion is deferred until a node's
   labels actually trigger the rule. Saves orders of magnitude
   of redundant work on large TBoxes.
3. **Model caching** — once a class is shown satisfiable, the
   witness model is cached and reused when testing supersets of
   that class. The next subsumption query that needs a model of
   `C ⊓ X` starts from `C`'s cached model rather than rebuilding.
4. **Anywhere blocking** + **subset blocking** combined — more
   aggressive blocking than our pair blocking, prunes infinite
   model exploration earlier.
5. **Disjunct ordering heuristics** — MOMS-style selection of
   which disjunct to try first dramatically affects backtrack
   depth on hard cases.

rustdl-side advantages we haven't fully exploited:

1. **16 cores** — HermiT is JVM single-threaded. We already use
   rayon in classify's pair loop and realize's individual loop.
   Saturation, tableau-search, and the per-pair classify body
   are still single-threaded.
2. **No GC pauses** — Rust's deterministic memory makes p99
   latency predictable in ways the JVM struggles with. Mostly
   shows up at scale and on tight allocators.
3. **Determinism** — already shipped (commit `b7857f5`). Same
   input → same timings. HermiT's run-to-run variance is
   harder to characterize.

## The two-pronged strategy

To close an 11–40× gap, **neither** "make per-call tableau 11×
faster" alone **nor** "use 11× more cores" alone is realistic.
We aim for a multiplicative win:

- **5–10× from per-call tableau speed** (Phase B + C below)
- **3–4× from broader parallelism** (Phase D below)
- → 15–40× combined

At the high end that's "comfortably beat HermiT on SULO at
50 ms or below." At the low end it's "competitive within 2–3×."
Either is a credible "rustdl is a serious DL reasoner" position.

## Phase A — Profile (1 session)

Before any optimization, **know where the time goes**. Without
this every B/C/D-phase decision is guessing. Concrete deliverables:

- A.1 Per-rule call counters on `TableauContext` (behind a
  `cfg(feature = "profile")` flag, off by default). Counts how
  many times each `apply_*` fires across a classify run.
- A.2 `cargo flamegraph` instructions in `docs/profiling.md` and
  a baseline flamegraph for SULO + corpus-86 committed as PNGs
  or SVGs in `docs/flamegraphs/`.
- A.3 Top-3 hottest functions identified with numbers in the
  strategy memory. Every later phase justifies its target
  against this baseline.

## Phase B — Per-call tableau speed (3–5 sessions)

### B.1 DepSet representation tuning (1 session)

Current `DepSet = Vec<u32>` allocates 24 bytes header per slot
even when empty. Phase 4's per-rule propagation pays this every
firing. Try:
- `SmallVec<[u32; 1]>` — inline the typical single-`branch_id`
  case, fall back to heap for many-branch sets
- `u128` bitmask — single CPU instruction for union, bounded at
  128 concurrent branches (fine for any practical search depth)
- `Option<Box<[u32]>>` — None for empty (the common case), heap
  for non-empty

Decision is data-driven: profile says which case dominates.
Target: shave 30 % of post-Phase-4 corpus overhead, i.e. corpus
76 → ~55 ms (closing on the pre-Phase-4 34 ms baseline).

### B.2 Lazy unfolding (2–3 sessions)

The hot loop in `saturate()` reapplies every absorbed rule to
every node every iteration. With axiom indexing (commits
`d744021` + `693a921`) we now look up *which* rules apply to a
given trigger, but the rule itself still adds its conclusion
eagerly to every matching node.

Lazy unfolding defers conclusion-label addition until that label
is actually needed by some downstream rule. Standard technique;
the implementation refactor is the work:
- B.2.1: rewrite `apply_concept_rules` to track a per-node
  "pending triggers" set and only fire when the trigger is
  observed.
- B.2.2: same for `apply_role_rules`.
- B.2.3: same for `apply_residual_gcis` — currently each node
  carries every residual GCI eagerly; the largest single
  inefficiency in our saturation.

Target on SULO: 10× per-call tableau speed (from ~25 ms typical
call down to ~2.5 ms). HermiT's documented gain from this
technique alone is roughly an order of magnitude on TBoxes with
many GCIs — SULO qualifies.

### B.3 Disjunct ordering heuristics (1 session)

`search::branch` tries options in ConceptId-sort order today.
The literature's MOMS (Maximum Occurrence in Minimum Size)
ordering picks the disjunct most likely to close the branch
quickly. Implementation:
- Compute a static "weight" per disjunct: number of clauses
  it appears in, weighted inversely by their size.
- Sort options by descending weight at branch entry.

Target: 20–40 % wall-clock reduction on
`16_forall_split_disjunction_sat` and similar branching-heavy
fixtures.

### B.4 Model caching across classify pairs (2–3 sessions)

Each pair `(i, j)` in classify currently builds a fresh tableau
context: clone the pool, seed ABox, add the test concept, run
search. The pool + ABox seeding is shared via `PreparedOntology`,
but everything inside the tableau is rebuilt.

The shape of the win: when `i ⊑ k` was confirmed satisfiable in
an earlier pair, the witness model of `i ⊓ ¬k` (or its
satisfiable abstraction) carries information that's reusable
when probing `i ⊓ ¬j`. Standard HermiT trick.

Concrete design:
- After a satisfiable verdict, snapshot the saturated graph
  (labels per node, structural fingerprint).
- For a future pair sharing the `sub` class, attempt to reuse
  the snapshot — discard if it conflicts with the new test
  concept, otherwise continue from there.
- Soundness invariant: cached models are *abstractions* — they
  prove satisfiability when reusable, never claim
  unsatisfiability from cache.

Target on SULO classify (which does 17 per-class unsat probes
and 210 pair-subsumption checks): 5–10× wall-clock reduction.

## Phase C — Algorithmic upgrades (research-grade, 7–13 sessions)

### C.1 Hypertableau (4–8 sessions)

The big one. HermiT's actual reasoning algorithm. Reference:
Motik, Shearer, Horrocks 2009 "Hypertableau Reasoning for
Description Logics". The shift is large:
- Axioms compiled into resolution-style HT-clauses, not NNF
  with absorbed `ConceptRule` / `RoleRule`.
- Rule application becomes clause resolution instead of label
  propagation.
- Branching factor of disjunction drops because clauses already
  encode the case analysis.

This is the project's biggest single architectural decision.
"Replace the tableau crate" scale of work. Worth it if and only
if Phase B's gains plateau before HermiT parity.

### C.2 Saturation inverse-chain (3–5 sessions)

Extend the EL saturation engine to handle inverse roles in
chain positions (e.g. SULO's `hasParticipant ∘ inverse(hasFeature) ⊑
hasParticipant`). Predecessor existentials needed:
- `Subsumers` gains an "every C-instance has at least one
  r-predecessor in D" relation alongside the existing successor
  relation.
- CR5-style propagation extended with the new fact type.
- Chain rule matches polarity per position (which the *tableau*
  chain rule already does — saturation just needs to catch up).

If this lands, SULO classify takes the pure-EL fast path and
runs in **milliseconds**, not seconds. The whole reason SULO is
expensive is that this one chain pattern keeps it out of the EL
saturation path.

## Phase D — Parallelism amplification (2–3 sessions)

### D.1 Concurrent saturation worklist (2 sessions)

The saturation engine's worklist is single-threaded today (the
strategy memory's "deferred pending profiling" item). BSP-style
chunking:
- Split the worklist into N chunks, one per core.
- Each chunk processes facts in parallel.
- Synchronize between rounds.

Target on synthetic-EL n=400: 4–8× speedup (1.35 s → 170–350 ms).
This is the path to "rustdl is the fastest EL reasoner in
Rust, period." Currently ELK and whelk both beat us at this
scale.

### D.2 Parallel disjunction exploration (1 session)

When `branch()` faces ≥ 4 disjuncts, explore them in parallel
on rayon (each branch gets its own checkpointed TableauContext
clone). With back-jumping (Phase 4 shipped), a sibling's clash
deps can prune still-running peers. Modest wall-clock win on
heavily-branching workloads.

## Estimated timeline and milestones

Assuming ~one session per item, ~half a working day per session:

- **Phase A**: 1 session → baseline established
- **Phase B**: 5–7 sessions → cumulative 5–10× per-call speedup
- **Phase C**: 7–13 sessions → hypertableau replaces tableau OR
  saturation inverse-chain lifts SULO into the fast path
- **Phase D**: 2–3 sessions → parallelism multiplier

**Total: 15–24 sessions to a credible "outperform HermiT" claim
across SULO + SIO.** That's roughly half of the strategy v2
plan's remaining 12-month budget. The shape of the win at each
milestone:

- After A + B.1: SULO 466 → 350 ms (still 8× behind)
- After A + B.1 + B.2: SULO 466 → ~80 ms (within 2×)
- After A + B.1 + B.2 + B.4: SULO 466 → ~30 ms (**at parity**)
- After + C.2: SULO drops into the pure-EL path → ~5 ms
  (**comfortably ahead**)
- After + C.1 (hypertableau): SIO becomes finishable
- After + D.1: synthetic-EL n=400 falls to ~200 ms (ahead of
  HermiT, competitive with ELK)

## What we can claim today, without further work

- Pure-EL workloads up to ~200 classes: rustdl already wins
- Consistency checks: under 10 ms on every input we've measured
- Reproducible per-process timings (no JVM warm-up, no GC)
- Sound across 87/87 differential fixtures
- The full Phase 4 tableau-optimization stack (DDB + restricted
  semantic branching + tier-parallel top-down classification)
  is in place — exactly the foundation Phase B and C build on.

## Risks and dead-ends to avoid

- **Phase 4 attempt 1 dead-end** (strategy memory): semantic
  branching without DDB regresses corpus 2×. Stay disciplined
  about the order of operations.
- **Phase 4 attempt 2 dead-end**: `apply_max` precise deps cost
  more than the over-approximation they replace at corpus
  scale. Bigger ontologies might flip this — measure before
  re-trying.
- **Top-down row-incremental** (this session): better algorithm
  serialised too much, lost to flat-parallel naive. Tier-
  parallel recovered some but still loses on SULO.
- **Hypertableau is one-way** — the conversion from existing
  tableau is irreversible. Don't start C.1 without solid
  Phase B numbers showing standard tableau has plateaued.

## What this plan deliberately doesn't include

- Datatypes / data property reasoning. Coverage gap, not a
  speed gap. Phase 3 deliverable per strategy v2.
- Length-3+ role chains. Same — coverage, not speed.
- SWRL or rule extensions. Out of scope for the "outperform on
  the implemented fragment" goal.
- Cluster / distributed reasoning. Single-machine wins first.

## How to decide what's next when picking up this plan

The honest order is **A → B → C → D**, but the order *within*
each phase is data-driven from the profiling baseline. If A.3
shows the dep-set bookkeeping is genuinely cheap (< 5 % of
runtime), skip B.1 and go straight to B.2 (lazy unfolding,
where the literature predicts the biggest single win).

If at any point the bench numbers stop moving despite 2+
consecutive sessions of work in a given direction, **stop and
reassess** — the assumption underlying that phase has gone
wrong. The strategy memory's "Tried-and-dropped" section is
where to record the finding so the next session doesn't repeat
it.
