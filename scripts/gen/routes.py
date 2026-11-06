#!/usr/bin/env python3
"""Route-pair laws over the generated cases (#363 package E, stateful half).

Same question, different route: does rustdl give the same semantic answer? Plan:
notes/plans/2026-11-03-363-package-e-stateful.md (v2). Laws:

  R1  thread count: RAYON_NUM_THREADS=1 vs 8 (one core), --pair-timeout-ms 1000: equal.
  R2  default vs reference (a: RUSTDL_HYPERTABLEAU_TRUST_SAT=0, b: RUSTDL_HYPERTABLEAU=0),
      both at --pair-timeout-ms 1000. NOT "default ⊆ reference": asymmetries are
      adjudicated by the oracle. Violations: an FP on either route (R2.3), differing
      `consistent` (R2.4), a difference where the default reports completeness_guaranteed
      (R2.2, vacuous by construction: kept as a sanity check).
  R3  cold (one CLI process per case) vs warm (one Python process for every case, in
      order and shuffled, queries interleaved): equal answers (R3.1), order-independent
      (R3.2), Classification.is_subclass agrees with the closure (R3.3).
  R4  interruption: --global-timeout-ms 1/5/25 and --pair-timeout-ms 1 vs the full run.
      Compared whatever the CUT run's flags say; excluded only if the FULL run gave up.
      R4.1 cut ⊄ full (an FP when the oracle refutes it), R4.2 cut ≠ full without a
      give-up flag (a silent cut), R4.3 cut `consistent: false` while full is true.
  R5  fragment: R5.1 a case with a must-leave-PureEl token reports `pure-EL`;
      R5.2 an el_guaranteed case does not (gate cost, reported apart);
      R5.3 text output ≠ JSON output (serialisation only).
  R4.4 (#466) a cut route claims consistency determined (`consistent: true`,
      `consistency_undetermined: false`) while the full route says `consistent: false`,
      or the full route is undetermined and the oracle says inconsistent. `incomplete`
      does not excuse it: consistency has its own give-up field (the #395 contract). Where
      the full route makes the same unflagged claim on an oracle-inconsistent KB, the miss
      is not the cut's: info, "shared pre-check miss".
  R6  the `no_verdict` contract (#462, #475): `no_verdict` / `no_verdict_pairs` mean "a
      probe ended in NoVerdict on an internal limit": a probe with NO deadline, or (#475) one
      whose saturation stalled, budgeted or not. Then the CLI prints #449's note and exits 3.
      Forced routes run with RUSTDL_TABLEAU_SATURATE_ITERS=1 (a deterministic stall).
      R6.1 every `nv_*` / `cons` route prints its command's JSON (never a bare `unknown`).
      R6.2 field set <=> note on stderr; field set => exit 3 and `incomplete`.
      R6.3 under the forced stall a budgeted probe reports the stall too: `nv_budget` sets
           the field iff `nv_free` does on the same case (same engine and stall), and
           `nv_cons` on a budgeted probe (the wedge-route fall-through, the #344
           data-counting check) sets it iff it gave up; a decided `nv_cons` never sets it.
      R6.4 a stall cut is sound: no FP against the oracle, and never `consistent: false`
           on an oracle-consistent KB.
      R6.5 a natural stall: an unforced route sets the field (`cons`, or any budgeted
           classify route: a finding, #437's kind).
      R6.6 a decision-depth cut is never the signal: on the `dcut` route (unforced, 20 s pair
           budget; run it on fixtures/depthcut), a run whose stderr shows the search-depth
           limit has the field at 0 and no note. Unresolved when the cut was not reached.

A law compares two answers only when neither run gave up (R4: only the full run).

  routes.py run    --set NAME=DIR ... --bin BIN --tag TAG --out OUT [--first N]
  routes.py warm   --set NAME=DIR ... --python VENV_PY --tag TAG --out OUT [--first N]
  routes.py laws   --out OUT --tag TAG [--warm TAG] --oracle-cache DIR
  routes.py detect --out OUT --control TAG --mutant TAG
  routes.py --self-test
"""
import argparse
import json
import os
import random
import re
import subprocess
import sys
from collections import Counter
from concurrent import futures
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))
import oracle as orc  # noqa: E402
import run  # noqa: E402
from owl_readers import closure  # noqa: E402

CAP = int(os.environ.get("ROUTES_CAP", "30"))
FULL = ["--pair-timeout-ms", "1000"]
CUTS = ("cut_g1", "cut_g5", "cut_g25", "cut_p1")
ROUTES = {
    "full": (["classify", "--json", *FULL], {"RAYON_NUM_THREADS": "1"}),
    "t8": (["classify", "--json", *FULL], {"RAYON_NUM_THREADS": "8"}),
    "refA": (["classify", "--json", *FULL],
             {"RAYON_NUM_THREADS": "1", "RUSTDL_HYPERTABLEAU_TRUST_SAT": "0"}),
    "refB": (["classify", "--json", *FULL],
             {"RAYON_NUM_THREADS": "1", "RUSTDL_HYPERTABLEAU": "0"}),
    "cut_g1": (["classify", "--json", "--global-timeout-ms", "1"], {"RAYON_NUM_THREADS": "1"}),
    "cut_g5": (["classify", "--json", "--global-timeout-ms", "5"], {"RAYON_NUM_THREADS": "1"}),
    "cut_g25": (["classify", "--json", "--global-timeout-ms", "25"], {"RAYON_NUM_THREADS": "1"}),
    "cut_p1": (["classify", "--json", "--pair-timeout-ms", "1"], {"RAYON_NUM_THREADS": "1"}),
    "text": (["classify", *FULL], {"RAYON_NUM_THREADS": "1"}),
}
GIVE_UP_FLAGS = ("incomplete", "prep_timed_out", "conversion_timed_out", "parse_timed_out",
                 "consistency_undetermined")
FLAGS = GIVE_UP_FLAGS + ("derived_same_undecided", "completeness_guaranteed",
                         "trusted_sat_refutations", "wedge_hierarchy_blind",
                         "wedge_clauses_refused", "relies_on_trusted_consistency",
                         "no_verdict_pairs")
