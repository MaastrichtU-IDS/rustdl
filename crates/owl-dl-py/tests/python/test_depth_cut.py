"""#510: every Python call whose instance probe can be cut by the tableau's
decision-depth limit says so in its `IncompleteQueryWarning`, with the advice that
helps (`RUSTDL_REALIZE_PAIR_TIMEOUT_MS=0`) instead of a larger budget, which does not.

The fixture is `X ⊑ ⊓_{k<280} (∀r_k.A ⊔ ∀s_k.A)`, `a : X`. The probe `{a} ⊓ ¬Y` needs
280 decision levels, past the bounded probe's cap of 256, and early abandon gives up
after 32 cap hits, long before a 60 s deadline. The per-pair deadline and the
early-abandon switch are read per call, so the arms run in one interpreter.
"""
import warnings

import pytest

import rustdl

NS = "http://example.org/d#"
A, X, Y = NS + "a", NS + "X", NS + "Y"
DEPTH = "decision-depth limit"
ADVICE = "RUSTDL_REALIZE_PAIR_TIMEOUT_MS=0"


def _fixture(tmp_path, name, *, told_exists=False, a_is_y=False):
    unions = "".join(
        f" ObjectUnionOf(ObjectAllValuesFrom(:r{k} :A) ObjectAllValuesFrom(:s{k} :A))"
        for k in range(280)
    )
    extra = ""
    if told_exists:
        # A told existential, so `materialize_existential_successors` runs its realize.
        extra += "Declaration(Class(:Z)) SubClassOf(:Z ObjectSomeValuesFrom(:p :A))\n"
    if a_is_y:
        extra += "ClassAssertion(:Y :a)\n"
    src = (
        "Prefix(:=<http://example.org/d#>)\nOntology(<http://example.org/d>\n"
        "Declaration(NamedIndividual(:a)) Declaration(Class(:Y)) Declaration(Class(:A))\n"
        f"SubClassOf(:X ObjectIntersectionOf({unions}))\n{extra}ClassAssertion(:X :a)\n)\n"
    )
    p = tmp_path / f"{name}.ofn"
    p.write_text(src)
    return str(p)


# label -> (call(paths) , kind). `kind`: "answer" (one probe, yes/no), "list" (OR-ed
# probes), "realize" (a list that runs realize), "explain" (justify / repair).
CALLS = {
    "is_instance_of": (lambda p: rustdl.is_instance_of(p["plain"], Y, A), "answer"),
    "instances_of": (lambda p: rustdl.instances_of(p["plain"], Y), "list"),
    "class_expression_instances": (
        lambda p: rustdl.class_expression_instances(p["plain"], ":Y"),
        "list",
    ),
    "realize": (lambda p: rustdl.realize(p["plain"]), "realize"),
    "materialize_inferred_class_assertions": (
        lambda p: rustdl.materialize_inferred_class_assertions(p["plain"]),
        "realize",
    ),
    "materialize_existential_successors": (
        lambda p: rustdl.materialize_existential_successors(p["exists"]),
        "realize",
    ),
    "justify": (lambda p: rustdl.justify(p["plain"], ["instance", A, Y]), "explain"),
    "justify_all": (lambda p: rustdl.justify_all(p["plain"], ["instance", A, Y]), "explain"),
    "repair": (lambda p: rustdl.repair(p["plain"], ["instance", A, Y]), "explain"),
    "repair_search": (lambda p: rustdl.repair(p["a_is_y"], ["instance", A, Y]), "explain"),
    "prepared_justify": (
        lambda p: rustdl.prepare(p["plain"]).justify(["instance", A, Y]),
        "explain",
    ),
    "prepared_justify_all": (
        lambda p: rustdl.prepare(p["plain"]).justify_all(["instance", A, Y]),
        "explain",
    ),
}


@pytest.fixture
def paths(tmp_path):
    return {
        "plain": _fixture(tmp_path, "plain"),
        "exists": _fixture(tmp_path, "exists", told_exists=True),
        "a_is_y": _fixture(tmp_path, "a_is_y", a_is_y=True),
    }


def _incomplete_warnings(call, paths):
    with warnings.catch_warnings(record=True) as w:
        warnings.simplefilter("always")
        value = call(paths)
    return value, [
        str(x.message) for x in w if issubclass(x.category, rustdl.IncompleteQueryWarning)
    ]


@pytest.mark.parametrize("label", sorted(CALLS))
def test_a_depth_cut_is_named_with_the_advice_that_helps(label, paths, monkeypatch):
    call, kind = CALLS[label]
    monkeypatch.setenv("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", "60000")
    _, msgs = _incomplete_warnings(call, paths)
    assert len(msgs) == 1, msgs
    msg = msgs[0]
    assert DEPTH in msg and ADVICE in msg and "does not help" in msg, msg
    if kind == "answer":
        # One probe: the depth limit was the cut, no budget sentence.
        assert "the probe hit" in msg and "budget" not in msg, msg
    elif kind in ("list", "realize"):
        # OR-ed over the probes: at least one hit the limit; others may be budget cuts.
        assert "at least one probe" in msg and "budget" in msg, msg
        assert ("classification step" in msg) == (kind == "realize"), msg
    else:
        assert "at least one instance check" in msg, msg


@pytest.mark.parametrize("label", sorted(CALLS))
def test_the_deadline_free_probe_answers_without_a_warning(label, paths, monkeypatch):
    call, _ = CALLS[label]
    monkeypatch.setenv("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", "0")
    _, msgs = _incomplete_warnings(call, paths)
    assert msgs == [], msgs


@pytest.mark.parametrize("label", sorted(CALLS))
def test_a_deadline_cut_keeps_the_budget_text(label, paths, monkeypatch):
    # Early abandon off and a 1 ms deadline: every cut is a deadline cut.
    call, _ = CALLS[label]
    monkeypatch.setenv("RUSTDL_TABLEAU_EARLY_ABANDON", "0")
    monkeypatch.setenv("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", "1")
    _, msgs = _incomplete_warnings(call, paths)
    assert len(msgs) == 1, msgs
    assert DEPTH not in msgs[0], msgs
