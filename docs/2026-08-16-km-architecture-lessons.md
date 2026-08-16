# What KM's architecture says about rustdl's label-cache problem

**Date:** 2026-08-16 · Prompted by "given that KM can build a fast global model, we might
look to it." It paid off — but not in the way the framing suggested.

## First correction: KM's speed on `ore_ont_11311` is NOT its consequence-based engine

| KM route on `ore_ont_11311` | wall |
|---|---|
| `cb_plain1` (CB only) | **43.7 s** |
| `cb_absorb1` | 44.2 s |
| `cb_trigger1` | 44.2 s |
| `production_all1` (portfolio) | **4.32 s** |

The **hypertableau** arm wins, 10× faster than KM's own CB. And rustdl's saturation does the
same ontology in **1.13 s** with a byte-identical closure — faster than either KM arm. So
"KM builds a fast global model that rustdl lacks" is not the situation. Both projects have a
fast global saturation; the difference is what they do *after* it.

## KM has independently diagnosed rustdl's exact problem, on the same ontology

From `docs/THROUGHPUT-SATURATION.md`:

> "The blowup is the RESIDUE COMPLETION: both the per-concept verify funnel and the bare QO
> branching classifier **re-saturate per residue concept** → 19 GB / timeout on **7914's**
> 7171 residue. **Konclude instead builds the model ONCE and branches only the small open
> core in place.**"

`ore_ont_7914` is one of the three ontologies where rustdl's `label_cache_build` consumes
114 s and `tier_walk` gets 11 ms. Two independent projects, the same ontology, the same
diagnosis: **re-running a per-concept computation `n` times is the blowup, and building once
and branching in place is the fix.**

That is precisely what rustdl's measurement said from the other side: ≥14.7 ms per class for
the per-class label cache against 0.14 ms per class for the shared saturation fixpoint,
**≥105×**.

## The architecture KM implements (and where it stops)

`qo_classify_global_fwd` + `KM_HT_QO_PMMERGE` + `KM_HT_QO_VERIFY`, "validated gold-exact on
7581":

1. **Global forward saturation** — fast, sound, drops inverse edges.
2. **Pseudo-model MERGE prune** (`PMMERGE`) — a refutation pre-filter.
3. **Verify funnel** — structural suspects → per-suspect inverse saturation → tight
   candidates.
4. **Residual `C ⊓ ¬D` test** on the survivors.

**Where KM stops, stated in their own docs:** step 4 uses the complete tableau, which "BLOWS
UP (7581: 244 s; 9724: timeout)". **Konclude decides that residual in KPSet saturation
instead** — a port KM has designed (`docs/KPSET-PLAN.md`) and not implemented.

KM also measured the same thing rustdl did about how much saturation alone gets: on
`ore_ont_9724`, "forward saturation already gives **clean_subs=456239 / gold 457090
(99.8%)** soundly". rustdl's independent figure: the post-saturation phase changes **nothing
on 46 of 53** sampled ontologies, and where it does the gain is 2–234 pairs.

## What this means for rustdl specifically

rustdl already has **steps 1 and 4 working**, and its step 4 is *not* the problem — `tableau
= 0` on every fixture measured, so the residual KM struggles with is empty here.

**rustdl's gap is step 2.** Its Phase-7 label cache is a *per-class* pseudo-model: it runs
the wedge once per class to obtain `L(C)`, then refutes `C ⊑ D` when `D ∉ L(C)`. The prune
rate is excellent — 1,217,499 prunes against 683 pass-throughs on `ore_ont_11378`, 99.94% —
and it is load-bearing, not overhead: with `RUSTDL_LABEL_HEURISTIC=0` that ontology goes from
3.0 s to **DNF at 300 s**.

The cost is that it is `n` independent wedge runs. KM/Konclude get the same refutation from
**merged pseudo-models** rather than one per class.

So the target is narrow and well-specified:

> **Replace `n` per-class label-cache builds with a merge-based pseudo-model refuter that
> preserves the prune rate.**

Not a "global model rewrite". Steps 1 and 4 stay; only step 2's implementation changes.

## The risk, unchanged and still unmeasured

**A global refuter must retain ~99.9% pruning.** That is what makes `tableau = 0` possible.
A merged model pruning 90% would put ~120,000 pairs on the tableau for `ore_ont_11378` alone
and be catastrophically worse than today. KM's own experience is the warning: their funnel is
gold-exact but its residual test blows up, and the residual is exactly what a weaker prune
rate would enlarge.

**Measure the prune rate of a merged model before building the merge.** That is the go/no-go,
and it replaces the expired P0 in
`docs/superpowers/specs/2026-06-10-global-model-rewrite-design.md`.

## Sources

`/tmp/km-latest` @ `v0.2.32` (`44d86fa`), docs `KPSET-PLAN.md`, `THROUGHPUT-SATURATION.md`,
`KONCLUDE-SATURATION-CACHE-SPEC.md`, `LEVER-C-CACHE.md`. KM is AGPL/research code; nothing is
copied — only the architectural findings and their measurements are cited.
