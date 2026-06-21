# `materialize_existential_successors` — existential-witness edges (design)

**Date:** 2026-06-21
**Status:** approved (brainstorming) → ready for implementation plan
**Branch:** `feat/existential-successors`

Backlog item #3. Surfaces the anonymous individuals that existential restrictions
force, for named individuals. The hard one — see the semantics note.

## Semantics & honesty (the crux)

**These are NOT entailed ground triples** (unlike #1/#2). A specific witness edge
`a R _:x` is *not* entailed — entailment holds in every model, and different models
have different witnesses. What *is* entailed is `a : ∃R.C`. So this function returns a
**blank-node representation of entailed existential restrictions**: one row per
entailed `a : ∃R.C` (over a named individual `a`), with a fresh blank node standing
for "some R-successor of `a` that is a C." The name is deliberately
`materialize_existential_successors` (NOT `..._property_assertions`), and the docs say
this explicitly.

## Source & algorithm (structural — sound in any fragment)

```
1. consistency pre-check: abox_saturation::saturate_abox_consistency(internal).clash
      → if clash: Err(ReasonError::Inconsistent)
2. realize(onto) → named individuals + entailed_types(a) for each
3. told-∃ index X → {(R, C)}: from each
     SubClassOf(Class X, sup)            and
     EquivalentClasses members (named class X ⊑ each other member)
   collect top-level / conjunct ObjectSomeValuesFrom{ ObjectProperty R, Class C }
   (named role, named class filler).
4. for each named individual a, each X ∈ entailed_types(a), each (R,C) told for X:
       record distinct (a, R, C)     [a:X ∧ X⊑∃R.C ⟹ a:∃R.C — sound in any fragment]
5. assign one stable blank id per distinct (a,R,C): sort the distinct set,
   id = `_:b{index}`. Emit (a, R, blank, C). Sort + dedup the output.
```

`a:X` comes from `realize` (sound — every reported type is entailed); `X⊑∃R.C` is a
told axiom; their composition `a:∃R.C` is sound regardless of fragment. **No fragment
gate is needed for soundness.**

## Output

`Vec<(subject_iri, property_iri, witness_blank_id, filler_class_iri)>` — one row per
entailed existential successor. Blank ids are deterministic (assigned over the sorted
distinct `(a, R, C)` set), so re-runs are stable and the same existential reuses its
blank node. Example row: `("urn:a", "urn:hasParent", "_:b0", "urn:Person")`.

## Soundness & scope

- **Sound by construction:** each row corresponds to a genuinely-entailed `a : ∃R.C`.
- **Under-approximate (documented):** only **told** `∃R.C`, over a **simple** named
  role with a **named class** filler; **1-step** (no recursion into witnesses → no
  infinite chains, blocking not needed); skips inverse-role fillers, complex/nested
  fillers, and existentials entailed only via deeper reasoning (the saturator
  ∃-marker-decode route, deferred).
- **Inconsistent ontology → `ReasonError::Inconsistent`** (parity with the other
  materializers).
- Read-only; classification untouched. (Not an FP risk: the output is explicitly a
  representation of entailed `∃`, not a claim of entailed ground triples; every row
  maps to a sound `a:∃R.C`.)

## Surfaces

- **Reasoner:** `materialize_existential_successors(onto) -> Result<Vec<(String, String, String, String)>, ReasonError>`.
- **Python:** `materialize_existential_successors(path) -> list[tuple[str,str,str,str]]`
  (in `owl-dl-py/src/materialize.rs`).
- **No CLI** — the blank-node output is a programmatic artifact; folding it into
  `realize --properties` (ground triples) would muddy that output. May add later.

## Testing

- `Person ⊑ ∃hasParent.Person`, `a : Person` ⇒ exactly one row
  `(a, hasParent, _:b, Person)`; the blank id is NOT a named individual; **1-step** —
  the witness itself produces no further row (no recursion).
- entailed-not-asserted type: `a : Y`, `Y ⊑ X`, `X ⊑ ∃r.C` ⇒ row present (uses
  `entailed_types`, not just asserted).
- two distinct existentials `X ⊑ ∃r.C`, `X ⊑ ∃r.D` ⇒ two rows, two distinct blanks;
  the same `(a, r, C)` reached via two types ⇒ a single row (dedup).
- **negative controls:** an individual with no entailed existential ⇒ no rows; an
  inverse-role or complex-filler existential ⇒ not emitted (documented under-approx).
- inconsistent ontology ⇒ `Err`.
- **determinism:** two calls produce byte-identical rows/blank ids.
- **Python** smoke test on the `Person ⊑ ∃hasParent.Person` case.
- **Corpus / FP-safe:** run on ABox-bearing fixtures (family, ro) — no panic, blank
  ids well-formed, classification closure byte-identical.

## Out of scope (v1 → backlog)

- Recursive / bounded-`k` unfolding with blocking (the "real" infinite-model
  representation).
- Inverse-role and complex/nested-filler existentials.
- Non-told existentials (needs the saturator ∃-marker decode).
- #4 disjunctive-reasoning-derived property assertions. **After #3, only #4 remains.**
