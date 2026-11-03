#!/usr/bin/env python3
"""Generated-ontology oracle net (#363 step 3): run rustdl on generated cases and score
it against the Konclude ∩ HermiT agreement oracle.

  run.py gen     --seed S --n N --cases DIR [--profile P]
  run.py run     --cases DIR --bin BIN --tag TAG --out DIR [--env K=V ...]
                 [--first N] [--cores 92,93,94] [--oracle-cache DIR]
  run.py report  --out DIR --tag TAG
  run.py detect  --out DIR --control TAG --mutant TAG [--first N]
  run.py --self-test

Arms (30 s cap each, one call per arm per case):
  classify --json, classify --saturation-only --json, consistent --json.

Scoring (plan v2 §3). FP kinds are findings whatever `incomplete` says:
  pair-FP, unsat-FP, consistency-FP, satonly-FP.
MISSED kinds are split by rustdl's own guarantee:
  MISSED-guaranteed (`completeness_guaranteed: true`): a finding;
  MISSED-exposed (otherwise): a declared limitation, reported, not a finding.
Consistency MISSED: on classify's `consistent` field it is a pre-check tier,
informational only; on `consistent --json` it is a finding only when `incomplete`,
`relies_on_trusted_consistency` are false, `wedge_clauses_refused` is 0 and nothing was
dropped. If the oracle says inconsistent, only consistency is scored.

Case buckets, exclusive, in this order: invalid, peer-fail, peer-disagree,
rustdl-fail, finding, exposed, clean.
"""
import argparse
import json
import os
import resource
import subprocess
import sys
from collections import Counter
from concurrent import futures
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))
import gen  # noqa: E402
import oracle as orc  # noqa: E402
from owl_readers import THING, closure  # noqa: E402

RUSTDL_CAP = int(os.environ.get("GEN_RUSTDL_CAP", "30"))
ARMS = {
    "classify": ["classify", "--json"],
    "satonly": ["classify", "--saturation-only", "--json"],
    "consistent": ["consistent", "--json"],
}
FP_KINDS = ("pair-FP", "unsat-FP", "consistency-FP", "satonly-FP")


# ------------------------------------------------------------------ rustdl
def _limit():
    resource.setrlimit(resource.RLIMIT_AS, (8_000_000_000, 8_000_000_000))


def run_arm(binary, arm, path, env):
    e = {**os.environ, **env}
    try:
        p = subprocess.run([binary, *ARMS[arm], str(path)], capture_output=True, text=True,
                           timeout=RUSTDL_CAP, env=e, preexec_fn=_limit)
    except subprocess.TimeoutExpired:
        return {"status": "timeout"}
    try:
        return {"status": "ok", "rc": p.returncode, "json": json.loads(p.stdout)}
    except json.JSONDecodeError:
        return {"status": "fail", "rc": p.returncode, "stderr": p.stderr[-400:]}


def hierarchy_of(j, classes):
    """(unsat, thing_equiv, closure) from a classify --json, restricted to `classes`."""
    unsat = set(j.get("unsatisfiable", [])) & classes
    thing_equiv = set()
    groups = []
    for g in j.get("equivalent_groups", []):
        if THING in g:
            thing_equiv |= set(g) - {THING}
        else:
            groups.append(g)
    edges = [tuple(e) for e in j.get("direct_subsumptions", [])]
    for s, t in edges:
        if s == THING:
            thing_equiv.add(t)
    clo = {(s, t) for s, t in closure(edges, groups) if s in classes and t in classes}
    return unsat, thing_equiv & classes, clo


