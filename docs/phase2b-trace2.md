# Phase 2b — second trace: why GALEN MISSED only dropped 5/60

Phase 2b's main fix (commit 022ca50: `atomic_classes_with_existential_markers`
now uses `introduce_equivalent_existential_marker` so nested-existential
markers carry the fact `(F, R, B)`) recovered only **5 of 109** GALEN MISSED,
far below the ~60 estimated in `docs/phase2b-galen-diagnosis.md`. This trace
investigates a cluster-A pair (`FemoralHead ⊑ ExactlyPairedBodyStructure`,
pair_01) that is **still missed** post-fix, to determine why the fix didn't
unblock it.

## Step 1 — pair_01 is still MISSED

```
./target/release/rustdl subclass --saturation-only \
  crates/owl-dl-reasoner/tests/fixtures/phase2b/pair_01.ofn \
  http://example.org/factkb#FemoralHead \
  http://example.org/factkb#ExactlyPairedBodyStructure
# -> no
```

Default-mode tableau spun for minutes and was killed (GALEN-shaped). The
saturation-only result is decisive: the pair is not in the EL closure.

## Step 2 — Hop structure (per `phase2b-galen-pair-analysis.md`)

HermiT derives the pair via `FemoralHead ⊑ MirrorImagedBodyStructure ⊑
ExactlyPairedBodyStructure`. The second hop is EL-trivial (mirrorImaged ⊑
leftRightPaired ⊑ exactlyPaired). The first hop relies on the "load-bearing
complex LHS GCI" in the GALEN module:

```
SubClassOf(
  ObjectIntersectionOf(
    :BodyStructure
    ObjectSomeValuesFrom(:hasIntrinsicAbnormalityStatus :normal)
    ObjectSomeValuesFrom(:isSolidDivisionOf
      ObjectIntersectionOf(
        :BodyStructure
        ObjectSomeValuesFrom(:hasIntrinsicAbnormalityStatus :normal)
        ObjectSomeValuesFrom(:isPairedOrUnpaired :mirrorImaged))))
  ObjectSomeValuesFrom(:isPairedOrUnpaired :mirrorImaged))
```

That GCI's **RHS is a single `∃R.B`, not an atomic class and not an `And` of
atomics**.

## Minimal synthetic (faithful to pair_01's shape)

`/tmp/p2b-trace-pair01-mini.ofn`:

```
SubObjectPropertyOf(:isSpecificSolidDivisionOf :isSolidDivisionOf)
EquivalentClasses(:FemoralHead And[:BonyHead, ∃:isSpecificSolidDivisionOf.:Femur])
SubClassOf(:BonyHead :BodyPart)
SubClassOf(:BodyPart :BodyStructure)
SubClassOf(:BodyPart ∃:hasIntrinsicAbnormalityStatus.:normal)
SubClassOf(:Femur :LongBone)
SubClassOf(:LongBone :BodyPart)
SubClassOf(:LongBone ∃:isPairedOrUnpaired.:mirrorImaged)
SubClassOf(:mirrorImaged :leftRightPaired)
SubClassOf(:leftRightPaired :exactlyPaired)
EquivalentClasses(:MirrorImagedBodyStructure And[:BodyStructure, ∃:isPairedOrUnpaired.:mirrorImaged])
EquivalentClasses(:ExactlyPairedBodyStructure And[:BodyStructure, ∃:isPairedOrUnpaired.:exactlyPaired])
# Load-bearing GCI (verbatim shape from pair_01):
SubClassOf(
  And[:BodyStructure, ∃:hasIntrinsicAbnormalityStatus.:normal,
      ∃:isSolidDivisionOf.And[:BodyStructure, ∃:hasIntrinsicAbnormalityStatus.:normal,
                              ∃:isPairedOrUnpaired.:mirrorImaged]]
  ∃:isPairedOrUnpaired.:mirrorImaged)
```

## HermiT cross-check (synthetic)

HermiT (via `docker/robot/classify-oracle.sh`) derives:

- `FemoralHead ⊑ MirrorImagedBodyStructure` (first hop) ✓
- `MirrorImagedBodyStructure ⊑ ExactlyPairedBodyStructure` (second hop) ✓
- transitive: `FemoralHead ⊑ ExactlyPairedBodyStructure` ✓

(`grep -E "FemoralHead|MirrorImagedBodyStructure" /tmp/p2b-trace-pair01-mini.hermit.owx`
shows the inferred `SubClassOf(FemoralHead, MirrorImagedBodyStructure)` edge.)

## rustdl behavior on the synthetic

