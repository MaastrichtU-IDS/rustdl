#!/usr/bin/env python3
"""Which laws caught a mutant? Compare two cross-command-laws report.json files.

A law catches the mutant on an ontology when the mutant's run has, and the
control's run (same seed, unmutated binary) does not have, any of:
  - a hard-law violation;
  - a HermiT-confirmed false positive;
  - more discrepancies for that (ontology, law) than the control, counting
    budget-cut ones only when HermiT refutes them (as the harness ratchet does).
Unresolved counts are not detections.

Prints one `LAW@ontology` per line, sorted; prints nothing if nothing caught.
"""
import json
import sys
from collections import Counter


def signals(path):
    r = json.load(open(path))
    fails = Counter((f["law"] if f.get("kind") == "violation" else f["law"] + "/FP", f["onto"])
                    for f in r["failures"])
    # Mirror the harness ratchet: a discrepancy raised while classify was cut by
    # its budget is load-sensitive and does not count, unless HermiT refutes the
    # per-pair claim (then it is a false positive whatever the budget did).
    disc = Counter((d["law"], d["onto"]) for d in r["discrepancies"]
                   if not d.get("budget_cut") or d.get("hermit") == "refutes")
    return fails, disc


def caught(control, mutant):
    cf, cd = signals(control)
    mf, md = signals(mutant)
    out = set()
    for (law, onto), n in mf.items():
        if n > cf.get((law, onto), 0):
            out.add(f"{law}@{onto}")
    for (law, onto), n in md.items():
        if n > cd.get((law, onto), 0):
            out.add(f"{law}@{onto}")
    return sorted(out)


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit("usage: detect.py CONTROL_REPORT MUTANT_REPORT")
    for line in caught(sys.argv[1], sys.argv[2]):
        print(line)
