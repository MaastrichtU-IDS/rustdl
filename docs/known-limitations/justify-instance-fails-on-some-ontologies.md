# `justify … instance` fails on some ontologies where `instance` succeeds — UNRESOLVED

**Status: diagnosed to a point, cause NOT found. Recorded so the next attempt starts here.**

## Reproducer

```sh
B=http://oaei.ontologymatching.org/2006/benchmarks/201/onto.rdf
f=ore_ont_10009.owl

rustdl justify  "$f" instance "$B#a32071928c" "$B#sqdsq"   # Error: class IRI not in ontology: …#a32071928c
rustdl instance "$f" "$B#sqdsq" "$B#a32071928c"            # yes
```

Same ontology, same two IRIs, contradictory outcomes.

## What is ESTABLISHED

* **The argument order is right.** `justify` takes `instance I C` (individual first) and that is
  confirmed by a working control: on `tests/fixtures/realize_derived_same/inverse-functional.ofn`,
  `justify … instance http://t/x http://t/B` returns a 4-axiom minimal justification. The `instance`
  subcommand separately takes `<CLASS_IRI> <INDIVIDUAL_IRI>` — its own `--help` Usage line says so
  explicitly, so the two surfaces differ by design and neither is wrong.
* **Both IRIs resolve.** `instance` on the same ontology returns `yes`, which requires both
  `class_id` and `individual_id` to succeed in `is_instance_of_internal`. So this is not a
  vocabulary asymmetry — a hypothesis I held and disproved.
* **It is NOT the ⊥-locality module pre-pass.** `RUSTDL_JUSTIFY_NO_MODULE=1` produces the identical
  error, refuting the leading hypothesis.
* **The individual is genuinely in the ontology** — `realize --json` lists it, and it appears in a
  `ClassAssertion` with **no** `Declaration(NamedIndividual(...))`, which is legal OWL 2.

## What was FIXED along the way

The error message was actively misleading. `is_instance_of_internal` raised
`ReasonError::UnknownClass(individual_iri)` when the **individual** lookup failed, so the message
read *"class IRI not in ontology: \<the individual\>"*. That cost two separate diagnosis attempts,
each chasing an argument-order bug that does not exist. A new `ReasonError::UnknownIndividual`
variant now reports the right argument kind:

```
instance A  nosuchind    -> individual IRI not in ontology: http://t/nosuchind
instance nosuchclass  x  -> class IRI not in ontology: http://t/nosuchclass
```

**An error that names the wrong argument KIND is worse than a generic one** — it does not merely
fail to help, it actively misdirects.

## Where to look next

The failing lookup is inside `justify`'s pipeline, after the point where `instance` succeeds, and it
is not module extraction. Candidates, untested:

1. `justify` may re-convert or re-build the ontology through a path that differs from
   `is_instance_of`'s (e.g. a probe injection that rebuilds the vocabulary, dropping undeclared
   individuals).
2. The `EquivalentClasses(Q, CE)` probe-injection pattern used for complex queries may run for
   `instance` too and re-intern.
3. Note the fixture that WORKS has an explicit `Declaration(NamedIndividual(:x))` while
   `ore_ont_10009` does **not** declare `a32071928c`. **That is the most promising single
   discriminator and it is one test away:** add a `Declaration(NamedIndividual(...))` to a copy of
   the failing ontology and re-run. If it then succeeds, the bug is that some path in `justify`
   builds its vocabulary from declarations only.
