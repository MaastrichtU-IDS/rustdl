# Two `#[ignore]`d sentinels tripped weeks ago and nobody could tell

**Date:** 2026-08-18 · Found while fixing `realize`'s dropped derived individual equality
(`docs/known-limitations/realize-drops-derived-individual-equality.md`).

## The finding

`crates/owl-dl-reasoner/tests/functional_enforcement.rs` carried two `#[ignore]`d sentinels
whose stated purpose was to **trip when an engine gap closed**, each with an explicit
instruction for that moment:

> "When the engine learns inverse-role predecessor merging, the two `#[ignore]`d sentinels
> below trip and the emit + a real canary should be added together."
>
> "tripping it means the engine learned inverse-role predecessor merging — at which point
> inverse-functional enforcement can be added (emit + canary) and this should flip to
> `assert!(!consistent...)`."

**The engine learned it on 2026-07-11** (`RUSTDL_INVERSE_FUNC_MERGE`, default ON). Both
sentinels had been out of date ever since. Nobody noticed, because **an `#[ignore]`d test that
starts passing produces exactly the same output as one that is still failing: none.**

The cost was not hypothetical. The comment block those sentinels anchored said the engine
"does not perform the `≤1 R⁻` predecessor merge, so emitting `∃R⁻.⊤ ⊑ ≤1 R⁻` would be a silent
no-op. We therefore do NOT emit it." That false claim is *why* the axiom went unwritten, and
its absence is the whole of the realize defect.

## The attribution, measured

`RUSTDL_ABOX_CHECK=0` throughout (the file's own isolation), varying
`RUSTDL_ABOX_SATURATION`:

| probe | `ABOX_SATURATION=1` | `=0` | conclusion |
|---|---|---|---|
| explicit `≤1 r⁻` (sentinel 2) | clash | **clash** | the WEDGE does the merge |
| `InverseFunctional(r)` (sentinel 1) | clash | consistent | answered by the pre-check only |
| sentinel 1 + `RUSTDL_INVERSE_FUNC_MAX=1` | clash | **clash** | the GCI is the sole trigger |

The third row is the discriminating experiment: with the ABox pre-check off the wedge is the
only route, and **the flag alone flips the verdict**. The merge was always there; the `≤1 r⁻`
constraint that triggers it was not.

**A second-order finding, worth more than the first:** sentinel 1 passes at the DEFAULT via
`abox_saturation`, *not* via the calculus this file's header claims to isolate. The
`RUSTDL_ABOX_CHECK=0` guard disables the A1 pre-check but **not** `abox_saturation`, which has
been default-ON since 2026-06-20 — so the file's isolation claim silently decayed too. Had the
sentinel simply been un-`#[ignore]`d on the strength of "it passes now", it would have been
recorded as calculus coverage it does not provide.

## Disposition

* Sentinel 1 un-`#[ignore]`d, with its true attribution stated in the doc comment.
* Sentinel 2 un-`#[ignore]`d and its assertion **flipped**, exactly as it instructed.
* A new canary, `inverse_functional_predecessor_merge_needs_the_derived_gci`, pins the
  discriminating experiment — control arm (flag off ⇒ sound MISS) *and* fix arm, so it fails if
  either the GCI stops reaching the wedge or some other route starts answering it and the
  experiment stops isolating anything.
* The stale module comment is replaced by the table above.

## The transferable lesson

This repo already knows that a guard test often fails to guard what it claims
(`sabotage-your-own-guard-tests`). This is the adjacent failure: **a test that is `#[ignore]`d
for failing is a claim about the engine, and nothing re-checks it.** Its passing is invisible,
so the prose built on top of it goes stale silently — and here that prose was the reason a
one-line fix went unwritten for five weeks.

Cheap mitigation, not yet adopted: run `cargo test -- --ignored` periodically and treat a
**pass** as a failure to triage. Every `#[ignore] = "known limitation"` in this tree is a
falsifiable assertion, and 80 of them are currently unchecked.
