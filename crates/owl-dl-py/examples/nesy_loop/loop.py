from __future__ import annotations
from dataclasses import dataclass, field
import rustdl
from . import gate
from .llm import LLM

# Always-applied output discipline (kept separate so a custom --task cannot drop it).
FORMAT = ("Reply with ONLY OWL functional-syntax axioms -- no prose, no markdown, no "
          "code fences, no explanation. Declare any new class first with "
          "Declaration(Class(:X)), then state its subclass axiom(s). Example of the "
          "exact required form:\n"
          "Declaration(Class(:Mozzarella))\nSubClassOf(:Mozzarella :CheeseTopping)")

# Default per-edit content instruction (override with run_loop(task=...)).
TASK = ("You are extending an OWL ontology, prefix ':' = <http://ex.org/pizza#>. "
        "Propose exactly ONE new axiom that enriches it (for example, introduce and "
        "classify a new topping or pizza). Use only classes that appear below or that "
        "you introduce with a Declaration.")


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


def _prompt(current_ofn: str, feedback: str | None, task: str) -> str:
    base = f"{task}\n\n{FORMAT}\n\nCurrent ontology:\n{current_ofn}"
    return base if feedback is None else base + f"\n\nYour previous axiom was rejected:\n{feedback}"


def run_loop(base_ofn: str, llm: LLM, n_edits: int, max_revisions: int, workdir: str,
             task: str | None = None) -> LoopResult:
    task = task or TASK
    baseline = set(rustdl.classify(_write_base(base_ofn, workdir)).unsatisfiable)
    result = LoopResult()
    current = base_ofn
    for i in range(n_edits):
        result.proposed += 1
        feedback = None
        had_clash = False
        had_malformed = False
        for rev in range(max_revisions + 1):
            axiom = llm.propose(_prompt(current, feedback, task))
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
        # These counters are per-edit-slot (index i counts at most once each),
        # not a partition of `proposed`: a single edit slot can rack up both a
        # clash and a malformed revision across its retries, so
        # clashes_caught + malformed need not sum to proposed.
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
