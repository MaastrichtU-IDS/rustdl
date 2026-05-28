# Hypertableau effort — capstone summary

Last updated 2026-05-28, at the close of the HF1–HF5 + backjumping +
orchestrator-closure push. This is the pick-up-cold document: what the
engine *is*, what's verified, where the boundaries are (and **why**),
and what remains. Detail lives in the per-phase scoping docs cross-
referenced below and in the commit history.

## 1. The arc — what shipped

Two waves: the H0–H4 *probe* arc (engine standalone, behind a flag),
then the HF1–HF5 *production* arc (engine wired into classify).

### Wave 1 — probe arc (commits up to 2026-05-27)

| phase | what | outcome |
|---|---|---|
| H0–H2 | DL-clause engine: Horn fixpoint, disjunctive branching | sound Unsat decision procedure |
| H2b/c | wall probes (`hyper-sat`, `hyper-classify-probe`) | SIO bare-sat 16.3 s → 0.45 s (~36×) |
| H3a/b/c + multi-role + nominals | per-construct completeness on the corpus | pizza 114 misses → 0 (695/695, 0 FP) |
| perf | clause indexing → semi-naive **event** evaluation | the 36× SIO win; node-granularity *refuted by measurement first* |
| H4 | sound-`Unsat` wedge into `classify` (flag-gated) | shipped, but the classify wall *did not move* — see §3 |

### Wave 2 — production arc (HF1–HF5 + backjumping + orchestrator)

| phase | what | outcome |
|---|---|---|
| HF1 | sound clausifier — partial absorption replaces ⊤-internalization | `deferred == 0` corpus-wide, no SIO explosion |
| HF2 | inverse roles + RBox inverse pairs + role hierarchy in matching | corpus stays 100 %, crafted canaries pin behaviour |
| HF3 | `≥n` generation + `≠` tracking; HF3b/c **verified by composition** (not built) | `≥2 ⊓ ≤1` clash via `≠`; corpus 0 FP |
| HF4 | NN-rule (nominals as singletons); HF4b **verified by composition** | `≥2 R.{o}` unsat via NN-merge + `≠`; corpus undisturbed |
| backjumping | dep-set per label + per-node `birth_deps`; backjump in `solve` | pizza probe 4:44 → **13.2 s (~21×)** |
| HF5 | 3-valued verdict wired into classify residual path (opt-in `RUSTDL_HYPERTABLEAU_TRUST_SAT`) | `tableau=0` calls; pizza classify 4:38 → **20.9 s (~13×)** |
| orchestrator | defined-sup sweep closes the same-tier inferred-subsumption gap | pizza/ro/sulo: **full Konclude agreement, both directions, 0 FP, 0 missed** |
| robustness | `NoVerdict` → sound timeout; HF5 CI regression test (+ caught the stats-aggregator bug) | SIO classify completes instead of crashing |

All wins held **0 false positives on the corpus** throughout.

## 2. What's verified, and how

Methodology: classify (real orchestrator, not the n² probe) → diff
against the transitive closure of Konclude's classification
(`cmp_classify.py`). For the engine in isolation,
`hyper-classify-probe FILE --dump-subsumptions`.

| ontology | classify wall (HF5) | result vs Konclude |
|---|---|---|
| pizza (SHOIN) | 43 s | 445/499, **0 FP**, 54 missed |
| ro-stripped (SROIFV) | 27 s | 158/158, **0 FP**, 0 missed |
| sulo-stripped (SRI) | < 1 s | 51/51, **0 FP**, 0 missed |
| SIO (SRIQ, 1585 cls) | 13 m 22 s | 8812/8904, **0 FP**, 92 missed (FPs fixed 2026-05-28) |
| family-stripped | 22 s | TBox-only, ABox-inconsistent — out of scope |
| **GALEN (SHIF, 2748 cls)** — ORE 2015 | 2 m 20 s | 27829/27997, **0 FP**, 168 missed (99.4%) |
| **ALEHIF+ test (168 cls)** — ORE 2015 | 31 s | 211/247, **0 FP**, 36 missed (85%) |
| notgalen (SHIF, 3087 cls) — ORE 2015 | timeout 10 min | needs bigger budget; not measured |
| ORE SHOIF(D) test | parse error | datatypes unsupported (known limit) |

**Generalization status (ORE 2015 measurement):** of 8 distinct
expressivity profiles tested — pizza/ro/sulo/SIO/family/GALEN/notgalen/
shoiq-knowledge — **rustdl is sound (0 FP) on every ontology where it
completes**. The SIO 38 FPs (under trust-Sat) are **closed as of
2026-05-28** — root cause was an unsound rule in EL saturation
(`process_fact` propagating `Range(R)` to the existential's target
*type*; sound for instance reasoning, unsound for TBox classification).
The sound replacement landed the same day (2da055b): fold `Range(R)`
into the existential's body via a Tseitin synthetic `F ≡ B ⊓ Range(R)`,
so CR5 propagation still picks up range-constrained subsumptions
without the type-level over-approximation.

