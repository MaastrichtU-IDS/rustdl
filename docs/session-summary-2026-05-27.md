# Session summary — 2026-05-25 → 2026-05-27

A multi-day push on rustdl performance and the default-mode gap to
HermiT/Konclude. ~35 commits from `ec8aefb` (output buffering) to
`f885d3e` (CDBL soundness finding). This doc is the index; each
area links to its own plan/diagnosis doc.

## Headline outcomes

**Shipped, user-visible wins:**

| Change | Effect | Commit |
|---|---|---|
| Top-down classify is the default | pizza 58 s → 29 s; **SIO DNF → 266 s** | `8c3fefa` |
| `--saturation-only` on 5 entry points | **SIO 266 s → 0.22 s** (1900×), sound under-approximation, 0.19 % edge loss | `adcdc7f` + 4 more |
| Lazy unfolding of Or-residuals + concept-rule Ors | **family 8.7 s → 6.3 s** (−28 %), verdicts unchanged | `1b41023`, `749ddd3` |
| BufWriter classify output | GO 37 s → 22 s | `ec8aefb` |
| `realize` uses top-down + gains `--saturation-only` | realize on SIO-scale no longer DNFs; family realize DNF → 0.16 s | `29f5b5d`, `c7bf996` |
| Per-call: early-presence check, inverse_pairs Vec | 5.6× fewer `add_label_calls`; removed a 14 % flamegraph frame | `2d843ae`, `5208c2b` |

**vs reference reasoners (ROBOT-docker), `--saturation-only`:**

| Workload | rustdl --sat | Konclude | HermiT |
|---|---|---|---|
| SULO | 0.01 s | 0.95 s | 3.94 s |
| pizza | 0.03 s | 1.44 s | 4.53 s |
| SIO | 0.22 s | 1.57 s | 69 s |

rustdl `--saturation-only` beats every reference reasoner on the
mostly-EL corpus (the trade: sound under-approximation, missing
tableau-only subsumptions — 0.19 % of edges on SIO, 20 % on pizza).

## The central finding: convergent vs timeout-bound walls

Every per-step optimisation this session moved **convergent-pair-
dominated** walls (family) and **none** moved **timeout-bound**
walls (pizza, SIO). The reason, pinned by counters
([`pizza-convergence-diagnosis.md`](pizza-convergence-diagnosis.md)):

- Pizza's per-pair tableau model is **small** (`add_edge_calls =
  965` cumulative). Blocking is **not buggy** — it correctly
  doesn't fire because sibling toppings carry genuinely different
  labels.
- The bottleneck is **search branching**: ~10 k disjunction-branch
  points explored per 200 ms probe without finding a satisfying
  assignment, because pizza's `DisjointClasses` make topping
  choices a constraint-satisfaction problem and the naive DFS
  re-discovers the same conflicts in every sub-tree.
- SIO is worse: per-class `sat(A)` itself **times out at 5 s** —
  the tableau can't build a model of a single SIO class, so
  there's nothing to cache and no completed model to reuse.

A timeout-bound wall = `per-pair-timeout × non-converging-pair-
count / parallelism`. Per-step efficiency can't shorten it; only
making pairs **converge** can.

## Architectural levers — status after the session

Full ranking + rationale in
[`architecture-roadmap.md`](architecture-roadmap.md).

| Lever | Verdict |
|---|---|
| Lazy unfolding (residual + concept-rule Ors) | ✅ **shipped**, family −28 %, sound. The one architectural win. |
| MOMS disjunct ordering | ✗ dead end — DL disjunctions are node-local; SAT MOMS doesn't transfer. 3 variants tried. |
| Syntactic module extraction | ✗ dead end — pizza/SIO have 1 signature component. |
| Model caching | ✗ dead for pizza/SIO — no completed model to cache (per-class `sat` times out). |
| CDBL (conflict-driven no-good learning) | ◑ **sound design found**; effective integration needs node-keyed no-goods (next increment). |
| Hypertableau | ⧗ multi-month; the general solution; not started. |

## CDBL — the soundness breakthrough

The original CDBL (branch-id-keyed no-goods) was unsound — it
changed pizza's verdict 2 → 0. This session built a **sound**
replacement ([`cdbl-plan.md`](cdbl-plan.md)):

- **Label-set no-goods**: "these concept labels co-occurring at a
  node are unsatisfiable" — structural, transferable across
  sub-trees (unlike run-local branch ids).
- **`verify_node_local_clash`**: the soundness gate — confirms a
  label-set clashes via node-local rules alone (no edges), which
  makes the no-good sound to reuse anywhere.

Phase 1 (decision tracking) + Phase 2a (verify) are **shipped and
tested**. The full record+lookup integration (Phase 2b/3) was
attempted and **held soundness** (pizza stayed at 2 unsat) but got
**0 lookup hits** because the no-good gathered decisions across
multiple nodes. The fix — node-keyed no-goods (propagate the clash
node up, key on the clash node's own decision labels) — is
precisely scoped for the next session.

## Diagnostic infrastructure added this session

- `RUSTDL_TRACE=1` — per-`search`/`branch` decision trace.
- `RUSTDL_COUNTERS=1` (+ `--features counters`) — per-rule call
  counts (pre-existing, used heavily).
- `rustdl locality-stats FILE` — signature-component histogram.
- `rustdl tbox-stats FILE` — absorbed-TBox rule + residual shape
  breakdown.
- `rustdl residual-triggers FILE` — lazy-unfolding trigger
  histogram.
- `scripts/bench-rustdl-modes.sh` — re-runnable mode comparison.

## Negative results documented (so they aren't re-run)

- MOMS — [`moms-plan.md`](moms-plan.md) §A.
- Model-caching root-labels — [`model-caching-plan.md`](model-caching-plan.md) §B.
- Syntactic module extraction — [`module-extraction-plan.md`](module-extraction-plan.md) §A.
- Lazy-fire residual Ors (vs proper deferral) — roadmap status table.
- CDBL cross-node keying — [`cdbl-plan.md`](cdbl-plan.md) §A.

## Recommended next steps (in priority order)

1. **CDBL node-keyed no-goods** — the one keying change to make the
   sound CDBL effective. Multi-week-ish; reuses everything built.
   Expected to help convergent workloads; pizza/SIO bounded by
   timeouts regardless.
2. **Hypertableau** — the only known path to pizza/SIO default-mode
   parity. Multi-month rewrite of the expansion algorithm.
3. **Coverage** (data properties, full role chains) — orthogonal to
   perf; unblocks ontologies that hard-error today.

For production use *now*, `--saturation-only` is the answer for
mostly-EL workloads and beats every reference reasoner on the
corpus.
