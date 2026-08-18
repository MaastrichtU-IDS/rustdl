"""End-to-end guard for docs/python-ontology-qa.md.

Runs the tutorial's full QA workflow so the documented snippets can't silently
rot: classify a healthy ontology, diagnose a broken one, apply the repair,
re-check, and read an inferred property assertion. The ontology strings and
assertions here mirror the tutorial exactly — if the API drifts, this fails and
the tutorial needs updating.
"""
import rustdl

# Step 3 — the broken ontology (disjoint-topping bug), in Manchester syntax.
BROKEN = """Prefix: : <urn:pizza#>
Ontology: <urn:pizza>
Class: CheeseTopping
Class: VegetableTopping
DisjointClasses: CheeseTopping, VegetableTopping
Class: CheeseyVegetableTopping
    SubClassOf: CheeseTopping, VegetableTopping
Class: SpicyCheeseyVegetableTopping
    SubClassOf: CheeseyVegetableTopping
"""

# Step 6 — the fixed ontology (DisjointClasses removed).
FIXED = """Prefix: : <urn:pizza#>
Ontology: <urn:pizza>
Class: CheeseTopping
Class: VegetableTopping
Class: CheeseyVegetableTopping
    SubClassOf: CheeseTopping, VegetableTopping
Class: SpicyCheeseyVegetableTopping
    SubClassOf: CheeseyVegetableTopping
"""

# Step 7 — inferred property assertion via a sub-property.
ABOX = """Prefix: : <urn:fam#>
Ontology: <urn:fam>
ObjectProperty: hasParent
    SubPropertyOf: hasAncestor
ObjectProperty: hasAncestor
Individual: a
    Facts: hasParent b
Individual: b
"""

ROOT = "urn:pizza#CheeseyVegetableTopping"
DERIVED = "urn:pizza#SpicyCheeseyVegetableTopping"


def _write(tmp_path, text, name):
    p = tmp_path / name
    p.write_text(text)
    return str(p)


def test_step2_classify_healthy_ontology():
    result = rustdl.classify(rustdl.examples.pizza())
    assert isinstance(result, rustdl.Classification)
    assert result.unsatisfiable == []        # coherent
    assert result.complete is True
    ns = rustdl.examples.PIZZA_NS
    assert result.is_subclass(ns + "AiryTexture", ns + "DoughTexture")


def test_step3_broken_has_unsatisfiable_classes(tmp_path):
    p = _write(tmp_path, BROKEN, "broken.omn")
    result = rustdl.classify(p)
    assert ROOT in result.unsatisfiable
    assert DERIVED in result.unsatisfiable


def test_step4_debug_partitions_root_and_derived(tmp_path):
    p = _write(tmp_path, BROKEN, "broken.omn")
    d = rustdl.debug(p)
    assert d.consistent is True
    assert ROOT in d.unsatisfiable and DERIVED in d.unsatisfiable

    root = next(r for r in d.roots if r.iri == ROOT)
    assert root.justification                       # non-empty, Manchester-rendered
    assert root.repairs                             # at least one minimal repair
    assert DERIVED in root.derives                  # the collateral class

    # dict access + JSON round-trip still work.
    assert d["roots"][0]["iri"] == d.roots[0].iri
    import json
    json.loads(json.dumps(d.to_dict()))


def test_step5_prepare_matches_the_one_shot_calls(tmp_path):
    # The tutorial claims `prepare()` gives the same answers as the one-shot
    # functions, setup paid once. Mirror both snippets so the claim can't drift.
    p = _write(tmp_path, BROKEN, "broken.omn")
    onto = rustdl.prepare(p)
    assert onto.justify(["unsat", ROOT]) == rustdl.justify(p, ["unsat", ROOT])
    assert onto.justify_all(["unsat", ROOT], max=10) == rustdl.justify_all(
        p, ["unsat", ROOT], 10
    )


def test_step6_fix_and_recheck(tmp_path):
    p = _write(tmp_path, FIXED, "fixed.omn")
    d = rustdl.debug(p)
    assert d.consistent is True
    assert d.unsatisfiable == ()                    # coherent again


def test_step7_materialize_inferred_property_assertion(tmp_path):
    p = _write(tmp_path, ABOX, "abox.omn")
    triples = rustdl.materialize_inferred_property_assertions(p)
    # hasAncestor(a, b) is inferred from hasParent ⊑ hasAncestor + hasParent(a, b).
    assert ("urn:fam#a", "urn:fam#hasAncestor", "urn:fam#b") in triples
