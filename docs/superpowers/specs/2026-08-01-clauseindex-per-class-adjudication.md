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

## OUTCOME (2026-08-01) — **R3 CONFIRMED**, and by a wider margin than it claimed

Run exactly as specified: `RAYON_NUM_THREADS=1`, `ulimit -v 24 GiB`, `timeout 400`,
`--pair-timeout-ms 20`, one probe at a time on an idle host, **min-of-3 interleaved**
(A,B,C ×3), each binary pinned to a uniquely named path and `sha256sum`-verified:

- `rustdl-armA-stock-v049` `edc535a95cbc06a2336edb24388ec6aa45db5ec7754ddfabb91234aad1f97056`
- `rustdl-armC-labelsamortize-flagged` `9e009c22b025d8ad5f314a5eea5e231505a429ccccb5d7f001e90c90be7dece3`

| ontology | A (stock) | B (`SAT_SEED=0`) | C (seeds kept, index amortized) | A−B | **A−C** |
|---|---|---|---|---|---|
| `ore_ont_1508` | 197.83 s | 114.12 s | **94.89 s** | 42.3% | **52.0%** |
| `ore_ont_12698` | 98.78 s | 49.80 s | **5.33 s** | 49.6% | **94.6%** |

`A − C ≥ 25%` on both ⟹ by the decision rule fixed above, **the lever is real**.

**C beats B on both ontologies**, which is the finding that settles the dispute on its
merits rather than on arithmetic. R4 was *methodologically right* that `SAT_SEED=0`
confounds rebuild with clause volume — but its substantive suspicion (that "the win was
mostly the clauses, and wiring the amortizer buys little") is **refuted**: keeping the
clauses and removing only the rebuild is *better* than removing both. The per-phase
`label_cache_build` counter isolates it without any whole-wall confound:

| ontology | A | B | C |
|---|---|---|---|
| `ore_ont_1508` `label_cache_build` | 163.3 s | 86.6 s | **59.9 s** |
| `ore_ont_12698` `label_cache_build` | 95.7 s | 49.9 s | **2.05 s** |

R3's own prediction was "C lands near B (119.9 s) on 1508"; C landed at 94.9 s. R3 also
predicted the seed's *value* is elsewhere and must be kept — that stands.

**Proof arm C took the amortized path** (the protocol's instrument-must-fire clause): a
one-shot stderr marker prints on the FIRST use of each branch under
`RUSTDL_LABEL_AMORTIZE_MARK=1`. Both branches were shown able to fire (flag OFF prints
`full-rebuild`; flag ON prints `engaged`; the stock arm-A binary prints neither, having
no marker code). **All 6 timed arm-C runs printed `engaged` and ZERO printed
`full-rebuild`** — so every class in every measured run took the delta path.

### Closure identity — and the budget-nondeterminism trap it exposed

At a **non-truncating** budget (`--pair-timeout-ms 1000`, where *no* pair times out on
either arm) `ore_ont_1508` is **byte-identical, 13 950 rows**, A vs C — while C still runs
97 s vs A's 199 s. `ore_ont_12698` is byte-identical at `--pair-timeout-ms 20`.

At the truncating `--pair-timeout-ms 20`, one arm-C sample differed from A by 4 `direct`
rows. **That is budget noise, not an arm effect, and the initial 2-sample A-vs-A control
was too small to reveal it.** Five samples of the SAME binary vary in timed-out-pair
count — A: 61, 57; C: 68, 64, 58 — and C-r1 differs from C-r4/C-r5 by *exactly the same
4 rows*, i.e. the difference reproduces **within** one flag setting. C-r4 and C-r5 are
byte-identical to A-r1 and A-r2. The complete (`pt1000`) closure contains neither of
C-r1's extra rows, so "C is more complete" would also have been the wrong reading: under
a per-pair wall-clock deadline, *which* pairs get decided is a race, and the transitive
reduction shifts with the decided set. **Any future A/B on this code must compare at a
non-truncating budget**; the in-tree gate is pinned to `--pair-timeout-ms 1000` for
exactly this reason.

### Gate

Flag `RUSTDL_CLASSIFY_LABELS_AMORTIZE`, **default OFF** (`=1` opts in). fmt clean; clippy
`--workspace --all-targets --all-features -D warnings` clean; `cargo test --workspace
--exclude owl-dl-py` **1418 passed / 0 failed**. FP=0 net **flag OFF and flag ON**, both
**11 VERIFIED, all closures exact** (galen 27997, notgalen 32739, sio 8904, ore-10908
6001, wine 653, pizza 499, alehif 247, ro 158, ore-15672 142, sulo 51, bibtex 16) with
the same 3 documented-absent stripped fixtures.

**Sabotage: 4 run, 2 caught, 2 survived** — reported including survivors.

| sabotage | result |
|---|---|
| delta built at `base_len + 1` | **CAUGHT** — 2 canaries fail (index out of bounds) |
| flag default flipped to ON (`is_some_and` → `is_none_or`) | **CAUGHT** — `flag_defaults_off` fails |
| amortized path silently drops the seed clauses (`extras.truncate(1)`) | **SURVIVED** |
| amortized path handed an EMPTY `disjoint_pairs` overlay | **SURVIVED** |

Both survivors are understood and recorded in the canaries' own doc comments rather than
papered over:

- **Seed-drop survives** because these probes pass `deadline: None`, and the SP2.1/SP3
  seed is a *convergence* aid, not a source of new entailments — with no deadline the
  probe converges anyway, so on a small fixture the seed is redundant. Pinning seed
  presence would require racing a deadline (flaky), so it is deliberately not done. What
  guards the seed instead is structural: both paths now consume the **same** `extras`
  vector. Residual risk, stated plainly: a future edit that drops seeds *only* on the
  amortized path would be a silent completeness regression on the nominal-heavy inputs
  SP2.1 exists for, and neither the canaries nor the curated corpus would catch it.
- **Empty-overlay survives** because the unsat fixture's clash comes from firing the
  ⊥-headed clause `B(X) ⊓ F(X) → ⊥` in the shared base clause set, not from the
  `disjoint_pairs` overlay (a merge/`≤n` shortcut). Losing the overlay would cost a
  pruning shortcut, not a verdict — benign, but the test says nothing about overlay
  plumbing, and its original comment claiming otherwise was corrected.

**METHOD NOTE, worth carrying:** the first attempt at sabotage #1 patched the *wrong
function*. The literal text `build_clause_index_delta(self.clauses.len(), &extras,
idx_hier)` occurs twice in `lib.rs`, and the first occurrence is the **shipped per-pair
`decide_with_stats`** — so a `replace(..., 1)` silently sabotaged production code instead
of the new path, "survived" all four canaries, and only a stack trace naming
`hyper_decide` revealed it. Incidentally it proves that off-by-one in the *per-pair*
amortizer panics on `ore_ont_10019` and that no in-tree test catches it — a separate,
unclaimed coverage gap. Anchor a sabotage by line number and assert on the surrounding
text; a survived sabotage is worthless evidence until you have proved it was applied
where you meant.

## Note on scope

R4 separately reported that `classify_labels`'s `self.clauses.clone()` (`lib.rs:2895/2896`)
measures **1.82 ms/class**, matching the long-recorded 0.55–6.3% and **not** a DNF lever —
while R3 measured **20.28% inclusive** on `ore_ont_1508`. That is a second, smaller
disagreement in the same function, and arm C will incidentally settle it, since amortizing the
index makes the clone's share directly observable.