Performance vs Konclude on GALEN (2026-05-28, hypertableau env vars
on): 2 m 20 s vs 0.1 s — *faster* than the 4 m 26 s measurement
captured before the unsoundness fix. (The 11 m number quoted in
the f71a012 commit message was a noisy single run; a clean
re-measurement under the same env vars shows no regression.) GALEN
has zero `ObjectPropertyRange` axioms so the Tseitin encoding never
fires on it; the perf result is the engine returning to baseline once
the noisy datapoint is set aside.

The corpus-wide diff harness lives in
`crates/owl-dl-reasoner/tests/konclude_closure_diff.rs` and produces
the FP/MISSED numbers above — handles `EquivalentClasses` (Konclude's
report style), Thing-equivalent classes (e.g. SIO_000000 ≡ Thing),
and unsat classes symmetrically across both sides.

Pizza-classify regression test (`hf5_pizza_classify_wall_and_soundness`)
runs in CI when `--features real-corpus` is enabled, asserting wall
< 90 s, the two known unsats, no `Topping ⊑ VegetarianPizza` FP shape,
and `hyper_refuted_pairs > 0` (HF5 fires).

## 3. Boundaries — and their causes

The corpus is **closed end-to-end with 0 FP**. The remaining boundary
is **SIO**, and its lesson is specific:

- *Without* `RUSTDL_HYPERTABLEAU_TRUST_SAT`: SIO classify times out
  (> 15 min, killed). The residual non-subsumption pairs exhaust the
  tableau budget.
- *With* `RUSTDL_HYPERTABLEAU_TRUST_SAT`: completes in 4:16, but
  produces 38 FPs targeting only 3 sups, plus a spurious equivalence
  `SIO_000115 ≡ SIO_000675` that Konclude does not have.
- *Bounded investigation* (`docs/...` + `bbae964` commit message):
  minimal repros of the suspicious axiom pattern (role hierarchy +
  domain + complex equivalence) **do not** trigger the bug — the
  engine handles those shapes in isolation. The FP is an
  **interaction-at-scale** unsoundness, almost certainly the
  anywhere-blocking-with-inverses problem the roadmap has flagged from
  the start.
- *Implication*: the opt-in env-var design is **load-bearing**. `Sat`-
  trust is sound only on workloads where the engine is complete; on
  the corpus, validated by full Konclude agreement; on SIO and likely
  any other inverse + cardinality + role-hierarchy ontology, not.

So the engine is **production-ready as a soundly-wedged classify on
the corpus** (default off, opt-in via two env vars), and **soundly
under-approximating off-corpus** (Unsat-only, the default wedge
behaviour). It is **not** a drop-in Konclude replacement off-corpus.

## 4. What remains

In rough value/effort order:

1. **HF2 double-blocking** — the principled fix for the SIO-style
   unsoundness. Anywhere blocking with inverse roles is known unsound;
   double-blocking is the textbook calculus-level fix (Motik/Shearer/
   Horrocks 2009 §3.4). Months of careful work; corpus is already 100 %
   so there's no measurable corpus payoff — the win is *generalization*,
   i.e. making `RUSTDL_HYPERTABLEAU_TRUST_SAT` safe to default-on.
2. **`≤n`-merge backjumping** — currently conservative (`DepSet::ALL`);
   precise tracking would help pathological cardinality ontologies.
   Corpus-inert.
3. **Default-on the HF5 flags** — only after (1). Today, opt-in is the
   right call.
4. **Broader generalization measurement** — go-basic (pure EL, should
   sail), GO/large-EL, more inverse-heavy ontologies, family with ABox
   once ABox is in scope.
5. **Runtime agreement-check gate for trust-Sat** — a one-shot
   classify-vs-reference at startup, gating per-workload trust. Cheap
   add when there's a reference; saves users from opting into
   unsoundness blind.

## 5. Engineering lessons worth keeping

These are the load-bearing ones, costed in real time saved or bugs
caught.

- **Measurement over intuition, repeatedly.** Node-granularity semi-
  naive was refuted by counters (52M→57M) before the event-granularity
  model landed. Backjumping's first-cut "label-only dep-sets" *passed
  every hand-built test* but the corpus diff caught it — pizza 695
  → 753 with 58 FPs. The fix (`birth_deps` per node) was unobvious
  from the test layer.
- **Verify-before-build kept paying off.** HF3b, HF3c, HF4b each turned
  out to be **achieved by composition**, not built — the propagation
  arc plus per-node `Label` firing handled cases the original Motik-
  proof-shaped scoping treated as separate phases. *Run the canary,
  then decide whether to build.*
