# `rustdl repair` — repair suggestions (design)

**Date:** 2026-06-21
**Status:** approved (brainstorming) → ready for implementation plan
**Branch:** `feat/repair-suggestions`

**Sub-feature B** of the explanation/debugging suite (after `justify`/`prove`, the
`diagnose` command — sub-project A — and `justify --laconic` — sub-feature C — all
shipped). The remaining sub-feature, **D** (visual rendering), stays out of scope.

## Goal

Given an unwanted entailment `η` (an unsatisfiable class, a global inconsistency, or
any subsumption / other entailment `justify` supports), compute minimal sets of
axioms to **remove** so that `η` no longer holds. These are Reiter-style diagnoses:
the minimal repairs are exactly the minimal hitting sets over all justifications of
`η`. Where `justify` answers "why does this hold?", `repair` answers "what do I drop
to make it stop?".

## Soundness framing

FP=0 is sacred. A repair derived from the *found* justifications is only guaranteed
to break `η` if we have **all** justifications. `find_all_justifications` is complete
on EL/Horn but `trust_sat`-bounded and `max`-capped out of fragment, so an unfound
justification could leave `η` standing after a "repair". The contract:

1. **Every reported repair is verified** by actually removing it and re-checking:
   `ontology_from(O \ repair) ⊭ η`. This holds even when `find_all` is incomplete —
   a hitting set that fails verification (because an unfound justification survives)
   is discarded, never reported. `repair` adds no entailments and never mutates the
   ontology on disk (it builds throwaway ontologies for the checks).
2. **Completeness** (finding *all* minimal repairs) inherits `justify`'s honesty:
   guaranteed on EL/Horn, best-effort out-of-fragment. The output states which.

## Architecture & placement

- New module `crates/owl-dl-reasoner/src/repair.rs` — `find_repairs(onto, q, max)`
  plus the minimal-hitting-set core. Returns a `Vec<Repair>` where a `Repair` wraps
  the axioms to remove (+ a completeness flag).
- **Reuses, does not reinvent:** `justify::{find_all_justifications, logical_axioms,
  ontology_from, entails, Entailment, Justification}`. The only new logic is the
  minimal-hitting-set enumeration and the per-repair verification; `find_all` does
  the justification discovery and `entails` does the verification.
- CLI: a new `repair` subcommand in `owl-dl-cli`, reusing justify's query parser
  (`parse_justify_query`) and the Manchester / `--labels` renderer.

Units with one responsibility: `minimal_hitting_sets(justifications)` is a pure
combinatorial function; `find_repairs` orchestrates (find_all → MHS → verify); the
CLI formats. Each is independently testable.

## Algorithm

```
1. Js ← find_all_justifications(onto, q, max)      [all justifications of η]
   └─ empty → "not entailed; nothing to repair." Done.
2. candidates ← minimal_hitting_sets(Js)            [each hits every justification]
3. for each H (smallest first):
      build O' = ontology_from(fixed, O_logical \ H)
      if entails(O', η) == false  → H is a sound repair → keep
      else                        → an unfound justification survives → discard (count it)
4. return the verified repairs, smallest-first, capped by `max`
```

A hitting set `H` removes at least one axiom from every justification, so it breaks
every *found* derivation of `η`. Step 3 confirms it breaks `η` outright. On EL/Horn
(all justifications found) every minimal hitting set verifies; out of fragment, some
may not, and those are dropped (with a count surfaced so the user knows the reported
set may be partial).

`O_logical \ H` removes the repair's axioms from the logical-axiom set; `fixed`
(declarations) is always retained.

## Minimal hitting set core (the new combinatorial logic)

`minimal_hitting_sets(justifications: &[BTreeSet<Component>]) -> Vec<BTreeSet<Component>>`
— standard minimal-transversal enumeration:

```
results: Vec<BTreeSet>            // minimal hitting sets found so far
worklist: Vec<BTreeSet> = [ ∅ ]
while let Some(h) = worklist.pop():
    match first justification NOT hit by h:
        None        => h hits all → if no proper subset of h is already in results,
                       insert h and drop any existing superset of h
        Some(ju)    => for each axiom a in ju: worklist.push(h ∪ {a})
return results
```

Minimality is maintained by the subset/superset check at insertion. Bounded by
`max` (return the smallest repairs first; sort by size then deterministically).
Cheap in practice — the number of justifications is small (single digits typically)
and each is short. `max` caps any blow-up; truncation is logged, never silent.

## Output

```
# repair: pizza.ofn  (unsat IceCream)
# 2 minimal repair(s) — complete (Horn)
repair 1 (remove 1 axiom):
  IceCream SubClassOf DomainConcept
repair 2 (remove 1 axiom):
  IceCream SubClassOf hasTopping only FruitTopping
```

For an out-of-fragment ontology the header reads `— w.r.t. found justifications
(completeness not guaranteed)`, and if any candidate hitting set failed verification
a line notes how many were dropped. Flags: `--max N` caps the number of repairs
(default 10); `--labels` glosses entities (reused from justify). Each repair lists
the axioms to remove in Manchester syntax. "Not entailed → nothing to repair" when
`η` does not hold.

## Testing

- **Unit (hitting-set core):** single justification (repairs = each of its axioms,
  all size 1); two disjoint justifications (repairs = one axiom from each, size 2);
  overlapping justifications (the shared axiom is a size-1 repair preferred over
  size-2 ones); minimality (no returned set is a superset of another); `max` cap.
- **Integration:** an ontology with a class unsatisfiable via **two** independent
  justifications → assert the repairs are exactly the minimal hitting sets, and that
  removing each makes the class satisfiable (end-to-end verification); a
  single-justification case; an inconsistency case (`repair inconsistent` → removing
  a repair restores consistency).
- **Soundness property:** for every reported repair `H`, assert
  `ontology_from(O \ H) ⊭ η` (re-check the verification in the test).
- **Corpus:** `repair` on pizza `unsat IceCream` / `unsat CheeseyVegetableTopping`;
  assert each repair verifies and the classification closure is byte-identical
  (read-only); FP=0 / no crash. `#[ignore]`d if slow (SHOIN justify cost, as with
  laconic's corpus test).

## Out of scope (v1)

- **Weakening-based repair** (remove the offending *part* of an axiom, à la laconic,
  rather than the whole axiom) — future; composes with sub-feature C.
- Ranking repairs by axiom usage / "importance", or protecting preferred axioms from
  removal.
- Sub-feature D (visual rendering) — `repair` emits text; D renders it later.
