import rustdl
import pytest


def test_classify_returns_classification(fixtures_dir):
    fixture = fixtures_dir / "datatype" / "datatype_definition.ofn"
    result = rustdl.classify(str(fixture))
    assert isinstance(result, rustdl.Classification)
    assert isinstance(result.classes, list)
    assert len(result.classes) > 0


def test_classification_is_subclass(fixtures_dir):
    fixture = fixtures_dir / "datatype" / "datatype_definition.ofn"
    result = rustdl.classify(str(fixture))
    # In this fixture: Adult ⊑ Person via direct SubClassOf axiom
    assert result.is_subclass("http://t/Adult", "http://t/Person")


def test_classify_bytes_ofn(fixtures_dir):
    fixture = fixtures_dir / "datatype" / "datatype_definition.ofn"
    data = fixture.read_bytes()
    result = rustdl.classify_bytes(data, format="ofn")
    assert "http://t/Adult" in result.classes


def test_classify_unknown_extension_raises(tmp_path):
    bad = tmp_path / "ontology.xyz"
    bad.write_text("Ontology()")
    with pytest.raises(rustdl.ParseError):
        rustdl.classify(str(bad))


def test_subclasses_of(fixtures_dir):
    fixture = fixtures_dir / "datatype" / "datatype_definition.ofn"
    result = rustdl.classify(str(fixture))
    # In this fixture: Adult ⊑ Person via direct SubClassOf axiom.
    # subclasses_of(Person) should include Adult (and Person reflexively
    # if the classifier includes the reflexive self-edge).
    subs = result.subclasses_of("http://t/Person")
    assert "http://t/Adult" in subs


def test_superclasses_of(fixtures_dir):
    fixture = fixtures_dir / "datatype" / "datatype_definition.ofn"
    result = rustdl.classify(str(fixture))
    sups = result.superclasses_of("http://t/Adult")
    assert "http://t/Person" in sups
