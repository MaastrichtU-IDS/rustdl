"""#392: `is_consistent` emits `TrustedConsistencyWarning` when its `True` rests
only on the wedge's `Sat`, which nothing verified. It is not an
`IncompleteQueryWarning` (nothing was cut), and the answer is unchanged.
`RUSTDL_WEDGE_CONSISTENCY` is read on every call, so `monkeypatch` suffices.
"""
import warnings

import rustdl

# Off pure EL (the union), with an ABox, no nominal and no data counting: the
# wedge decides it.
SRC = """Prefix(:=<http://ex/#>)
Ontology(<http://ex/trustedsat>
  Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:D))
  Declaration(ObjectProperty(:r))
  Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
  SubClassOf(:A ObjectUnionOf(:C :D))
  SubClassOf(:C :B) SubClassOf(:D :B)
  ClassAssertion(:A :a) ObjectPropertyAssertion(:r :a :b)
  {extra}
)
"""


def _is_consistent(tmp_path, extra=""):
    p = tmp_path / "o.ofn"
    p.write_text(SRC.format(extra=extra))
    with warnings.catch_warnings(record=True) as caught:
        warnings.simplefilter("always")
        result = rustdl.is_consistent(str(p))
    return result, [w.category for w in caught]


def test_a_wedge_sat_warns(tmp_path, monkeypatch):
    monkeypatch.delenv("RUSTDL_WEDGE_CONSISTENCY", raising=False)
    result, cats = _is_consistent(tmp_path)
    assert result is True
    assert rustdl.TrustedConsistencyWarning in cats, cats
    assert rustdl.IncompleteQueryWarning not in cats, cats


def test_no_warning_on_a_main_tableau_model(tmp_path, monkeypatch):
    monkeypatch.setenv("RUSTDL_WEDGE_CONSISTENCY", "0")
    result, cats = _is_consistent(tmp_path)
    assert result is True
    assert cats == [], cats


def test_no_warning_on_an_inconsistency(tmp_path):
    result, cats = _is_consistent(tmp_path, "ClassAssertion(ObjectComplementOf(:B) :a)")
    assert result is False
    assert cats == [], cats
