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
    inconsistent: bool = False


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

    if c.inconsistent:
        just, reps = _explain(path, ["inconsistent"])
        return GateResult(ok=False, inconsistent=True,
                          justification=just, repairs=reps)

    new_unsat = sorted(set(c.unsatisfiable) - baseline_unsat)
    if not new_unsat:
        return GateResult(ok=True)

    target = new_unsat[0]
    try:
        _, roots, _ = rustdl.diagnose(path)
    except rustdl.RustdlError:
        roots = []
    just, reps = _explain(path, ["unsat", target])
    return GateResult(ok=False, new_unsat=new_unsat, roots=list(roots),
                      justification=just, repairs=reps)


def _explain(path, query):
    """Best-effort justify + repair, tolerant of engine errors on the ad-hoc
    ontologies an LLM produces (e.g. an unsatisfiable class that was never
    declared, which classify surfaces but justify rejects with UnknownClassError).
    A rejected edit is still reported as a clash; only the extra detail is dropped."""
    try:
        just = rustdl.justify(path, query)
    except rustdl.RustdlError:
        just = []
    try:
        reps = rustdl.repair(path, query)
    except rustdl.RustdlError:
        reps = []
    return just, reps


def _short(iri: str) -> str:
    return iri.rsplit("#", 1)[-1].rsplit("/", 1)[-1]


def format_feedback(r: GateResult) -> str:
    if r.parse_error:
        return f"Your axiom did not parse ({r.parse_error}). Return one valid OWL functional-syntax axiom."
    lines = []
    if r.inconsistent:
        lines.append("That edit makes the whole ontology inconsistent.")
    else:
        target = r.new_unsat[0]
        extra = f" (and {len(r.new_unsat) - 1} other class(es))" if len(r.new_unsat) > 1 else ""
        lines.append(f"That edit makes {_short(target)} unsatisfiable{extra}.")
    if r.justification:
        lines.append("Minimal cause:")
        lines += [f"  - {a}" for a in r.justification]
    if r.repairs:
        fix = "; ".join(r.repairs[0])
        lines.append(f"To fix, remove one of these axioms, e.g.: {fix}")
    lines.append("Propose a revised axiom that does not introduce this clash.")
    return "\n".join(lines)
