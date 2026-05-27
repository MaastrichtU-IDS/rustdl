# Hypertableau H4 — wiring the engine in as a sound accelerator (scoping)

Drafted 2026-05-27. The engine is sound, ~3× Konclude on SIO, and
corpus-complete on TBox subsumption (pizza/ro/sulo 100 %, SIO 0-unsat,
all 0 false positives, HermiT-corroborated). H4 wires it into the
reasoner so real queries benefit — **before any code**, per the
H1/H3b/H3c pattern.

## §0 — Soundness foundation (why this is safe)

`hyper.decide(sub ⊓ ¬sup) == Unsat ⇒ sub ⊑ sup`, for **any** ontology.
The clause set is obtained from the axioms by sound transformations
(NNF, structural) that only ever **drop** or **weaken** constraints —
deferred constructs are dropped; a nominal `{a}` is relaxed to a plain
class. Dropping/weakening enlarges the model set:
`Models(ontology) ⊆ Models(clause-set)`. So if the clause set has *no*
model of `sub ⊓ ¬sup` (`Unsat`), neither does the ontology — the
subsumption genuinely holds. `Sat`/`Stalled` carry **no** information
(could be a real non-subsumption, or one hyper can't see), so they
must fall through to the complete engine.

This is the same asymmetry validated empirically in §H2b–nominals (0
false positives across the corpus and two reference reasoners).

## §1 — Integration: a per-query, `Unsat`-only wedge

The orchestrator's subsumption decision is: reflexive shortcut → EL
saturation (sound, fast) → pure-EL completeness shortcut → **tableau**
(`sub ⊓ ¬sup` satisfiability — the slow path that times out on
pizza/SIO). The wedge goes **between the EL-saturation miss and the
tableau**:

```
… EL saturation says no, not pure-EL …
if HYPER ENABLED && hyper.decide(sub ⊓ ¬sup) == Unsat { return true }   // sound, fast
… existing tableau …
```

Two call sites reach the tableau after EL saturation:
- `classify.rs::subsumes_via_tableau` (the N² classify pair loop, via
  `prepared.decide`) — **the site that matters**, hit per pair.
- `lib.rs::is_subclass_of_internal_full` (single-query, via
  `run_satisfiability`).

Factor one helper — `hyper_proves(cache, sub, sup) -> bool` — and call
it in both, just before their tableau call. `classify`,
`is_class_satisfiable`, and `is_subclass_of` all funnel through these,
so one helper covers every caller.

**`Unsat`-only. No complete-mode.** The temptation — "if the clausifier
deferred nothing, trust `Sat` too and skip the tableau" — is
explicitly *out of scope*. The "fully supported" check (deferred count,
`match_body`'s own defer paths, ABox presence, every H3/multi-role
corner) is fiddly, and getting it wrong leaks **false negatives**
silently. The `Unsat`-only path delivers the actual win — the
wall-bound positive subsumptions EL saturation can't prove and the
tableau is slow on — with zero soundness risk. Revisit complete-mode
only if a workload measurably needs it.

## §2 — The per-call cost: cache in `PreparedOntology`

`is_subclass_of_internal_full` is called **per pair** in spirit; the
classify loop calls `subsumes_via_tableau` per pair against a shared
`PreparedOntology`. Clausifying per pair would be catastrophic
(convert→clausify→indexes→`sup_neg` pre-pass on every call).

`PreparedOntology` (built once in `from_internal`, shared across the
pair loop — already holds the absorbed TBox, hierarchy, ABox) is where
the clausified state goes. Add a `HyperCache`:

```rust
struct HyperCache {
    clauses: Vec<DlClause>,           // base clauses + complement clash clauses
    indexes: Rc<ClauseIndexes>,       // built once (the shared-index path, now justified)
    sup_neg: HashMap<ClassId, Vec<Atom>>,  // ¬sup expansion per defined sup
    fresh_q: ClassId,                 // the injection helper concept
    base_len: usize,                  // truncate point for per-pair Q-clauses
}
```

`from_internal` builds it once (the body of today's
`hyper_subsumption_probe` pre-pass, lifted out of the probe). Per pair,
`hyper_proves` clones the base, pushes the 2–3 Q-clauses for (sub,sup),
runs a fresh `HyperEngine`, returns `result == Unsat`. The single-query
path builds a one-shot cache (one query, no amortisation needed).

This finally justifies the shared `Rc<ClauseIndexes>` that was reverted
as a standalone no-op — here it's built once and reused across the N²
loop, where it matters.

## §3 — Flag, default OFF, for one release

Even a sound accelerator can carry an *integration* bug (clausifier vs
reasoner IRI→ClassId drift, shared-state aliasing). Gate it:
- CLI: `--hypertableau` on `classify` / `subclass` / `consistent`.
- Internally: a bool threaded into `PreparedOntology` / the
  single-query path (no new public reasoner-mode enum, no public API
  change — keep it inside the orchestrator).

Default **off**. Run the full reasoner test suite in *both* modes to
prove no divergence. Flip the default after a release of soak time.

## §4 — Validation gate (in order)

1. All reasoner tests pass with the flag **off** (gated ⇒ regression
   impossible).
2. All reasoner tests pass with the flag **on** (no integration bug on
   the cases the existing path already handles).
3. `classify(pizza | ro-stripped | sulo-stripped)` is **correct** and
   **faster** flag-on. If not faster, diagnose before shipping.
4. `classify(SIO, --pair-timeout-ms 30000)`: pairs that previously
   timed out now resolve (the wall moved *through the orchestrator* —
   measured via `classify`, **not** `hyper-classify-probe`, which is
   the probe's own loop).

## §5 — Encoding-drift regression test (the trap)

The tableau builds `sub ⊓ ¬sup` via a pool-mutating closure; hyper
uses Q-injection into a separate clause vec. **Different encodings of
the same query.** Add a test that runs both on a fixed ontology and
asserts they agree — specifically that hyper `Unsat` occurs exactly
where the tableau reports `sub ⊓ ¬sup` unsatisfiable on the canonical
cases. Catches encoding drift before it reaches users.

## §6 — Out of scope

- Complete-mode (trusting hyper `Sat`) — §1.
- A `classify`-level pre-pass instead of the per-query wedge — pick the
  per-query wedge (one insertion, all callers).
- Public API / new reasoner-mode enum changes — §3.
- ABox/consistency acceleration — the engine is TBox-only.

## §7 — Result (shipped) — and the honest reframe

The wedge is implemented, sound, flag-gated (`--hypertableau` /
`RUSTDL_HYPERTABLEAU`), default off. All reasoner tests pass flag-off
(regression-impossible) **and** flag-on (no integration bug). The
encoding-drift guard (`hyper_wedge_agrees_with_tableau`) confirms every
pair hyper proves agrees with the complete tableau. `HyperCache::proves`
is unit-tested in isolation.

**But the pizza/SIO *classify* wall is not an `Unsat`-only problem, and
the wedge does not move it.** Measured: `classify(pizza)` flag-on =
4 m 38 s, **1119 timed-out pairs, 0 hyper-proven**. Diagnosis (a clean
empirical result, not a bug — `HyperCache::proves` works in isolation):

- The classify orchestrator already proves *positive* subsumptions via
  EL saturation (353 of pizza's 695) and transitive closure. So the
  residual pairs reaching `subsumes_via_tableau` are dominated by
  **candidate non-subsumptions** the tableau refutes via `sat(A⊓¬sup)`.
- That is a **model-search problem on satisfiable instances** (find a
  model, search branches explosively) — the actual wall. Hyper's `Sat`
  cannot be trusted to refute (unsound under dropped/deferred axioms:
  `Models(ontology) ⊆ Models(fragment)`, so fragment-`Sat` ⇏
  ontology-`Sat`). The `Unsat`-only wedge has no work to do here.
- `ro-stripped` classify hangs flag-on too — the same negative-refutation
  wall (and HermiT itself hangs on `ro-stripped`).

**Complete-mode (trusting `Sat`) is not the fix.** A sound "fully
supported" gate (0 deferred + no ABox + no per-query defer) would
*correctly decline* exactly the wall workloads (pizza deferred=7) and
only enable on 0-deferred ones (GO, EL — already fast via saturation).
It helps where help isn't needed and declines where it is. Rejected.

**What the wedge *does* deliver (and where the engine's value is):**
- A sound, fast `yes` for **positive subsumptions EL saturation misses,
  transitive closure doesn't propagate, and the tableau would run** —
  a non-empty category on richer-TBox ontologies (pizza's positives
  happen to factor through EL+transitivity, so pizza isn't
  representative).
- The engine's measured wins are **probe-shaped**: single-query
  positive subsumption (corpus agreement, 0 FP) and **per-class
  satisfiability** (SIO 0.45 s vs the tableau's >135 s) — real, and
  separately deliverable as first-class APIs, *not* via the classify
  orchestrator's negative-refutation bottleneck.

So H4 ships the sound wedge (default off, documented scope), and the
"classify wall moved" claim is **not** made — the classify wall is
negative refutation, outside an `Unsat`-only accelerator's reach.

## §8 — Next directions (not this session)

1. Extend the clausifier to cover the last deferred shapes (qualified
   `≤n`, self-restriction) so a sound complete-mode becomes safe on the
   workloads we care about — the only path to moving the classify wall
   with this engine.
2. Or accept the engine's value is probe-shaped and ship single-query
   subsumption + per-class sat as first-class reasoner APIs.