# R6 (#462): forced-stall routes and the consistency route. Their records also keep the exit
# code, whether #449's note was printed, and (consistent) which probe ran, from the trace.
# The forced classify routes run with the wedge off: the stall knob is read only by the main
# tableau's search, and with the wedge on, plain classify's wedge answers the generated cases
# before the main tableau runs (smoke: `nv_free` reached the stall on 0 of 12 cases with the
# wedge on, 7 with it off). `--n2-classify` reached it on the same 7 with identical counts
# either way, so the wedge setting is immaterial for `nv_free_n2`. `nv_cons` keeps the wedge
# on, so that the budgeted wedge-route fall-through stays reachable.
STALL = {"RUSTDL_TABLEAU_SATURATE_ITERS": "1"}
NO_WEDGE = {"RUSTDL_HYPERTABLEAU": "0"}
ROUTES.update({
    "nv_free": (["classify", "--json", "--pair-timeout-ms", "0"],
                {"RAYON_NUM_THREADS": "1", **STALL, **NO_WEDGE}),
    "nv_free_n2": (["classify", "--n2-classify", "--json", "--pair-timeout-ms", "0"],
                   {"RAYON_NUM_THREADS": "1", **STALL, **NO_WEDGE}),
    "nv_budget": (["classify", "--json", *FULL], {"RAYON_NUM_THREADS": "1", **STALL, **NO_WEDGE}),
    "cons": (["consistent", "--json"], {"RAYON_NUM_THREADS": "1", "RUSTDL_TRACE": "1"}),
    "nv_cons": (["consistent", "--json"], {"RAYON_NUM_THREADS": "1", "RUSTDL_TRACE": "1", **STALL}),
    # R6.6 (#475): an unforced budgeted classify that can reach the decision-depth cut. Run
    # it on fixtures/depthcut (the 280-union #288 shape); on generated cases it adds nothing.
    "dcut": (["classify", "--json", "--pair-timeout-ms", "20000"], {"RAYON_NUM_THREADS": "1"}),
})
DEPTH_LINE = "search-depth limit"
NV_CLASSIFY = ("nv_free", "nv_free_n2", "nv_budget")
CONS = ("cons", "nv_cons")
FORCED = ("nv_free", "nv_free_n2", "nv_budget", "nv_cons")
BUDGETED_CLASSIFY = ("full", "t8", "refA", "refB", *CUTS, "nv_budget")
NOTE = "likely a reasoner bug"
PURE_EL = "pure-EL"
# Tokens whose presence takes a case outside is_pure_el (plan v2 [F5]). Deliberately NOT
# here: ObjectComplementOf (neg-to-⊥), data ∃/value/domain/range/sub-properties, and bare
# Symmetric/InverseObjectProperties: the gate admits or rewrites those.
LEAVES_PURE_EL = ("ObjectAllValuesFrom", "ObjectUnionOf", "DisjointUnion",
                  "ObjectMinCardinality", "ObjectMaxCardinality", "ObjectExactCardinality",
                  "DataMinCardinality", "DataMaxCardinality", "DataExactCardinality",
                  "ObjectOneOf", "ObjectHasValue", "ObjectHasSelf", "FunctionalObjectProperty",
                  "InverseFunctionalObjectProperty", "FunctionalDataProperty",
                  "ReflexiveObjectProperty", "IrreflexiveObjectProperty",
                  "AsymmetricObjectProperty", "DisjointObjectProperties", "ClassAssertion",
                  "ObjectPropertyAssertion", "DataPropertyAssertion", "SameIndividual",
                  "DifferentIndividuals", "ObjectInverseOf")


# ------------------------------------------------------------------ answers
def answer_of_json(j, classes):
    unsat, te, clo = run.hierarchy_of(j, classes)
    return {"consistent": bool(j.get("consistent")), "unsat": sorted(unsat),
            "closure": sorted([s, t] for s, t in clo), "flags": {k: j.get(k) for k in FLAGS}}


def answer_of_text(out, classes):
    """The text output, normalised as JSON is: unsat classes are dropped from `equiv`."""
    edges, groups, unsat, fragment = [], [], set(), None
    for line in out.splitlines():
        if line.startswith("# fragment:"):
            fragment = line.split(":", 1)[1].strip()
        elif line.startswith("#"):
            continue
        parts = line.split("\t")
        if parts[0] == "direct" and len(parts) == 3:
            edges.append((parts[1], parts[2]))
        elif parts[0] == "equiv" and len(parts) >= 3:
            groups.append(parts[1:])
        elif parts[0] == "unsat" and len(parts) == 2:
            unsat.add(parts[1])
    groups = [[m for m in g if m not in unsat] for g in groups]
    groups = [g for g in groups if len(g) >= 2]
    clo = {(s, t) for s, t in closure(edges, groups) if s in classes and t in classes}
    return {"unsat": sorted(unsat & classes), "closure": sorted([s, t] for s, t in clo),
            "fragment": fragment}


def gave_up(rec):
    if rec.get("status") != "ok":
        return True
    f = rec["answer"]["flags"]
    return any(f.get(k) for k in GIVE_UP_FLAGS) or bool(f.get("derived_same_undecided"))


def key(ans):
    return (ans["consistent"], tuple(ans["unsat"]), tuple(map(tuple, ans["closure"])))


def consistency_probe(stderr):
    """Which probe decided `consistent`, from its RUSTDL_TRACE lines (lib.rs)."""
    if "bounded tableau fall-through" in stderr or "data counting verified" in stderr:
        return "budgeted"
    if any(m in stderr for m in ("consistency: wedge Unsat", "consistency: wedge Sat",
                                 "consistency: saturation proves",
                                 "consistency: EL saturator decides",
                                 "consistency pre-check: inconsistent",
                                 "abox_check: inconsistent")):
        return "decided"
    return "free"


