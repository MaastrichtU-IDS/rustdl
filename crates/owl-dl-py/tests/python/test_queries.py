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