# ------------------------------------------------------------------ scoring
def score(oracle_ans, classes, arms):
    """Findings for one case: [{kind, tier, witness, arm}].

    tier: "finding" | "exposed" | "pre-check". `arms` maps arm -> run_arm record."""
    out = []

    def add(kind, tier, witness, arm):
        out.append({"kind": kind, "tier": tier, "witness": witness, "arm": arm})

    cls = arms.get("classify", {})
    sat = arms.get("satonly", {})
    con = arms.get("consistent", {})
    cj = cls.get("json") if cls.get("status") == "ok" else None
    sj = sat.get("json") if sat.get("status") == "ok" else None
    kj = con.get("json") if con.get("status") == "ok" else None

    if not oracle_ans["consistent"]:
        # Every class is unsat and every pair entailed: score consistency only.
        if cj is not None and cj.get("consistent"):
            add("consistency-MISSED", "pre-check", "KB", "classify")
        if kj is not None and kj.get("consistent"):
            gated = (not kj.get("incomplete") and not kj.get("relies_on_trusted_consistency")
                     and not kj.get("wedge_clauses_refused") and not kj.get("dropped"))
            add("consistency-MISSED", "finding" if gated else "exposed", "KB", "consistent")
        return out

    o_unsat, o_te = set(oracle_ans["unsat"]), set(oracle_ans["thing_equiv"])
    o_pairs = {tuple(p) for p in oracle_ans["pairs"]}

    if kj is not None and not kj.get("consistent"):
        add("consistency-FP", "finding", "KB", "consistent")

    for arm, j, fp_kind in (("classify", cj, None), ("satonly", sj, "satonly-FP")):
        if j is None:
            continue
        if not j.get("consistent"):
            add(fp_kind or "consistency-FP", "finding", "KB", arm)
            continue
        r_unsat, r_te, r_clo = hierarchy_of(j, classes)
        for c in sorted(r_unsat - o_unsat):
            add(fp_kind or "unsat-FP", "finding", c, arm)
        for c in sorted(r_te - o_te):
            add(fp_kind or "pair-FP", "finding", f"{THING}|{c}", arm)
        excl = o_unsat | r_unsat | o_te | r_te
        r_cmp = {(s, t) for s, t in r_clo if s not in excl and t not in excl}
        o_cmp = {(s, t) for s, t in o_pairs if s not in excl and t not in excl}
        for s, t in sorted(r_cmp - o_cmp):
            add(fp_kind or "pair-FP", "finding", f"{s}|{t}", arm)
        if arm != "classify":
            continue  # saturation-only is a documented under-approximation: no MISSED
        tier = "finding" if j.get("completeness_guaranteed") else "exposed"
        for c in sorted(o_unsat - r_unsat):
            add("unsat-MISSED", tier, c, arm)
        for s, t in sorted(o_cmp - r_cmp):
            add("pair-MISSED", tier, f"{s}|{t}", arm)
    return out


def bucket(rec):
    v = rec["oracle"]["verdict"]
    if v != "agree":
        return v
    if any(a.get("status") != "ok" for a in rec["arms"].values()):
        return "rustdl-fail"
    tiers = {f["tier"] for f in rec["findings"]}
    if "finding" in tiers:
        return "finding"
    if "exposed" in tiers:
        return "exposed"
    return "clean"


# ------------------------------------------------------------------ one case
_CORE = None


def _pin(q):
    global _CORE
    _CORE = q.get()
    os.sched_setaffinity(0, {_CORE})


def one_case(path, binary, env, cache, out_dir):
    dest = Path(out_dir) / "cases" / (Path(path).stem + ".json")
    if dest.exists():
        return json.loads(dest.read_text())
    text = Path(path).read_text()
    classes = orc.declared_classes(text)
    ora = orc.oracle(path, cache)
    rec = {"case": Path(path).stem, "header": text.splitlines()[0], "sha256": ora["sha256"],
           "oracle": {"verdict": ora["verdict"], "kinds": ora["kinds"]}, "arms": {},
           "findings": []}
    if ora["verdict"] in ("agree", "peer-disagree"):
        for arm in ARMS:
            rec["arms"][arm] = run_arm(binary, arm, path, env)
    if ora["verdict"] == "agree":
        rec["findings"] = score(ora["answer"], classes, rec["arms"])
    elif ora["verdict"] == "peer-disagree":
        rec["sides"] = sides(ora, classes, rec["arms"])
    rec["bucket"] = bucket(rec)
    # keep the per-case record small: drop the raw JSON of clean arms
    for a in rec["arms"].values():
        j = a.pop("json", None)
        if j is not None:
            a["summary"] = {k: j.get(k) for k in (
                "consistent", "incomplete", "completeness_guaranteed",
                "relies_on_trusted_consistency", "wedge_clauses_refused",
                "trusted_sat_refutations", "wedge_hierarchy_blind", "dropped",
                "consistency_undetermined")}
    dest.parent.mkdir(parents=True, exist_ok=True)
    tmp = dest.with_suffix(".part")
    tmp.write_text(json.dumps(rec))
    tmp.replace(dest)
    return rec