# ------------------------------------------------------------------ run (CLI routes)
def run_route(binary, route, path, classes):
    args, env = ROUTES[route]
    try:
        p = subprocess.run([binary, *args, str(path)], capture_output=True, text=True,
                           timeout=CAP, env={**os.environ, **env}, preexec_fn=run._limit)
    except subprocess.TimeoutExpired:
        return {"status": "timeout"}
    if route == "text":
        if p.returncode != 0:
            return {"status": "fail", "rc": p.returncode}
        return {"status": "ok", "answer": answer_of_text(p.stdout, classes)}
    if route in NV_CLASSIFY or route in CONS or route == "dcut":
        io = {"rc": p.returncode, "note": NOTE in p.stderr}
        if route == "dcut":
            io["depth_line"] = DEPTH_LINE in p.stderr
        try:
            j = json.loads(p.stdout)
            if route in CONS:
                ans = {"consistent": j["consistent"], "incomplete": j["incomplete"],
                       "no_verdict": j.get("no_verdict"), "probe": consistency_probe(p.stderr)}
            else:
                ans = answer_of_json(j, classes)
        except (json.JSONDecodeError, KeyError, TypeError):
            return {"status": "fail", **io, "stdout_head": p.stdout[:80],
                    "stderr": p.stderr[-300:]}
        return {"status": "ok", "answer": ans, **io}
    try:
        return {"status": "ok", "answer": answer_of_json(json.loads(p.stdout), classes)}
    except (json.JSONDecodeError, KeyError):
        return {"status": "fail", "rc": p.returncode, "stderr": p.stderr[-300:]}


def one_case(setname, path, binary, out_dir, routes):
    dest = Path(out_dir) / "cases" / setname / (Path(path).stem + ".json")
    if dest.exists():
        return json.loads(dest.read_text())
    text = Path(path).read_text()
    classes = orc.declared_classes(text)
    rec = {"set": setname, "case": Path(path).stem, "sha256": orc.sha(text), "routes": {}}
    for r in routes:
        rec["routes"][r] = run_route(binary, r, path, classes)
    dest.parent.mkdir(parents=True, exist_ok=True)
    dest.with_suffix(".part").write_text(json.dumps(rec))
    dest.with_suffix(".part").replace(dest)
    return rec


def case_files(sets, first):
    out = []
    for spec in sets:
        name, _, d = spec.partition("=")
        files = sorted(Path(d).glob("case-*.ofn"))
        out += [(name, f) for f in (files[:first] if first else files)]
    return out


def cmd_run(a):
    out = Path(a.out) / a.tag
    out.mkdir(parents=True, exist_ok=True)
    files = case_files(a.set, a.first)
    routes = a.routes.split(",") if a.routes else list(ROUTES)
    (out / "manifest.json").write_text(json.dumps({"tag": a.tag, "binary": a.bin,
                                                   "routes": routes, "n": len(files),
                                                   "cap_s": CAP, "sets": a.set}, indent=1))
    cores = [int(c) for c in a.cores.split(",")]
    import multiprocessing as mp
    q = mp.Manager().Queue()
    for c in cores:
        q.put(c)
    done = 0
    with futures.ProcessPoolExecutor(max_workers=len(cores), initializer=run._pin,
                                     initargs=(q,)) as ex:
        futs = {ex.submit(one_case, s, str(f), a.bin, str(out), routes): (s, f)
                for s, f in files}
        for fu in futures.as_completed(futs):
            done += 1
            try:
                fu.result()
                if done % 100 == 0 or done == len(files):
                    print(f"[{done}/{len(files)}]", flush=True)
            except Exception as exc:  # noqa: BLE001
                print(f"[{done}/{len(files)}] {futs[fu]} ERROR {exc!r}", flush=True)
    return 0


# ------------------------------------------------------------------ warm (Python)
WARM_DRIVER = r'''
import json, random, sys, warnings
warnings.simplefilter("ignore")
import rustdl
files, out, seed = json.load(open(sys.argv[1])), sys.argv[2], int(sys.argv[3])
order = list(range(len(files)))
if seed:
    random.Random(seed).shuffle(order)
with open(out, "w") as fh:
    for i in order:
        setname, path, classes = files[i]
        rec = {"set": setname, "case": path.rsplit("/", 1)[-1][:-4]}
        try:
            # 25 s global budget: a long case gives up (budget-cut) as the CLI's 30 s cap does.
            c = rustdl.classify(path, per_pair_timeout_ms=1000, global_timeout_ms=25000)
            unsat = set(c.unsatisfiable) & set(classes)
            edges = [(x, y) for x in classes if x not in unsat for y in c.direct_subsumers(x)]
            groups = [[x] + list(c.equivalent_classes(x)) for x in classes if x not in unsat]
            rec.update(status="ok", consistent=not c.inconsistent, unsat=sorted(unsat),
                       edges=edges, groups=groups,
                       complete=c.complete, derived_same_undecided=c.derived_same_undecided,
                       consistency_undetermined=c.consistency_undetermined,
                       prep_timed_out=c.prep_timed_out,
                       conversion_timed_out=c.conversion_timed_out)
            # R3.3: is_subclass over every declared satisfiable pair
            rec["is_subclass"] = [[x, y] for x in classes for y in classes
                                  if x != y and x not in unsat and y not in unsat
                                  and c.is_subclass(x, y)]
            # interleaved queries: the realistic thread-local leak path. is_consistent and
            # is_subclass_of exercise the per-thread query-consistency cells #428 (#442)
            # added. is_subclass_of used to run only when is_consistent said True: on
            # default case 438 (TBox-only, inconsistent) its H4 wedge had no deadline and
            # did not finish (> 150 s on 0df06f64). #457 bounds that wedge
            # (RUSTDL_QUERY_WEDGE_MS), so it runs ungated again. The call is still
            # unbounded where the MAIN tableau does not finish, and the Python API has no
            # timeout: on the generated sets this driver's pair hangs on default case 1911
            # (#460; consistent, so the old gate did not skip it either). Checked on the
            # #457 branch via the CLI: the 2,624 other cases with a pair answer within 60 s.
            try:
                rustdl.is_consistent(path)
            except Exception:  # noqa: BLE001 (a give-up is an error here; the call still ran)
                pass
            pair = [x for x in classes if x not in unsat][:2]
            if len(pair) == 2:
                try:
                    rustdl.is_subclass_of(path, pair[0], pair[1])
                except Exception:  # noqa: BLE001 (the answer is not compared; the call ran)
                    pass
        except Exception as e:  # noqa: BLE001
            rec.update(status="fail", error=repr(e)[:300])
        fh.write(json.dumps(rec) + "\n")
        fh.flush()
        print(f"{order.index(i) + 1}/{len(order)} {rec['set']} {rec['case']} {rec['status']}",
              file=sys.stderr, flush=True)
'''


