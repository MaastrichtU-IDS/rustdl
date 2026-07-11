# NeSy LLM-Assisted Authoring Loop — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A minimal LLM-in-the-loop ontology-authoring demo on rustdl's Python bindings — the LLM proposes ontology edits, rustdl gates each with classify/consistency and returns justify/diagnose/repair feedback, the LLM revises — producing real transcripts and a small quantitative table to realize the paper's neurosymbolic claim.

**Architecture:** A deterministic Python scaffold around a swappable LLM. Each turn: apply the LLM's proposed axiom to a clean base ontology, write it to a temp file, run rustdl (path-based API); if the edit introduces a newly-unsatisfiable class or inconsistency, format rustdl's `diagnose`/`justify`/`repair` output as feedback and ask the LLM to revise. All non-LLM logic is unit-tested with a `ScriptedLLM`; the real transcript comes from one run with `AnthropicLLM`.

**Tech Stack:** Python ≥3.10, the `rustdl` package (built from `crates/owl-dl-py` via maturin), the `anthropic` SDK, pytest.

## Global Constraints

- Python ≥ 3.10 (matches `crates/owl-dl-py/pyproject.toml`).
- The `rustdl` package is built/installed via `maturin develop` inside a venv; do **not** modify any Rust crate — this is a front-end demo only.
- rustdl's explanation API is **path-based**: `classify(path,…)`, `is_consistent(path)`, `diagnose(path)`, `justify(path, query)`, `repair(path, query)`. Every turn writes the candidate ontology to a temp file.
- LLM access is behind an `LLM` protocol. `AnthropicLLM` uses the `anthropic` SDK with model `claude-sonnet-5` and `ANTHROPIC_API_KEY`. **All tests use `ScriptedLLM`** (no network, deterministic).
- All new code lives under `crates/owl-dl-py/examples/nesy_loop/`.
- Ontologies are OWL functional syntax (`.ofn`); a proposed edit is exactly one axiom string appended before the ontology's closing `)`.

---

### Task 1: Environment + package build

**Files:**
- Create: `crates/owl-dl-py/examples/nesy_loop/README.md`
- Create: `crates/owl-dl-py/examples/nesy_loop/requirements.txt`

**Interfaces:**
- Produces: an importable `rustdl` in the active venv; `anthropic` + `pytest` installed.

- [ ] **Step 1: Create the venv and install build tooling**

Run:
```bash
cd ~/code/rustdl/crates/owl-dl-py
python3 -m venv .venv && source .venv/bin/activate
pip install -U maturin pytest anthropic
```

- [ ] **Step 2: Build + install rustdl into the venv**

Run: `maturin develop --release`
Expected: builds `rustdl._native`, ends `📦 Built ... 🛠 Installed rustdl`.

- [ ] **Step 3: Smoke-test the import and a classify call**

Run:
```bash
python -c "import rustdl; print(sorted(n for n in dir(rustdl) if not n.startswith('_'))[:6])"
```
Expected: a list including `classify`, `diagnose`, `is_consistent`, `justify`, `repair`.

- [ ] **Step 4: Write `requirements.txt`**

```
anthropic>=0.40
pytest>=8
```

- [ ] **Step 5: Write `README.md`** (how to build, run tests, run the real loop — see Task 7 for commands) and **commit**

```bash
git add crates/owl-dl-py/examples/nesy_loop
git commit -m "chore(nesy): example scaffold + build instructions"
```

---

### Task 2: Clean seed ontology fixture

**Files:**
- Create: `crates/owl-dl-py/examples/nesy_loop/fixtures/seed.ofn`
- Test: `crates/owl-dl-py/examples/nesy_loop/tests/test_seed.py`

**Interfaces:**
- Produces: a consistent ontology with **zero** unsatisfiable classes, containing two disjoint classes (`CheeseTopping`, `VegetableTopping`) so that a later "subclass of both" edit is cleanly unsatisfiable. Prefix IRI `http://ex.org/pizza#`.

