"""Package 2 prerequisite (#363): the class-expression queries warn only on a
give-up, like their named siblings, and run under the single-query node cap.

`RUSTDL_QUERY_MAX_NODES` reaches only calls that hold `single_query_node_cap()`,
and the caps are cached once per process, so each case runs in a fresh
interpreter with `RUSTDL_MAX_NODES` unset.
"""
import json
import os
import subprocess
import sys
import textwrap

# Off EL (`∀s.E`); C and D each need an `:r`-successor, so a cap of 1 trips.
# `a` is a told member of C; untyped `b` needs a probe, which the cap cuts.
SRC = """Prefix(:=<http://ex/#>)
Ontology(<http://ex/exprgiveup>
  Declaration(Class(:C)) Declaration(Class(:D)) Declaration(Class(:E))
  Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))
  Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
  SubClassOf(:C ObjectSomeValuesFrom(:r :C))
  SubClassOf(:D ObjectSomeValuesFrom(:r :D))
  SubClassOf(:C ObjectAllValuesFrom(:s :E))
  ClassAssertion(:C :a)
)
"""

# label -> (function, args)
CALLS = {
    "class_expression_satisfiable": ("class_expression_satisfiable", (":C",)),
    "class_expression_entailed_subclass": ("class_expression_entailed_subclass", (":C", ":D")),
    "class_expression_instances": ("class_expression_instances", (":D",)),
    "instances_of": ("instances_of", ("http://ex/#D",)),
    "class_expression_instances_C": ("class_expression_instances", (":C",)),
    "instances_of_C": ("instances_of", ("http://ex/#C",)),
}


def _run(path: str, query_cap: str | None) -> dict:
    script = textwrap.dedent(
        f"""
        import json, warnings, rustdl
        out = {{}}
        for label, (name, args) in {CALLS!r}.items():
            with warnings.catch_warnings(record=True) as w:
                warnings.simplefilter("always")
                value = getattr(rustdl, name)({path!r}, *args)
            out[label] = {{
                "value": value,
                "warned": any(issubclass(x.category, rustdl.IncompleteQueryWarning) for x in w),
            }}
        print(json.dumps(out))
        """
    )
    env = dict(os.environ, RUSTDL_PSEUDO_MODEL="0")
    env.pop("RUSTDL_MAX_NODES", None)
    env.pop("RUSTDL_QUERY_MAX_NODES", None)
    if query_cap is not None:
        env["RUSTDL_QUERY_MAX_NODES"] = query_cap
    res = subprocess.run(
        [sys.executable, "-c", script], env=env, capture_output=True, text=True, check=True
    )
    return json.loads(res.stdout.strip().splitlines()[-1])


def test_a_give_up_under_the_query_cap_warns(tmp_path):
    p = tmp_path / "o.ofn"
    p.write_text(SRC)
    capped = _run(str(p), "1")
    for name in CALLS:
        assert capped[name]["warned"], f"{name} must hold the query cap and warn: {capped[name]}"
    # Sound directions.
    assert capped["class_expression_satisfiable"]["value"] is True
    assert capped["class_expression_entailed_subclass"]["value"] is False
    # A give-up keeps the members already proved.
    assert capped["class_expression_instances_C"]["value"] == ["http://ex/#a"]
    assert capped["instances_of_C"]["value"] == ["http://ex/#a"]


def test_off_el_without_a_give_up_does_not_warn(tmp_path):
    p = tmp_path / "o.ofn"
    p.write_text(SRC)
    control = _run(str(p), None)
    for name in CALLS:
        assert not control[name]["warned"], f"{name} warned without a give-up: {control[name]}"
    assert control["class_expression_satisfiable"]["value"] is True
    assert control["class_expression_entailed_subclass"]["value"] is False
    assert control["class_expression_instances"]["value"] == []
    assert control["class_expression_instances_C"]["value"] == ["http://ex/#a"]
