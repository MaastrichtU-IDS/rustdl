# `unsat_probe` starves `tier_walk`: two DNFs are budget-allocation, not calculus

**Date:** 2026-08-14 · Follow-on from `docs/2026-08-14-unsat-probe-cluster-rootcause.md`.

## Headline

**Two of the four `unsat_probe`-bucket DNFs complete with EXACT oracle agreement at
`--pair-timeout-ms 5`** — a value far below the 25/50/100/200 candidates the parked per-pair
budget spec screened.

| ontology | default (1000 ms) | `--pair-timeout-ms 5` | FP | MISSED |
|---|---|---|---|---|
| `ore_ont_934` | **DNF** | **50.1 s** | **0** | **0** |
| `ore_ont_10517` | **DNF** | **119.3 s** | **0** | **0** |
| `ore_ont_7828` | DNF | DNF | — | — |
| `ore_ont_8273` | DNF | DNF | — | — |

Both are exact against a **Konclude oracle, properly normalised** (see the adjudication note —
the naive comparison said FP=3577 and was wrong). At 20 ms both DNF again, so the useful range
is narrow.

## The mechanism

At the default 1000 ms per-pair budget, `unsat_probe` gives **each** of the N per-class
satisfiability probes the *global* deadline via `effective_deadline`. None of them concludes,
so each burns its full second: `ore_ont_934` ⇒ ~108 s, matching the census's 103,541 ms
exactly. `tier_walk` — the phase that actually computes the hierarchy — **never starts**.

Lowering the per-pair budget collapses `unsat_probe` and leaves the budget to `tier_walk`:

| `ore_ont_934` pure-ALCH core | `unsat_probe` | `tier_walk` | closure |
|---|---|---|---|
| default 1000 ms | eats everything | **never runs** | 0 / 365 |
| 20 ms | 2,176 ms | 58,924 ms | **365 / 365** |
| 5 ms | **549 ms** | 14,355 ms | **365 / 365** |

`unsat_probe` costs ~200× more at the default and reaches the *same* conclusion — that no class
is unsatisfiable. It is spending 108 seconds to answer a question whose answer is "no".

## The more striking fact: saturation already has the answer

On the 604-line pure-ALCH core (`docs/ore934-pure-alch-core.ofn`),
**`rustdl classify --saturation-only` returns 365 of 365 ground-truth subsumptions — 100%,
FP=0, MISSED=0 — in 0.01 s.**

The complete, correct hierarchy is obtainable instantly by machinery rustdl already ships. The
hybrid path then spends 180 s+ failing to do better, because the fragment gate sees ∀/⊔/¬,
routes to hybrid, and the hybrid's first phase does not terminate. The disjunctive machinery
contributes **nothing** to this ontology's atomic subsumption hierarchy.

This is not a claim that `--saturation-only` is complete in general — it is a sound
under-approximation, and here the under-approximation happens to be exact. But it means the
gap on this ontology is **not** a missing calculus.

## Peer context: a CB engine alone does it in 0.35 s

Both peers classify the pure-ALCH core trivially, and they **agree exactly**:

| reasoner | wall | closure |
|---|---|---|
| Konclude 0.7.0 | **0.09 s** | 365 |
| KM v0.2.11 `production_all` | 0.49 s | 365 |
| KM `cb_absorb1` (**CB-only, 1 thread**) | **0.35 s** | 365 |
| KM `cb_plain1` (CB-only, 1 thread) | 0.45 s | 365 |
| rustdl hybrid | **DNF @180 s** | 0 |
| rustdl `--saturation-only` | **0.01 s** | **365** |

Konclude's transitive closure excluding `Thing`/`Nothing` is **365**, identical to KM's 365, so
the ground truth is uncontested rather than one reasoner's opinion.

That **CB-only, single-threaded** routes solve it matters for the CB question: it is the
consequence-based calculus doing the work, not a portfolio race or parallelism. KM's own
default route DNFs on other ontologies, so this is not a blanket KM advantage.

