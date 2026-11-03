"""Export a frozen sample of a generated-net run as an in-repo test fixture.

The `el_guaranteed` net (#363 step 3a) runs only off-CI: it needs Konclude and HermiT.
This freezes a sample of one run, the case files plus the oracle answers both peers
agreed on, so `crates/owl-dl-reasoner/tests/el_generated_guard.rs` can check in CI that
every case the pure-EL gate certifies is classified exactly as the oracle says. That is
the guard #435/#436 lacked.

Selection: every case the run bucketed as a `finding`, then a stratified sample of the
other `agree` cases (round-robin over `# motif:` groups, each shuffled with --seed)
until --n cases. Peers are not run: the answers come from the oracle cache the run used.

Usage (python3 -I):
    python3 -I scripts/gen/export_fixture.py --run-out ~/rustdl/work-363-gen/out/el-control \\
        --cases ~/rustdl/work-363-gen/cases-el_guaranteed --oracle-cache ~/rustdl/work-363-gen/ocache \\
        --certified-from ~/rustdl/work-435/gen/out4/fix435b \\
        --certified-build "rustdl 5f325194, #439's head, merged as 6ae07d54" \\
        --source-build "rustdl a5a2e063, before #435/#436" \\
        --oracle-peers "Konclude v0.7.0-1138 and HermiT (robot.jar)" \\
        --dest crates/owl-dl-reasoner/tests/fixtures/el_generated --n 300 --seed 20261106
"""
import argparse
import collections
import datetime
import hashlib
import json
import os
import random
import re
import shutil

PREFIX = "http://gen.rustdl/o#"