- [ ] **Step 1: Write the failing test**

```python
# tests/test_seed.py
import os, rustdl
SEED = os.path.join(os.path.dirname(__file__), "..", "fixtures", "seed.ofn")

def test_seed_is_clean():
    c = rustdl.classify(SEED)
    assert c.inconsistent() is False
    assert c.unsatisfiable() == []
```

- [ ] **Step 2: Run it to verify it fails**

Run: `pytest tests/test_seed.py -v`
Expected: FAIL (file not found / fixture missing).

- [ ] **Step 3: Write `fixtures/seed.ofn`**

```
Prefix(:=<http://ex.org/pizza#>)
Ontology(<http://ex.org/pizza>
Declaration(Class(:Pizza))
Declaration(Class(:Topping))
Declaration(Class(:CheeseTopping))
Declaration(Class(:VegetableTopping))
Declaration(Class(:Mozzarella))
Declaration(Class(:Tomato))
SubClassOf(:CheeseTopping :Topping)
SubClassOf(:VegetableTopping :Topping)
SubClassOf(:Mozzarella :CheeseTopping)
SubClassOf(:Tomato :VegetableTopping)
DisjointClasses(:CheeseTopping :VegetableTopping)
)
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pytest tests/test_seed.py -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/owl-dl-py/examples/nesy_loop/fixtures crates/owl-dl-py/examples/nesy_loop/tests/test_seed.py
git commit -m "test(nesy): clean seed ontology fixture"
```

---

### Task 3: The rustdl gate (apply edit + check + feedback)

**Files:**
- Create: `crates/owl-dl-py/examples/nesy_loop/gate.py`
- Test: `crates/owl-dl-py/examples/nesy_loop/tests/test_gate.py`

**Interfaces:**
- Produces:
  - `apply_edit(base_ofn: str, axiom: str) -> str` — returns `base_ofn` with `axiom` inserted before the final `)`.
  - `@dataclass GateResult(ok: bool, parse_error: str | None, new_unsat: list[str], roots: list[str], justification: list[str], repairs: list[list[str]])`.
  - `check(base_ofn: str, axiom: str, baseline_unsat: set[str], workdir: str) -> GateResult` — writes the edited ontology, classifies it; `ok=True` iff it parses, stays consistent, and adds no new unsatisfiable class. On failure fills `new_unsat`/`roots`/`justification`/`repairs`.
  - `format_feedback(r: GateResult) -> str` — natural-language feedback for the LLM.
- Consumes: `rustdl.classify`, `rustdl.diagnose`, `rustdl.justify`, `rustdl.repair`, `rustdl.ParseError`.

- [ ] **Step 1: Write the failing tests**

```python
# tests/test_gate.py
import os
from nesy_loop import gate
SEED = open(os.path.join(os.path.dirname(__file__), "..", "fixtures", "seed.ofn")).read()
P = "http://ex.org/pizza#"

def test_apply_edit_inserts_before_close():
    out = gate.apply_edit(SEED, "SubClassOf(:A :B)")
    assert out.count("Ontology(") == 1
    assert out.rstrip().endswith(")")
    assert "SubClassOf(:A :B)" in out

def test_good_edit_passes(tmp_path):
    r = gate.check(SEED, "SubClassOf(:Tomato :Topping)", set(), str(tmp_path))
    assert r.ok and r.parse_error is None and r.new_unsat == []

def test_bad_edit_caught_with_justification_and_repair(tmp_path):
    axiom = "SubClassOf(:CheeseAndVeg :CheeseTopping) SubClassOf(:CheeseAndVeg :VegetableTopping) Declaration(Class(:CheeseAndVeg))"
    r = gate.check(SEED, axiom, set(), str(tmp_path))
    assert r.ok is False
    assert f"{P}CheeseAndVeg" in r.new_unsat
    assert any("Disjoint" in a for a in r.justification)
    assert len(r.repairs) >= 1

def test_parse_error_reported(tmp_path):
    r = gate.check(SEED, "this is not an axiom", set(), str(tmp_path))
    assert r.ok is False and r.parse_error is not None

def test_feedback_mentions_class_and_fix(tmp_path):
    axiom = "Declaration(Class(:X)) SubClassOf(:X :CheeseTopping) SubClassOf(:X :VegetableTopping)"
    r = gate.check(SEED, axiom, set(), str(tmp_path))
    fb = gate.format_feedback(r)
    assert "unsatisfiable" in fb.lower() and "X" in fb
```

