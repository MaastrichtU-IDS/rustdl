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

## Role-polarity fuzz (profile `polarity`, `run.py run --realize`)

Built after #526–#528, three role-polarity bugs (an inverse role `r⁻` read as `r`) that this net could not see. The default profile draws role characteristics, disjoint roles and role assertions on named roles only, has no negative role assertion, draws `r⁻` in 15% of class expressions, makes the ABox optional, and has no instance or realize arm.

**The profile** (`gen.py`, `PolarityCase`, its own RNG stream):
- **Signed roles everywhere:** characteristics, disjoint roles, Domain/Range, positive and negative assertions, ≤n/≥n/∀/∃, with `r⁻` at 0.5. Every case has 3–5 individuals.
- **Planted motifs:** half the cases carry one motif (`# motif: <name>/<variant>`), shaped after #526, #527, #528, P3/P6/P9, `≤1 r⁻` against two predecessors, `∀r⁻` and `≥2 r⁻.{a, c}`. Each is a consistent variant (`ok`) or a control that changes the answer (`ctl`).
- **A floor:** every case has an `r⁻` in some role-consuming axiom.
- **A census:** `# polarity:` lists the kinds present.

**The realize arms** (`--realize`): `realize --json` and `realize --saturation-only --json`, besides the usual three.
- **The peers** classify an augmented copy with `N_x ≡ {x}` per individual (fresh names, a conservative extension), so `x : C` iff `N_x ⊑ C`.
- **rustdl** always runs on the original, because the nominals would switch its nominal gates on.
- **Agreement** is computed per projection: the original classes (the classify and consistent arms) and the `N_x` rows (the types). A disagreement on one projection does not unscore the other.
- **Scoring:** a type rustdl reports that both peers refute is a `type-FP` (or `satonly-type-FP`) finding. A missed type is exposed only. `realize` refusing a consistent KB is a `consistency-FP`.
- **Shrinking:** `shrink.py --realize` re-augments every subset.

**Note:** `Asymmetric(r⁻)` ≡ `Asymmetric(r)` (likewise Irreflexive), so the `p6` motifs check the `r⁻` spelling, not a polarity drop. `--sat` adds `sat` for every declared class (the per-query surface). A `sat` refusal of an inconsistent KB is a verdict; any other failure on an agreed-consistent case is a `rustdl-fail`.

**Mutants** (`mutants/M13.patch`, `mutants/M14.patch`): data, never applied in the tree, never built by CI. They are polarity faults planted where no audit read the code line by line:
- M13: `apply_max` counts `≤n r⁻` over `r`-successors;
- M14: the wedge's predecessor count drops `flip()`.

Each header gives its base. Build one with `git apply`, then `cargo build --release -p owl-dl-cli`, then revert; run it as a `detect` arm against a control built from the same base.

## Universal-role fuzz (profile `universal`)

Built after #624 (#543 part 2), which states the Direct-Semantics meaning of each foldable use of `owl:topObjectProperty` as built-in axioms: `∃u.X` / `≥1 u.X` with X non-empty → ⊤; `∀u.⊥`, `∀u.¬X`, `≤0 u.X` → ⊥; `Domain/Range(u, C)` → `⊤ ⊑ C`. Those are new positive derivations, and no other profile emits a top property.

**The profile** (`gen.py`, `UniversalCase`, its own RNG stream):
- **Top in every OWL 2 DL position:** `∃top`, `∀top`, `≤0 top`, `≥1 top`, `hasValue(top, a)`, Domain and Range of top, `Reflexive/Symmetric/Transitive(top)`, `r ⊑ top`, `top ⊑ r`, positive and negative top assertions, `∃top.C(a)`, and `d ⊑ topDataProperty`. `R()` returns top 30% of the time.
- **Legality as HermiT checks it** (measured one file per form): HermiT treats top as non-simple. So top never appears where a simple role is required, and `top ⊑ r` is drawn first, then `r` and every role above it leave the simple positions too.
- **Not covered:** #624's folds of `Self(u)`, `Irreflexive/Asymmetric(u)`, `Disjoint(u, s)`, `≤1`/`=1` on top, and `topDataProperty` in a class expression. HermiT rejects all of these, so the peers never adjudicate them.
- **Licences:** half the cases have 2–4 individuals with told types, for the conditional folds.
- **A floor and a census:** every case uses a top property; `# universal:` lists the kinds present.

**Scoring note:** `consistent --json`'s gate now also requires `universal_role_ordinary: false`. With it set, rustdl declares that a negative answer may be wrong, so a "consistent" on an inconsistent KB is exposed, not a finding. The first calibration batch flagged such a case (a non-folded `≤0 top.C`) as a finding before this change.

**Mutants** (`mutants/MU1.patch`, `mutants/MU2.patch`), faults planted in `top_bottom_roles.rs`'s `FoldCtx`:
- MU1: every atomic class counts as non-empty, so the licence check is gone and `∃u.C` folds to ⊤ for any `C`;
- MU2: `∀u.X` folds to ⊥ for any filler, not only `⊥` or `¬X` with X non-empty.

## Stop rule

A finding on unmutated main stops at a minimised reproducer that both peers adjudicate. It then goes to the manager as an issue draft. Nothing is fixed inside this package.
