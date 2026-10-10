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

    monkeypatch.setenv("RUSTDL_HARD_GLOBAL_DEADLINE", "1")
    with warnings.catch_warnings(record=True) as rec:
        warnings.simplefilter("always")
        r = rustdl.classify(str(p), global_timeout_ms=1)
    assert r.prep_timed_out is True
    assert r.complete is False
    assert r.consistency_undetermined is True
    msgs = [
        str(w.message)
        for w in rec
        if issubclass(w.category, rustdl.IncompleteClassificationWarning)
    ]
    assert len(msgs) == 1 and "preparation" in msgs[0], msgs

    # No budget: nothing can be cut. (Not "1 ms, hard mode off": whether that
    # bounds saturation depends on host speed, see the #230 review.)
    monkeypatch.delenv("RUSTDL_HARD_GLOBAL_DEADLINE")
    r = rustdl.classify(str(p), global_timeout_ms=0)
    assert r.prep_timed_out is False
    assert r.is_subclass("http://ex#A0", "http://ex#C0")