def sides(ora, classes, arms):
    """On a peer disagreement, which peer rustdl's classify agrees with, per kind."""
    j = arms.get("classify", {}).get("json")
    k, h = ora["konclude"], ora["hermit"]
    if j is None or k.get("status") != "ok" or h.get("status") != "ok":
        return "n/a"
    rc = bool(j.get("consistent"))
    if k["consistent"] != h["consistent"]:
        return "konclude" if rc == k["consistent"] else "hermit"
    if not rc or not k["consistent"]:
        return "n/a"
    r_unsat, r_te, r_clo = hierarchy_of(j, classes)
    kk = {tuple(p) for p in k["pairs"]}
    hh = {tuple(p) for p in h["pairs"]}
    votes = []
    if set(k["unsat"]) != set(h["unsat"]):
        votes.append("konclude" if r_unsat == set(k["unsat"]) else
                     "hermit" if r_unsat == set(h["unsat"]) else "neither")
    k_te, h_te = set(k["thing_equiv"]), set(h["thing_equiv"])
    if k_te != h_te:
        # rustdl does not mark `C ≡ owl:Thing`: it puts every class under C. So on the
        # disputed classes only, read "everything satisfiable is below C" as C ≡ Thing.
        sat = classes - r_unsat
        disputed = k_te ^ h_te
        r_has = {c for c in disputed
                 if c in r_te or all((x, c) in r_clo for x in sat if x != c)}
        votes.append("konclude" if r_has == disputed & k_te else
                     "hermit" if r_has == disputed & h_te else "neither")
    excl = set(k["unsat"]) | set(h["unsat"]) | k_te | h_te
    diff = {p for p in kk ^ hh if not set(p) & excl}
    if diff:
        votes.append("konclude" if all((p in r_clo) == (p in kk) for p in diff) else
                     "hermit" if all((p in r_clo) == (p in hh) for p in diff) else "neither")
    return ",".join(votes) or "n/a"


# ------------------------------------------------------------------ commands
def parse_env(items):
    env = {}
    for it in items or ():
        k, _, v = it.partition("=")
        env[k] = v
    return env


def cmd_gen(a):
    return 1 if gen.generate(a.seed, a.n, a.cases, a.profile, a.start) else 0


def cmd_run(a):
    out = Path(a.out) / a.tag
    out.mkdir(parents=True, exist_ok=True)
    env = parse_env(a.env)
    files = sorted(Path(a.cases).glob("case-*.ofn"))
    if a.first:
        files = files[: a.first]
    (out / "manifest.json").write_text(json.dumps({
        "tag": a.tag, "binary": a.bin, "env": env, "cases": str(a.cases), "n": len(files),
        "rustdl_cap_s": RUSTDL_CAP, "peer_cap_s": orc.PEER_CAP, "cores": a.cores,
        "gen_hash": gen.gen_hash(), "oracle_version": orc.ORACLE_VERSION}, indent=1))
    cores = [int(c) for c in a.cores.split(",")]
    import multiprocessing as mp
    q = mp.Manager().Queue()
    for c in cores:
        q.put(c)
    done = 0
    with futures.ProcessPoolExecutor(max_workers=len(cores), initializer=_pin,
                                     initargs=(q,)) as ex:
        futs = {ex.submit(one_case, str(f), a.bin, env, a.oracle_cache, str(out)): f
                for f in files}
        for f in futures.as_completed(futs):
            done += 1
            try:
                r = f.result()
                print(f"[{done}/{len(files)}] {r['case']} {r['bucket']}", flush=True)
            except Exception as exc:  # noqa: BLE001
                print(f"[{done}/{len(files)}] {futs[f].stem} ERROR {exc!r}", flush=True)
    return cmd_report(argparse.Namespace(out=a.out, tag=a.tag, first=a.first))


def load(out, tag, first=None):
    recs = {}
    for p in sorted((Path(out) / tag / "cases").glob("case-*.json")):
        recs[p.stem] = json.loads(p.read_text())
    if first:
        keep = sorted(recs)[:first]
        recs = {k: recs[k] for k in keep}
    return recs