- [ ] **Step 2: Run to verify they fail**

Run: `PYTHONPATH=.. pytest tests/test_gate.py -v`
Expected: FAIL (module `nesy_loop.gate` missing).

- [ ] **Step 3: Implement `gate.py`**

```python
# gate.py
from __future__ import annotations
import os
from dataclasses import dataclass, field
import rustdl


def apply_edit(base_ofn: str, axiom: str) -> str:
    s = base_ofn.rstrip()
    assert s.endswith(")"), "base ontology must end with ')'"
    return s[:-1].rstrip() + "\n" + axiom.strip() + "\n)\n"


@dataclass
class GateResult:
    ok: bool
    parse_error: str | None = None
    new_unsat: list[str] = field(default_factory=list)
    roots: list[str] = field(default_factory=list)
    justification: list[str] = field(default_factory=list)
    repairs: list[list[str]] = field(default_factory=list)


def _write(workdir: str, ofn: str) -> str:
    path = os.path.join(workdir, "candidate.ofn")
    with open(path, "w") as f:
        f.write(ofn)
    return path


def check(base_ofn: str, axiom: str, baseline_unsat: set[str], workdir: str) -> GateResult:
    path = _write(workdir, apply_edit(base_ofn, axiom))
    try:
        c = rustdl.classify(path)
    except rustdl.ParseError as e:
        return GateResult(ok=False, parse_error=str(e))
    except rustdl.RustdlError as e:
        return GateResult(ok=False, parse_error=str(e))

    if c.inconsistent():
        just = rustdl.justify(path, ["inconsistent"])
        reps = rustdl.repair(path, ["inconsistent"])
        return GateResult(ok=False, new_unsat=["<ontology inconsistent>"],
                          justification=just, repairs=reps)

    new_unsat = sorted(set(c.unsatisfiable()) - baseline_unsat)
    if not new_unsat:
        return GateResult(ok=True)

    target = new_unsat[0]
    consistent, roots, _ = rustdl.diagnose(path)
    just = rustdl.justify(path, ["unsat", target])
    reps = rustdl.repair(path, ["unsat", target])
    return GateResult(ok=False, new_unsat=new_unsat, roots=list(roots),
                      justification=just, repairs=reps)


def _short(iri: str) -> str:
    return iri.rsplit("#", 1)[-1].rsplit("/", 1)[-1]


def format_feedback(r: GateResult) -> str:
    if r.parse_error:
        return f"Your axiom did not parse ({r.parse_error}). Return one valid OWL functional-syntax axiom."
    lines = []
    unsat = ", ".join(_short(u) for u in r.new_unsat)
    lines.append(f"That edit makes {unsat} unsatisfiable. Minimal cause:")
    lines += [f"  - {a}" for a in r.justification]
    if r.repairs:
        fix = "; ".join(r.repairs[0])
        lines.append(f"To fix, remove one of these axioms, e.g.: {fix}")
    lines.append("Propose a revised axiom that does not introduce this clash.")
    return "\n".join(lines)
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `PYTHONPATH=.. pytest tests/test_gate.py -v`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/owl-dl-py/examples/nesy_loop/gate.py crates/owl-dl-py/examples/nesy_loop/tests/test_gate.py
git commit -m "feat(nesy): rustdl gate — apply edit, check, format feedback"
```

---

### Task 4: LLM interface (protocol + ScriptedLLM + AnthropicLLM)

