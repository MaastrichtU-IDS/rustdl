#!/usr/bin/env python3
"""perquery.py: the per-query CLI surface on the generated net (#363 R7).

`subclass`, `sat` and `explain` answer one question per process, on a path `classify` does not
take while the wedge's `Sat` is trusted (#76 lived there for months; #457 and #460 too). This
asks every ordered pair and every class of each generated case through those commands, and
checks the answers against the same binary's `classify` and against the cached Konclude ∩
HermiT oracle (`oracle.py`, agreement only, so a refuted pair is definite).

It extends, and does not duplicate:
  * `crates/owl-dl-reasoner/tests/per_pair_fp_gate.rs`: that gate audits the per-pair tableau by
    proxy (`classify` under RUSTDL_CLASSIFY_VERIFY_REFUTATIONS=1) on 4 curated ontologies. Its
    proxy is the `vr` route here, on 4000 shapes;
  * `scripts/cross-command-laws.py`: that harness runs these commands on the curated corpus,
    sampled, against Konclude with HermiT on demand, with R1/R2 as ratchets. Its operand readers
    are reused here; on the generated cases the oracle is definite, so the FP laws are hard.
Plan: notes/plans/2026-11-06-363-r7-per-query-surface.md (Fable: go with changes, all taken).

Per case: `full` classify (--pair-timeout-ms 1000) and `vr` (the same with
RUSTDL_CLASSIFY_VERIFY_REFUTATIONS=1), `subclass A B` on every ordered pair of declared classes,
`sat C` on every declared class, `explain A B` on 3 seeded pairs (a classify positive when there
is one, a negative, a random one; every pair on an --extra-case). Per-query calls are capped on CPU time (RLIMIT_CPU soft
CPU_CAP, hard CPU_CAP + 2) with a WALL_CAP backstop; after a command's first capped call on a
case, its remaining calls there are skipped ("after-hang").

Laws (outcomes: ok / violation / info / unresolved / known):
  R7.0.vr   R2.3's oracle FP check on the `vr` classify route, given-up runs included.
  R7.0.full the same on `full` (reported for contrast: the proxy exists because `full` is blind).
  R7.0c     on el_only / el_guaranteed (where the flag is inert): `vr` answer = `full` answer.
  R7.1      FP: `subclass` / `explain` = yes on an oracle-agreed consistent KB where the oracle
            refutes A ⊑ B (A not oracle-unsat, B not oracle-Thing-equivalent, (A,B) not in its
            pairs). Hard.
  R7.2      false unsat: `sat C` = unsat on an agreed consistent KB where C is oracle-sat. Hard.
  R7.3      agrees with classify. Classify implies A ⊑ B iff (A,B) is in its closure, A is
            classify-unsat or B classify-Thing-equivalent; it implies C unsat iff C is listed.
            dir 1: classify (status ok, consistent) implies it, the command says no / sat:
                   violation (rows are sound under a cut; the give-up flags go in the detail).
            dir 2: the command says yes / unsat, classify lacks it: the oracle decides. Refuted
                   is R7.1/R7.2; confirmed is a violation (D10) when classify had no give-up
                   and completeness_guaranteed, else info "classify miss"; no oracle: unresolved.
  R7.4      `explain` = `subclass` on the same pair, both definite.
  R7.5      oracle-inconsistent KB:
              wrong answer: `sat` = sat, `subclass`/`explain` = no. Hard on every KB; a
                            trusted-consistency note on stderr is attributed in the detail.
              silent answer: `sat` = unsat or `subclass`/`explain` = yes, exit 0, without the
                            "consistency could not be determined" warning. Correct, but a breach
                            of #428: "known #457" on a TBox-only KB, a violation otherwise.
              With the warning it is signalled: ok. `inconsistent` (the error): ok.
            A wrong answer stays a violation even with the "undetermined" warning: today the
            CLI prints that warning only after a positive answer (lib.rs: `!sat && …`), so a
            negative answer with it would itself be a new shape worth a look.
            Stricter than cross-command-laws.py's L5, which accepts `sat` = unsat here.
  R7.6      a call that hit the cap: "capped", a finding candidate to recheck serially. Only
            case 1911's shape on a consistent KB is a #460 regression; other shapes are new.
  R7.7      misses (measurement, not a law): `subclass` no / `sat` sat where the oracle entails.

Usage:
  perquery.py run --bin B --tag T --out DIR --set NAME=DIR ... [--first N] [--cores 92,93]
                  [--env K=V ...] [--extra-case FILE ...] [--cache OCACHE] [--cases-file F]
  perquery.py laws --out DIR --tag T [--cache OCACHE] [--list N]
  perquery.py detect --out DIR --control A --mutant M [--cache OCACHE]
  perquery.py recheck --out DIR --tag T --bin B [--core 92] [--max 30] [--cap 120]
  perquery.py --self-test
"""
import argparse
import importlib.util
import json
import os
import random
import re
import resource
import signal
import subprocess
import sys
import time
from collections import Counter, defaultdict
from concurrent import futures
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))
import oracle as orc  # noqa: E402
import routes  # noqa: E402
import run  # noqa: E402

