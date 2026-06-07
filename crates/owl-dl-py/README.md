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

# A small OWL 2 DL ontology ships inside the wheel (gzip-compressed) — no
# download needed. `examples.pizza()` returns its file path (decompressed
# into a per-user cache dir on first use); `examples.PIZZA_NS` is its
# namespace, so class IRIs are PIZZA_NS + local name (e.g. + "Pizza").
from rustdl.examples import pizza, PIZZA_NS, SULO_NS

# Classify. Format is auto-detected from the extension:
# .ofn (OWL Functional), .owx (OWL/XML), .rdf / .owl (RDF/XML).
result = rustdl.classify(pizza())

print(f"{len(result.classes)} classes, {len(result.unsatisfiable)} unsatisfiable, "
      f"complete={result.complete}")
# -> 88 classes, 0 unsatisfiable, complete=True

# Query the computed hierarchy
print(result.is_subclass(PIZZA_NS + "BoxedPizza", PIZZA_NS + "Pizza"))
# -> True
print(len(result.subclasses_of(PIZZA_NS + "FoodMaterial")))
# -> 25

# The pizza ontology is aligned to the SULO upper ontology, so reasoning
# spans both — e.g. a pizza-making timestamp is inferred to be a SULO StartTime:
print(result.is_subclass(PIZZA_NS + "BakingStartTime", SULO_NS + "StartTime"))
# -> True

# Other hierarchy queries (all take full class IRIs):
result.superclasses_of(PIZZA_NS + "Cheese")        # -> list[str]
result.equivalent_classes(PIZZA_NS + "Pizza")      # -> list[str]
result.direct_subsumers(PIZZA_NS + "BoxedPizza")   # -> list[str] (Hasse-direct parents)
```

### Bundled examples

Three real ontologies ship inside the wheel, gzip-compressed (~200 KB total).
They classify with **no network access** — each `examples.X()` decompresses
its ontology into a per-user cache dir (`$XDG_CACHE_HOME/rustdl/examples` or
`~/.cache/rustdl/examples`) on first use, then reuses it. Each `examples.X_NS`
is the namespace, so a class IRI is the namespace plus the local name.

| helper | ontology | classes | notes |
|---|---|---|---|
| `pizza()` / `PIZZA_NS` | ontostart pizza | 88 | SULO-aligned pizza-making ontology; classifies instantly + complete |
| `sulo()` / `SULO_NS` | SULO (Simple Upper-Level Ontology) | 17 | tiny; classifies in milliseconds |
| `sio()` / `SIO_NS` | SIO (Semanticscience Integrated Ontology) | ~1600 | realistic larger workload; takes tens of seconds. Class IRIs are numeric codes, e.g. `SIO_NS + "SIO_000006"` ("process") |

```python
import rustdl
from rustdl import examples

r = rustdl.classify(examples.sulo())
print(r.is_subclass(examples.SULO_NS + "StartTime", examples.SULO_NS + "Object"))
# -> True
```

## API

### Classification

```python
result = rustdl.classify(path, *, per_pair_timeout_ms=1000, saturation_only=False)
result = rustdl.classify_bytes(data, format="ofn", *, per_pair_timeout_ms=1000, saturation_only=False)
```

- `per_pair_timeout_ms` — bound each subsumption test (**default 1000**;
  `0` = unbounded). A pair that exceeds the budget is recorded as "not
  subsumed": **sound** (never a false subsumption) but the result may be
  **incomplete**. When that happens, an `IncompleteClassificationWarning`
  is emitted and `result.complete` is `False`. Pass `0` for the complete,
  unbounded classification. The default bounds pathological SROIQ inputs
  so classification can't hang silently. Conversely, on nominal-heavy
  ontologies (e.g. the W3C wine ontology) the engines never terminate on
  the hard pairs and only burn the full budget, so a *low* value like
  `per_pair_timeout_ms=25` is much faster with no completeness loss
  (wine: 7.5× faster, identical hierarchy, MISSED=0 vs HermiT).
- `saturation_only` — skip the tableau entirely; EL-closure-only
  under-approximation. Dramatically faster on mostly-EL ontologies, and
  always `complete` (no tableau ⇒ no timeout).

`classify` / `classify_bytes` return a `Classification`:

| member | type | meaning |
|---|---|---|
| `.classes` | `list[str]` | all declared class IRIs |
| `.unsatisfiable` | `list[str]` | classes proved `⊑ ⊥` |
| `.inconsistent` | `bool` | whole ontology unsatisfiable |
| `.complete` | `bool` | `False` if any pair hit the timeout (result may miss edges) |
| `.timed_out_pairs` | `int` | how many pairs hit the timeout |
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