**Files:**
- Create: `crates/owl-dl-py/examples/nesy_loop/llm.py`
- Test: `crates/owl-dl-py/examples/nesy_loop/tests/test_llm.py`

**Interfaces:**
- Produces:
  - `class LLM(Protocol): def propose(self, prompt: str) -> str: ...`
  - `class ScriptedLLM: __init__(self, replies: list[str])`, `propose` returns replies in order (raises `IndexError` if exhausted). Deterministic; for tests + recorded replay.
  - `class AnthropicLLM: __init__(self, model: str = "claude-sonnet-5")`, `propose` calls `anthropic.Anthropic().messages.create(...)` and returns the first text block, stripped.
- Consumes: nothing from earlier tasks.

- [ ] **Step 1: Write the failing test** (ScriptedLLM only — no network)

```python
# tests/test_llm.py
import pytest
from nesy_loop.llm import ScriptedLLM

def test_scripted_returns_in_order():
    m = ScriptedLLM(["a", "b"])
    assert m.propose("x") == "a"
    assert m.propose("y") == "b"

def test_scripted_exhausted_raises():
    m = ScriptedLLM(["only"])
    m.propose("x")
    with pytest.raises(IndexError):
        m.propose("y")
```

- [ ] **Step 2: Run to verify it fails**

Run: `PYTHONPATH=.. pytest tests/test_llm.py -v`
Expected: FAIL (module missing).

- [ ] **Step 3: Implement `llm.py`**

```python
# llm.py
from __future__ import annotations
from typing import Protocol


class LLM(Protocol):
    def propose(self, prompt: str) -> str: ...


class ScriptedLLM:
    """Deterministic LLM stub: returns canned replies in order."""
    def __init__(self, replies: list[str]):
        self._replies = list(replies)
        self._i = 0

    def propose(self, prompt: str) -> str:
        reply = self._replies[self._i]  # IndexError when exhausted
        self._i += 1
        return reply


class AnthropicLLM:
    def __init__(self, model: str = "claude-sonnet-5"):
        from anthropic import Anthropic
        self._client = Anthropic()  # reads ANTHROPIC_API_KEY
        self._model = model

    def propose(self, prompt: str) -> str:
        resp = self._client.messages.create(
            model=self._model, max_tokens=1024,
            messages=[{"role": "user", "content": prompt}],
        )
        return resp.content[0].text.strip()
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `PYTHONPATH=.. pytest tests/test_llm.py -v`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/owl-dl-py/examples/nesy_loop/llm.py crates/owl-dl-py/examples/nesy_loop/tests/test_llm.py
git commit -m "feat(nesy): LLM interface with scripted + Anthropic implementations"
```

---

### Task 5: The authoring loop (orchestration + metrics + transcript)

**Files:**
- Create: `crates/owl-dl-py/examples/nesy_loop/loop.py`
- Test: `crates/owl-dl-py/examples/nesy_loop/tests/test_loop.py`

**Interfaces:**
- Consumes: `gate.check`, `gate.format_feedback`, `gate.GateResult`; `llm.LLM`.
- Produces:
  - `@dataclass Turn(edit_index: int, revision: int, axiom: str, accepted: bool, feedback: str | None)`
  - `@dataclass LoopResult(turns: list[Turn], proposed: int, clashes_caught: int, fixed_after_repair: int, final_unsat: int)`
  - `run_loop(base_ofn: str, llm: LLM, n_edits: int, max_revisions: int, workdir: str) -> LoopResult` — for each of `n_edits`: ask the LLM for an axiom (`propose`), gate it; if rejected, feed `format_feedback` back and re-ask up to `max_revisions` times; accept the first passing axiom into the running ontology. Track metrics.
- The LLM prompt is built by `run_loop`; the first ask includes the current ontology and the task, each revision includes the feedback.

- [ ] **Step 1: Write the failing test** (end-to-end with ScriptedLLM — deterministic, exercises real rustdl)