def cmd_report(a):
    recs = load(a.out, a.tag, getattr(a, "first", None))
    buckets = Counter(r["bucket"] for r in recs.values())
    kinds = Counter((f["kind"], f["tier"]) for r in recs.values() for f in r["findings"])
    case_kinds = Counter((f["kind"], f["tier"]) for r in recs.values()
                         for f in {(x["kind"], x["tier"]): x for x in r["findings"]}.values())
    disagree = Counter((tuple(r["oracle"]["kinds"]), r.get("sides", "n/a"))
                       for r in recs.values() if r["bucket"] == "peer-disagree")
    fails = Counter(f"{arm}:{a2.get('status')}" for r in recs.values()
                    for arm, a2 in r["arms"].items() if a2.get("status") != "ok")
    peer_fail = Counter(tuple(r["oracle"]["kinds"]) for r in recs.values()
                        if r["bucket"] in ("peer-fail", "invalid"))
    rep = {
        "tag": a.tag, "n": len(recs), "buckets": dict(buckets),
        "findings_by_kind_tier": {f"{k}/{t}": n for (k, t), n in sorted(kinds.items())},
        "cases_by_kind_tier": {f"{k}/{t}": n for (k, t), n in sorted(case_kinds.items())},
        "finding_cases": sorted(c for c, r in recs.items() if r["bucket"] == "finding"),
        "peer_disagree": {f"{'+'.join(k)} rustdl-sides:{s}": n for (k, s), n in disagree.items()},
        "peer_fail_or_invalid": {"+".join(k): n for k, n in peer_fail.items()},
        "rustdl_arm_failures": dict(fails),
    }
    Path(a.out, a.tag, "report.json").write_text(json.dumps(rep, indent=1))
    print(json.dumps(rep, indent=1))
    return 0


def differential(control, mutant):
    """Per case, the (kind, tier, witness) findings in the mutant and not the control."""
    out = {}
    for case, m in mutant.items():
        c = control.get(case)
        if c is None or m["oracle"]["verdict"] != "agree":
            continue
        base = {(f["kind"], f["witness"]) for f in c["findings"]}
        new = [f for f in m["findings"] if (f["kind"], f["witness"]) not in base]
        if new:
            out[case] = new
    return out


def cmd_resides(a):
    """Recompute `sides` for the peer-disagree cases of a run (reruns classify only)."""
    out = Path(a.out) / a.tag
    man = json.loads((out / "manifest.json").read_text())
    for p in sorted((out / "cases").glob("case-*.json")):
        rec = json.loads(p.read_text())
        if rec["bucket"] != "peer-disagree":
            continue
        case = Path(man["cases"]) / f"{rec['case']}.ofn"
        ora = orc.oracle(case, a.oracle_cache)
        arms = {"classify": run_arm(man["binary"], "classify", case, man["env"])}
        rec["sides"] = sides(ora, orc.declared_classes(case.read_text()), arms)
        p.write_text(json.dumps(rec))
    return cmd_report(argparse.Namespace(out=a.out, tag=a.tag, first=None))


def split_detections(mutant, diff):
    """Split a differential by how much it can be trusted.

    `finding`: a new finding-tier result (FPs are never budget-exempt).
    `exposed`: a new exposed-tier miss, with no deadline in the mutant's arm.
    `budget_cut`: a new exposed-tier miss where the mutant's arm reported
      `incomplete: true`; under load these appear with no fault at all, so they are
      reported but never counted as a detection.
    `pre_check`: classify's consistency pre-check tier, informational only."""
    out = {"finding": {}, "exposed": {}, "budget_cut": {}, "pre_check": {}}
    for case, fs in diff.items():
        arms = mutant[case]["arms"]
        for f in fs:
            if f["tier"] == "finding":
                key = "finding"
            elif f["tier"] == "pre-check":
                key = "pre_check"
            elif (arms.get(f["arm"], {}).get("summary") or {}).get("incomplete"):
                key = "budget_cut"
            else:
                key = "exposed"
            out[key].setdefault(case, []).append(f)
    return out