KM on the full cluster splits **across** the mechanism boundary, which is worth noting because
it means KM's difficulty is orthogonal to the ∀-vs-no-∀ distinction:

| | KM | Konclude |
|---|---|---|
| `ore_ont_934` (∀-disj) | 1.1 s | 0.46 s |
| `ore_ont_8273` (∀-disj) | **DNF** | 0.29 s |
| `ore_ont_7828` (no ∀) | 0.1 s | 0.10 s |
| `ore_ont_10517` (no ∀) | **DNF** | 0.41 s |

## Adjudication note: a 3,577-pair "false positive" that was not one

The naive comparison for `ore_ont_10517` at 5 ms read **rustdl=5744, Konclude=2167, FP=3577** —
which, taken at face value, is a soundness violation and the most serious kind of finding in
this project.

It was an artifact of my comparison, not the reasoner. Konclude's output contained
`EquivalentClasses(Thing, MorphosyntacticCategory, Gender, …)` — 10 classes collapsed into a
TOP-equivalence group. Every member is ≡⊤ and so subsumes everything; rustdl states those
subsumptions explicitly while Konclude folds them into one equivalence axiom, and my parser
read only `<SubClassOf>` blocks and ignored `<EquivalentClasses>` entirely.

Expanding equivalences into pairwise subsumptions and excluding thing-equivalent and
unsatisfiable classes **on both sides** — the normalisation the 2026-08-05 KM retraction
established — gives **rustdl=2187, Konclude=2187, FP=0, MISSED=0**.

This is the third time this exact artifact has produced a spurious FP figure in this project.
The rule that catches it: **adjudicate via `X − (Konclude ∪ HermiT)` with equivalence expansion
and unsat/TOP normalisation applied symmetrically** — never a raw `SubClassOf` diff.

## What this does and does not license

**Does:** it revives the parked per-pair budget spec
(`docs/superpowers/specs/2026-08-13-per-pair-budget-default-design.md`) with concrete evidence,
and shows its **candidate set was too conservative** — it screened 25/50/100/200, while the
value that recovers these two is **5**, and at 20 ms both DNF again.

**Does not:** license lowering the default. Two of four cluster members are not rescued at any
tested budget, and a value as aggressive as 5 ms will cost completeness on ontologies that
currently answer correctly — which is exactly what that spec's pre-registered gate is for:

> Ship iff `ok → dnf` = 0 AND ΔMISSED < 5%.

The MISSED net exists to price that, and per the design record it does **not** replace a
full-corpus sweep, because its frame is drawn from completers and cannot observe an
`ok → dnf`.

**A better-targeted change was proposed here and has since been BUILT AND REFUTED.**
`RUSTDL_UNSAT_PROBE_MS` (default OFF) caps the unsat probe per class, independently of the pair
budget. The mechanism works exactly as designed — on `ore_ont_934` at the default pair budget it
takes `unsat_probe` from 73,807 ms to **556 ms (133×)** and `tier_walk` from 0 ms to 73,309 ms,
so the phase really is unblocked and decides 27 subsumptions it never previously reached.

**It rescues nothing.** With the cap at 5 ms and the pair budget at 50 or 100 ms, both
`ore_ont_934` and `ore_ont_10517` still DNF; only `--pair-timeout-ms 5` completes them. So the
recovery reported above comes from **both** phases being small, not from `unsat_probe` being
cheap, and the hoped-for benefit — a generous pair budget for completeness plus a cheap probe
phase — does not exist, because the pair budget that rescues these ontologies is far below any
value at which the cap would bind.

The "starvation" framing in this document was **mechanically correct and practically empty**.
Corrected here rather than deleted, because the mechanism measurement is real and the negative
result is the useful output. See `unsat_probe_cap`'s doc comment.

## Status

Nothing here is a shipped change. `ore_ont_7828` and `ore_ont_8273` remain unexplained at any
budget.
