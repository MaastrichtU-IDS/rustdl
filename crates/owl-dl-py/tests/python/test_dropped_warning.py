"""#597: every Python query warns `DroppedAxiomsWarning` when conversion dropped
axioms (before, only the class-expression queries did, #552). `HasKey` is the
confirmed live drop (crates/owl-dl-reasoner/tests/dropped_axioms.rs)."""
import inspect
import warnings

import pytest

import rustdl

E = "http://e#"
HEAD = """Prefix(:=<http://e#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(
Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))
Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))
Declaration(DataProperty(:d)) Declaration(DataProperty(:e))
Declaration(NamedIndividual(:x)) Declaration(NamedIndividual(:y))
SubClassOf(:A :B) SubObjectPropertyOf(:r :s) SubDataPropertyOf(:d :e)
ClassAssertion(:A :x) ObjectPropertyAssertion(:r :x :y)
DataPropertyAssertion(:d :x "1"^^xsd:integer)
"""
CLEAN = HEAD + ")\n"
WITH_KEY = HEAD + "HasKey(:A (:r) ())\n)\n"

# Every query `_with_conversion_notes` wraps, with arguments.
WRAPPED = {
    "justify": (["subclass", E + "A", E + "B"],),
    "justify_all": (["subclass", E + "A", E + "B"],),
    "justify_result": (["subclass", E + "A", E + "B"],),
    "justify_all_result": (["subclass", E + "A", E + "B"],),
    "diagnose": (),
    "repair": (["subclass", E + "A", E + "B"],),
    "is_consistent": (),
    "is_class_satisfiable": (E + "A",),
    "is_subclass_of": (E + "A", E + "B"),
    "is_instance_of": (E + "B", E + "x"),
    "instances_of": (E + "B",),
    "realize": (),
    "materialize_inferred_subclass_axioms": (),
    "materialize_inferred_class_assertions": (),
    "materialize_existential_successors": (),
    "materialize_inferred_property_assertions": (),
    "materialize_inferred_data_property_assertions": (),
    "materialize_inferred_subobjectproperty_axioms": (),
    "materialize_inferred_subdataproperty_axioms": (),
    "object_property_hierarchy": (),
    "data_property_hierarchy": (),
    "disjoint_classes": (),
    "disjoint_object_properties": (),
    "disjoint_data_properties": (),
    "same_individuals": (),
    "different_individuals": (),
    "object_property_values": (),
    "data_property_values": (),
}


def _write(tmp_path, name, src):
    p = tmp_path / name
    p.write_text(src)
    return str(p)


def _record(fn, *args):
    with warnings.catch_warnings(record=True) as rec:
        warnings.simplefilter("always")
        result = fn(*args)
    return result, [w for w in rec if issubclass(w.category, rustdl.DroppedAxiomsWarning)]


@pytest.mark.parametrize("name", sorted(WRAPPED))
def test_every_wrapped_query_warns_once(tmp_path, name):
    path = _write(tmp_path, "key.ofn", WITH_KEY)
    _, ws = _record(getattr(rustdl, name), path, *WRAPPED[name])
    assert len(ws) == 1, [str(w.message) for w in ws]
    msg = str(ws[0].message)
    assert msg.startswith(name + ":") and "HasKey" in msg, msg
    # stacklevel: the warning points at the caller, not at rustdl.
    assert ws[0].filename == __file__, ws[0].filename


@pytest.mark.parametrize("name", sorted(WRAPPED))
def test_a_clean_ontology_is_silent(tmp_path, name):
    path = _write(tmp_path, "clean.ofn", CLEAN)
    _, ws = _record(getattr(rustdl, name), path, *WRAPPED[name])
    assert not ws, [str(w.message) for w in ws]


def test_classify_reads_the_drops_off_the_result(tmp_path):
    path = _write(tmp_path, "key.ofn", WITH_KEY)
    c, ws = _record(rustdl.classify, path)
    assert len(ws) == 1 and "HasKey" in str(ws[0].message)
    assert ws[0].filename == __file__, ws[0].filename
    assert any("HasKey" in k for k in c.dropped), c.dropped
    assert c.completeness_guaranteed is False
    with warnings.catch_warnings(record=True) as rec:
        warnings.simplefilter("always")
        c = rustdl.classify_bytes(WITH_KEY.encode(), format="ofn")
    ws = [w for w in rec if issubclass(w.category, rustdl.DroppedAxiomsWarning)]
    assert len(ws) == 1 and ws[0].filename == __file__
    assert any("HasKey" in k for k in c.dropped), c.dropped


def test_classify_clean_has_no_drops(tmp_path):
    path = _write(tmp_path, "clean.ofn", CLEAN)
    c, ws = _record(rustdl.classify, path)
    assert not ws
    assert c.dropped == {}


def test_prepared_ontology_warns(tmp_path):
    path = _write(tmp_path, "key.ofn", WITH_KEY)
    for p in (rustdl.prepare(path), rustdl.prepare_bytes(WITH_KEY.encode(), format="ofn")):
        assert any("HasKey" in k for k in p.dropped), p.dropped
        _, ws = _record(p.justify, ["subclass", E + "A", E + "B"])
        assert len(ws) == 1 and ws[0].filename == __file__
        _, ws = _record(p.justify_all, ["subclass", E + "A", E + "B"])
        assert len(ws) == 1
    clean = rustdl.prepare(_write(tmp_path, "clean.ofn", CLEAN))
    assert clean.dropped == {}
    _, ws = _record(clean.justify, ["subclass", E + "A", E + "B"])
    assert not ws


