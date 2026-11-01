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
    # p1_no_bot has ClassAssertion(:Sat :a)
    fixture = fixtures_dir / "abox" / "p1_no_bot.ofn"
    assert rustdl.is_instance_of(str(fixture), "http://t/Sat", "http://t/a") is True


def test_instances_of_simple(fixtures_dir):
    fixture = fixtures_dir / "abox" / "p1_no_bot.ofn"
    instances = rustdl.instances_of(str(fixture), "http://t/Sat")
    assert "http://t/a" in instances


def test_instance_queries_raise_on_an_inconsistent_kb(fixtures_dir):
    # p1_direct_bot asserts a : Unsat with Unsat ⊑ ⊥ (#313: these used to answer).
    fixture = str(fixtures_dir / "abox" / "p1_direct_bot.ofn")
    with pytest.raises(rustdl.RustdlError):
        rustdl.is_instance_of(fixture, "http://t/Unsat", "http://t/a")
    with pytest.raises(rustdl.RustdlError):
        rustdl.instances_of(fixture, "http://t/Unsat")


def test_realize_returns_dict(fixtures_dir):
    fixture = fixtures_dir / "abox" / "p1_no_bot.ofn"
    realization = rustdl.realize(str(fixture))
    assert isinstance(realization, dict)
    assert "http://t/a" in realization
    assert "http://t/Sat" in realization["http://t/a"]


def test_realize_raises_on_an_inconsistent_kb(fixtures_dir):
    # p1_direct_bot asserts a : Unsat with Unsat ⊑ ⊥ (#310: realize used to answer).
    fixture = fixtures_dir / "abox" / "p1_direct_bot.ofn"
    with pytest.raises(rustdl.RustdlError):
        rustdl.realize(str(fixture))


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


def test_different_individuals_disjoint_types_is_complete(tmp_path, recwarn):
    # Same disjoint-types shape as test_different_individuals: the extension
    # probe resolves the (a, b) pair well within the 1s per-pair deadline, so
    # `incomplete` stays False and no IncompleteQueryWarning is raised — the
    # honesty signal is observable by its ABSENCE here.
    p = tmp_path / "o.ofn"
    p.write_text(
        "Prefix(:=<http://ex/#>)\n"
        "Ontology(<http://ex/>\n"
        "  Declaration(Class(:A)) Declaration(Class(:B))\n"
        "  Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n"
        "  DisjointClasses(:A :B)\n"
        "  ClassAssertion(:A :a) ClassAssertion(:B :b))\n"
    )
    rustdl.different_individuals(str(p))
    assert not any(
        issubclass(w.category, rustdl.IncompleteQueryWarning) for w in recwarn.list
    )


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


def test_same_individuals_extension_probe_warns_incomplete(tmp_path):
    # Same fixture as test_same_individuals: :a is not related to :b/:c by the
    # sound-complete seed (SameIndividual + functional-forced merge), so
    # resolving (a, b) / (a, c) requires an extension probe — which always
    # marks the result `incomplete`, per SameIndividuals::incomplete's
    # doc ("true iff ... ANY extension probe ... was consulted").
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
    with pytest.warns(rustdl.IncompleteQueryWarning):
        groups = rustdl.same_individuals(str(p))
    assert any(
        "http://ex/#b" in group and "http://ex/#c" in group for group in groups
    )


def test_object_property_values(tmp_path):
    p = tmp_path / "o.ofn"
    p.write_text(
        "Prefix(:=<http://ex/#>)\n"
        "Ontology(<http://ex/>\n"
        "  Declaration(ObjectProperty(:r))\n"
        "  Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n"
        "  SymmetricObjectProperty(:r)\n"
        "  ObjectPropertyAssertion(:r :a :b))\n"
    )
    triples = rustdl.object_property_values(str(p))
    assert ("http://ex/#b", "http://ex/#r", "http://ex/#a") in triples


def test_ce_satisfiable(tmp_path):
    p = tmp_path / "o.ofn"
    p.write_text(
        "Prefix(:=<http://ex/#>)\n"
        "Ontology(<http://ex/>\n"
        "  Declaration(Class(:A)))\n"
    )
    assert rustdl.class_expression_satisfiable(str(p), ":A and not :A") is False
    assert rustdl.class_expression_satisfiable(str(p), ":A") is True


def test_ce_entailed_subclass(tmp_path):
    p = tmp_path / "o.ofn"
    p.write_text(
        "Prefix(:=<http://ex/#>)\n"
        "Ontology(<http://ex/>\n"
        "  Declaration(Class(:A)) Declaration(Class(:B))\n"
        "  SubClassOf(:A :B))\n"
    )
    assert rustdl.class_expression_entailed_subclass(str(p), ":A", ":B") is True
    assert rustdl.class_expression_entailed_subclass(str(p), ":B", ":A") is False


def test_ce_instances(tmp_path):
    p = tmp_path / "o.ofn"
    p.write_text(
        "Prefix(:=<http://ex/#>)\n"
        "Ontology(<http://ex/>\n"
        "  Declaration(Class(:A)) Declaration(NamedIndividual(:x)) ClassAssertion(:A :x))\n"
    )
    assert "http://ex/#x" in rustdl.class_expression_instances(str(p), ":A")


