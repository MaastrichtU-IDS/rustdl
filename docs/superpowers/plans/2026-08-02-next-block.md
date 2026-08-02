# Next Work Block Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Unblock two built-but-dormant correctness levers, settle one algorithmic hypothesis by experiment before building anything, fix a user-facing query-cost absurdity, decide one default on evidence, and close one unguarded value.

**Architecture:** Five independent tasks. Tasks 1, 3 and 5 change the reasoner; Task 2 is an experiment that writes no production code; Task 4 is a measurement that flips one default. No task depends on another's output, so they can be executed in any order — but the recommended order is 1 → 2 → 3 → 5 → 4, because Task 4 is the longest-running and Task 2 may cancel later work.

**Tech Stack:** Rust (edition 2024, toolchain pinned 1.95.0 but **build with `RUSTUP_TOOLCHAIN=stable`**), `owl-reasoner-harness` (Rust + Python) for corpus measurement, Konclude + HermiT as oracles.

## Global Constraints

Every task inherits all of these.

- **FP=0 is absolute.** No change may add a subsumption the Konclude ∪ HermiT oracle lacks. Any FP is a hard stop, not a trade-off.
- **Build:** `RUSTUP_TOOLCHAIN=stable cargo build --release`. A bare `cargo` FAILS (the pinned 1.95.0 toolchain has no cargo binary) and a skipped build then **silently reuses a stale binary**.
- **Pin binaries:** copy each build to a uniquely named path *immediately after the build that produced it*, named for its configuration, and `sha256sum` it before measuring. Measuring from a shared `target/release/` path has produced two retracted results in this project.
- **Cap every probe** in wall AND address space: `( ulimit -v $((24*1024*1024)); RAYON_NUM_THREADS=1 timeout N … )`. An uncapped run once reached 237 GB and OOM-killed this shared host.
- **Sabotage every canary** and report counts **as run, including survivors**. Recent honest counts here: 8/9, 4/6, 2/4, 6/6, 5/8. An unsabotaged guard is not evidence. **Sabotage runs must be strictly serial** — two concurrent sabotage builds in one worktree previously produced two false "survived" readings.
- **Prove any instrument fires** before believing its output. An instrument the compiler had removed was measured here for three rounds with silence read as data; a cut-based probe was later found not to fire at all on four ontologies and would have read as four confirmations.
- **Gates for any reasoner change:** `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets --all-features -- -D warnings`; `cargo test --workspace --exclude owl-dl-py` (**exclude `owl-dl-py`** — it fails to LINK on Python symbols in this environment, verified pre-existing and unrelated); and `./scripts/run-soundness-diff.sh` showing **11 VERIFIED with exact closures** — galen 27997, notgalen 32739, sio 8904, ore-10908 6001, wine 653, pizza 499, alehif 247, ro 158, ore-15672 142, sulo 51, bibtex 16 — with 3 documented-absent fixtures.
- **Never** `cmd | tail` then read `$?` (that is tail's status). `grep -c`/`pgrep -c` print `0` **and exit 1** on no match, so `x=$(grep -c … || echo 0)` yields `"0\n0"`; this has caused three bugs here.
- **Corpus:** `/data/dumontier/ore-run/pool_sample/files/<stem>.owl` (OWL functional syntax despite the extension), 1,920 files.
- **Commit** with `git -c user.name="Michel Dumontier" -c user.email="michel.dumontier@maastrichtuniversity.nl" commit`. Do not push; do not merge to `main`.
- **Disk is tight (~20 GB).** Do not accumulate per-ontology outputs; delete or digest.

---

## File Structure

| file | responsibility | tasks |
|---|---|---|
| `crates/owl-dl-core/src/convert.rs` | DKey lowering + the two dormant flags (`dkey_oneof_seed_enabled` `:2689`, `dkey_emit_order_enabled` `:2762`) | 1 |
| `crates/owl-dl-reasoner/src/lib.rs` | single-class query entry points (`is_class_satisfiable_internal_full` `:4239`); `classify_inconsistency_budget_ms` | 3, 5 |
| `crates/owl-dl-reasoner/src/classify.rs` | fragment gates (`is_pure_el` `:1358`, `saturator_complete_fragment` `:1711`, `tbox_only_saturator_eligible` `:1782`) | 3 |
| `crates/owl-dl-reasoner/tests/single_class_query_gate.rs` | **new** — Task 3 canaries | 3 |
| `crates/owl-dl-reasoner/tests/classify_inconsistency.rs` | existing — add the budget-value guard | 5 |
| `owl-reasoner-harness/scripts/sweep-arm.sh` | existing two-arm corpus sweep (per-chunk output files) | 1, 4 |
| `docs/2026-08-02-dkey-volume-scan.md` | **new** — Task 1 result | 1 |
| `docs/2026-08-02-binary-absorption-falsification.md` | **new** — Task 2 result | 2 |
| `docs/2026-08-02-domain-absorption-default.md` | **new** — Task 4 result | 4 |

---

## Task 1: DKey volume scan → decide both dormant flags

**Why:** `RUSTDL_DKEY_EMIT_ORDER` fixes a **live, non-monotonic** defect — `∀p.[0,5] ⊓ ∃p.{9}` is unsatisfiable alone, but adding an *unrelated* property mentioning the same keys makes it satisfiable again. Adding an axiom must never remove an entailment. `RUSTDL_DKEY_ONEOF_SEED` is the sixth D10-class bug (gate certifies complete, engine drops the axiom). Both are implemented, gated, FP=0-verified and sitting OFF **solely** because nobody ran the volume scan. Each emits *more* axioms, so the risk is volume re-inflation — the shape that caused the v0.3.29 conversion DNFs.

**Files:**
- Modify: `crates/owl-dl-core/src/convert.rs:2689` and `:2762` (defaults only, if the scan passes)
- Create: `docs/2026-08-02-dkey-volume-scan.md`

**Interfaces:**
- Consumes: `rustdl tbox-stats <file>` → prints `# concept_rules: N`.
- Produces: nothing other tasks depend on.

- [ ] **Step 1: Build and pin three binaries**

```bash
cd /data/dumontier/rustdl
export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
RUSTUP_TOOLCHAIN=stable cargo build --release -p owl-dl-cli
cp target/release/rustdl /tmp/rustdl-dkeyscan
sha256sum /tmp/rustdl-dkeyscan
```

One binary suffices — both flags are env-gated, so the arms are env settings, not builds.

- [ ] **Step 2: Verify the flags actually change something before scanning**

The scan is worthless if the flags are inert on the corpus. Prove they fire first:

```bash
cd /data/dumontier/rustdl
C=/data/dumontier/ore-run/pool_sample/files
for f in "" "RUSTDL_DKEY_EMIT_ORDER=1" "RUSTDL_DKEY_ONEOF_SEED=1" "RUSTDL_DKEY_EMIT_ORDER=1 RUSTDL_DKEY_ONEOF_SEED=1"; do
  echo -n "[$f] 5368: "
  env $f timeout 60 /tmp/rustdl-dkeyscan tbox-stats $C/ore_ont_5368.owl 2>/dev/null | grep -oP '^# concept_rules:\s+\K[0-9]+'
done
```

Expected: **`ore_ont_5368` reads 18,620,251 with both flags OFF.** That is the DKey-area
discriminator. **`ore_ont_9347` CANNOT validate this area** — it reads 113 under both a correct
gate and a no-op build, per CLAUDE.md. Report both, but judge on `5368`.

If no configuration changes any number anywhere in Step 3, the flags are corpus-inert and the
correct outcome is "flip ON, zero volume cost" — say so rather than manufacturing an effect.

- [ ] **Step 3: Scan `concept_rules` across the corpus, all four arms**

```bash
cd /data/dumontier/rustdl
C=/data/dumontier/ore-run/pool_sample/files
: > /tmp/dkeyscan.tsv
for o in $(ls $C/*.owl | xargs -n1 basename | sed 's/\.owl$//'); do
  row="$o"
  for f in "" "RUSTDL_DKEY_EMIT_ORDER=1" "RUSTDL_DKEY_ONEOF_SEED=1" "RUSTDL_DKEY_EMIT_ORDER=1 RUSTDL_DKEY_ONEOF_SEED=1"; do
    v=$( ( ulimit -v $((24*1024*1024)); env $f timeout 60 /tmp/rustdl-dkeyscan tbox-stats $C/$o.owl ) 2>/dev/null \
         | grep -oP '^# concept_rules:\s+\K[0-9]+' | head -1 )
    row="$row	${v:-NA}"
  done
  echo "$row" >> /tmp/dkeyscan.tsv
done
wc -l /tmp/dkeyscan.tsv
```

Conversion-only, so most are sub-second. Run with at most 3 concurrent if you parallelise;
record that you did. **Count and report `NA` rows** (conversion exceeded 60 s) — do not drop
them silently, a silent exclusion once inflated a corpus share 3× here.

- [ ] **Step 4: Analyse — the decision rule, fixed now**

```bash
python3 - <<'PY'
rows=[l.rstrip('\n').split('\t') for l in open('/tmp/dkeyscan.tsv') if l.strip()]
ok=[(r[0],*[int(x) for x in r[1:]]) for r in rows if all(x.isdigit() for x in r[1:])]
na=[r[0] for r in rows if not all(x.isdigit() for x in r[1:])]
print(f"measured {len(ok)}  NA {len(na)}")
for i,name in ((1,'EMIT_ORDER'),(2,'ONEOF_SEED'),(3,'BOTH')):
    worse=[(o,b,v[i-1]) for o,b,*v in [(r[0],r[1],*r[2:]) for r in ok] if False]
    grew=[(o,r[1],r[1+i]) for r in ok for o in [r[0]] if r[1+i] > r[1]]
    big=[(o,b,a) for o,b,a in grew if a > 2*max(b,1) and a-b > 100000]
    tot_b=sum(r[1] for r in ok); tot_a=sum(r[1+i] for r in ok)
    print(f"\n{name}: total concept_rules {tot_b} -> {tot_a} ({100*(tot_a-tot_b)/max(tot_b,1):+.2f}%)")
    print(f"  ontologies that GREW: {len(grew)}   grew >2x AND >100k: {len(big)}")
    for o,b,a in sorted(big,key=lambda t:-(t[2]-t[1]))[:8]: print(f"    {o}  {b} -> {a}")
PY
```

**Decision rule, fixed before seeing the numbers:**
- **No ontology grows >2× AND >100k `concept_rules`** ⇒ **flip both ON**. Volume risk is the
  only thing holding them back and it did not materialise.
- **Any ontology does** ⇒ keep that flag OFF, record the exact ontologies, and do **not**
  flip it on the argument that the correctness win outweighs — the v0.3.29 DNFs were caused by
  exactly this and cost more than the fix was worth.
- **`ore_ont_5368` must remain 18,620,251 in every arm.** If it moves, the flag is doing
  something unintended in the merge-aware component logic; stop and report.

- [ ] **Step 5: If the rule says flip, flip both defaults**

`convert.rs:2689` and `:2762`, using the house default-ON idiom (`is_none_or(|v| v != "0")`, as
seven other flags do; note empty-string ENABLES for default-ON flags — only an explicit `=0`
reverts). Update each doc comment to say **default ON since 0.4.12** and `=0` reverts.

- [ ] **Step 6: Update the flag-default tests**

Each flag needs a test pinning **both** halves: unset ⇒ enabled, and `=0` ⇒ disabled. The
second half matters as much as the first — a flag whose opt-out silently stopped working leaves
no way back. Find the existing default tests by `grep -rn 'RUSTDL_DKEY_EMIT_ORDER\|RUSTDL_DKEY_ONEOF_SEED' crates/ --include=*.rs | grep -i default` and invert them.

- [ ] **Step 7: Full gates**

Run every gate in Global Constraints. Additionally re-verify the two DKey discriminators
(`9347` = 113, `5368` = 18,620,251) with the new defaults.

- [ ] **Step 8: Write `docs/2026-08-02-dkey-volume-scan.md` and commit**

Must state: the per-arm totals, the grew-list, the `5368`/`9347` readings, the decision rule
*as fixed in advance*, and which flags were flipped. If the flags turned out corpus-inert, say
that plainly — inertness is a result, and it means the FP=0 net demonstrates non-regression
only. The curated corpus **cannot** validate the DKey area (`datatype_value_membership.rs` says
so itself: "the corpus has NO such clash, so these canaries are the ENTIRE safety net").

---

## Task 2: Binary-absorption falsification (experiment only — no production code)

**Why:** `concept_rule_or` is the one factor that discriminates completing from failing ontologies (AUC **0.849**, holding **0.808/0.815/0.880** within three size-matched bands; 56% of completing ontologies have **zero** disjunctive concept rules against 15% of failing ones). Binary absorption (Hudek & Weddell) is the technique that reduces it, with a measured population of **34,667** `Or`-conclusion concept rules still carrying a `¬Atomic`. **But AUC is discrimination, not causation**, and this project has twice built on exactly that inference and had to retract. So: falsify first, build only if it survives.

**Files:**
- Create: `docs/2026-08-02-binary-absorption-falsification.md`
- **No source changes.**

**Interfaces:**
- Consumes: `rustdl tbox-stats <file>` → `# concept_rule_or: N`.
- Produces: a GO/NO-GO that gates any future binary-absorption work.

- [ ] **Step 1: Pick targets by measurement, not by guess**

```bash
cd /data/dumontier/rustdl
C=/data/dumontier/ore-run/pool_sample/files
: > /tmp/ruleor.tsv
while read -r o; do
  v=$( ( ulimit -v $((24*1024*1024)); timeout 60 ./target/release/rustdl tbox-stats $C/$o.owl ) 2>/dev/null \
       | grep -oP '^# concept_rule_or:\s+\K[0-9]+' | head -1 )
  c=$( ( timeout 60 ./target/release/rustdl tbox-stats $C/$o.owl ) 2>/dev/null | grep -oP '^# concept_rules:\s+\K[0-9]+' | head -1 )
  echo -e "$o\t${v:-NA}\t${c:-NA}" >> /tmp/ruleor.tsv
done < /data/dumontier/owl-reasoner-harness/baselines/2026-08-01-survivors-167-list.txt
sort -t$'\t' -k2,2nr /tmp/ruleor.tsv | head -20
```

Choose **6 targets**: the 3 highest `concept_rule_or`, and 3 with high `concept_rule_or` but
**small `concept_rules`** (under ~10,000). The second group matters because the two giants in
the previous falsification had 51,810 class declarations and their non-recovery could be
dismissed as scale. A small ontology cannot be.

- [ ] **Step 2: Construct the intervention and PROVE IT FIRES**

Binary absorption removes disjunctive concept rules arising from `A ⊓ B ⊑ C`. The cheap proxy
is to delete the source axioms whose antecedent is a conjunction:

```bash
C=/data/dumontier/ore-run/pool_sample/files
o=<TARGET>
grep -vE '^SubClassOf\(\s*ObjectIntersectionOf' $C/$o.owl > /tmp/$o-cut.ofn
echo "before: $(./target/release/rustdl tbox-stats $C/$o.owl 2>/dev/null | grep -oP 'concept_rule_or:\s+\K[0-9]+')"
echo "after : $(./target/release/rustdl tbox-stats /tmp/$o-cut.ofn 2>/dev/null | grep -oP 'concept_rule_or:\s+\K[0-9]+')"
```

**MANDATORY GATE: if `concept_rule_or` does not drop substantially, the intervention did not
fire and that target is unusable — discard it and pick another.** In the previous falsification
four of six targets were measured with a cut that changed nothing (residuals unchanged), and the
runs would have read as four confirmations. Report every target you discarded and why.

If `ObjectIntersectionOf` antecedents are not the source, find what is by inspecting the
ontology, and say so.

- [ ] **Step 3: Run the falsification**

```bash
for f in "$C/$o.owl:uncut" "/tmp/$o-cut.ofn:CUT"; do
  p=${f%%:*}; t=${f##*:}
  s=$(date +%s.%N)
  ( ulimit -v $((24*1024*1024)); RAYON_NUM_THREADS=1 timeout 120 ./target/release/rustdl classify "$p" ) >/tmp/bf.out 2>/dev/null
  rc=$?; e=$(date +%s.%N)
  printf "%s %s rc=%s %.1fs subs=%s\n" "$o" "$t" "$rc" "$(python3 -c "print($e-$s)")" "$(grep -c '^direct' /tmp/bf.out)"
done
```

Serially, on an idle host, one at a time.

- [ ] **Step 4: Apply the decision rule, fixed now**

**The logic is one-directional and that is what makes it cheap:** deletion is strictly
**stronger** than absorption, because absorption removes the rule *while preserving semantics*.
So **no rescue under deletion ⇒ no rescue under absorption**, and binary absorption dies for
the price of six runs.

- **0 of 6 rescued** ⇒ **NO-GO.** Record it. Absorption as a family is then closed
  (domain done, qualified-∃ closed, binary closed) and the tail needs a different lens.
- **≥1 rescued, including at least one small ontology** ⇒ **GO.** Write a spec; the build is
  bounded (no backward role rule needed, unlike qualified-∃).
- **Only large ontologies rescued** ⇒ inconclusive on scale grounds; report as such rather
  than rounding to GO.

- [ ] **Step 5: Write the doc and commit**

State the targets, the fire-check for each (including discards), the runs, the rule as fixed in
advance, and the verdict. **A NO-GO is a successful outcome** — it stops a build that would not
have paid, which is exactly what the qualified-∃ experiment achieved.

---

## Task 3: Single-class queries must not cost more than classifying everything

**Why:** `is_class_satisfiable` / `is_subclass_of` / `is_consistent` gate their fast path on `classify::is_pure_el` only, never on `saturator_complete_fragment` or `tbox_only_saturator_eligible` — which `classify` and `realize` both use. Consequence measured on `ore_ont_10068`: asking about **one** class takes **3.50 s** while classifying **every** class takes **2.52 s**. That is a user-facing absurdity in the public API, not a benchmark artifact.

**Files:**
- Modify: `crates/owl-dl-reasoner/src/lib.rs:4239` (`is_class_satisfiable_internal_full`) and the sibling subclass/consistency entry points
- Create: `crates/owl-dl-reasoner/tests/single_class_query_gate.rs`

**Interfaces:**
- Consumes: `classify::saturator_complete_fragment(&InternalOntology) -> bool` (`classify.rs:1711`) and `classify::tbox_only_saturator_eligible(&InternalOntology) -> bool` (`classify.rs:1782`), both already `pub(crate)`.
- Produces: no new public API.

- [ ] **Step 1: Write the failing test**

```rust
// crates/owl-dl-reasoner/tests/single_class_query_gate.rs
//! A single-class query must not be slower than classifying every class.
//! Gate parity: the one-class entry points must admit the same fragments
//! `classify` does, not just `is_pure_el`.

use std::time::Instant;

#[test]
fn one_class_query_is_not_slower_than_classifying_all() {
    let path = "/data/dumontier/ore-run/pool_sample/files/ore_ont_10068.owl";
    if !std::path::Path::new(path).exists() {
        eprintln!("fixture absent; skipping");
        return;
    }
    let onto = load(path);

    let t0 = Instant::now();
    let all = owl_dl_reasoner::classify(&onto).expect("classify");
    let classify_all = t0.elapsed();
    let some_class = all.classes().first().expect("at least one class").clone();

    let t1 = Instant::now();
    let _ = owl_dl_reasoner::is_class_satisfiable(&onto, &some_class).expect("sat");
    let one_class = t1.elapsed();

    assert!(
        one_class <= classify_all,
        "one-class query {one_class:?} exceeded classify-all {classify_all:?} — \
         the single-class gate is narrower than classify's"
    );
}
```

Replace `load` and the exact API names with whatever the crate's existing integration tests use
(`grep -rn 'fn load' crates/owl-dl-reasoner/tests/ | head`), and `all.classes()` with the real
accessor. Do not invent API.

- [ ] **Step 2: Run it and confirm it FAILS**

```
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test single_class_query_gate
```
Expected: FAIL, one-class ≈3.50 s vs classify-all ≈2.52 s.

- [ ] **Step 3: Widen the gate**

In `is_class_satisfiable_internal_full` (`lib.rs:4239`) the line `let pure_el = classify::is_pure_el(&internal);` decides the fast path. Widen it to the same disjunction `classify` uses, behind a flag **default OFF**:

```rust
let pure_el = classify::is_pure_el(&internal)
    || (single_class_gate_parity_enabled()
        && (classify::saturator_complete_fragment(&internal)
            || classify::tbox_only_saturator_eligible(&internal)));
```

Apply the identical change to the subclass and consistency entry points. **Read each one first**
— if a caller depends on `pure_el` meaning literally "pure EL" for something other than fast-path
dispatch, do not overload it; introduce a separate `fast_path_eligible` local instead.

Add, next to the other flag readers:

```rust
/// Gate parity for the single-class query entry points
/// (`RUSTDL_SINGLE_CLASS_GATE_PARITY`, **default OFF**; `=1` enables).
///
/// `is_class_satisfiable` / `is_subclass_of` / `is_consistent` gated their
/// saturation fast path on `is_pure_el` alone, while `classify` and `realize`
/// also admit `saturator_complete_fragment` and `tbox_only_saturator_eligible`.
/// On `ore_ont_10068` that made ONE class cost 3.50 s against 2.52 s to
/// classify EVERY class.
///
/// Sound by reuse: these are the same gates `classify` already trusts, and the
/// saturation path they select is the one `classify` already runs.
#[must_use]
pub fn single_class_gate_parity_enabled() -> bool {
    std::env::var_os("RUSTDL_SINGLE_CLASS_GATE_PARITY").is_some_and(|v| v == "1")
}
```

- [ ] **Step 4: Run the test with the flag on; confirm it passes**

```
RUSTDL_SINGLE_CLASS_GATE_PARITY=1 RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test single_class_query_gate
```

- [ ] **Step 5: Verdict identity — the core correctness claim**

The widened gate must not change any *answer*. On at least 8 ontologies spanning EL, Horn and
hybrid (use curated fixtures under `ontologies/` plus 3 ORE ontologies that complete), compare
flag-ON vs flag-OFF for `is_class_satisfiable` on every class, and for `is_consistent`. **Every
verdict must match.** A widened gate that changes a verdict means the gate admits something the
saturator does not handle — which would be a seventh D10-class bug, and a hard stop.

- [ ] **Step 6: Sabotage the canaries**

At minimum: (a) revert the widening → the timing test must fail; (b) widen the gate to admit
*everything* (`|| true`) → a verdict-identity test must fail, proving the identity check is not
vacuous. Report counts as run.

- [ ] **Step 7: Full gates, then flip default ON if Step 5 passed**

This repairs a user-facing cost absurdity with no verdict change, so default ON is right —
argue if you disagree. Add a default test pinning both halves (unset ⇒ ON, `=0` ⇒ OFF).

- [ ] **Step 8: Commit**

---

## Task 4: Domain-absorption default decision, by two-arm sweep

**Why:** `RUSTDL_DOMAIN_ABSORPTION` is merged **default OFF**. It is sound by logical identity with `ObjectPropertyDomain`, verdict-preserving (13/13 curated and 10/10 ORE closures byte-identical, FP=0 net exact), and recovers **4** real DNFs (`ore_ont_3281`: 28 residuals → 0, DNF@300 s → 11.4 s). It is OFF only because it alters the absorbed TBox of **1,030 of 1,913** pool ontologies and has never had a wall check at that scale — and this project has already shipped one regression (v0.4.8's `CLASSIFY_INCONSISTENCY`) by flipping a default on a 12-ontology benchmark.

**Files:**
- Modify: the `RUSTDL_DOMAIN_ABSORPTION` reader (find with `grep -rn 'RUSTDL_DOMAIN_ABSORPTION' crates/ --include=*.rs | grep var_os`), defaults only
- Create: `docs/2026-08-02-domain-absorption-default.md`

**Interfaces:**
- Consumes: `owl-reasoner-harness/scripts/sweep-arm.sh <pinned_binary> <tag>` — writes **one output file per chunk** and concatenates. Use it; a previous sweep pointed four concurrent chunks at a single `--out` and produced 40 unparseable records plus 73 silently missing ontologies.
- Produces: nothing other tasks depend on.

- [ ] **Step 1: Build and pin one binary; the arms are env settings**

```bash
cd /data/dumontier/rustdl
export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
RUSTUP_TOOLCHAIN=stable cargo build --release --workspace
cp target/release/rustdl /tmp/rustdl-domabs && sha256sum /tmp/rustdl-domabs
```

- [ ] **Step 2: Wrap each arm so the env is baked into the "reasoner"**

`sweep-arm.sh` takes a binary path, so create two wrappers:

```bash
printf '#!/usr/bin/env bash\nexec env RUSTDL_DOMAIN_ABSORPTION=0 /tmp/rustdl-domabs "$@"\n' > /tmp/arm-off.sh
printf '#!/usr/bin/env bash\nexec env RUSTDL_DOMAIN_ABSORPTION=1 /tmp/rustdl-domabs "$@"\n' > /tmp/arm-on.sh
chmod +x /tmp/arm-off.sh /tmp/arm-on.sh
/tmp/arm-on.sh --version   # prove the wrapper runs
```

- [ ] **Step 3: Run both arms SEQUENTIALLY**

```bash
cd /data/dumontier/owl-reasoner-harness
./scripts/sweep-arm.sh /tmp/arm-off.sh DOMABS-OFF
./scripts/sweep-arm.sh /tmp/arm-on.sh  DOMABS-ON
```

Sequential, not concurrent: contention would inflate whichever arm shares the host and could
manufacture or hide a regression. Expect several hours per arm. `sweep-arm.sh` already passes
`--cap-secs 60 --threads 1 --digest-output`.

- [ ] **Step 4: Compare with the same analysis the v0.4.6→main sweep used**

Report: outcome transitions both directions (**`ok → dnf` is the blocking finding**), answer
changes among both-completing ontologies via the stdout digest, count materially slower
(>25% AND >2 s), and peak-RSS growth (>2× AND >500 MB).

**Digest caveat, learned the hard way:** a digest difference is **not** automatically an answer
change — banner lines are nondeterministic. For any digest mismatch, strip `^#` lines and
re-compare, and run an ON-vs-ON control on that ontology before attributing a difference to the
flag. And when checking "identical", make sure an **empty** output is not being counted as a
difference by your own guard — that produced a false "4 differ" reading previously.

- [ ] **Step 5: Decide, by a rule fixed now**

- **0 regressions, 0 answer changes, no material slowdowns** ⇒ **flip default ON**.
- **Any `ok → dnf`** ⇒ keep OFF, record the ontologies, and root-cause before reconsidering.
- **Any answer change surviving the strip-and-control check** ⇒ **hard stop.** The change is
  advertised as verdict-preserving; a real diff means that claim is false.

- [ ] **Step 6: If flipping, update the default and its test; run all gates; commit with the doc**

---

## Task 5: Guard the `CLASSIFY_INCONSISTENCY_MS` value

**Why:** sabotage of the v0.4.11 regression fix found that **slashing the default from 3000 ms to 1 ms passes every canary** — because every cheap synthetic clash is also caught by an unbudgeted route. The substance of a shipped default is untested. The value is load-bearing: `family.ofn`'s ABox saturation alone takes ~2.0 s, so any budget under ~2.5 s silently re-breaks the detection the flag exists for.

**Files:**
- Modify: `crates/owl-dl-reasoner/tests/classify_inconsistency.rs`

**Interfaces:**
- Consumes: `owl_dl_reasoner::classify_inconsistency_budget_ms() -> u64`.
- Produces: nothing.

- [ ] **Step 1: Write the failing test**

```rust
/// The BUDGET VALUE is load-bearing and was previously unguarded: sabotage
/// showed that slashing the default 3000 ms to 1 ms passed every canary,
/// because each cheap synthetic clash is also caught by an unbudgeted route.
/// `family.ofn`'s ABox saturation alone takes ~2.0 s, so any default below
/// ~2.5 s silently re-breaks the detection this flag exists for — a fix that
/// quietly reintroduces the bug it repairs.
#[test]
fn budget_default_is_large_enough_for_family() {
    let ms = owl_dl_reasoner::classify_inconsistency_budget_ms();
    assert!(
        ms >= 2500,
        "default budget {ms} ms is below the ~2.5 s that family.ofn's ABox \
         saturation needs; the inconsistency detection would silently regress"
    );
}
```

- [ ] **Step 2: Run it — it must PASS on the current default (3000)**

```
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner --test classify_inconsistency budget_default
```

- [ ] **Step 3: Prove non-vacuity by sabotage**

Temporarily set `DEFAULT_MS` to `1` in `classify_inconsistency_budget_ms`, re-run, and confirm
the test FAILS. Restore. **This is the whole point of the task** — the previous canary set
passed under exactly this mutation.

- [ ] **Step 4: Add the behavioural half**

A constant check alone is weak. Add an end-to-end test that `family.ofn` is still detected
inconsistent **at the default budget**, and — crucially — that it is **NOT** detected at a
deliberately tiny budget, which proves the budget is what governs:

```rust
#[test]
fn family_detection_depends_on_the_budget() {
    let p = "ontologies/real/family.ofn";
    if !std::path::Path::new(p).exists() { eprintln!("fixture absent; skipping"); return; }
    // default budget: detected
    let _g = EnvGuard::set(&[("RUSTDL_CLASSIFY_INCONSISTENCY_MS", "3000")]);
    assert!(!classify_reports_consistent(p), "family must be inconsistent at 3000 ms");
    drop(_g);
    // 1 ms: the pre-check is abandoned, so classify falls through and does NOT report it
    let _g = EnvGuard::set(&[("RUSTDL_CLASSIFY_INCONSISTENCY_MS", "1")]);
    assert!(classify_reports_consistent(p), "at 1 ms the pre-check must be abandoned");
}
```

Use the file's existing `EnvGuard` and add a `classify_reports_consistent` helper following the
existing tests' style. **If the 1 ms arm still detects inconsistency, that is informative, not a
failure to paper over** — it means another route finds it and the budget is less load-bearing
than believed. Report that and adjust the assertion to match reality.

- [ ] **Step 5: Full gates and commit**

---

## Execution order and stopping rules

Recommended: **1 → 2 → 3 → 5 → 4.** Task 4 is the longest (two full-corpus arms) and should run
last or in background; Task 2 may cancel future work and so is worth doing early.

**Stopping rules, operational:**
- **Task 1:** if any ontology grows >2× AND >100k `concept_rules`, that flag stays OFF. Do not
  override on correctness grounds — the v0.3.29 conversion DNFs came from exactly this.
- **Task 2:** if the intervention does not fire on a target (measured, not assumed), that target
  is discarded and reported, not counted.
- **Task 3:** any verdict change under the widened gate is a hard stop, not a tuning problem.
- **Task 4:** any `ok → dnf` blocks the flip.
- **Any task:** if a premise does not reproduce, stop that task and report it. Three premises
  were refuted in the previous block and each saved more time than it cost.

## Self-review notes

- Every task states its decision rule **before** the measurement, so results cannot be
  rationalised afterwards.
- Every measurement task has an explicit **instrument-fires** check, because a cut-based probe
  silently failed to fire on four of six targets in the previous block.
- No task depends on another's output, so a NO-GO in Task 2 does not block Tasks 1, 3, 4 or 5.