def test_a_proved_inconsistency_does_not_mask_the_drops(tmp_path):
    src = WITH_KEY.replace("HasKey", "ClassAssertion(owl:Nothing :y)\nHasKey")
    path = _write(tmp_path, "incons.ofn", src)
    consistent, ws = _record(rustdl.is_consistent, path)
    assert consistent is False
    assert len(ws) == 1, "a repair over the weaker KB may not be one of the full KB"


def test_both_notes_from_one_conversion(tmp_path):
    src = WITH_KEY.replace(
        "HasKey",
        "SubClassOf(owl:Thing ObjectSomeValuesFrom(:r :C))\n"
        "SubClassOf(ObjectSomeValuesFrom(owl:topObjectProperty :C) owl:Nothing)\nHasKey",
    )
    path = _write(tmp_path, "both.ofn", src)
    with warnings.catch_warnings(record=True) as rec:
        warnings.simplefilter("always")
        rustdl.is_subclass_of(path, E + "A", E + "B")
    cats = [w.category for w in rec]
    assert cats.count(rustdl.DroppedAxiomsWarning) == 1, cats
    assert cats.count(rustdl.UniversalRoleWarning) == 1, cats


def _no_second_parse(monkeypatch):
    def refuse(p):
        raise AssertionError(f"fell back to a second parse of {p}")

    monkeypatch.setattr(rustdl, "_conversion_notes_native", refuse)


@pytest.mark.parametrize("name", sorted(WRAPPED))
def test_no_query_parses_the_file_twice(tmp_path, monkeypatch, name):
    """Each native records the notes from its own parse; the wrapper's fallback
    (a second parse) is never used. The three stats-reporting queries convert again
    only when something was dropped."""
    _no_second_parse(monkeypatch)
    for fixture, expected in ((WITH_KEY, 1), (CLEAN, 0)):
        path = _write(tmp_path, "o.ofn", fixture)
        _, ws = _record(getattr(rustdl, name), path, *WRAPPED[name])
        assert len(ws) == expected, (name, [str(w.message) for w in ws])


def test_debug_warns_once(tmp_path, monkeypatch):
    _no_second_parse(monkeypatch)
    path = _write(tmp_path, "key.ofn", WITH_KEY.replace("HasKey", "SubClassOf(:C owl:Nothing)\nHasKey"))
    d, ws = _record(rustdl.debug, path)
    assert d.roots, "the fixture has an unsatisfiable root, so justify/repair run"
    assert len(ws) == 1, [str(w.message) for w in ws]
    assert str(ws[0].message).startswith("debug:") and ws[0].filename == __file__


# Path-taking public functions that warn from their own result, not the wrapper.
_SELF_REPORTING = {
    "classify",
    "debug",
    "prepare",
    "class_expression_satisfiable",
    "class_expression_satisfiable_result",
    "class_expression_entailed_subclass",
    "class_expression_entailed_subclass_result",
    "class_expression_instances",
    "class_expression_instances_result",
}
# Path-taking functions that report, rather than answer a query.
_REPORTS = {"dropped_axioms", "universal_role_ordinary", "render_manchester"}


def test_every_path_taking_query_is_covered():
    """A new path-taking query must be wrapped (and listed in WRAPPED) or report
    its drops itself."""
    path_taking = set()
    for name in rustdl.__all__:
        fn = getattr(rustdl, name)
        if not callable(fn) or isinstance(fn, type):
            continue
        try:
            params = list(inspect.signature(fn).parameters)
        except (TypeError, ValueError):
            continue
        if params[:1] == ["path"]:
            path_taking.add(name)
    assert path_taking - _REPORTS == set(WRAPPED) | _SELF_REPORTING, (
        sorted(path_taking - _REPORTS - set(WRAPPED) - _SELF_REPORTING),
        sorted((set(WRAPPED) | _SELF_REPORTING) - path_taking),
    )
    for name in WRAPPED:
        assert hasattr(getattr(rustdl, name), "__wrapped__"), name


def test_a_symlink_is_parsed_as_the_query_parsed_it(tmp_path):
    """A content-addressed store (DVC, git-annex, Nix) links `onto.ofn` to a
    target with no extension; the format comes from the name the caller used."""
    target = tmp_path / "ab12cd"
    target.write_text(WITH_KEY)
    link = tmp_path / "onto.ofn"
    link.symlink_to(target)
    _, ws = _record(rustdl.is_consistent, str(link))
    assert len(ws) == 1


def test_warnings_as_errors_raise(tmp_path):
    """The documented behaviour change: under `-W error` a query on an ontology
    with dropped axioms raises."""
    path = _write(tmp_path, "key.ofn", WITH_KEY)
    with warnings.catch_warnings():
        warnings.simplefilter("error", rustdl.DroppedAxiomsWarning)
        with pytest.raises(rustdl.DroppedAxiomsWarning):
            rustdl.is_subclass_of(path, E + "A", E + "B")


def test_the_role_level_flag_goes_to_the_role_level_queries(tmp_path):
    """`top ⊑ r` relates every pair, which the role-level surfaces do not list, but
    it leaves the class-level answers unaffected (#543)."""
    path = _write(
        tmp_path, "topsub.ofn", CLEAN[:-2] + "SubObjectPropertyOf(owl:topObjectProperty :s)\n)\n"
    )
    for name, expected in (
        ("object_property_values", 1),
        ("materialize_inferred_property_assertions", 1),
        ("is_subclass_of", 0),
        ("instances_of", 0),
    ):
        with warnings.catch_warnings(record=True) as rec:
            warnings.simplefilter("always")
            getattr(rustdl, name)(path, *WRAPPED[name])
        got = [w for w in rec if issubclass(w.category, rustdl.UniversalRoleWarning)]
        assert len(got) == expected, (name, [str(w.message) for w in got])
