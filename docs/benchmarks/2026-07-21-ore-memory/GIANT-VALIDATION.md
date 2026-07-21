# Soundness/completeness validation of the v0.3.30 recovered giants

v0.3.30's sparse `Classification.entailed` matrix made 8 ORE-2015 giant
ontologies (517k–981k classes) classify for the first time — they OOM'd before,
so their output had **never been checked against any oracle**. This validates that
output empirically.

## Why an EL oracle, and which one

All 8 giants are **pure, nominal-free EL TBoxes** — zero `ObjectOneOf`,
`ObjectHasValue`, `NamedIndividual`, `ClassAssertion` (so no ABox to drop, no
nominals: the Lever-A / Lever-1 ABox-handling soundness question does not arise).
rustdl classifies them purely via the EL saturator (`mode: pure EL`, tableau=0).

The oracle is **ELK** (via ROBOT, `-Xmx120g`), the SNOMED-scale consequence-based
EL reasoner — the right tool at this scale. Konclude/HermiT are full-DL engines
that OOM/timeout at ~1M classes; ELK reasons over 981k classes in ~96 s. This is
an *implementation* check: "the EL calculus is sound" is by construction, but
"rustdl's implementation is correct on these specific unverified inputs at this
scale" — including the brand-new sparse-matrix arm (engaged for every ontology
> 60k classes) — is empirical.

## Method

For each ontology: transitive-close rustdl's reported subsumptions and ELK's
inferred subclass axioms (both filtering `owl:Thing`/`owl:Nothing`/reflexive —
rustdl omits `C ⊑ ⊤`, ROBOT emits it for every class), then diff:

- `rustdl − ELK` = candidate **false positives** (the soundness-critical direction)
- `ELK − rustdl` = candidate **misses**

Tooling: [`harness/elk_diff_giant.py`](harness/elk_diff_giant.py) (+ the streaming
`owl:Thing`-filtering extractor). Dedup checked by md5 and by output diff.

## Result — every distinct giant is byte-for-byte identical to ELK

| ont | classes | rustdl closure | ELK closure | FP | MISS |
|---|---|---|---|---|---|
| ore_ont_14042 (≡ 11395) | ~517k | 11,609 | 11,609 | **0** | **0** |
| ore_ont_16008 | ~733k | 11,682,473 | 11,682,473 | **0** | **0** |
| ore_ont_14459 | ~848k | 13,114,303 | 13,114,303 | **0** | **0** |
| ore_ont_8486 | ~904k | 13,839,004 | 13,839,004 | **0** | **0** |
| ore_ont_868 (≡ 10689) | 981,151 | 14,809,043 | 14,809,043 | **0** | **0** |
| ore_ont_9674 | ~981k | 14,809,043 | 14,809,043 | **0** | **0** |

Dedup: `868 ≡ 10689` and `14042 ≡ 11395` (identical rustdl output); the other four
are distinct closures. All 8 giants are thus validated (6 by direct ELK diff, 2 by
identity to a validated twin) — no extrapolation.

## Conclusion

On the newly-classifiable EL giants, rustdl's output is **exactly** ELK's
subsumption closure at up to 981k classes / 14.8M subsumptions — **FP=0 and
MISSED=0**, sound *and* complete. This confirms the saturator and the new sparse
`EntailmentMatrix` are correct at scale (the small-n identity test in
`crates/owl-dl-reasoner/tests/sparse_classification_identity.rs` validates the
sparse code; these giants validate it at 517k–981k against an independent oracle).
The v0.3.30 memory fix therefore not only makes these ontologies classifiable but
does so correctly.
