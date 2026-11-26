"""#579: an unresolved `Import` is counted in `dropped`, and the object property
values / same-individuals surfaces read `dropped` (Python sees it through their
`IncompleteQueryWarning`, and the data half through `StructuralGapWarning`)."""

import warnings

import rustdl

HEAD = "Prefix(:=<http://e#>)\nOntology(<http://e/o>\n"
IMPORT = "Import(<http://example.org/not-loaded>)\n"
DECLS = (
    "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(ObjectProperty(:r))\n"
    "Declaration(DataProperty(:d))\n"
)
ABOX = (
    "Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n"
    "ClassAssertion(:A :a) ClassAssertion(:B :b)\n"
)
RULE = (
    "DLSafeRule(Body(ClassAtom(:A Variable(<urn:x>)) ClassAtom(:B Variable(<urn:y>))) "
    "Head(ObjectPropertyAtom(:r Variable(<urn:x>) Variable(<urn:y>))))\n"
)


def _write(tmp_path, name, text):
    p = tmp_path / name
    p.write_text(HEAD + text + ")\n")
    return str(p)


def _warned(fn, *args):
    with warnings.catch_warnings(record=True) as w:
        warnings.simplefilter("always")
        value = fn(*args)
    return value, {x.category for x in w}


def test_an_import_is_counted_and_reaches_the_expression_queries(tmp_path):
    path = _write(tmp_path, "imp.ofn", IMPORT + DECLS + "SubClassOf(:A :B)\n")
    assert rustdl.dropped_axioms(path) == {
        "Import: unsupported axiom (unresolved import)": 1
    }
    v, cats = _warned(rustdl.class_expression_satisfiable_result, path, ":A")
    assert v.completeness_guaranteed is False
    assert rustdl.DroppedAxiomsWarning in cats

    control = _write(tmp_path, "plain.ofn", DECLS + "SubClassOf(:A :B)\n")
    assert rustdl.dropped_axioms(control) == {}
    v, cats = _warned(rustdl.class_expression_satisfiable_result, control, ":A")
    assert v.completeness_guaranteed is True
    assert rustdl.DroppedAxiomsWarning not in cats


def test_object_property_values_warn_on_a_drop(tmp_path):
    # The rule entails r(a, b); no seed edge, so nothing else sets `incomplete`.
    for name, text in [
        ("rule.ofn", DECLS + ABOX + RULE),
        ("imp.ofn", IMPORT + DECLS + ABOX),
    ]:
        path = _write(tmp_path, name, text)
        triples, cats = _warned(rustdl.object_property_values, path)
        assert triples == [], name
        assert rustdl.IncompleteQueryWarning in cats, name
    control = _write(tmp_path, "plain.ofn", DECLS + ABOX)
    _, cats = _warned(rustdl.object_property_values, control)
    assert rustdl.IncompleteQueryWarning not in cats


def test_data_property_values_report_a_drop_as_a_gap(tmp_path):
    path = _write(tmp_path, "imp.ofn", IMPORT + DECLS + ABOX)
    _, cats = _warned(rustdl.data_property_values, path)
    assert rustdl.StructuralGapWarning in cats
    control = _write(tmp_path, "plain.ofn", DECLS + ABOX)
    _, cats = _warned(rustdl.data_property_values, control)
    assert rustdl.StructuralGapWarning not in cats
