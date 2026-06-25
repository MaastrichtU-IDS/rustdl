#!/usr/bin/env python3
"""ddmin over the 52-axiom neighborhood (fast: tiny ontology per test)."""
import re, subprocess, os, tempfile
RUSTDL="/Users/micheldumontier/code/rustdl/target/release/rustdl"
SRC=os.path.expanduser("~/data/ore-run/input/ore_ont_9786.ofn")
SUB="http://semanticscience.org/resource/SIO_000500"
SUP="http://semanticscience.org/resource/SIO_000441"
raw=open(SRC).read()
m=re.match(r'(.*?Ontology\([^\n]*\n)(.*)\n\)\s*$',raw,re.S)
header=m.group(1)
lines=[l.strip() for l in m.group(2).split('\n') if l.strip()]
logical=[l for l in lines if not l.startswith(('Declaration(','Prefix('))]
core={'SIO_000500','SIO_000501','SIO_000441','SIO_000440','SIO_000507','SIO_000532',
      'SIO_000402','SIO_000541','SIO_000400','SIO_000401','SIO_000313','SIO_000369'}
def ids(s): return set(re.findall(r'SIO_\d+',s)) | set(re.findall(r'SIO_0\d+',s))
nb=[l for l in logical if ids(l) & core]
def write(sub):
    used=set().union(*[ids(a) for a in sub]) if sub else set()
    props=set(re.findall(r'SIO_\d+', " ".join(sub)))
    decls=[f"Declaration(Class(<http://semanticscience.org/resource/{c}>))" for c in sorted(used)]
    # declare object properties used in role positions
    pdecls=[f"Declaration(ObjectProperty(<http://semanticscience.org/resource/{p}>))"
            for p in ('SIO_000369','SIO_000313','SIO_000310','SIO_000273','SIO_000068','SIO_000059')]
    f=tempfile.NamedTemporaryFile("w",suffix=".ofn",delete=False)
    f.write(header+"\n".join(decls+pdecls+sub)+"\n)\n"); f.close(); return f.name
def holds(sub):
    p=write(sub)
    try: out=subprocess.run([RUSTDL,"explain",p,SUB,SUP],capture_output=True,text=True,timeout=30).stdout
    except subprocess.TimeoutExpired: os.unlink(p); return False
    os.unlink(p); return " : yes" in out
print(f"neighborhood: {len(nb)} axioms; FP reproduces: {holds(nb)}", flush=True)
if not holds(nb):
    print("FP does NOT reproduce in neighborhood alone — needs wider context."); raise SystemExit
c=nb; n=2
while len(c)>=2:
    chunk=max(1,len(c)//n); parts=[c[i:i+chunk] for i in range(0,len(c),chunk)]; red=False
    for i in range(len(parts)):
        comp=[x for j,p in enumerate(parts) if j!=i for x in p]
        if comp and holds(comp): c=comp; n=max(n-1,2); red=True; print(f"  -> {len(c)}",flush=True); break
    if not red:
        if n>=len(c): break
        n=min(len(c),n*2)
print(f"\n=== MINIMAL: {len(c)} axioms ===")
for a in c: print(a)
open(os.path.expanduser("~/data/ore-run/min_repro_final.ofn"),"w").write(open(write(c)).read())
print("\nwrote ~/data/ore-run/min_repro_final.ofn")
