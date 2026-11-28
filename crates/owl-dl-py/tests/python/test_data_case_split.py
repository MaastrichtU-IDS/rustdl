"""#589: where an entailment may need reasoning by cases over data values, which rustdl
does not do (a data range is an atom to every engine), Python says so:
`DataCaseSplitWarning`, `Classification.data_case_split_risk` (which voids
`completeness_guaranteed`), the expression results' `data_case_split_risk`, and
`rustdl._data_case_split_risk(path)`. The positive fixture entails `C ⊑ E`
(`C ⊑ ∃d.{1,2}`, `hasValue(d,1) ⊑ E`, `hasValue(d,2) ⊑ E`); the pure-EL gate would
otherwise certify its closure complete. The control has only single values, so no
split is needed."""
import warnings

import rustdl

E = "http://e#"
HEAD = """Prefix(:=<http://e#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(
Declaration(Class(:C)) Declaration(Class(:E)) Declaration(DataProperty(:d))
Declaration(NamedIndividual(:a))
"""
POSITIVE_AXIOMS = """SubClassOf(:C DataSomeValuesFrom(:d DataOneOf("1"^^xsd:integer "2"^^xsd:integer)))
SubClassOf(DataHasValue(:d "1"^^xsd:integer) :E)
SubClassOf(DataHasValue(:d "2"^^xsd:integer) :E)
"""
POSITIVE = HEAD + POSITIVE_AXIOMS + ")"
# Every range a single value: `C ⊑ ∃d.{1}` and `hasValue(d,1) ⊑ E`.
POINTS = HEAD + """SubClassOf(:C DataSomeValuesFrom(:d DataOneOf("1"^^xsd:integer)))
SubClassOf(DataHasValue(:d "1"^^xsd:integer) :E)
)"""
# The positive fixture made inconsistent: a proved inconsistency masks the flag.
MASKED = HEAD + POSITIVE_AXIOMS + "ClassAssertion(owl:Nothing :a)\n)"


def _write(tmp_path, name, src):
    p = tmp_path / name
    p.write_text(src)
    return str(p)


def _split_warnings(fn, *args, **kwargs):
    with warnings.catch_warnings(record=True) as rec:
        warnings.simplefilter("always")
        result = fn(*args, **kwargs)
    return result, [w for w in rec if issubclass(w.category, rustdl.DataCaseSplitWarning)]


def test_a_split_risk_warns_and_voids_the_guarantee(tmp_path):
    path = _write(tmp_path, "pos.ofn", POSITIVE)
    assert rustdl._data_case_split_risk(path) is True
    c, ws = _split_warnings(rustdl.classify, path)
    assert len(ws) == 1, [str(w.message) for w in ws]
    assert str(ws[0].message).startswith("classify:")
    # stacklevel: the warning points at the caller, not at rustdl.
    assert ws[0].filename == __file__, ws[0].filename
    assert c.data_case_split_risk is True
    assert c.completeness_guaranteed is False
    c, ws = _split_warnings(rustdl.classify_bytes, POSITIVE.encode(), format="ofn")
    assert ws, "classify_bytes"
    assert c.data_case_split_risk is True
    _, ws = _split_warnings(rustdl.is_subclass_of, path, E + "C", E + "E")
    assert len(ws) == 1, "is_subclass_of"
    assert ws[0].filename == __file__, ws[0].filename
    _, ws = _split_warnings(rustdl.is_consistent, path)
    assert ws, "is_consistent"
    # The flag has no role-level form; the role-level functions take it too.
    _, ws = _split_warnings(rustdl.data_property_values, path)
    assert ws, "data_property_values"


def test_single_values_are_silent(tmp_path):
    path = _write(tmp_path, "points.ofn", POINTS)
    assert rustdl._data_case_split_risk(path) is False
    c, ws = _split_warnings(rustdl.classify, path)
    assert not ws, [str(w.message) for w in ws]
    assert c.data_case_split_risk is False
    # The same shape as the positive fixture, so its `False` there is the flag's.
    assert c.completeness_guaranteed is True
    _, ws = _split_warnings(rustdl.is_subclass_of, path, E + "C", E + "E")
    assert not ws
    v, ws = _split_warnings(rustdl.class_expression_entailed_subclass_result, path, ":C", ":E")
    assert not ws
    assert v.data_case_split_risk is False


def test_an_expression_query_carries_the_flag(tmp_path):
    path = _write(tmp_path, "expr.ofn", POSITIVE)
    v, ws = _split_warnings(rustdl.class_expression_entailed_subclass_result, path, ":C", ":E")
    assert len(ws) == 1, [str(w.message) for w in ws]
    assert ws[0].filename == __file__, ws[0].filename
    assert v.data_case_split_risk is True
    assert v["data_case_split_risk"] is True
    assert v.completeness_guaranteed is False
    r, ws = _split_warnings(rustdl.class_expression_instances_result, path, ":C")
    assert ws
    assert r.data_case_split_risk is True
    assert r.completeness_guaranteed is False


def test_a_proved_inconsistency_masks_the_flag(tmp_path):
    path = _write(tmp_path, "masked.ofn", MASKED)
    c, ws = _split_warnings(rustdl.classify, path)
    assert c.inconsistent is True
    assert c.data_case_split_risk is False
    assert not ws
    consistent, ws = _split_warnings(rustdl.is_consistent, path)
    assert consistent is False
    assert not ws, "a proved inconsistency is not hidden by the data case split"


def test_prepared_justify_warns(tmp_path):
    path = _write(tmp_path, "prep.ofn", POSITIVE)
    p = rustdl.prepare(path)
    assert p.data_case_split_risk is True
    _, ws = _split_warnings(p.justify, ["subclass", E + "C", E + "E"])
    assert ws
    clean = rustdl.prepare(_write(tmp_path, "prep-points.ofn", POINTS))
    assert clean.data_case_split_risk is False
    _, ws = _split_warnings(clean.justify, ["subclass", E + "C", E + "E"])
    assert not ws


def test_conversion_notes_carry_the_flag(tmp_path):
    """The wrappers' fallback (`_conversion_notes`) and the native's record agree."""
    path = _write(tmp_path, "notes.ofn", POSITIVE)
    dropped, uro, uro_for_roles, dcs = rustdl._conversion_notes(path)
    assert dropped == () and uro is False and uro_for_roles is False
    assert dcs is True
