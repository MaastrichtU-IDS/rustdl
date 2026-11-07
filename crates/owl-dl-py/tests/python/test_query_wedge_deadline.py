"""#457: `is_subclass_of` did not finish on an 8-axiom inconsistent TBox: the
named subsumption query ran its H4 hypertableau wedge with no deadline. It now
runs under `RUSTDL_QUERY_WEDGE_MS` (default 3000) and falls through to the main
tableau. The query runs in a child interpreter, so a regression fails at the
timeout instead of hanging the suite.
"""
import os
import pathlib
import subprocess
import sys

FIXTURE = (
    pathlib.Path(__file__).resolve().parents[3]
    / "owl-dl-cli"
    / "tests"
    / "fixtures"
    / "query_wedge_hang_457.ofn"
)

CHILD = """
import sys, rustdl
try:
    print(rustdl.is_subclass_of(sys.argv[1], sys.argv[2], sys.argv[3]))
except rustdl.RustdlError as e:
    print("error", e)
"""


def test_is_subclass_of_finishes_on_the_wedge_hang_reproducer():
    assert FIXTURE.exists(), FIXTURE
    env = {k: v for k, v in os.environ.items() if not k.startswith("RUSTDL_")}
    out = subprocess.run(
        [
            sys.executable,
            "-c",
            CHILD,
            str(FIXTURE),
            "http://gen.rustdl/o#C0",
            "http://gen.rustdl/o#C1",
        ],
        capture_output=True,
        text=True,
        env=env,
        timeout=60,
    )
    assert out.returncode == 0, out.stderr
    # The KB is inconsistent and has no individual: since #457's second residual
    # the tableau-answered query runs the bounded main-tableau consistency check
    # (RUSTDL_QUERY_NO_SEED_TAIL), so it raises instead of answering a vacuous True.
    answer = out.stdout.strip()
    assert answer.startswith("error") and "inconsistent" in answer.lower(), answer
