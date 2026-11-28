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
  classify --json, classify --saturation-only --json, consistent --json; with --realize
  also realize --json and realize --saturation-only --json (role-polarity fuzz).

With --realize the PEERS classify an augmented copy that adds `N_x ≡ {x}` for each named
individual x (fresh names: a conservative extension), so `x : C` iff `N_x ⊑ C`; rustdl
always runs on the original file (nominals would switch rustdl's nominal gates on).
Agreement is computed separately on the original classes (the classify / consistent
arms) and on the `N_x` rows (the realize arms), so a disagreement on one projection
does not unscore the other. `realize` exits 1 on an inconsistent KB: that is read as a
verdict, not an arm failure.

Scoring (plan v2 §3). FP kinds are findings whatever `incomplete` says:
  pair-FP, unsat-FP, consistency-FP, satonly-FP.
MISSED kinds are split by rustdl's own guarantee:
  MISSED-guaranteed (`completeness_guaranteed: true`): a finding;
  MISSED-exposed (otherwise): a declared limitation, reported, not a finding.
Consistency MISSED: on classify's `consistent` field it is a pre-check tier,
informational only; on `consistent --json` it is a finding only when `incomplete`,
`relies_on_trusted_consistency`, `universal_role_ordinary` (#543) and `data_case_split_risk`
(#589), each a declared reason a negative answer may be wrong, are false, `wedge_clauses_refused` is 0 and nothing was
dropped. If the oracle says inconsistent, only consistency is scored.

Case buckets, exclusive, in this order: invalid, peer-fail, peer-disagree,
rustdl-fail, finding, exposed, clean.
"""
import argparse
import json
import os
import re
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
REALIZE_ARMS = {
    "realize": ["realize", "--json"],
    "realize_satonly": ["realize", "--saturation-only", "--json"],
}
ALL_ARMS = {**ARMS, **REALIZE_ARMS}
FP_KINDS = ("pair-FP", "unsat-FP", "consistency-FP", "satonly-FP", "type-FP",
            "satonly-type-FP", "sat-FP")
# `--sat`: the per-query surface (`rustdl sat FILE C` for every declared class), which the
# classify arms do not exercise: it reaches the main tableau's rules (e.g. apply_max) on
# paths classify answers elsewhere. One call per class, each under RUSTDL_CAP.
SAT_ARM = "sat"
INCONSISTENT_ERR = "ontology is inconsistent"


def arms_for(realize, sat=False):
    arms = dict(ALL_ARMS) if realize else dict(ARMS)
    if sat:
        arms[SAT_ARM] = None
    return arms


def run_sat_arm(binary, path, env):
    """{"status": "ok", "json": {"answers": {C: sat|unsat|unknown|timeout|fail}}}."""
    e = {**os.environ, **env}
    answers = {}
    for c in sorted(orc.declared_classes(Path(path).read_text())):
        try:
            p = subprocess.run([binary, "sat", str(path), c], capture_output=True, text=True,
                               timeout=RUSTDL_CAP, env=e, preexec_fn=_limit)
        except subprocess.TimeoutExpired:
            answers[c] = "timeout"
            continue
        first = (p.stdout.strip().splitlines() or [""])[0]
        if first in ("sat", "unsat", "unknown"):
            answers[c] = first
        elif p.returncode == 1 and INCONSISTENT_ERR in p.stderr:
            answers[c] = "inconsistent"  # `sat` refuses an inconsistent KB: a verdict
        else:
            answers[c] = "fail"
    return {"status": "ok", "json": {"answers": answers}}


def score_sat(oracle_ans, arm):
    """`sat` per class. An `unsat` the oracle refutes is a finding (`sat-FP`); a `sat` on a
    class the oracle calls unsat (or on any class of an inconsistent KB) is exposed only
    (the per-query surface claims no completeness). Give-ups are never scored."""
    out = []
    j = arm.get("json") if arm and arm.get("status") == "ok" else None
    if j is None:
        return out
    o_unsat = None if not oracle_ans["consistent"] else set(oracle_ans["unsat"])
    if o_unsat is not None and "inconsistent" in j["answers"].values():
        out.append({"kind": "consistency-FP", "tier": "finding", "witness": "KB", "arm": SAT_ARM})
    for c, a in sorted(j["answers"].items()):
        if a == "unsat" and o_unsat is not None and c not in o_unsat:
            out.append({"kind": "sat-FP", "tier": "finding", "witness": c, "arm": SAT_ARM})
        elif a == "sat" and (o_unsat is None or c in o_unsat):
            out.append({"kind": "sat-MISSED", "tier": "exposed", "witness": c, "arm": SAT_ARM})
    return out


# ------------------------------------------------------------------ rustdl
def _limit():
    resource.setrlimit(resource.RLIMIT_AS, (8_000_000_000, 8_000_000_000))


def run_arm(binary, arm, path, env):
    if arm == SAT_ARM:
        return run_sat_arm(binary, path, env)
    e = {**os.environ, **env}
    try:
        p = subprocess.run([binary, *ALL_ARMS[arm], str(path)], capture_output=True,
                           text=True, timeout=RUSTDL_CAP, env=e, preexec_fn=_limit)
    except subprocess.TimeoutExpired:
        return {"status": "timeout"}
    try:
        return {"status": "ok", "rc": p.returncode, "json": json.loads(p.stdout)}
    except json.JSONDecodeError:
        if arm in REALIZE_ARMS and p.returncode == 1 and INCONSISTENT_ERR in p.stderr:
            # realize refuses an inconsistent KB: a verdict, scored as such.
            return {"status": "ok", "rc": 1, "json": {"consistent": False}}
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


# ------------------------------------------------------------------ realize oracle
_IND = re.compile(r"Declaration\(\s*NamedIndividual\(\s*:(\w+)\s*\)\s*\)")
_BASE = re.compile(r"Prefix\(\s*:\s*=\s*<([^>]*)>\s*\)")


def augment(text):
    """(augmented text, {N_x IRI: x IRI}): `N__x ≡ {x}` for each declared individual."""
    m = _BASE.search(text)
    assert m, "augment expects a `Prefix(:=<...>)` file (the generator's form)"
    base = m.group(1)
    inds = _IND.findall(text)
    assert len(inds) == text.count("Declaration(NamedIndividual("), \
        "augment reads `:name` individual declarations only"
    lines = text.rstrip("\n").splitlines()
    assert lines[-1].strip() == ")", "expects a closing ')'"
    add = []
    for x in inds:
        add += [f"Declaration(Class(:N__{x}))", f"EquivalentClasses(:N__{x} ObjectOneOf(:{x}))"]
    return ("\n".join(lines[:-1] + add + [")"]) + "\n",
            {f"{base}N__{x}": f"{base}{x}" for x in inds})


def restrict(peer, keep):
    """A peer answer restricted to the classes in `keep`."""
    if peer.get("status") != "ok" or not peer.get("consistent"):
        return peer
    return {**peer, "unsat": sorted(set(peer["unsat"]) & keep),
            "thing_equiv": sorted(set(peer["thing_equiv"]) & keep),
            "pairs": sorted([a, b] for a, b in peer["pairs"] if a in keep and b in keep)}


def peer_types(peer, classes, nmap):
    """{x: sorted named types} from a consistent peer answer on the augmented file.

    x : C iff N_x ⊑ C. The peers' pair sets carry equivalences both ways and omit
    nothing for N_x (a consistent KB never makes N_x unsat). A Thing-equivalent C is a
    type of every x. If N_x is itself Thing-equivalent (the domain is {x}), x : C iff
    ⊤ ⊑ C, so its types are exactly the Thing-equivalent classes."""
    te = set(peer["thing_equiv"])
    pairs = {tuple(q) for q in peer["pairs"]}
    out = {}
    for n, x in nmap.items():
        if n in te:
            out[x] = sorted(te & classes)
        else:
            out[x] = sorted(c for c in classes if c in te or (n, c) in pairs)
    return out


def project(ora, classes, nmap):
    """Split an augmented-file oracle record into the classify projection (original
    classes) and the types projection (the N_x rows), each with its own agreement."""
    k, h = ora["konclude"], ora["hermit"]
    rec = {"sha256": ora["sha256"], "verdict": ora["verdict"], "kinds": ora["kinds"],
           "answer": None, "konclude": k, "hermit": h, "types": {"verdict": "n/a"}}
    if ora["verdict"] in ("invalid", "peer-fail"):
        return rec
    kr, hr = restrict(k, classes), restrict(h, classes)
    verdict, kinds, ans = orc.agree(kr, hr)
    rec.update(verdict=verdict, kinds=kinds, answer=ans, konclude=kr, hermit=hr)
    if verdict == "agree" and ans["consistent"]:
        kt, ht = peer_types(k, classes, nmap), peer_types(h, classes, nmap)
        if kt == ht:
            rec["types"] = {"verdict": "agree", "types": kt}
        else:
            rec["types"] = {"verdict": "peer-disagree",
                            "individuals": sorted(x for x in kt if kt[x] != ht.get(x))}
    return rec


def oracle_for(path, text, cache, realize, aug_dir):
    """The oracle record for one case: plain, or projected from the augmented file."""
    if not realize:
        return orc.oracle(path, cache)
    aug, nmap = augment(text)
    ap = Path(aug_dir) / f"{orc.sha(aug)[:20]}.ofn"
    if not ap.exists():
        ap.parent.mkdir(parents=True, exist_ok=True)
        ap.write_text(aug)
    return project(orc.oracle(ap, cache), orc.declared_classes(text), nmap)


def score_types(types_rec, consistent, classes, arms):
    """Findings of the realize arms. FPs are findings; a missed type is exposed only
    (realize claims no completeness)."""
    out = []

    def add(kind, tier, witness, arm):
        out.append({"kind": kind, "tier": tier, "witness": witness, "arm": arm})

    for arm, fp_kind in (("realize", "type-FP"), ("realize_satonly", "satonly-type-FP")):
        a = arms.get(arm, {})
        j = a.get("json") if a.get("status") == "ok" else None
        if j is None:
            continue
        if not consistent:
            if j.get("consistent", True) and arm == "realize":
                add("consistency-MISSED", "exposed", "KB", arm)
            continue
        if j.get("consistent") is False:
            add("consistency-FP", "finding", "KB", arm)
            continue
        if types_rec.get("verdict") != "agree":
            continue
        o = types_rec["types"]
        for ind in j.get("individuals", []):
            x = ind["iri"]
            if x not in o:
                continue
            got = set(ind.get("types", [])) & classes
            for c in sorted(got - set(o[x])):
                add(fp_kind, "finding", f"{x}|{c}", arm)
            if arm == "realize":
                for c in sorted(set(o[x]) - got):
                    add("type-MISSED", "exposed", f"{x}|{c}", arm)
    return out


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
            # #543 / #589: `universal_role_ordinary` and `data_case_split_risk` declare
            # that a negative answer (here "consistent") may be wrong, so a miss under
            # either is exposed, not a finding.
            gated = (not kj.get("incomplete") and not kj.get("relies_on_trusted_consistency")
                     and not kj.get("wedge_clauses_refused") and not kj.get("dropped")
                     and not kj.get("universal_role_ordinary")
                     and not kj.get("data_case_split_risk") and not collapsed(kj))
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
        # #538: rustdl reads n-ary operands as a set (`collapsed_operands`), the peers as a
        # list, so `DisjointClasses(:C :C)` makes C unsat for them only: declared, exposed.
        tier = ("finding" if j.get("completeness_guaranteed") and not collapsed(j)
                else "exposed")
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


def _cpu():
    """CPU seconds of this worker and its reaped children (the peers and rustdl). Read
    per case because the pool's forkserver workers are never reaped into the parent's
    rusage, so a `time` around run.py misses nearly all of it."""
    s, c = resource.getrusage(resource.RUSAGE_SELF), resource.getrusage(resource.RUSAGE_CHILDREN)
    return s.ru_utime + s.ru_stime + c.ru_utime + c.ru_stime


def one_case(path, binary, env, cache, out_dir, realize=False, sat=False):
    dest = Path(out_dir) / "cases" / (Path(path).stem + ".json")
    if dest.exists():
        return json.loads(dest.read_text())
    cpu0 = _cpu()
    text = Path(path).read_text()
    classes = orc.declared_classes(text)
    ora = oracle_for(path, text, cache, realize, Path(out_dir).parent / "augmented")
    head = text.splitlines()[:3]
    motif = next((l.split(":", 1)[1].strip() for l in head if l.startswith("# motif:")), None)
    stratum = next((l.split(":", 1)[1].strip() for l in head
                    if l.startswith("# data-stratum:")), None)
    polarity = next((l.split(":", 1)[1].strip() for l in head if l.startswith("# polarity:")),
                    None)
    rec = {"case": Path(path).stem, "header": text.splitlines()[0], "motif": motif,
           "stratum": stratum,
           "polarity": polarity, "sha256": ora["sha256"],
           "oracle": {"verdict": ora["verdict"], "kinds": ora["kinds"]}, "arms": {},
           "findings": []}
    if realize:
        rec["oracle_types"] = ora["types"]
    if ora["verdict"] in ("agree", "peer-disagree"):
        for arm in arms_for(realize, sat):
            rec["arms"][arm] = run_arm(binary, arm, path, env)
    if ora["verdict"] == "agree":
        rec["oracle_consistent"] = ora["answer"]["consistent"]
        rec["findings"] = score(ora["answer"], classes, rec["arms"])
        if realize:
            rec["findings"] += score_types(ora["types"], ora["answer"]["consistent"], classes,
                                           rec["arms"])
            rec["types_scored"] = (len(ora["types"]["types"])
                                   if ora["types"].get("verdict") == "agree" else 0)
            rz = rec["arms"].get("realize", {}).get("json") or {}
            rec["types_compared"] = (sum(1 for i in rz.get("individuals", [])
                                         if i["iri"] in ora["types"]["types"])
                                     if ora["types"].get("verdict") == "agree" else 0)
        if sat:
            rec["findings"] += score_sat(ora["answer"], rec["arms"][SAT_ARM])
            sa = rec["arms"][SAT_ARM]
            bad = sorted(c for c, x in sa["json"]["answers"].items() if x in ("fail", "timeout"))
            if bad and ora["answer"]["consistent"]:
                # A crash or a cap on a KB the peers call consistent is a failure, not clean.
                sa["status"] = "fail"
                sa["failed_classes"] = bad[:5]
    elif ora["verdict"] == "peer-disagree":
        rec["sides"] = sides(ora, classes, rec["arms"])
        hans = hermit_superset(ora)
        if hans is not None:
            # Declared before the data_range control (#363): Konclude under-reports data
            # entailments (case splits, counting), so where HermiT's answer strictly
            # contains Konclude's, rustdl is also scored against HermiT, in a separate tier.
            # The case stays peer-disagree; every finding here is adjudicated by hand.
            rec["hermit_superset"] = True
            rec["hermit_findings"] = score(hans, classes, rec["arms"])
            if sat and SAT_ARM in rec["arms"] and rec["arms"][SAT_ARM].get("json"):
                rec["hermit_findings"] += score_sat(hans, rec["arms"][SAT_ARM])
    rec["bucket"] = bucket(rec)
    # keep the per-case record small: drop the raw JSON of clean arms
    for name, a in rec["arms"].items():
        j = a.pop("json", None)
        if name == SAT_ARM and j is not None:
            vals = list(j["answers"].values())
            a["summary"] = {k: vals.count(k) for k in ("sat", "unsat", "unknown", "inconsistent",
                                                         "timeout", "fail")}
            continue
        if j is not None:
            a["summary"] = {k: j.get(k) for k in (
                "consistent", "incomplete", "completeness_guaranteed",
                "relies_on_trusted_consistency", "wedge_clauses_refused",
                "trusted_sat_refutations", "wedge_hierarchy_blind", "dropped",
                "consistency_undetermined", "universal_role_ordinary",
                "data_case_split_risk", "collapsed_operands")}
    rec["cpu_s"] = round(_cpu() - cpu0, 3)
    dest.parent.mkdir(parents=True, exist_ok=True)
    tmp = dest.with_suffix(".part")
    tmp.write_text(json.dumps(rec))
    tmp.replace(dest)
    return rec


def collapsed(j):
    """#538: an n-ary axiom repeated an operand, read as a set by rustdl, as a list by the
    peers (`docs/json-schema.md`, `collapsed_operands`)."""
    return bool((j.get("collapsed_operands") or {}).get("axioms"))


def hermit_superset(ora):
    """HermiT's answer when it strictly contains Konclude's (every Konclude entailment is
    HermiT's, and HermiT has more); else None. Inconsistency contains everything."""
    k, h = ora.get("konclude") or {}, ora.get("hermit") or {}
    if k.get("status") != "ok" or h.get("status") != "ok":
        return None
    if not h["consistent"]:
        return {"consistent": False} if k["consistent"] else None
    if not k["consistent"]:
        return None
    hu, hte = set(h["unsat"]), set(h["thing_equiv"])
    if not set(k["unsat"]) <= hu or not set(k["thing_equiv"]) <= hte | hu:
        return None
    excl = hu | hte
    kp, hp = orc.pairs_excluding(k, excl), orc.pairs_excluding(h, excl)
    if not kp <= hp:
        return None
    return {"consistent": True, "unsat": h["unsat"], "thing_equiv": h["thing_equiv"],
            "pairs": sorted([s, t2] for s, t2 in hp)}


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
        "realize": a.realize, "sat": a.sat, "gen_hash": gen.gen_hash(),
        "oracle_version": orc.ORACLE_VERSION},
        indent=1))
    cores = [int(c) for c in a.cores.split(",")]
    import multiprocessing as mp
    q = mp.Manager().Queue()
    for c in cores:
        q.put(c)
    done = 0
    with futures.ProcessPoolExecutor(max_workers=len(cores), initializer=_pin,
                                     initargs=(q,)) as ex:
        futs = {ex.submit(one_case, str(f), a.bin, env, a.oracle_cache, str(out), a.realize,
                          a.sat): f
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
    def guaranteed(rs):
        """Share of agreed, consistent, classify-ok cases reporting completeness_guaranteed:
        the cases on which a classify miss is a finding."""
        n = g = 0
        for r in rs:
            s = (r["arms"].get("classify") or {}).get("summary") or {}
            if r["oracle"]["verdict"] == "agree" and s.get("consistent"):
                n += 1
                g += bool(s.get("completeness_guaranteed"))
        return {"scored": n, "guaranteed": g, "rate": round(g / n, 4) if n else None}

    by_motif = {}
    for r in recs.values():
        if r.get("motif"):
            by_motif.setdefault(r["motif"], []).append(r)

    def denominators(rs):
        """What a motif's cases actually scored: agreed cases, consistent or not, and
        individuals whose types both peers agree on. A zero here is not a clean result."""
        rs = list(rs)
        agreed = [r for r in rs if r["oracle"]["verdict"] == "agree"]
        return {"cases": len(rs), "agreed": len(agreed),
                "agreed_consistent": sum(r.get("oracle_consistent") is True for r in agreed),
                "peer_disagree": sum(r["bucket"] == "peer-disagree" for r in rs),
                "types_peer_disagree": sum((r.get("oracle_types") or {}).get("verdict")
                                           == "peer-disagree" for r in rs),
                "individuals_scored": sum(r.get("types_scored", 0) for r in rs),
                # individuals the realize arm actually reported and that were compared
                "individuals_compared": sum(r.get("types_compared", 0) for r in rs),
                "finding_cases": sum(r["bucket"] == "finding" for r in rs)}
    def summ(r, arm="classify"):
        return (r["arms"].get(arm) or {}).get("summary") or {}

    def tiers(rs):
        """The data_range tiers: agreed consistent classify cases, those certified
        (`completeness_guaranteed`), and those certified with no #589 split risk."""
        rs = [r for r in rs if r["oracle"]["verdict"] == "agree" and summ(r).get("consistent")]
        g = [r for r in rs if summ(r).get("completeness_guaranteed")]
        return {"scored": len(rs), "guaranteed": len(g),
                "guaranteed_no_split_risk": sum(not summ(r).get("data_case_split_risk")
                                                for r in g),
                "split_risk": sum(bool(summ(r).get("data_case_split_risk")) for r in rs)}

    by_stratum = {}
    for r in recs.values():
        if r.get("stratum"):
            by_stratum.setdefault(r["stratum"], []).append(r)

    def miss_reason(r):
        """Why an exposed classify miss is not a finding: the first declared reason."""
        s = summ(r)
        if collapsed(s):
            return "collapsed_operands"
        for k in ("incomplete", "dropped", "data_case_split_risk", "universal_role_ordinary",
                  "relies_on_trusted_consistency", "wedge_hierarchy_blind"):
            if s.get(k):
                return k
        if (s.get("trusted_sat_refutations") or 0) > 0:
            return "trusted_sat_refutations"
        return "not_guaranteed_other"

    miss_reasons = Counter(miss_reason(r) for r in recs.values()
                           if any(f["tier"] == "exposed" and f["kind"].endswith("MISSED")
                                  and f["arm"] == "classify" for f in r["findings"]))
    # #589 / D10: a certificate must never coexist with a declared reason to doubt it.
    cert_conflicts = sorted(
        c for c, r in recs.items() if summ(r).get("completeness_guaranteed")
        and (summ(r).get("data_case_split_risk") or summ(r).get("dropped")
             or summ(r).get("incomplete")))
    rep = {
        "tag": a.tag, "n": len(recs), "buckets": dict(buckets),
        "tiers_by_stratum": {s: tiers(rs) for s, rs in sorted(by_stratum.items())},
        "classify_exposed_miss_reasons": dict(miss_reasons),
        "hermit_superset": {
            "cases": sum(bool(r.get("hermit_superset")) for r in recs.values()),
            "finding_cases": sorted(c for c, r in recs.items() if any(
                f["tier"] == "finding" for f in r.get("hermit_findings", []))),
            "findings_by_kind_tier": dict(Counter(
                f"{f['kind']}/{f['tier']}" for r in recs.values()
                for f in r.get("hermit_findings", [])))},
        "certificate_conflicts": cert_conflicts,
        "guaranteed": guaranteed(recs.values()),
        "guaranteed_by_motif": {m: guaranteed(rs) for m, rs in sorted(by_motif.items())},
        "denominators_by_motif": {m: denominators(rs) for m, rs in sorted(by_motif.items())},
        "findings_by_kind_tier": {f"{k}/{t}": n for (k, t), n in sorted(kinds.items())},
        "cases_by_kind_tier": {f"{k}/{t}": n for (k, t), n in sorted(case_kinds.items())},
        "finding_cases": sorted(c for c, r in recs.items() if r["bucket"] == "finding"),
        "peer_disagree": {f"{'+'.join(k)} rustdl-sides:{s}": n for (k, s), n in disagree.items()},
        "peer_fail_or_invalid": {"+".join(k): n for k, n in peer_fail.items()},
        "rustdl_arm_failures": dict(fails),
        "sat_answers": dict(sum((Counter((r["arms"].get(SAT_ARM) or {}).get("summary") or {})
                                 for r in recs.values()), Counter())),
        "sat_fail_on_agreed_consistent": sorted(
            c for c, r in recs.items() if (r["arms"].get(SAT_ARM) or {}).get("status") == "fail"),
        "cpu_s_measured": round(sum(r.get("cpu_s", 0) for r in recs.values()), 1),
        "cases_without_cpu": sum("cpu_s" not in r for r in recs.values()),
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
    hdiff = {}
    for case, m in mutant.items():
        c = control.get(case)
        if c is None or not m.get("hermit_superset"):
            continue
        base = {(f["kind"], f["witness"]) for f in c.get("hermit_findings", [])}
        new = [f for f in m.get("hermit_findings", []) if (f["kind"], f["witness"]) not in base
               and f["tier"] == "finding"]
        if new:
            hdiff[case] = new
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
        "hermit_superset_detecting_cases": sorted(hdiff),
        "by_kind": dict(Counter(f["kind"] for fs in diff.values() for f in fs)),
        "by_kind_tier": dict(Counter(f"{f['kind']}/{f['tier']}" for fs in diff.values()
                                     for f in fs)),
        "first_case_by_kind": first,
        "cases": {c: fs[:5] for c, fs in sorted(diff.items())[:20]},
        # every detecting case (the 20-case `cases` sample above truncates: #638 review)
        "all_detecting_cases": counted,
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
    ok("oracle inconsistent: a declared universal role exposes it",
       kinds(inc, consistent=kj(universal_role_ordinary=True)),
       [("consistency-MISSED", "exposed", "KB"), ("consistency-MISSED", "pre-check", "KB")])
    ok("oracle inconsistent: a declared data case split exposes it",
       kinds(inc, consistent=kj(data_case_split_risk=True)),
       [("consistency-MISSED", "exposed", "KB"), ("consistency-MISSED", "pre-check", "KB")])
    ok("a consistency-FP stays a finding under a declared universal role",
       kinds(cons, consistent=kj(consistent=False, universal_role_ordinary=True)),
       [("consistency-FP", "finding", "KB")])
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
    # #502: oracle.declared_classes reads full IRIs and any prefix, not only `:Name`.
    ok("declared_classes: `:`, `<IRI>` and `ex:` forms",
       sorted(orc.declared_classes(
           "Prefix(:=<http://g#>)\nPrefix(ex:=<http://x#>)\nOntology(\n"
           "Declaration(Class(:A))\nDeclaration(Class(<http://y#B>))\n"
           "Declaration(Class(ex:C))\nDeclaration(Class(owl:Thing))\n"
           "Declaration(ObjectProperty(:r))\n)")),
       ["http://g#A", "http://x#C", "http://y#B"])
    # ---- the realize arm (role-polarity fuzz)
    aug, nmap = augment("# gen x\nPrefix(:=<http://g#>)\nOntology(<http://g/o>\n"
                        "Declaration(Class(:A))\nDeclaration(NamedIndividual(:a))\n"
                        "Declaration(NamedIndividual(:b))\nClassAssertion(:A :a)\n)\n")
    ok("augment adds N_x ≡ {x} per individual",
       ("EquivalentClasses(:N__a ObjectOneOf(:a))" in aug,
        "Declaration(Class(:N__b))" in aug, nmap),
       (True, True, {"http://g#N__a": "http://g#a", "http://g#N__b": "http://g#b"}))
    Na, Nb, xa, xb = "http://g#N__a", "http://g#N__b", "http://g#a", "http://g#b"

    def peer(pairs, te=(), unsat=()):
        return {"status": "ok", "consistent": True, "unsat": sorted(unsat),
                "thing_equiv": sorted(te), "pairs": sorted(list(q) for q in pairs)}
    nm = {Na: xa, Nb: xb}
    ok("types: N_x ⊑ C rows, Thing-equivalent classes added",
       peer_types(peer([(Na, A), (Na, B)], te=[C]), classes, nm),
       {xa: [A, B, C], xb: [C]})
    ok("types: C ≡ {x} (pairs both ways) gives x : C; N_y rows never leak",
       peer_types(peer([(Na, A), (A, Na), (Na, Nb), (Nb, Na)]), classes, nm),
       {xa: [A], xb: []})
    ok("types: N_x Thing-equivalent means exactly the Thing-equivalent classes",
       peer_types(peer([(Na, A)], te=[Na, B]), classes, nm), {xa: [B], xb: [B]})
    ora = {"sha256": "s", "verdict": "peer-disagree", "kinds": ["pairs"],
           "konclude": peer([(A, B), (Na, A)]), "hermit": peer([(A, B), (Na, A), (Na, B)])}
    pr = project(ora, classes, nm)
    ok("a types-only disagreement keeps the classify projection scored",
       (pr["verdict"], pr["answer"]["pairs"], pr["types"]["verdict"], pr["types"]["individuals"]),
       ("agree", [[A, B]], "peer-disagree", [xa]))
    ora["hermit"] = peer([(A, B), (Na, A)])
    pr = project(ora, classes, nm)
    ok("agreeing peers give the types", pr["types"], {"verdict": "agree",
                                                      "types": {xa: [A], xb: []}})
    trec = {"verdict": "agree", "types": {xa: [A], xb: []}}

    def rz(types_a, cons=None):
        j = {"individuals": [{"iri": xa, "types": types_a}, {"iri": xb, "types": []}]}
        if cons is not None:
            j = {"consistent": cons}
        return {"status": "ok", "json": j}

    def tk(cons, **arms):
        return sorted((f["kind"], f["tier"], f["witness"], f["arm"])
                      for f in score_types(trec, cons, classes, arms))
    ok("types agree: clean", tk(True, realize=rz([A]), realize_satonly=rz([A])), [])
    ok("type-FP and satonly-type-FP are findings",
       tk(True, realize=rz([A, B]), realize_satonly=rz([C])),
       [("satonly-type-FP", "finding", f"{xa}|{C}", "realize_satonly"),
        ("type-FP", "finding", f"{xa}|{B}", "realize")])
    ok("a missed type is exposed, and only on the full arm",
       tk(True, realize=rz([]), realize_satonly=rz([])),
       [("type-MISSED", "exposed", f"{xa}|{A}", "realize")])
    ok("realize refusing a consistent KB is a consistency-FP",
       tk(True, realize=rz([], cons=False)), [("consistency-FP", "finding", "KB", "realize")])
    ok("realize refusing an inconsistent KB is clean", tk(False, realize=rz([], cons=False)), [])
    ok("realize answering on an inconsistent KB is an exposed miss",
       tk(False, realize=rz([A])), [("consistency-MISSED", "exposed", "KB", "realize")])
    ok("types not agreed: no type scoring",
       sorted(f["kind"] for f in score_types({"verdict": "peer-disagree"}, True, classes,
                                             {"realize": rz([A, B])})), [])
    def sk(o, answers):
        return sorted((f["kind"], f["tier"], f["witness"])
                      for f in score_sat(o, {"status": "ok", "json": {"answers": answers}}))
    ok("sat agrees: clean", sk(dict(cons, unsat=[C]), {A: "sat", B: "sat", C: "unsat"}), [])
    ok("sat-FP is a finding, sat-MISSED exposed, unknown unscored",
       sk(dict(cons, unsat=[C]), {A: "unsat", B: "unknown", C: "sat"}),
       [("sat-FP", "finding", A), ("sat-MISSED", "exposed", C)])
    ok("an inconsistent KB: every sat answer is an exposed miss",
       sk(inc, {A: "sat", B: "unsat"}), [("sat-MISSED", "exposed", A)])
    ok("sat refusing a consistent KB is a consistency-FP",
       sk(cons, {A: "inconsistent", B: "inconsistent", C: "inconsistent"}),
       [("consistency-FP", "finding", "KB")])
    ok("sat refusing an inconsistent KB is clean", sk(inc, {A: "inconsistent"}), [])
    ok("a certified miss under collapsed operands (#538) is exposed",
       kinds(dict(cons, unsat=[C]), classify=cj(collapsed_operands={"axioms": 1, "vacuous": 1}))
       [:1], [("unsat-MISSED", "exposed", C)])
    ok("a certified miss with no collapsed operands stays a finding",
       kinds(dict(cons, unsat=[C]), classify=cj(collapsed_operands={"axioms": 0, "vacuous": 0}))
       [:1], [("unsat-MISSED", "finding", C)])
    # hermit_superset: HermiT strictly containing Konclude is scored; anything else is not.
    def peers(kp, hp, ku=(), hu=(), kc=True, hc=True):
        mk = lambda c, ps, u: {"status": "ok", "consistent": c, "unsat": list(u),
                               "thing_equiv": [], "pairs": [list(x) for x in ps]}
        return {"konclude": mk(kc, kp, ku), "hermit": mk(hc, hp, hu)}
    ok("HermiT ⊋ Konclude pairs is scored against HermiT",
       hermit_superset(peers([], [(A, B)]))["pairs"], [[A, B]])
    ok("Konclude has a pair HermiT lacks: not scored",
       hermit_superset(peers([(A, C)], [(A, B)])), None)
    ok("HermiT inconsistent, Konclude consistent: scored as inconsistent",
       hermit_superset(peers([], [], hc=False)), {"consistent": False})
    ok("Konclude inconsistent, HermiT consistent: not scored",
       hermit_superset(peers([], [], kc=False)), None)
    ok("Konclude unsat HermiT lacks: not scored",
       hermit_superset(peers([], [], ku=[C])), None)
    ok("a miss against the HermiT answer is scored like an agreed one",
       [x["kind"] for x in score(hermit_superset(peers([], [(A, B), (A, C)])), classes,
                                 {"classify": cj(direct_subsumptions=[[A, B]])})],
       ["pair-MISSED"])
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
    p.add_argument("--realize", action="store_true",
                   help="add the realize arms; the peers classify the N_x-augmented file")
    p.add_argument("--sat", action="store_true",
                   help="add the per-class `sat` arm (the per-query surface)")
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
