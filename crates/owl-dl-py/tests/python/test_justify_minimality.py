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
# Three justifications of `A ⊑ D` in EL: {A⊑D}, {A⊑B, B⊑D}, {A⊑C, C⊑D}.
MULTI = """Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/m>
Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:D))
SubClassOf(:A :B) SubClassOf(:B :D) SubClassOf(:A :C) SubClassOf(:C :D) SubClassOf(:A :D)
)
"""
# Two unsatisfiable roots, each only through a disjunction (out of EL/Horn).
TWO_ROOTS = """Prefix(:=<http://ex.org/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://ex.org/t>
Declaration(Class(:X)) Declaration(Class(:Y)) Declaration(Class(:B)) Declaration(Class(:C))
SubClassOf(:B owl:Nothing) SubClassOf(:C owl:Nothing)
SubClassOf(:X ObjectUnionOf(:B :C)) SubClassOf(:Y ObjectUnionOf(:B :C))
)
"""


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
    r = rustdl.justify_all_result(path, SUB, 0)
    assert r.justifications == () and r.enumeration_complete is False


@pytest.mark.parametrize("max_, found, complete", [(2, 2, False), (3, 3, False), (5, 3, True)])
def test_truncation_and_exactly_max(tmp_path, max_, found, complete):
    """Fewer than `max` found is complete; exactly `max` may be truncated, so not."""
    path = _write(tmp_path, "multi.ofn", MULTI)
    r, ws = _minimality(rustdl.justify_all_result, path, SUB, max_)
    assert len(r.justifications) == found and len(r.minimal) == found
    assert r.enumeration_complete is complete and all(r.minimal) and not ws
    assert len(rustdl.justify_all(path, SUB, max_)) == found


def test_a_not_entailed_query_is_silent(tmp_path):
    path = _write(tmp_path, "disj.ofn", DISJUNCTIVE)
    r, ws = _minimality(rustdl.justify_result, path, ["subclass", "http://ex.org/D", "http://ex.org/A"])
    assert r.justifications == () and r.minimal == () and not ws
    assert r.enumeration_complete is True and r.incomplete is False


def test_the_warning_points_at_the_caller(tmp_path):
    path = _write(tmp_path, "disj.ofn", DISJUNCTIVE)
    p = rustdl.prepare(path)
    for label, call in (
        ("justify", lambda: rustdl.justify(path, SUB)),
        ("justify_result", lambda: rustdl.justify_result(path, SUB)),
        ("justify_all", lambda: rustdl.justify_all(path, SUB)),
        ("justify_all_result", lambda: rustdl.justify_all_result(path, SUB)),
        ("p.justify", lambda: p.justify(SUB)),
        ("p.justify_result", lambda: p.justify_result(SUB)),
        ("p.justify_all", lambda: p.justify_all(SUB)),
        ("p.justify_all_result", lambda: p.justify_all_result(SUB)),
        ("debug", lambda: rustdl.debug(_write(tmp_path, "two.ofn", TWO_ROOTS))),
    ):
        _, ws = _minimality(call)
        assert len(ws) == 1, (label, [str(w.message) for w in ws])
        # The lambda's line is in this file; so is the call site the warning names.
        assert ws[0].filename == __file__, (label, ws[0].filename)


def test_prepared_ontology_carries_minimality(tmp_path):
    p = rustdl.prepare(_write(tmp_path, "h2.ofn", H2))
    r, ws = _minimality(p.justify_result, ["inconsistent"])
    assert r.minimal == (False,) and len(ws) == 1
    r, ws = _minimality(p.justify_all_result, ["inconsistent"], 5)
    assert r.enumeration_complete is False and len(ws) == 1
    js, ws = _minimality(p.justify_all, ["inconsistent"], 5)
    assert js and len(ws) == 1


def test_debug_warns_once_over_several_roots(tmp_path):
    d, ws = _minimality(rustdl.debug, _write(tmp_path, "two.ofn", TWO_ROOTS))
    # `B` and `C` are roots too (each told ⊑ ⊥). The fragment is the whole ontology's,
    # which the disjunction takes out of EL/Horn, so no root's justification is
    # guaranteed minimal: four roots, one warning.
    assert {r.iri for r in d.roots} >= {"http://ex.org/X", "http://ex.org/Y"}
    assert all(r.minimal is False for r in d.roots)
    assert len(ws) == 1 and f"{len(d.roots)} justification(s)" in str(ws[0].message)


def test_debug_warns_once_and_marks_each_section(tmp_path):
    path = _write(tmp_path, "h2.ofn", H2)
    d, ws = _minimality(rustdl.debug, path)
    assert d.consistent is False and d.inconsistency.minimal is False
    assert len(ws) == 1 and str(ws[0].message).startswith("debug:")
    assert d["inconsistency"]["minimal"] is False
