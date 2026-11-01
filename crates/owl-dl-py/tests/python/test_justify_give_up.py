"""#396: the explanation calls warn when a reasoner check under them gives up.

`RUSTDL_MAX_NODES` is cached once per process, so each case runs in a fresh
interpreter with the cap set in its environment. The fixtures are the CLI's
(`owl-dl-cli/tests/justify_give_up.rs`), which says why each gives up.
"""
import json
import os
import subprocess
import sys
import textwrap

C, D = "http://ex/#C", "http://ex/#D"

# The top-level check gives up: an empty result must not read as "not entailed".
TOP_LEVEL = """Prefix(:=<http://ex/#>)
Ontology(<http://ex/top>
  Declaration(Class(:C)) Declaration(Class(:D)) Declaration(Class(:E))
  Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))
  SubClassOf(:C ObjectSomeValuesFrom(:r :C))
  SubClassOf(:D ObjectSomeValuesFrom(:r :D))
  SubClassOf(:C ObjectAllValuesFrom(:s :E))
)
"""

# QuickXplain gives up below an entailed top level: the result still entails.
QX = """Prefix(:=<http://ex/#>)
Ontology(<http://ex/qx>
  Declaration(Class(:C)) Declaration(Class(:X)) Declaration(Class(:D)) Declaration(Class(:E))
  Declaration(ObjectProperty(:r))
  SubClassOf(:C ObjectSomeValuesFrom(:r :C))
  SubClassOf(:C ObjectAllValuesFrom(:r :E))
  EquivalentClasses(:C :X)
  EquivalentClasses(:X :D)
)
"""

# Every class unsatisfiable; diagnose's consistency check gives up.
ALL_UNSAT = """Prefix(:=<http://ex/au#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://ex/allunsat>
  Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))
  Declaration(ObjectProperty(:r))
  SubClassOf(owl:Thing ObjectSomeValuesFrom(:r ObjectUnionOf(:A :B)))
  SubClassOf(ObjectSomeValuesFrom(:r :A) owl:Nothing)
  SubClassOf(ObjectSomeValuesFrom(:r :B) owl:Nothing)
)
"""


def _run(paths: dict, cap: str) -> dict:
    """Each call: (result, warned with IncompleteQueryWarning)."""
    script = textwrap.dedent(
        f"""
        import json, warnings, rustdl
        p = {paths!r}
        calls = {{
            "justify": lambda: rustdl.justify(p["top"], ["subclass", {C!r}, {D!r}]),
            "justify_all": lambda: rustdl.justify_all(p["top"], ["subclass", {C!r}, {D!r}]),
            "repair": lambda: rustdl.repair(p["top"], ["subclass", {C!r}, {D!r}]),
            "justify_qx": lambda: rustdl.justify(p["qx"], ["subclass", {C!r}, {D!r}]),
            "prepared_justify": lambda: rustdl.prepare(p["qx"]).justify(
                ["subclass", {C!r}, {D!r}]
            ),
            "prepared_justify_all": lambda: rustdl.prepare(p["top"]).justify_all(
                ["subclass", {C!r}, {D!r}]
            ),
            "diagnose": lambda: list(rustdl.diagnose(p["all_unsat"])),
            "debug": lambda: rustdl.debug(p["all_unsat"]).consistent,
        }}
        out = {{}}
        for name, call in calls.items():
            with warnings.catch_warnings(record=True) as w:
                warnings.simplefilter("always")
                value = call()
            out[name] = {{
                "value": value,
                "warned": any(issubclass(x.category, rustdl.IncompleteQueryWarning) for x in w),
            }}
        print(json.dumps(out))
        """
    )
    env = dict(os.environ, RUSTDL_MAX_NODES=cap)
    res = subprocess.run(
        [sys.executable, "-c", script], env=env, capture_output=True, text=True, check=True
    )
    return json.loads(res.stdout.strip().splitlines()[-1])


def _paths(tmp_path) -> dict:
    out = {}
    for name, src in [("top", TOP_LEVEL), ("qx", QX), ("all_unsat", ALL_UNSAT)]:
        p = tmp_path / f"{name}.ofn"
        p.write_text(src)
        out[name] = str(p)
    return out


def test_a_give_up_under_an_explanation_call_warns(tmp_path):
    capped = _run(_paths(tmp_path), "1")
    for name, got in capped.items():
        assert got["warned"], f"{name} must warn on a give-up: {got}"
    # Return types are unchanged; a top-level give-up returns the empty result.
    assert capped["justify"]["value"] == []
    assert capped["repair"]["value"] == []
    # A QuickXplain give-up still returns an entailing justification.
    assert len(capped["justify_qx"]["value"]) == 2
    assert len(capped["prepared_justify"]["value"]) == 2


def test_no_warning_without_a_give_up(tmp_path):
    control = _run(_paths(tmp_path), "0")
    for name, got in control.items():
        assert not got["warned"], f"{name} warned without a give-up: {got}"
    assert control["justify"]["value"] == []
    assert len(control["justify_qx"]["value"]) == 2
    assert control["diagnose"]["value"][0] is False
    assert control["debug"]["value"] is False