def local(iri):
    if not iri.startswith(PREFIX):
        raise SystemExit(f"unexpected IRI outside the generator namespace: {iri}")
    return iri[len(PREFIX):]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--run-out", required=True, help="a run's --out/<tag> dir (has cases/*.json)")
    ap.add_argument("--cases", required=True)
    ap.add_argument("--oracle-cache", required=True, help="scripts/gen/oracle.py's cache dir")
    ap.add_argument("--certified-from", required=True,
                    help="a run's --out/<tag> dir on the build the fixture is pinned to; its "
                         "classify `completeness_guaranteed` per case is recorded as `certified`")
    ap.add_argument("--certified-build", default="",
                    help="what build the --certified-from run used, for the MANIFEST")
    ap.add_argument("--source-build", default="",
                    help="what build the --run-out run used, for the MANIFEST")
    ap.add_argument("--oracle-peers", default="",
                    help="the oracle peers and versions behind --oracle-cache, for the MANIFEST")
    ap.add_argument("--dest", required=True)
    ap.add_argument("--n", type=int, default=300)
    ap.add_argument("--seed", type=int, default=20261106)
    a = ap.parse_args()

    recs = {}
    for name in sorted(os.listdir(os.path.join(a.run_out, "cases"))):
        r = json.load(open(os.path.join(a.run_out, "cases", name)))
        recs[r["case"]] = r
    agree = {c: r for c, r in recs.items() if r["oracle"]["verdict"] == "agree"}
    findings = sorted(c for c, r in agree.items() if r["bucket"] == "finding")
    if len(findings) > a.n:
        raise SystemExit(f"{len(findings)} findings exceed --n {a.n}")
    rng = random.Random(a.seed)
    by_motif = collections.defaultdict(list)
    for c in sorted(agree):
        if c not in findings:
            by_motif[agree[c]["motif"] or "none"].append(c)
    for v in by_motif.values():
        rng.shuffle(v)
    chosen = list(findings)
    motifs = sorted(by_motif)
    while len(chosen) < a.n and any(by_motif.values()):
        for m in motifs:
            if by_motif[m] and len(chosen) < a.n:
                chosen.append(by_motif[m].pop())
    chosen.sort()

    def certified(c):
        r = json.load(open(os.path.join(a.certified_from, "cases", c + ".json")))
        cls = r["arms"]["classify"]
        if cls.get("status") != "ok":
            raise SystemExit(f"{c}: classify arm {cls.get('status')} in --certified-from")
        return bool(cls["summary"]["completeness_guaranteed"])

    decl = re.compile(rb"Declaration\(Class\(:([A-Za-z0-9_]+)\)\)")
    cases_dest = os.path.join(a.dest, "cases")
    if os.path.isdir(cases_dest):
        shutil.rmtree(cases_dest)
    os.makedirs(cases_dest)
    lines = []
    for c in chosen:
        r = agree[c]
        src = os.path.join(a.cases, c + ".ofn")
        data = open(src, "rb").read()
        sha = hashlib.sha256(data).hexdigest()
        if sha != r["sha256"]:
            raise SystemExit(f"{c}: case file sha256 {sha} does not match the run record")
        open(os.path.join(cases_dest, c + ".ofn"), "wb").write(data)
        cached = json.load(open(os.path.join(a.oracle_cache, sha[:2], sha + ".json")))
        if cached.get("verdict") != "agree":
            raise SystemExit(f"{c}: cached oracle verdict is {cached.get('verdict')!r}, not agree")
        ans = cached["answer"]
        lines.append(json.dumps({
            "case": c,
            "sha256": sha,
            "motif": r["motif"],
            "was_finding": c in findings,
            "certified": certified(c),
            "classes": sorted(m.decode() for m in decl.findall(data)),
            "consistent": ans["consistent"],
            "unsat": sorted(local(x) for x in ans["unsat"]),
            "thing_equiv": sorted(local(x) for x in ans["thing_equiv"]),
            "pairs": sorted([local(s), local(t)] for s, t in ans["pairs"]),
        }, separators=(",", ":")))
    open(os.path.join(a.dest, "expected.jsonl"), "w").write("\n".join(lines) + "\n")
    # "# gen <commit> seed <seed> case <n> profile <p> ..." -> the run-wide part.
    words = agree[chosen[0]]["header"].lstrip("# ").split()
    header = " ".join(words[:4] + words[6:8])
    motif_counts = collections.Counter(agree[c]["motif"] or "none" for c in chosen)
    manifest = f"""# el_generated: a frozen sample of the #363 `el_guaranteed` generated net

Source run:      {os.path.basename(os.path.normpath(a.run_out))} ({a.source_build or "build not recorded"})
Generator:       {header}
Oracle:          {a.oracle_peers or "peers not recorded"}, agreement only
                 (`scripts/gen/oracle.py`); every expected answer is one both peers gave.
Certified:       `certified` per case = classify's `completeness_guaranteed` in run
                 {os.path.basename(os.path.normpath(a.certified_from))} ({a.certified_build or "build not recorded"}), the build
                 the fixture was frozen against;
                 the test fails if a certified case stops being certified.
Selection:       all {len(findings)} cases the source run bucketed `finding`, then a
                 stratified sample of the other `agree` cases (round-robin over motifs,
                 random.Random({a.seed})), {len(chosen)} cases in all.
Motifs:          {dict(sorted(motif_counts.items()))}
Exported:        {datetime.date.today().isoformat()} by scripts/gen/export_fixture.py
Regenerate:      python3 -I scripts/gen/export_fixture.py --run-out <run out/tag> \\
                     --cases <cases dir> --oracle-cache <oracle cache> \\
                     --certified-from <post-fix run out/tag> --certified-build '<build>' \\
                     --source-build '<build>' --oracle-peers '<peers>' --dest {a.dest} --n {a.n} --seed {a.seed}
Licence:         generated by rustdl's own `scripts/gen` from a seed; no third-party
                 content. Covered by the repository's licence.

expected.jsonl:  one JSON object per case; class names are local names in the
                 generator namespace <{PREFIX}>. `pairs` is the oracle's subsumption
                 closure over named classes (as `scripts/gen/run.py::score` compares it).
"""
    open(os.path.join(a.dest, "MANIFEST"), "w").write(manifest)
    print(f"exported {len(chosen)} cases ({len(findings)} former findings) to {a.dest}")


if __name__ == "__main__":
    main()
