import rustdl


def test_explain_subclass_returns_justifications(fixtures_dir):
    # datatype_definition.ofn entails Adult ⊑ Person (see test_materialize).
    fixture = fixtures_dir / "datatype" / "datatype_definition.ofn"
    js = rustdl.explain(str(fixture), "http://t/Adult", "http://t/Person")
    assert isinstance(js, list)
    assert js, "entailed subsumption should have at least one justification"
    # Each justification is a non-empty list of rendered axiom strings.
    for j in js:
        assert isinstance(j, list) and j
        assert all(isinstance(s, str) for s in j)


def test_explain_not_entailed_is_empty(fixtures_dir):
    # The reverse, Person ⊑ Adult, is NOT entailed.
    fixture = fixtures_dir / "datatype" / "datatype_definition.ofn"
    assert rustdl.explain(str(fixture), "http://t/Person", "http://t/Adult") == []


def test_explain_all(fixtures_dir):
    fixture = fixtures_dir / "datatype" / "datatype_definition.ofn"
    js = rustdl.explain(str(fixture), "http://t/Adult", "http://t/Person", all=True, max=5)
    assert isinstance(js, list) and js
    assert all(isinstance(j, list) for j in js)
