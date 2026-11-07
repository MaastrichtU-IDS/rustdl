#!/usr/bin/env python3
"""Konclude ∩ HermiT oracle for generated cases (#363 step 3): agreement only.

A case gets an oracle answer only when BOTH peers finish and agree on consistency, the
unsat set, the Thing-equivalent set and the pair closure (over classes neither side
calls unsat or Thing-equivalent). Otherwise it is `peer-disagree` or `peer-fail`,
counted and kept, never scored. Input a peer rejects as illegal is `invalid`: a
generator defect, never scored.

Peer verdicts come from output CONTENT, never from exit codes:
  * Konclude exits 0 on junk and writes an 896-byte Thing/Nothing-only hierarchy, so a
    run counts only if its hierarchy mentions every declared class. Inconsistency is
    read from the hierarchy (owl:Thing in the owl:Nothing group), NOT from the "is
    inconsistent" log line, which also appears after an aborted run.
  * HermiT (robot.jar's HermiT CommandLine, `-c`) writes nothing and logs
    `InconsistentOntologyException` on an inconsistent KB. `Non-simple property` and
    `not regular` mean the input is illegal (invalid). Any other exception or parser
    noise is a peer failure. A clean run logs nothing.
Results are cached by the ontology's sha256, so mutant arms rerun rustdl only.
"""
import hashlib
import json
import os
import re
import resource
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from owl_readers import (  # noqa: E402
    _PFX, _TERM, NOTHING, THING, java_bin, ofn_hierarchy, owx_declared_classes, owx_hierarchy)

HOME = Path.home()
KONCLUDE = os.environ.get("GEN_KONCLUDE", str(HOME / ".local/bin/konclude"))
ROBOT = os.environ.get("ROBOT_JAR", str(HOME / "rustdl/peers/robot.jar"))
JAVA = str(java_bin())
PEER_CAP = int(os.environ.get("GEN_PEER_CAP", "60"))
ORACLE_VERSION = 1  # bump when the reading of peer output changes: invalidates the cache

_DECL = re.compile(r"Declaration\(\s*Class\(\s*" + _TERM + r"\s*\)\s*\)")


def declared_classes(text):
    """Every declared named class, as a full IRI: `<IRI>` and any declared prefix (#502).
    The reader used to match only `:Name` under `Prefix(:=…)`, so on a real ontology
    (full IRIs) it found no class and `oracle()` marked the file "invalid"."""
    pfx = dict(_PFX.findall(text))
    out = set()
    for t in _DECL.findall(text):
        if t.startswith("<"):
            out.add(t[1:-1])
        else:
            p, _, local = t.partition(":")
            if p in pfx:
                out.add(pfx[p] + local)
    return out - {THING, NOTHING}


def _limit():
    # 8 GB address space for native peers; the JVM gets -Xmx instead (it reserves more
    # virtual memory than it uses, so RLIMIT_AS would kill it spuriously).
    resource.setrlimit(resource.RLIMIT_AS, (8_000_000_000, 8_000_000_000))


def _run(cmd, cap, limit=True):
    try:
        p = subprocess.run(cmd, capture_output=True, text=True, timeout=cap,
                           preexec_fn=_limit if limit else None)
        return p.returncode, p.stdout + p.stderr
    except subprocess.TimeoutExpired:
        return None, "timeout"


def konclude(path, classes, work):
    out = Path(work) / "kon.owx"
    rc, log = _run([KONCLUDE, "classification", "-i", str(path), "-o", str(out)], PEER_CAP)
    if rc is None:
        return {"status": "timeout"}
    if rc != 0 or not out.exists():
        return {"status": "fail", "why": f"rc={rc}", "log": log[-400:]}
    seen = owx_declared_classes(out)
    if not classes <= seen:
        # junk / unparsed: Konclude wrote its Thing/Nothing-only stub
        return {"status": "fail", "why": "hierarchy omits declared classes", "log": log[-400:]}
    h = owx_hierarchy(out)
    return answer(h, classes)


