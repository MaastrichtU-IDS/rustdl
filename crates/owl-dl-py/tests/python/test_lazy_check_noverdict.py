"""#493 part 2: a stall of the #369 lazy consistency check's own probe, after a positive
answer, is reported as #449's likely bug: the answer stands, with
`ConsistencyUndeterminedWarning` and an `IncompleteQueryWarning` carrying the "likely a
reasoner bug" text. The stall is forced with `RUSTDL_TABLEAU_SATURATE_ITERS`, which the
tableau reads once per process, so each case runs in a fresh interpreter.
"""
import json
import os
import subprocess
import sys
import textwrap
from pathlib import Path

FIXTURES = Path(__file__).resolve().parents[3] / "owl-dl-cli" / "tests" / "fixtures"

# `a : C` is told (a closure hit, no probe); the opt-in tail runs with the wedge off.
ABOX = """Prefix(:=<http://ex/#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://ex/aboxtold>
  Declaration(Class(:C)) Declaration(Class(:D)) Declaration(Class(:E))
  Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
  Declaration(ObjectProperty(:r))
  SubClassOf(owl:Thing :E) SubClassOf(:C ObjectAllValuesFrom(:r :E))
  ClassAssertion(:C :a) ClassAssertion(:D :b) ObjectPropertyAssertion(:r :a :b)
)
"""
MARKER = "likely a reasoner bug"


def _run(call: str, env_extra: dict) -> dict:
    script = textwrap.dedent(
        f"""
        import json, warnings, rustdl
        with warnings.catch_warnings(record=True) as w:
            warnings.simplefilter("always")
            value = {call}
        print(json.dumps({{
            "value": value,
            "bug": any(issubclass(x.category, rustdl.IncompleteQueryWarning)
                       and {MARKER!r} in str(x.message) for x in w),
            "undetermined": any(issubclass(x.category, rustdl.ConsistencyUndeterminedWarning)
                                for x in w),
        }}))
        """
    )
    env = {k: v for k, v in os.environ.items() if not k.startswith("RUSTDL_")}
    env.update(env_extra)
    res = subprocess.run(
        [sys.executable, "-c", script], env=env, capture_output=True, text=True, check=True
    )
    return json.loads(res.stdout.strip().splitlines()[-1])


def test_is_instance_of_yes_with_a_stalled_lazy_tail(tmp_path):
    p = tmp_path / "o.ofn"
    p.write_text(ABOX)
    call = f"rustdl.is_instance_of({str(p)!r}, 'http://ex/#C', 'http://ex/#a')"
    env = {"RUSTDL_WEDGE_CONSISTENCY": "0", "RUSTDL_QUERY_LAZY_CONSISTENCY_TAIL": "1"}
    got = _run(call, {**env, "RUSTDL_TABLEAU_SATURATE_ITERS": "1"})
    assert got == {"value": True, "bug": True, "undetermined": True}, got
    control = _run(call, env)
    assert control["value"] is True and not control["bug"], control


def test_is_subclass_of_yes_with_a_stalled_no_seed_tail():
    # The chain8 shape (`TBox`-only, the #492 no-seed tail); H4 off so the main tableau
    # answers (the prepared route).
    f = str(FIXTURES / "lazy_tail_chain8_493.ofn")
    call = f"rustdl.is_subclass_of({f!r}, 'http://ex/#X', 'http://ex/#Y')"
    env = {"RUSTDL_HYPERTABLEAU": "0"}
    got = _run(call, {**env, "RUSTDL_TABLEAU_SATURATE_ITERS": "4"})
    assert got == {"value": True, "bug": True, "undetermined": True}, got
    control = _run(call, env)
    assert control["value"] is True and not control["bug"], control