_spec = importlib.util.spec_from_file_location("ccl", HERE.parent / "cross-command-laws.py")
ccl = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(ccl)

CPU_CAP = int(os.environ.get("PERQUERY_CPU_CAP", "10"))
WALL_CAP = int(os.environ.get("PERQUERY_WALL_CAP", "60"))
CLASSIFY_CAP = int(os.environ.get("PERQUERY_CLASSIFY_CAP", "30"))
OCACHE = os.path.expanduser("~/rustdl/work-363-gen/ocache")
FULL = ["classify", "--json", "--pair-timeout-ms", "1000"]
VR_ENV = {"RUSTDL_CLASSIFY_VERIFY_REFUTATIONS": "1"}
EL_SETS = ("el_only", "el_guaranteed")
UNDET = "consistency could not be determined"
TRUST = ("relies on the KB being consistent",
         "took the ontology as consistent on the consistency wedge's model alone")
# ABox content the #428 guard can seed from: assertions and nominals. A bare
# `Declaration(NamedIndividual(..))` is not: the guard reports has_individuals=false there, so
# such a KB is #457's TBox-only shape.
ABOX = re.compile(r"ClassAssertion|ObjectPropertyAssertion|DataPropertyAssertion"
                  r"|SameIndividual|DifferentIndividuals|NegativeObjectPropertyAssertion"
                  r"|NegativeDataPropertyAssertion|ObjectOneOf|ObjectHasValue")


def tbox_only(text):
    return not ABOX.search(text)


# ------------------------------------------------------------------ calls
def _limits(cpu):
    def f():
        resource.setrlimit(resource.RLIMIT_AS, (8_000_000_000, 8_000_000_000))
        if cpu:
            resource.setrlimit(resource.RLIMIT_CPU, (cpu, cpu + 2))
    return f


def call(binary, args, env, cpu=CPU_CAP, wall=WALL_CAP):
    """One process. Returns the harness's record shape plus cpu seconds and `capped`."""
    r0 = resource.getrusage(resource.RUSAGE_CHILDREN)
    killed = False
    try:
        p = subprocess.run([binary, *args], capture_output=True, text=True, timeout=wall,
                           env={**os.environ, "RAYON_NUM_THREADS": "1", **env},
                           preexec_fn=_limits(cpu))
        rc, out, err = p.returncode, p.stdout, p.stderr
    except subprocess.TimeoutExpired:
        killed = True
        rc, out, err = None, "", ""
    r1 = resource.getrusage(resource.RUSAGE_CHILDREN)
    cpu_s = (r1.ru_utime - r0.ru_utime) + (r1.ru_stime - r0.ru_stime)
    if rc is not None and rc < 0 and -rc in (signal.SIGXCPU, signal.SIGKILL):
        killed = True
    return {"args": args, "env": env, "rc": rc, "stdout": out, "stderr": err, "killed": killed,
            "cpu": round(cpu_s, 3)}


def op_of(r, kind):
    """A compact stored operand: the harness reading plus what R7 needs from stderr."""
    if kind == "sat":
        a = ccl.read_token(r, "sat", "unsat")
    elif kind == "subclass":
        a = ccl.read_token(r, "yes", "no")
    else:
        a = ccl.read_explain(r)
    if r["killed"]:
        a = ccl.ans("unknown", f"capped (cpu {CPU_CAP}s / wall {WALL_CAP}s)", None, None)
    return {"v": a["v"], "why": a["why"], "rc": r["rc"], "cpu": r["cpu"],
            "capped": r["killed"], "undet": UNDET in r["stderr"],
            "trust": any(t in r["stderr"] for t in TRUST)}


def classify_route(binary, path, env, classes):
    r = call(binary, [*FULL, str(path)], env, cpu=0, wall=CLASSIFY_CAP)
    if r["killed"]:
        return {"status": "timeout", "cpu": r["cpu"]}
    try:
        j = json.loads(r["stdout"])
    except json.JSONDecodeError:
        return {"status": "fail", "rc": r["rc"], "stderr": r["stderr"][-300:]}
    unsat, te, clo = run.hierarchy_of(j, classes)
    return {"status": "ok", "cpu": r["cpu"], "answer": {
        "consistent": bool(j.get("consistent")), "unsat": sorted(unsat),
        "thing_equiv": sorted(te), "closure": sorted([s, t] for s, t in clo),
        "flags": {k: j.get(k) for k in routes.FLAGS}}}


