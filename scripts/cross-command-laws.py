#!/usr/bin/env python3
"""cross-command-laws.py — rustdl's own commands must agree (#363, package 2).

Runs the rustdl CLI on real ontologies and checks that commands answering
equivalent questions agree. A disagreement between two DEFINITE answers is a
bug in one of them. Agreement is not proof of correctness.

Every operand is three-valued: yes / no / unknown (plus "inconsistent" for an
`Inconsistent` error). Unknown covers exit 3 / `unknown`, a surface's give-up
flag, any other error, and a wall-clock kill. A law is CHECKED only when every
operand it needs is definite; otherwise it is UNRESOLVED, with the reason.

Hard laws (a violation fails the run):
  L1  classify closure A ⊑ B            => `subclass A B` = yes
  L2  classify equivalence {A, B}       => `subclass` both ways = yes
  L3  classify unsat C                  => `sat C` = unsat
  L4  classify `consistent: false`      => `consistent` = inconsistent   (smoke;
      never exercised on the curated corpus here: family's classify reports
      `consistency_undetermined: true` at `--pair-timeout-ms 1000` on this host)
  L5  `consistent` = inconsistent, or classify `consistent: false`
                                        => the guarded surfaces (instance,
      realize, disjoint, individuals, *-expr, and since #428 `sat` / `subclass` /
      `explain`) answer yes or error `Inconsistent`, never "no" (`sat`: never
      "satisfiable").
      `sat owl:Thing` is the consistency check itself (#427) and is not sampled.
  L6  named vs expression surface (smoke): sat = sat-expr,
      subclass = subclass-expr, instance = membership in instances-expr
  L7  `disjoint` pair (A, B)            => `sat-expr "A and B"` = unsat, and
      `subclass A B` != yes unless A is unsat
  L8  `classify --saturation-only` closure ⊆ classify closure
  L10 realize type C of a               => `instance C a` = yes
  L11 `individuals` different (a, b)    => `sat-expr "{a} and {b}"` = unsat
      (never exercised in the first full run: `individuals` was killed at 30 s
      wherever it ran)

Ratchets (counted against a committed per-ontology baseline):
  R1  `subclass A B` = yes, classify closure lacks (A, B)
  R2  `sat C` = unsat, classify lists C as satisfiable
  R3  `consistent` = inconsistent, classify `consistent: true`
      (not `consistency_undetermined`)
  U1  classify unsat C, the Konclude oracle has C satisfiable. Unlike L3 this
      compares rustdl with an ORACLE, so it sees a false unsat that every rustdl
      surface shares (mutant M8b). Never exempted for a budget cut: a deadline
      never makes a class unsat. A class in a spec's hand-adjudicated
      `known_sat` list that classify calls unsat is a hard U1 violation.
      Oracle unsat classes classify misses are listed in report.json
      (`unsat_missed`), not ratcheted. Unlike R1/R2, a U1 discrepancy over the
      baseline fails the run without HermiT: an empty baseline admits none.
Adjudication: a pair in the committed Konclude closure is a classify MISS; a
pair absent from it is a CANDIDATE FP, confirmed only by HermiT (`--hermit`).
Konclude alone never fails a run (it under-reports in documented cases); a U1
baseline entry therefore needs a recorded HermiT verdict, never Konclude alone.
HermiT is asked per claim (`--consistency=C`, `--supers=C`), so it also decides on
an ontology with unsat classes. A HermiT answer that decides nothing (timeout,
failure, an inconsistent ontology) is listed under `unadjudicated` and fails the
run, and `--hermit` without a JVM is an error (#502): an oracle that cannot
answer must never read as zero false positives.

COVERAGE, stated: `--self-test` shows each law CAN fire, on fabricated operands;
that checks the harness, not the engine. Env sabotages (`--sabotage`) cover R1
(#76, both `RUSTDL_MAX_*` merge flags off on maxmerge_fp), R2 (#98) and R3
(family.ofn, with the pre-check and the consistency probe off). Source mutants for every hard law are in scripts/mutants/ (replay
with scripts/mutants/replay-mutants.sh); their table, including the two faults
no law catches (both on a path every surface shares), is in
docs/assurance/query-surface-ledger.md.

Usage:
    scripts/cross-command-laws.py --self-test
    scripts/cross-command-laws.py --bin PINNED_RUSTDL --out DIR [--quick]
        [--cores 92,93,94 | --workers N] [--hermit] [--baseline FILE] [--write-baseline FILE]
    scripts/cross-command-laws.py --bin PINNED_RUSTDL --out DIR --sabotage
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import random
import re
import resource
import subprocess
import sys
import time
from collections import Counter, defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from owl_readers import (  # noqa: E402  (shared with scripts/gen)
    _PFX as _OFN_PFX, closure, java_bin, ofn_named_closure, owx_closure)

OWL = "http://www.w3.org/2002/07/owl#"
THING, NOTHING = OWL + "Thing", OWL + "Nothing"
REPO = Path(__file__).resolve().parent.parent
REAL = REPO / "ontologies" / "real"
LAWS_FIX = REPO / "crates" / "owl-dl-cli" / "tests" / "fixtures" / "laws"
KONCLUDE = REAL / "konclude-input"
CALL_TIMEOUT_S = 30
CLASSIFY_TIMEOUT_S = 60
PAIR_MS = "1000"

# --------------------------------------------------------------------------
# Corpus. Caps follow the plan's measured per-call costs (v4 change 3).
# `laws` limits which operand families run (family: only L4/L5/R3).
# --------------------------------------------------------------------------
DEFAULT_CAPS = dict(pos=40, neg=40, sat=20, inst=20, disj=20, eq=10)
SMALL_CAPS = dict(pos=5, neg=5, sat=5, inst=5, disj=5, eq=3)

CODE_PIZZA = "http://www.co-ode.org/ontologies/pizza/pizza.owl#"
CURATED = [
    # The #76 pairs on the Stanford (co-ode) pizza. Pins whose IRIs are not declared
    # (another pizza vintage) are dropped in observe().
    dict(id="pizza", path=REAL / "pizza.ofn", oracle="pizza-classified.owx",
         pin_neg=[(CODE_PIZZA + "Margherita", CODE_PIZZA + "InterestingPizza"),
                  (CODE_PIZZA + "QuattroFormaggi", CODE_PIZZA + "InterestingPizza")]),
    dict(id="ro", path=REAL / "ro.ofn", oracle="ro-classified.owx"),
    dict(id="sulo", path=REAL / "sulo.ofn", oracle="sulo-classified.owx"),
    dict(id="bibtex", path=REAL / "bibtex.ofn", oracle="bibtex-classified.owx"),
    dict(id="wine", path=REAL / "wine.ofn", oracle="wine-classified.owx", caps=SMALL_CAPS),
    dict(id="sio", path=REAL / "sio.ofn", oracle="sio-classified.owx",
         caps=dict(pos=10, neg=10, sat=10, inst=10, disj=10, eq=5)),
    dict(id="go-basic", path=REAL / "go-basic.ofn", caps=SMALL_CAPS),
    dict(id="family", path=REAL / "family.ofn", laws={"L4", "L5", "R3"},
         caps=dict(pos=0, neg=0, sat=5, inst=5, disj=5, eq=0)),
]
SYNTHETIC = [
    # #76's non-entailed `A ⊑ I`; pinned as an R1 probe. Its sabotage (both
    # `RUSTDL_MAX_CHOICE_POINT=0` and `RUSTDL_MAX_TRIAL_MERGE=0`) is in SABOTAGES.
    dict(id="maxmerge_fp", path=LAWS_FIX / "maxmerge_fp.ofn",
         pin_neg=[("http://ex.org/p#A", "http://ex.org/p#I")]),
    dict(id="complex_qualifier_98", path=LAWS_FIX / "complex_qualifier_98.ofn"),
    # Targets for the package F-lite mutants (scripts/mutants/). Each one makes a
    # law able to see one fault class on the unmutated engine from now on.
    dict(id="all_unsat", path=LAWS_FIX / "all_unsat.ofn"),
    dict(id="neg_disjunct_forall", path=LAWS_FIX / "neg_disjunct_forall.ofn",
         pin_neg=[("http://ex.org/nd#T", "http://ex.org/nd#S")]),
    dict(id="disjoint_up", path=LAWS_FIX / "disjoint_up.ofn"),
    dict(id="realize_disjunction", path=LAWS_FIX / "realize_disjunction.ofn"),
    dict(id="gci_domain_nonatomic", path=LAWS_FIX / "gci_domain_nonatomic.ofn",
         pin_neg=[("http://ex.org/gd#X", "http://ex.org/gd#Z")]),
    # `known_sat`: classes adjudicated satisfiable by hand (no Konclude oracle for
    # the synthetics); U1 checks classify's unsat set against them.
    dict(id="functional_data_min1", path=LAWS_FIX / "functional_data_min1.ofn",
         known_sat=["http://ex.org/fd#C", "http://ex.org/fd#D"]),
    dict(id="inconsistent_small", path=LAWS_FIX / "inconsistent_small.ofn"),
    dict(id="individuals_three", path=LAWS_FIX / "individuals_three.ofn"),
]
QUICK_IDS = {"pizza", "ro", "sulo", "bibtex"} | {s["id"] for s in SYNTHETIC}
QUICK_CAPS = dict(pos=10, neg=10, sat=10, inst=10, disj=10, eq=5)

SABOTAGES = [
    dict(name="R2: RUSTDL_COMPLEX_QUALIFIER_VERIFY=0 on #98", onto="complex_qualifier_98",
         env={"RUSTDL_COMPLEX_QUALIFIER_VERIFY": "0"}, expect="R2"),
    # #446: the consistency probe is off too. With the pre-check off, family's run is
    # admitted to the probe whenever a pair hits the deadline (`incomplete_abox`), and a
    # probe give-up now sets `consistency_undetermined`, which R3 exempts. So without
    # this the sabotage fired or not depending on timing.
    dict(name="R3: RUSTDL_CLASSIFY_INCONSISTENCY=0 RUSTDL_CLASSIFY_CONSISTENCY_PROBE=0 on family",
         onto="family",
         env={"RUSTDL_CLASSIFY_INCONSISTENCY": "0", "RUSTDL_CLASSIFY_CONSISTENCY_PROBE": "0"},
         expect="R3"),
    # #76: both flags off restore the first-pair `≤n` merge, so `subclass A I` says
    # yes on the synthetic (HermiT refutes it). One flag alone is not enough: the
    # #293 choice point and the #76 trial merge each prevent it.
    dict(name="R1: RUSTDL_MAX_CHOICE_POINT=0 RUSTDL_MAX_TRIAL_MERGE=0 on #76", onto="maxmerge_fp",
         env={"RUSTDL_MAX_CHOICE_POINT": "0", "RUSTDL_MAX_TRIAL_MERGE": "0"}, expect="R1"),
]


# --------------------------------------------------------------------------
# Answers and outcomes
# --------------------------------------------------------------------------
def ans(v, why="", cmd=None, rc=None, extra=None):
    """An operand: v in {yes, no, unknown, inconsistent} (or a payload)."""
    return dict(v=v, why=why, cmd=cmd, rc=rc, extra=extra or {})


def definite(a):
    return a is not None and a["v"] not in ("unknown",)


def no_verdict(a):
    """#493: the answer came with the no_verdict signal: a text answer with exit 3 and
    #449's note, or a JSON payload's `no_verdict` / `no_verdict_pairs` (classify)."""
    if a is None:
        return False
    extra = a.get("extra") or {}
    return bool(extra.get("no_verdict")) or (extra.get("no_verdict_pairs") or 0) > 0


