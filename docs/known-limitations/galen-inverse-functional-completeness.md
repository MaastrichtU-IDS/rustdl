# galen: 10 → 1 → 0 missed subsumptions (functional merge across an inverse edge)

**Status:** CLOSED (2026-07-12). galen now classifies with **MISSED = 0**
against the Konclude∩HermiT oracle (closure 28007=28007, FP=0). The
functional/`≤1`-role-merge-across-an-inverse-edge mechanism below is
**default ON** and recovered 9 of the original 10 misses; the 10th (a
different mechanism, defined-class ∃-monotonicity) is now closed too, by the
label-cache back-fold (`RUSTDL_CLASSIFY_BACKFOLD`, also **default ON**) — see
[`docs/known-limitations/galen-defined-class-monotonicity-residual.md`](galen-defined-class-monotonicity-residual.md)
for that mechanism's resolution. Both fixes are independent, sound, and
default-on; together they take galen from MISSED 10 → 1 → 0.

**Discovered:** 2026-07-11, via the authoritative curated matrix
(`docs/benchmarks/2026-07-11-curated/MATRIX.md`), which diffs rustdl against a
Konclude oracle (HermiT independently derives the same 10, corroborating).

**Closed (9 of 10):** 2026-07-11 (same day), by making the merge **incremental**
— folded directly into `horn_fixpoint`'s existing fact-processing loop (with
resolve-on-read so a head derived onto a folded node lands on the survivor)
instead of the old whole-graph re-fire. galen now classifies in well under a
second with **MISSED = 1** (down from 10), FP = 0 held throughout.

## The finding (original)

rustdl classifies galen as **Horn** and its completeness contract states
`completeness_guaranteed()` true ⟹ Horn/PureEl + no timeout ⟹ MISSED = 0. galen
met that condition yet rustdl **missed 10 genuine subsumptions** (e.g.
`Femur ⊑ Space`) that both Konclude and HermiT derive. rustdl asserts nothing
false (FP = 0 held throughout) — this was a **completeness contract violation**,
not a soundness issue.

## The minimal pattern

All 10 original misses reduce to one shape: a functional (`≤1`) role merge
across an inverse-induced edge, in a cyclic model.

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

## The fix, take 1: sound, but impractically slow (superseded)

The first fix made `distinct_role_succ` union outgoing `edges` with incoming
`preds`/flip (mirroring the existing inverse handling in `enumerate_matches`),
deduped via `resolve()`. This derived all 10 galen subsumptions correctly, but
fired the merge via the old **whole-graph re-fire** path: each merge fold
re-ran deterministic clause processing over its neighborhood, and on galen's
dense, highly-cyclic role graph this cascaded into an `O(graph) × O(depth)`
clause-firing blowup — galen did not finish within the benchmark budget
(**6.6-minute DNF**). So the flag was shipped **default OFF**
(`RUSTDL_INVERSE_FUNC_MERGE=1` to opt in), sound-but-impractical: correct
output if it terminated, but it didn't terminate on the one ontology it was
meant to fix.

## The fix, take 2: incremental merge, default ON

The blowup was in *how* the merge re-fired clauses, not in the merge itself.
Firing the `≤1`/functional merge **incrementally** — as part of
`horn_fixpoint`'s existing fact-processing loop, touching only the folded
node's delta (with resolve-on-read so a head derived onto a folded node lands
on the survivor, not a stranded ghost) — avoids the whole-graph re-fire
entirely. This is sound (same merge, same semantics) and **fast**: galen
MISSED 10 → 1 in well under a second (down from the 6.6-minute DNF); wine
19.78 s → 90 ms. Corpus-wide closure-diff (`konclude_closure_diff.rs`, all
available fixtures): FP = 0 held throughout.

`RUSTDL_INVERSE_FUNC_MERGE` is now **default ON** (`crate::inverse_func_merge_enabled()`,
`crates/owl-dl-tableau/src/lib.rs`); set it to `0` to revert to the old
(incomplete, MISSED = 10) behaviour.

## The residual: 1 pair, a different mechanism — now also closed

One galen pair remained missed after the incremental merge:
`TibialTuberosity ⊑ TibialInterCondylarEminence`. This was **not** an instance
of the functional-merge-across-inverse pattern above — it is a defined-class
∃-monotonicity subsumption pruned by the label cache (needing the ¬-expansion
disjunction + ∀-propagation path if verified naively). It is now closed by the
label-cache back-fold rule (`RUSTDL_CLASSIFY_BACKFOLD`, default ON since
2026-07-12) — a sound, branch-free direct derivation over the sat graph that
bypasses the disjunctive path entirely. See
[`docs/known-limitations/galen-defined-class-monotonicity-residual.md`](galen-defined-class-monotonicity-residual.md)
for the precise justification pattern and the fix. **galen is now MISSED = 0.**

## Pointers

- Root-cause design: `docs/superpowers/specs/2026-07-11-funcmerge-inverse-completeness-design.md`
- Root-cause plan: `docs/superpowers/plans/2026-07-11-funcmerge-inverse-completeness.md`
- Incremental-merge design: `docs/superpowers/specs/2026-07-11-wedge-incremental-functional-merge-design.md`
- Incremental-merge plan: `docs/superpowers/plans/2026-07-11-wedge-incremental-merge.md`
- Related deferred item: `docs/known-limitations/hf3-general-predecessor-aware-merge.md`
- Residual (1 pair): `docs/known-limitations/galen-defined-class-monotonicity-residual.md`
- Authoritative numbers (pre-fix baseline): `docs/benchmarks/2026-07-11-curated/MATRIX.md`
  (superseded by the regenerated matrix at the same path, post-fix — see its
  `run-metadata.json` timestamp/git_sha to distinguish).