def cmd_detect(a):
    control = load(a.out, a.control, a.first)
    mutant = load(a.out, a.mutant, a.first)
    diff = differential(control, mutant)
    split = split_detections(mutant, diff)
    counted = sorted(set(split["finding"]) | set(split["exposed"]))
    first = {}
    for case in sorted(diff):
        for f in diff[case]:
            first.setdefault(f["kind"], case)
    rep = {
        "control": a.control, "mutant": a.mutant, "n_compared": len(mutant),
        "detecting_cases": len(counted),
        "detecting_cases_by_split": {k: len(v) for k, v in split.items()},
        "first_detecting_case": counted[0] if counted else None,
        "differing_cases": len(diff),
        "by_kind": dict(Counter(f["kind"] for fs in diff.values() for f in fs)),
        "by_kind_tier": dict(Counter(f"{f['kind']}/{f['tier']}" for fs in diff.values()
                                     for f in fs)),
        "first_case_by_kind": first,
        "cases": {c: fs[:5] for c, fs in sorted(diff.items())[:20]},
    }
    Path(a.out, a.mutant, f"detect-vs-{a.control}.json").write_text(json.dumps(rep, indent=1))
    print(json.dumps(rep, indent=1))
    return 0


# ------------------------------------------------------------------ self-test
def self_test():
    """Fabricated (oracle, rustdl) triples: one per kind and gate tier."""
    A, B, C = "http://g#A", "http://g#B", "http://g#C"
    classes = {A, B, C}
    cons = {"consistent": True, "unsat": [], "thing_equiv": [], "pairs": [[A, B]]}
    inc = {"consistent": False}

    def cj(**kw):
        j = {"consistent": True, "incomplete": False, "completeness_guaranteed": True,
             "unsatisfiable": [], "equivalent_groups": [], "direct_subsumptions": [[A, B]]}
        j.update(kw)
        return {"status": "ok", "json": j}

    def kj(**kw):
        j = {"consistent": True, "incomplete": False, "relies_on_trusted_consistency": False,
             "wedge_clauses_refused": 0, "dropped": {}}
        j.update(kw)
        return {"status": "ok", "json": j}

    def kinds(o, **arms):
        base = {"classify": cj(), "satonly": cj(), "consistent": kj()}
        base.update(arms)
        return sorted((f["kind"], f["tier"], f["witness"]) for f in score(o, classes, base))

    fails = 0

    def ok(name, got, want):
        nonlocal fails
        good = got == want
        fails += not good
        print(f"{'ok  ' if good else 'FAIL'} {name}: got {got}, want {want}")

    ok("all agree is clean", kinds(cons), [])
    ok("pair-FP", kinds(cons, classify=cj(direct_subsumptions=[[A, B], [B, C]])),
       [("pair-FP", "finding", f"{A}|{C}"), ("pair-FP", "finding", f"{B}|{C}")])
    ok("pair-FP is not budget-exempt",
       kinds(cons, classify=cj(direct_subsumptions=[[A, B], [B, C]], incomplete=True,
                               completeness_guaranteed=False))[:1],
       [("pair-FP", "finding", f"{A}|{C}")])
    ok("unsat-FP", kinds(cons, classify=cj(unsatisfiable=[C])), [("unsat-FP", "finding", C)])
    ok("Thing-equivalence FP",
       kinds(cons, classify=cj(equivalent_groups=[[THING, C]])),
       [("pair-FP", "finding", f"{THING}|{C}")])
    ok("consistency-FP on classify", kinds(cons, classify=cj(consistent=False)),
       [("consistency-FP", "finding", "KB")])
    ok("consistency-FP on consistent", kinds(cons, consistent=kj(consistent=False)),
       [("consistency-FP", "finding", "KB")])
    ok("satonly-FP", kinds(cons, satonly=cj(direct_subsumptions=[[A, B], [C, A]])),
       [("satonly-FP", "finding", f"{C}|{A}"), ("satonly-FP", "finding", f"{C}|{B}")])
    ok("satonly misses are not scored", kinds(cons, satonly=cj(direct_subsumptions=[])), [])
    ok("MISSED-guaranteed", kinds(cons, classify=cj(direct_subsumptions=[])),
       [("pair-MISSED", "finding", f"{A}|{B}")])
    ok("MISSED-exposed",
       kinds(cons, classify=cj(direct_subsumptions=[], completeness_guaranteed=False)),
       [("pair-MISSED", "exposed", f"{A}|{B}")])
    ok("unsat-MISSED exposed",
       kinds(dict(cons, unsat=[C]), classify=cj(completeness_guaranteed=False)),
       [("unsat-MISSED", "exposed", C)])
    ok("oracle inconsistent: classify consistent is pre-check, consistent is a finding",
       kinds(inc), [("consistency-MISSED", "finding", "KB"),
                    ("consistency-MISSED", "pre-check", "KB")])
    ok("oracle inconsistent: gated consistent miss is exposed",
       kinds(inc, consistent=kj(relies_on_trusted_consistency=True)),
       [("consistency-MISSED", "exposed", "KB"), ("consistency-MISSED", "pre-check", "KB")])
    ok("oracle inconsistent: refused clauses expose it",
       kinds(inc, consistent=kj(wedge_clauses_refused=2)),
       [("consistency-MISSED", "exposed", "KB"), ("consistency-MISSED", "pre-check", "KB")])
    ok("oracle inconsistent, rustdl agrees: clean",
       kinds(inc, classify=cj(consistent=False), satonly=cj(consistent=False),
             consistent=kj(consistent=False)), [])
    ok("Thing-equivalent class excluded from pairs",
       kinds(dict(cons, thing_equiv=[C], pairs=[[A, B]]),
             classify=cj(direct_subsumptions=[[A, B], [A, C]], equivalent_groups=[[THING, C]])),
       [])
    rec = {"oracle": {"verdict": "agree"}, "arms": {"classify": {"status": "timeout"}},
           "findings": []}
    ok("timeout bucket", bucket(rec), "rustdl-fail")
    m = {"x": {"oracle": {"verdict": "agree"},
               "findings": [{"kind": "pair-FP", "tier": "finding", "witness": "a|b"},
                            {"kind": "pair-MISSED", "tier": "exposed", "witness": "c|d"}]}}
    c = {"x": {"oracle": {"verdict": "agree"},
               "findings": [{"kind": "pair-MISSED", "tier": "exposed", "witness": "c|d"}]}}
    ok("differential keeps only new findings",
       [f["kind"] for f in differential(c, m)["x"]], ["pair-FP"])
    mm = {"x": {"arms": {"classify": {"summary": {"incomplete": True}}},
                "oracle": {"verdict": "agree"}}}
    sp = split_detections(mm, {"x": [{"kind": "pair-MISSED", "tier": "exposed", "arm": "classify",
                                       "witness": "a|b"},
                                      {"kind": "pair-FP", "tier": "finding", "arm": "classify",
                                       "witness": "c|d"}]})
    ok("a deadline-cut exposed miss is not a detection, an FP is",
       ({k: len(v) for k, v in sp.items()}),
       {"finding": 1, "exposed": 0, "budget_cut": 1, "pre_check": 0})
    print(f"run self-test: {'all ok' if not fails else f'{fails} FAILED'}")
    return 1 if fails else 0