def outcome(law, onto, kind, reason, *operands):
    return dict(law=law, onto=onto, kind=kind, reason=reason,
                operands=[{k: o.get(k) for k in ("v", "why", "cmd", "rc")} for o in operands if o])


# --------------------------------------------------------------------------
# Closure (fresh BFS per node: no shared memo, see closure-diff.py's warning)
# --------------------------------------------------------------------------
# `closure` lives in owl_readers.py.


# --------------------------------------------------------------------------
# OFN declarations (classes and individuals for sampling)
# --------------------------------------------------------------------------
_PFX = re.compile(r"Prefix\(\s*([\w.-]*):\s*=\s*<([^>]*)>\s*\)")
_DECL = re.compile(r"Declaration\(\s*(Class|NamedIndividual)\(\s*(<[^>]*>|[\w.-]*:[^\s()]*)\s*\)")


def declarations(path):
    text = Path(path).read_text(errors="replace")
    pfx = {p: i for p, i in _PFX.findall(text)}
    out = {"Class": set(), "NamedIndividual": set()}
    for kind, name in _DECL.findall(text):
        if name.startswith("<"):
            iri = name[1:-1]
        else:
            p, _, local = name.partition(":")
            if p not in pfx:
                continue
            iri = pfx[p] + local
        out[kind].add(iri)
    out["Class"] -= {THING, NOTHING}
    return out


# --------------------------------------------------------------------------
# Konclude OWX oracle (SubClassOf / EquivalentClasses between named classes)
# --------------------------------------------------------------------------
# `owx_closure` lives in owl_readers.py.


# --------------------------------------------------------------------------
# Running the CLI (one JSONL record per call, resumable)
# --------------------------------------------------------------------------
class Runner:
    def __init__(self, binary, core, cache_path, env_extra=None):
        self.binary, self.core = binary, core
        self.env_extra = env_extra or {}
        self.cache_path = cache_path
        self.cache = {}
        if cache_path.exists():
            for line in cache_path.read_text().splitlines():
                try:
                    r = json.loads(line)
                    self.cache[r["key"]] = r
                except (json.JSONDecodeError, KeyError):
                    pass

    def run(self, onto_id, args, timeout=CALL_TIMEOUT_S):
        env_items = sorted(self.env_extra.items())
        key = json.dumps([onto_id, args, env_items])
        if key in self.cache:
            return self.cache[key]
        env = dict(os.environ)
        env.pop("RUSTDL_MAX_NODES", None)
        env.pop("RUSTDL_QUERY_MAX_NODES", None)
        env.update(self.env_extra)
        cmd = ["taskset", "-c", str(self.core), self.binary] + args if self.core is not None \
            else [self.binary] + args
        before = resource.getrusage(resource.RUSAGE_CHILDREN)
        t0 = time.monotonic()
        killed = False
        try:
            p = subprocess.run(cmd, env=env, capture_output=True, text=True, timeout=timeout)
            rc, out, err = p.returncode, p.stdout, p.stderr
        except subprocess.TimeoutExpired as e:
            killed, rc = True, None
            out = e.stdout.decode(errors="replace") if isinstance(e.stdout, bytes) else (e.stdout or "")
            err = e.stderr.decode(errors="replace") if isinstance(e.stderr, bytes) else (e.stderr or "")
        after = resource.getrusage(resource.RUSAGE_CHILDREN)
        cpu = (after.ru_utime - before.ru_utime) + (after.ru_stime - before.ru_stime)
        r = dict(key=key, onto=onto_id, args=args, env=dict(env_items), rc=rc, killed=killed,
                 stdout=out, stderr=err[-4000:], cpu=round(cpu, 3),
                 wall=round(time.monotonic() - t0, 3))
        with self.cache_path.open("a") as f:
            f.write(json.dumps(r) + "\n")
        self.cache[key] = r
        return r


def cmdline(r):
    env = " ".join(f"{k}={v}" for k, v in r.get("env", {}).items())
    return (env + " " if env else "") + "rustdl " + " ".join(r["args"])


# #449's note: the tableau reached an internal limit, likely a reasoner bug.
NOVERDICT_NOTE = "likely a reasoner bug"


def _answer_with_no_verdict(r):
    """#493: on a TEXT answer (a bare token or `explain`'s line), exit 3 with #449's note
    AFTER a definite answer is that answer plus the no_verdict signal (the #369 lazy
    check's own probe stalled), not a give-up. Only `read_token` / `read_explain` use it;
    a JSON payload with exit 3 stays a give-up in `_base` (Fable's review of #500)."""
    last = r["stdout"].strip().splitlines()[-1].strip() if r["stdout"].strip() else ""
    return (r["rc"] == 3 and NOVERDICT_NOTE in r["stderr"] and last != ""
            and not last.startswith(("{", "}", "[", "]"))
            and last != "unknown" and not last.endswith(": unknown")
            and " : unknown" not in last)


def _base(r):
    """Common unknown/inconsistent handling. Returns an answer or None."""
    c = cmdline(r)
    if r["killed"]:
        return ans("unknown", f"killed at {CALL_TIMEOUT_S}s", c, None)
    if r["rc"] == 1 and "ontology is inconsistent" in r["stderr"]:
        return ans("inconsistent", "error Inconsistent", c, 1)
    if r["rc"] == 3:
        return ans("unknown", "exit 3 (gave up)", c, 3)
    if r["rc"] != 0:
        return ans("unknown", f"error rc={r['rc']}: {r['stderr'].strip()[-160:]}", c, r["rc"])
    return None


def read_token(r, yes, no):
    b = None if _answer_with_no_verdict(r) else _base(r)
    if b:
        return b
    tok = r["stdout"].strip().splitlines()[-1].strip() if r["stdout"].strip() else ""
    if tok == "unknown":
        return ans("unknown", "unknown", cmdline(r), r["rc"])
    nv = {"no_verdict": True} if _answer_with_no_verdict(r) else None
    if tok == yes:
        return ans("yes", "no_verdict (#493)" if nv else "", cmdline(r), r["rc"], nv)
    if tok == no:
        return ans("no", "no_verdict (#493)" if nv else "", cmdline(r), r["rc"], nv)
    return ans("unknown", f"unparsed output {tok[:60]!r}", cmdline(r), r["rc"])


def read_explain(r):
    """`explain`: `<sub> ⊑ <sup> : yes|no — answered by …`; exit 3 is a give-up (#493:
    unless after a definite answer with #449's note)."""
    b = None if _answer_with_no_verdict(r) else _base(r)
    if b:
        return b
    line = r["stdout"].strip().splitlines()[-1] if r["stdout"].strip() else ""
    verdict = line.split(" : ", 1)[1].split()[0] if " : " in line else ""
    if verdict in ("yes", "no"):
        nv = {"no_verdict": True} if _answer_with_no_verdict(r) else None
        return ans(verdict, "no_verdict (#493)" if nv else "", cmdline(r), r["rc"], nv)
    return ans("unknown", f"unparsed output {line[:60]!r}", cmdline(r), r["rc"])


def read_json(r, field, give_up="incomplete"):
    b = _base(r)
    if b:
        return b
    try:
        j = json.loads(r["stdout"])
    except json.JSONDecodeError:
        return ans("unknown", "unparsed JSON", cmdline(r), r["rc"])
    if give_up and j.get(give_up):
        return ans("unknown", f"{give_up}: true", cmdline(r), r["rc"], j)
    v = j.get(field)
    return ans("yes" if v is True else "no" if v is False else "unknown",
               "", cmdline(r), r["rc"], j)


