# INCOMPLETENESS: the default top-down `classify()` misses equivalence classes off-EL

**Date:** 2026-08-20
**Severity:** MISSED ≠ 0 in the shipped public API. Sound (no false positives) but materially
incomplete on out-of-EL input. Strictly less severe than
[`dkey-id-aliasing-classify-fp.md`](dkey-id-aliasing-classify-fp.md), which is an FP.
**Status:** OPEN — reproduced, not fixed.
**Pre-existing:** yes. Not introduced by the incremental-reasoning work.
**Found by:** the Task 8 identity gate; corroborated independently by the Task 8 reviewer and
then by the controller from the CLI.

## The finding

`classify()` routes through `classify_top_down_internal`. On `bench-corpus/paper5.ofn` — a
**tracked** fixture — the top-down walk reports **13** hierarchy lines and **zero** equivalence
classes, where the budget-free `n²` sweep reports **23** lines and **three** equivalence classes.

Reproduced budget-free, so this is not a timeout artifact:

```sh
rustdl classify bench-corpus/paper5.ofn                                 # 13 lines, 0 equiv
rustdl classify bench-corpus/paper5.ofn --n2-classify --pair-timeout-ms 0  # 23 lines, 3 equiv
```

The three equivalences the default path misses:

```
equiv  ConstitutiveHemorrhagicDisorder ≡ DisorderContraindicatingHeparin
equiv  DisorderContraindicatingAspirin ≡ DisorderContraindicatingTicagrelor ≡ HemorrhagicDisorder
equiv  DisorderOKWithAspirin ≡ DisorderOKWithTicagrelor
```

## It is not a reporting-format difference

That was the first hypothesis and it is wrong. Both paths emit the same `unsat` / `equiv` /
`direct` line format. Diffing the two outputs shows top-down holds one direction of each
equivalence and not the other — e.g. it emits
`direct DisorderContraindicatingAspirin HemorrhagicDisorder` while never deriving the converse,
so it cannot collapse the pair into an `equiv`. The four lines unique to top-down are exactly the
degenerate one-directional forms of equivalences the `n²` sweep resolves properly.

So the top-down walk is **missing real entailments**, not formatting them differently.

## Why it matters here

Task 8 discovered this because the `IncrementalSession` was calling `classify_internal` (the `n²`
sweep) while the public `classify()` calls the top-down walk, and the identity gate compares the
two. The session was rerouted to `classify_top_down_internal` so that a session agrees with the
public API — which is the right call for the identity contract.

The consequence is worth stating plainly: **the session is now strictly less complete on
out-of-EL input than it was before that commit, by design**, because it inherits this
incompleteness from the path it now matches. Fixing the top-down walk would lift both at once.

## Scope

Not observed on the EL fast path (`el-partonomy.ofn`, `derived-overlay.ofn` both agree), which is
consistent with `docs/fragment-completeness.md`: completeness is claimed by construction for
EL+/Horn and only empirically for the broader fragment. This is a concrete, small, tracked
counterexample in that broader fragment — useful precisely because it is 16 classes rather than a
GALEN-scale mystery.

## Suggested next step

`paper5.ofn` is small enough to debug by hand. The question to answer is why the top-down walk
establishes `A ⊑ B` but never attempts or never derives `B ⊑ A` for these pairs — whether the
candidate-parent enumeration prunes the converse direction, or the equivalence-detection step
runs before the closing subsumption is available.
