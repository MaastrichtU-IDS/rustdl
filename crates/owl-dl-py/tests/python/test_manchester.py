"""Manchester syntax (.omn) input — read path wired to horned-owl's omn reader.

rustdl already *writes* Manchester (justify/diagnose/repair output); these tests
cover the symmetric *read* path: classify/debug accept `.omn` files and
`classify_bytes(..., format="omn")`.
"""
import rustdl
import pytest

PIZZA_OMN = """Prefix: : <urn:pizza#>
Ontology: <urn:pizza>
Class: Food
Class: Pizza
    SubClassOf: Food
Class: VegetarianPizza
    SubClassOf: Pizza
Class: Margherita
    SubClassOf: VegetarianPizza
"""

# A deliberately broken ontology: CheeseyVegetableTopping is a subclass of two
# disjoint classes (unsatisfiable), and SpicyCheeseyVegetableTopping inherits
# the contradiction (derived-unsatisfiable).
BROKEN_OMN = """Prefix: : <urn:pizza#>
Ontology: <urn:pizza>
Class: CheeseTopping
Class: VegetableTopping
DisjointClasses: CheeseTopping, VegetableTopping
Class: CheeseyVegetableTopping
    SubClassOf: CheeseTopping, VegetableTopping
Class: SpicyCheeseyVegetableTopping
    SubClassOf: CheeseyVegetableTopping
"""


def _write(tmp_path, text, name="o.omn"):
    p = tmp_path / name
    p.write_text(text)
    return str(p)


def test_classify_omn_path(tmp_path):
    p = _write(tmp_path, PIZZA_OMN)
    result = rustdl.classify(p)
    assert isinstance(result, rustdl.Classification)
    # The subsumption chain reads correctly from Manchester.
    assert result.is_subclass("urn:pizza#Margherita", "urn:pizza#Pizza")
    assert result.is_subclass("urn:pizza#Pizza", "urn:pizza#Food")


def test_classify_bytes_omn():
    result = rustdl.classify_bytes(PIZZA_OMN.encode("utf-8"), format="omn")
    assert "urn:pizza#Margherita" in result.classes
    assert result.is_subclass("urn:pizza#Margherita", "urn:pizza#Food")


def test_classify_bytes_manchester_alias():
    # "manchester" is accepted as an alias for "omn".
    result = rustdl.classify_bytes(PIZZA_OMN.encode("utf-8"), format="manchester")
    assert "urn:pizza#Pizza" in result.classes


def test_debug_omn_broken(tmp_path):
    p = _write(tmp_path, BROKEN_OMN, name="broken.omn")
    d = rustdl.debug(p)
    assert d.consistent is True
    assert "urn:pizza#CheeseyVegetableTopping" in d.unsatisfiable
    root = next(r for r in d.roots if r.iri == "urn:pizza#CheeseyVegetableTopping")
    # Justification axioms render in Manchester.
    assert root.justification
    assert "urn:pizza#SpicyCheeseyVegetableTopping" in root.derives
