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


def test_completeness_signal_and_warning(fixtures_dir):
    import warnings
    fixture = fixtures_dir / "datatype" / "datatype_definition.ofn"

    # Default budget on a tiny EL-ish ontology: completes, no warning.
    with warnings.catch_warnings(record=True) as rec:
        warnings.simplefilter("always")
        r = rustdl.classify(str(fixture))
    assert r.complete is True
    assert r.timed_out_pairs == 0
    assert not any(
        issubclass(w.category, rustdl.IncompleteClassificationWarning) for w in rec
    )


def test_saturation_only_is_complete(fixtures_dir):
    fixture = fixtures_dir / "datatype" / "datatype_definition.ofn"
    r = rustdl.classify(str(fixture), saturation_only=True)
    # saturation-only never invokes the tableau, so no pair can time out
    assert r.complete is True
    assert r.timed_out_pairs == 0


def test_unbounded_timeout_accepted(fixtures_dir):
    fixture = fixtures_dir / "datatype" / "datatype_definition.ofn"
    # per_pair_timeout_ms=0 means unbounded — must classify, complete.
    r = rustdl.classify(str(fixture), per_pair_timeout_ms=0)
    assert r.complete is True


def test_wedge_hierarchy_blind_is_its_own_signal(fixtures_dir, monkeypatch):
    # #214: with Layer A off the wedge ignores the role hierarchy. That is
    # reported by `wedge_hierarchy_blind` and warned about, while `complete`
    # keeps meaning "no timeout fired". Fixture: #214's probe t9.
    import warnings
    fixture = str(fixtures_dir / "role_hierarchy_blind.ofn")
    monkeypatch.setenv("RUSTDL_CLASSIFY_ROLE_HIERARCHY", "0")
    with warnings.catch_warnings(record=True) as rec:
        warnings.simplefilter("always")
        r = rustdl.classify(str(fixture), per_pair_timeout_ms=0, global_timeout_ms=0)
    assert r.wedge_hierarchy_blind is True
    assert r.complete is True
    assert sum(
        issubclass(w.category, rustdl.IncompleteClassificationWarning) for w in rec
    ) == 1

    monkeypatch.setenv("RUSTDL_CLASSIFY_ROLE_HIERARCHY", "1")
    r = rustdl.classify(str(fixture), per_pair_timeout_ms=0, global_timeout_ms=0)
    assert r.wedge_hierarchy_blind is False
    assert "http://ex.org/A" in r.unsatisfiable


def test_wedge_clauses_refused_is_its_own_signal(tmp_path, monkeypatch):
    # #353: a clause body the wedge cannot match is reported by
    # `wedge_clauses_refused` and warned about, while `complete` keeps meaning
    # "no timeout fired". With the clausifier's split reverted, the 9-variable
    # body of the sufficient condition below is refused.
    import warnings
    lhs = " ".join(f"ObjectSomeValuesFrom(:s{i} :S{i})" for i in range(1, 9))
    exists = "\n".join(
        f"SubClassOf(:A ObjectSomeValuesFrom(:s{i} :S{i}))" for i in range(1, 9)
    )
    fixture = tmp_path / "wide.ofn"
    fixture.write_text(
        "Prefix(:=<http://ex.org/>)\nOntology(<http://ex.org/o>\n"
        "SubClassOf(:B1 :J) SubClassOf(:B2 :J) DisjointClasses(:B1 :B2)\n"
        "SubClassOf(:A ObjectSomeValuesFrom(:q :B1)) "
        "SubClassOf(:A ObjectSomeValuesFrom(:q :B2))\n"
        f"{exists}\n"
        f"SubClassOf(ObjectIntersectionOf({lhs} ObjectMinCardinality(2 :q :J)) :G)\n)\n"
    )
    monkeypatch.setenv("RUSTDL_WEDGE_BODY_DECOMPOSE", "0")
    with warnings.catch_warnings(record=True) as rec:
        warnings.simplefilter("always")
        r = rustdl.classify(str(fixture), per_pair_timeout_ms=0, global_timeout_ms=0)
    assert r.wedge_clauses_refused == 1
    assert r.complete is True
    assert sum(
        issubclass(w.category, rustdl.IncompleteClassificationWarning) for w in rec
    ) == 1

    monkeypatch.delenv("RUSTDL_WEDGE_BODY_DECOMPOSE")
    r = rustdl.classify(str(fixture), per_pair_timeout_ms=0, global_timeout_ms=0)
    assert r.wedge_clauses_refused == 0
    assert r.is_subclass("http://ex.org/A", "http://ex.org/G")


def test_derived_same_undecided_is_its_own_signal(tmp_path, monkeypatch):
    # #354: a candidate individual equality past the probe cap is reported by
    # `derived_same_undecided` and warned about once, while `complete` keeps
    # meaning "no timeout fired". With the cap lifted, the equality is used.
    import warnings
    fixture = tmp_path / "eq.ofn"
    fixture.write_text(
        "Prefix(:=<http://ex.org/>)\nOntology(<http://ex.org/o>\n"
        "EquivalentClasses(:C1 ObjectHasValue(:p :a))\n"
        "EquivalentClasses(:C2 ObjectHasValue(:p :b))\n"
        "ClassAssertion(:W :x) SubClassOf(:W ObjectMaxCardinality(1 :f))\n"
        "ObjectPropertyAssertion(:f :x :a) ObjectPropertyAssertion(:f :x :b)\n)\n"
    )
    monkeypatch.setenv("RUSTDL_CLASSIFY_DERIVED_SAME_MAX", "0")
    with warnings.catch_warnings(record=True) as rec:
        warnings.simplefilter("always")
        r = rustdl.classify(str(fixture), per_pair_timeout_ms=0, global_timeout_ms=0)
    assert r.derived_same_undecided == 1
    assert r.complete is True
    assert sum(
        issubclass(w.category, rustdl.IncompleteClassificationWarning) for w in rec
    ) == 1

    monkeypatch.delenv("RUSTDL_CLASSIFY_DERIVED_SAME_MAX")
    r = rustdl.classify(str(fixture), per_pair_timeout_ms=0, global_timeout_ms=0)
    assert r.derived_same_undecided == 0
    assert r.is_subclass("http://ex.org/C1", "http://ex.org/C2")