def explain_pairs(sha, classes, full):
    rng = random.Random(sha)
    cls = sorted(classes)
    pairs = [(a, b) for a in cls for b in cls if a != b]
    if not pairs:
        return []
    pos = sorted(tuple(p) for p in (full.get("answer") or {}).get("closure", []))
    clo = set(pos)
    neg = [p for p in pairs if p not in clo]
    out = []
    if pos:
        out.append(rng.choice(pos))
    if neg:
        out.append(rng.choice(neg))
    rest = [p for p in pairs if p not in out]
    if rest:
        out.append(rng.choice(rest))
    return out


def one_case(setname, path, binary, env, out_dir):
    dest = Path(out_dir) / "cases" / setname / (Path(path).stem + ".json")
    if dest.exists():
        return "cached"
    text = Path(path).read_text()
    classes = orc.declared_classes(text)
    sha = orc.sha(text)
    rec = {"set": setname, "case": Path(path).stem, "sha256": sha, "path": str(path),
           "classes": sorted(classes), "tbox_only": tbox_only(text), "env": env}
    rec["full"] = classify_route(binary, path, env, classes)
    rec["vr"] = classify_route(binary, path, {**env, **VR_ENV}, classes)
    q, hung = {}, set()

    def ask(kind, *names):
        k = "|".join((kind, *names))
        if kind in hung:
            q[k] = {"v": "unknown", "why": "after-hang", "rc": None, "cpu": 0, "capped": False,
                    "skipped": True, "undet": False, "trust": False}
            return
        r = call(binary, [kind, str(path), *names], env)
        q[k] = op_of(r, kind)
        if q[k]["capped"]:
            hung.add(kind)

    cls = sorted(classes)
    for a in cls:
        for b in cls:
            if a != b:
                ask("subclass", a, b)
    for c in cls:
        ask("sat", c)
    # Extra cases are small hand-picked positives: `explain` asks every pair there.
    xp = ([(a, b) for a in cls for b in cls if a != b] if setname == "extra"
          else explain_pairs(sha, classes, rec["full"]))
    for a, b in xp:
        ask("explain", a, b)
    rec["q"] = q
    dest.parent.mkdir(parents=True, exist_ok=True)
    dest.with_suffix(".part").write_text(json.dumps(rec))
    dest.with_suffix(".part").replace(dest)
    return f"{sum(o['cpu'] for o in q.values()):.1f}s cpu, {len(q)} calls, capped {len(hung)}"


def cmd_run(a):
    out = Path(a.out) / a.tag
    out.mkdir(parents=True, exist_ok=True)
    env = dict(e.split("=", 1) for e in (a.env or []))
    files = routes.case_files(a.set or [], a.first)
    if a.cases_file:
        want = set(Path(a.cases_file).read_text().split())
        files = [(s, f) for s, f in files if f"{s}/{f.stem}" in want]
    for x in a.extra_case or []:
        try:
            rec = orc.oracle(x, a.cache)
        except FileNotFoundError:  # another arm wrote the same cache record first
            rec = orc.oracle(x, a.cache)
        print(f"extra case {x}: oracle {rec['verdict']} {rec.get('kinds')}", flush=True)
        files.append(("extra", Path(x)))
    (out / "manifest.json").write_text(json.dumps({
        "tag": a.tag, "binary": a.bin, "env": env, "n": len(files), "cpu_cap_s": CPU_CAP,
        "wall_cap_s": WALL_CAP, "classify_cap_s": CLASSIFY_CAP, "sets": a.set,
        "extra": a.extra_case, "cases_file": a.cases_file}, indent=1))
    cores = [int(c) for c in a.cores.split(",")]
    import multiprocessing as mp
    qu = mp.Manager().Queue()
    for c in cores:
        qu.put(c)
    done, t0 = 0, time.time()
    with futures.ProcessPoolExecutor(max_workers=len(cores), initializer=run._pin,
                                     initargs=(qu,)) as ex:
        futs = {ex.submit(one_case, s, str(f), a.bin, env, str(out)): (s, f) for s, f in files}
        for fu in futures.as_completed(futs):
            done += 1
            s, f = futs[fu]
            try:
                msg = fu.result()
            except Exception as exc:  # noqa: BLE001
                msg = f"ERROR {exc!r}"
            print(f"[{done}/{len(files)} {time.time() - t0:.0f}s] {s}/{f.stem}: {msg}", flush=True)
    return 0


