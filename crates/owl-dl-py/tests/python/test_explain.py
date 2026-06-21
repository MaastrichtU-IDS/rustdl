"""Tests for the Python explanation/debugging surface + materialize re-exports."""
import rustdl

BROKEN = """Prefix(:=<urn:>)
Ontology(
  Declaration(Class(:A)) Declaration(Class(:Bad)) Declaration(Class(:SubBad))
  SubClassOf(:Bad ObjectIntersectionOf(:A ObjectComplementOf(:A)))
  SubClassOf(:SubBad :Bad)
)
"""


def _write(tmp_path, text, name="o.ofn"):
    p = tmp_path / name
    p.write_text(text)
    return str(p)


# Regression: the materialize_* / explain functions must be reachable as rustdl.X
# (this is the missing __init__ re-export bug that shipped silently).
def test_reexports_present():
    for name in [
        "materialize_inferred_property_assertions",
        "materialize_inferred_data_property_assertions",
        "materialize_inferred_subobjectproperty_axioms",
        "materialize_inferred_subdataproperty_axioms",
        "materialize_existential_successors",
        "justify",
        "justify_all",
        "diagnose",
        "repair",
        "debug",
    ]:
        assert hasattr(rustdl, name), f"rustdl.{name} not exported"
        assert name in rustdl.__all__, f"{name} missing from __all__"


def test_justify(tmp_path):
    p = _write(tmp_path, BROKEN)
    ax = rustdl.justify(p, ["unsat", "urn:Bad"])
    assert ax, "expected a non-empty justification"
    assert any("Bad" in a for a in ax)


def test_diagnose(tmp_path):
    p = _write(tmp_path, BROKEN)
    consistent, roots, derived = rustdl.diagnose(p)
    assert consistent is True
    assert "urn:Bad" in roots
    assert any(d == "urn:SubBad" for (d, _) in derived)


def test_repair(tmp_path):
    p = _write(tmp_path, BROKEN)
    reps = rustdl.repair(p, ["unsat", "urn:Bad"], 10)
    assert reps and all(isinstance(r, list) for r in reps)


def test_debug_consistent_with_unsat(tmp_path):
    p = _write(tmp_path, BROKEN)
    d = rustdl.debug(p)
    assert d["consistent"] is True
    assert "urn:Bad" in d["unsatisfiable"]
    bad = next(r for r in d["roots"] if r["iri"] == "urn:Bad")
    assert bad["justification"] and bad["repairs"]
    assert "urn:SubBad" in bad["derives"]


def test_debug_coherent(tmp_path):
    p = _write(tmp_path, "Prefix(:=<urn:>)\nOntology(Declaration(Class(:A)))\n")
    d = rustdl.debug(p)
    assert d["consistent"] is True
    assert d["unsatisfiable"] == []


def test_materialize_property_assertions(tmp_path):
    p = _write(tmp_path, """Prefix(:=<urn:>)
Ontology(
  Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
  Declaration(ObjectProperty(:hasParent)) Declaration(ObjectProperty(:hasAncestor))
  SubObjectPropertyOf(:hasParent :hasAncestor)
  ObjectPropertyAssertion(:hasParent :a :b)
)
""")
    triples = rustdl.materialize_inferred_property_assertions(p)
    assert ("urn:a", "urn:hasAncestor", "urn:b") in triples
