# Generated-ontology oracle net (#363 step 3)

The net generates small, legal OWL 2 DL ontologies and runs rustdl on each. It flags any answer where rustdl contradicts something Konclude and HermiT **both** agree on. It is a local tool and does not run in CI. The results are in `docs/assurance/query-surface-ledger.md` ("Generated-ontology oracle net").

| file | role |
|---|---|
| `gen.py` | seeded generator: 11 families; legality (regularity, simplicity) by construction and re-checked. Profile `el_guaranteed` (#363 step 3a) is a separate generator that stays inside rustdl's pure-EL gate, so `completeness_guaranteed` holds and a missed entailment is a finding; it plants #114/#81-shaped motifs and records them as `# motif:`. Profile `el_top` (#444/#445) is `el_guaranteed` plus ⊤-LHS GCIs (`⊤ ⊑ X`, `⊤ ≡ X`), which `el_guaranteed` never emits, on its own RNG stream; its motifs `top_exists`, `top_chain` and `top_marker` each end in a scored pair |
| `oracle.py` | Konclude ∩ HermiT agreement oracle, cached by sha256 |
| `run.py` | runs three rustdl arms per case and scores them; `report`, `detect` (mutant vs control) |
| `shrink.py` | ddmin to a minimal reproducer of one finding, gated on peer agreement |
| `seeds.txt` | the committed seeds |
| `../owl_readers.py` | peer-output readers, shared with `cross-command-laws.py` |

## Run it

```sh
source ~/rustdl/env.sh                      # JAVA_HOME, ROBOT_JAR; Konclude at ~/.local/bin/konclude
W=~/rustdl/work-363-gen
python3 scripts/gen/run.py gen --seed 20261103 --n 2000 --cases $W/cases
python3 scripts/gen/run.py run --cases $W/cases --bin <pinned rustdl> --tag control \
    --out $W/out --oracle-cache $W/ocache --cores 92,93,94
python3 scripts/gen/run.py run ... --tag M8b --bin <mutant pin> --first 500
python3 scripts/gen/run.py detect --out $W/out --control control --mutant M8b --first 500
python3 scripts/gen/shrink.py --case $W/cases/case-00042.ofn --kind pair-FP \
    --witness 'http://gen.rustdl/o#C1|http://gen.rustdl/o#C3' --bin <pin> \
    --oracle-cache $W/ocache --out $W/shrunk/42
```

Self-tests: `gen.py --self-test`, `run.py --self-test`.

## How a case is bucketed

A case goes into the first bucket that applies:

1. **invalid**: HermiT rejects the input as illegal (non-simple role or irregular hierarchy), or the input declares no classes. This is a generator defect, and the case is never scored.
2. **peer-fail**: a peer timed out (60 s cap) or failed.
3. **peer-disagree**: both peers finished but disagree on consistency, unsat classes, Thing-equivalent classes or pairs. The case is kept, never scored. The report says which peer rustdl sides with.
4. **rustdl-fail**: a rustdl arm crashed or hit its 30 s cap.
5. **finding**: an FP of any kind (never budget-exempt), a pair or unsat MISSED with `completeness_guaranteed: true`, or a gated `consistent --json` miss.
6. **exposed**: only misses that rustdl itself declares possible (`completeness_guaranteed: false`, trusted Sat, refused clauses, a deadline).

   A consistency miss on classify's `consistent` field is the pre-check tier: it is reported, but it does not change the bucket (a case with only that is **clean**).
7. **clean**: none of the above.

Peer verdicts are read from output content, never from exit codes:
- Konclude exits 0 on junk. Its run counts only if the hierarchy mentions every declared class, and inconsistency is read from the hierarchy, not from the log.
- HermiT writes nothing on an inconsistent KB and logs `InconsistentOntologyException`. A clean run logs nothing.

## Stop rule

A finding on unmutated main stops at a minimised reproducer that both peers adjudicate. It then goes to the manager as an issue draft. Nothing is fixed inside this package.