# ------------------------------------------------------------------ laws
def implies(cl, a, b):
    """Does a completed classify answer imply A ⊑ B?"""
    return (a in cl["unsat"]) or (b in cl.get("thing_equiv", [])) or ([a, b] in cl["closure"])


def oracle_entails(o, a, b):
    return a in o["unsat"] or b in o.get("thing_equiv", []) or [a, b] in o["pairs"]


def evaluate(rec, oans):
    """[(law, outcome, detail)] for one case."""
    out = []

    def add(law, outcome, detail=""):
        out.append((law, outcome, detail))

    classes = set(rec["classes"])
    full, vr = rec.get("full", {}), rec.get("vr", {})
    # R7.0: the proxy route's FP check, and `full` for contrast.
    for name, r in (("vr", vr), ("full", full)):
        if oans is None or r.get("status") != "ok":
            add(f"R7.0.{name}", "unresolved", "no oracle" if oans is None else r.get("status", "?"))
            continue
        fps = routes.fp_findings(oans, classes, r["answer"])
        add(f"R7.0.{name}", "violation" if fps else "ok",
            "; ".join(f"{f['kind']} {f['witness']}" for f in fps[:3]))
    if rec["set"] in EL_SETS:
        if full.get("status") == "ok" and vr.get("status") == "ok" \
                and not routes.gave_up(full) and not routes.gave_up(vr):
            same = routes.key(full["answer"]) == routes.key(vr["answer"])
            add("R7.0c", "ok" if same else "violation", "" if same else "vr != full on an EL set")
        else:
            add("R7.0c", "unresolved", "a classify route gave up")

    ok_cl = full.get("status") == "ok" and full["answer"]["consistent"]
    cl = full["answer"] if ok_cl else None
    cl_gave = routes.gave_up(full) if full.get("status") == "ok" else True
    cl_complete = ok_cl and not cl_gave and bool(cl["flags"].get("completeness_guaranteed"))
    ocons = None if oans is None else bool(oans["consistent"])
    q = rec.get("q", {})

    for k, o in q.items():
        kind, *names = k.split("|")
        v = o["v"]
        tag = f"{kind} {' '.join(n.rsplit('#', 1)[-1] for n in names)}"
        if o.get("capped"):
            add("R7.6", "capped", f"{tag} (cpu {o['cpu']}s)")
        if o.get("skipped"):
            add("R7.6", "unresolved", f"{tag}: after-hang")
            continue
        if v == "unknown":
            add(f"R7.{'2' if kind == 'sat' else '1'}", "unresolved", f"{tag}: {o['why']}")
            continue
        # R7.5: an oracle-inconsistent KB.
        if ocons is False:
            if v == "inconsistent":
                add("R7.5", "ok", "")
                continue
            wrong = (kind == "sat" and v == "yes") or (kind != "sat" and v == "no")
            if wrong:
                add("R7.5", "violation", f"{tag}: wrong answer on an inconsistent KB"
                    + (" (trusted-consistency note)" if o.get("trust") else ""))
            elif o.get("undet"):
                add("R7.5", "ok", f"{tag}: signalled (consistency undetermined)")
            elif rec.get("tbox_only"):
                add("R7.5", "known", f"{tag}: silent answer, TBox-only (#457)")
            else:
                add("R7.5", "violation", f"{tag}: silent answer on a KB with individuals"
                    + (" (trusted-consistency note)" if o.get("trust") else ""))
            continue
        if v == "inconsistent":
            if ocons is True:
                add("R7.5", "violation", f"{tag}: error Inconsistent on an oracle-consistent KB")
            else:
                add("R7.5", "unresolved", f"{tag}: error Inconsistent, no oracle")
            continue
        if kind in ("subclass", "explain"):
            a, b = names
            # R7.1 / R7.7 against the oracle.
            if ocons is True:
                ent = oracle_entails(oans, a, b)
                if v == "yes":
                    add("R7.1", "ok" if ent else "violation", "" if ent else f"{tag}: oracle refutes")
                else:
                    add("R7.7", "miss" if ent else "ok", tag if ent else "")
            # R7.3 against classify.
            if cl is not None:
                ci = implies(cl, a, b)
                if ci and v == "no":
                    add("R7.3", "violation", f"{tag}: classify has it, the command says no"
                        + (" (classify gave up)" if cl_gave else ""))
                elif not ci and v == "yes":
                    if ocons is True and oracle_entails(oans, a, b):
                        add("R7.3", "violation" if cl_complete else "info",
                            f"{tag}: classify miss" + (" with completeness_guaranteed (D10)"
                                                         if cl_complete else ""))
                    elif ocons is None:
                        add("R7.3", "unresolved", f"{tag}: classify lacks it, no oracle")
                    # refuted: R7.1 has it
                else:
                    add("R7.3", "ok", "")
        else:  # sat
            (c,) = names
            if ocons is True:
                osat = c not in oans["unsat"]
                if v == "no":
                    add("R7.2", "violation" if osat else "ok", f"{tag}: oracle has it sat" if osat else "")
                else:
                    add("R7.7", "ok" if osat else "miss", "" if osat else tag)
            if cl is not None:
                cu = c in cl["unsat"]
                if cu and v == "yes":
                    add("R7.3", "violation", f"{tag}: classify has it unsat, `sat` says sat")
                elif not cu and v == "no":
                    if ocons is True and c in oans["unsat"]:
                        add("R7.3", "violation" if cl_complete else "info",
                            f"{tag}: classify miss (unsat)" + (" with completeness_guaranteed (D10)"
                                                                if cl_complete else ""))
                    elif ocons is None:
                        add("R7.3", "unresolved", f"{tag}: classify lacks the unsat, no oracle")
                else:
                    add("R7.3", "ok", "")
    # R7.4: explain vs subclass.
    for k, o in q.items():
        kind, *names = k.split("|")
        if kind != "explain":
            continue
        s = q.get("|".join(("subclass", *names)))
        if s is None:
            continue
        tag = f"explain {' '.join(n.rsplit('#', 1)[-1] for n in names)}"
        dv, sv = o["v"], s["v"]
        if dv in ("yes", "no", "inconsistent") and sv in ("yes", "no", "inconsistent"):
            add("R7.4", "ok" if dv == sv else "violation", "" if dv == sv else f"{tag}: explain {dv}, subclass {sv}")
        else:
            add("R7.4", "unresolved", f"{tag}: explain {dv}, subclass {sv}")
    return out