def hermit(path, classes, work):
    out = Path(work) / "her.ofn"
    rc, log = _run([JAVA, "-Xmx2g", "-Dfile.encoding=UTF-8", "-cp", ROBOT,
                    "org.semanticweb.HermiT.cli.CommandLine", "-c", "-o", str(out), str(path)],
                   PEER_CAP, limit=False)
    if rc is None:
        return {"status": "timeout"}
    if "InconsistentOntologyException" in log:
        return {"status": "ok", "consistent": False}
    if "Non-simple property" in log or "not regular" in log:
        return {"status": "invalid", "why": log.strip().splitlines()[0][:300]}
    if rc != 0 or re.search(r"Exception|Parser:|Error", log) or not out.exists():
        return {"status": "fail", "why": f"rc={rc}", "log": log[-400:]}
    return answer(ofn_hierarchy(out.read_text(errors="replace")), classes)


def answer(h, classes):
    """Restrict a peer's hierarchy to the case's declared classes and serialise it."""
    if not h.consistent:
        return {"status": "ok", "consistent": False}
    return {
        "status": "ok",
        "consistent": True,
        "unsat": sorted(h.unsat & classes),
        "thing_equiv": sorted(h.thing_equiv & classes),
        "pairs": sorted([s, t] for s, t in h.pairs_without(set()) if s in classes and t in classes),
    }


def pairs_excluding(ans, exclude):
    return {(s, t) for s, t in ans["pairs"] if s not in exclude and t not in exclude}


def agree(k, h):
    """(verdict, kinds, oracle answer or None)."""
    st = (k["status"], h["status"])
    if "invalid" in st:
        return "invalid", ["invalid"], None
    if st != ("ok", "ok"):
        return "peer-fail", [f"konclude:{k['status']}", f"hermit:{h['status']}"], None
    if k["consistent"] != h["consistent"]:
        return "peer-disagree", ["consistency"], None
    if not k["consistent"]:
        return "agree", [], {"consistent": False}
    kinds = []
    if k["unsat"] != h["unsat"]:
        kinds.append("unsat")
    if k["thing_equiv"] != h["thing_equiv"]:
        kinds.append("thing_equiv")
    excl = set(k["unsat"]) | set(h["unsat"]) | set(k["thing_equiv"]) | set(h["thing_equiv"])
    if pairs_excluding(k, excl) != pairs_excluding(h, excl):
        kinds.append("pairs")
    if kinds:
        return "peer-disagree", kinds, None
    return "agree", [], {
        "consistent": True,
        "unsat": k["unsat"],
        "thing_equiv": k["thing_equiv"],
        "pairs": sorted([s, t] for s, t in pairs_excluding(k, excl)),
    }


def sha(text):
    return hashlib.sha256(text.encode()).hexdigest()


def oracle(path, cache_dir):
    """The cached oracle record for one ontology file."""
    text = Path(path).read_text()
    key = sha(text)
    cache = Path(cache_dir) / f"{key[:2]}/{key}.json"
    if cache.exists():
        rec = json.loads(cache.read_text())
        if rec.get("oracle_version") == ORACLE_VERSION:
            return rec
    classes = declared_classes(text)
    if not classes:
        # Nothing to check a peer's output against: an empty set would let Konclude's
        # junk stub pass the "mentions every declared class" test vacuously.
        rec = {"oracle_version": ORACLE_VERSION, "sha256": key, "verdict": "invalid",
               "kinds": ["no declared classes"], "answer": None, "konclude": {}, "hermit": {}}
        return rec
    with tempfile.TemporaryDirectory(prefix="gen-oracle-") as work:
        src = Path(work) / "case.ofn"
        src.write_text(text)
        k = konclude(src, classes, work)
        h = hermit(src, classes, work)
    verdict, kinds, ans = agree(k, h)
    rec = {"oracle_version": ORACLE_VERSION, "sha256": key, "verdict": verdict,
           "kinds": kinds, "answer": ans, "konclude": k, "hermit": h}
    cache.parent.mkdir(parents=True, exist_ok=True)
    tmp = cache.with_suffix(".part")
    tmp.write_text(json.dumps(rec))
    tmp.replace(cache)
    return rec


if __name__ == "__main__":
    import argparse
    ap = argparse.ArgumentParser(description="oracle record for one or more .ofn files")
    ap.add_argument("files", nargs="+")
    ap.add_argument("--cache", required=True)
    a = ap.parse_args()
    for f in a.files:
        r = oracle(f, a.cache)
        print(f, r["verdict"], r["kinds"], json.dumps(r["answer"])[:300])