def read_payload(r):
    """A JSON payload whose LISTED members are sound positives whatever its
    give-up flag says. Returns ans(v='ok', extra=json) or an unknown."""
    b = _base(r)
    if b:
        return b
    try:
        j = json.loads(r["stdout"])
    except json.JSONDecodeError:
        return ans("unknown", "unparsed JSON", cmdline(r), r["rc"])
    return ans("ok", "", cmdline(r), r["rc"], j)


# --------------------------------------------------------------------------
# Observation: run every operand one ontology needs
# --------------------------------------------------------------------------
def observe(spec, runner, caps, seed, log=print):
    oid, path = spec["id"], str(spec["path"])
    laws = spec.get("laws")
    want = (lambda *names: laws is None or any(n in laws for n in names))
    rng = random.Random(f"{seed}:{oid}")
    obs = dict(onto=oid, q={}, sample={})
    q = obs["q"]
    # U1 compares classify's unsat set with an oracle's; the oracle is data, so it
    # is loaded here and evaluate() stays pure (and self-testable).
    if spec.get("oracle") and (KONCLUDE / spec["oracle"]).exists():
        obs["oracle_unsat"] = owx_closure(KONCLUDE / spec["oracle"])[1]
    if spec.get("known_sat"):
        obs["known_sat"] = set(spec["known_sat"])

    # classify once at the in-tree convention, and saturation-only for L8.
    r = runner.run(oid, ["classify", "--json", "--pair-timeout-ms", PAIR_MS, path],
                   timeout=CLASSIFY_TIMEOUT_S)
    cl = read_payload(r)
    if cl["v"] == "ok":
        j = cl["extra"]
        unsat = set(j["unsatisfiable"])
        obs["classify"] = dict(
            status="ok", consistent=j["consistent"],
            undetermined=j.get("consistency_undetermined", False),
            unsat=unsat, equiv=j["equivalent_groups"],
            closure=closure([tuple(p) for p in j["direct_subsumptions"]], j["equivalent_groups"],
                            drop=unsat),
            incomplete=j.get("incomplete"), cmd=cl["cmd"])
    else:
        obs["classify"] = dict(status="unknown", why=cl["why"], cmd=cl["cmd"])
    if want("L8") and obs["classify"]["status"] == "ok":
        r = runner.run(oid, ["classify", "--saturation-only", "--json", path],
                       timeout=CLASSIFY_TIMEOUT_S)
        so = read_payload(r)
        if so["v"] == "ok":
            j = so["extra"]
            obs["satonly"] = dict(unsat=set(j["unsatisfiable"]), cmd=so["cmd"],
                                  closure=closure([tuple(p) for p in j["direct_subsumptions"]],
                                                  j["equivalent_groups"],
                                                  drop=set(j["unsatisfiable"])))
    if want("L4", "L5", "R3"):
        q[("consistent",)] = read_json(runner.run(oid, ["consistent", "--json", path]),
                                       "consistent")

    decl = declarations(path)
    classes = sorted(decl["Class"])
    inds = sorted(decl["NamedIndividual"])
    c = obs["classify"]
    clo = c.get("closure", set())
    inconsistent = q.get(("consistent",), {}).get("v") == "no"

    # --- samples
    pos = sorted(clo)
    rng.shuffle(pos)
    pos = pos[:caps["pos"]]
    sats = [x for x in classes if x not in c.get("unsat", set())]
    neg = []
    if caps["neg"] and len(sats) > 1:
        # Bias toward siblings: pairs that share a classify ancestor.
        anc = defaultdict(set)
        for s, t in clo:
            anc[s].add(t)
        # Bounded: enumerating every ordered pair is O(n^2) and ran out of
        # memory on go-basic (~50k classes).
        pool_ = rng.sample(sats, min(len(sats), 300))
        cands = [(a, b) for a in pool_ for b in pool_
                 if a != b and (a, b) not in clo and anc[a] & anc[b]]
        if len(cands) < caps["neg"]:
            cands += [(a, b) for a in rng.sample(sats, min(len(sats), 40))
                      for b in rng.sample(sats, min(len(sats), 40)) if a != b and (a, b) not in clo]
        rng.shuffle(cands)
        neg = list(dict.fromkeys(cands))[:caps["neg"]]
    declared = set(classes)
    for p_ in spec.get("pin_neg", []):
        if p_[0] in declared and p_[1] in declared:
            if tuple(p_) not in neg:
                neg.append(tuple(p_))
        else:
            # Expected for another pizza vintage; for any other spec it means a
            # typo or a changed fixture, so say so rather than drop it silently.
            log(f"  {oid}: pinned pair not declared, skipped: {p_[0]} ⊑ {p_[1]}")
    sat_classes = rng.sample(classes, min(len(classes), caps["sat"]))
    sat_classes += [x for x in sorted(c.get("unsat", set())) if x not in sat_classes][:caps["sat"]]
    eq = [g for g in c.get("equiv", []) if all(m not in (THING, NOTHING) for m in g)][:caps["eq"]]
    inst_pairs = []
    if inds and classes:
        inst_pairs = [(rng.choice(classes), rng.choice(inds)) for _ in range(caps["inst"])]
    obs["sample"] = dict(pos=pos, neg=neg, sat=sat_classes, eq=eq, inst=inst_pairs)
    log(f"  {oid}: classify={c['status']} classes={len(classes)} inds={len(inds)} "
        f"pos={len(pos)} neg={len(neg)} sat={len(sat_classes)} eq={len(eq)} inst={len(inst_pairs)}")

    sub = lambda a, b: read_token(runner.run(oid, ["subclass", path, a, b]), "yes", "no")
    expl = lambda a, b: read_explain(runner.run(oid, ["explain", path, a, b]))
    subx = lambda a, b: read_json(runner.run(oid, ["subclass-expr", "--json", path,
                                                   f"<{a}>", f"<{b}>"]), "entailed")
    satq = lambda x: read_token(runner.run(oid, ["sat", path, x]), "sat", "unsat")
    satx = lambda ce: read_json(runner.run(oid, ["sat-expr", "--json", path, ce]), "satisfiable")

    # On an inconsistent KB only a few of each are sampled, for L5 (#428).
    if not inconsistent:
        if want("L1", "R1", "L6"):
            for a, b in pos + neg:
                q[("subclass", a, b)] = sub(a, b)
        if want("L2"):
            for g in eq:
                for a in g:
                    for b in g:
                        if a != b:
                            q[("subclass", a, b)] = sub(a, b)
        if want("L3", "R2", "L6"):
            for x in sat_classes:
                q[("sat", x)] = satq(x)
        if want("L6"):
            for x in sat_classes[:5]:
                q[("sat-expr", x)] = satx(f"<{x}>")
            for a, b in (pos + neg)[:5]:
                q[("subclass-expr", a, b)] = subx(a, b)
    else:
        for x in sat_classes:
            q[("sat-expr", x)] = satx(f"<{x}>")
        l5_pairs = (pos + neg + [(x, y) for x in sat_classes[:2] for y in sat_classes[2:4]])[:5]
        for a, b in l5_pairs:
            q[("subclass-expr", a, b)] = subx(a, b)
        if want("L5"):
            for x in [x for x in sat_classes if x not in (THING, NOTHING)][:5]:
                q[("sat", x)] = satq(x)
            for a, b in l5_pairs:
                q[("subclass", a, b)] = sub(a, b)
            # `explain` shares `subclass`'s probe but is a separate CLI path; one pair
            # covers it without doubling the per-pair cost on a slow inconsistent KB.
            for a, b in l5_pairs[:1]:
                q[("explain", a, b)] = expl(a, b)

    if want("L5", "L6", "L10") and inst_pairs:
        for cl_, a in inst_pairs:
            q[("instance", cl_, a)] = read_token(runner.run(oid, ["instance", path, cl_, a]),
                                                 "yes", "no")
        for cl_ in sorted({p[0] for p in inst_pairs})[:5]:
            r = runner.run(oid, ["instances-expr", "--json", path, f"<{cl_}>"])
            q[("instances-expr", cl_)] = read_payload(r)
    if want("L5", "L10") and inds:
        q[("realize",)] = read_payload(runner.run(oid, ["realize", "--json", path]))
    if want("L5", "L7") and len(classes) <= 150:
        q[("disjoint",)] = read_payload(runner.run(
            oid, ["disjoint", "--json", "--pair-timeout-ms", "50", path]))
    if want("L5", "L11") and inds and len(inds) <= 150:
        q[("individuals",)] = read_payload(runner.run(
            oid, ["individuals", "--json", "--pair-timeout-ms", "50", path]))

    # Follow-up operands that depend on a listed result.
    d = q.get(("disjoint",))
    if d and d["v"] == "ok" and not inconsistent:
        for a, b in d["extra"].get("disjoint_classes", [])[:caps["disj"]]:
            q[("sat-expr-and", a, b)] = satx(f"<{a}> and <{b}>")
            if ("subclass", a, b) not in q:
                q[("subclass", a, b)] = sub(a, b)
            if ("sat", a) not in q:
                q[("sat", a)] = satq(a)
    iv = q.get(("individuals",))
    if iv and iv["v"] == "ok" and not inconsistent:
        for a, b in iv["extra"].get("different_pairs", [])[:caps["disj"]]:
            q[("sat-expr-nom", a, b)] = satx(f"{{<{a}>}} and {{<{b}>}}")
    rz = q.get(("realize",))
    if rz and rz["v"] == "ok" and not inconsistent:
        typed = [(t, i["iri"]) for i in rz["extra"].get("individuals", []) for t in i["types"]
                 if t not in (THING, NOTHING)]
        rng.shuffle(typed)
        for t, a in typed[:caps["inst"]]:
            if ("instance", t, a) not in q:
                q[("instance", t, a)] = read_token(runner.run(oid, ["instance", path, t, a]),
                                                   "yes", "no")
    return obs


