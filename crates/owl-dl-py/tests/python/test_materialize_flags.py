"""#391: the materialize functions built on `realize` and `classify` warn when
their answer may be short, as `realize` and `classify` do.

`RUSTDL_MAX_NODES` is cached once per process, so the capped cases run in a
fresh interpreter. The pseudo-model prune would answer the probes without
building them, so those runs turn it off (as `test_query_give_up.py` does).
"""
import json
import os
import subprocess
import sys
import textwrap
import warnings

import rustdl

# #395's fixture: each class generates an `:r`-successor, so a probe trips a
# node cap of 1; `∀s.E` keeps it off the saturation fast path; `C ⊑ ∃r.C` is
# the told existential the successor rows join on.
SRC = """Prefix(:=<http://ex/#>)
Ontology(<http://ex/querygiveup>
  Declaration(Class(:C)) Declaration(Class(:D)) Declaration(Class(:E))
  Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))
  Declaration(NamedIndividual(:a))
  SubClassOf(:C ObjectSomeValuesFrom(:r :C))
  SubClassOf(:D ObjectSomeValuesFrom(:r :D))
  SubClassOf(:C ObjectAllValuesFrom(:s :E))
  ClassAssertion(:C :a)
)
"""

REALIZE_BACKED = ("materialize_inferred_class_assertions", "materialize_existential_successors")


def _run(path: str, cap: str) -> dict:
    script = textwrap.dedent(
        f"""
        import json, warnings, rustdl
        out = {{}}
        for name in {REALIZE_BACKED!r}:
            with warnings.catch_warnings(record=True) as w:
                warnings.simplefilter("always")
                value = getattr(rustdl, name)({path!r})
            out[name] = {{
                "value": value,
                "warned": any(issubclass(x.category, rustdl.IncompleteQueryWarning) for x in w),
            }}
        print(json.dumps(out))
        """
    )
    env = dict(os.environ, RUSTDL_MAX_NODES=cap, RUSTDL_PSEUDO_MODEL="0")
    res = subprocess.run(
        [sys.executable, "-c", script], env=env, capture_output=True, text=True, check=True
    )
    return json.loads(res.stdout.strip().splitlines()[-1])


def test_a_cut_probe_warns_and_the_rows_stay(tmp_path):
    p = tmp_path / "o.ofn"
    p.write_text(SRC)
    capped, control = _run(str(p), "1"), _run(str(p), "0")
    for name in REALIZE_BACKED:
        assert capped[name]["warned"], f"{name} must warn on a cut probe"
        assert not control[name]["warned"], f"{name} warned without a cut"
        # Only a signal: the rows are those of the uncapped run.
        assert capped[name]["value"] == control[name]["value"], name
    assert control["materialize_existential_successors"]["value"] == [
        ["http://ex/#a", "http://ex/#r", "_:b0", "http://ex/#C"]
    ]


# `a : X, a : Y` off the EL fragment, plus a told `X ⊑ ∃r.D` so the successor
# function actually runs `realize` (without it, no row can exist and it returns
# early, which would make the negative check below vacuous).
TWO_TYPES = """Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/o>
  Declaration(Class(:X)) Declaration(Class(:Y)) Declaration(Class(:C))
  Declaration(Class(:D)) Declaration(ObjectProperty(:r))
  Declaration(NamedIndividual(:a))
  SubClassOf(:C ObjectAllValuesFrom(:r :D))
  SubClassOf(:X ObjectSomeValuesFrom(:r :D))
  ClassAssertion(:X :a)
  ClassAssertion(:Y :a)
)
"""


def _warnings(fn, path):
    with warnings.catch_warnings(record=True) as rec:
        warnings.simplefilter("always")
        value = fn(path)
    return value, [w.category for w in rec]


