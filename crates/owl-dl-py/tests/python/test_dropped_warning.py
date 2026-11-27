"""#597: every Python query warns `DroppedAxiomsWarning` when conversion dropped
axioms (before, only the class-expression queries did, #552). `HasKey` is the
confirmed live drop (crates/owl-dl-reasoner/tests/dropped_axioms.rs)."""
import os
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


def test_the_cache_converts_an_unchanged_file_once(tmp_path, monkeypatch):
    path = _write(tmp_path, "key.ofn", WITH_KEY)
    calls = []
    native = rustdl._conversion_notes_native

    def counting(p):
        calls.append(p)
        return native(p)

    monkeypatch.setattr(rustdl, "_conversion_notes_native", counting)
    rustdl._conversion_notes_keyed.cache_clear()
    for _ in range(3):
        _, ws = _record(rustdl.is_consistent, path)
        assert len(ws) == 1
    # A relative path to the same file is the same entry.
    monkeypatch.chdir(tmp_path)
    _, ws = _record(rustdl.is_consistent, "key.ofn")
    assert len(ws) == 1
    assert len(calls) == 1, calls


def test_a_same_size_rewrite_with_the_same_mtime_is_reread(tmp_path):
    """The worst case for a stat key: same path, inode, size and mtime (a coarse
    mtime, or `cp -p` / `os.utime`). Only ctime tells the files apart."""
    key = "HasKey(:A (:r) ())"
    clean = WITH_KEY.replace(key, "#" + "x" * (len(key) - 1))
    assert len(clean.encode()) == len(WITH_KEY.encode())
    path = _write(tmp_path, "rw.ofn", WITH_KEY)
    rustdl._conversion_notes_keyed.cache_clear()
    _, ws = _record(rustdl.is_consistent, path)
    assert len(ws) == 1
    st = os.stat(path)
    with open(path, "w") as f:
        f.write(clean)
    os.utime(path, ns=(st.st_atime_ns, st.st_mtime_ns))
    assert os.stat(path).st_mtime_ns == st.st_mtime_ns
    assert os.stat(path).st_size == st.st_size
    _, ws = _record(rustdl.is_consistent, path)
    assert not ws, "stale notes served from the cache"


def test_debug_warns_through_its_queries(tmp_path):
    path = _write(tmp_path, "key.ofn", WITH_KEY.replace("HasKey", "SubClassOf(:C owl:Nothing)\nHasKey"))
    _, ws = _record(rustdl.debug, path)
    names = {str(w.message).split(":", 1)[0] for w in ws}
    assert "diagnose" in names and names <= {"diagnose", "justify", "repair"}, names
