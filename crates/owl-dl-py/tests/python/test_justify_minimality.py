"""#625: Python's justify functions carry minimality and stop claiming it when it is
not guaranteed. `H2` (#584) is inconsistent and found on the saturation-only oracle:
a 7-axiom set, 5 of which are inconsistent. Its control is the full oracle
(`RUSTDL_JUSTIFY_SAT_ORACLE=0`, read per call), which returns the minimal set. The
disjunctive ontology is out of the EL/Horn fragment; the EL one is the control."""
import warnings

import pytest

import rustdl

H2 = """Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/o>
Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))
Declaration(Class(:D)) Declaration(Class(:E))
Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:q))
Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
ClassAssertion(:A :a)
SubClassOf(:A ObjectIntersectionOf(:D ObjectAllValuesFrom(:r :B)))
ObjectPropertyAssertion(:r :a :b)
ObjectPropertyRange(:r :C)
DisjointClasses(:B :C)
SubClassOf(:D ObjectHasValue(:q :b))
ObjectPropertyRange(:q :B)
)
"""
DISJUNCTIVE = """Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/d>
Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:D))
SubClassOf(:A ObjectUnionOf(:B :C))
SubClassOf(:B :D)
SubClassOf(:C :D)
)
"""
EL = """Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/e>
Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:D))
SubClassOf(:A :B)
SubClassOf(:B :D)
)
"""
SUB = ["subclass", "http://ex.org/A", "http://ex.org/D"]


@pytest.fixture(autouse=True)
def _default_oracle(monkeypatch):
    monkeypatch.delenv("RUSTDL_JUSTIFY_SAT_ORACLE", raising=False)


def _write(tmp_path, name, src):
    p = tmp_path / name
    p.write_text(src)
    return str(p)


def _minimality(fn, *args):
    with warnings.catch_warnings(record=True) as rec:
        warnings.simplefilter("always")
        result = fn(*args)
    ws = [w for w in rec if issubclass(w.category, rustdl.MinimalityNotGuaranteedWarning)]
    return result, ws


def test_the_saturation_oracle_is_signalled(tmp_path, monkeypatch):
    path = _write(tmp_path, "h2.ofn", H2)
    r, ws = _minimality(rustdl.justify_result, path, ["inconsistent"])
    assert r.saturation_oracle and r.in_fragment
    assert r.minimal == (False,)
    assert len(r.justifications[0]) == 7
    assert len(ws) == 1 and "saturation-only consistency oracle" in str(ws[0].message)
    axioms, ws = _minimality(rustdl.justify, path, ["inconsistent"])
    assert len(axioms) == 7 and len(ws) == 1
    # Control: the full oracle finds the minimal set and claims it, silently.
    monkeypatch.setenv("RUSTDL_JUSTIFY_SAT_ORACLE", "0")
    r, ws = _minimality(rustdl.justify_result, path, ["inconsistent"])
    assert not r.saturation_oracle
    assert r.minimal == (True,) and len(r.justifications[0]) == 5
    assert not ws


def test_justify_all_names_the_justifications_and_the_enumeration(tmp_path, monkeypatch):
    path = _write(tmp_path, "h2.ofn", H2)
    r, ws = _minimality(rustdl.justify_all_result, path, ["inconsistent"], 5)
    assert r.enumeration_complete is False
    assert len(ws) == 1
    msg = str(ws[0].message)
    assert "justification(s) 0" in msg and "missing from the enumeration" in msg
    js, ws = _minimality(rustdl.justify_all, path, ["inconsistent"], 5)
    assert [len(j) for j in js] == [len(j) for j in r.justifications] and len(ws) == 1
    monkeypatch.setenv("RUSTDL_JUSTIFY_SAT_ORACLE", "0")
    r, ws = _minimality(rustdl.justify_all_result, path, ["inconsistent"], 5)
    assert all(r.minimal) and r.enumeration_complete is True and not ws


def test_an_ontology_outside_el_horn_is_signalled(tmp_path):
    path = _write(tmp_path, "disj.ofn", DISJUNCTIVE)
    r, ws = _minimality(rustdl.justify_result, path, SUB)
    assert not r.in_fragment and r.minimal == (False,) and not r.saturation_oracle
    assert len(ws) == 1 and "outside the EL/Horn fragment" in str(ws[0].message)
    r, ws = _minimality(rustdl.justify_all_result, path, SUB)
    assert r.enumeration_complete is False and len(ws) == 1


def test_el_is_minimal_and_silent(tmp_path):
    path = _write(tmp_path, "el.ofn", EL)
    r, ws = _minimality(rustdl.justify_result, path, SUB)
    assert r.in_fragment and r.minimal == (True,) and not ws
    r, ws = _minimality(rustdl.justify_all_result, path, SUB)
    assert r.minimal == (True,) and r.enumeration_complete is True and not ws
    # A truncated set is not complete: `max` equal to... below the count.
    assert rustdl.justify_all_result(path, SUB, 0).enumeration_complete is False


def test_the_warning_points_at_the_caller(tmp_path):
    path = _write(tmp_path, "disj.ofn", DISJUNCTIVE)
    for fn, args in ((rustdl.justify, (SUB,)), (rustdl.justify_all_result, (SUB,))):
        _, ws = _minimality(fn, path, *args)
        assert ws[0].filename == __file__, (fn.__name__, ws[0].filename)
    p = rustdl.prepare(path)
    _, ws = _minimality(p.justify, SUB)
    assert len(ws) == 1 and ws[0].filename == __file__


def test_prepared_ontology_carries_minimality(tmp_path):
    p = rustdl.prepare(_write(tmp_path, "h2.ofn", H2))
    r, ws = _minimality(p.justify_result, ["inconsistent"])
    assert r.minimal == (False,) and len(ws) == 1
    r, ws = _minimality(p.justify_all_result, ["inconsistent"], 5)
    assert r.enumeration_complete is False and len(ws) == 1
    js, ws = _minimality(p.justify_all, ["inconsistent"], 5)
    assert js and len(ws) == 1


def test_debug_warns_once_and_marks_each_section(tmp_path):
    path = _write(tmp_path, "h2.ofn", H2)
    d, ws = _minimality(rustdl.debug, path)
    assert d.consistent is False and d.inconsistency.minimal is False
    assert len(ws) == 1 and str(ws[0].message).startswith("debug:")
    assert d["inconsistency"]["minimal"] is False
