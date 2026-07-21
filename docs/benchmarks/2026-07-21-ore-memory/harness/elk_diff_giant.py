#!/usr/bin/env python3
"""Full-closure FP/MISS diff for one EL giant vs ELK (SNOMED-scale gold standard).
Convert .owl->owx (ROBOT), ELK reason (direct subclass, -Xmx120g), parse+filter
owl:Thing/Nothing/reflexive, transitive-close both sides, diff.
  rustdl - ELK = candidate FP (soundness-critical);  ELK - rustdl = candidate MISS
Usage: elk_diff_giant.py <ont.owl>"""
import sys, os
import xml.etree.ElementTree as ET
SP = "/mnt/um-share-drive/dumontier/rustdl-scratch"
JAVA = f"{SP}/tools/jdk-17.0.19+10-jre/bin/java"; ROBOT = f"{SP}/tools/robot.jar"
g = {}
exec(open(f"{SP}/four_way.py").read().split("def main()")[0], g)
rustdl_c, frag, run = g["rustdl_c"], g["frag"], g["run"]
g["RAWCAP"] = 60_000_000; g["OUTCAP"] = 60_000_000; g["BYTECAP"] = 600_000_000
close = g["close"]
TRIVIAL = {"Thing", "Nothing", "owl:Thing", "owl:Nothing"}
def _local(t): return t.rsplit("}", 1)[-1]
def _cf(el): return frag(el.get("IRI") or el.get("abbreviatedIRI") or "")
def elk_pairs(path):
    pairs = []
    for _, el in ET.iterparse(path, events=("end",)):
        t = _local(el.tag)
        if t == "SubClassOf":
            ks = [k for k in el if _local(k.tag) == "Class"]
            if len(ks) == 2:
                a, b = _cf(ks[0]), _cf(ks[1])
                if a and b and a != b and a not in TRIVIAL and b not in TRIVIAL: pairs.append((a, b))
            el.clear()
        elif t == "EquivalentClasses":
            ks = [k for k in (_cf(k) for k in el if _local(k.tag) == "Class") if k and k not in TRIVIAL]
            for x in ks:
                for y in ks:
                    if x != y: pairs.append((x, y))
            el.clear()
    return pairs

ofn = sys.argv[1]; base = os.path.basename(ofn)
owx = f"{SP}/gt_{base}.owx"; elk = f"{SP}/gt_{base}.elk.owx"
print(f"[{base}] convert ...", flush=True)
run([JAVA, "-Xmx60g", "-jar", ROBOT, "convert", "-i", ofn, "-o", owx, "--format", "owx"], tmo=900)
print(f"[{base}] ELK reason ...", flush=True)
run([JAVA, "-Xmx120g", "-jar", ROBOT, "reason", "-r", "ELK", "-i", owx,
     "--axiom-generators", "subclass", "-o", elk], tmo=1500)
print(f"[{base}] rustdl ...", flush=True)
R = rustdl_c(ofn)
E = close(elk_pairs(elk))
fp, miss = R - E, E - R
print(f"RESULT {base}: rustdl={len(R)} elk={len(E)} FP={len(fp)} MISS={len(miss)}")
for lbl, s in (("FP", fp), ("MISS", miss)):
    for a, b in list(sorted(s))[:10]: print(f"    {lbl}: {a} <= {b}")
for f in (owx, elk):
    try: os.unlink(f)
    except OSError: pass