def oracle_view(cache, sha):
    """The agreed oracle answer in the shape the laws read, or None."""
    p = Path(cache) / sha[:2] / f"{sha}.json"
    if not p.exists():
        return None
    o = json.loads(p.read_text())
    if o.get("verdict") != "agree":
        return None
    return o["answer"]


def load_tag(out, tag):
    for p in sorted((Path(out) / tag / "cases").glob("*/*.json")):
        yield json.loads(p.read_text())


def all_outcomes(out, tag, cache):
    res = {}
    for rec in load_tag(out, tag):
        p = Path(rec.get("path", ""))
        if p.is_file():  # recomputed: records written before the declaration fix stored it
            rec["tbox_only"] = tbox_only(p.read_text())
        res[(rec["set"], rec["case"])] = evaluate(rec, oracle_view(cache, rec["sha256"]))
    return res


def cmd_laws(a):
    res = all_outcomes(a.out, a.tag, a.cache)
    tally = defaultdict(Counter)
    viol, known, capped = [], [], []
    for (s, c), outs in sorted(res.items()):
        for law, o, d in outs:
            tally[law][o] += 1
            if o == "violation":
                viol.append(f"VIOLATION {s} {c} {law} {d}")
            elif o == "known":
                known.append(f"KNOWN {s} {c} {law} {d}")
            elif o == "capped":
                capped.append(f"CAPPED {s} {c} {d}")
    rep = {"tag": a.tag, "cases": len(res), "laws": {k: dict(v) for k, v in sorted(tally.items())},
           "violations": len(viol), "known": len(known), "capped": len(capped)}
    print(json.dumps(rep, indent=1))
    for line in (viol + capped + known)[:a.list]:
        print(line)
    (Path(a.out) / a.tag / "laws.json").write_text(json.dumps(
        {**rep, "violation_list": viol, "known_list": known, "capped_list": capped}, indent=1))
    return 0


def cmd_detect(a):
    """Violations in the mutant that the control lacks, and one-arm unknowns to recheck."""
    c = all_outcomes(a.out, a.control, a.cache)
    m = all_outcomes(a.out, a.mutant, a.cache)
    new = []
    for k, outs in m.items():
        base = {(l, d) for l, o, d in c.get(k, []) if o == "violation"}
        for l, o, d in outs:
            if o == "violation" and (l, d) not in base:
                new.append([k[0], k[1], l, d])
    crec = {(r["set"], r["case"]): r for r in load_tag(a.out, a.control)}
    one_arm = []
    for r in load_tag(a.out, a.mutant):
        cr = crec.get((r["set"], r["case"]))
        if not cr:
            continue
        for k, o in r.get("q", {}).items():
            co = cr.get("q", {}).get(k)
            if co and (o["v"] == "unknown") != (co["v"] == "unknown"):
                one_arm.append([r["set"], r["case"], k, co["v"], o["v"]])
    rep = {"control": a.control, "mutant": a.mutant,
           "detecting_cases": len({(v[0], v[1]) for v in new}),
           "by_law": dict(Counter(v[2] for v in new)), "first": new[:30],
           "one_arm_unknown": len(one_arm), "one_arm_first": one_arm[:30]}
    Path(a.out, a.mutant, f"detect-vs-{a.control}.json").write_text(json.dumps(
        {**rep, "all": new, "one_arm_all": one_arm}, indent=1))
    print(json.dumps(rep, indent=1))
    return 0