# --------------------------------------------------------------------------
# Laws: pure functions over an observation (unit-tested by --self-test)
# --------------------------------------------------------------------------
def check(law, onto, ok, reason, *ops):
    return outcome(law, onto, "ok" if ok else "violation", "" if ok else reason, *ops)


def unresolved(law, onto, *ops):
    why = "; ".join(f"{o['cmd'] or '?'}: {o['why']}" for o in ops if o and not definite(o))
    return outcome(law, onto, "unresolved", why or "operand missing", *ops)


def evaluate(obs):
    o, q, c = obs["onto"], obs["q"], obs.get("classify", {"status": "unknown"})
    out = []
    cl_ok = c["status"] == "ok"
    cl_op = ans("ok" if cl_ok else "unknown", c.get("why", ""), c.get("cmd"))
    clo = c.get("closure", set())
    unsat = c.get("unsat", set())
    cons = q.get(("consistent",))
    # Classify's `consistent: false` also makes the KB inconsistent for the laws. Without it,
    # a killed default `consistent` (30 s) left L3 to read every per-query `sat` that errors
    # "Inconsistent" as a violation (ore_ont_13052; HermiT and Konclude confirm it
    # inconsistent).
    kb_inconsistent = ((cons is not None and cons["v"] == "no")
                       or (cl_ok and c.get("consistent") is False))
    incons_op = cons if cons is not None and cons["v"] == "no" else cl_op

    for key, a in q.items():
        kind = key[0]
        if kind == "subclass" and not kb_inconsistent:
            s, t = key[1], key[2]
            if not cl_ok:
                continue
            if (s, t) in clo:
                law = "L2" if any(s in g and t in g for g in c.get("equiv", [])) else "L1"
                out.append(check(law, o, a["v"] == "yes",
                                 f"classify has {s} ⊑ {t}, subclass says {a['v']}", cl_op, a)
                           if definite(a) else unresolved(law, o, a))
            elif not definite(a):
                out.append(unresolved("R1", o, a))
            elif a["v"] == "yes" and s not in unsat and t not in unsat:
                # #3: under a budget cut classify may lack an entailed pair; the
                # discrepancy is still listed and HermiT-checked, but not ratcheted.
                out.append(outcome("R1", o, "discrepancy",
                                   f"subclass says {s} ⊑ {t}, classify lacks it", cl_op, a)
                           | dict(pair=[s, t], budget_cut=bool(c.get("incomplete"))))
        elif kind == "sat" and not kb_inconsistent and cl_ok:
            x = key[1]
            if x in unsat:
                out.append(check("L3", o, a["v"] == "no", f"classify unsat {x}, sat says {a['v']}",
                                 cl_op, a) if definite(a) else unresolved("L3", o, a))
            elif not definite(a):
                out.append(unresolved("R2", o, a))
            elif a["v"] == "no":
                out.append(outcome("R2", o, "discrepancy",
                                   f"sat says {x} unsat, classify lists it satisfiable", cl_op, a)
                           | dict(pair=[x, NOTHING], budget_cut=bool(c.get("incomplete"))))
        elif kind == "sat-expr" and not kb_inconsistent:
            n = q.get(("sat", key[1]))
            if n is None:
                continue
            out.append(check("L6", o, n["v"] == a["v"], f"sat={n['v']} sat-expr={a['v']}", n, a)
                       if definite(n) and definite(a) else unresolved("L6", o, n, a))
        elif kind == "subclass-expr" and not kb_inconsistent:
            n = q.get(("subclass", key[1], key[2]))
            if n is None:
                continue
            out.append(check("L6", o, n["v"] == a["v"], f"subclass={n['v']} subclass-expr={a['v']}",
                             n, a)
                       if definite(n) and definite(a) else unresolved("L6", o, n, a))
        elif kind == "instance" and not kb_inconsistent:
            cl_, ind = key[1], key[2]
            ie = q.get(("instances-expr", cl_))
            if ie is not None:
                if ie["v"] == "ok" and not ie["extra"].get("incomplete") and definite(a):
                    member = ind in ie["extra"].get("instances", [])
                    out.append(check("L6", o, (a["v"] == "yes") == member,
                                     f"instance={a['v']}, instances-expr membership={member}",
                                     a, ie))
                else:
                    out.append(unresolved("L6", o, a, ie if ie["v"] != "ok" else
                                          ans("unknown", "instances-expr gave up", ie["cmd"])))
            rz = q.get(("realize",))
            if rz is not None and rz["v"] == "ok":
                types = {i["iri"]: set(i["types"]) for i in rz["extra"].get("individuals", [])}
                if cl_ in types.get(ind, ()):
                    out.append(check("L10", o, a["v"] == "yes",
                                     f"realize types {ind} as {cl_}, instance says {a['v']}", rz, a)
                               if definite(a) else unresolved("L10", o, a))
        elif kind == "sat-expr-and":
            if definite(a):
                out.append(check("L7", o, a["v"] == "no",
                                 f"disjoint lists ({key[1]}, {key[2]}), sat-expr says satisfiable",
                                 q.get(("disjoint",)), a))
                s = q.get(("subclass", key[1], key[2]))
                sa = q.get(("sat", key[1]))
                if s is not None and definite(s) and s["v"] == "yes":
                    a_unsat = key[1] in unsat or (sa is not None and sa["v"] == "no")
                    out.append(check("L7", o, a_unsat,
                                     f"disjoint ({key[1]}, {key[2]}) but subclass says yes and "
                                     f"{key[1]} is not unsat", q.get(("disjoint",)), s, sa))
            else:
                out.append(unresolved("L7", o, a))
        elif kind in ("disjoint", "individuals", "realize") and not kb_inconsistent:
            if a["v"] != "ok":
                law = {"disjoint": "L7", "individuals": "L11", "realize": "L10"}[kind]
                out.append(unresolved(law, o, a))
        elif kind == "sat-expr-nom":
            out.append(check("L11", o, a["v"] == "no",
                             f"individuals says {key[1]} ≠ {key[2]}, sat-expr says satisfiable",
                             q.get(("individuals",)), a)
                       if definite(a) else unresolved("L11", o, a))

    # L4 / R3
    if cons is not None and cl_ok:
        if c["consistent"] is False:
            out.append(check("L4", o, cons["v"] == "no",
                             f"classify consistent:false, consistent says {cons['v']}", cl_op, cons)
                       if definite(cons) else unresolved("L4", o, cons))
        elif cons["v"] == "no" and not c.get("undetermined"):
            out.append(outcome("R3", o, "discrepancy",
                               "consistent says inconsistent, classify consistent:true",
                               cl_op, cons) | dict(pair=[THING, NOTHING]))
    # L5: guarded surfaces on an inconsistent KB
    if kb_inconsistent:
        for key, a in q.items():
            if key[0] == "consistent":
                continue
            if a["v"] in ("inconsistent", "unknown"):
                out.append(check("L5", o, True, "", incons_op, a) if a["v"] == "inconsistent"
                           else unresolved("L5", o, a))
                continue
            bad = None
            if key[0] == "instance" and a["v"] == "no":
                bad = "instance says no"
            elif key[0] == "sat" and a["v"] == "yes":
                bad = "sat says satisfiable"
            elif key[0] == "subclass" and a["v"] == "no":
                bad = "subclass says not entailed"
            elif key[0] == "explain" and a["v"] == "no":
                bad = "explain says not entailed"
            elif key[0] == "sat-expr" and a["v"] == "yes":
                bad = "sat-expr says satisfiable"
            elif key[0] == "subclass-expr" and a["v"] == "no":
                bad = "subclass-expr says not entailed"
            elif key[0] in ("instances-expr", "realize", "disjoint", "individuals"):
                bad = f"{key[0]} answered instead of erroring"
            out.append(check("L5", o, bad is None, f"inconsistent KB: {bad}", incons_op, a))
    # U1: classify unsat vs an oracle. Skipped when either side says the KB is
    # inconsistent (every class is then unsat).
    if cl_ok and c.get("consistent") and not kb_inconsistent:
        ou = obs.get("oracle_unsat")
        if ou is not None and THING not in ou:
            # One `ok` per agreeing comparison, so a run shows U1 actually compared
            # (silence would look the same as a missing oracle).
            if not unsat - ou:
                out.append(check("U1", o, True, "", cl_op))
            for x in sorted(unsat - ou):
                out.append(outcome("U1", o, "discrepancy",
                                   f"classify says {x} unsat, Konclude has it satisfiable", cl_op)
                           | dict(pair=[x, NOTHING]))
            for x in sorted(ou - unsat):
                out.append(outcome("U1", o, "unsat-missed",
                                   f"Konclude says {x} unsat, classify lists it satisfiable", cl_op))
        known = obs.get("known_sat")
        if known is not None:
            bad = sorted(unsat & known)
            if not bad:
                out.append(check("U1", o, True, "", cl_op))
            for x in bad:
                out.append(check("U1", o, False,
                                 f"classify says {x} unsat; it is satisfiable (known_sat)", cl_op))
    # L8
    so = obs.get("satonly")
    if so is not None and cl_ok:
        extra = {(s, t) for s, t in so["closure"]
                 if s not in unsat and t not in unsat} - clo
        missing_unsat = so["unsat"] - unsat
        ok = not extra and not missing_unsat
        out.append(check("L8", o, ok,
                         f"saturation-only finds {len(extra)} pair(s) / {len(missing_unsat)} unsat "
                         f"classify lacks, e.g. {sorted(extra)[:2] or sorted(missing_unsat)[:2]}",
                         cl_op, ans("ok", "", so["cmd"])))
    return out


