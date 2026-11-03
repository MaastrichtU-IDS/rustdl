#!/usr/bin/env python3
"""Shrink a generated case to a minimal reproducer of ONE finding (#363 step 3).

  shrink.py --case FILE --kind KIND --witness W --bin BIN --oracle-cache DIR --out DIR
            [--env K=V ...]

ddmin over logical axioms (the shape of scripts/ddmin-inconsistent-core.py).
Declarations are kept throughout and unused ones pruned at the end. Runs under the
arm's env, so a mutant detection is shrunk under the mutant's env.

A subset still fails iff ALL of these hold:
  * both peers finish and agree on it (oracle verdict `agree`);
  * scoring finds the same kind with the same witness, so the witness is still a
    compared class (not unsat on either side, not Thing-equivalent: `score` excludes
    those);
  * the rustdl arm that produced it reports `incomplete: false`. That replaces
    "two stable runs", which is not stability evidence.
`--kind arm-fail:<arm>` instead shrinks an arm that errors out (stderr contains
`--witness`) on input both peers agree on; it is not a scored kind.
A subset where a peer fails or the peers disagree counts as NOT failing, so the axiom
is kept: the conservative direction.
"""
import argparse
import hashlib
import json
import re
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import oracle as orc  # noqa: E402
import run  # noqa: E402


def split(text):
    lines = text.rstrip("\n").splitlines()
    assert lines[-1].strip() == ")", "expects one axiom per line and a closing ')'"
    body = lines[:-1]
    i = max(k for k, l in enumerate(body)
            if l.startswith(("Ontology(", "Declaration(", "Prefix(", "#")))
    return body[: i + 1], body[i + 1:]


def render(head, axioms):
    return "\n".join(head + list(axioms) + [")"]) + "\n"


class Pred:
    def __init__(self, a, head, env):
        self.a, self.head, self.env = a, head, env
        self.calls = 0
        self.work = Path(tempfile.mkdtemp(prefix="gen-shrink-"))

    def __call__(self, axioms, head=None):
        self.calls += 1
        text = render(head or self.head, axioms)
        path = self.work / f"s-{hashlib.sha256(text.encode()).hexdigest()[:16]}.ofn"
        path.write_text(text)
        ora = orc.oracle(path, self.a.oracle_cache)
        if self.a.kind.startswith("peer:"):
            assert self.a.kind == "peer:konclude-consistent", "the only peer predicate"
            # Not a rustdl finding: a peer disagreement to record as an oracle defect.
            # `peer:konclude-consistent` keeps Konclude saying consistent while HermiT
            # says inconsistent. Both peers must finish, as everywhere else.
            k, h = ora["konclude"], ora["hermit"]
            hit = (k.get("status") == "ok" and h.get("status") == "ok"
                   and k["consistent"] and not h["consistent"])
            return (hit, {"oracle": ora, "arms": {}, "path": str(path),
                          "finding": {"kind": self.a.kind, "witness": self.a.witness}}
                    if hit else None)
        if ora["verdict"] != "agree":
            return False, None
        if self.a.oracle_consistent is not None and \
                ora["answer"]["consistent"] != (self.a.oracle_consistent == "true"):
            return False, None
        arms = {arm: run.run_arm(self.a.bin, arm, path, self.env) for arm in run.ARMS}
        if self.a.kind.startswith("arm-fail:"):
            # Not a scored kind: an arm that errors out (e.g. `NoVerdict`, documented as
            # a bug indicator) on input both peers decide. `--witness` is a substring
            # of the arm's stderr.
            arm = arms[self.a.kind.split(":", 1)[1]]
            hit = arm.get("status") == "fail" and self.a.witness in arm.get("stderr", "")
            return (hit, {"oracle": ora, "arms": arms, "path": str(path),
                          "finding": {"kind": self.a.kind, "witness": self.a.witness}}
                    if hit else None)
        found = [f for f in run.score(ora["answer"], orc.declared_classes(text), arms)
                 if f["kind"] == self.a.kind and f["witness"] == self.a.witness
                 and (self.a.tier is None or f["tier"] == self.a.tier)]
        if not found:
            return False, None
        arm = arms.get(found[0]["arm"], {})
        if arm.get("status") != "ok" or (arm["json"].get("incomplete")
                                          and not self.a.allow_incomplete):
            return False, None
        return True, {"oracle": ora, "arms": arms, "finding": found[0], "path": str(path)}


