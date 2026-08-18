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

Cheap mitigation: run `cargo test -- --ignored` periodically and treat a **pass** as a failure
to triage. Every `#[ignore] = "known limitation"` is a falsifiable assertion about the engine,
and nothing re-checks it.

**Census, counted rather than estimated** (a first draft of this paragraph said "80 of them are
currently unchecked", taken from the suite's runtime `ignored=` line — wrong, and wrong in the
alarming direction):

| | count |
|---|---|
| `#[ignore]` **attribute sites** in `crates/` | **67** |
| …ignored for FIXTURE availability or COST (gitignored corpus, `--nocapture` probes, release-only timings) | 50 |
| …whose reason already says **"PASSES via …"** — deliberately ignored *and* known to pass | 6 |
| …remaining: genuine falsifiable "this fails" claims | **~11** |

A grep for lines *containing* `#[ignore` returns 97, inflated by prose in doc comments —
including text added by this very document. The attribute-site count requires anchoring the
pattern to the start of the line.

So the sweep is cheap **because the falsifiable population is ~11, not ~80**, and two of those
eleven were the sentinels above. Two caveats when reading a pass:

1. **A pass is a signal to triage, not a licence to un-`#[ignore]`.** Attribute it to an engine
   first — sentinel 1 passed via a pre-check, not the calculus its file claimed to isolate.
2. **The 50 fixture/cost ignores carry no claim**, so their passing means nothing. Separate them
   out before counting, or the signal drowns.

---

## The sweep, run rather than recommended (2026-08-18)

`cargo test --release --workspace --exclude owl-dl-py --no-fail-fast -- --ignored`.

**Coverage: 78 ran — 64 passed, 14 failed.**

### The first attempt silently covered 5 of 78

Without `--no-fail-fast`, cargo stops remaining test binaries after one fails. One ignored test
failed early, so the sweep reported **4 passes out of 5 tests run** and looked like a complete
result. Reading "4 stale claims" off it would have been a fabricated finding. It was caught only
by checking the count against the expected 78 — the *"prove the instrument fires, by a numeric
criterion declared in advance"* rule, applied to a sweep of my own design.

**Anyone repeating this must pass `--no-fail-fast`.**

### Most passes carry no claim, and one is VACUOUS

Of the 64 passes, the large majority are the fixture/cost category — corpus closure gates
(galen, sio, wine, pizza, ro, sulo, notgalen, alehif, bibtex, ore_*), `anytime_*` sweeps,
`*_probe`/`*_report` diagnostics, and the six already documented as "PASSES via …". Their
passing is the expected state.

Worse than uninformative: **`family_stripped_inconsistency_detected` passes VACUOUSLY.** Its
fixture is absent and the test's skip-if-absent guard returns early, so it is green while
verifying nothing. Its sibling `family_inconsistency_detected` genuinely passes. The two are
distinguishable **only** by reading the `[fp0] … VERIFIED / NOT VERIFIED` marker, not the test
result:

```
[fp0] family-stripped (inconsistency): NOT VERIFIED (fixture absent: …)   → ok   ← vacuous
[fp0] family (inconsistency): VERIFIED (is_consistent=false, oracle: false) → ok   ← real
```

That is the whole justification for the `[fp0]` marker convention, demonstrated.

### The one real stale claim found

`family_inconsistency_detected` was `#[ignore]`d with *"stretch: may not close without
functional-merge work"*. **It closes** — `is_consistent=false` in ~4 s, oracle-matching. It shut
via `abox_saturation` (2026-06-20) and `RUSTDL_CLASSIFY_INCONSISTENCY` (v0.4.8), **not** the
functional-merge work its reason predicted. Two `CLAUDE.md` claims went stale with it:

* the SP1 note that *"full `family.ofn` remains a sound MISS … so `family*_inconsistency_detected`
  stay `#[ignore]`d"* — it stays ignored, but for FIXTURE availability, an entirely different
  reason;
* the Phase A1 note that family *"still timeout"*.

Both corrected. The test keeps its `#[ignore]` (the fixture is gitignored) with a reason that now
says so.

### The 14 failures are the healthy case

Claims still true. Mostly report-style probes that assert nothing runnable
(`bjgap_histogram`, `find_hard_pairs`, `ore_10019_backjump_precision_report`) plus genuine
documented residuals (`ddmin_core_residual_divergence`,
`nested_existential_poisoned_role_via_chain`). One informative pair:
`family_core_detected_by_saturation` **fails** while `full_family_detected_by_saturation`
**passes** — consistent with the ddmin core being inconsistent via the *wedge* route, which
`abox_saturation` does not reach.

### Net

One genuine stale claim (plus the two `CLAUDE.md` sentences resting on it), one vacuous-pass
hazard documented, and the two sentinels that started this. **The sweep is worth repeating after
any engine-behaviour change**, and it costs one command — but only with `--no-fail-fast`, and
only if each pass is attributed before being believed.
