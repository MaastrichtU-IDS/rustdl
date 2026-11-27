# A deadline-free wedge decide regrows a nominal's witness chain at every depth level

**Date:** 2026-11-27 · **Status:** known, **not bounded** (by decision, #580) ·
**Severity:** slow, not wrong; only with no deadline at all

## Shape

`X ⊑ ∃t.X`, `X ⊑ ∃s.{a}`, `{a} ⊑ ≤n s⁻` (the #569 graph), run on a hand-built wedge engine
(`HyperEngine::new(..).with_incremental_fixpoint().with_double_blocking().with_nominals(..)`,
the `hyper.rs` unit tests). Every `X` node is one of `a`'s witnesses, so the #376 NN guard
refuses each as a blocker and the `t`-chain grows until `a` has `n + 1` witnesses. The `≤n`
partition search merges some of them. The merged node regrows the chain, which violates
`≤n` again one depth level down.

Through the product the same axioms (with `X(b)`) are decided at once: `classify` and
`consistent` with no deadline find a model on the first partition (Fable review of #580).
No product-level input is known to reach this.

## What #580 changed and what it left

Before #580, with no deadline, every level enumerated all its partitions after the innermost
leaves stalled on the depth bound. That is nested-exponential, and the graph did not return.

Since #580 (`RUSTDL_WEDGE_PARTITION_STALL_STOP`), the first stalled leaf ends each `≤n`
level, so directly nested `≤n` levels run one leaf each. A disjunction between two levels
still tries each of its disjuncts after a stalled child.

Each level still regrows the witness chain on a larger graph, so the cost per level grows
with depth, and the total is about quadratic in the depth bound. Measured on `≤100` (release
build, one pinned core, after #620's re-fire bound, 2026-11-27):

| depth | leaves | CPU |
|---:|---:|---:|
| 16 | 16 | 42.6 s |
| 32 | 32 | 160.3 s |
| 64 | 64 | 632.8 s |

At the wedge's `HYPER_WEDGE_DEPTH = 256` that extrapolates to about 2.8 CPU-h (not run). The
verdict is `Stalled`, never `Sat` or `Unsat`.

## Who can hit it

Only a decide with no deadline:
- `classify` with `--global-timeout-ms 0 --pair-timeout-ms 0`;
- a library `classify()` with no budget;
- the other `None`-deadline routes listed in the #580 audit.

With a deadline (every CLI default), #569's probe ends the search at the deadline. A deadline
that is `Some` but very far away (a huge `--global-timeout-ms` with `--pair-timeout-ms 0`)
leaves #580's stop off, and the full enumeration runs until #569's probe fires.

No partition leaf stalled before its deadline in the #580 census:
- 566 ORE files at CLI defaults, plus 9 curated ontologies;
- only 39 partition enumerations in total, 9 of them on ORE;
- 57 files were cut at 32 s and observed only until then.

## Why not bounded

The regrowth is the engine's own fixpoint work, not the enumeration, and a cap on it would
be a new budget on the deadline-free surface. The decision (rustdl-manager, #580) was to
record it, not cap it.
