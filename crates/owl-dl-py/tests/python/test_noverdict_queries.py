"""#493: the instance queries and realize keep their result on a likely-bug NoVerdict.

A probe that ends in NoVerdict on an internal limit (#449: a probe with no deadline, or
one whose saturation stalled, #475) used to make `realize`, `instances_of`,
`class_expression_instances` and the class-assertion / existential materializers raise
(deadline-free) or answer silently (budgeted). They now return their result and emit
`IncompleteQueryWarning` with the "likely a reasoner bug" text; `is_instance_of`, whose
answer would be a bare "no", raises `RustdlError` as before.

The stall is forced with `RUSTDL_TABLEAU_SATURATE_ITERS=1`, which the tableau reads once
per process, so each case runs in a fresh interpreter.
"""
import json
import os
import subprocess
import sys
import textwrap

# Off EL (`∀s.E`), so neither the saturation fast path nor realize's saturation path
# answers; `⊤ ⊑ E` gives every probe root a rule, so the forced stall fires.
SRC = """Prefix(:=<http://ex/#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://ex/noverdict-py>
  Declaration(Class(:C)) Declaration(Class(:D)) Declaration(Class(:E))
  Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))
  Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
  SubClassOf(:C ObjectSomeValuesFrom(:r :C))
  SubClassOf(:D ObjectSomeValuesFrom(:r :D))
  SubClassOf(:C ObjectAllValuesFrom(:s :E))
  SubClassOf(owl:Thing :E)
  ClassAssertion(:C :a) ClassAssertion(:D :b) ObjectPropertyAssertion(:r :a :b)
)
"""

# label -> (function, args after the path)
CALLS = {
    "realize": ("realize", ()),
    "instances_of": ("instances_of", ("http://ex/#D",)),
    "class_expression_instances": ("class_expression_instances", (":D",)),
    "materialize_inferred_class_assertions": ("materialize_inferred_class_assertions", ()),
    "materialize_existential_successors": ("materialize_existential_successors", ()),
    "is_instance_of": ("is_instance_of", ("http://ex/#D", "http://ex/#a")),
}
MARKER = "likely a reasoner bug"


def _run(path: str, forced: bool, pair_ms: str) -> dict:
    script = textwrap.dedent(
        f"""
        import json, warnings, rustdl
        out = {{}}
        for label, (name, args) in {CALLS!r}.items():
            with warnings.catch_warnings(record=True) as w:
                warnings.simplefilter("always")
                try:
                    value = getattr(rustdl, name)({path!r}, *args)
                    raised = None
                except rustdl.RustdlError as e:
                    value, raised = None, str(e)
            out[label] = {{
                "raised": raised,
                "returned": value is not None,
                "bug_warned": any(
                    issubclass(x.category, rustdl.IncompleteQueryWarning)
                    and {MARKER!r} in str(x.message)
                    for x in w
                ),
            }}
        print(json.dumps(out))
        """
    )
    env = {k: v for k, v in os.environ.items() if not k.startswith("RUSTDL_")}
    env.update(
        RUSTDL_HYPERTABLEAU="0",
        RUSTDL_WEDGE_CONSISTENCY="0",
        RUSTDL_PSEUDO_MODEL="0",
        RUSTDL_REALIZE_PAIR_TIMEOUT_MS=pair_ms,
    )
    if forced:
        env["RUSTDL_TABLEAU_SATURATE_ITERS"] = "1"
    res = subprocess.run(
        [sys.executable, "-c", script], env=env, capture_output=True, text=True, check=True
    )
    return json.loads(res.stdout.strip().splitlines()[-1])


def test_a_stalled_probe_warns_and_keeps_the_result(tmp_path):
    p = tmp_path / "o.ofn"
    p.write_text(SRC)
    # Deadline-free (0) and budgeted (a generous 20 s, so the stall cannot be a deadline).
    for pair_ms in ("0", "20000"):
        got = _run(str(p), True, pair_ms)
        for name in CALLS:
            if name == "is_instance_of":
                assert got[name]["raised"], f"{pair_ms}: {name} raises on a bug-signal no: {got[name]}"
                continue
            assert got[name]["raised"] is None, f"{pair_ms}: {name} keeps its result: {got[name]}"
            assert got[name]["returned"], f"{pair_ms}: {name}: {got[name]}"
            assert got[name]["bug_warned"], f"{pair_ms}: {name} warns with #449's text: {got[name]}"


def test_without_the_stall_nothing_is_reported(tmp_path):
    p = tmp_path / "o.ofn"
    p.write_text(SRC)
    got = _run(str(p), False, "20000")
    for name in CALLS:
        assert got[name]["raised"] is None, f"{name}: {got[name]}"
        assert not got[name]["bug_warned"], f"{name}: {got[name]}"
