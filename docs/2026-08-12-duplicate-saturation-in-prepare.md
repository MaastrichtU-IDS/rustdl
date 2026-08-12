# `prepare` runs the EL saturation TWICE — ~50% of it is duplicated work

2026-08-12. Found by asking the question the phase census hid: *not* which phase
dominates, but what `prepare` costs in absolute terms.

## The census framing hid this

The three census runs classified ontologies by *dominant* phase, and on that basis
`prepare` shrank 28 → 13 → 3 members and I retracted it as a class. That retraction was
correct about dominance and **wrong about cost**, because a phase stops being "dominant"
the moment a later phase outgrows it — while still costing exactly as much.

Absolute `prepare_ms` across the 164-ontology tail (120 s cap):

| threshold | ontologies |
|---|---|
| ≥ 1 s | **93** |
| ≥ 5 s | 58 |
| ≥ 10 s | **32** |
| ≥ 20 s | 19 |
| ≥ 30 s | 9 |
| max | **89.0 s** (`ore_ont_8475`) |

**Total prepare time across the tail: 1,082 s.** And most of the worst offenders are *not*
dominant-`prepare` — `ore_ont_7507` spends 45 s there and is classified
`label_cache_build`.

**It is not class-count-driven.** `ore_ont_1833` spends **32.1 s with 195 classes**;
`ore_ont_10926` spends 35.8 s with **176,225**. Near-identical cost across three orders of
magnitude.

## Where prepare goes: ~50/50 saturate + `HyperCache::build`

Per-pass instrumentation inside `from_internal_with_deadline` (temporary, reverted):

| ontology | saturate | told | dkey | data_counting | `HyperCache::build` | census prepare |
|---|---|---|---|---|---|---|
| `ore_ont_8475` | 44,913 ms | 105 | 0 | 0 | **45,449 ms** | 89.0 s |
| `ore_ont_11196` | 16,563 | 14 | 0 | 0 | **17,001** | 34.0 s |
| `ore_ont_10926` | 15,632 | 467 | 0 | 0 | **16,534** | 35.8 s |
| `ore_ont_1833` | 723 | **3,028** | 3 | 32 | 10,657 | 32.1 s (rest unaccounted) |

`told`, `dkey_ranges` and `data_counting` are negligible except on `1833`, where
`build_told_tables` costs 3 s — consistent with its 195 classes but heavy axiom set, and
worth noting because `told` is one of the passes that does **not** check the deadline.

## The duplication

`classify_top_down_internal` computes a saturation closure for its fast-path check, then
calls `from_internal_with_deadline(internal.clone(), deadline)` — whose signature takes
**only** `(internal, deadline)`. With no closure parameter, it saturates the same ontology
again. Measured:

| ontology | classify's saturate | prepare's saturate | recoverable |
|---|---|---|---|
| `ore_ont_8475` | 46,836 ms | 46,318 ms | **~46 s** |
| `ore_ont_11196` | 16,657 | 16,414 | ~16 s |
| `ore_ont_10926` | 14,433 | 15,800 | ~14 s |

The two figures match to within 3%, which is what you expect from the identical
computation run twice — and it is **roughly half the total classify wall** on these
ontologies.

## Why reuse is sound, and the one gate it needs

Both closures are computed on the **same unmutated ontology**: classify does
`saturate(internal)`; `from_internal` receives `internal.clone()` and, with
`RUSTDL_LAZY_ABOX_SATURATION` off (the default), takes the `else` branch
`saturate(&internal)` *before* any mutation. `abox_irrelevant_to_classify` is computed
early but applied later, so it cannot affect the closure. Identical input ⇒ identical
result.

**The gate:** classify's own call may be `saturate_with_deadline(...)`, which returns
`(closure, aborted)`. An **aborted** closure is a sound under-approximation, so reusing it
would hand `from_internal` a *weaker* closure than it would have built. Reuse must
therefore be conditional on `!aborted`; when aborted, fall through to today's recompute.

## Proposed change

Add an optional precomputed closure to `from_internal_with_deadline` and thread
classify's non-aborted closure through:

```rust
pub(crate) fn from_internal_with_deadline(
    mut internal: InternalOntology,
    deadline: Option<Instant>,
    precomputed_closure: Option<Subsumers>,   // NEW: reuse iff !aborted
) -> Result<Option<Self>, ReasonError>
```

Expected: **~50% off classify wall** on the 32 ontologies spending ≥10 s in prepare, and
proportionally on the 93 spending ≥1 s. Verdict-preserving by construction (same closure
value), so the gate is a byte-identity check on classify output plus the FP=0 net, not a
completeness argument.

## BUILT — prepare roughly halves

Shipped **unflagged**: it is not a behaviour change, it is the removal of a repeated
computation, and gating it would only preserve the waste behind a flag.

`from_internal_with_deadline` gained a third parameter,
`precomputed_closure: Option<Subsumers>`; `classify_top_down_internal` passes
`Some(closure.clone())`. Only two call sites existed, so the change is small.

**The completeness gate turned out to be free.** I proposed making reuse conditional on
`!sat_aborted`; in fact `classify_top_down_internal` already **returns early** on
`sat_aborted` (into `classify_prep_timeout`), so its closure is complete by construction at
the call site. No conditional was needed — but the doc comment records the requirement, so
a future caller cannot quietly violate it.

| ontology | prepare BEFORE | prepare AFTER | |
|---|---|---|---|
| `ore_ont_8475` | 95,169 ms | **48,347 ms** | −49% |
| `ore_ont_11196` | 33,117 ms | **16,773 ms** | −49% |
| `ore_ont_10926` | 36,031 ms | **20,355 ms** | −44% |

The `Subsumers` clone costs ~760 ms on a 74,258-class ontology (48,347 vs a perfect
47,584), i.e. ~1.6% of what it saves.

**Wall follows only where the ontology can finish.** `ore_ont_10926` went **229.5 s →
107.3 s**; `8475` and `11196` both hit the 200 s cap either way and spend the freed time in
later phases instead — which is the correct behaviour (more work inside the same budget),
not a null result.

### Gates

* Workspace **1,605 pass / 0 fail**; fmt and clippy clean.
* **Byte-identity on all 10 curated fixtures**, base vs reuse (answers sorted and hashed).
  This is the right gate for this change: the claim is that the reused closure is the same
  *value*, so identical output is the direct test.
* FP=0 net: **13 closures VERIFIED, zero `FP>0`, zero `MISSED>0`**.

**Still open: the other ~50%.** `HyperCache::build` costs about as much as the saturation
did (45,449 ms on `8475`) and is **not** obviously duplicated — no other site builds it. It
is now the single largest identified item inside `prepare`.

## Method note

This is the third time in this arc that a *classification* obscured a *quantity*. The
census's dominant-phase label is useful for ranking clusters and actively misleading about
cost, because it is a max, not a sum. When a bucket shrinks, check whether the work went
away or merely stopped being the largest.
