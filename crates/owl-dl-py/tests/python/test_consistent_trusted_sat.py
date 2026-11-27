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


def _star():
    """The #357 star: a 9-variable body `RUSTDL_WEDGE_BODY_DECOMPOSE=0` leaves refused."""
    s = (
        "SubClassOf(:B1 :J) SubClassOf(:B2 :J) DisjointClasses(:B1 :B2)\n"
        "SubClassOf(:A ObjectSomeValuesFrom(:q :B1)) SubClassOf(:A ObjectSomeValuesFrom(:q :B2))\n"
    )
    lhs = ""
    for i in range(1, 9):
        s += f"SubClassOf(:A ObjectSomeValuesFrom(:s{i} :S{i}))\n"
        lhs += f" ObjectSomeValuesFrom(:s{i} :S{i})"
    return s + f"SubClassOf(ObjectIntersectionOf({lhs} ObjectMinCardinality(2 :q :J)) :G)\n"


def test_the_warning_names_the_wedges_weaker_kb(tmp_path, monkeypatch):
    """#558: the trusted wedge's refused clauses are named in the warning; with the body
    decomposed (the default) nothing is refused and the text does not mention them."""
    monkeypatch.delenv("RUSTDL_WEDGE_CONSISTENCY", raising=False)
    p = tmp_path / "star.ofn"
    p.write_text(SRC.format(extra=_star()))
    for decompose, named in (("0", True), ("1", False)):
        monkeypatch.setenv("RUSTDL_WEDGE_BODY_DECOMPOSE", decompose)
        with warnings.catch_warnings(record=True) as caught:
            warnings.simplefilter("always")
            assert rustdl.is_consistent(str(p)) is True
        msgs = [str(w.message) for w in caught if w.category is rustdl.TrustedConsistencyWarning]
        assert msgs, caught
        assert ("1 clause(s) it cannot match" in msgs[0]) is named, msgs


def test_the_warning_names_the_wedges_deferrals(tmp_path, monkeypatch):
    """#558 review: a `DisjointUnion` with a complex member makes the consistency
    wedge's clausifier defer its covering; the warning names that deferral, and the
    control (no `DisjointUnion`) does not."""
    monkeypatch.delenv("RUSTDL_WEDGE_CONSISTENCY", raising=False)
    # #600 encodes this covering; the deferral (and so the warning) needs the fix off.
    monkeypatch.setenv("RUSTDL_CLAUSIFY_COMPLEX_DISJOINT", "0")
    du = (
        "Declaration(Class(:E)) Declaration(Class(:F)) Declaration(Class(:G))\n"
        "DisjointUnion(:E :F ObjectSomeValuesFrom(:r :G)) ClassAssertion(:E :b)"
    )
    for extra, named in ((du, True), ("", False)):
        p = tmp_path / f"du{int(named)}.ofn"
        p.write_text(SRC.format(extra=extra))
        with warnings.catch_warnings(record=True) as caught:
            warnings.simplefilter("always")
            assert rustdl.is_consistent(str(p)) is True
        msgs = [str(w.message) for w in caught if w.category is rustdl.TrustedConsistencyWarning]
        assert msgs, caught
        assert ("1 deferral(s)" in msgs[0]) is named, msgs