```python
# tests/test_loop.py
import os
from nesy_loop import loop
from nesy_loop.llm import ScriptedLLM
SEED = open(os.path.join(os.path.dirname(__file__), "..", "fixtures", "seed.ofn")).read()

def test_good_edit_accepted_first_try(tmp_path):
    llm = ScriptedLLM(["SubClassOf(:Tomato :Topping)"])
    r = loop.run_loop(SEED, llm, n_edits=1, max_revisions=2, workdir=str(tmp_path))
    assert r.proposed == 1 and r.clashes_caught == 0
    assert r.turns[-1].accepted is True and r.final_unsat == 0

def test_bad_then_repaired(tmp_path):
    # First proposal clashes (subclass of both disjoint); revision fixes it.
    bad = "Declaration(Class(:Q)) SubClassOf(:Q :CheeseTopping) SubClassOf(:Q :VegetableTopping)"
    good = "Declaration(Class(:Q)) SubClassOf(:Q :CheeseTopping)"
    llm = ScriptedLLM([bad, good])
    r = loop.run_loop(SEED, llm, n_edits=1, max_revisions=2, workdir=str(tmp_path))
    assert r.clashes_caught == 1 and r.fixed_after_repair == 1
    assert r.turns[0].accepted is False and r.turns[1].accepted is True
    assert r.turns[0].feedback and "unsatisfiable" in r.turns[0].feedback.lower()
    assert r.final_unsat == 0

def test_unrepaired_clash_counts(tmp_path):
    bad = "Declaration(Class(:Q)) SubClassOf(:Q :CheeseTopping) SubClassOf(:Q :VegetableTopping)"
    llm = ScriptedLLM([bad, bad])  # never fixes
    r = loop.run_loop(SEED, llm, n_edits=1, max_revisions=1, workdir=str(tmp_path))
    assert r.clashes_caught == 1 and r.fixed_after_repair == 0
    assert r.turns[-1].accepted is False
```

- [ ] **Step 2: Run to verify it fails**

Run: `PYTHONPATH=.. pytest tests/test_loop.py -v`
Expected: FAIL (module missing).

- [ ] **Step 3: Implement `loop.py`**

```python
# loop.py
from __future__ import annotations
from dataclasses import dataclass, field
import rustdl
from . import gate
from .llm import LLM

TASK = ("You are extending an OWL ontology. Propose exactly ONE new axiom in OWL "
        "functional syntax, using the prefix ':' = <http://ex.org/pizza#>. Reply "
        "with only the axiom, no prose.")


@dataclass
class Turn:
    edit_index: int
    revision: int
    axiom: str
    accepted: bool
    feedback: str | None = None


@dataclass
class LoopResult:
    turns: list[Turn] = field(default_factory=list)
    proposed: int = 0
    clashes_caught: int = 0
    fixed_after_repair: int = 0
    final_unsat: int = 0


def _prompt(current_ofn: str, feedback: str | None) -> str:
    base = f"{TASK}\n\nCurrent ontology:\n{current_ofn}"
    return base if feedback is None else base + f"\n\nYour previous axiom was rejected:\n{feedback}"


def run_loop(base_ofn: str, llm: LLM, n_edits: int, max_revisions: int, workdir: str) -> LoopResult:
    import os
    baseline = set(rustdl.classify(_write_base(base_ofn, workdir)).unsatisfiable())
    result = LoopResult()
    current = base_ofn
    for i in range(n_edits):
        result.proposed += 1
        feedback = None
        had_clash = False
        for rev in range(max_revisions + 1):
            axiom = llm.propose(_prompt(current, feedback))
            r = gate.check(current, axiom, baseline, workdir)
            if r.ok:
                result.turns.append(Turn(i, rev, axiom, True, feedback))
                current = gate.apply_edit(current, axiom)
                if had_clash:
                    result.fixed_after_repair += 1
                break
            had_clash = True
            feedback = gate.format_feedback(r)
            result.turns.append(Turn(i, rev, axiom, False, feedback))
        if had_clash:
            result.clashes_caught += 1
    result.final_unsat = len(rustdl.classify(_write_base(current, workdir)).unsatisfiable()) - len(baseline)
    return result


def _write_base(ofn: str, workdir: str) -> str:
    import os
    path = os.path.join(workdir, "current.ofn")
    with open(path, "w") as f:
        f.write(ofn)
    return path
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `PYTHONPATH=.. pytest tests/test_loop.py -v`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/owl-dl-py/examples/nesy_loop/loop.py crates/owl-dl-py/examples/nesy_loop/tests/test_loop.py
git commit -m "feat(nesy): authoring loop with metrics + transcript turns"
```