# --------------------------------------------------------------------------
# Adjudication of ratchet discrepancies
# --------------------------------------------------------------------------
def adjudicate(outcomes, spec, hermit=None):
    oracle = None
    if spec.get("oracle") and (KONCLUDE / spec["oracle"]).exists():
        oracle = owx_closure(KONCLUDE / spec["oracle"])
    for d in outcomes:
        if d["kind"] != "discrepancy":
            continue
        s, t = d["pair"]
        if oracle is None:
            d["verdict"] = ("no Konclude oracle: HermiT decides" if hermit and d["law"] in ("R1", "R2", "U1")
                            else "no oracle: manual adjudication")
        elif d["law"] == "R1":
            d["verdict"] = "classify miss (Konclude has it)" if (s, t) in oracle[0] \
                else "candidate FP (Konclude lacks it)"
        elif d["law"] == "R2":
            d["verdict"] = "classify miss (Konclude: unsat)" if s in oracle[1] \
                else "candidate FP (Konclude: satisfiable)"
        elif d["law"] == "U1":
            d["verdict"] = "candidate FP (Konclude: satisfiable)"
        else:
            d["verdict"] = "classify miss (residual of #392)"
        if d.get("budget_cut"):
            d["verdict"] += "; classify was budget-cut (incomplete), not ratcheted"
        if hermit and (d["verdict"].startswith("candidate FP")
                       or d["verdict"].startswith("no Konclude oracle")):
            d["hermit"] = hermit(spec, d)
    return outcomes


ROBOT_JAR = next((p for p in (Path(os.environ.get("ROBOT_JAR", "/nonexistent")),
                               Path.home() / "rustdl" / "peers" / "robot.jar",
                               Path.home() / "peers" / "robot.jar") if p.exists()),
                 Path("/nonexistent"))
HERMIT_CAP_S = 600
_HERMIT_MEMO = {}


def hermit_missing():
    """Why `--hermit` cannot run, or None (#502: never a silent "unavailable")."""
    java = java_bin()
    if not java.exists():
        return f"no JVM at {java} (set JAVA_HOME)"
    if not ROBOT_JAR.exists():
        return "no robot.jar (set ROBOT_JAR)"
    return None


def hermit_cli(path, args):
    """One HermiT CommandLine query (robot.jar's `org.semanticweb.HermiT.cli`). Returns
    (status, stdout): status is ok, inconsistent, timeout or fail."""
    key = (hashlib.sha256(Path(path).read_bytes()).hexdigest(), tuple(args))
    if key in _HERMIT_MEMO:
        return _HERMIT_MEMO[key]
    try:
        p = subprocess.run([str(java_bin()), "-Xmx4g", "-Dfile.encoding=UTF-8", "-cp",
                            str(ROBOT_JAR), "org.semanticweb.HermiT.cli.CommandLine", *args,
                            str(path)], capture_output=True, text=True, timeout=HERMIT_CAP_S)
    except subprocess.TimeoutExpired:
        return ("timeout", "")  # not memoised: a timeout under load is not an answer
    log = p.stdout + p.stderr
    if "InconsistentOntologyException" in log:
        r = ("inconsistent", "")
    # stderr only: HermiT echoes the queried IRI and every super on stdout, so a class
    # named `…#Error` must not read as a failure (Fable, #503).
    elif p.returncode != 0 or re.search(r"Exception|Error", p.stderr):
        r = ("fail", log[-300:])
    else:
        r = ("ok", p.stdout)
    _HERMIT_MEMO[key] = r
    return r


def _expand(name, pfx):
    """HermiT prints names with the ontology's own prefixes (`pizza:Food`). `pfx` comes
    from the file's OFN `Prefix(` lines: every input here is functional syntax (an
    RDF/XML input would need its xmlns map)."""
    name = name.strip()
    if name.startswith("<") and name.endswith(">"):
        return name[1:-1]
    p, sep, local = name.partition(":")
    if sep and p in pfx and not local.startswith("//"):
        return pfx[p] + local
    return name


def hermit_check(spec, d, cli=hermit_cli):
    """Check one rustdl claim with HermiT, per claim (#502). Returns 'refutes' (the claim
    is false: a false positive), 'confirms' (it holds: a classify miss), or
    'unavailable (why)', which `summarize` lists as unadjudicated and fails the run on.

    The claim: R2 / U1 `s` is unsat; R1 `s ⊑ t`. Per-claim queries replace the old
    whole-file `robot reason`, which writes nothing on an ontology with an unsat class
    and so could never adjudicate an R1 claim there (pizza has two)."""
    path, (s, t), law = spec["path"], d["pair"], d["law"]
    st, out = cli(path, [f"--consistency=<{s}>"])
    if st == "inconsistent":
        return "unavailable (HermiT: the ontology is inconsistent)"
    if st != "ok":
        return f"unavailable (HermiT {st})"
    if " is not satisfiable." in out:
        return "confirms"
    if " is satisfiable." not in out:
        return f"unavailable (HermiT output {out[-80:]!r})"
    if law in ("R2", "U1"):
        return "refutes"
    if t == THING:
        return "confirms"
    # `--supers` lists strict super-classes only; a class equivalent to `s` is listed by
    # `--equivalents` (Fable, #503: `s ≡ t` was refuted).
    pfx = dict(_OFN_PFX.findall(Path(path).read_text(errors="replace")))
    pfx.setdefault("owl", OWL)
    names = set()
    for flag, head in (("--supers", "All super-classes"), ("--equivalents", "Classes equivalent")):
        st, out = cli(path, [f"{flag}=<{s}>"])
        if st != "ok":
            return f"unavailable (HermiT {st})"
        lines = out.splitlines()
        if not lines or not lines[0].startswith(head):
            return f"unavailable (HermiT output {out[:80]!r})"
        names |= {_expand(x, pfx) for x in lines[1:] if x.strip()}
    return "confirms" if t in names else "refutes"


# `ofn_named_closure` lives in owl_readers.py.