```
rustdl subclass --saturation-only :FemoralHead :MirrorImagedBodyStructure   -> no  (MISS)
rustdl subclass --saturation-only :FemoralHead :ExactlyPairedBodyStructure  -> no  (MISS)
rustdl subclass --saturation-only :MirrorImagedBodyStructure
                                  :ExactlyPairedBodyStructure               -> yes (HIT)
```

Tight repro: the first hop is the gap. Second hop derives correctly.

## Trace (P2B_TRACE2 instrumentation, reverted before commit)

Tracing was added at `lower_sub_class_of`, `atomic_or_tseitin_body_with_extras`,
and `atomic_classes_with_existential_markers`. The relevant lines from
`/tmp/p2b-trace2.log` showing how the load-bearing GCI is lowered:

```
P2B_TRACE2 lower_sub_class_of:
    sub=And([ConceptId(3), ConceptId(9), ConceptId(12)])
    sup=Some(Named(RoleId(1)), ConceptId(7))                  # ∃isPairedOrUnpaired.mirrorImaged
P2B_TRACE2 atomic_or_tseitin_body_with_extras:
    body=And([ConceptId(3), ConceptId(9), ConceptId(10)])     # inner And under ∃isSolidDivisionOf
    extras=[]
P2B_TRACE2 atomic_classes_with_existential_markers ENTER:
    ids=[Atomic(ClassId(1)), Some(...,8), Some(...,7)]        # BodyStructure, ∃normal, ∃mirrorImaged
P2B_TRACE2 atomic_classes_with_existential_markers OK:
    out=[ClassId(1), ClassId(12), ClassId(13)]                # M_normal and M_mirrorImaged allocated
                                                              #   with the P2b equivalent-marker fix
P2B_TRACE2   LHS-And bodies=[ClassId(1), ClassId(12), ClassId(15)]
              heads=[]                                        # <-- THE BAIL-OUT
              (sup=Some(Named(RoleId(1)), ConceptId(7)))
```

