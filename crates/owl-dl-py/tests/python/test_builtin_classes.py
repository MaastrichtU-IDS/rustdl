"""#427: owl:Thing / owl:Nothing are the built-in ⊤ / ⊥ on the named-class
queries, declared or not. On main they raised UnknownClassError when undeclared
and answered wrongly when declared (is_subclass_of(A, owl:Thing) was False)."""

import pytest

import rustdl

THING = "http://www.w3.org/2002/07/owl#Thing"
NOTHING = "http://www.w3.org/2002/07/owl#Nothing"

BODY = """Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:U))
  Declaration(NamedIndividual(:a))
  SubClassOf(:A :B) SubClassOf(:U owl:Nothing) ClassAssertion(:A :a)"""


def _write(tmp_path, declared):
    decls = (
        "Declaration(Class(owl:Thing)) Declaration(Class(owl:Nothing))"
        if declared
        else ""
    )
    path = tmp_path / f"builtin-{declared}.ofn"
    path.write_text(
        "Prefix(:=<http://ex/#>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n"
        f"Ontology(<http://ex/o>\n{decls}\n{BODY}\n)\n"
    )
    return str(path)


@pytest.mark.parametrize("declared", [False, True])
def test_queries_resolve_the_built_ins(tmp_path, declared):
    path = _write(tmp_path, declared)
    assert rustdl.is_class_satisfiable(path, THING) is True
    assert rustdl.is_class_satisfiable(path, NOTHING) is False
    assert rustdl.is_subclass_of(path, "http://ex/#A", THING) is True
    assert rustdl.is_subclass_of(path, NOTHING, "http://ex/#A") is True
    assert rustdl.is_subclass_of(path, "http://ex/#U", NOTHING) is True
    assert rustdl.is_subclass_of(path, THING, NOTHING) is False
    assert rustdl.is_instance_of(path, THING, "http://ex/#a") is True
    assert rustdl.instances_of(path, THING) == ["http://ex/#a"]


@pytest.mark.parametrize("declared", [False, True])
def test_classification_resolves_the_built_ins(tmp_path, declared):
    c = rustdl.classify(_write(tmp_path, declared))
    assert THING not in c.classes and NOTHING not in c.classes
    assert c.is_subclass("http://ex/#A", THING) is True
    assert c.is_subclass("http://ex/#U", NOTHING) is True


def test_a_typo_beside_a_built_in_still_raises(tmp_path):
    with pytest.raises(rustdl.UnknownClassError):
        rustdl.is_subclass_of(_write(tmp_path, True), "http://ex/#Aa", THING)
