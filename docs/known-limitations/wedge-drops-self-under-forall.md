# `classify` silently drops subsumptions when `ObjectHasSelf` appears under `∀`

**Status: RESOLVED 2026-09-03 by `d51ac27` (closes #90). Root cause was TWO
refusals in `eval_order`, each of which discarded a whole clause.**

`eval_order` built a tree over a clause's role atoms and refused any body whose
atom targeted an already-bound variable. A refused body got **no match plan at
all**, so the clause never fired and its constraint was silently unenforced — the
wedge returning a `Sat` it was not entitled to give. Such an atom is not
unsupported: with both endpoints bound there is nothing to enumerate, so it is a
**filter** — a check that the edge exists. Fixed as `ClauseMatchPlan::filters`,
evaluated at the leaf beside `other_classes`.

| shape | old refusal |
|---|---|
| `∀p.¬Self`, `∀p.(Self ⊔ ¬Z)`, and the no-`∀` case | `NotTree` — the self-loop `R(y,y)` / `R(X,X)` |
| `∀p.(¬Self ⊔ ¬Z)` | `Disconnected` — the naming clause sat on a successor variable nothing binds |

Both halves had to be fixed, which is exactly why the earlier anchoring attempt
measured as a no-op: on `X` the body becomes `R(X,X)`, which the *other* refusal
then discarded. The two sabotages confirm each half is load-bearing — reverting
the matcher filter fails 4 canaries with both controls passing; reverting the
anchoring fails only the `Disconnected` case.

Evidence, with `ro.ofn` as the discriminating case rather than an inert one: it
goes from **6 `NotTree` refusals to none** — six clauses on a real curated
ontology move from silently-ignored to enforced — while its oracle-verified
closure is **unchanged at 158=158**. FP=0 net 22/0 with all 11 closures exact;
two-arm pinned binaries identical on all 10 curated fixtures and all 9
`Self`-bearing ORE ontologies; **0 of 150** sampled ORE ontologies produce the
refusal at all. Canaries `crates/owl-dl-reasoner/tests/self_under_forall.rs`
(6, including an oracle-adjudicated FP guard — enforcing a previously-ignored
body atom ADDS clashes, so the risk here runs toward a false positive).

The sections below are kept as the defect record.

**Correction 1 — "`Self` OUTSIDE a `∀` is fine" is wrong for `classify`.** That
control was checked with `rustdl sat`, which passes. Two axioms, no `∀`:
`U ⊑ ∃r.Self` + `U ⊑ ¬∃r.Self`. Konclude reports
`EquivalentClasses(owl:Nothing, :U)`, `rustdl sat :U` says `unsat`, and
**`classify` reports 0 unsat rows.** Worse than the subsumption cases: `TRUST_SAT=0`,
`CLASSIFY_VERIFY_REFUTATIONS=1`, `COUNTING_PAIR_VERIFY=0`,
`NOMINAL_COUNTING_VERIFY=1` and `COMPLEX_QUALIFIER_VERIFY=0` all leave it at 0, and
**only `RUSTDL_HYPERTABLEAU=0` recovers it** — which localises the fault to the wedge,
not to `trust_sat`, and puts this half in the #91/#98 `needs_verify` family.

**Correction 2 — the three fillers have DIFFERENT clause sets, so one patch could
never have settled them.** Measured with `examples/clause_stats_probe` (added
2026-09-03 because nothing surfaced it — the `# fragment:` banner is identical on all
four fixtures): deferred clauses are **2** for the single-literal `¬∃r.Self`, **0** for
`¬∃r.Self ⊔ ¬Z`, **1** for `∃r.Self ⊔ ¬Z`, 0 for the #78 control. The `Or` case has a
COMPLETE clause set and still fails. This also explains why the anchoring patch
recorded below "changed nothing": on the single-literal fixture the clause is dropped
*before* anchoring can matter, so a patch tested there reads as inert whether or not it
is right.

**Partially fixed (`8ae984c`), and it does NOT close this.** `emit_head`'s `Not` arm
handled only an atomic inner, so `body → ¬∃R.Self` hit `defer("head-not-nonatomic")`
and the axiom left the wedge theory — a silent drop, the D10 shape. The missing arm is
added, appending `Role(R,var,var)` to the enclosing already-anchored body as the atomic
case does. **Deferrals go to 0 on all three fillers and every answer is unchanged**, so
a complete clause set is necessary and not sufficient: the residual fault is in the
ENGINE. Next step is the wedge's self-loop handling — `Role(r,x,x)` as a body atom to
match and as a head atom to derive; note `head_atom_satisfied` returns `false` for
every `Atom::Role`, so such a head is never recognised as satisfied.

Corpus reach of the fix is provably zero (`ObjectHasSelf` in 9 of 1,920 ORE, none
negated, none under `∀`), so its FP=0 net green is non-regression; the correctness
argument is that `body → ¬∃R.Self` *is* `body ∧ R(x,x) → ⊥`.

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