def ddmin(cur, test, log):
    n = 2
    while len(cur) > 1:
        chunk = max(1, len(cur) // n)
        i, progressed = 0, False
        while i < len(cur):
            trial = cur[:i] + cur[i + chunk:]
            if trial and test(trial)[0]:
                cur, progressed = trial, True
                log(f"  kept reduction -> {len(cur)} axioms")
                i = 0
                continue
            i += chunk
        if progressed:
            n = 2
        elif n >= len(cur):
            break
        else:
            n = min(len(cur), n * 2)
    return cur


def prune_declarations(head, axioms, witness=""):
    """Drop declarations no kept axiom uses; the witness's classes always stay."""
    used = set(re.findall(r":(\w+)", "\n".join(axioms)))
    if "|" in witness or witness.startswith("http"):
        used |= {re.split(r"[#/]", w)[-1] for w in witness.split("|")}
    keep = []
    for line in head:
        m = re.match(r"Declaration\(\w+\(:(\w+)\)\)", line)
        if m and m.group(1) not in used:
            continue
        keep.append(line)
    return keep


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--case", required=True)
    ap.add_argument("--kind", required=True)
    ap.add_argument("--witness", required=True)
    ap.add_argument("--bin", required=True)
    ap.add_argument("--oracle-cache", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--env", action="append")
    ap.add_argument("--tier", choices=["finding", "exposed", "pre-check"],
                    help="also require the same tier: a MISSED can change tier when a "
                         "reduction changes the fragment. run.py's detect output gives it")
    ap.add_argument("--allow-incomplete", action="store_true",
                    help="keep a finding whose arm reports incomplete: true (an FP made by "
                         "a budget-cutting env, e.g. M7's); recorded in record.json")
    ap.add_argument("--oracle-consistent", choices=["true", "false"],
                    help="also require the peers' agreed consistency verdict")
    a = ap.parse_args()
    env = run.parse_env(a.env)
    text = Path(a.case).read_text()
    head, body = split(text)
    test = Pred(a, head, env)

    def log(m):
        print(m, flush=True)

    ok, _ = test(body)
    log(f"{a.case}: {len(body)} axioms; full set reproduces {a.kind} {a.witness}: {ok}")
    if not ok:
        return 2
    core = ddmin(list(body), test, log)
    small_head = prune_declarations(head, core, a.witness)
    # the witness classes must stay declared: re-check, else keep every declaration
    ok, info = test(core, small_head)
    if not ok:
        small_head = head
        ok, info = test(core, head)
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    minimal = render(small_head, core)
    (out / "minimal.ofn").write_text(minimal)
    binsha = hashlib.sha256(Path(a.bin).read_bytes()).hexdigest()
    ora = info["oracle"]
    rec = {
        "original": a.case, "header": text.splitlines()[0], "kind": a.kind,
        "witness": a.witness, "tier": a.tier, "env": env, "allow_incomplete": a.allow_incomplete, "binary": a.bin, "binary_sha256": binsha,
        "axioms_before": len(body), "axioms_after": len(core), "predicate_calls": test.calls,
        "peers": {"verdict": ora["verdict"], "answer": ora["answer"],
                  "konclude": ora["konclude"], "hermit": ora["hermit"]},
        "rustdl": {arm: {"cmd": " ".join([a.bin, *run.ARMS[arm], "minimal.ofn"]),
                         "status": r.get("status"), "json": r.get("json")}
                   for arm, r in info["arms"].items()},
        "finding": info["finding"],
    }
    (out / "record.json").write_text(json.dumps(rec, indent=1))
    log(f"  CORE = {len(core)} axioms, {test.calls} predicate calls -> {out / 'minimal.ofn'}")
    for line in core:
        log(f"    {line}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
