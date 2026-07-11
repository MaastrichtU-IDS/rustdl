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
    rejection: str | None = None


@dataclass
class LoopResult:
    turns: list[Turn] = field(default_factory=list)
    proposed: int = 0
    clashes_caught: int = 0
    fixed_after_repair: int = 0
    final_unsat: int = 0
    malformed: int = 0


def _prompt(current_ofn: str, feedback: str | None) -> str:
    base = f"{TASK}\n\nCurrent ontology:\n{current_ofn}"
    return base if feedback is None else base + f"\n\nYour previous axiom was rejected:\n{feedback}"


def run_loop(base_ofn: str, llm: LLM, n_edits: int, max_revisions: int, workdir: str) -> LoopResult:
    baseline = set(rustdl.classify(_write_base(base_ofn, workdir)).unsatisfiable)
    result = LoopResult()
    current = base_ofn
    for i in range(n_edits):
        result.proposed += 1
        feedback = None
        had_clash = False
        had_malformed = False
        for rev in range(max_revisions + 1):
            axiom = llm.propose(_prompt(current, feedback))
            r = gate.check(current, axiom, baseline, workdir)
            if r.ok:
                result.turns.append(Turn(i, rev, axiom, True, feedback, rejection=None))
                current = gate.apply_edit(current, axiom)
                if had_clash:
                    result.fixed_after_repair += 1
                break
            kind = "parse" if r.parse_error else "clash"
            if kind == "clash":
                had_clash = True
            else:
                had_malformed = True
            feedback = gate.format_feedback(r)
            result.turns.append(Turn(i, rev, axiom, False, feedback, rejection=kind))
        if had_clash:
            result.clashes_caught += 1
        if had_malformed:
            result.malformed += 1
    result.final_unsat = len(rustdl.classify(_write_base(current, workdir)).unsatisfiable) - len(baseline)
    return result


def _write_base(ofn: str, workdir: str) -> str:
    import os
    path = os.path.join(workdir, "current.ofn")
    with open(path, "w") as f:
        f.write(ofn)
    return path
