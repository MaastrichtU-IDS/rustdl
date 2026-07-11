# galen: 10 missed subsumptions (functional merge across an inverse edge)

**Status:** open (sound; near-complete, not complete). Not scheduled — closing it
efficiently needs deeper work (see "Future work" below).

**Discovered:** 2026-07-11, via the authoritative curated matrix
(`docs/benchmarks/2026-07-11-curated/MATRIX.md`), which diffs rustdl against a
Konclude oracle (HermiT independently derives the same 10, corroborating).

## The finding

rustdl classifies galen as **Horn** and its completeness contract states
`completeness_guaranteed()` true ⟹ Horn/PureEl + no timeout ⟹ MISSED = 0. galen
met that condition yet rustdl **missed 10 genuine subsumptions** (e.g.
`Femur ⊑ Space`) that both Konclude and HermiT derive. rustdl asserts nothing
false (FP = 0 held throughout) — this is a **completeness contract violation**,
not a soundness issue, and it is now honestly disclosed here and in the
top-level docs (README.md, CLAUDE.md, `docs/reasoner-comparison-2026-06-21.md`)
rather than left as a stale "complete" claim.

## The minimal pattern

All 10 misses reduce to one shape: a functional (`≤1`) role merge across an
inverse-induced edge, in a cyclic model.

```
A    ⊑ ∃f.N
f    ≡ inverse(g)
Functional(g)
N    ≡ ∃g.(Y ⊓ ∃h.LFC)
Y    ⊑ Z
LFC  ≡ ∃g.A          # the cycle — without it rustdl derives A ⊑ Y correctly
```

Intended derivation (Konclude/HermiT, in ms): `A —f→ n:N`; since `f ≡ inverse(g)`,
`n —g→ A`; `N ⊑ ∃g.(Y⊓…)` gives `n —g→ m` with `m : Y`; `Functional(g)` on `n`
forces its two `g`-successors (`A` and `m`) to **merge** ⟹ `A : Y ⊑ Z`.

Root cause: the hypertableau wedge's `≤n`-successor counter
(`distinct_role_succ` in `crates/owl-dl-tableau/src/hyper.rs`) only scanned a
node's outgoing `edges`, never its incoming `preds` with the role polarity
flipped. So `n`'s inverse-induced `g`-successor `A` was invisible, `n` appeared
to have only 1 `g`-successor (`m`), the `≤1 g` constraint never looked violated,
and the merge never fired. See
`docs/superpowers/specs/2026-07-11-funcmerge-inverse-completeness-design.md` for
the full localization.

## The fix: sound, but opt-in

A sound fix exists — make `distinct_role_succ` union outgoing `edges` with
incoming `preds`/flip (mirroring the existing inverse handling in
`enumerate_matches`), deduped via `resolve()`. This does derive all 10 galen
subsumptions correctly (regression test:
`crates/owl-dl-reasoner/tests/funcmerge_inverse.rs`, `funcmerge_cyclic_derives_a_sub_y`).

It is gated behind an env flag, **default OFF**:

```
RUSTDL_INVERSE_FUNC_MERGE=1
```

(`crate::inverse_func_merge_enabled()`, `crates/owl-dl-tableau/src/lib.rs`.)

The flag is off by default because turning it on makes **galen classification
explode** rather than terminate promptly. The newly-visible inverse successors
trigger a much larger set of `≤1`/functional merges across the corpus; each
merge fold re-fires deterministic clause processing on its neighborhood, and on
galen's dense, highly-cyclic role graph this cascades into an
O(graph) × O(depth) clause-firing blowup in the Horn fixpoint
(`horn_fixpoint`) — the run does not finish within the benchmark budget (DNF).
So the flag is sound-but-impractical today: correct output if it terminates,
but it doesn't terminate on the one ontology it's meant to fix.

## Future work

Closing this gap *efficiently* needs bounding or memoizing the clause-firing
cascade across merge-branch recursion levels — e.g. capping re-fire depth,
memoizing per-node clause results across folds, or restricting the new
inverse-successor counting to the specific role shapes that actually need it
(functional + declared-inverse, rather than every `≤n` check). An incremental
reseed / "only re-fire the delta" spike was tried during scoping and did **not**
suffice to tame the blowup — the cascade is not simply a matter of avoiding
redundant re-processing of already-seen facts. This is tracked as future work,
not scheduled.

## Pointers

- Design: `docs/superpowers/specs/2026-07-11-funcmerge-inverse-completeness-design.md`
- Plan: `docs/superpowers/plans/2026-07-11-funcmerge-inverse-completeness.md`
- Related deferred item: `docs/known-limitations/hf3-general-predecessor-aware-merge.md`
- Authoritative numbers: `docs/benchmarks/2026-07-11-curated/MATRIX.md`
