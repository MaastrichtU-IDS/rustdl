import rustdl


def test_materialize_inferred_subclass_axioms(fixtures_dir):
    fixture = fixtures_dir / "datatype" / "datatype_definition.ofn"
    axioms = rustdl.materialize_inferred_subclass_axioms(str(fixture))
    assert isinstance(axioms, list)
    assert all(isinstance(a, tuple) and len(a) == 2 for a in axioms)
    # Adult ⊑ Person should appear
    assert ("http://t/Adult", "http://t/Person") in axioms


def test_materialize_inferred_class_assertions(fixtures_dir):
    # p1_no_bot.ofn: ClassAssertion(:Sat :a) with :Sat satisfiable.
    # p1_direct_bot.ofn was not used because :Unsat ⊑ owl:Nothing causes
    # realize() to return empty most_specific_types for :a (correct behavior).
    fixture = fixtures_dir / "abox" / "p1_no_bot.ofn"
    axioms = rustdl.materialize_inferred_class_assertions(str(fixture))
    assert isinstance(axioms, list)
    assert all(isinstance(a, tuple) and len(a) == 2 for a in axioms)
    # (class, individual) for ClassAssertion(:Sat :a)
    assert ("http://t/Sat", "http://t/a") in axioms
