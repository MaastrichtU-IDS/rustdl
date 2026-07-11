# HF3 — general predecessor-aware merge (deferred)

**Status:** deferred follow-up (not scheduled).
**Context:** `docs/superpowers/specs/2026-07-11-funcmerge-inverse-completeness-design.md`,
`docs/known-limitations/galen-inverse-functional-completeness.md`.

## What this is

`merge_with_cause` (`crates/owl-dl-tableau/src/hyper.rs`) folds one node into
another (its "survivor") when a `≤n`/functional constraint forces an equality.
Today this fold copies labels and outgoing `edges` onto the survivor, but does
**not** redirect the folded node's **incoming** edges (`preds`) to point at the
survivor — merges are, in the existing in-file terminology, **root-successor-only**.

## Why it is sound today

A stale predecessor edge (still pointing at the folded, non-survivor node)
remains a valid R-relationship — the merge only adds an equality, it never
invalidates an edge that was true before the fold. And every label read goes
through `resolve()` (the union-find lookup that maps a folded node to its
current survivor), so a reader that resolves before reading labels sees the
correct, up-to-date class set regardless of which node an edge nominally
points at. So the current behavior is a sound simplification, not a soundness
gap: it just doesn't broaden the graph as much as a fully general merge would.

## What remains (the deferred work)

A **general predecessor-aware merge** would redirect a folded node's `preds`
to the survivor at merge time, so subsequent traversals starting from an
in-edge see the survivor's full (post-merge) label and successor set directly,
rather than relying on every caller to `resolve()` first. This is more
invasive (touches the graph's edge-rewriting invariants, and interacts with
trail-based backtracking/undo) and no known corpus fixture currently requires
it — the `RUSTDL_INVERSE_FUNC_MERGE` fix (see the galen note) needed only the
successor-*counting* side (`distinct_role_succ` unioning `edges` + `preds`/flip),
not a change to how merges themselves redirect edges.

Revisit if a future ontology or completeness investigation surfaces a case
where root-successor-only merging under-derives a subsumption that a fully
general (predecessor-redirecting) merge would catch — i.e. where resolving at
read time is not enough because some traversal path does not resolve before
consuming an edge.

## Pointers

- `crates/owl-dl-tableau/src/hyper.rs` — `merge_with_cause`, `resolve`.
- Design spec: `docs/superpowers/specs/2026-07-11-funcmerge-inverse-completeness-design.md`
  (filed this as "HF3" under Non-goals/follow-up).
- Related: `docs/known-limitations/galen-inverse-functional-completeness.md`.