# --------------------------------------------------------------------------
# Self-test: each law fires on fabricated operands; unknowns never violate
# --------------------------------------------------------------------------
def self_test():
    Y, N, U = ans("yes", cmd="x"), ans("no", cmd="x"), ans("unknown", "killed", "x")
    I = ans("inconsistent", "error Inconsistent", "x", 1)
    base = lambda **kw: dict(onto="t", sample={}, classify=dict(
        status="ok", consistent=True, undetermined=False, unsat=set(), equiv=[],
        closure=set(), cmd="classify") | kw)

    def laws_of(obs, kind):
        return sorted(o["law"] for o in evaluate(obs) if o["kind"] == kind)

    cases = []
    # L1
    ob = dict(onto="t", q={("subclass", "A", "B"): N}, classify=base(closure={("A", "B")})["classify"])
    cases.append(("L1 fires", laws_of(ob, "violation"), ["L1"]))
    ob["q"] = {("subclass", "A", "B"): U}
    cases.append(("L1 unknown -> unresolved", laws_of(ob, "unresolved") + laws_of(ob, "violation"), ["L1"]))
    # L2
    ob = dict(onto="t", q={("subclass", "A", "B"): N},
              classify=base(closure={("A", "B"), ("B", "A")}, equiv=[["A", "B"]])["classify"])
    cases.append(("L2 fires", laws_of(ob, "violation"), ["L2"]))
    # L3
    ob = dict(onto="t", q={("sat", "C"): Y}, classify=base(unsat={"C"})["classify"])
    cases.append(("L3 fires", laws_of(ob, "violation"), ["L3"]))
    # L4
    ob = dict(onto="t", q={("consistent",): Y}, classify=base(consistent=False)["classify"])
    cases.append(("L4 fires", laws_of(ob, "violation"), ["L4"]))
    # L5
    ob = dict(onto="t", q={("consistent",): N, ("instance", "C", "a"): N, ("sat-expr", "C"): I},
              classify=base(consistent=False)["classify"])
    cases.append(("L5 fires", laws_of(ob, "violation"), ["L5"]))
    ob = dict(onto="t", q={("consistent",): N, ("sat", "C"): Y}, classify=base(consistent=False)["classify"])
    cases.append(("L5 fires on sat", laws_of(ob, "violation"), ["L5"]))
    ob = dict(onto="t", q={("consistent",): N, ("subclass", "C", "D"): N},
              classify=base(consistent=False)["classify"])
    cases.append(("L5 fires on subclass", laws_of(ob, "violation"), ["L5"]))
    # Default `consistent` killed, classify consistent:false (ore_ont_13052).
    ob = dict(onto="t", q={("consistent",): U, ("sat", "C"): I},
              classify=base(consistent=False, unsat={"C"})["classify"])
    cases.append(("killed consistent + classify inconsistent: sat inconsistent is no violation",
                  laws_of(ob, "violation"), []))
    cases.append(("killed consistent + classify inconsistent: L5 ok", laws_of(ob, "ok"), ["L5"]))
    cases.append(("killed consistent + classify inconsistent: L5's operand is classify",
                  [x["operands"][0]["cmd"] for x in evaluate(ob) if x["law"] == "L5"],
                  ["classify"]))
    ob = dict(onto="t", q={("consistent",): U, ("sat", "C"): Y},
              classify=base(consistent=False, unsat={"C"})["classify"])
    cases.append(("killed consistent + classify inconsistent: sat yes fires L5",
                  laws_of(ob, "violation"), ["L5"]))
    ob = dict(onto="t", q={("consistent",): N, ("sat", "C"): I, ("subclass", "C", "D"): Y},
              classify=base(consistent=False)["classify"])
    cases.append(("L5 sat error / subclass yes -> no violation", laws_of(ob, "violation"), []))
    ob = dict(onto="t", q={("consistent",): N, ("explain", "C", "D"): N},
              classify=base(consistent=False)["classify"])
    cases.append(("L5 fires on explain", laws_of(ob, "violation"), ["L5"]))
    ob = dict(onto="t", q={("consistent",): N, ("explain", "C", "D"): I},
              classify=base(consistent=False)["classify"])
    cases.append(("L5 explain error -> no violation", laws_of(ob, "violation"), []))
    fake = dict(killed=False, rc=0, stderr="", stdout="http://x#C ⊑ http://x#D : no — answered by tableau\n",
                args=["explain", "f", "C", "D"])
    cases.append(("read_explain parses no", [read_explain(fake)["v"]], ["no"]))
    # L6
    ob = dict(onto="t", q={("sat", "C"): Y, ("sat-expr", "C"): N}, classify=base()["classify"])
    cases.append(("L6 fires", laws_of(ob, "violation"), ["L6"]))
    ob["q"][("sat-expr", "C")] = U
    cases.append(("L6 unknown -> no violation", laws_of(ob, "violation"), []))
    # L7
    dj = ans("ok", cmd="disjoint", extra={"disjoint_classes": [["A", "B"]]})
    ob = dict(onto="t", q={("disjoint",): dj, ("sat-expr-and", "A", "B"): Y},
              classify=base()["classify"])
    cases.append(("L7 fires", laws_of(ob, "violation"), ["L7"]))
    # L8
    ob = dict(onto="t", q={}, classify=base()["classify"],
              satonly=dict(closure={("A", "B")}, unsat=set(), cmd="so"))
    cases.append(("L8 fires", laws_of(ob, "violation"), ["L8"]))
    # L10
    rz = ans("ok", cmd="realize", extra={"individuals": [{"iri": "a", "types": ["C"]}]})
    ob = dict(onto="t", q={("realize",): rz, ("instance", "C", "a"): N}, classify=base()["classify"])
    cases.append(("L10 fires", laws_of(ob, "violation"), ["L10"]))
    # L11
    ob = dict(onto="t", q={("sat-expr-nom", "a", "b"): Y}, classify=base()["classify"])
    cases.append(("L11 fires", laws_of(ob, "violation"), ["L11"]))
    # Ratchets
    ob = dict(onto="t", q={("subclass", "A", "B"): Y}, classify=base()["classify"])
    cases.append(("R1 discrepancy", laws_of(ob, "discrepancy"), ["R1"]))
    ob = dict(onto="t", q={("sat", "C"): N}, classify=base()["classify"])
    cases.append(("R2 discrepancy", laws_of(ob, "discrepancy"), ["R2"]))
    ob = dict(onto="t", q={("consistent",): N}, classify=base()["classify"])
    cases.append(("R3 discrepancy", laws_of(ob, "discrepancy"), ["R3"]))
    ob = dict(onto="t", q={("consistent",): N}, classify=base(undetermined=True)["classify"])
    cases.append(("R3 not on consistency_undetermined", laws_of(ob, "discrepancy"), []))
    # Agreeing operands produce no violation anywhere.
    ob = dict(onto="t", q={("subclass", "A", "B"): Y, ("sat", "C"): N, ("sat-expr", "C"): N,
                           ("consistent",): Y},
              classify=base(closure={("A", "B")}, unsat={"C"})["classify"])
    cases.append(("agreement is clean", laws_of(ob, "violation") + laws_of(ob, "discrepancy"), []))
    # Closure: equivalence cycles are closed for every member (closure-diff.py's bug).
    cl = closure([("A", "B"), ("B", "C")], [["C", "D"]])
    cases.append(("closure through a cycle", sorted(t for s, t in cl if s == "A"), ["B", "C", "D"]))

    # A HermiT-refuted per-pair claim is a confirmed FP; a confirmed claim is not.
    res = [dict(onto="t", outcomes=[
        dict(law="R1", kind="discrepancy", reason="", operands=[], hermit="refutes"),
        dict(law="R1", kind="discrepancy", reason="", operands=[], hermit="confirms")])]
    cases.append(("HermiT refutation fails the run",
                  [f["kind"] for f in summarize(res, None)[1]], ["HermiT-confirmed FP"]))
    # Ratchet: a count above baseline is reported, at or below is not.
    res = [dict(onto="t", outcomes=[dict(law="R2", kind="discrepancy", reason="", operands=[])] * 2)]
    cases.append(("ratchet over baseline", [o["count"] for o in summarize(res, {"t": {"R2": 1}})[3]], [2]))
    cases.append(("ratchet at baseline", summarize(res, {"t": {"R2": 2}})[3], []))
    # robot's prefixed output parses to full IRIs.
    doc = "Prefix(:=<http://e#>)\nOntology(\nSubClassOf(:A :B)\nEquivalentClasses(:B :C)\nSubClassOf(:D owl:Nothing)\n)"
    clo_, uns_ = ofn_named_closure(doc.replace("owl:Nothing", "<" + NOTHING + ">"))
    cases.append(("ofn closure", (sorted(clo_), sorted(uns_)),
                  ([("http://e#A", "http://e#B"), ("http://e#A", "http://e#C"),
                    ("http://e#B", "http://e#C"), ("http://e#C", "http://e#B")], ["http://e#D"])))

    # Readers, on records shaped like Runner.run's.
    rec = lambda rc, out="", err="", killed=False: dict(
        args=["sat", "f", "C"], env={}, rc=rc, killed=killed, stdout=out, stderr=err)
    cases.append(("reader: sat", read_token(rec(0, "sat\n"), "sat", "unsat")["v"], "yes"))
    cases.append(("reader: unsat", read_token(rec(0, "unsat\n"), "sat", "unsat")["v"], "no"))
    cases.append(("reader: exit 3", read_token(rec(3, "unknown\n"), "sat", "unsat")["v"], "unknown"))
    cases.append(("reader: killed", read_token(rec(None, killed=True), "sat", "unsat")["v"], "unknown"))
    cases.append(("reader: Inconsistent error",
                  read_token(rec(1, err="Error: x\nCaused by:\n    ontology is inconsistent; every"),
                             "sat", "unsat")["v"], "inconsistent"))
    cases.append(("reader: other error mentioning inconsistent",
                  read_token(rec(1, err="Error: inconsistent role hierarchy"), "sat", "unsat")["v"],
                  "unknown"))
    cases.append(("reader: json give-up",
                  read_json(rec(3, '{"incomplete": true, "satisfiable": true}'), "satisfiable")["v"],
                  "unknown"))
    cases.append(("reader: json definite",
                  read_json(rec(0, '{"incomplete": false, "satisfiable": false}'), "satisfiable")["v"],
                  "no"))
    cases.append(("reader: payload kept on give-up",
                  read_payload(rec(0, '{"incomplete": true, "instances": ["a"]}'))["extra"]["instances"],
                  ["a"]))
    # Unknown operands are recorded, never dropped.
    ob = dict(onto="t", q={("subclass", "A", "B"): U, ("sat", "C"): U,
                           ("disjoint",): U, ("individuals",): U, ("realize",): U},
              classify=base()["classify"])
    cases.append(("unknowns recorded as unresolved", laws_of(ob, "unresolved"),
                  ["L10", "L11", "L7", "R1", "R2"]))
    # A budget-cut classify: R1 listed but not ratcheted.
    ob = dict(onto="t", q={("subclass", "A", "B"): Y}, classify=base(incomplete=True)["classify"])
    res = [dict(onto="t", outcomes=evaluate(ob))]
    cases.append(("budget-cut R1 not ratcheted",
                  (laws_of(ob, "discrepancy"), dict(summarize(res, {})[2])), (["R1"], {})))
    # ... but a HermiT-refuted per-pair yes is an FP whatever classify's budget did.
    res[0]["outcomes"] = [dict(o, hermit="refutes") for o in res[0]["outcomes"]]
    cases.append(("budget-cut HermiT-refuted R1 still fails",
                  [f["kind"] for f in summarize(res, {})[1]], ["HermiT-confirmed FP"]))

    # U1: classify unsat vs the oracle's unsat set.
    ob = dict(onto="t", q={}, classify=base(unsat={"A", "B"})["classify"], oracle_unsat={"A", "C"})
    cases.append(("U1 false unsat is a discrepancy", laws_of(ob, "discrepancy"), ["U1"]))
    cases.append(("U1 lists an oracle unsat classify misses", laws_of(ob, "unsat-missed"), ["U1"]))
    ob["oracle_unsat"] = {"A", "B"}
    cases.append(("U1 agreement is one ok, no discrepancy",
                  (laws_of(ob, "ok"), laws_of(ob, "discrepancy")), (["U1"], [])))
    ob["oracle_unsat"] = {THING, "A"}
    cases.append(("U1 skipped on an inconsistent oracle", laws_of(ob, "discrepancy"), []))
    ob = dict(onto="t", q={}, classify=base(consistent=False, unsat={"A"})["classify"],
              oracle_unsat=set())
    cases.append(("U1 skipped when classify says inconsistent", laws_of(ob, "discrepancy"), []))
    ob = dict(onto="t", q={}, classify=base(unsat={"C"})["classify"], known_sat={"C", "D"})
    cases.append(("U1 known_sat class unsat is a violation", laws_of(ob, "violation"), ["U1"]))
    # A deadline never makes a class unsat, so U1 is ratcheted even when budget-cut.
    ob = dict(onto="t", q={}, classify=base(unsat={"B"}, incomplete=True)["classify"],
              oracle_unsat=set())
    res = [dict(onto="t", outcomes=evaluate(ob))]
    cases.append(("budget-cut U1 still ratcheted", dict(summarize(res, {})[2]), {("t", "U1"): 1}))
    # The OWLAPI form of an unsat class in an oracle file.
    import tempfile
    with tempfile.NamedTemporaryFile("w", suffix=".owx", delete=False) as f:
        f.write(f'<SubClassOf><Class IRI="http://e/C"/><Class IRI="{NOTHING}"/></SubClassOf>'
                f'<EquivalentClasses><Class IRI="{NOTHING}"/><Class IRI="http://e/D"/>'
                '</EquivalentClasses>'
                # ROBOT's relative form.
                '<SubClassOf><Class IRI="http://e/E"/><Class IRI="Nothing"/></SubClassOf>')
    cases.append(("owx_closure reads both unsat forms", sorted(owx_closure(f.name)[1]),
                  ["http://e/C", "http://e/D", "http://e/E"]))
    os.unlink(f.name)

    bad = 0
    # #493: exit 3 + #449's note after a definite answer is that answer with no_verdict;
    # exit 3 without the note, or `unknown`, stays a give-up.
    def run(rc, out, err=""):
        return dict(killed=False, rc=rc, stdout=out, stderr=err, args=["rustdl", "subclass"])
    note = "warning: ... (NoVerdict). This is likely a reasoner bug; please report the input."
    a = read_token(run(3, "yes\n", note), "yes", "no")
    cases.append(("#493 yes + note + rc 3 is yes with no_verdict", (a["v"], no_verdict(a)),
                  ("yes", True)))
    a = read_token(run(3, "unknown\n", note), "yes", "no")
    cases.append(("#493 unknown + note stays unknown", (a["v"], no_verdict(a)), ("unknown", False)))
    a = read_token(run(3, "no\n", "warning: the instance probe was cut"), "yes", "no")
    cases.append(("#493 rc 3 without the note stays a give-up", a["v"], "unknown"))
    a = read_token(run(0, "yes\n"), "yes", "no")
    cases.append(("#493 a clean yes has no no_verdict", (a["v"], no_verdict(a)), ("yes", False)))
    a = read_explain(run(3, "A ⊑ B : yes — answered by tableau\n", note))
    cases.append(("#493 explain yes + note", (a["v"], no_verdict(a)), ("yes", True)))
    a = read_explain(run(3, "A ⊑ B : unknown — the tableau reached an internal limit\n", note))
    cases.append(("#493 explain unknown + note stays unknown", a["v"], "unknown"))
    payload = '{\n  "no_verdict_pairs": 1,\n  "consistent": true\n}\n'
    a = read_payload(run(3, payload, note))
    cases.append(("#493 a JSON payload with rc 3 + note stays a give-up (Fable, #500)",
                  a["v"], "unknown"))
    a = read_payload(run(0, '{"no_verdict_pairs": 2}\n'))
    cases.append(("#493 no_verdict() reads classify's no_verdict_pairs", no_verdict(a), True))
    # #502: per-claim HermiT verdicts, from canned CommandLine output.
    def fake(**answers):
        return lambda path, args: answers.get(args[0].split("=")[0], ("fail", ""))
    import tempfile
    _pfx_file = Path(tempfile.mkdtemp()) / "p.ofn"  # hermit_check reads only its Prefix lines
    _pfx_file.write_text("Prefix(:=<http://e#>)\nOntology(<http://e>)\n")
    sp = dict(id="t", path=_pfx_file)
    sat_ = ("ok", "http://e#A is satisfiable.\n")
    cases.append(("#502 HermiT: R2 on a satisfiable class refutes",
                  hermit_check(sp, dict(law="R2", pair=["http://e#A", NOTHING]),
                               fake(**{"--consistency": sat_})), "refutes"))
    cases.append(("#502 HermiT: R2 on an unsat class confirms",
                  hermit_check(sp, dict(law="R2", pair=["http://e#A", NOTHING]),
                               fake(**{"--consistency": ("ok", "x is not satisfiable.\n")})),
                  "confirms"))
    sup = ("ok", "All super-classes of 'http://e#A':\n\towl:Thing\n\t<http://e#B>\n")
    # HermiT's real format (Fable, #503): the doubled-bracket header and prefixed members.
    eqv = ("ok", "Classes equivalent to '<<http://e#A>>':\n\t:Q\n\t:A\n")
    cases.append(("#502 HermiT: R1 to a listed superclass confirms",
                  hermit_check(sp, dict(law="R1", pair=["http://e#A", "http://e#B"]),
                               fake(**{"--consistency": sat_, "--supers": sup, "--equivalents": eqv})), "confirms"))
    cases.append(("#502 HermiT: R1 to an unlisted class refutes",
                  hermit_check(sp, dict(law="R1", pair=["http://e#A", "http://e#C"]),
                               fake(**{"--consistency": sat_, "--supers": sup, "--equivalents": eqv})), "refutes"))
    cases.append(("#503 HermiT: R1 to an equivalent class confirms (--supers omits it)",
                  hermit_check(sp, dict(law="R1", pair=["http://e#A", "http://e#Q"]),
                               fake(**{"--consistency": sat_, "--supers": sup,
                                       "--equivalents": eqv})), "confirms"))
    cases.append(("#502 HermiT: a timeout is unavailable, never a verdict",
                  hermit_check(sp, dict(law="R1", pair=["http://e#A", "http://e#C"]),
                               fake(**{"--consistency": ("timeout", "")})).startswith("unavailable"),
                  True))
    cases.append(("#502 HermiT prints prefixed names",
                  [_expand(x, {"pizza": "http://p#", "owl": OWL})
                   for x in ("pizza:Food", "owl:Thing", "<http://q#A>", "http://r#B")],
                  ["http://p#Food", THING, "http://q#A", "http://r#B"]))
    res = [dict(onto="t", outcomes=[
        dict(law="R1", kind="discrepancy", reason="", operands=[], hermit="unavailable (x)"),
        dict(law="R2", kind="discrepancy", reason="", operands=[], hermit="confirms"),
        dict(law="R2", kind="discrepancy", reason="", operands=[])])]
    cases.append(("#502 an undecided HermiT answer is listed as unadjudicated",
                  [u["law"] for u in summarize(res, None)[4]], ["R1"]))
    cases.append(("#502 java_bin: JAVA_HOME first, else the peers JDK",
                  [str(java_bin({"JAVA_HOME": "/j"})), java_bin({}).parts[-4:]],
                  ["/j/bin/java", ("peers", "jdk-17.0.20.1+1", "bin", "java")]))
    saved_env = os.environ.get("JAVA_HOME")
    os.environ["JAVA_HOME"] = "/nonexistent-jdk"
    cases.append(("#502 a missing JVM is reported, not answered",
                  (hermit_missing() or "").startswith("no JVM"), True))
    if saved_env is None:
        os.environ.pop("JAVA_HOME")
    else:
        os.environ["JAVA_HOME"] = saved_env
    for name, got, want in cases:
        ok = got == want
        bad += not ok
        print(f"{'ok  ' if ok else 'FAIL'} {name}: got {got}, want {want}")
    print(f"self-test: {len(cases) - bad}/{len(cases)} passed")
    return 1 if bad else 0


