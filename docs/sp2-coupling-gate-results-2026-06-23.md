# SP2 (coupling / seed the wedge from saturation) — verdict: NO-GO for the sound form — 2026-06-23

Spec: `docs/superpowers/specs/2026-06-23-coupled-saturation-tableau-design.md` §SP2.
Prior gate: SP0 (`docs/sp0-saturation-spike-results-2026-06-23.md`). SP1 increments
1–3 shipped on branch `feat/saturator-forall-propagation` (commits 0b1a702, 1f69b43,
d2fe403); all sound, FP=0, byte-identical corpus-wide.

## Verdict

**NO-GO for SP2's sound form (deterministic entailed-fact seeding).** Reached by a
cheap SP0-style gate — *without* building the seed prototype — because it fails the
pre-committed verdict rule on a load-bearing assumption.

## The pre-committed rule (advisor-gated)

The only *sound* coupling is seeding the per-pair wedge with the saturation's
**all-model (deterministic) consequences**. This is verdict-preserving by construction
(adding entailed clauses leaves the theory logically unchanged — fundamentally unlike
the snapshot cache, which replayed a single *non-deterministic* model's choices and was
FP-unsound). But deterministic seeding accelerates **UNSAT** (reaching a clash sooner),
NOT **SAT** (completing a clash-free model). So:

> SP2-sound-seed is GO only if the matched wine test's cost is **UNSAT-dominated**
> (something a seed can cut). If **SAT-dominated**, it is a reasoned NO-GO.

## Measurement — cheapest probe first (no seed code)

### 1. The matched test is SATISFIABLE (so seeding can't cut it)

`Probe ≡ AlsatianWine ⊓ ¬AmericanWine` injected into `wine.ofn`
(`scratchpad/wine-probe.ofn`):

| engine | `sat(Probe)` | wall |
|---|---|---|
| **rustdl** | DNF | timeout at 90s |
| **Konclude** (native) | **SAT** — `Probe ⊑ AlsatianWine`, not `⊑ Nothing` | 110ms classify |

Logically certain too: `AlsatianWine ⊑ FrenchWine`, `FrenchWine` disjoint `AmericanWine`
(different `locatedIn`), so `Probe ≡ AlsatianWine` — satisfiable. rustdl DNFs because
the verdict is **Sat**: a clash-free model-completion search over the combinatorial
value-assignment, which no deterministic seed shrinks.

### 2. Wine's wedge workload is refutation-dominated (corpus-level)

`rustdl classify --pair-timeout-ms 50 wine.ofn` banner:

```
# subsumption: saturation=622 tableau=0      ← wedge finds ZERO subsumptions
# satisfiability probes: saturation=58 tableau=79
# pairs-per-sub: total=8316
# timed-out pairs: 8251  (defaulted to not-subsumed)
# hyper-proven pairs: 21
```

Every positive subsumption (622) is found by **saturation**; the wedge finds **none**
(`tableau=0`). 8251 of 8316 pairs time out — these are wedge **refutation** searches
(proving non-subsumption = completing a Sat model) that can't finish in 50ms. The
wedge's entire expensive workload on wine is SAT-completion, not clash-finding.

This is the third independent confirmation of the same fact:
- **SP0**: wine's cost is the joint ∀+≤n+nominal *interaction* = disjunctive
  value-assignment branching, not deterministic re-derivation.
- **`[[inverse-aware-classification-no-win]]`**: "saturator answers 100% of positives;
  residual cost is refutation, unaccelerable by saturation."
- **SP1 byte-identical**: extending the saturator's closure changed **zero** wine
  verdicts — the wedge was already finding everything; the cost is refutation.

## Why the reference number doesn't rescue it

Konclude's 1ms on this SAT test comes from **reusing its precomputed model** (backend
cache + compatibility-gated *non-deterministic* completion-graph reuse), not from
per-test deterministic seeding. The motivating number is itself evidence that the sound
seed is the wrong mechanism — the fast path is exactly the non-deterministic reuse that
rustdl cannot make sound.

## Guardrail (do not slide into the reuse-trap)

The only thing that collapses this SAT search is Konclude's compatibility-gated
non-deterministic model reuse — the **reuse-trap** the snapshot cache already died on
(FP-unsound on the disjunctive fragment; flipped default-OFF as a soundness fix). That
is the multi-month nominal re-architecture repeatedly costed and deferred
(`[[wine-wall-bjgap1-genuine]]` CLOSED as not-worth-it: wall-only, MISSED=0, one
fixture, 640× gap). SP2's NO-GO does **not** reopen it.

## Disposition

- **SP2 sound-seed: dead** — saved the clause-augmentation build via a ~15-minute gate.
- **SP1 (increments 1–3): kept** as sound, reviewed, FP=0 foundation on
  `feat/saturator-forall-propagation` (corpus-invisible alone; not merged — bank or
  merge as foundation, but it carries no wine payoff on its own, exactly as SP0 warned).
- **SP3 (KPSet pair-pruning): orthogonal** — prunes the O(n²) pair *count* for
  full-classify parity, not per-test SAT cost; not gated by this result, but also not
  the wine lever (wine's wall is per-pair SAT timeouts, not pair count: 102 subs,
  8316 pairs).
- The coupled-saturation project's only remaining path to the wine wall is the
  non-deterministic-reuse re-architecture — a known, deferred, multi-month build with
  the soundness bar (compatibility gating) as its hard part. No cheap on-ramp exists.

Probe scratch: `scratchpad/wine-probe.ofn` (not committed).
