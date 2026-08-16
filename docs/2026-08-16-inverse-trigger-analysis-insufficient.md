# Static inverse-trigger analysis: sound, and too weak to matter

**Date:** 2026-08-16 · **Status: measured NEGATIVE result. Nothing built.**

## The idea

`ore_ont_11311` classifies in 1.13 s via saturation with an answer byte-identical to KM's
(79,803 pairs), while the hybrid path cannot finish in 240 s. Its only out-of-EL feature is
three `InverseObjectProperties` declarations, and those are **provably inert** — Konclude
returns identical 10,667 `SubClassOf` with and without them.

So: could a static analysis identify the classes an inverse could affect, run the expensive
per-class phase on only those, and terminate when nothing new is learned?

## Why the naive form fails

"Classes mentioning an inverse-paired role" is **5,437 of 8,022 (67.8%)** on `ore_ont_11311`
— a 1.5× saving. `part_of` alone appears 7,012 times.

## Why the refined form also fails

The ELI inference needs a **trigger**: `X ⊑ ∃r.Y` plus `∃r⁻.X ⊑ Z`. Occurrence in a
right-hand-side existential cannot fire anything by itself, so the affected set should be
computed backwards from LHS positions — defined classes, `ObjectPropertyDomain`, and
`ObjectPropertyRange` (which is the domain of the inverse).

Implemented that (deliberately generous — over-approximating triggers is the sound
direction). Over the 424-ontology release population, **222 carry inverse pairs**:

| | n | share |
|---|---|---|
| inverse pairs **provably inert** (no trigger at all) | **13** | **6%** |
| at least one live pair | 209 | 94% |

**`ore_ont_11311` is NOT among the 13.** All three of its pairs are "live", because 1,876
defined classes of the form `C ≡ … ∃part_of …` are triggers. The refined analysis gives the
identical 5,437 classes — **no improvement whatsoever** over the naive one.

## Why the analysis is too weak, precisely

A trigger `∃part_of.D ⊑ C` firing against `X ⊑ ∃has_part.Y` places the **witness** in `C`,
yielding `X ⊑ ∃has_part.(Y ⊓ C)` — an *existential refinement*, not an atomic subsumption.
Nothing reaches the named class hierarchy.

Syntax sees a live trigger. Semantics sees nothing propagate. Separating the two requires
exactly the reasoning the analysis was meant to avoid, so this is not a matter of tightening
the syntactic rule — the information is not in the syntax.

## What the 13 would buy

Sound, but small, and mostly aimed at ontologies that are already fast:

| ontology | classes | rustdl today |
|---|---|---|
| `ore_ont_7416` | 17,295 | 10.2 s |
| `ore_ont_7203` | 6,724 | 9.3 s |
| `ore_ont_1966` | 20,514 | 5.2 s |
| the other 10 | ≤41,932 | **≤ 0.9 s** |

**3 of 13 are slower than 5 s; none DNFs.** So the addressable set is three ontologies whose
worst case is 10.2 s. That does not justify a new gate in a subsystem where a mistake means
certifying completeness while dropping an axiom — the D10 failure mode, hit six times in this
project.

## What survives

**The premise is right even though this realisation of it is not.** Over 53 sampled
ontologies, **46 (87%) have `--saturation-only` output identical to full classify** — the
entire post-saturation phase changes nothing. Of the 7 that differ the deltas are
+15/−6/−5/−3/+2/+1/+1 rows on hierarchies of 100–8,700.

So the expensive phase is usually confirming an answer already computed. The difficulty is
**knowing which case you are in without doing the work**, and the trigger analysis was an
attempt to answer that statically. It cannot.

Remaining options, in the order I would try them:

1. **Dynamic and sound**: run the per-class phase in an order that reaches
   possibly-affected classes first, and terminate the round when nothing new is derived —
   iterating to a fixpoint, since one new fact can affect another class. This needs no
   static affected-set and preserves completeness by construction. The cost is that a round
   which learns nothing still costs a full pass.
2. **Report the saturation answer on timeout, flagged `incomplete`** — already implemented,
   already sound, and worth an internal default deadline
   (`docs/2026-08-16-global-deadline-measurement-artifact.md`).
3. **Nothing.** 87% of the phase is provably redundant, but no cheap oracle distinguishes
   the 87% from the 13%.

The 12-axiom ELI probe (`A ⊑ ∃r.B`, `domain(s)=R` with `s=r⁻`, `B ⊓ R ⊑ ⊥` ⟹ `A ⊑ ⊥`)
should guard any future attempt: `--saturation-only` **misses** it while KM v0.2.32, Konclude
and rustdl's hybrid all get it. Any analysis that would call that ontology's inverse inert is
wrong, and the probe costs milliseconds to run.
