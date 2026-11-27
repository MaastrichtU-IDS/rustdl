"""#543: where rustdl reads `owl:topObjectProperty` as an ordinary property, Python
says so: `UniversalRoleWarning`, `Classification.universal_role_ordinary` (which voids
`completeness_guaranteed`), and `rustdl.universal_role_ordinary(path)`. HermiT: the
positive ontology is inconsistent (`⊤ ⊑ ∃r.D`, `∃top.D ⊑ ⊥`); rustdl's answers are
unchanged (part 2 implements the semantics). The control uses top only as a
super-property, which is inert."""
import warnings

import rustdl

HEAD = """Prefix(:=<http://e#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(
Declaration(Class(:C)) Declaration(Class(:D)) Declaration(ObjectProperty(:r))
Declaration(NamedIndividual(:a))
"""
POSITIVE = HEAD + """SubClassOf(owl:Thing ObjectSomeValuesFrom(:r :D))
SubClassOf(ObjectSomeValuesFrom(owl:topObjectProperty :D) owl:Nothing)
)"""
INERT = HEAD + """SubObjectPropertyOf(:r owl:topObjectProperty)
SubClassOf(:C ObjectSomeValuesFrom(:r :D))
)"""
QUERY_ONLY = HEAD + "ClassAssertion(:D :a)\n)"


def _write(tmp_path, name, src):
    p = tmp_path / name
    p.write_text(src)
    return str(p)


def _universal_warnings(fn, *args):
    with warnings.catch_warnings(record=True) as rec:
        warnings.simplefilter("always")
        result = fn(*args)
    return result, [w for w in rec if issubclass(w.category, rustdl.UniversalRoleWarning)]


def test_inert_use_is_silent(tmp_path):
    path = _write(tmp_path, "inert.ofn", INERT)
    assert rustdl.universal_role_ordinary(path) is False
    c, ws = _universal_warnings(rustdl.classify, path)
    assert not ws
    assert c.universal_role_ordinary is False
    assert c.completeness_guaranteed is True
    _, ws = _universal_warnings(rustdl.is_consistent, path)
    assert not ws


def test_a_semantic_use_warns_and_voids_the_guarantee(tmp_path):
    path = _write(tmp_path, "pos.ofn", POSITIVE)
    assert rustdl.universal_role_ordinary(path) is True
    c, ws = _universal_warnings(rustdl.classify, path)
    assert ws, "classify"
    assert c.universal_role_ordinary is True
    assert c.completeness_guaranteed is False
    consistent, ws = _universal_warnings(rustdl.is_consistent, path)
    assert consistent is True, "unchanged until part 2"
    assert ws, "is_consistent"
    _, ws = _universal_warnings(rustdl.is_subclass_of, path, "http://e#C", "http://e#D")
    assert ws, "is_subclass_of"


def test_a_use_in_the_query_only_is_caught(tmp_path):
    path = _write(tmp_path, "q.ofn", QUERY_ONLY)
    assert rustdl.universal_role_ordinary(path) is False
    r, ws = _universal_warnings(
        # `:C` has no told instance, so the use is not folded (#543 part 2).
        rustdl.class_expression_instances_result, path, "owl:topObjectProperty some :C"
    )
    assert ws
    assert r.universal_role_ordinary is True
    assert r.completeness_guaranteed is False


def test_a_role_above_top_flags_the_role_level_functions(tmp_path):
    src = HEAD + "SubObjectPropertyOf(owl:topObjectProperty :r)\n)"
    path = _write(tmp_path, "topsub.ofn", src)
    assert rustdl.universal_role_ordinary(path) is False
    assert rustdl.universal_role_ordinary(path, role_level=True) is True


def test_is_consistent_is_masked_by_a_proved_inconsistency(tmp_path):
    src = HEAD + """SubClassOf(:C ObjectSomeValuesFrom(owl:topObjectProperty :D))
ClassAssertion(owl:Nothing :a)
)"""
    path = _write(tmp_path, "masked.ofn", src)
    consistent, ws = _universal_warnings(rustdl.is_consistent, path)
    assert consistent is False
    assert not ws, "a proved inconsistency is not hidden by the universal role"


def test_prepared_justify_warns(tmp_path):
    path = _write(tmp_path, "prep.ofn", POSITIVE)
    p = rustdl.prepare(path)
    assert p.universal_role_ordinary is True
    _, ws = _universal_warnings(p.justify, ["subclass", "http://e#C", "http://e#D"])
    assert ws


def test_a_folded_use_answers_right_without_a_warning(tmp_path):
    """#543 part 2: `∃top.D ⊑ E` with `D(a)` is folded: `⊤ ⊑ E` (HermiT), no warning."""
    src = HEAD + """Declaration(Class(:E))
ClassAssertion(:D :a)
SubClassOf(ObjectSomeValuesFrom(owl:topObjectProperty :D) :E)
)"""
    path = _write(tmp_path, "fold.ofn", src)
    assert rustdl.universal_role_ordinary(path) is False
    sub, ws = _universal_warnings(
        rustdl.is_subclass_of, path, "http://www.w3.org/2002/07/owl#Thing", "http://e#E"
    )
    assert sub is True
    assert not ws
