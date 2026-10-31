"""#363 A2: a query that gives up must warn instead of looking definite.

`RUSTDL_MAX_NODES` is cached once per process, so each case runs in a fresh
interpreter with the cap set in its environment.
"""
import json
import os
import subprocess
import sys
import textwrap

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

CALLS = {
    "is_class_satisfiable": ("http://ex/#C",),
    "is_subclass_of": ("http://ex/#C", "http://ex/#D"),
    "is_instance_of": ("http://ex/#D", "http://ex/#a"),
    "instances_of": ("http://ex/#D",),
    "realize": (),
}


def _run(path: str, cap: str) -> dict:
    script = textwrap.dedent(
        f"""
        import json, warnings, rustdl
        out = {{}}
        for name, args in {CALLS!r}.items():
            with warnings.catch_warnings(record=True) as w:
                warnings.simplefilter("always")
                value = getattr(rustdl, name)({path!r}, *args)
            out[name] = {{
                "value": value if not isinstance(value, dict) else sorted(value),
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


def test_a_node_cap_give_up_warns(tmp_path):
    p = tmp_path / "o.ofn"
    p.write_text(SRC)
    capped = _run(str(p), "1")
    for name in CALLS:
        assert capped[name]["warned"], f"{name} must warn on a give-up: {capped[name]}"
    # Answers stay in their sound direction.
    assert capped["is_class_satisfiable"]["value"] is True
    assert capped["is_subclass_of"]["value"] is False
    assert capped["is_instance_of"]["value"] is False


def test_no_warning_without_a_give_up(tmp_path):
    p = tmp_path / "o.ofn"
    p.write_text(SRC)
    control = _run(str(p), "0")
    for name in CALLS:
        assert not control[name]["warned"], f"{name} warned without a give-up: {control[name]}"
    assert control["is_class_satisfiable"]["value"] is True
    assert control["is_subclass_of"]["value"] is False
    assert control["is_instance_of"]["value"] is False


SRC_344 = """Prefix(:=<http://ex.org/>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/o>
  Declaration(Class(:P)) Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
  SubClassOf(:P DataMaxCardinality(1 :p DataOneOf("1"^^xsd:integer "2"^^xsd:integer)))
  ClassAssertion(:P :a)
  DataPropertyAssertion(:p :a "1"^^xsd:integer)
  DataPropertyAssertion(:p :a "2"^^xsd:integer)
)
"""


def _is_consistent(path: str, verify_ms: str | None) -> dict:
    script = textwrap.dedent(
        f"""
        import json, warnings, rustdl
        with warnings.catch_warnings(record=True) as w:
            warnings.simplefilter("always")
            value = rustdl.is_consistent({path!r})
        print(json.dumps({{
            "value": value,
            "warned": any(issubclass(x.category, rustdl.IncompleteQueryWarning) for x in w),
        }}))
        """
    )
    env = dict(os.environ)
    env.pop("RUSTDL_CONSISTENCY_DATA_COUNTING_VERIFY_MS", None)
    if verify_ms is not None:
        env["RUSTDL_CONSISTENCY_DATA_COUNTING_VERIFY_MS"] = verify_ms
    res = subprocess.run(
        [sys.executable, "-c", script], env=env, capture_output=True, text=True, check=True
    )
    return json.loads(res.stdout.strip().splitlines()[-1])


def test_is_consistent_warns_on_a_data_counting_verification_give_up(tmp_path):
    """#344: with a zero verification budget the check gives up; `is_consistent`
    keeps the sound `True` and warns. With the default budget it decides `False`."""
    p = tmp_path / "o.ofn"
    p.write_text(SRC_344)
    gave_up = _is_consistent(str(p), "0")
    assert gave_up == {"value": True, "warned": True}, gave_up
    decided = _is_consistent(str(p), None)
    assert decided == {"value": False, "warned": False}, decided
