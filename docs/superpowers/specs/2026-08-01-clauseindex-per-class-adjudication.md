# Adjudicating R3 vs R4 on the per-class `ClauseIndexes` rebuild

**Site:** `crates/owl-dl-reasoner/src/lib.rs:2958` — `classify_labels` obtains a
`HyperEngine` per **class**.

Two reviewers examined the same code on the same day and reached opposite verdicts. This
document is the experiment that decides between them, written *before* running it so the
result cannot be rationalised after the fact.

## The two claims

**R3 — CONFIRMED, and it is a DNF-scale lever.** `classify_labels` full-rebuilds
`ClauseIndexes` per class because SP2.1/SP3 seed clauses are absent from `base_indexes` while
`RUSTDL_SAT_SEED` defaults ON, so v0.3.39's per-**pair** amortization never reaches the
label-cache build. Measured 31.00% inclusive + 6.31% drop on `ore_ont_1508`, vanishing
entirely under `RUSTDL_SAT_SEED=0`. Isolated effect **209.6 → 119.9 s** (`1508`) and
**109.9 → 52.8 s** (`12698`), closures byte-identical in both.

**R4 — SUSPECTED, and the measurement is confounded.** `classify_labels` takes
`HyperEngine::new` (a full O(#clauses) index rebuild per class) whenever `sat_seed.is_some()`,
which is always, so the amortized branch is dead code. But its setup-only measurement
(19,987 ms vs 6,414 ms on `ore_ont_10080`) is **confounded by seed-clause removal** — turning
`SAT_SEED` off does not only disable the rebuild, it also removes clauses, so less work of
*every* kind is done.

**They agree on the mechanism and disagree on whether the measurement isolates it.** R4 is
right that `SAT_SEED=0` is not a clean intervention. R3's numbers come from the same
intervention, so R3's headline figure inherits the same confound — even though R3's
conclusion may still be correct.

## Why this cannot be settled by re-reading the profiles

Both used `RUSTDL_SAT_SEED=0` as the off-arm. That flag changes **two** things at once:
1. the per-class `ClauseIndexes` rebuild stops being forced, and
2. the seed clauses themselves disappear, shrinking every downstream loop.

No amount of attribution within that A/B can separate them. A third arm is required.

## The experiment

Build **three** binaries, each pinned to a uniquely named path immediately after its build
(a shared `target/release/` path has produced two retracted results in this project):

| arm | configuration | isolates |
|---|---|---|
| **A** | stock v0.4.8, `SAT_SEED` ON | the status quo |
| **B** | stock v0.4.8, `RUSTDL_SAT_SEED=0` | R3's and R4's off-arm — **confounded** |
| **C** | **seed clauses PRESENT, index rebuild AMORTIZED** — wire the existing `hyper.rs:1349/1167` amortizer into `classify_labels` behind a flag | **the rebuild alone** |

**C is the whole point.** It keeps the clauses and removes only the rebuild, so `A − C` is
the rebuild's true cost while `A − B` is the rebuild *plus* the clause volume. If
`A − C ≈ A − B`, R3 is right and the lever is real. If `A − C` is small while `A − B` is
large, R4 is right: the win was mostly the clauses, and wiring the amortizer buys little.

**Predictions, recorded now:**
- R3 correct ⟹ `ore_ont_1508` arm C lands near 119.9 s.
- R4 correct ⟹ arm C stays near 209.6 s and the gain seen in B was the clause volume.

Either outcome is publishable internally; a refuted R3 is as useful as a confirmed one, and
saves building a lever that does not pay.

## Protocol

- Ontologies: `ore_ont_1508` and `ore_ont_12698` (both cited by R3, both currently complete,
  so wall is a real measurement rather than a cap).
- `RAYON_NUM_THREADS=1`, `( ulimit -v $((24*1024*1024)); timeout 400 … )`, **one at a time,
  on an idle host** — no sweep, no other agent. Contention would inflate the arm that happens
  to run alongside something else and could invent or erase the entire effect.
- Min-of-3 interleaved (A,B,C, A,B,C, A,B,C), not three runs of A then three of B — machine
  drift over minutes is comparable to the effect size being argued about.
- **Verify each binary by sha256 before use**, and confirm arm C actually took the amortized
  path rather than silently falling back. An instrument that cannot fire has already been
  measured in this project for three rounds, with silence read as data.
- Closures must be **byte-identical across all three arms**. Arm C changing any verdict is a
  bug, not a win.

## Decision rule, fixed in advance

- `A − C ≥ 25%` on both ontologies, closures identical ⟹ **build it**, promote through the
  normal gate (flag default OFF → canaries → sabotage → FP=0 net → flag-OFF byte-identity).
- `A − C < 10%` ⟹ **R4 is right**; record the refutation, do not build, and correct the
  characterization doc's ranking, which currently lists this as the largest unclaimed lever.
- In between ⟹ report the number and decide on cost, not on the prior.

## Note on scope

R4 separately reported that `classify_labels`'s `self.clauses.clone()` (`lib.rs:2895/2896`)
measures **1.82 ms/class**, matching the long-recorded 0.55–6.3% and **not** a DNF lever —
while R3 measured **20.28% inclusive** on `ore_ont_1508`. That is a second, smaller
disagreement in the same function, and arm C will incidentally settle it, since amortizing the
index makes the clone's share directly observable.
