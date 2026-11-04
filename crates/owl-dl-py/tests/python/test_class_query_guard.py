"""#428: `is_class_satisfiable` and `is_subclass_of` raise on a KB their guard
proves inconsistent, and emit `ConsistencyUndeterminedWarning` for a positive
answer whose KB consistency stayed undetermined. `owl:Thing` is the consistency
check (#427) and answers instead of raising.
"""
import warnings

import pytest

import rustdl

CLASH = """Prefix(:=<http://ex/#>)
Ontology(<http://ex/clash>
  Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:X)) Declaration(Class(:Z))
  Declaration(NamedIndividual(:a))
  DisjointClasses(:A :B) ClassAssertion(:A :a) ClassAssertion(:B :a)
)
"""

CONSISTENT = """Prefix(:=<http://ex/#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://ex/ok>
  Declaration(Class(:Y)) Declaration(Class(:W)) Declaration(Class(:N))
  Declaration(NamedIndividual(:a))
  ClassAssertion(:Y :a) SubClassOf(:Y :W) SubClassOf(:N owl:Nothing)
)
"""

X, Z = "http://ex/#X", "http://ex/#Z"
Y, W, N = "http://ex/#Y", "http://ex/#W", "http://ex/#N"
THING = "http://www.w3.org/2002/07/owl#Thing"


@pytest.fixture(autouse=True)
def _clean_env(monkeypatch):
    for k in ("RUSTDL_WEDGE_CONSISTENCY", "RUSTDL_CLASS_QUERY_GUARD"):
        monkeypatch.delenv(k, raising=False)


def _write(tmp_path, src):
    p = tmp_path / "o.ofn"
    p.write_text(src)
    return str(p)


def _call(fn, *args):
    with warnings.catch_warnings(record=True) as caught:
        warnings.simplefilter("always")
        result = fn(*args)
    return result, [w.category for w in caught]


def test_an_inconsistent_kb_raises(tmp_path):
    path = _write(tmp_path, CLASH)
    with pytest.raises(rustdl.RustdlError, match="inconsistent"):
        rustdl.is_class_satisfiable(path, X)
    with pytest.raises(rustdl.RustdlError, match="inconsistent"):
        rustdl.is_subclass_of(path, X, Z)
    assert rustdl.is_class_satisfiable(path, THING) is False


def test_the_guard_can_be_turned_off(tmp_path, monkeypatch):
    monkeypatch.setenv("RUSTDL_CLASS_QUERY_GUARD", "0")
    path = _write(tmp_path, CLASH)
    assert rustdl.is_class_satisfiable(path, X) is False
    assert rustdl.is_subclass_of(path, X, Z) is True


def test_an_undetermined_positive_answer_warns(tmp_path, monkeypatch):
    monkeypatch.setenv("RUSTDL_WEDGE_CONSISTENCY", "0")
    path = _write(tmp_path, CONSISTENT)
    sat, caught = _call(rustdl.is_class_satisfiable, path, N)
    assert sat is False and rustdl.ConsistencyUndeterminedWarning in caught
    holds, caught = _call(rustdl.is_subclass_of, path, Y, W)
    assert holds is True and rustdl.ConsistencyUndeterminedWarning in caught
    # Negative answers never run the check.
    sat, caught = _call(rustdl.is_class_satisfiable, path, Y)
    assert sat is True and rustdl.ConsistencyUndeterminedWarning not in caught


def test_a_consistent_kb_does_not_warn(tmp_path):
    path = _write(tmp_path, CONSISTENT)
    holds, caught = _call(rustdl.is_subclass_of, path, Y, W)
    assert holds is True and rustdl.ConsistencyUndeterminedWarning not in caught
