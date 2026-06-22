# Debugging an ontology with rustdl (Python QA tutorial)

This is an end-to-end walkthrough of using `rustdl` from Python to **find and fix a
modeling bug** in an OWL ontology, then **read inferred facts** back out. It uses
only the bundled offline example and small in-memory ontologies — no downloads, no
external reasoner, no JVM.

Everything here is **sound**: rustdl never reports a subsumption or inconsistency
that doesn't hold. Where it is incomplete it says so (a timed-out classification is
flagged, never silently truncated).

> Ontologies in this tutorial are written in **OWL Manchester syntax** (`.omn`) —
> the most human-readable OWL serialization. rustdl reads it natively (and renders
> its explanations in it). It also reads OWL Functional (`.ofn`), OWL/XML (`.owx`),
> and RDF/XML (`.owl` / `.rdf`).

## 1. Install

```sh
pip install rustdl          # Python 3.10+, prebuilt wheels (ABI3)
```

```python
import rustdl
```

## 2. Classify a healthy ontology

Start with the bundled pizza ontology — a small, coherent ontology that ships
inside the wheel (decompressed to a per-user cache on first use; offline-safe):

```python
result = rustdl.classify(rustdl.examples.pizza())

print(len(result.classes), "classes")           # 88
print("unsatisfiable:", result.unsatisfiable)   # []
print("complete:", result.complete)             # True

ns = rustdl.examples.PIZZA_NS                    # "https://w3id.org/ontostart/pizza/"
print(result.is_subclass(ns + "AiryTexture", ns + "DoughTexture"))   # True
```

A healthy ontology has **no unsatisfiable classes**. `result.complete` is `True`
when every subsumption test finished within its time budget. (`classify` bounds each
pairwise test at 1000 ms by default and flags incompleteness loudly; pass
`per_pair_timeout_ms=0` for an unbounded, fully complete run.)

## 3. A broken ontology

Now a deliberately broken one. Save this Manchester ontology as `broken.omn`:

```python
broken = """Prefix: : <urn:pizza#>
Ontology: <urn:pizza>
Class: CheeseTopping
Class: VegetableTopping
DisjointClasses: CheeseTopping, VegetableTopping
Class: CheeseyVegetableTopping
    SubClassOf: CheeseTopping, VegetableTopping
Class: SpicyCheeseyVegetableTopping
    SubClassOf: CheeseyVegetableTopping
"""
open("broken.omn", "w").write(broken)
```

`CheeseTopping` and `VegetableTopping` are declared **disjoint**, yet
`CheeseyVegetableTopping` is asserted to be a subclass of **both** — a contradiction.
Classify it:

```python
result = rustdl.classify("broken.omn")
print(result.unsatisfiable)
# ['urn:pizza#CheeseyVegetableTopping', 'urn:pizza#SpicyCheeseyVegetableTopping']
```

Two unsatisfiable classes. But which is the *cause* and which is just collateral?

## 4. `debug()` — the one-call diagnosis

`rustdl.debug(path)` returns a structured `Diagnosis` object that answers exactly
that. It partitions the unsatisfiable classes into **roots** (the real causes) and
**derived** (classes that are only broken because a root is), and justifies each
root:

```python
d = rustdl.debug("broken.omn")

print(d.consistent)                 # True  (the ontology is consistent; some classes are just empty)
print(d.unsatisfiable)              # ('urn:pizza#CheeseyVegetableTopping', 'urn:pizza#SpicyCheeseyVegetableTopping')

for root in d.roots:
    print("ROOT:", root.iri)
    for axiom in root.justification:        # the minimal set of axioms responsible
        print("   ", axiom)
    print("  derives:", root.derives)       # classes broken as a consequence
    print("  repairs:", root.repairs)       # minimal ways to fix it
```

Output:

```
ROOT: urn:pizza#CheeseyVegetableTopping
    CheeseyVegetableTopping SubClassOf VegetableTopping
    CheeseyVegetableTopping SubClassOf CheeseTopping
    CheeseTopping DisjointWith VegetableTopping
  derives: ('urn:pizza#SpicyCheeseyVegetableTopping',)
  repairs: (('CheeseyVegetableTopping SubClassOf CheeseTopping',), ('CheeseyVegetableTopping SubClassOf VegetableTopping',), ('CheeseTopping DisjointWith VegetableTopping',))
```

So `CheeseyVegetableTopping` is the **root** cause (three axioms collide), and
`SpicyCheeseyVegetableTopping` is **derived** — it inherits the contradiction and
will fix itself once the root is fixed. Justifications are rendered in Manchester
syntax.