def cmd_recheck(a):
    """Serial recheck of capped calls: at most one per (case, command), the first --max."""
    os.sched_setaffinity(0, {a.core})
    seen, todo = set(), []
    for rec in load_tag(a.out, a.tag):
        for k, o in rec.get("q", {}).items():
            kind = k.split("|", 1)[0]
            if o.get("capped") and (rec["set"], rec["case"], kind) not in seen:
                seen.add((rec["set"], rec["case"], kind))
                todo.append((rec, k))
    res = []
    for rec, k in todo[:a.max]:
        kind, *names = k.split("|")
        r = call(a.bin, [kind, rec["path"], *names], rec.get("env", {}), cpu=a.cap, wall=a.cap * 3)
        o = op_of(r, kind)
        res.append({"set": rec["set"], "case": rec["case"], "op": k, "v": o["v"], "cpu": o["cpu"],
                    "capped": o["capped"]})
        print(json.dumps(res[-1]), flush=True)
    rep = {"tag": a.tag, "capped_calls": len(todo), "rechecked": len(res),
           "not_rechecked": max(0, len(todo) - a.max), "results": res}
    (Path(a.out) / a.tag / "recheck.json").write_text(json.dumps(rep, indent=1))
    print(json.dumps({k: v for k, v in rep.items() if k != "results"}))
    return 0


