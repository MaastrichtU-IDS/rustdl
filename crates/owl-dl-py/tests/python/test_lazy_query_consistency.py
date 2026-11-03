"""#369: the query functions raise on an inconsistency only the tableau finds,
and emit `ConsistencyUndeterminedWarning` for a positive answer whose KB
consistency stayed undetermined. `RUSTDL_WEDGE_CONSISTENCY` and the #369 flags
are read on every call, so `monkeypatch` suffices.
"""
import warnings

import pytest

import rustdl

# `{b} ⊑ ≥MIN r.⊤` with `r` functional: MIN=2 is inconsistent (b cannot exist),
# MIN=1 consistent.
SRC = """Prefix(:=<http://ex/#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://ex/t2>
  Declaration(Class(:X)) Declaration(Class(:Y)) Declaration(ObjectProperty(:r))
  Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b)) Declaration(NamedIndividual(:c))
  SubClassOf(ObjectOneOf(:b) ObjectMinCardinality(MIN :r owl:Thing))
  FunctionalObjectProperty(:r)
  ClassAssertion(:Y :a) ClassAssertion(:Y :c)
)
"""

X, Y, A = "http://ex/#X", "http://ex/#Y", "http://ex/#a"

CALLS = [
    ("is_instance_of", (X, A)),
    ("instances_of", (X,)),
    ("class_expression_satisfiable", (":Y",)),
    ("class_expression_entailed_subclass", (":Y", "owl:Thing")),
    ("class_expression_instances", (":X",)),
    ("same_individuals", ()),
    ("different_individuals", ()),
]


@pytest.fixture(autouse=True)
def _clean_env(monkeypatch):
    for k in (
        "RUSTDL_WEDGE_CONSISTENCY",
        "RUSTDL_QUERY_LAZY_CONSISTENCY",
        "RUSTDL_QUERY_LAZY_CONSISTENCY_TAIL",
    ):
        monkeypatch.delenv(k, raising=False)


def _onto(tmp_path, minimum):
    p = tmp_path / "o.ofn"
    p.write_text(SRC.replace("MIN", str(minimum)))
    return str(p)


def _call(name, path, args):
    with warnings.catch_warnings(record=True) as caught:
        warnings.simplefilter("always")
        result = getattr(rustdl, name)(path, *args)
    return result, [w.category for w in caught]


def test_the_warning_is_its_own_class():
    assert not issubclass(rustdl.ConsistencyUndeterminedWarning, rustdl.IncompleteQueryWarning)
    assert "ConsistencyUndeterminedWarning" in rustdl.__all__


@pytest.mark.parametrize("name,args", CALLS)
def test_an_inconsistent_kb_raises(tmp_path, name, args):
    with pytest.raises(rustdl.RustdlError, match="inconsistent"):
        getattr(rustdl, name)(_onto(tmp_path, 2), *args)


@pytest.mark.parametrize("name,args", CALLS)
def test_an_undetermined_kb_warns_and_keeps_the_answer(tmp_path, monkeypatch, name, args):
    monkeypatch.setenv("RUSTDL_WEDGE_CONSISTENCY", "0")
    _, cats = _call(name, _onto(tmp_path, 2), args)
    assert rustdl.ConsistencyUndeterminedWarning in cats, cats


def test_a_consistent_kb_does_not_warn(tmp_path):
    members, cats = _call("instances_of", _onto(tmp_path, 1), (Y,))
    assert members == ["http://ex/#a", "http://ex/#c"]
    assert rustdl.ConsistencyUndeterminedWarning not in cats, cats


# #369 follow-up: the property and materialize functions, on SRC plus property
# assertions, sub-properties and property disjointness.
PROPS = SRC.replace(
    "  ClassAssertion(:Y :a) ClassAssertion(:Y :c)\n",
    "  ClassAssertion(:Y :a) ClassAssertion(:Y :c)\n"
    "  Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:q))"
    " Declaration(ObjectProperty(:s))\n"
    "  Declaration(DataProperty(:d)) Declaration(DataProperty(:e))"
    " Declaration(DataProperty(:f))\n"
    '  ObjectPropertyAssertion(:p :a :c) DataPropertyAssertion(:d :a "1")\n'
    "  SubObjectPropertyOf(:p :s) SubDataPropertyOf(:d :f)\n"
    "  DisjointObjectProperties(:p :q) DisjointDataProperties(:d :e)\n",
)

PROP_CALLS = [
    "object_property_values",
    "data_property_values",
    "disjoint_object_properties",
    "disjoint_data_properties",
    "materialize_inferred_property_assertions",
    "materialize_inferred_data_property_assertions",
    "materialize_inferred_subobjectproperty_axioms",
    "materialize_inferred_subdataproperty_axioms",
]


def _props(tmp_path, minimum):
    p = tmp_path / "props.ofn"
    assert "DisjointObjectProperties" in PROPS
    p.write_text(PROPS.replace("MIN", str(minimum)))
    return str(p)


@pytest.mark.parametrize("name", PROP_CALLS)
def test_a_property_surface_raises_on_an_inconsistent_kb(tmp_path, name):
    with pytest.raises(rustdl.RustdlError, match="inconsistent"):
        getattr(rustdl, name)(_props(tmp_path, 2))


@pytest.mark.parametrize("name", PROP_CALLS)
def test_a_property_surface_warns_when_undetermined(tmp_path, monkeypatch, name):
    monkeypatch.setenv("RUSTDL_WEDGE_CONSISTENCY", "0")
    result, cats = _call(name, _props(tmp_path, 2), ())
    assert result, f"{name}: the fixture answers non-empty"
    assert rustdl.ConsistencyUndeterminedWarning in cats, cats


@pytest.mark.parametrize("name", PROP_CALLS)
def test_a_property_surface_on_a_consistent_kb_does_not_warn(tmp_path, name):
    result, cats = _call(name, _props(tmp_path, 1), ())
    assert result, name
    assert rustdl.ConsistencyUndeterminedWarning not in cats, cats