def cmd_warm(a):
    out = Path(a.out) / a.tag
    out.mkdir(parents=True, exist_ok=True)
    files = [(s, str(f), sorted(orc.declared_classes(f.read_text())))
             for s, f in case_files(a.set, a.first)]
    drv = out / "warm_driver.py"
    drv.write_text(WARM_DRIVER)
    lst = out / "warm_files.json"  # passed as a file: 4000 paths exceed ARG_MAX
    lst.write_text(json.dumps(files))
    for seed in (0, 363):
        dest = out / f"warm-{seed}.jsonl"
        subprocess.run(["taskset", "-c", a.core, a.python, str(drv), str(lst), str(dest),
                        str(seed)], check=True)
        print(f"warm pass seed={seed} -> {dest}", flush=True)
    return 0


def warm_answer(r, classes):
    clo = {(s, t) for s, t in closure([tuple(e) for e in r["edges"]],
                                      [g for g in r["groups"] if len(g) >= 2])
           if s in classes and t in classes}
    gave = (not r["complete"]) or r["consistency_undetermined"] or r["prep_timed_out"] \
        or r["conversion_timed_out"] or r["derived_same_undecided"]
    return {"consistent": r["consistent"], "unsat": sorted(r["unsat"]),
            "closure": sorted([s, t] for s, t in clo)}, bool(gave), clo


# ------------------------------------------------------------------ laws
def oracle_answer(cache, sha):
    p = Path(cache) / sha[:2] / f"{sha}.json"
    if not p.exists():
        return None
    o = json.loads(p.read_text())
    return o["answer"] if o["verdict"] == "agree" else None


def fp_findings(oans, classes, ans):
    """FP kinds of one route's answer against the oracle (run.score on a fake arm)."""
    j = {"consistent": ans["consistent"], "unsatisfiable": ans["unsat"],
         "equivalent_groups": [], "direct_subsumptions": ans["closure"],
         "completeness_guaranteed": False, "incomplete": False}
    arms = {"classify": {"status": "ok", "json": j}}
    return [f for f in run.score(oans, classes, arms) if f["kind"].endswith("-FP")]


def r44(cr, full, oans):
    """R4.4 (#466) for one cut route: (outcome, detail), or None if the cut makes no
    determined-consistency claim."""
    if cr.get("status") != "ok":
        return None
    ca = cr["answer"]
    if not ca["consistent"] or ca["flags"].get("consistency_undetermined"):
        return None
    full_ok = full.get("status") == "ok"
    if full_ok and full["answer"]["consistent"] is False:
        if oans is not None and oans["consistent"]:
            return ("info", "full says inconsistent, the oracle consistent")
        return ("violation", "cut claims consistent; full says inconsistent")
    full_undetermined = (not full_ok) or bool(full["answer"]["flags"].get("consistency_undetermined"))
    if oans is not None and not oans["consistent"]:
        if full_undetermined:
            return ("violation", "cut claims consistent; oracle inconsistent, full undetermined")
        return ("info", "shared pre-check miss")
    if full_undetermined and oans is None:
        return ("info", "no reference: full undetermined and no oracle")
    return ("ok", "")


