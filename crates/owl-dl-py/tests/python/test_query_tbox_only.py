"""#401: a single query answered without the ABox, on the strength of a wedge
`Sat` for consistency, emits `TrustedConsistencyWarning`. It is not an
`IncompleteQueryWarning` (nothing was cut), and the answer is unchanged.
`RUSTDL_QUERY_TBOX_ONLY` is read on every call, so `monkeypatch` suffices.

The seeded probe runs first (`RUSTDL_QUERY_SEEDED_FIRST_MS`) and would answer
this tiny fixture itself, so the tests of the skip set it to 0.
"""
import warnings

import pytest

import rustdl

# Off pure EL (the union), with an ABox and no nominal.
SRC = """Prefix(:=<http://ex/#>)
Ontology(<http://ex/querytboxonly>
  Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:D))
  Declaration(ObjectProperty(:r))
  Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
  SubClassOf(:A ObjectUnionOf(:C :D))
  SubClassOf(:C :B) SubClassOf(:D :B)
  ClassAssertion(:A :a) ObjectPropertyAssertion(:r :a :b)
)
"""

A, B, C = "http://ex/#A", "http://ex/#B", "http://ex/#C"

CALLS = [
    ("is_class_satisfiable", (A,), True),
    ("is_subclass_of", (A, C), False),
    ("class_expression_satisfiable", (":A",), True),
    ("class_expression_entailed_subclass", (":A", ":C"), False),
]


@pytest.fixture(autouse=True)
def _skip_seeded_first(monkeypatch):
    monkeypatch.setenv("RUSTDL_QUERY_SEEDED_FIRST_MS", "0")


def _call(path, name, args):
    with warnings.catch_warnings(record=True) as caught:
        warnings.simplefilter("always")
        result = getattr(rustdl, name)(str(path), *args)
    return result, [w.category for w in caught]


def test_trusted_consistency_is_its_own_warning():
    assert not issubclass(rustdl.TrustedConsistencyWarning, rustdl.IncompleteQueryWarning)
    assert "TrustedConsistencyWarning" in rustdl.__all__


@pytest.mark.parametrize("name,args,expected", CALLS)
def test_each_query_warns_when_the_abox_is_skipped(tmp_path, name, args, expected):
    p = tmp_path / "o.ofn"
    p.write_text(SRC)
    result, cats = _call(p, name, args)
    assert result is expected
    assert rustdl.TrustedConsistencyWarning in cats, cats
    assert rustdl.IncompleteQueryWarning not in cats or name.startswith("class_expression")


@pytest.mark.parametrize("name,args,expected", CALLS)
def test_no_warning_when_the_abox_is_seeded(tmp_path, monkeypatch, name, args, expected):
    monkeypatch.setenv("RUSTDL_QUERY_TBOX_ONLY", "0")
    p = tmp_path / "o.ofn"
    p.write_text(SRC)
    result, cats = _call(p, name, args)
    assert result is expected
    assert rustdl.TrustedConsistencyWarning not in cats, cats


@pytest.mark.parametrize("name,args,expected", CALLS)
def test_at_the_default_budget_the_seeded_probe_answers_first(
    tmp_path, monkeypatch, name, args, expected
):
    monkeypatch.delenv("RUSTDL_QUERY_SEEDED_FIRST_MS", raising=False)
    p = tmp_path / "o.ofn"
    p.write_text(SRC)
    result, cats = _call(p, name, args)
    assert result is expected
    assert rustdl.TrustedConsistencyWarning not in cats, cats
