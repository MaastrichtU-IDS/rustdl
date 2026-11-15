# A data assertion with an unsupported datatype under `owl:bottomDataProperty` is dropped whole (#539 residual)

**Status:** open, a sound under-approximation flagged only by `dropped`.

## The case

```
SubDataPropertyOf(:p owl:bottomDataProperty)
DataPropertyAssertion(:p :a "2020-01-01T00:00:00Z"^^xsd:dateTimeStamp)
```

`p` is below the empty data role, so any `p`-value is a contradiction, and HermiT and Konclude both call the file inconsistent. rustdl answers as follows:

- `consistent` prints `consistent`.
- `instance D a` prints `no`.
- On stderr: `warning: 1 axiom(s) not understood and dropped (DataPropertyAssertion: unsupported data range ×1)`.
- `dropped` in the JSON surfaces carries the same count.

## Why

The converter has no `DKey` bucket for `xsd:dateTimeStamp` (or for any datatype outside the supported set), so the whole assertion fails to convert and is recorded in `dropped`. #539's conversion axiom `∃owl:bottomDataProperty.⊤ ⊑ ⊥` empties every sub-role of the bottom data role. But it can only act on assertions that reached the IR, and this one didn't.

The answer is a sound under-approximation, and the only signal is the `dropped` channel. On the definite-negative surfaces (`instance` "no") that signal is a stderr warning, not a field of the answer. #540 records the related gap: the expression queries' `completeness_guaranteed` ignores `dropped`.

## Possible fix (not done)

Salvage the role half of a dropped data assertion: `p(a, v)` implies `(∃p.⊤)(a)` whatever `v`'s datatype is. Emitting that class assertion when the literal's datatype is unsupported would make this case inconsistent on every surface. It would follow the precedent of `salvage_necessary_conjuncts` in `convert.rs`, which keeps the necessary consequences of a dropped axiom.

Pinned by `known_limitation_an_unsupported_literal_under_bottom_data_is_dropped_and_flagged` in `crates/owl-dl-cli/tests/bottom_role_cli.rs`, which asserts today's `consistent` plus the drop warning. Flip it when the salvage lands.