def test_prep_timed_out_is_its_own_signal(tmp_path, monkeypatch):
    # #162: a preparation cut by the global deadline is reported by
    # `prep_timed_out` and warned about once as a cut preparation, not as a
    # timed-out class pair. The ladder is `hard_global_deadline.rs`'s: every
    # `A_i ⊑ C_i` is derived, so a saturation stopped at once reports nothing.
    import warnings
    lines = ["Prefix(:=<http://ex#>)", "Ontology("]
    for i in range(3000):
        lines.append(f"SubClassOf(:A{i} ObjectSomeValuesFrom(:r :B{i}))")
        lines.append(f"SubClassOf(ObjectSomeValuesFrom(:r :B{i}) :C{i})")
    lines.append(")")
    p = tmp_path / "ladder.ofn"
    p.write_text("\n".join(lines))
    monkeypatch.setenv("RUSTDL_PREP_DEADLINE", "1")

    def incomplete_warnings(rec):
        return [
            str(w.message)
            for w in rec
            if issubclass(w.category, rustdl.IncompleteClassificationWarning)
        ]

    # Conversion is let finish, so the cut lands in preparation.
    monkeypatch.setenv("RUSTDL_HARD_GLOBAL_DEADLINE", "1")
    monkeypatch.setenv("RUSTDL_CONVERT_DEADLINE", "0")
    with warnings.catch_warnings(record=True) as rec:
        warnings.simplefilter("always")
        r = rustdl.classify(str(p), global_timeout_ms=1)
    assert r.prep_timed_out is True
    assert r.conversion_timed_out is False
    assert r.complete is False
    assert r.consistency_undetermined is True
    msgs = incomplete_warnings(rec)
    assert len(msgs) == 1 and "preparation" in msgs[0], msgs

    # With the conversion deadline on, conversion is cut too. In Python the
    # budget starts after loading, so parsing does not spend it; the component
    # sort and the first 1,024-component batch do, and every `∃r.B_i ⊑ C_i`
    # sorts after the 3,000 `A_i` axioms, so none is read. One warning, naming
    # conversion.
    monkeypatch.setenv("RUSTDL_CONVERT_DEADLINE", "1")
    with warnings.catch_warnings(record=True) as rec:
        warnings.simplefilter("always")
        r = rustdl.classify(str(p), global_timeout_ms=1)
    assert r.conversion_timed_out is True
    assert r.prep_timed_out is True
    assert r.complete is False
    assert not r.is_subclass("http://ex#A0", "http://ex#C0")
    msgs = incomplete_warnings(rec)
    assert len(msgs) == 1 and "conversion" in msgs[0], msgs

    # No budget: nothing can be cut. (Not "1 ms, hard mode off": whether that
    # bounds saturation depends on host speed, see the #230 review.)
    monkeypatch.delenv("RUSTDL_HARD_GLOBAL_DEADLINE")
    r = rustdl.classify(str(p), global_timeout_ms=0)
    assert r.prep_timed_out is False
    assert r.conversion_timed_out is False
    assert r.is_subclass("http://ex#A0", "http://ex#C0")


# The completeness guarantee (docs/assurance/completeness-guarantee-ledger.md).
# Inline ontologies, unbounded budgets so a loaded host cannot flip them.
_EL = b"""Prefix(:=<http://e#>)
Ontology(
Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))
SubClassOf(:A :B) SubClassOf(:B :C)
)"""

_OUT_OF_FRAGMENT = b"""Prefix(:=<http://e#>)
Ontology(
Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:D))
EquivalentClasses(:D ObjectIntersectionOf(:A ObjectComplementOf(:B)))
SubClassOf(:A :B)
)"""

# Pure EL once the unsupported DataAllValuesFrom(xsd:hexBinary) is dropped; the
# input entails A ⊑ E, which classify misses (#246).
_DROPPED = b"""Prefix(:=<http://e#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(
Declaration(DataProperty(:dp)) Declaration(ObjectProperty(:r))
Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:E))
SubClassOf(:A ObjectSomeValuesFrom(:r ObjectIntersectionOf(:B DataAllValuesFrom(:dp xsd:hexBinary))))
SubClassOf(ObjectSomeValuesFrom(:r :B) :E)
)"""


def _classify_unbounded(data):
    import warnings
    with warnings.catch_warnings():
        warnings.simplefilter("ignore")
        return rustdl.classify_bytes(
            data, format="ofn", per_pair_timeout_ms=0, global_timeout_ms=0
        )


def test_completeness_guaranteed_on_pure_el():
    r = _classify_unbounded(_EL)
    assert r.complete is True
    assert r.completeness_guaranteed is True


def test_complete_is_weaker_than_completeness_guaranteed():
    # Nothing timed out, so `complete` is True, but the ontology is out of the
    # fragment where completeness is provable.
    r = _classify_unbounded(_OUT_OF_FRAGMENT)
    assert r.complete is True
    assert r.completeness_guaranteed is False


def test_dropped_axiom_voids_the_guarantee():
    r = _classify_unbounded(_DROPPED)
    assert r.complete is True
    assert not r.is_subclass("http://e#A", "http://e#E")
    assert r.completeness_guaranteed is False