def evaluate(rec, oans, classes, text_of_case, warm=None):
    """[(law, outcome, detail)] for one case; outcome in ok/violation/budget-cut/info."""
    out = []
    R = rec["routes"]
    full = R.get("full", {})
    full_gave = gave_up(full)

    def add(law, outcome, detail=""):
        out.append((law, outcome, detail))

    # R1 threads
    t8 = R.get("t8")
    if t8 is not None:
        if full_gave or gave_up(t8):
            add("R1", "budget-cut")
        else:
            add("R1", "ok" if key(full["answer"]) == key(t8["answer"]) else "violation")
    # R2 references
    for ref in ("refA", "refB"):
        rr = R.get(ref)
        if rr is None:
            continue
        # R2.3, the FP check, is never budget-exempt: it runs on every completed run, as a
        # cut-publishing bug is exactly what makes a run `incomplete`.
        if oans is not None and oans["consistent"]:
            for name, run_ in (("default", full), (ref, rr)):
                if run_.get("status") == "ok":
                    for f in fp_findings(oans, classes, run_["answer"]):
                        add(f"R2.3.{ref}", "violation", f"{name} {f['kind']} {f['witness']}")
        if full_gave or gave_up(rr):
            add(f"R2.{ref}", "budget-cut")
            continue
        fa, ra = full["answer"], rr["answer"]
        if fa["consistent"] != ra["consistent"]:
            add(f"R2.4.{ref}", "violation", "consistent differs")
        if key(fa) == key(ra):
            add(f"R2.{ref}", "ok")
        else:
            if fa["flags"].get("completeness_guaranteed"):
                add(f"R2.2.{ref}", "violation", "guaranteed default differs")
            add(f"R2.{ref}", "info", "asymmetric (oracle adjudicates)" if oans
                else "asymmetric, no oracle")
    # R4 interruption
    for cut in CUTS:
        cr = R.get(cut)
        if cr is None:
            continue
        # R4.4 runs before the full-gave-up skip: an undetermined full run is where the
        # oracle arm applies.
        r44_out = r44(cr, full, oans)
        if r44_out is not None:
            add(f"R4.4.{cut}", *r44_out)
        if full_gave or cr.get("status") != "ok":
            add(f"R4.{cut}", "budget-cut")
            continue
        fa, ca = full["answer"], cr["answer"]
        # A pair whose subclass the full run calls unsatisfiable is implied by it (an unsat
        # class is below everything), so it is not "more" than the full answer.
        extra = {(s, t) for s, t in map(tuple, ca["closure"])
                 if (s, t) not in set(map(tuple, fa["closure"])) and s not in fa["unsat"]}
        extra_u = set(ca["unsat"]) - set(fa["unsat"])
        if extra or extra_u:
            fps = fp_findings(oans, classes, ca) if (oans and oans["consistent"]) else []
            if fps:
                add(f"R4.1.{cut}", "violation", f"FP {fps[0]['kind']} {fps[0]['witness']}")
            else:
                add(f"R4.1.{cut}", "info", "cut ⊄ full, not refuted by the oracle")
        flagged = any(ca["flags"].get(k) for k in GIVE_UP_FLAGS) \
            or bool(ca["flags"].get("derived_same_undecided"))
        broke = False
        if key(ca) != key(fa) and not flagged:
            add(f"R4.2.{cut}", "violation", "silent cut")
            broke = True
        if not ca["consistent"] and fa["consistent"]:
            add(f"R4.3.{cut}", "violation", "cut says inconsistent")
            broke = True
        if any(l in (f"R4.1.{cut}", f"R4.4.{cut}") and o == "violation" for l, o, _ in out):
            broke = True
        if not broke:
            add(f"R4.{cut}", "ok")
    # R6 the no_verdict contract (#462, #475)
    for name in BUDGETED_CLASSIFY:
        rr = R.get(name)
        if rr is not None and rr.get("status") == "ok" and name != "nv_budget":
            if rr["answer"]["flags"].get("no_verdict_pairs"):
                add("R6.5", "violation", f"{name}: a natural stall on a budgeted route")
    dc = R.get("dcut")
    if dc is not None and dc.get("status") == "ok":
        nvp = dc["answer"]["flags"].get("no_verdict_pairs")
        if not dc.get("depth_line"):
            add("R6.6", "unresolved", "the depth cut was not reached")
        elif nvp or dc.get("note"):
            add("R6.6", "violation", f"dcut: a depth cut reported as the signal "
                f"(no_verdict_pairs {nvp}, note {dc.get('note')})")
        else:
            add("R6.6", "ok")
    for name in (*NV_CLASSIFY, *CONS):
        rr = R.get(name)
        if rr is None:
            continue
        if rr.get("status") == "timeout":
            add("R6.1", "timeout", name)
            continue
        if rr.get("status") != "ok":
            add("R6.1", "violation",
                f"{name}: not JSON (rc {rr.get('rc')}): {rr.get('stdout_head', '')!r}")
            continue
        add("R6.1", "ok")
        a = rr["answer"]
        if name in CONS:
            field, incomplete = bool(a["no_verdict"]), bool(a["incomplete"])
            budgeted = a["probe"] != "free"
        else:
            field = bool(a["flags"].get("no_verdict_pairs"))
            incomplete = bool(a["flags"].get("incomplete"))
            budgeted = name == "nv_budget"
        if field != rr["note"]:
            add("R6.2", "violation", f"{name}: field {field}, note {rr['note']}")
        elif field and (rr["rc"] != 3 or not incomplete):
            add("R6.2", "violation", f"{name}: field set, rc {rr['rc']}, incomplete {incomplete}")
        else:
            add("R6.2", "ok")
        if name == "nv_budget":
            nf = R.get("nv_free")
            if nf is not None and nf.get("status") == "ok":
                want = bool(nf["answer"]["flags"].get("no_verdict_pairs"))
                add("R6.3", "ok" if field == want else "violation",
                    "" if field == want else f"nv_budget field {field}, nv_free field {want}")
        elif name == "nv_cons" and budgeted:
            if a["probe"] == "budgeted":
                add("R6.3", "ok" if field == incomplete else "violation",
                    "" if field == incomplete else
                    f"nv_cons budgeted probe: field {field}, incomplete {incomplete}")
            else:
                add("R6.3", "violation" if field else "ok",
                    "nv_cons: field set with no main-tableau give-up (decided)" if field else "")
        if name == "cons":
            add("R6.5", "violation" if field else "ok",
                "a natural NoVerdict on a deadline-free probe" if field else "")
        if name in FORCED and oans is not None and oans["consistent"]:
            if not a["consistent"]:
                add("R6.4", "violation", f"{name}: inconsistent under the stall")
            elif name in NV_CLASSIFY:
                fps = fp_findings(oans, classes, a)
                add("R6.4", "violation" if fps else "ok",
                    f"{name}: FP {fps[0]['kind']} {fps[0]['witness']}" if fps else "")
            else:
                add("R6.4", "ok")
    # R5 fragment and serialisation
    tx = R.get("text")
    if tx is not None and tx.get("status") == "ok":
        frag = tx["answer"]["fragment"] or ""
        # A degenerate `ObjectUnionOf(:C :C)` is just `C`, which the gate admits.
        scan = re.sub(r"ObjectUnionOf\((:C\d+) \1\)", r"\1", text_of_case)
        leaves = [t for t in LEAVES_PURE_EL if t in scan]
        if leaves and frag.startswith(PURE_EL):
            add("R5.1", "violation", f"pure-EL despite {leaves[:3]}")
        elif leaves:
            add("R5.1", "ok")
        if rec["set"] == "el_guaranteed":
            add("R5.2", "ok" if frag.startswith(PURE_EL) else "info", frag[:40])
        if not full_gave:
            same = (tx["answer"]["unsat"] == full["answer"]["unsat"]
                    and tx["answer"]["closure"] == full["answer"]["closure"])
            add("R5.3", "ok" if same else "violation")
    # R3 warm
    if warm is not None:
        w0, w1 = warm
        if w0 is None or w0.get("status") != "ok":
            add("R3.1", "budget-cut" if w0 else "info", "no warm record")
        else:
            wa, wgave, wclo = warm_answer(w0, classes)
            if full_gave or wgave:
                add("R3.1", "budget-cut")
            else:
                add("R3.1", "ok" if key(wa) == key(full["answer"]) else "violation")
            iss = {tuple(p) for p in w0["is_subclass"]}
            sat = classes - set(w0["unsat"])
            want = {(s, t) for s, t in wclo if s in sat and t in sat}
            add("R3.3", "ok" if iss == want else "violation",
                f"{len(iss ^ want)} pairs differ" if iss != want else "")
            if w1 is not None and w1.get("status") == "ok":
                wb, wbg, _ = warm_answer(w1, classes)
                if wgave or wbg:
                    add("R3.2", "budget-cut")
                else:
                    add("R3.2", "ok" if key(wa) == key(wb) else "violation")
    return out