# ------------------------------------------------------------------ self-test
def self_test():
    NS = "http://g#"
    A, B, C = NS + "A", NS + "B", NS + "C"
    fails = 0

    def ok(name, got, want):
        nonlocal fails
        good = got == want
        fails += not good
        print(f"{'ok  ' if good else 'FAIL'} {name}: got {got}")

    def op(v, **kw):
        return {"v": v, "why": "", "rc": 0, "cpu": 0.03, "capped": False, "undet": False,
                "trust": False, **kw}

    def cl(closure=(), unsat=(), te=(), cons=True, **flags):
        return {"status": "ok", "answer": {
            "consistent": cons, "unsat": list(unsat), "thing_equiv": list(te),
            "closure": [list(p) for p in closure],
            "flags": {"incomplete": False, "completeness_guaranteed": True, **flags}}}

    def rec(q, full=None, vr=None, setname="default", tbox_only=True):
        full = full or cl()
        return {"set": setname, "case": "c", "sha256": "x", "classes": [A, B, C],
                "tbox_only": tbox_only, "full": full, "vr": vr or full, "q": q}

    def orc_(pairs=(), unsat=(), te=(), cons=True):
        return {"consistent": cons, "unsat": list(unsat), "thing_equiv": list(te),
                "pairs": [list(p) for p in pairs]}

    def laws(r, o, only=None):
        return {(l, x) for l, x, _ in evaluate(r, o) if only is None or l.startswith(only)}

    sub = lambda a, b: f"subclass|{a}|{b}"  # noqa: E731
    o_ab = orc_(pairs=[(A, B)])
    # R7.1
    ok("R7.1 FP refuted by the oracle", laws(rec({sub(B, A): op("yes")}), o_ab, "R7.1"),
       {("R7.1", "violation")})
    ok("R7.1 yes the oracle entails", laws(rec({sub(A, B): op("yes")}, cl([(A, B)])), o_ab, "R7.1"),
       {("R7.1", "ok")})
    ok("R7.1 yes on an oracle-unsat A is right", laws(rec({sub(A, C): op("yes")}),
                                                      orc_(unsat=[A]), "R7.1"), {("R7.1", "ok")})
    ok("R7.1 yes to an oracle-Thing-equivalent B is right",
       laws(rec({sub(A, C): op("yes")}), orc_(te=[C]), "R7.1"), {("R7.1", "ok")})
    ok("R7.1 explain FP", laws(rec({f"explain|{B}|{A}": op("yes")}), o_ab, "R7.1"),
       {("R7.1", "violation")})
    ok("R7.1 no oracle: nothing", laws(rec({sub(B, A): op("yes")}), None, "R7.1"), set())
    ok("R7.1 unknown is unresolved", laws(rec({sub(B, A): op("unknown")}), o_ab, "R7.1"),
       {("R7.1", "unresolved")})
    # R7.2
    ok("R7.2 false unsat", laws(rec({f"sat|{A}": op("no")}), orc_(), "R7.2"),
       {("R7.2", "violation")})
    ok("R7.2 true unsat", laws(rec({f"sat|{A}": op("no")}, cl(unsat=[A])), orc_(unsat=[A]), "R7.2"),
       {("R7.2", "ok")})
    # R7.3
    ok("R7.3 dir1: classify has it, subclass says no",
       laws(rec({sub(A, B): op("no")}, cl([(A, B)])), o_ab, "R7.3"), {("R7.3", "violation")})
    ok("R7.3 dir1 holds under a classify cut",
       laws(rec({sub(A, B): op("no")}, cl([(A, B)], incomplete=True)), o_ab, "R7.3"),
       {("R7.3", "violation")})
    ok("R7.3 classify-unsat A implies A ⊑ B",
       laws(rec({sub(A, B): op("yes")}, cl(unsat=[A])), orc_(unsat=[A]), "R7.3"), {("R7.3", "ok")})
    ok("R7.3 classify-Thing-equiv B implies A ⊑ B",
       laws(rec({sub(A, B): op("yes")}, cl(te=[B])), orc_(te=[B]), "R7.3"), {("R7.3", "ok")})
    ok("R7.3 dir2 D10: confirmed, classify complete and lacks it",
       laws(rec({sub(A, B): op("yes")}), o_ab, "R7.3"), {("R7.3", "violation")})
    ok("R7.3 dir2 classify miss without the guarantee is info",
       laws(rec({sub(A, B): op("yes")}, cl(completeness_guaranteed=False)), o_ab, "R7.3"),
       {("R7.3", "info")})
    ok("R7.3 dir2 classify miss under a give-up is info",
       laws(rec({sub(A, B): op("yes")}, cl(incomplete=True)), o_ab, "R7.3"), {("R7.3", "info")})
    ok("R7.3 dir2 no oracle: unresolved",
       laws(rec({sub(A, B): op("yes")}), None, "R7.3"), {("R7.3", "unresolved")})
    ok("R7.3 dir2 refuted is R7.1's, not R7.3's",
       laws(rec({sub(B, A): op("yes")}), o_ab, "R7.3"), set())
    ok("R7.3 sat: classify unsat, `sat` sat",
       laws(rec({f"sat|{A}": op("yes")}, cl(unsat=[A])), orc_(unsat=[A]), "R7.3"),
       {("R7.3", "violation")})
    ok("R7.3 inconsistent classify: not compared",
       laws(rec({sub(A, B): op("no")}, cl(cons=False)), None, "R7.3"), set())
    # R7.4
    ok("R7.4 explain != subclass",
       laws(rec({sub(A, B): op("yes"), f"explain|{A}|{B}": op("no")}, cl([(A, B)])), o_ab, "R7.4"),
       {("R7.4", "violation")})
    ok("R7.4 agree", laws(rec({sub(A, B): op("yes"), f"explain|{A}|{B}": op("yes")}, cl([(A, B)])),
                          o_ab, "R7.4"), {("R7.4", "ok")})
    # R7.5
    inc = orc_(cons=False)
    ok("R7.5 wrong answer (sat) on an inconsistent KB",
       laws(rec({f"sat|{A}": op("yes")}), inc, "R7.5"), {("R7.5", "violation")})
    ok("R7.5 wrong answer (subclass no) is hard even TBox-only, even with the warning",
       laws(rec({sub(A, B): op("no", undet=True)}), inc, "R7.5"), {("R7.5", "violation")})
    ok("R7.5 silent answer TBox-only is known #457",
       laws(rec({f"sat|{A}": op("no")}), inc, "R7.5"), {("R7.5", "known")})
    ok("R7.5 silent answer with individuals is a violation",
       laws(rec({f"sat|{A}": op("no")}, tbox_only=False), inc, "R7.5"), {("R7.5", "violation")})
    ok("R7.5 signalled silent answer is ok",
       laws(rec({sub(A, B): op("yes", undet=True)}, tbox_only=False), inc, "R7.5"), {("R7.5", "ok")})
    ok("R7.5 the Inconsistent error is ok", laws(rec({sub(A, B): op("inconsistent")}), inc, "R7.5"),
       {("R7.5", "ok")})
    ok("R7.5 Inconsistent on an oracle-consistent KB",
       laws(rec({sub(A, B): op("inconsistent")}), orc_(), "R7.5"), {("R7.5", "violation")})
    # R7.6
    ok("R7.6 capped and after-hang",
       laws(rec({sub(A, B): op("unknown", capped=True, why="capped"),
                 sub(B, A): op("unknown", skipped=True, why="after-hang")}), o_ab, "R7.6"),
       {("R7.6", "capped"), ("R7.6", "unresolved")})
    # R7.7
    ok("R7.7 miss", laws(rec({sub(A, B): op("no")}), o_ab, "R7.7"), {("R7.7", "miss")})
    # R7.0
    ok("R7.0.vr FP", laws(rec({}, vr=cl([(B, A)])), o_ab, "R7.0.vr"), {("R7.0.vr", "violation")})
    ok("R7.0.vr clean", laws(rec({}, vr=cl([(A, B)])), o_ab, "R7.0.vr"), {("R7.0.vr", "ok")})
    ok("R7.0c vr != full on an EL set",
       laws(rec({}, full=cl([(A, B)]), vr=cl(), setname="el_only"), o_ab, "R7.0c"),
       {("R7.0c", "violation")})
    ok("R7.0c holds on an EL set",
       laws(rec({}, full=cl([(A, B)]), vr=cl([(A, B)]), setname="el_only"), o_ab, "R7.0c"),
       {("R7.0c", "ok")})
    # readers and stderr
    r = {"args": ["sat"], "env": {}, "rc": 0, "stdout": "unsat\n", "stderr": f"warning: sat: the KB's {UNDET} (x)",
         "killed": False, "cpu": 0.1}
    ok("op_of reads the undetermined warning", (op_of(r, "sat")["v"], op_of(r, "sat")["undet"]),
       ("no", True))
    r2 = {**r, "rc": 1, "stdout": "", "stderr": "Error: x\nCaused by:\n    ontology is inconsistent; every"}
    ok("op_of reads the Inconsistent error", op_of(r2, "subclass")["v"], "inconsistent")
    r3 = {**r, "rc": None, "stdout": "", "stderr": "", "killed": True}
    ok("op_of: a capped call is unknown and capped", (op_of(r3, "sat")["v"], op_of(r3, "sat")["capped"]),
       ("unknown", True))
    r4 = {**r, "stdout": "http://g#A ⊑ http://g#B : yes — answered by tableau\n", "stderr": TRUST[0]}
    ok("op_of reads explain and the trust note", (op_of(r4, "explain")["v"], op_of(r4, "explain")["trust"]),
       ("yes", True))
    ok("tbox_only: a bare individual declaration is still TBox-only",
       (tbox_only("Declaration(NamedIndividual(:a0))\nSubClassOf(:A :B)"),
        tbox_only("ClassAssertion(:A :a0)"), tbox_only("SubClassOf(:A ObjectOneOf(:a0))")),
       (True, False, False))
    ok("explain_pairs: 3 with a positive, 2 without",
       (len(explain_pairs("s", {A, B, C}, cl([(A, B)]))), len(explain_pairs("s", {A, B, C}, cl()))),
       (3, 2))
    print(f"perquery self-test: {'all ok' if not fails else f'{fails} FAILED'}")
    return 1 if fails else 0