# --------------------------------------------------------------------------
# Driver
# --------------------------------------------------------------------------
def corpus(args):
    specs = [s for s in CURATED + SYNTHETIC if Path(s["path"]).exists()]
    missing = [s["id"] for s in CURATED + SYNTHETIC if not Path(s["path"]).exists()]
    ore = list(args.ore or [])
    if args.ore_list:
        pool = Path(args.ore_pool).expanduser()
        for line in Path(args.ore_list).read_text().splitlines():
            stem = line.split("#", 1)[0].strip()
            if stem:
                ore.append(str(pool / f"{stem}.owl"))
    for f in ore:
        p = Path(f)
        if not p.exists():
            missing.append(p.stem)
            continue
        specs.append(dict(id=p.stem, path=p, caps=dict(pos=20, neg=20, sat=10, inst=10, disj=10,
                                                         eq=5)))
    if args.quick:
        synth = {s_["id"] for s_ in SYNTHETIC}
        specs = [dict(s, caps=QUICK_CAPS) if s["id"] not in synth
                 else s for s in specs if s["id"] in QUICK_IDS]
    if args.only:
        specs = [s for s in specs if s["id"] in args.only.split(",")]
    return specs, missing


def _pin_worker(cores_q):
    """Pool initializer: pin THIS worker (and the rustdl processes it starts) to
    one core, so no two workers share a core whatever job order the pool picks."""
    core = cores_q.get()
    if core is not None:
        os.sched_setaffinity(0, {core})


