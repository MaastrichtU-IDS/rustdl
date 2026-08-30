# `classify` silently drops subsumptions when `ObjectHasSelf` appears under `∀`

**Status: live on v0.4.24 (`0642465`). Not fixed. Reproduced, oracle-adjudicated,
mechanism isolated — root cause NOT yet localised to a line.**

## The defect

`classify` omits subsumptions that `rustdl subclass` on the same binary proves,
whenever `ObjectHasSelf` occurs inside an `ObjectAllValuesFrom`. This is the #66
signature (classify disagreeing with its own per-pair surface), and it survives
the #78/#83 fix that closed #66's original instance.

Minimal shape — `S` and `T` share a definition, so `T ⊑ S` holds for *any*
filler:

```
EquivalentClasses(:S ObjectAllValuesFrom(:p FILLER))
SubClassOf(       :T ObjectAllValuesFrom(:p FILLER))
```

| FILLER | `classify` | `subclass` | HermiT |
|---|---|---|---|
| `ObjectComplementOf(ObjectHasSelf(:r))` | **drops `T ⊑ S`** | yes | yes |
| `ObjectUnionOf(ObjectComplementOf(ObjectHasSelf(:r)), ObjectComplementOf(:Z))` | **drops it** | yes | yes |
| `ObjectUnionOf(ObjectHasSelf(:r), ObjectComplementOf(:Z))` | **drops it** | yes | yes |
| `ObjectUnionOf(ObjectComplementOf(:Z), ObjectComplementOf(:S))` | ok (#78 shape) | yes | yes |

`RUSTDL_HYPERTABLEAU_TRUST_SAT=0` recovers every case, and so does
`RUSTDL_CLASSIFY_VERIFY_REFUTATIONS=1`. So the mechanism is the documented one:
the wedge returns `Sat`, and `trust_sat` converts that into a silent MISS with
`incomplete` staying `false`, so a consumer cannot tell.

## What this is NOT — a correction worth keeping

CLAUDE.md's #78 entry recorded a suspected sibling: `head_atom_for` emits
`Q ∧ R(var,var) → ⊥` for `¬∃R.Self` on the local variable, "so it may be
matchable, but I did not test it". That framing is **wrong in two ways**, and
testing it is what showed so:

1. **It is not the negated-disjunct pattern.** A **positive** `Self`
   (`∀p.(∃r.Self ⊔ ¬Z)`) fails identically, and that expression never reaches
   the `Not` arm at all.
2. **It is not disjunction-specific.** The **single-literal** `∀p.¬∃r.Self`
   fails too — whereas #78's defect provably required *both* a disjunction and a
   negated atomic (a single literal went through `emit_head`'s `Not` arm, which
   appends to an already-anchored body).

Acting on that framing, I anchored the `¬∃R.Self` clause on `X` exactly as #78
was fixed. **It changed nothing** — which is the evidence that the naming clause
is not the faulty line. The patch was discarded rather than kept as a plausible
no-op. Whatever is wrong lies in how the wedge handles `Self` under `∀`
generally, in both polarities.

Self OUTSIDE a `∀` is fine: `U ⊑ ∃r.Self` with `U ⊑ ¬∃r.Self` is correctly
`unsat`, so this is not "Self is unimplemented".

## Reproducing

`crates/owl-dl-reasoner/tests/…` has no canary for this yet. The fixtures above
are self-contained; build them as `.ofn`, run

```sh
rustdl classify --json f.ofn          # T ⊑ S absent
rustdl subclass f.ofn 'http://ex#T' 'http://ex#S'   # yes
RUSTDL_HYPERTABLEAU_TRUST_SAT=0 rustdl classify --json f.ofn   # T ⊑ S present
```

## Severity — corpus impact MEASURED: zero

Completeness, not soundness — a dropped subsumption, never an invented one. But
it is **silent**: `incomplete` stays `false`.

**Corpus probe (2026-08-30, all 1,920 ORE ontologies, nesting-aware scan):**
`ObjectHasSelf` occurs in **9** ontologies, and in **0** of them does it occur
under an `ObjectAllValuesFrom`. So the trigger shape has **no corpus presence at
all** and this defect is unobservable on ORE.

**Pre-existing, not a v0.4.24 regression:** the fixture behaves identically on the
pinned v0.4.23 control and the v0.4.24 candidate.

Priority is therefore low on evidence, not on assumption. Worth fixing for
correctness, not for measured reach.
