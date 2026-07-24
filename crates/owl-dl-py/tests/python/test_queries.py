import rustdl
import pytest


def test_is_consistent_true(fixtures_dir):
    fixture = fixtures_dir / "datatype" / "datatype_definition.ofn"
    assert rustdl.is_consistent(str(fixture)) is True


def test_is_class_satisfiable_true(fixtures_dir):
    fixture = fixtures_dir / "datatype" / "datatype_definition.ofn"
    assert rustdl.is_class_satisfiable(str(fixture), "http://t/Person") is True


def test_is_subclass_of_direct(fixtures_dir):
    fixture = fixtures_dir / "datatype" / "datatype_definition.ofn"
    assert rustdl.is_subclass_of(str(fixture), "http://t/Adult", "http://t/Person") is True


def test_is_class_satisfiable_unknown_class_raises(fixtures_dir):
    fixture = fixtures_dir / "datatype" / "datatype_definition.ofn"
    with pytest.raises(rustdl.UnknownClassError):
        rustdl.is_class_satisfiable(str(fixture), "http://t/NonExistent")


def test_is_instance_of_simple(fixtures_dir):
    # p1_direct_bot has ClassAssertion(:Unsat :a)
    fixture = fixtures_dir / "abox" / "p1_direct_bot.ofn"
    assert rustdl.is_instance_of(str(fixture), "http://t/Unsat", "http://t/a") is True


def test_instances_of_simple(fixtures_dir):
    fixture = fixtures_dir / "abox" / "p1_direct_bot.ofn"
    instances = rustdl.instances_of(str(fixture), "http://t/Unsat")
    assert "http://t/a" in instances


def test_realize_returns_dict(fixtures_dir):
    fixture = fixtures_dir / "abox" / "p1_direct_bot.ofn"
    realization = rustdl.realize(str(fixture))
    assert isinstance(realization, dict)
    assert "http://t/a" in realization
    assert isinstance(realization["http://t/a"], list)


def test_disjoint_classes(tmp_path):
    p = tmp_path / "o.ofn"
    p.write_text(
        "Prefix(:=<http://ex/#>)\n"
        "Ontology(<http://ex/>\n"
        "  Declaration(Class(:A)) Declaration(Class(:B))\n"
        "  DisjointClasses(:A :B))\n"
    )
    pairs = rustdl.disjoint_classes(str(p))
    assert ("http://ex/#A", "http://ex/#B") in pairs or ("http://ex/#B", "http://ex/#A") in pairs


def test_object_property_hierarchy_direct_subsumption(tmp_path):
    p = tmp_path / "o.ofn"
    p.write_text(
        "Prefix(:=<http://ex/#>)\n"
        "Ontology(<http://ex/>\n"
        "  Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))\n"
        "  SubObjectPropertyOf(:r :s))\n"
    )
    _equivalent_groups, direct_subsumptions = rustdl.object_property_hierarchy(str(p))
    assert ("http://ex/#r", "http://ex/#s") in direct_subsumptions


def test_different_individuals(tmp_path):
    p = tmp_path / "o.ofn"
    p.write_text(
        "Prefix(:=<http://ex/#>)\n"
        "Ontology(<http://ex/>\n"
        "  Declaration(Class(:A)) Declaration(Class(:B))\n"
        "  Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n"
        "  DisjointClasses(:A :B)\n"
        "  ClassAssertion(:A :a) ClassAssertion(:B :b))\n"
    )
    pairs = rustdl.different_individuals(str(p))
    assert ("http://ex/#a", "http://ex/#b") in pairs or ("http://ex/#b", "http://ex/#a") in pairs


def test_same_individuals(tmp_path):
    p = tmp_path / "o.ofn"
    p.write_text(
        "Prefix(:=<http://ex/#>)\n"
        "Ontology(<http://ex/>\n"
        "  Declaration(ObjectProperty(:r))\n"
        "  Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n"
        "  Declaration(NamedIndividual(:c))\n"
        "  FunctionalObjectProperty(:r)\n"
        "  ObjectPropertyAssertion(:r :a :b) ObjectPropertyAssertion(:r :a :c))\n"
    )
    groups = rustdl.same_individuals(str(p))
    assert any(
        "http://ex/#b" in group and "http://ex/#c" in group for group in groups
    )