def test_dropped_axioms(tmp_path):
    # SubClassOf(:A :B) is fully supported; HasKey(:A (:r) ()) is the
    # confirmed live drop (see crates/owl-dl-reasoner/tests/dropped_axioms.rs)
    # — conversion records it instead of aborting, so classify/consistent
    # still succeed (graceful degradation).
    p = tmp_path / "o.ofn"
    p.write_text(
        "Prefix(:=<http://ex/#>)\n"
        "Ontology(<http://ex/>\n"
        "  Declaration(Class(:A)) Declaration(Class(:B))\n"
        "  Declaration(ObjectProperty(:r))\n"
        "  SubClassOf(:A :B)\n"
        "  HasKey(:A (:r) ()))\n"
    )
    dropped = rustdl.dropped_axioms(str(p))
    assert isinstance(dropped, dict)
    assert dropped
    assert any("HasKey" in k for k in dropped)

    # graceful degradation: classify/consistent must not raise despite the drop
    classification = rustdl.classify(str(p))
    assert classification.is_subclass("http://ex/#A", "http://ex/#B")
    assert rustdl.is_consistent(str(p)) is True


def test_dropped_axioms_empty_for_fully_supported_ontology(tmp_path):
    p = tmp_path / "o.ofn"
    p.write_text(
        "Prefix(:=<http://ex/#>)\n"
        "Ontology(<http://ex/>\n"
        "  Declaration(Class(:A)) Declaration(Class(:B))\n"
        "  SubClassOf(:A :B))\n"
    )
    assert rustdl.dropped_axioms(str(p)) == {}


def test_data_property_values(tmp_path):
    p = tmp_path / "o.ofn"
    p.write_text(
        "Prefix(:=<http://ex/#>)\n"
        "Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n"
        "Ontology(<http://ex/>\n"
        "  Declaration(DataProperty(:dp))\n"
        "  Declaration(NamedIndividual(:a))\n"
        "  DataPropertyAssertion(:dp :a \"5\"^^xsd:integer))\n"
    )
    quints = rustdl.data_property_values(str(p))
    # Assert the ARITY, not just a couple of indices: the previous version of
    # this test read q[0]/q[1] only, so it passed at any tuple width and did
    # not notice when the row grew a language-tag column (issue #72).
    assert all(len(q) == 5 for q in quints), quints
    assert any(
        q[0] == "http://ex/#a" and q[1] == "http://ex/#dp" and q[4] == ""
        for q in quints
    ), quints


def test_data_property_values_keeps_language_tags(tmp_path):
    """Issue #72: the tag is part of the literal's identity.

    Two tagged assertions on ONE subject sharing a lexical form must stay TWO
    rows. Dropping `lang` before dedup merged them, losing an assertion.
    """
    p = tmp_path / "lang.ofn"
    p.write_text(
        "Prefix(:=<http://ex/#>)\n"
        "Ontology(<http://ex/>\n"
        "  Declaration(DataProperty(:dp))\n"
        "  Declaration(NamedIndividual(:a))\n"
        '  DataPropertyAssertion(:dp :a "bonjour"@fr)\n'
        '  DataPropertyAssertion(:dp :a "bonjour"@de))\n'
    )
    quints = rustdl.data_property_values(str(p))
    tags = sorted(q[4] for q in quints)
    assert tags == ["de", "fr"], quints


_GAPS_ONTOLOGY = (
    "Prefix(:=<http://ex/#>)\n"
    "Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n"
    "Ontology(<http://ex/>\n"
    "  Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:t))\n"
    "  Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:q))\n"
    "  Declaration(DataProperty(:d)) Declaration(DataProperty(:e))\n"
    "  Declaration(Class(:A)) Declaration(Class(:B))\n"
    "  Declaration(NamedIndividual(:a))\n"
    "  SubObjectPropertyOf(:r :t) FunctionalObjectProperty(:t)\n"
    "  ObjectPropertyDomain(:p :A) ObjectPropertyDomain(:q :B) DisjointClasses(:A :B)\n"
    "  SubDataPropertyOf(:d :e) FunctionalDataProperty(:e)\n"
    '  DataPropertyAssertion(:d :a "1"^^xsd:integer))\n'
)


def test_structural_gap_warning(tmp_path):
    """#389/#390: the structural property queries warn with StructuralGapWarning,
    which is NOT an IncompleteQueryWarning (nothing was cut), and stay silent on
    an ontology without the constructs they ignore."""
    import warnings

    assert not issubclass(rustdl.StructuralGapWarning, rustdl.IncompleteQueryWarning)
    p = tmp_path / "gaps.ofn"
    p.write_text(_GAPS_ONTOLOGY)
    calls = [
        (rustdl.object_property_hierarchy, "object-counting"),
        (rustdl.data_property_hierarchy, "data-counting"),
        (rustdl.disjoint_object_properties, "object-class-constraint"),
        (rustdl.disjoint_data_properties, None),
        (rustdl.data_property_values, "data-counting"),
        (rustdl.materialize_inferred_subobjectproperty_axioms, "object-counting"),
        (rustdl.materialize_inferred_subdataproperty_axioms, "data-counting"),
        (rustdl.materialize_inferred_data_property_assertions, "data-counting"),
    ]
    for fn, kind in calls:
        with warnings.catch_warnings(record=True) as caught:
            warnings.simplefilter("always")
            fn(str(p))
        gap = [w for w in caught if issubclass(w.category, rustdl.StructuralGapWarning)]
        if kind is None:
            assert not gap, fn.__name__
        else:
            assert gap and kind in str(gap[0].message), fn.__name__
        assert not any(
            issubclass(w.category, rustdl.IncompleteQueryWarning) for w in caught
        ), fn.__name__

    plain = tmp_path / "plain.ofn"
    plain.write_text(
        "Prefix(:=<http://ex/#>)\n"
        "Ontology(<http://ex/>\n"
        "  Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))\n"
        "  SubObjectPropertyOf(:r :s))\n"
    )
    with warnings.catch_warnings():
        warnings.simplefilter("error", rustdl.StructuralGapWarning)
        assert rustdl.object_property_hierarchy(str(plain))[1] == [
            ("http://ex/#r", "http://ex/#s")
        ]
