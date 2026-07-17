# Saturator disjointness in the complete fragment (Phase A, increment 1) — design (2026-07-17)

First increment of **Phase A — Horn-SROIQ consequence-based closure** (`docs/2026-07-17-deficiency-roadmap.md`,
Tier 3). Grounded in the 2026-07-17 measurement of the 150 Horn-but-not-EL DNF ontologies.

## Why (measured)

The 150 Horn-ish DNF ontologies are **enormous** (median 58,376 classes, p90 232k, max 981k).
They DNF on **scale** — the O(n²) per-pair tableau loop (~3.4 billion pairs at the median) — not
on any hard construct. **One-pass saturation is tractable at that scale** (validated: 41k classes
in 4.2 s, 78k in 14.4 s, 104k in 24.7 s; ~0.25 ms/class, near-linear). They fall off the EL
one-pass fast path because they use out-of-EL constructs the saturator's `saturator_complete_fragment`
allowlist excludes — chiefly symmetric (49), disjoint (39), inverse (32).

Phase A brings these giants onto the complete one-pass CB path, construct by construct. **This
increment adds disjointness.** Disjointness is a **foundation, not a standalone win**: 31 of the
39 disjoint onts also use symmetric, so disjointness alone recovers **~0** DNF onts — the
measurable recovery arrives with the next increment (symmetric). This increment's job is to make
disjointness a sound, complete part of the fast-path fragment so the symmetric increment can build
on it.

## Approach (B: no-functional gate — settled by measurement)

Only **4 of 39** disjoint onts have `FunctionalObjectProperty` (0 have InverseFunctional), so the
"disjoint × functional-merge unproven" concern (the reason `DisjointClasses` is currently excluded)
is **moot for 35/39**. Approach B accepts `DisjointClasses` into the complete fragment **only when
the ontology has no functional or inverse-functional roles** — capturing ~90 % of the value with
**zero unproven interaction** and a simpler gate. The 4 disjoint+functional onts stay on the hybrid
path (a later increment may prove that interaction; not this one). (Rejected: Approach A, verify
the functional interaction — not worth it for 4 onts; Approach C, rewrite the disjointness rule —
it already exists.)

## What already exists (reuse — no engine change)

The saturator already: builds `disjoint_pairs` / `disjoints_by_class` from `DisjointClasses` /
`DisjointUnion`; fires `ElRule::DisjointnessClash` (`Sub(C,A) ∧ Sub(C,B) ∧ Disjoint(A,B) ⟹
Unsat(C)`); and propagates unsat via `process_unsat` **both** to subclasses (`Sub(d,c) ⟹ Unsat(d)`)
**and back through ∃-facts** (`facts_by_target`: `D ⊑ ∃r.C ∧ Unsat(C) ⟹ Unsat(D)`). On the
EL+disjoint fragment **with no functional/∀/cardinality interaction, disjointness produces only
unsatisfiability** (there is no negation in the language beyond the disjointness constraint, so no
new positive subsumptions), and that unsat propagates completely via the two `process_unsat`
channels. Hence the saturator is **complete on this fragment by construction** — the increment is a
gate change, not a new rule.

## The change (small, localized to the allowlist)

`crates/owl-dl-reasoner/src/classify.rs`:

1. `saturator_complete_fragment` — additionally collect the inverse-functional roles (it currently
   collects only `Axiom::FunctionalRole`; add an `Axiom::InverseFunctionalRole` collection), and
   thread both sets into `is_saturator_axiom`.
2. `is_saturator_axiom` — add arms accepting `Axiom::DisjointClasses(_)` and
   `Axiom::DisjointUnion { .. }`, returning `functional_roles.is_empty() &&
   inverse_functional_roles.is_empty()`. When either set is non-empty, these arms return `false`
   (the whole ontology falls to the hybrid path — the current conservative behaviour, unchanged for
   disjoint+functional onts).