---

### Task 6: CLI runner (transcript + metrics output)

**Files:**
- Create: `crates/owl-dl-py/examples/nesy_loop/run.py`
- Test: `crates/owl-dl-py/examples/nesy_loop/tests/test_run.py`

**Interfaces:**
- Consumes: `loop.run_loop`, `loop.LoopResult`; `llm.ScriptedLLM`/`AnthropicLLM`.
- Produces:
  - `write_transcript(res: LoopResult, path: str) -> None` — one JSON object per turn (`edit_index`, `revision`, `axiom`, `accepted`, `feedback`) as JSONL.
  - `metrics_markdown(res: LoopResult) -> str` — a Markdown table (proposed / clashes caught / fixed after repair / final unsat) for the paper.
  - `main(argv)` — flags `--n-edits`, `--max-revisions`, `--scripted FILE` (JSON list of replies; default uses `AnthropicLLM`), `--out DIR`; writes `transcript.jsonl` + `metrics.md`.

- [ ] **Step 1: Write the failing test**

```python
# tests/test_run.py
import json, os
from nesy_loop import run
from nesy_loop.loop import LoopResult, Turn

def test_write_transcript_jsonl(tmp_path):
    res = LoopResult(turns=[Turn(0, 0, "SubClassOf(:A :B)", True, None)], proposed=1)
    p = os.path.join(tmp_path, "t.jsonl")
    run.write_transcript(res, p)
    rows = [json.loads(l) for l in open(p)]
    assert rows[0]["axiom"] == "SubClassOf(:A :B)" and rows[0]["accepted"] is True

def test_metrics_markdown_has_counts():
    res = LoopResult(proposed=5, clashes_caught=2, fixed_after_repair=2, final_unsat=0)
    md = run.metrics_markdown(res)
    assert "| 5 |" in md and "clashes caught" in md.lower()
```

- [ ] **Step 2: Run to verify it fails**

Run: `PYTHONPATH=.. pytest tests/test_run.py -v`
Expected: FAIL (module missing).

- [ ] **Step 3: Implement `run.py`**