def load_tag(out, tag):
    recs = {}
    for p in (Path(out) / tag / "cases").glob("*/case-*.json"):
        r = json.loads(p.read_text())
        recs[(r["set"], r["case"])] = r
    return recs


def load_warm(out, tag):
    res = {}
    for seed in (0, 363):
        p = Path(out) / tag / f"warm-{seed}.jsonl"
        if p.exists():
            for line in p.read_text().splitlines():
                r = json.loads(line)
                res.setdefault((r["set"], r["case"]), [None, None])[0 if seed == 0 else 1] = r
    return res


def case_text(rec, sets):
    d = dict(s.split("=", 1) for s in sets)
    return (Path(d[rec["set"]]) / f"{rec['case']}.ofn").read_text()


def all_outcomes(a):
    man = json.loads((Path(a.out) / a.tag / "manifest.json").read_text())
    recs = load_tag(a.out, a.tag)
    warm = load_warm(a.out, a.warm) if a.warm else {}
    res = {}
    for k, rec in recs.items():
        text = case_text(rec, man["sets"])
        classes = orc.declared_classes(text)
        oans = oracle_answer(a.oracle_cache, rec["sha256"])
        res[k] = evaluate(rec, oans, classes, text, warm.get(k) if a.warm else None)
    return res


def cmd_laws(a):
    res = all_outcomes(a)
    tally = {}
    viol = []
    for k, outs in sorted(res.items()):
        for law, outcome, detail in outs:
            tally.setdefault(law, Counter())[outcome] += 1
            if outcome == "violation":
                viol.append([k[0], k[1], law, detail])
    rep = {"tag": a.tag, "warm": a.warm, "cases": len(res),
           "laws": {l: dict(c) for l, c in sorted(tally.items())},
           "violations": len(viol), "violation_list": viol[:200]}
    Path(a.out, a.tag, "laws.json").write_text(json.dumps(rep, indent=1))
    print(json.dumps({k: v for k, v in rep.items() if k != "violation_list"}, indent=1))
    for v in viol[:40]:
        print("VIOLATION", *v)
    return 0


def cmd_detect(a):
    """Law violations in the mutant run that the control run lacks on the same case."""
    a_c = argparse.Namespace(**{**vars(a), "tag": a.control, "warm": a.control_warm})
    a_m = argparse.Namespace(**{**vars(a), "tag": a.mutant, "warm": a.mutant_warm})
    c, m = all_outcomes(a_c), all_outcomes(a_m)
    new = []
    for k, outs in m.items():
        base = {(l, d) for l, o, d in c.get(k, []) if o == "violation"}
        for l, o, d in outs:
            if o == "violation" and (l, d) not in base:
                new.append([k[0], k[1], l, d])
    by_law = Counter(v[2] for v in new)
    rep = {"control": a.control, "mutant": a.mutant, "detecting_cases": len({(v[0], v[1]) for v in new}),
           "by_law": dict(by_law), "first": new[:20]}
    Path(a.out, a.mutant, f"detect-vs-{a.control}.json").write_text(json.dumps(rep, indent=1))
    print(json.dumps(rep, indent=1))
    return 0


