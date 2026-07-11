# Draft upstream issue for INCATools/whelk-rs

**Do not file without review.** Target repo: https://github.com/INCATools/whelk-rs
Reproduced against commit `701710d58b6039794bc5a4348880d813eecf2bbb`.

---

## Title
Unsound: super-property existential matched to a sub-property conjunct of a defined class (spurious subsumptions)

## Summary
`assert()` derives subsumptions that are not entailed when a defined class uses an
existential over a **sub-property**. For a defined class

```
C ≡ A ⊓ ∃s.F        with   s ⊑ r   (SubObjectPropertyOf(s, r))
```

whelk-rs appears to fire the conjunctive trigger for `C` on any `D` that is an `A`
and has an existential on the **super-property** `r` (i.e. `D ⊑ ∃r.F′` with
`F′ ⊑ F`), concluding `D ⊑ C`. This is unsound: `∃r.F` does not entail `∃s.F`
(the sound direction is the reverse, `∃s.F ⊑ ∃r.F`).

## Reproduction
Ontology: `ore_ont_3406` from the ORE-2015 corpus (an OBO CHEBI/GOCHE ontology).
Relevant axioms (abridged):

```
SubObjectPropertyOf(:GOCHEREL_0000004 :RO_0000087)
EquivalentClasses(:GOCHE_37527
  ObjectIntersectionOf(:CHEBI_24431
    ObjectSomeValuesFrom(:GOCHEREL_0000004 :CHEBI_37527)))
SubClassOf(:CHEBI_100147 ObjectSomeValuesFrom(:RO_0000087 ...))   # super-property r
```

Running `assert()` and reading off named subsumptions, whelk-rs derives **1,350**
subsumptions of the form `CHEBI_* ⊑ GOCHE_37527` and `CHEBI_* ⊑ GOCHE_51086`
(where `GOCHE_37527 ⊑ GOCHE_51086`), e.g. `CHEBI_100147 ⊑ GOCHE_37527`.

## Expected
None of those 1,350 subsumptions hold. Two independent OWL 2 reasoners agree:
- **Konclude** (v0.7.0): `CHEBI_100147`'s only named superclasses are
  `CHEBI_25384` and `CHEBI_73537`; `GOCHE_37527` is not among them.
- A separate EL reasoner (rustdl) likewise does not derive them.

`CHEBI_100147` has existentials only on the super-property `RO_0000087`, never on
the sub-property `GOCHEREL_0000004` that `GOCHE_37527`'s definition requires, so
it is not an instance of `GOCHE_37527`.

## Likely location
The conjunctive-trigger matching for defined classes seems to index/match the
existential conjunct by a property that includes super-properties (or ignores the
sub/super direction), so a super-property existential satisfies a sub-property
requirement. The fix is to require the trigger's existential to be matched only by
the same property or a **sub**-property of it, not a super-property.

## Environment
- whelk-rs `701710d` (git main at time of testing)
- macOS arm64
- Adjudicating reasoners: Konclude v0.7.0-1138; rustdl (our reasoner).

## Notes
Found via differential testing across whelk-rs / Konclude / our own reasoner; we
are happy to share the exact `ore_ont_3406` file and a `compare-whelk` harness, or
to help reduce this to a self-contained minimal test case. Filed in good faith to
improve whelk-rs. [Disclose AI assistance / authorship per your preference.]