### Attribute access, dict access, or JSON

`Diagnosis` is both a typed object and a `Mapping`, so all three styles work:

```python
d.roots[0].justification            # attribute access (typed, IDE-completable)
d["roots"][0]["justification"]      # dict access (back-compatible)

import json
json.dumps(d.to_dict())             # JSON — note: use to_dict() (a Mapping isn't a dict to json)
```

A coherent ontology returns `d.unsatisfiable == ()` and `d.roots == ()`. If the
ontology were *inconsistent* (not just incoherent), `d.consistent` would be `False`
and `d.inconsistency` would hold the justification + repairs for the inconsistency.

## 5. Zoom in (and the CLI)

`debug()` bundles several primitives you can also call individually:

```python
rustdl.justify("broken.omn", ["unsat", "urn:pizza#CheeseyVegetableTopping"])
# the minimal responsible axiom set (same axioms shown above)

rustdl.repair("broken.omn", ["unsat", "urn:pizza#CheeseyVegetableTopping"], 10)
# every minimal repair, each verified by removal
```

The same workflow is available on the command line, including a richer
`justify --laconic` (which pinpoints the responsible *part* of each axiom) and a
self-contained HTML report:

```sh
rustdl diagnose broken.omn              # root vs derived, justified
rustdl justify  broken.omn unsat urn:pizza#CheeseyVegetableTopping
rustdl justify --laconic broken.omn unsat urn:pizza#CheeseyVegetableTopping
rustdl repair   broken.omn unsat urn:pizza#CheeseyVegetableTopping
rustdl report   broken.omn -o report.html
```

## 6. Fix it and re-check

`debug()` told us three minimal repairs. The modeling intent is "this topping is
both cheese and vegetable", so the over-strong `DisjointClasses` axiom is the one to
drop. Write the fixed ontology and re-run:

```python
fixed = """Prefix: : <urn:pizza#>
Ontology: <urn:pizza>
Class: CheeseTopping
Class: VegetableTopping
Class: CheeseyVegetableTopping
    SubClassOf: CheeseTopping, VegetableTopping
Class: SpicyCheeseyVegetableTopping
    SubClassOf: CheeseyVegetableTopping
"""
open("fixed.omn", "w").write(fixed)

d = rustdl.debug("fixed.omn")
print(d.consistent)       # True
print(d.unsatisfiable)    # ()  — coherent again
```

The loop is closed: diagnose → repair → re-check.

## 7. Bonus — read inferred facts

Once an ontology is healthy, rustdl can surface what it *entails*, not just what was
asserted. Take a tiny ABox where `hasParent` is a sub-property of `hasAncestor`:

```python
abox = """Prefix: : <urn:fam#>
Ontology: <urn:fam>
ObjectProperty: hasParent
    SubPropertyOf: hasAncestor
ObjectProperty: hasAncestor
Individual: a
    Facts: hasParent b
Individual: b
"""
open("abox.omn", "w").write(abox)

triples = rustdl.materialize_inferred_property_assertions("abox.omn")
assert ("urn:fam#a", "urn:fam#hasAncestor", "urn:fam#b") in triples
```

`hasAncestor(a, b)` was never asserted — it's inferred from
`hasParent ⊑ hasAncestor` and `hasParent(a, b)`. Companion helpers cover the rest of
the inference surface:

```python
rustdl.materialize_inferred_data_property_assertions(path)   # inferred data property assertions (5-tuples)
rustdl.materialize_inferred_subobjectproperty_axioms(path)   # inferred object property hierarchy
rustdl.materialize_inferred_subdataproperty_axioms(path)     # inferred data property hierarchy
rustdl.materialize_existential_successors(path)              # entailed ∃-successors (blank-node witnesses)
```

> Note on `materialize_existential_successors`: it returns a *representation* of
> entailed existentials (one blank-node witness row per entailed `a : ∃R.C`), not
> entailed ground triples — the witnesses are model-relative, so they are blank
> nodes, not named individuals.

## Where next

- Full API reference: the `rustdl` package is typed (PEP 561) — your IDE will
  autocomplete every function, `Classification`, and `Diagnosis` field.
- `README.md` for the soundness/coverage contract and the CLI surface.
- The reasoner is sound on every measured ontology (FP=0 vs Konclude). It is a
  near-complete approximation by default; see the README's soundness contract for
  the knobs that trade speed for guaranteed completeness.