# ------------------------------------------------------------------ self-test
def self_test():
    A, B, C = "http://g#A", "http://g#B", "http://g#C"
    classes = {A, B, C}
    flags0 = {k: False for k in FLAGS}
    flags0["derived_same_undecided"] = 0

    def r(clo, unsat=(), cons=True, **fl):
        return {"status": "ok", "answer": {"consistent": cons, "unsat": sorted(unsat),
                                           "closure": sorted(map(list, clo)),
                                           "flags": {**flags0, **fl}}}

    def txt(clo, unsat=(), frag="out-of-EL"):
        return {"status": "ok", "answer": {"unsat": sorted(unsat),
                                           "closure": sorted(map(list, clo)), "fragment": frag}}

    base = {(A, B)}
    oans = {"consistent": True, "unsat": [], "thing_equiv": [], "pairs": [[A, B]]}

    def laws(routes, setname="default", text="", warm=None):
        rec = {"set": setname, "case": "c", "routes": routes}
        return {(l, o) for l, o, _ in evaluate(rec, oans, classes, text, warm)}

    fails = 0

    def ok(name, got, want):
        nonlocal fails
        good = want <= got if isinstance(want, set) else got == want
        fails += not good
        print(f"{'ok  ' if good else 'FAIL'} {name}: got {sorted(got) if isinstance(got, set) else got}")

    full = r(base)
    ok("R1 equal", laws({"full": full, "t8": r(base)}), {("R1", "ok")})
    ok("R1 differs", laws({"full": full, "t8": r(set())}), {("R1", "violation")})
    ok("R1 give-up is budget-cut", laws({"full": full, "t8": r(set(), incomplete=True)}),
       {("R1", "budget-cut")})
    ok("R1 derived_same_undecided is a give-up",
       laws({"full": full, "t8": r(set(), derived_same_undecided=2)}), {("R1", "budget-cut")})
    ok("R2 reference FP", laws({"full": full, "refA": r(base | {(B, C)})}),
       {("R2.3.refA", "violation")})
    ok("R2 reference miss is info, not a violation", laws({"full": full, "refB": r(set())}),
       {("R2.refB", "info")})
    ok("R2.4 consistency differs", laws({"full": full, "refA": r(base, cons=False)}),
       {("R2.4.refA", "violation")})
    ok("R2.2 guaranteed default differs",
       laws({"full": r(base, completeness_guaranteed=True), "refA": r(set())}),
       {("R2.2.refA", "violation")})
    ok("R4.1 FP under a cut, flags ignored",
       laws({"full": full, "cut_p1": r(base | {(B, C)}, incomplete=True)}),
       {("R4.1.cut_p1", "violation")})
    ok("R4 is not tallied ok when R4.1 fired",
       ("R4.cut_p1", "ok") in laws({"full": full, "cut_p1": r(base | {(B, C)}, incomplete=True)}),
       False)
    ok("R2.3 FP check runs on a given-up reference run",
       laws({"full": full, "refA": r(base | {(B, C)}, incomplete=True)}),
       {("R2.3.refA", "violation"), ("R2.refA", "budget-cut")})
    ok("R4.2 silent cut", laws({"full": full, "cut_g1": r(set())}), {("R4.2.cut_g1", "violation")})
    ok("R4.2 flagged cut is fine", laws({"full": full, "cut_g1": r(set(), incomplete=True)}),
       {("R4.cut_g1", "ok")})
    ok("R4.3 cut inconsistent", laws({"full": full, "cut_g5": r(set(), cons=False, incomplete=True)}),
       {("R4.3.cut_g5", "violation")})
    ok("R4.1 a cut missing an unsat class is not 'more'",
       laws({"full": r(set(), unsat={C}), "cut_g1": r({(C, A)}, incomplete=True)}),
       {("R4.cut_g1", "ok")})
    ok("R4 skipped when the full run gave up",
       laws({"full": r(base, incomplete=True), "cut_g5": r(set())}), {("R4.cut_g5", "budget-cut")})
    ok("R5.1 pure-EL despite ∀", laws({"full": full, "text": txt(base, frag="pure-EL (x)")},
                                      text="ObjectAllValuesFrom(:r :A)"), {("R5.1", "violation")})
    ok("R5.1 complement alone is not a predictor",
       laws({"full": full, "text": txt(base, frag="pure-EL (x)")}, text="ObjectComplementOf(:A)"),
       {("R5.3", "ok")})
    ok("R5.1 a degenerate union is not a predictor",
       {x for x in laws({"full": full, "text": txt(base, frag="pure-EL (x)")},
                        text="SubClassOf(:A ObjectUnionOf(:C1 :C1))") if x[0] == "R5.1"}, set())
    ok("R5.2 el_guaranteed not pure-EL is info",
       laws({"full": full, "text": txt(base, frag="Horn")}, setname="el_guaranteed"),
       {("R5.2", "info")})
    ok("R5.3 text differs", laws({"full": full, "text": txt(set())}), {("R5.3", "violation")})
    w = {"status": "ok", "consistent": True, "unsat": [], "edges": [[A, B]], "groups": [],
         "complete": True, "derived_same_undecided": 0, "consistency_undetermined": False,
         "prep_timed_out": False, "conversion_timed_out": False, "is_subclass": [[A, B]]}
    ok("R3 warm equal", laws({"full": full}, warm=(w, w)),
       {("R3.1", "ok"), ("R3.2", "ok"), ("R3.3", "ok")})
    w2 = {**w, "edges": []}
    ok("R3.1 warm differs", laws({"full": full}, warm=(w2, w)), {("R3.1", "violation")})
    ok("R3.2 order matters", laws({"full": full}, warm=(w, w2)), {("R3.2", "violation")})
    ok("R3.3 is_subclass disagrees", laws({"full": full}, warm=({**w, "is_subclass": []}, w)),
       {("R3.3", "violation")})
    # R4.4 (#466)
    inc = {**oans, "consistent": False}

    def laws_o(routes, o):
        rec = {"set": "default", "case": "c", "routes": routes}
        return {(l, x) for l, x, _ in evaluate(rec, o, classes, "", None)}
    ok("R4.4 cut claims consistent, full inconsistent (no oracle)",
       laws_o({"full": r(set(), cons=False), "cut_g1": r(set(), incomplete=True)}, None),
       {("R4.4.cut_g1", "violation")})
    ok("R4.4 is not tallied R4 ok", ("R4.cut_g1", "ok") in
       laws_o({"full": r(set(), cons=False), "cut_g1": r(set(), incomplete=True)}, None), False)
    ok("R4.4 full inconsistent but the oracle consistent is info (full's FP is R2's)",
       laws({"full": r(set(), cons=False), "cut_g1": r(set(), incomplete=True)}),
       {("R4.4.cut_g1", "info")})
    ok("R4.4 flagged cut is fine",
       {x for x in laws({"full": r(set(), cons=False),
                         "cut_g1": r(set(), incomplete=True, consistency_undetermined=True)})
        if x[0].startswith("R4.4")}, set())
    ok("R4.4 full undetermined, oracle inconsistent",
       laws_o({"full": r(set(), consistency_undetermined=True), "cut_g5": r(set(), incomplete=True)},
              inc), {("R4.4.cut_g5", "violation")})
    ok("R4.4 shared pre-check miss is info",
       laws_o({"full": r(set()), "cut_g5": r(set(), incomplete=True)}, inc),
       {("R4.4.cut_g5", "info")})
    ok("R4.4 full undetermined, no oracle: info, not ok",
       laws_o({"full": r(set(), consistency_undetermined=True), "cut_g5": r(set(), incomplete=True)},
              None), {("R4.4.cut_g5", "info")})
    ok("R4.4 agreeing claim is ok", laws({"full": full, "cut_g25": r(base)}), {("R4.4.cut_g25", "ok")})
    # R6 (#462)

    def nv(field, note, rc=3, incomplete=True, cons=True, **fl):
        return {"status": "ok", "rc": rc, "note": note,
                "answer": {"consistent": cons, "unsat": [], "closure": [[A, B]],
                           "flags": {**flags0, "incomplete": incomplete,
                                     "no_verdict_pairs": 2 if field else 0, **fl}}}

    def cs(field, note, probe, rc=3, incomplete=True, cons=True):
        return {"status": "ok", "rc": rc, "note": note,
                "answer": {"consistent": cons, "incomplete": incomplete, "no_verdict": field,
                           "probe": probe}}
    bare = {"status": "fail", "rc": 3, "note": True, "stdout_head": "unknown\n"}
    ok("R6.1 bare unknown", laws({"full": full, "nv_free": bare}), {("R6.1", "violation")})
    ok("R6.1 timeout is not a violation", laws({"full": full, "nv_free": {"status": "timeout"}}),
       {("R6.1", "timeout")})
    ok("R6 deadline-free stall reported", laws({"full": full, "nv_free": nv(True, True)}),
       {("R6.1", "ok"), ("R6.2", "ok")})
    ok("R6.2 field without the note", laws({"full": full, "nv_free": nv(True, False)}),
       {("R6.2", "violation")})
    ok("R6.2 note without the field", laws({"full": full, "nv_free_n2": nv(False, True)}),
       {("R6.2", "violation")})
    ok("R6.2 field with exit 0", laws({"full": full, "nv_free": nv(True, True, rc=0)}),
       {("R6.2", "violation")})
    ok("R6.3 nv_budget reports the stall nv_free reports",
       {x for x in laws({"full": full, "nv_free": nv(True, True), "nv_budget": nv(True, True)})
        if x[0] == "R6.3"}, {("R6.3", "ok")})
    ok("R6.3 nv_budget hides the stall nv_free reports (pre-#475)",
       {x for x in laws({"full": full, "nv_free": nv(True, True),
                         "nv_budget": nv(False, False, rc=0)}) if x[0] == "R6.3"},
       {("R6.3", "violation")})
    ok("R6.3 nv_budget reports a stall nv_free does not",
       {x for x in laws({"full": full, "nv_free": nv(False, False, rc=0),
                         "nv_budget": nv(True, True)}) if x[0] == "R6.3"},
       {("R6.3", "violation")})
    ok("R6.5 field on an unforced budgeted route is a natural stall",
       {x for x in laws({"full": r(base, no_verdict_pairs=1)}) if x[0].startswith("R6")},
       {("R6.5", "violation")})
    ok("R6.3 consistent fall-through gave up and reports the stall",
       {x for x in laws({"full": full, "nv_cons": cs(True, True, "budgeted")}) if x[0] == "R6.3"},
       {("R6.3", "ok")})
    ok("R6.3 consistent fall-through gave up silently (pre-#475)",
       {x for x in laws({"full": full, "nv_cons": cs(False, False, "budgeted")})
        if x[0] == "R6.3"}, {("R6.3", "violation")})
    ok("R6.3 a decided consistent never sets the field",
       {x for x in laws({"full": full, "nv_cons": cs(True, True, "decided")}) if x[0] == "R6.3"},
       {("R6.3", "violation")})
    def dcut(nvp, note, line):
        return {**r(base, incomplete=True, no_verdict_pairs=nvp), "rc": 0, "note": note,
                "depth_line": line}
    ok("R6.6 a depth cut stays a cut", {x for x in laws({"full": full, "dcut": dcut(0, False, True)})
                                        if x[0] == "R6.6"}, {("R6.6", "ok")})
    ok("R6.6 a depth cut reported as the signal",
       {x for x in laws({"full": full, "dcut": dcut(2, True, True)}) if x[0] == "R6.6"},
       {("R6.6", "violation")})
    ok("R6.6 the cut not reached", {x for x in laws({"full": full, "dcut": dcut(0, False, False)})
                                    if x[0] == "R6.6"}, {("R6.6", "unresolved")})
    ok("R6.3 deadline-free consistent may set it",
       {x for x in laws({"full": full, "nv_cons": cs(True, True, "free")}) if x[0] == "R6.3"},
       set())
    ok("R6.5 natural stall", laws({"full": full, "cons": cs(True, True, "free")}),
       {("R6.5", "violation")})
    ok("R6.4 stall FP", laws({"full": full, "nv_free": {**nv(True, True), "answer": {
        **nv(True, True)["answer"], "closure": [[A, B], [B, C]]}}}), {("R6.4", "violation")})
    ok("R6.4 stall says inconsistent", laws({"full": full, "nv_cons": cs(True, True, "free", cons=False)}),
       {("R6.4", "violation")})
    ok("consistency_probe reads the trace",
       [consistency_probe(x) for x in ("consistency: wedge Stalled — bounded tableau fall-through",
                                       "consistency: wedge Sat — consistent (trusted)",
                                       "consistency pre-check: inconsistent (abox_saturation or empty clause)",
                                       "abox_check: inconsistent — Clash", "")],
       ["budgeted", "decided", "decided", "decided", "free"])
    ok("text parser drops unsat from equiv",
       answer_of_text("unsat\thttp://g#C\nequiv\thttp://g#A\thttp://g#C\ndirect\thttp://g#A\thttp://g#B\n",
                      classes)["closure"], [[A, B]])
    print(f"routes self-test: {'all ok' if not fails else f'{fails} FAILED'}")
    return 1 if fails else 0


