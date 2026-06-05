# SIO disjunction-common-subsumer pass — full corpus parity

Run 2026-06-06. Closes the last 2 corpus MISSES (SIO), giving **FP=0,
MISSED=0 across all 9 corpus ontologies** — full Konclude parity corpus-wide.

## Headline

| Fixture | FP | MISSED |
|---|---|---|
| alehif | 0 | 0 |
| galen | 0 | 0 |
| notgalen | 0 | 0 |
| ro | 0 | 0 |
| shoiq-knowledge | 0 | 0 |
| sulo | 0 | 0 |
| ore-10908-sroiq | 0 | 0 |
| ore-15672-shoin | 0 | 0 |
| **sio** | **0** | **2 → 0** |

`sio`: rustdl_closure = konclude_closure = 8904.

## Root cause

`SIO_010092 ⊑ SIO_001353` and `⊑ SIO_010410` (one sub, two supers). The
gap reduces to deriving `SIO_010092 ⊑ ∃SIO_000644.SIO_000340`:

```
SIO_010092 ⊑ ∃SIO_000225.(SIO_010090 ⊔ SIO_010091)   (has-function . (template-RNA ⊔ template-DNA))
SIO_010090, SIO_010091 ⊑ SIO_010088 ⊑* SIO_000340     (both ⊑ realizable-entity)
SIO_000225 ⊑* SIO_000644                              (has-function ⊑* has-realizable-property)
```

The out-of-EL step is **`(D1 ⊔ D2) ⊑ C` when both disjuncts share subsumer
C**, inside an existential filler. The consequence-based EL saturator drops
existentials with a disjunction filler entirely, so it never derived
`∃SIO_000225.SIO_010088` (and thence `∃SIO_000644.SIO_000340`). All other legs
(the two target conjuncts `SIO_000776`/`SIO_000004`, `SIO_010088 ⊑ SIO_000340`,
the role chain) are already EL-derivable.

The full tableau proves it but times out (>2 min/pair); the wedge proves it on
a minimal module but at full SIO scale it does not close within the classify
budget (deferred=0, so not a dropped-axiom issue — a search-scale issue). Not
the label heuristic (`RUSTDL_LABEL_HEURISTIC=0` still misses).

## The fix

New preprocessing pass `crates/owl-dl-core/src/disjunction_existential.rs`, run
in `convert_ontology`. For `X ⊑ ∃R.(D1 ⊔ … ⊔ Dn)` with all `Di` atomic, it
emits `X ⊑ ∃R.C` for each **minimal common told-subsumer** C of the disjuncts
(told tables are reflexive + transitively closed). The EL saturator then
derives the rest, so classify recovers the pair via its saturation-closure
short-circuit — sidestepping the intractable per-pair tableau/wedge.

**Soundness (under-approximation).** `Di ⊑ C` for all i ⟹ `(D1 ⊔ … ⊔ Dn) ⊑ C`
⟹ `∃R.(…) ⊑ ∃R.C`. Only *told* subsumers of *atomic* disjuncts are used, so no
false positive is possible. Non-atomic disjuncts / derived-only common
subsumers are left to the tableau (a completeness, not soundness, limitation).
FP=0 held across all 9 corpus ontologies (the pass runs on every convert).

## Tests

`crates/owl-dl-core/src/disjunction_existential.rs`:
- `pass_emits_minimal_common_subsumer_existential` — emits `X ⊑ ∃R.E`, not the
  non-minimal `∃R.F`.
- `pass_no_common_subsumer_emits_nothing` — no shared subsumer ⇒ no axiom.

Corpus gate above; `clippy --workspace`, `test --workspace`, doctests, fmt all
clean. Wall impact within noise (told-table build per convert; SIO 31→40 s is
the closure-diff harness, not classify).
