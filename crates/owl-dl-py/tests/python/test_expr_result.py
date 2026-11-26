"""#552: the class-expression queries expose `completeness_guaranteed` and `dropped`
(through their `*_result` forms) and warn on dropped axioms
(`DroppedAxiomsWarning`), so a Python caller can tell a complete "no" from one over
a lossy conversion.

`xsd:dateTimeStamp` is an unsupported data range, so every axiom that mentions it
is dropped and the rest is pure EL; `xsd:string` drops nothing. The query-side
case puts the unsupported range in the QUERY, on an ontology that drops nothing.
"""
import warnings

import pytest

import rustdl

HEAD = """Prefix(:=<http://e#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(
Declaration(Class(:C)) Declaration(Class(:E)) Declaration(DataProperty(:p))
Declaration(NamedIndividual(:a))
"""

# C ⊑ ∃p.R ⊑ E: C ⊑ E for every data range R. TBox-only, so sat/subclass take the
# pure-EL fast path (an ABox takes an ontology off it).
BODY = """SubClassOf(:C DataSomeValuesFrom(:p RANGE))
SubClassOf(DataSomeValuesFrom(:p RANGE) :E)
)"""

# As BODY plus C(a): a : E. The instance query's fast path admits the ABox.
INST = """SubClassOf(:C DataSomeValuesFrom(:p RANGE))
SubClassOf(DataSomeValuesFrom(:p RANGE) :E)
ClassAssertion(:C :a)
)"""

# Off EL (`∀r.E`) and drops nothing.
OFF_EL = """Declaration(ObjectProperty(:r))
SubClassOf(:C ObjectAllValuesFrom(:r :E))
)"""


def _write(tmp_path, name, body, rng="xsd:string"):
    p = tmp_path / name
    p.write_text(HEAD + body.replace("RANGE", rng))
    return str(p)


def _call(fn, *args):
    with warnings.catch_warnings(record=True) as w:
        warnings.simplefilter("always")
        value = fn(*args)
    dropped_warned = [x for x in w if issubclass(x.category, rustdl.DroppedAxiomsWarning)]
    return value, dropped_warned


def test_input_side_drop_is_reported(tmp_path):
    path = _write(tmp_path, "drop.ofn", BODY, "xsd:dateTimeStamp")
    assert rustdl.dropped_axioms(path), "precondition: the input drops axioms"

    v, w = _call(rustdl.class_expression_entailed_subclass_result, path, ":C", ":E")
    assert isinstance(v, rustdl.ClassExpressionVerdict)
    assert v.holds is False, "precondition: C ⊑ E is missed"
    assert v.completeness_guaranteed is False
    assert v.incomplete is False, "nothing was cut: the two signals differ"
    assert dict(v.dropped) == rustdl.dropped_axioms(path)
    assert w, "DroppedAxiomsWarning expected"

    v, w = _call(rustdl.class_expression_satisfiable_result, path, ":C")
    assert v.completeness_guaranteed is False and v.dropped and w

    ipath = _write(tmp_path, "drop-inst.ofn", INST, "xsd:dateTimeStamp")
    r, w = _call(rustdl.class_expression_instances_result, ipath, ":E")
    assert isinstance(r, rustdl.ClassExpressionInstances)
    assert r.individuals == (), "precondition: a : E is missed"
    assert r.completeness_guaranteed is False and r.dropped and w

    # The bare functions keep their return types and warn the same way.
    for fn, args in (
        (rustdl.class_expression_satisfiable, (":C",)),
        (rustdl.class_expression_entailed_subclass, (":C", ":E")),
    ):
        value, w = _call(fn, path, *args)
        assert type(value) is bool and w
    value, w = _call(rustdl.class_expression_instances, ipath, ":E")
    assert type(value) is list and w


def test_control_drops_nothing_and_is_guaranteed(tmp_path):
    path = _write(tmp_path, "keep.ofn", BODY)
    with warnings.catch_warnings():
        warnings.simplefilter("error", rustdl.DroppedAxiomsWarning)
        v = rustdl.class_expression_entailed_subclass_result(path, ":C", ":E")
        assert v.holds is True and v.completeness_guaranteed is True and v.dropped == ()
        # A guaranteed "no": E ⋢ C, on the pure-EL fast path.
        v = rustdl.class_expression_entailed_subclass_result(path, ":E", ":C")
        assert v.holds is False and v.completeness_guaranteed is True
        ipath = _write(tmp_path, "keep-inst.ofn", INST)
        r = rustdl.class_expression_instances_result(ipath, ":E")
        assert r.individuals == ("http://e#a",)
        assert r.completeness_guaranteed is True and r.dropped == ()
        # The bare forms still return the plain values.
        assert rustdl.class_expression_instances(ipath, ":E") == ["http://e#a"]
        assert rustdl.class_expression_entailed_subclass(path, ":C", ":E") is True


def test_query_side_drop_is_reported(tmp_path):
    """The binding must read the PROBED conversion's drops, not the input's."""
    path = _write(tmp_path, "clean.ofn", BODY)
    assert rustdl.dropped_axioms(path) == {}, "precondition: the input drops nothing"
    x = ":p some xsd:dateTimeStamp"
    v, w = _call(rustdl.class_expression_entailed_subclass_result, path, x, x)
    assert v.holds is False, "precondition: X ⊑ X is missed"
    assert v.dropped, "the query expression's own drop"
    assert v.completeness_guaranteed is False and w


def test_off_el_is_not_guaranteed_and_does_not_warn(tmp_path):
    """The two signals are separate: off EL is not guaranteed, but nothing dropped."""
    path = _write(tmp_path, "offel.ofn", OFF_EL)
    with warnings.catch_warnings():
        warnings.simplefilter("error", rustdl.DroppedAxiomsWarning)
        v = rustdl.class_expression_entailed_subclass_result(path, ":E", ":C")
    assert v.holds is False
    assert v.completeness_guaranteed is False
    assert v.dropped == ()


def test_result_is_a_read_only_mapping(tmp_path):
    path = _write(tmp_path, "keep.ofn", BODY)
    v = rustdl.class_expression_satisfiable_result(path, ":C")
    assert v["holds"] is True and dict(v)["completeness_guaranteed"] is True
    assert set(v.to_dict()) == {
        "holds",
        "incomplete",
        "completeness_guaranteed",
        "relies_on_trusted_consistency",
        "consistency_undetermined",
        "no_verdict",
        "dropped",
    }
    with pytest.raises(Exception):
        v.holds = False  # frozen


def test_results_are_immutable_and_hashable(tmp_path):
    """#596 review: `dropped` was a mutable dict, so a "frozen" result could still be
    changed through it, and no result could be hashed."""
    path = _write(tmp_path, "drop.ofn", BODY, "xsd:dateTimeStamp")
    v = rustdl.class_expression_satisfiable_result(path, ":C")
    assert isinstance(v.dropped, tuple) and v.dropped
    assert all(isinstance(p, tuple) for p in v.dropped)
    assert hash(v) == hash(rustdl.class_expression_satisfiable_result(path, ":C"))
    ipath = _write(tmp_path, "drop-inst.ofn", INST, "xsd:dateTimeStamp")
    r = rustdl.class_expression_instances_result(ipath, ":E")
    assert isinstance(r.individuals, tuple) and isinstance(r.dropped, tuple)
    hash(r)