```python
# run.py
from __future__ import annotations
import argparse, json, os, sys
from dataclasses import asdict
from .loop import run_loop, LoopResult
from .llm import ScriptedLLM, AnthropicLLM


def write_transcript(res: LoopResult, path: str) -> None:
    with open(path, "w") as f:
        for t in res.turns:
            f.write(json.dumps(asdict(t)) + "\n")


def metrics_markdown(res: LoopResult) -> str:
    return (
        "| edits proposed | clashes caught | fixed after repair | final new-unsat |\n"
        "|---:|---:|---:|---:|\n"
        f"| {res.proposed} | {res.clashes_caught} | {res.fixed_after_repair} | {res.final_unsat} |\n"
    )


def main(argv=None) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--seed", default=os.path.join(os.path.dirname(__file__), "fixtures", "seed.ofn"))
    ap.add_argument("--n-edits", type=int, default=8)
    ap.add_argument("--max-revisions", type=int, default=2)
    ap.add_argument("--scripted", default=None, help="JSON file with a list of replies (offline)")
    ap.add_argument("--out", default="out")
    a = ap.parse_args(argv)
    os.makedirs(a.out, exist_ok=True)
    base = open(a.seed).read()
    llm = ScriptedLLM(json.load(open(a.scripted))) if a.scripted else AnthropicLLM()
    res = run_loop(base, llm, a.n_edits, a.max_revisions, a.out)
    write_transcript(res, os.path.join(a.out, "transcript.jsonl"))
    open(os.path.join(a.out, "metrics.md"), "w").write(metrics_markdown(res))
    print(metrics_markdown(res))
    return 0


if __name__ == "__main__":
    sys.exit(main())
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `PYTHONPATH=.. pytest tests/test_run.py -v` then the full suite `PYTHONPATH=.. pytest -v`
Expected: PASS (all tests green).

- [ ] **Step 5: Commit**

```bash
git add crates/owl-dl-py/examples/nesy_loop/run.py crates/owl-dl-py/examples/nesy_loop/tests/test_run.py
git commit -m "feat(nesy): CLI runner emitting transcript.jsonl + metrics.md"
```

---

### Task 7: Real run + capture for the paper (documentation task)

**Files:**
- Create: `crates/owl-dl-py/examples/nesy_loop/out/transcript.jsonl` (captured)
- Create: `crates/owl-dl-py/examples/nesy_loop/out/metrics.md` (captured)

**Interfaces:**
- Consumes: everything above.

- [ ] **Step 1: Run the loop against the real LLM**

Run (with `ANTHROPIC_API_KEY` set, venv active):
```bash
cd ~/code/rustdl/crates/owl-dl-py/examples/nesy_loop
PYTHONPATH=.. python -m nesy_loop.run --n-edits 8 --max-revisions 2 --out out
```
Expected: prints the metrics table; writes `out/transcript.jsonl` + `out/metrics.md`.

- [ ] **Step 2: Sanity-check the transcript**

Confirm `out/transcript.jsonl` contains at least one rejected→revised→accepted sequence (a real caught-and-repaired clash), and that the feedback text shows a rustdl justification + repair. If the run produced no clash (LLM too cautious), re-run with a prompt nudge (`--n-edits 12`) so at least one clash is exercised; note this in the README.

- [ ] **Step 3: Record a deterministic replay**

Extract the axioms the LLM produced into `fixtures/recorded_replies.json` (a JSON list) so the exact transcript replays offline via `--scripted fixtures/recorded_replies.json`. Verify replay reproduces the same metrics.

- [ ] **Step 4: Commit the captured artifacts**

```bash
git add crates/owl-dl-py/examples/nesy_loop/out crates/owl-dl-py/examples/nesy_loop/fixtures/recorded_replies.json
git commit -m "docs(nesy): captured real transcript + deterministic replay"
```

- [ ] **Step 5: Hand the numbers to the paper**

Copy `metrics.md`'s table and one transcript excerpt into the paper's §4.x scenario (replacing the hypothetical pizza walkthrough with the real run), and update the honesty framing from "future work" to "a minimal realization" for the loop (keeping the full autonomous agent as future work). This edit happens in the `rustdl-paper` repo, not here.

---

## Self-Review

- **Spec coverage:** LLM-proposes/rustdl-checks loop (Tasks 3,5) ✓; justify/diagnose/repair feedback (Task 3) ✓; real transcript (Task 7) ✓; quantitative table (Tasks 6,7) ✓; realizes the paper claim (Task 7 step 5) ✓.
- **Placeholder scan:** all steps carry real code/commands; no TBDs.
- **Type consistency:** `GateResult`, `Turn`, `LoopResult` fields and `check`/`run_loop`/`apply_edit`/`format_feedback` signatures are used identically across Tasks 3/5/6.
- **Known risks:** (1) `maturin develop` needs a Rust toolchain (present on this host). (2) The `anthropic` message-block access `resp.content[0].text` assumes a text block first — true for a plain text reply. (3) LLM nondeterminism is quarantined to Task 7; all logic is tested with `ScriptedLLM`. (4) If the LLM emits multiple axioms or Manchester instead of functional syntax, `gate.check` reports it as a parse error and the loop feeds that back — acceptable behaviour, not a crash.
