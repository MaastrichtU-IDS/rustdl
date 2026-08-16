# What fragment would actually unlock the DNF tail — measured

**Date:** 2026-08-16 · Answers "is the ALCH half of the CB arc a bigger market than the ELHI
half?" **Yes — 2.6×. But the biggest single lever is neither disjunction nor inverse: it is
CARDINALITY.** And no small fragment gets far.

## Method, and the two errors it was built to avoid

`cb_eli_blocker` reports only the FIRST out-of-fragment feature, which cannot answer a
coverage question: an ontology whose first blocker is `All` may also carry `Max`, so counting
first-blockers over-credits every candidate fragment. `cb_fragment_features` returns the FULL
set, and it **recurses into fillers** — a `∀r.(A ⊔ B)` registers as both `All` and `Or`,
because counting only the outermost constructor makes the same mistake one level down.

Confirmed live: `ore_ont_10140` reports first-blocker `Min` and has feature set `Max,Min,Or`.

Coverage is by subset test with the `ABox` always permitted (TBox-only classification, per
`cb_eli_eligible_tbox_only`).

## The ladder

Base: **129 of 141** v0.4.19 DNF ontologies with a resolved feature set (12 too slow to reach
the probe even at a 20 s budget).

| fragment | covers | marginal |
|---|---|---|
| **Horn-ELHI** (as specced) | **17** | 17 |
| + functional / symmetric / role chars | 22 | +5 |
| + `Or`, `Not` | **45** | **+23** |
| + `All` | 46 | +1 |
| + `Min`, `Max` | **90** | **+44** |
| + `Nominal` | 101 | +11 |
| + `Self`, `DKey` | 125 | +24 |

Only **4** ontologies are not covered even then, and their sole residual feature is
`DisjointUnion` — a macro for pairwise-disjoint plus a covering disjunction, so it is not an
independent capability.

Feature prevalence across the tail: `Or` 85, `Max` 77, `ClassAssertion` 61, `FunctionalRole`
55, `SymmetricRole` 51, `Min` 42, `All` 31, `Not` 24, `DKey` 24, `Nominal` 24.

## What this decides

**The ELHI arc's DEFER stands, and is now better justified.** 17 of 129 (13%). Confirmed
independently of the earlier count, which reached 16 by a different route.

**The ALCH half IS bigger — 45 vs 17, 2.6×** — but that is well short of the 3–5× I predicted
before measuring, and it does not stand alone: 45 requires `Or`+`Not` **on top of** ELHI and
role characteristics, i.e. the parked ALCH engine *plus* inverse *plus* the role work.

**The dominant lever is cardinality (+44), which the CB literature treats as the hard part.**
Sequoia's second-maximal-atom trick exists precisely to keep at-most reasoning from going
2ⁿ, and rustdl already measured its own second-maximal attempt as **~3× WORSE** because
eligibility relaxation grows the cross-product (`RUSTDL_CB_SECOND_MAXIMAL`, default off).

**So there is no cheap fragment.** Market grows roughly with how much of SROIQ you implement:
13% → 35% → 70% → 78% → 97%. Reaching the bulk of the tail means ALCHIQ + role hierarchy +
inverse — essentially Sequoia, a multi-increment build.

## Coverage is not tractability — the caveat that matters most

**A fragment covering an ontology means the calculus can express it, not that it will finish.**
The parked `owl-dl-cb` already implements ALCH and *hangs >30 s on `adversarial(13)`*, where
KM does it in 88 ms and Konclude in 32 ms. Every number above is an upper bound on what an
engine for that fragment could rescue, assuming the blowup is tamed — and taming it is the
unbuilt backward-propagation / lazy-successor work in
`docs/superpowers/specs/2026-07-28-cb-lazy-successor-design-seed.md`.

Two prior measurements bound the optimism further: of rustdl's DNF tail, **Konclude classifies
94% at a median 3.57 s**, so these ontologies are not intrinsically hard — but rustdl's own
ALCH CB engine is an existence proof that *having the fragment* is not the same as *being
fast on it*.

## Recommendation

**Do not start a CB engine on a fragment basis.** The honest options are:

1. **Close the CB arc.** Horn-ELHI is 13% of the tail; ALCH-plus-inverse is 35% and needs the
   untamed blowup fixed first. Neither justifies its build cost on these numbers alone.
2. **If it is ever revived, revive it for `Min`/`Max`** — the single +44 step — and gate it on
   the `adversarial(N)` taming, not on a fragment census. That is the measurement that
   decides whether a CB engine can be fast, and it is cheap: the generator and baseline
   already exist on `feat/cb-alch-taming`.

**Do not read the +44 as a plan.** It is the size of a prize behind a mechanism nobody in this
repo has yet made tractable, and one whose most obvious lever (second-maximal) already
measured out at 3× worse.

## Instrumentation

`cb_fragment_features` (public, diagnostic-only) + the `feats=` field on
`RUSTDL_CB_ELI_PROBE`. Dead code: the probe is the only caller. Raw data
`docs/benchmarks/data-2026-08-16-cb-fragment-features-dnf141.txt`.