def run_one(job):
    spec, binary, core, outdir, seed, env_extra = job
    tag = "-".join(f"{k}={v}" for k, v in sorted(env_extra.items())) or "default"
    # The cache is per binary: a new pin never resumes another binary's answers.
    sha12 = hashlib.sha256(Path(binary).read_bytes()).hexdigest()[:12]
    runner = Runner(binary, core, outdir / f"calls-{spec['id']}-{tag}-{sha12}.jsonl", env_extra)
    t0 = time.monotonic()
    caps = dict(DEFAULT_CAPS, **spec.get("caps", {}))
    obs = observe(spec, runner, caps, seed, log=lambda *a: print(*a, flush=True))
    outs = adjudicate(evaluate(obs), spec)
    calls = list(runner.cache.values())
    return dict(onto=spec["id"], outcomes=outs, wall=round(time.monotonic() - t0, 1),
                calls=len(calls), kills=sum(r["killed"] for r in calls),
                cpu=round(sum(r["cpu"] for r in calls), 1))


def summarize(results, baseline):
    by_law = defaultdict(Counter)
    fails, ratchet, unadj = [], Counter(), []
    for res in results:
        for o in res["outcomes"]:
            by_law[o["law"]][o["kind"]] += 1
            if o["kind"] == "violation":
                fails.append(o)
            if o["kind"] == "discrepancy" and not o.get("budget_cut"):
                ratchet[(res["onto"], o["law"])] += 1
            # hermit_check reports whether HermiT CONFIRMS rustdl's per-pair claim;
            # a refuted claim is a false positive of the per-pair surface, whatever
            # classify's budget did, so it fails the run even when budget-cut.
            if o["kind"] == "discrepancy" and o.get("hermit") == "refutes":
                fails.append(dict(o, kind="HermiT-confirmed FP"))
            # #502: HermiT was asked and did not decide. Listed and failing the run, so a
            # run where the oracle never answered cannot read as zero false positives.
            if o["kind"] == "discrepancy" and "hermit" in o \
                    and o["hermit"] not in ("refutes", "confirms"):
                unadj.append(o | {"onto": res["onto"]})
    over = []
    for (onto, law), n in sorted(ratchet.items()):
        allowed = baseline.get(onto, {}).get(law, 0) if baseline is not None else None
        if allowed is not None and n > allowed:
            over.append(dict(onto=onto, law=law, count=n, baseline=allowed))
    return by_law, fails, ratchet, over, unadj


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--self-test", action="store_true")
    ap.add_argument("--bin")
    ap.add_argument("--out", type=Path)
    ap.add_argument("--quick", action="store_true")
    ap.add_argument("--only", help="comma-separated ontology ids")
    ap.add_argument("--ore", nargs="*", help="extra ORE ontology files")
    ap.add_argument("--ore-list", help="file of ORE stems (one per line, # comments)")
    ap.add_argument("--ore-pool", default=os.environ.get(
        "RUSTDL_ORE_POOL", "~/rustdl/corpus/ore-run/pool_sample/files"))
    ap.add_argument("--cores", default="",
                    help="comma-separated cores to pin workers to (taskset); empty = no pinning")
    ap.add_argument("--workers", type=int, default=3, help="parallel ontologies when unpinned")
    ap.add_argument("--seed", default="363")
    ap.add_argument("--hermit", action="store_true", help="confirm candidate FPs with HermiT")
    ap.add_argument("--baseline", type=Path)
    ap.add_argument("--write-baseline", type=Path)
    ap.add_argument("--sabotage", action="store_true", help="run the engine-level sabotages")
    ap.add_argument("--env", action="append", default=[], metavar="K=V",
                    help="set K=V for every rustdl call (part of the cache key and file tag)")
    a = ap.parse_args()
    if a.self_test:
        return self_test()
    if not a.bin or not a.out:
        ap.error("--bin and --out are required (or --self-test)")
    if a.hermit and hermit_missing():
        # #502: this used to mark every candidate "unavailable" and pass the run.
        ap.error(f"--hermit cannot run: {hermit_missing()}")
    a.out.mkdir(parents=True, exist_ok=True)
    binary = str(Path(a.bin).resolve())
    sha = hashlib.sha256(Path(binary).read_bytes()).hexdigest()
    version = subprocess.run([binary, "--version"], capture_output=True, text=True).stdout.strip()
    cores = [int(c) for c in a.cores.split(",") if c] or [None] * a.workers
    specs, missing = corpus(a)

    if a.sabotage and a.env:
        ap.error("--env does not apply to --sabotage (each sabotage sets its own env)")
    if a.sabotage:
        allspecs = {s["id"]: s for s in CURATED + SYNTHETIC}
        rows, bad = [], 0
        for sb in SABOTAGES:
            spec = allspecs[sb["onto"]]
            if not Path(spec["path"]).exists():
                rows.append(dict(sb, result="fixture missing"))
                continue
            spec = dict(spec, laws={sb["expect"]})
            res = run_one((spec, binary, cores[0], a.out, a.seed, sb["env"]))
            ctl = run_one((spec, binary, cores[0], a.out, a.seed, {}))
            hit = sum(o["law"] == sb["expect"] and o["kind"] == "discrepancy" for o in res["outcomes"])
            base = sum(o["law"] == sb["expect"] and o["kind"] == "discrepancy" for o in ctl["outcomes"])
            fired = hit > base
            bad += not fired
            rows.append(dict(name=sb["name"], sabotaged=hit, control=base,
                             result="FIRES" if fired else "DOES NOT FIRE"))
            print(f"{rows[-1]['result']}: {sb['name']} (sabotaged {hit}, control {base})")
        (a.out / "sabotage.json").write_text(json.dumps(rows, indent=1))
        return 1 if bad else 0

    import multiprocessing
    from concurrent.futures import ProcessPoolExecutor
    env_extra = dict(kv.split("=", 1) for kv in a.env)
    jobs = [(s, binary, None, a.out, a.seed, env_extra) for s in specs]
    cores_q = multiprocessing.get_context().Queue()
    for c_ in cores:
        cores_q.put(c_)
    print(f"binary {binary} sha256 {sha[:12]} {version}; {len(specs)} ontologies on cores {cores}"
          + (f"; missing fixtures: {missing}" if missing else ""), flush=True)
    t0 = time.monotonic()
    results = []
    with ProcessPoolExecutor(max_workers=len(cores), initializer=_pin_worker,
                             initargs=(cores_q,)) as ex:
        for res in ex.map(run_one, jobs):
            results.append(res)
            print(f"done {res['onto']}: {res['calls']} calls, {res['kills']} kills, "
                  f"cpu {res['cpu']}s, wall {res['wall']}s", flush=True)
    baseline = json.loads(a.baseline.read_text()) if a.baseline else None
    if a.hermit:
        allspecs = {s["id"]: s for s in specs}
        for res in results:
            adjudicate(res["outcomes"], allspecs[res["onto"]], hermit=hermit_check)
    by_law, fails, ratchet, over, unadj = summarize(results, baseline)
    report = dict(binary=binary, sha256=sha, version=version, seed=a.seed, quick=a.quick,
                  env=env_extra,
                  wall=round(time.monotonic() - t0, 1), missing_fixtures=missing,
                  per_ontology=[{k: r[k] for k in ("onto", "calls", "kills", "cpu", "wall")}
                                for r in results],
                  by_law={k: dict(v) for k, v in sorted(by_law.items())},
                  failures=fails, ratchet_over_baseline=over, unadjudicated=unadj,
                  discrepancies=[o | {"onto": r["onto"]} for r in results for o in r["outcomes"]
                                 if o["kind"] == "discrepancy"],
                  unresolved=[o for r in results for o in r["outcomes"] if o["kind"] == "unresolved"],
                  unsat_missed=[o | {"onto": r["onto"]} for r in results for o in r["outcomes"]
                                if o["kind"] == "unsat-missed"])
    (a.out / "report.json").write_text(json.dumps(report, indent=1, default=list))
    if a.write_baseline:
        bl = defaultdict(dict)
        for (onto, law), n in ratchet.items():
            bl[onto][law] = n
        a.write_baseline.write_text(json.dumps(dict(sorted(bl.items())), indent=1, sort_keys=True) + "\n")
    print("\nlaw    ok  violation  unresolved  discrepancy")
    for law in sorted(by_law):
        k = by_law[law]
        print(f"{law:5} {k['ok']:4} {k['violation']:10} {k['unresolved']:11} {k['discrepancy']:12}")
    for f in fails:
        print(f"FAIL {f['law']} {f['onto']}: {f['reason']}")
        for op in f["operands"]:
            print(f"     {op['v']:>12}  {op['cmd']}  {op['why']}")
    for ov in over:
        print(f"RATCHET {ov['onto']} {ov['law']}: {ov['count']} > baseline {ov['baseline']}")
    for u in unadj:
        print(f"UNADJUDICATED {u['law']} {u['onto']}: {u['reason']} -- {u['hermit']}")
    n_disc = sum(o["kind"] == "discrepancy" and "hermit" not in o
                 for r in results for o in r["outcomes"])
    if n_disc:
        print(f"note: {n_disc} discrepancies not sent to HermiT"
              + ("" if a.hermit else " (no --hermit)"))
    print(f"report: {a.out / 'report.json'}")
    return 1 if fails or over or unadj else 0


if __name__ == "__main__":
    sys.exit(main())