def test_class_assertions_warn_direct_types_unverified(tmp_path, monkeypatch):
    p = tmp_path / "o.ofn"
    p.write_text(TWO_TYPES)
    monkeypatch.setenv("RUSTDL_REALIZE_VERIFY_DIRECT", "0")
    rows, cats = _warnings(rustdl.materialize_inferred_class_assertions, str(p))
    assert sorted(rows) == [("http://ex.org/X", "http://ex.org/a"), ("http://ex.org/Y", "http://ex.org/a")]
    assert rustdl.DirectTypesUnverifiedWarning in cats
    # The successor rows use entailed types, which that flag does not touch. The
    # row proves realize ran; the warning check pins that the flag stays off this
    # surface if someone later wires it in.
    rows, cats = _warnings(rustdl.materialize_existential_successors, str(p))
    assert rows == [("http://ex.org/a", "http://ex.org/r", "_:b0", "http://ex.org/D")]
    assert rustdl.DirectTypesUnverifiedWarning not in cats
    # With the check on, the pair is decided and nothing is said.
    monkeypatch.setenv("RUSTDL_REALIZE_VERIFY_DIRECT", "1")
    _, cats = _warnings(rustdl.materialize_inferred_class_assertions, str(p))
    assert rustdl.DirectTypesUnverifiedWarning not in cats


def test_subclass_axioms_warn_as_classify(fixtures_dir, monkeypatch):
    # #214's fixture: with Layer A off, `classify` warns `wedge_hierarchy_blind`.
    fixture = str(fixtures_dir / "role_hierarchy_blind.ofn")
    monkeypatch.setenv("RUSTDL_CLASSIFY_ROLE_HIERARCHY", "0")
    _, cats = _warnings(rustdl.materialize_inferred_subclass_axioms, fixture)
    # Exactly one: wedge_hierarchy_blind. A second one on a red run means a cut
    # under host load (same exposure as test_query_give_up.py), not this wiring.
    assert cats.count(rustdl.IncompleteClassificationWarning) == 1
    monkeypatch.setenv("RUSTDL_CLASSIFY_ROLE_HIERARCHY", "1")
    _, cats = _warnings(rustdl.materialize_inferred_subclass_axioms, fixture)
    assert rustdl.IncompleteClassificationWarning not in cats


# #387: a Horn ABox, and the same with a disjunction the witness must choose.
# `E ⊑ ∃s.H` is the told existential the successor rows join on.
WITNESS_BASE = """Prefix(:=<http://ex/#>)
Ontology(<http://ex/wp>
  Declaration(Class(:D)) Declaration(Class(:E)) Declaration(Class(:F))
  Declaration(Class(:G)) Declaration(Class(:H))
  Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))
  Declaration(NamedIndividual(:a))
  SubClassOf(:D :E) SubClassOf(:G ObjectAllValuesFrom(:r :F))
  SubClassOf(:E ObjectSomeValuesFrom(:s :H))
  %s
  ClassAssertion(:D :a)
)
"""
WITNESS_SURFACES = (
    "realize",
    "materialize_inferred_class_assertions",
    "materialize_existential_successors",
)


def test_a_branched_witness_warns_witness_prune(tmp_path, monkeypatch):
    horn, union = tmp_path / "horn.ofn", tmp_path / "union.ofn"
    horn.write_text(WITNESS_BASE % "")
    union.write_text(WITNESS_BASE % "SubClassOf(:D ObjectUnionOf(:F :G))")
    # Keep the disjunction a search decision rather than a lookahead deduction.
    monkeypatch.setenv("RUSTDL_SEMANTIC_BRANCHING", "0")
    monkeypatch.setenv("RUSTDL_SAT_LOOKAHEAD", "0")
    for name in WITNESS_SURFACES:
        fn = getattr(rustdl, name)
        value, cats = _warnings(fn, str(union))
        assert rustdl.WitnessPruneWarning in cats, name
        # Its own warning: nothing was cut.
        assert rustdl.IncompleteQueryWarning not in cats, name
        _, cats = _warnings(fn, str(horn))
        assert rustdl.WitnessPruneWarning not in cats, f"{name} warned on a Horn witness"
        # Only a signal: the answer is that of the unpruned run, which warns nothing.
        monkeypatch.setenv("RUSTDL_PSEUDO_MODEL", "0")
        off, cats = _warnings(fn, str(union))
        monkeypatch.delenv("RUSTDL_PSEUDO_MODEL")
        assert rustdl.WitnessPruneWarning not in cats, name
        assert value == off, name
    rows, _ = _warnings(rustdl.materialize_existential_successors, str(union))
    assert rows == [("http://ex/#a", "http://ex/#s", "_:b0", "http://ex/#H")]