**Implementation-order check (concrete first step, not a placeholder):** confirm the form
disjointness takes at the `is_saturator_axiom` check point. `absorb.rs` decomposes
`DisjointClasses` into pairwise `SubClassOf(Ci, ¬Cj)` *rules*, but `is_saturator_axiom`'s existing
`_ => false` arm comment explicitly lists `DisjointClasses` / `DisjointUnion`, indicating the
`Axiom::DisjointClasses` variant is still present in `internal.axioms` at the check point. Verify
this (add a probe/assert in the first test); if the disjoint fragment ALSO reaches the check as
`SubClassOf(A, Not(Atomic))`, extend `is_saturator_concept` to accept `Not(Atomic)` on a
`SubClassOf` RHS under the same no-functional gate. Scope to the explicit `DisjointClasses` /
`DisjointUnion` forms; `SubClassOf(A,¬B)` handled only if the check confirms it reaches there.

## Gate — three tiers (the giants have no baseline/oracle, so the gate cannot be uniform)

The target giants DNF, so there is no hybrid closure to match on them and no reasoner scales to
981k classes as an oracle. The gate is therefore tiered:

1. **Non-regression (byte-identity):** on small/medium disjoint-no-functional ontologies the hybrid
   path *can* classify today, the fast-path closure must be **byte-identical** to the hybrid-path
   closure. Also: the whole curated oracle net (galen/notgalen/sio/wine/ore-10908/ore-15672/alehif/
   pizza/…) stays FP=0/MISSED=0 — most are disjoint-free or functional, so unaffected; any curated
   disjoint-no-functional ont must be byte-identical fast-vs-hybrid.
2. **Empirical MISSED=0 / FP=0 vs Konclude∩HermiT** on the oracle-classifiable disjoint-no-functional
   subset (the D10 unsound-completeness guard — routing to an incomplete saturator that silently
   MISSES is the failure mode to prevent).
3. **By-construction completeness** on the giants no oracle scales to: the CB disjointness rule +
   `process_unsat` back-prop is complete on the EL+disjoint-no-functional Horn fragment (argued
   above). Stated as a by-construction claim — the point of going CB — not an empirical one.

**Canaries** (`crates/owl-dl-reasoner/tests/`):
- basic disjointness clash via the fast path (`X⊑A, X⊑B, Disjoint(A,B)` ⇒ `X` unsatisfiable);
- clash through ∃-fact back-prop (`Y⊑∃r.X, X⊑A, X⊑B, Disjoint(A,B)` ⇒ `X` and `Y` unsatisfiable);
- **no-functional gate**: a disjoint+`FunctionalObjectProperty` ontology is NOT admitted to the
  complete fragment (`saturator_complete_fragment` returns `false`) — assert it takes the hybrid
  path;
- non-regression: a disjoint-no-functional ontology's fast-path closure == its hybrid-path closure.

## Acceptance & non-goals

**Acceptance (foundation — NOT DNF-recovery):** sound + the three-tier gate green. Standalone
DNF-recovery is **expected to be ~0** (documented) because 31/39 disjoint onts also need symmetric;
success is that disjointness is now a sound, complete, fast-path construct that the **symmetric
increment (next)** builds on to reach the first measurable recovery (~31 {disjoint,symmetric} onts).

**Non-goals (YAGNI):** disjoint×functional-merge (the 4 onts — stays hybrid; a later increment);
symmetric / inverse / ABox (later increments); `DisjointObjectProperties`; the `SubClassOf(A,¬B)`
form unless the implementation-order check shows it reaches the allowlist.

## Files touched

- `crates/owl-dl-reasoner/src/classify.rs` — `saturator_complete_fragment` (collect
  inverse-functional) + `is_saturator_axiom` (accept `DisjointClasses` / `DisjointUnion` under the
  no-functional gate).
- `crates/owl-dl-reasoner/tests/` — new disjointness fast-path canaries (the four above).
