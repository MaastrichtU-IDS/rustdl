"""#429: classify, realize, the materialize functions over them, justify,
repair and diagnose emit TrustedConsistencyWarning when an answer rests on the
consistency wedge's model alone, and do not with RUSTDL_WEDGE_CONSISTENCY=0."""

import warnings

import pytest

import rustdl

ONTO = """Prefix(:=<http://ex/#>)
Ontology(<http://ex/trusted429py>
  Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))
  Declaration(Class(:D)) Declaration(ObjectProperty(:r))
  SubClassOf(:A ObjectUnionOf(:C :D)) SubClassOf(:C :B) SubClassOf(:D :B)
  ClassAssertion(:A :a) ObjectPropertyAssertion(:r :a :b))
"""

NOT_ENTAILED = ["subclass", "http://ex/#A", "http://ex/#C"]

SURFACES = [
    ("classify", lambda p: rustdl.classify(p)),
    ("realize", lambda p: rustdl.realize(p)),
    ("materialize_inferred_subclass_axioms", lambda p: rustdl.materialize_inferred_subclass_axioms(p)),
    ("materialize_inferred_class_assertions", lambda p: rustdl.materialize_inferred_class_assertions(p)),
    ("diagnose", lambda p: rustdl.diagnose(p)),
    ("justify", lambda p: rustdl.justify(p, NOT_ENTAILED)),
    ("justify_all", lambda p: rustdl.justify_all(p, NOT_ENTAILED)),
    ("repair", lambda p: rustdl.repair(p, NOT_ENTAILED)),
]


def _trusted_warnings(fn, path):
    with warnings.catch_warnings(record=True) as caught:
        warnings.simplefilter("always")
        fn(path)
    return [w for w in caught if issubclass(w.category, rustdl.TrustedConsistencyWarning)]


@pytest.fixture
def path(tmp_path):
    p = tmp_path / "trusted429.ofn"
    p.write_text(ONTO)
    return str(p)


@pytest.mark.parametrize("name,fn", SURFACES, ids=[s[0] for s in SURFACES])
def test_warns_on_a_trusted_wedge_sat(path, name, fn):
    found = _trusted_warnings(fn, path)
    assert len(found) == 1, f"{name}: {[str(w.message) for w in found]}"


@pytest.mark.parametrize("name,fn", SURFACES, ids=[s[0] for s in SURFACES])
def test_silent_with_the_wedge_off(path, name, fn, monkeypatch):
    monkeypatch.setenv("RUSTDL_WEDGE_CONSISTENCY", "0")
    assert _trusted_warnings(fn, path) == [], name


def test_classification_exposes_the_flag(path, monkeypatch):
    with warnings.catch_warnings():
        warnings.simplefilter("ignore")
        assert rustdl.classify(path).relies_on_trusted_consistency is True
        monkeypatch.setenv("RUSTDL_WEDGE_CONSISTENCY", "0")
        assert rustdl.classify(path).relies_on_trusted_consistency is False
