#!/usr/bin/env python3
"""Reduce an ontology to a minimal inconsistent core, using Konclude as oracle.

Konclude decides these files in 1-3 s while rustdl times out, so the oracle is the
peer. Verdict is read from Konclude's LOG CONTENT ("is inconsistent"), never from an
exit code -- it exits 0 on junk. A subset that Konclude cannot process at all counts
as NOT inconsistent, which is the conservative direction: it keeps axioms rather than
dropping them, so the result is a genuine superset of a true core.
"""
import subprocess, sys, os, tempfile

KON = "/data/dumontier/reasoners/konclude"
def split(path):
    txt = open(path, encoding="utf-8", errors="replace").read().splitlines()
    i = next(k for k, l in enumerate(txt) if l.startswith("Ontology("))
    head, body = txt[: i + 1], txt[i + 1 :]
    while body and body[-1].strip() in (")", ""):
        body.pop()
    return head, body

def inconsistent(head, axioms, tmp):
    with open(tmp, "w", encoding="utf-8") as f:
        f.write("\n".join(head) + "\n" + "\n".join(axioms) + "\n)\n")
    try:
        p = subprocess.run([KON, "classification", "-i", tmp, "-o", os.devnull],
                           capture_output=True, text=True, timeout=180)
    except subprocess.TimeoutExpired:
        return False
    return "is inconsistent" in (p.stdout + p.stderr)

def reduce_(head, body, tmp, log):
    cur, n = list(body), 2
    while len(cur) > 1:
        chunk = max(1, len(cur) // n)
        i, progressed = 0, False
        while i < len(cur):
            trial = cur[:i] + cur[i + chunk:]
            if trial and inconsistent(head, trial, tmp):
                cur, progressed = trial, True
                log(f"    kept reduction -> {len(cur)} axioms")
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

if __name__ == "__main__":
    src = sys.argv[1]
    head, body = split(src)
    tmp = tempfile.mktemp(suffix=".ofn")
    def log(m): print(m, flush=True)
    log(f"{os.path.basename(src)}: {len(body)} axioms")
    log(f"  control FULL set inconsistent? {inconsistent(head, body, tmp)}")
    log(f"  control EMPTY set inconsistent? {inconsistent(head, ['Declaration(Class(owl:Thing))'], tmp)}")
    core = reduce_(head, body, tmp, log)
    out = src.replace(".owl", "") + "-core.ofn"
    with open(out, "w", encoding="utf-8") as f:
        f.write("\n".join(head) + "\n" + "\n".join(core) + "\n)\n")
    log(f"  CORE = {len(core)} axioms -> {out}")
    for a in core: log(f"    {a[:150]}")