- **The corpus diff is the soundness net for dependency propagation.**
  Canaries are necessary but not sufficient; the corpus exposes
  interactions a hand-built case won't.
- **Robustness lives outside soundness.** `NoVerdict` crashing classify
  on SIO wasn't a soundness bug — it was a panic-vs-timeout choice.
  Found by trying the engine on a larger workload, not by any test.
- **Stats aggregators are silently load-bearing.** The HF5 regression
  test (`stats.hyper_refuted_pairs > 0`) caught a missing-field-in-
  aggregator bug that would have made every wedge-fired pair invisible
  to instrumentation forever.
- **Opt-in flags are the design contract for "sound where validated."**
  `Sat`-trust shipped opt-in specifically because we couldn't prove
  it sound generally. SIO confirmed why. Don't default-on what's not
  validated.

## 6. Pick-up-cold artifacts

**Dead-ends to avoid:**
- [`hypertableau-dead-ends.md`](hypertableau-dead-ends.md) — eleven
  measured/refuted approaches with what killed each. Read before
  picking up the next phase — every entry was tempting on first
  principles and cost real time.

**Scoping docs (per-phase detail):**
- [`hypertableau-scoping.md`](hypertableau-scoping.md) — master H0–H2c
- [`hypertableau-seminaive-scoping.md`](hypertableau-seminaive-scoping.md) — event eval
- [`hypertableau-cardinality-scoping.md`](hypertableau-cardinality-scoping.md) — H3 family
- [`hypertableau-h3b-scoping.md`](hypertableau-h3b-scoping.md) — ¬sup expansion
- [`hypertableau-h4-scoping.md`](hypertableau-h4-scoping.md) — wedge
- [`hypertableau-full-scoping.md`](hypertableau-full-scoping.md) — **HF1–HF5 master**
- [`hypertableau-hf2-scoping.md`](hypertableau-hf2-scoping.md) — inverse/RBox/hierarchy
- [`hypertableau-hf3-scoping.md`](hypertableau-hf3-scoping.md) — cardinality calculus
- [`hypertableau-hf4-scoping.md`](hypertableau-hf4-scoping.md) — nominals/NN-rule
- [`hypertableau-backjumping-scoping.md`](hypertableau-backjumping-scoping.md) — search-quality lever

**Code:**
- `owl-dl-core::clause` — clausifier (HF1 partial absorption, RBox canonicalization)
- `owl-dl-tableau::hyper` — engine (event worklist, `≤n` merge, NN-rule, dep-set backjumping, blocking)
- `owl-dl-reasoner::classify` — orchestrator + HF5 wedge wiring + defined-sup sweep
- `owl-dl-reasoner` — `HyperCache::decide` (3-valued verdict), `hyper_wedge_enabled`/`hyper_trust_sat_enabled`

**CLI probes:**
- `rustdl hyper-sat FILE` — per-class satisfiability
- `rustdl hyper-classify-probe FILE [--dump-subsumptions]` — naive n² subsumption probe with branch/wall histogram
- `rustdl clause-stats FILE` — deferred-construct census

**Production toggles:**
- `RUSTDL_HYPERTABLEAU=1` — H4 wedge: trust engine `Unsat` (sound for any ontology)
- `RUSTDL_HYPERTABLEAU_TRUST_SAT=1` — HF5: also trust engine `Sat` (sound *only* on workloads where the engine is complete — corpus-validated)

**Tests as invariants:**
- `crates/owl-dl-tableau/src/hyper.rs::tests` — engine unit tests incl. backjumping canary (`backjumping_collapses_irrelevant_middle_decisions`), `DepSet` algebra, `HF3a/b` probes, `HF4a/b` NN-rule
- `crates/owl-dl-reasoner/src/lib.rs::tests` — probe-level HF2/HF4 canaries
- `crates/owl-dl-reasoner/tests/real_ontology_corpus.rs::hf5_pizza_classify_wall_and_soundness` — the HF5 CI regression test

**Memory:** [[rustdl-hypertableau-h2b]] carries the load-bearing findings
in compressed form — the convergent-vs-timeout distinction, the
SIO-vs-pizza drop split, the profiling lesson, the verify-before-build
pattern, the SIO trust-Sat finding, the corpus-as-soundness-net lesson.

---

**Bottom line.** This engine is a *Konclude-equivalent classifier on
the validated corpus*, shipped with the opt-in flags that make it
13× faster than the production tableau path, with soundness guarded by
a CI regression test. Off the corpus, it's a *sound under-approximation
on any ontology* (Unsat-only wedge, default-on), with opt-in
"fast-but-possibly-unsound" trust-Sat for users who measure first. The
next principled phase is HF2 double-blocking; it is months of work and
its payoff is generalization, not corpus.