def main():
    if "--self-test" in sys.argv:
        return self_test()
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("run")
    p.add_argument("--set", action="append", required=True)
    p.add_argument("--bin", required=True)
    p.add_argument("--tag", required=True)
    p.add_argument("--out", required=True)
    p.add_argument("--first", type=int)
    p.add_argument("--routes")
    p.add_argument("--cores", default="92,93,94")
    p = sub.add_parser("warm")
    p.add_argument("--set", action="append", required=True)
    p.add_argument("--python", required=True)
    p.add_argument("--tag", required=True)
    p.add_argument("--out", required=True)
    p.add_argument("--first", type=int)
    p.add_argument("--core", default="94")
    p = sub.add_parser("laws")
    p.add_argument("--out", required=True)
    p.add_argument("--tag", required=True)
    p.add_argument("--warm")
    p.add_argument("--oracle-cache", required=True)
    p = sub.add_parser("detect")
    p.add_argument("--out", required=True)
    p.add_argument("--control", required=True)
    p.add_argument("--mutant", required=True)
    p.add_argument("--control-warm")
    p.add_argument("--mutant-warm")
    p.add_argument("--oracle-cache", required=True)
    a = ap.parse_args()
    return {"run": cmd_run, "warm": cmd_warm, "laws": cmd_laws, "detect": cmd_detect}[a.cmd](a)


if __name__ == "__main__":
    sys.exit(main())
