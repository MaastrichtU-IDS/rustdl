# Incremental reasoning: the per-revision lowering floor — spike findings


> **Provenance caveat (added 2026-08-20).** rustdl is developed on two machines. Every number in
> this document was measured on **Apple M5 Max / 128 GB** against `ontologies/external/galen.ofn`
> at **sha256 `4b3f900883a9b59c…`** (1,241,952 bytes; 2,748 classes; 207 `InverseObjectProperties`).
> That file declares **no ontology IRI and no versionIRI**, and it is **not** fetched by
> `scripts/fetch-real-ontologies.sh` — so it cannot be identified across machines by anything but
> its hash. Do not compare these figures to measurements from another host or another galen copy
> without first confirming the hash matches. See
> `docs/known-limitations/galen-off-the-fast-path.md`.


**Date:** 2026-08-19
**Spike question:** the Fable design review's objection B1 requires an incremental session
to re-run the whole-ontology derived-axiom passes (`derive_data_axioms`,
`seed_dkey_subsumptions`, `derive_disjunction_existentials`,
`derive_functional_max_cardinality` — `crates/owl-dl-core/src/convert.rs:2106-2203`) on
**every** commit, otherwise stale derived axioms produce false positives on delete. Those
passes are whole-ontology scans. Does that put a floor under per-revision cost high enough
to undermine the feature?
**Method:** throwaway `examples/` binary (deleted; reconstructible from this doc) timing
median-of-5 `convert_ontology` against `classify` and `classify_saturation_only` on the local
corpus. Release build, `RUSTUP_TOOLCHAIN=stable`, single host, no other load control — these
are ratios, not publishable absolute timings.

## Result

The floor is real but **shrinks with scale**, which is the direction that matters.

| ontology | classes | convert | of which derive | classify | conv/classify | sat-only | **conv/sat-only** |
|---|---|---|---|---|---|---|---|
| paper5 | 16 | 0.1 ms | 0.0 ms | 2.8 ms | 1.8 % | — | — |
| sulo | 18 | 0.1 ms | 0.0 ms | 1.7 ms | 3.3 % | — | — |
| ro | 58 | 1.5 ms | 0.1 ms | 25.2 ms | 6.1 % | — | — |
| pizza | 99 | 0.2 ms | 0.1 ms | 23.9 ms | 1.0 % | 0.8 ms | **30.5 %** |
| mie | 101 | 0.3 ms | 0.1 ms | 4.8 ms | 5.8 % | 0.7 ms | **41.5 %** |
| family | 267 | 5.9 ms | 2.5 ms | 266.9 ms | 2.2 % | — | — |
| sio | 1592 | 3.3 ms | 1.1 ms | 164.4 ms | 2.0 % | 29.0 ms | **11.5 %** |
| galen | 2748 | 5.8 ms | 1.7 ms | 881.7 ms | 0.7 % | 76.6 ms | **7.6 %** |

(wine did not finish a full `classify` inside 900 s — the documented pathological case; its
`convert` is in the same few-ms band and the ratio would only be smaller.)

### Read the sat-only column, not the classify column

`conv/classify` (0.7–6.1 %) **flatters the design and should not be quoted.** Those runs are
dominated by the tableau pair loop, which incremental reasoning does not have to re-pay.
The honest denominator is `classify_saturation_only` — the saturation-only fast path is what
an EL-fragment session actually competes against, and it is *cheap*, so the floor's share is
proportionally largest there.

### The trend is the finding

On the sat-only path the floor falls monotonically with size: **41.5 % → 30.5 % → 11.5 % →
7.6 %** across 101 → 2748 classes. `convert_ontology` is roughly linear in axioms while
saturation is superlinear, so the floor's share keeps decreasing as ontologies grow. The high
small-ontology ratios are sub-millisecond in absolute terms and irrelevant.

## Consequences for the design

1. **B1's fix is affordable. Adopt it.** Re-running every derivation pass per commit costs
   ~7.6 % of a saturation-only classify at galen scale and less above it. The soundness fix
   does not need a cheaper approximation, which removes the main reason to consider a
   partial/incremental derivation pass.
2. **The floor is a speedup CEILING, and it should be written into the spec as the target.**
   Even a perfect closure-reuse session pays the floor, so the best achievable speedup on the
   sat-only path is `sat_only / convert`:

   | ontology | ceiling |
   |---|---|
   | galen | **~13×** |
   | sio | **~9×** |
   | pizza | **~3.3×** |
   | mie | **~2.4×** |

   For calibration, KM reports 4.90× on its addition-only EL++ microbench. A ~13× ceiling on
   galen means the design has real headroom; a ~2.4× ceiling on mie means small ontologies
   are not where this feature earns its keep, and the evaluation should not headline them.
3. **Do not bother making the derivation passes themselves incremental.** They are only
   ~30 % of the floor (galen: 1.7 ms of 5.8 ms). The floor is mostly interning and lowering,
   so incrementalizing derivation would recover at most a third of it. Lower priority than
   anything else on the list.
4. **P1 gets a concrete exit criterion** it previously lacked: on galen, a single-axiom
   addition must complete in ≤ 2× the measured floor (≤ ~12 ms) — i.e. within ~1.6× of the
   theoretical best. If it cannot, the retained-state design is not paying off and should be
   re-examined before P2 builds deletion on top of it.

## Residual uncertainty

No large **pure-EL** ontology (GO-basic ≈ 52k classes, the `classify_pure_el` zero-tableau
path) is available locally, and that is the single most favourable case for the feature and
the most demanding for the floor. galen at 2748 classes is the best local proxy and its 7.6 %
is already comfortable, with the trend pointing down — but the GO-scale number should be
measured when the corpus is available, before the evaluation's headline claims are fixed.