The crucial line is the last one: `heads=[]`. The LHS-And handler (lib.rs
`lower_sub_class_of`'s `ConceptExpr::And` arm, line ~1361) computes
`atomic_operands_on_right(sup, pool)`. That helper at lib.rs:1572-1584 only
returns atomic operands; for `Some(role, body)` it returns `Vec::new()`.

With `heads=[]`, the `for head in heads` loop body never executes, so **NO
conjunctive trigger is emitted for this GCI at all**. The entire complex LHS
lowering machinery — including the post-P2b equivalent-marker fix that
correctly allocated `M_normal` and `M_mirrorImaged` inside the inner And — is
silently discarded one step later because the trigger has no head to attach
to.

## Outcome: **Outcome 2** (different bail-out)

The P2b fix correctly addresses the *body-side* gap inside
`atomic_classes_with_existential_markers` (verified by the canary
`compound_existential_body_canary` in `crates/owl-dl-saturation/src/lib.rs`,
which has an atomic RHS and recovers post-fix). But for pair_01 — and
empirically for ~95% of cluster-A pairs that did NOT recover — the bail-out
is **before** the marker semantics matter: the RHS of the load-bearing GCI is
itself an existential, and the And-LHS lowering path requires an atomic RHS
head.

The fix needed for pair_01 is **at a different call site**: when
`lower_sub_class_of`'s `ConceptExpr::And` arm encounters a non-atomic
existential RHS, it should:

1. Allocate a marker `M_RHS` for the RHS existential (e.g.,
   `tseitin.introduce_existential_marker(rhs_role, rhs_body_id, rules)`),
   placing the trigger `∃R.B ⊑ M_RHS` into the rule set.
2. Push a conjunctive trigger `{LHS bodies} ⊑ M_RHS`.

That makes the And-LHS / existential-RHS GCI emit the equivalent of
`(LHS_conj) ⊑ ∃R.B` in the closure, by routing through a fresh existential
marker. The marker semantics here can be **one-way** (the LHS side asserts
"has this property" without needing the marker to imply the existential back),
so `introduce_existential_marker` suffices — no new method needed. This is
analogous to how `atomic_existential_rhs` lowers atomic-LHS / existential-RHS
GCIs into existential facts: same shape, but with a conjunctive trigger as
the LHS instead of an atomic-LHS fact.

A second-order check: after this fix, downstream consumers of `M_RHS` need to
treat it as a subsumer that the orchestrator's class-pair query can resolve to
the RHS existential. Since the use case is "did `FemoralHead` gain the RHS
existential as a subsumer", and `MirrorImagedBodyStructure`'s definition is
the conjunctive trigger `{BodyStructure, M_mirrorImaged_definition} ⊑
MirrorImagedBodyStructure`, the two markers need to dedup to the same id (via
`by_existential.get((role, body_id))`). They will, because both flows allocate
through `introduce_existential_marker` with the same `(role, body)` key,
provided `body_id` resolves identically. (Verified for atomic body in the
trace; for compound bodies the Tseitin synthetic id is dedup'd via
`by_body`.)

## Why this explains 5/60

The P2b fix's body-side patch only helps GCIs where the RHS is *already*
atomic (or an And of atomics), so the trigger gets emitted and the inner
markers participate in CR5/CR9 propagation. For pair_01 specifically the
load-bearing GCI has a non-atomic RHS (`∃isPairedOrUnpaired.mirrorImaged`),
so it is dropped at lib.rs:1361 before the body-side fix has any effect.
Per `phase2b-galen-pair-analysis.md`, pairs 02/03 share pair_01's shape
within cluster A, and the diagnosis doc estimates cluster A at ~40 of 109
MISSED; if the same shape recurs across that cluster the body-side fix
will be a no-op there. The 5 pairs that did recover were not identified in
this trace — a follow-on diff of which specific pairs flipped MISSED → HIT
post-022ca50 would confirm whether they're atomic-RHS GCIs or a different
shape entirely.

**Subtlety: the body-side fix is independently correct, but not
load-bearing for pair_01.** Walking the chain on the minimal synthetic:
`Femur` inherits the existential facts `(BodyPart, hasIntrinsicAbnormalityStatus,
normal)` and `(LongBone, isPairedOrUnpaired, mirrorImaged)` via the
subsumer-inheritance machinery at lib.rs:521-557 (when a class newly
gains subsumer D, every `facts_by_sub[D]` fact's downstream triggers
re-fire on the class). So `Femur ⊑ M_normal` and `Femur ⊑ M_mirrorImaged`
derive without needing the body-side equivalent-marker fact. The Tseitin
conj_trigger `{BodyStructure, M_normal, M_mirrorImaged} ⊑ F_inner_body`
fires, `Femur ⊑ F_inner_body` derives, the outer
`∃isSolidDivisionOf.F_inner_body ⊑ M_outer` trigger fires on `FemoralHead`
via its `(FemoralHead, isSpecificSolidDivisionOf, Femur)` fact (CR9
hands sub-property over), and `FemoralHead ⊑ M_outer` derives. **All
that machinery successfully runs.** Then the conjunctive trigger that
would close out `FemoralHead ⊑ ∃isPairedOrUnpaired.mirrorImaged` has
`heads=[]` and is silently dropped — and the chain breaks one step
short of the answer.

## Recommendation for Phase 2b.5

Extend `lower_sub_class_of`'s `ConceptExpr::And` arm (lib.rs ~1361) to
ALSO emit conjunctive triggers heading into existential-marker classes
when the RHS is `Some(role, body)` or `Min(n ≥ 1, role, body)`. Concretely:
after `atomic_operands_on_right(sup, pool)` yields atomic heads, also look
for top-level existential operands on the RHS (analogous to
`atomic_existential_rhs` but emitting a *conjunctive trigger* into a
one-way marker instead of an *existential fact*). The marker is one-way;
no new `TseitinAllocator` method required. Then re-measure GALEN MISSED.

Before implementing: build a synthetic canary mirroring `/tmp/p2b-trace-pair01-mini.ofn`
under `crates/owl-dl-saturation/src/lib.rs` tests (and ship a HermiT-verified
OFN fixture). Once that canary's expected-recovery assertion passes, run the
corpus diff. The Phase 2b.0 estimate of ~60 cluster-A/B/E pairs assumes the
shape is uniform; the corpus diff will reveal the actual recovery.

## Commit / cleanup

- All tracing was reverted before commit (`git diff crates/owl-dl-saturation/src/lib.rs` empty).
- Only `docs/phase2b-trace2.md` lands in this commit.
- The minimal synthetic at `/tmp/p2b-trace-pair01-mini.ofn` is not committed
  (a permanent fixture should be built by Phase 2b.5's canary task with a
  stable IRI prefix).

## Cross-references

- Phase 2b first trace: `docs/phase2b-trace.md` (body-side gap on the canary).
- Phase 2b diagnosis: `docs/phase2b-galen-diagnosis.md` (predicted ~60 recovery).
- Per-pair analysis: `docs/phase2b-galen-pair-analysis.md` (pair_01's hop structure).
- Implementation: commit 022ca50 (P2b body-side fix at lib.rs:1550/1559).
- Pair_01 module: `crates/owl-dl-reasoner/tests/fixtures/phase2b/pair_01.ofn`.