def main():
    if "--self-test" in sys.argv:
        return self_test()
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("gen")
    p.add_argument("--seed", type=int, required=True)
    p.add_argument("--n", type=int, required=True)
    p.add_argument("--start", type=int, default=0)
    p.add_argument("--cases", required=True)
    p.add_argument("--profile", default="default")
    p = sub.add_parser("run")
    p.add_argument("--cases", required=True)
    p.add_argument("--bin", required=True)
    p.add_argument("--tag", required=True)
    p.add_argument("--out", required=True)
    p.add_argument("--env", action="append")
    p.add_argument("--first", type=int)
    p.add_argument("--cores", default="92,93,94")
    p.add_argument("--oracle-cache", required=True)
    p = sub.add_parser("report")
    p.add_argument("--out", required=True)
    p.add_argument("--tag", required=True)
    p.add_argument("--first", type=int)
    p = sub.add_parser("resides")
    p.add_argument("--out", required=True)
    p.add_argument("--tag", required=True)
    p.add_argument("--oracle-cache", required=True)
    p = sub.add_parser("detect")
    p.add_argument("--out", required=True)
    p.add_argument("--control", required=True)
    p.add_argument("--mutant", required=True)
    p.add_argument("--first", type=int)
    a = ap.parse_args()
    return {"gen": cmd_gen, "run": cmd_run, "report": cmd_report, "detect": cmd_detect,
            "resides": cmd_resides}[a.cmd](a)


if __name__ == "__main__":
    sys.exit(main())
