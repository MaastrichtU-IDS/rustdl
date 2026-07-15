# Evaluatable cardinality disjunct atoms — design

**Status:** validated by prototype (2026-07-16); for productionization.
**Parent:** dense-SROIQ diagnosis `docs/2026-07-16-phase0-instance-diagnosis.md` + the
depth/rescue-rate/recurrence probes. The approved "new search architecture" was, on the
evidence, a targeted preprocessing fix — this is it.

## Problem

`crates/owl-dl-core/src/clause.rs::head_atom_for` maps a head *disjunct* to a single
atom. It has arms for `Class` / `Some` (→`Exists`) / `Not` (→ complement `Q`), and a
catch-all `_ => atomic_name_of` that names any other compound (including `Min`/`Max`
cardinality) with an **opaque fresh structural class `Q`** and an auxiliary `Q ⊑ disjunct`
clause. `emit_head` (top-level heads) DOES emit evaluatable `AtLeast`/`AtMost` for
`Min`/`Max` — but the *disjunct* path does not.

Consequence: the sufficient (⇐) direction of a defined class `D ≡ soft ⊓ =n r.C ⊓ …`
clausifies (via `absorb_hard_antecedent`) to `soft → ¬(=n r.C) ⊔ … ⊔ D`, where
`¬(=n r.C) = Max(n-1) ⊔ Min(n+1)` — and those `Max`/`Min` disjuncts become **opaque `Q`s**.
`head_atom_satisfied` checks a `Class(Q)` disjunct by `node.has(Q)` — always false until
asserted — so the clause is **never recognized as already-satisfied**, even on a node with
no `r`-successors (where `≤0` holds vacuously). The clause stays perpetually "open" and
the search branches spuriously. With a common `soft` trigger (e.g. `CarbonAtom`, shared by
~15 definitions), every node carrying it branches on all of them → the `ore_ont_10019`
explosion (a bare `CarbonAtom` stalls at 7000 branches / depth 95).

## Fix

Add `Min`/`Max` arms to `head_atom_for` that emit first-class `Atom::AtLeast` / `Atom::AtMost`
(mirroring `emit_head`'s top-level handling: `cardinality_qualifier` for the filler, the
`DKey` guard, and `Min(0)` = `≥0` = ⊤-disjunct ⟹ the Or head is a tautology ⟹ defer/drop
the clause — sound + complete). Then a `¬cardinality` disjunct is an evaluatable atom:
`head_atom_satisfied`'s existing `AtMost` arm (`distinct_role_succ(...).len() ≤ n`)
recognizes the `≤`-part as satisfied on a node with too few successors → the clause is not
open → no branch. When the node genuinely has the cardinality, the disjunct is unsatisfied
and `D` is derived → the ⇐ subsumption is preserved.

**Soundness:** a representation change, not a semantics change — `AtMost`/`AtLeast` ARE the
meaning of `¬cardinality`, and `apply_head_atom` already realises them via the normal
`≤n`/`≥n` rules. It only makes an already-entailed disjunct evaluatable; it cannot
manufacture a clash. **Completeness:** the clause is NOT dropped (unlike the rejected
"defer" option), so ⇐ subsumptions survive.

## Validation (prototype, `RUSTDL_SPIKE_CARD_ATOM`)

| gate | result |
|---|---|
| `ore_ont_10019` | 33→**2** stalled, 216k→**28k** branches |
| `ore_ont_10019` vs Konclude | MISSED **12→3**, FP=0 (150→159) |
| pizza | **byte-identical** (⇐ completeness preserved) |
| sio/ore-10908/ore-15672/alehif/wine/galen/notgalen | **all byte-identical** |
| non-Horn `ore_ont_13723` FP oracle | **FP=0/MISSED=0** |
| full workspace test suite | green (only pre-existing funcmerge-absent env failure) |

A strict improvement: fixes the over-branching, preserves all curated verdicts, and
*recovers* 9 `ore_ont_10019` subsumptions toward Konclude parity.

## Productionization

- Convert the spike flag to a **default-ON** env gate (`RUSTDL_CARD_DISJUNCT_ATOMS`,
  `=0` reverts) read in the clausifier — matches the project's revertible-fix convention
  (`RUSTDL_INVERSE_FUNC_MERGE` etc.). Default-ON because it is a validated strict improvement.
- Tests (TDD): (a) clausification unit test — a `¬(=n r.C)` head disjunct yields
  `AtMost`/`AtLeast` atoms, not an opaque `Class(Q)`; (b) the `Min(0)`-tautology-drop
  arm; (c) the `DKey`-filler guard (no wedge cardinality head over a datatype filler);
  (d) a completeness canary that a defined-class ⇐ subsumption survives (pizza-style);
  (e) FP=0 gate on the non-Horn `ore_ont_13723` oracle.
- Full gate: curated FP=0/MISSED=0 byte-identical + `ore_ont_10019` improvement + workspace green.

## Open / follow-on (not blocking)

- 2 `ore_ont_10019` classes still stall / 3 MISSED — likely the deepest-nested
  `SulfonicAcid*` defs (≥2 cardinality + deep ∃), and possibly the `≥`-part (`AtLeast`)
  which `head_atom_satisfied` does NOT satisfaction-check (TODO(HF3)). Making the
  `AtLeast` disjunct satisfaction-aware is the natural next increment toward full parity.
- Advisor to pressure-test the production soundness (AtLeast-head-atom-in-disjunction
  generating successors; the `Min(0)` tautology-drop; DKey) before default-ON.
