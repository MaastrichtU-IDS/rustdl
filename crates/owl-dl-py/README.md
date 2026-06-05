# rustdl

Sound, performant **OWL 2 DL (SROIQ) reasoner** in Rust, with Python bindings.
No JVM, no subprocess — native classification via [PyO3](https://pyo3.rs).

`rustdl` beats HermiT on every measured ORE workload and wins outright against
Konclude on Horn-fragment ontologies. See the
[project README](https://github.com/MaastrichtU-IDS/rustdl) for the full
benchmark table.

## Install

```sh
pip install rustdl
```

Wheels are published for CPython 3.10+ on Linux (x86_64, aarch64), macOS
(Apple Silicon), and Windows (AMD64). Other platforms build from the sdist
(needs a Rust toolchain).

## Quick start

```python
import rustdl

# Classify an ontology. Format auto-detected from the extension:
# .ofn (OWL Functional), .owx (OWL/XML), .rdf / .owl (RDF/XML).
result = rustdl.classify("pizza.ofn")

print(f"{len(result.classes)} classes, {len(result.unsatisfiable)} unsatisfiable")

# Query the computed hierarchy
result.is_subclass("http://ex.org/Margherita", "http://ex.org/Pizza")  # -> bool
result.subclasses_of("http://ex.org/Pizza")     # -> list[str]
result.superclasses_of("http://ex.org/Margherita")  # -> list[str]
result.equivalent_classes("http://ex.org/Pizza")    # -> list[str]
result.direct_subsumers("http://ex.org/Margherita") # -> list[str] (Hasse-direct parents)
```

## API

### Classification

```python
result = rustdl.classify(path, *, per_pair_timeout_ms=None, saturation_only=False)
result = rustdl.classify_bytes(data, format="ofn", *, per_pair_timeout_ms=None, saturation_only=False)
```

- `per_pair_timeout_ms` — bound each subsumption test; pairs that exceed it
  default to "not subsumed" (a sound under-approximation — robust against
  pathological SROIQ inputs).
- `saturation_only` — skip the tableau entirely; EL-closure-only
  under-approximation. Dramatically faster on mostly-EL ontologies.

`classify` / `classify_bytes` return a `Classification`:

| member | type | meaning |
|---|---|---|
| `.classes` | `list[str]` | all declared class IRIs |
| `.unsatisfiable` | `list[str]` | classes proved `⊑ ⊥` |
| `.inconsistent` | `bool` | whole ontology unsatisfiable |
| `.is_subclass(sub, sup)` | `bool` | is `sub ⊑ sup` entailed? |
| `.subclasses_of(cls)` | `list[str]` | every `D` with `D ⊑ cls` |
| `.superclasses_of(cls)` | `list[str]` | every `D` with `cls ⊑ D` |
| `.equivalent_classes(cls)` | `list[str]` | classes equivalent to `cls` |
| `.direct_subsumers(cls)` | `list[str]` | Hasse-direct parents of `cls` |

### One-shot queries

Each parses the file, answers one question, and returns:

```python
rustdl.is_consistent(path)                        # -> bool
rustdl.is_class_satisfiable(path, class_iri)      # -> bool
rustdl.is_subclass_of(path, sub_iri, sup_iri)     # -> bool
rustdl.is_instance_of(path, class_iri, indiv_iri) # -> bool
rustdl.instances_of(path, class_iri)              # -> list[str]
rustdl.realize(path)                              # -> dict[str, list[str]]
```

`realize` returns each individual IRI mapped to its most-specific entailed
class IRIs.

> For repeated queries over the same ontology, prefer `classify(path)` once and
> query the returned `Classification` — each top-level function re-parses.

### Inference materialization

```python
rustdl.materialize_inferred_subclass_axioms(path)   # -> list[tuple[str, str]]
rustdl.materialize_inferred_class_assertions(path)  # -> list[tuple[str, str]]
```

`materialize_inferred_subclass_axioms` yields `(sub, sup)` pairs for every
entailed subsumption (excluding reflexive, `owl:Thing`/`owl:Nothing`, and
unsatisfiable classes). `materialize_inferred_class_assertions` yields
`(class, individual)` pairs. Useful for writing an inferred ontology back to
disk.

### Errors

```python
rustdl.RustdlError            # base — catches everything from the library
rustdl.ParseError             # the OWL file couldn't be parsed
rustdl.UnsupportedAxiomError  # HasKey, role chains > length 2, etc.
rustdl.UnknownClassError      # an IRI argument isn't a declared class
```

```python
try:
    result = rustdl.classify("ontology.ofn")
except rustdl.ParseError as e:
    print(f"bad input: {e}")
except rustdl.RustdlError as e:
    print(f"reasoning failed: {e}")
```

## Soundness & coverage

`rustdl` is **sound**: every reported subsumption is a genuine entailment
(FP=0 against Konclude on the validation corpus). Completeness is partial — the
default classifier is empirically near-complete across the measured corpus but
not provably complete on all of SROIQ. `saturation_only` and
`per_pair_timeout_ms` are sound-but-incomplete by construction.

Data-property and datatype axioms outside the recognized preprocessing patterns
are silently dropped (a sound under-approximation). `HasKey` and role chains
longer than length 2 raise `UnsupportedAxiomError`. SWRL rules are skipped.

See the [project documentation](https://github.com/MaastrichtU-IDS/rustdl) for
the full coverage matrix, soundness contract, and architecture notes.

## License

Apache-2.0 OR MIT.