def main():
    if "--self-test" in sys.argv:
        return self_test()
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("run")
    r.add_argument("--bin", required=True)
    r.add_argument("--tag", required=True)
    r.add_argument("--out", required=True)
    r.add_argument("--set", action="append")
    r.add_argument("--first", type=int, default=0)
    r.add_argument("--cores", default="92")
    r.add_argument("--env", action="append")
    r.add_argument("--extra-case", action="append")
    r.add_argument("--cases-file")
    r.add_argument("--cache", default=OCACHE)
    lw = sub.add_parser("laws")
    lw.add_argument("--out", required=True)
    lw.add_argument("--tag", required=True)
    lw.add_argument("--cache", default=OCACHE)
    lw.add_argument("--list", type=int, default=60)
    d = sub.add_parser("detect")
    d.add_argument("--out", required=True)
    d.add_argument("--control", required=True)
    d.add_argument("--mutant", required=True)
    d.add_argument("--cache", default=OCACHE)
    rc = sub.add_parser("recheck")
    rc.add_argument("--out", required=True)
    rc.add_argument("--tag", required=True)
    rc.add_argument("--bin", required=True)
    rc.add_argument("--core", type=int, default=92)
    rc.add_argument("--max", type=int, default=30)
    rc.add_argument("--cap", type=int, default=120)
    a = ap.parse_args()
    return {"run": cmd_run, "laws": cmd_laws, "detect": cmd_detect, "recheck": cmd_recheck}[a.cmd](a)


if __name__ == "__main__":
    sys.exit(main())
