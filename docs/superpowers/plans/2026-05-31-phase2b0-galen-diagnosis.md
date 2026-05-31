# Phase 2b.0 — GALEN MISSED Diagnosis Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Determine WHAT calculus pattern(s) the 109 GALEN MISSED actually require, by extracting the full pair list, sampling representatively, building minimal HermiT-verified repros, and writing up the actual pattern clusters with rule-shape recommendations for Phase 2b proper.

**Architecture:** This is forensic, not implementation work. Output is `docs/phase2b-galen-diagnosis.md` (the report). Method: bump the harness's MISSED-print limit temporarily to capture all 109 pairs; sample ~8 covering different IRI clusters; for each, use ROBOT's `extract --method bot` to compute a ⊥-locality syntactic module (sound under-approximation: HermiT-equivalent on the targeted pair, with all axioms not needed for THAT entailment stripped); confirm HermiT derives the entailment on the minimal module; confirm rustdl `--saturation-only` AND default classify miss it on the same module; analyse the actual axiom shapes; cluster; recommend.

**Tech Stack:** ROBOT v1.9.6 + HermiT via Docker (`docker/robot/classify-oracle.sh` from Phase 0 Task 1 + `robot extract`), the existing closure-diff harness, bash + grep for axiom extraction, no Rust changes beyond the harness limit bump.

---

## Background the executor needs

- Phase 2a (commits `13dc25d..f61e06a`) implemented the EL++ functional-role witness-merge rule based on the handoff's `PathologicalCondition` trace. T6 measurement: 109 GALEN MISSED → 109 (zero recovered). The handoff's trace was incomplete; the actual MISSED pattern is unknown. `docs/phase2a-results.md` records the disproof; `docs/hypertableau-dead-ends.md` §14 records the T4 dead-end. Phase 2b is now gated on this diagnosis (`docs/superpowers/specs/2026-05-31-soundness-completeness-perf-design.md` §"Phase 2 — Deep completeness calculus" 2b.0).
- The harness `crates/owl-dl-reasoner/tests/konclude_closure_diff.rs` prints up to 50 MISSED pairs (line 262). For GALEN's 109 we need a temporary bump.
- GALEN fixture is `ontologies/external/galen.{ofn,-classified.owx}`. Already on disk; rustdl parses it (verified in Phase 2a T6). HermiT classifies it (the `-classified.owx` reference is HermiT- or Konclude-derived).
- ROBOT's `extract --method bot --term-file <iris> --input galen.owl --output module.owl` produces a ⊥-locality syntactic module preserving entailments over the term set. We use it to get a minimal `(sub, sup)`-relevant slice — typically dozens of axioms instead of thousands — that's tractable to read manually.
- HermiT is the sound+complete reference. If HermiT derives the entailment on the minimal module AND rustdl misses it, the gap is real and characterizable. If HermiT also misses on the minimal module, the locality extract was too aggressive — broaden the term set and retry.
- Dead-end #12's frame-change discipline applies: when localizing a rustdl gap, alway run BOTH `--saturation-only` AND default `classify` to know whether the miss is in saturation, in the wedge, or in the tableau path.
- Wall budget: GALEN classify takes ~12 min on this hardware at 200ms per-pair (per Phase 2a T6). The MISSED-list extraction is one re-run (~12 min); the per-pair forensic work uses MINIMAL modules (sub-second). Plan as half a day of wall, mostly waiting on the GALEN re-run.

---

## Task 1: Extract the full GALEN MISSED pair list

**Files:**
- Temporarily modify: `crates/owl-dl-reasoner/tests/konclude_closure_diff.rs:262` (revert after capture).
- Create: `docs/phase2b-galen-missed-pairs.txt` (the full 109-line dump, committed).

- [ ] **Step 1: Bump the print limit**

Edit `crates/owl-dl-reasoner/tests/konclude_closure_diff.rs:262` from:

```rust
let missed_limit = if missed.len() <= 50 { missed.len() } else { 50 };
```

to:

```rust
let missed_limit = missed.len();  // P2b.0 diagnosis: print all MISSED
```

- [ ] **Step 2: Run GALEN, capture all MISSED lines**

```bash
timeout 1500 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    galen_closure_matches_konclude \
    -- --ignored --nocapture 2>&1 | tee /tmp/p2b0-galen-missed.log
```

Expected: ~12-15 min wall. The output's MISSED lines have shape `" MISSED: <sub-iri> ⊑ <sup-iri>"`. The headline harness line should still show `FP=0 MISSED=109` matching Phase 2a's measurement.

If the count differs from 109 (a few +/-): that's fine, recent rule changes may have shifted by a tiny amount. Note the actual count.

If the wall caps out at 1500s without producing the harness line: report DONE_WITH_CONCERNS — the diagnosis can't proceed without the pair list. Try with `--release` build cached and a 30-min cap.

- [ ] **Step 3: Extract the pair list to a tracked file**

```bash
grep "^ MISSED:" /tmp/p2b0-galen-missed.log | sed 's/^ MISSED: //' > docs/phase2b-galen-missed-pairs.txt
wc -l docs/phase2b-galen-missed-pairs.txt
```

Expected: 109 lines (or the observed count from Step 2). Each line is `<sub-iri> ⊑ <sup-iri>`.

- [ ] **Step 4: Revert the harness bump**

```bash
git checkout crates/owl-dl-reasoner/tests/konclude_closure_diff.rs
```

Confirm with `git diff crates/owl-dl-reasoner/tests/konclude_closure_diff.rs` — should be empty.

- [ ] **Step 5: Commit the pair list**

```bash
git add docs/phase2b-galen-missed-pairs.txt
git commit -m "docs(phase2b.0): GALEN MISSED pair list (109 pairs) for diagnosis"
```

---

## Task 2: Stratified sample + IRI-prefix histogram

**Files:**
- Create: `docs/phase2b-galen-sample.md` (the sample selection + rationale; committed).

The 109 MISSED almost certainly cluster into a small number of distinct patterns. Stratify by IRI prefix (GALEN uses things like `#NAMEDPathologicalStructure`, `#<X>Pathology`, `#PairedBodyStructure`, etc.) to ensure the sample covers the diversity.

- [ ] **Step 1: Histogram the sub-class IRIs by local-name prefix**

```bash
# Extract the local name (after #) from each sub-iri, then strip
# trailing digits/numbers/word-fragments to get the "prefix shape."
# E.g. "#GastricPathology" → "Pathology"; "#PairedBodyStructure" → "PairedBodyStructure".
awk '{print $1}' docs/phase2b-galen-missed-pairs.txt \
    | sed 's|.*#||' \
    | sort | uniq -c | sort -rn | head -40
```

The output shows distinct sub-class IRIs with counts. Group visually: which IRI families dominate? Common GALEN patterns:
- `*Pathology` / `PathologicalProcess`
- `*BodyStructure` / `*PairedBodyStructure` / `MirrorImaged*`
- anatomical-region names

- [ ] **Step 2: Same for super-class IRIs**

```bash
awk '{print $3}' docs/phase2b-galen-missed-pairs.txt \
    | sed 's|.*#||' \
    | sort | uniq -c | sort -rn | head -40
```

Often the super-class is the more diagnostic side — the MISSED converge on a few "umbrella" classes like `PathologicalCondition`, `Anatomy`, etc.

- [ ] **Step 3: Cross-tab the top patterns**

```bash
sed 's|.*#\([^ ]*\) ⊑ .*#\([^ ]*\)$|\1 ⊑ \2|' docs/phase2b-galen-missed-pairs.txt \
    | sort | uniq -c | sort -rn | head -30
```

(That sed is intentionally simple: it extracts both local names; if the IRIs use `/` instead of `#`, adjust the separator.)

This collapses pairs by "local-name to local-name" so you see if (say) 47 of the 109 are `*Pathology ⊑ PathologicalCondition`.

- [ ] **Step 4: Sample 8 pairs spanning the visible patterns**

From the histograms, pick 8 pairs as a stratified sample:
- 3 from the LARGEST cluster (to confirm the pattern is consistent within it).
- 2-3 from the SECOND-largest cluster.
- 2-3 from smaller clusters (one each, to confirm they're genuinely different).

Avoid pairs where one side is `owl:Thing` or `owl:Nothing` (the harness already filters those, but spot-check the list).

- [ ] **Step 5: Write the sample doc**

Create `docs/phase2b-galen-sample.md` with EXACTLY this structure:

```markdown
# Phase 2b.0 — GALEN MISSED sample

Selected from the 109 MISSED pairs in `phase2b-galen-missed-pairs.txt`
(Phase 2a measurement, commit f61e06a). Stratified by local-name
prefix family to ensure coverage of distinct patterns.

## IRI histograms

### Sub-class local-name families (top 15)

<paste the Step 1 output, top 15 rows>

### Super-class local-name families (top 15)

<paste the Step 2 output, top 15 rows>

### Pair patterns (top 15)

<paste the Step 3 output, top 15 rows>

## Selected pairs

| # | Sub IRI | Sup IRI | Cluster |
|---|---|---|---|
| 1 | <full sub> | <full sup> | <which cluster — biggest / second / other> |
| 2 | ... | ... | ... |
| ... up to 8 ... |

## Rationale

<one paragraph: why these 8; which patterns they cover; what's
intentionally NOT covered (e.g., if there are 3 obvious clusters
and we only sample from 3, but a tail of singleton pairs is left
unsampled, say so).>
```

- [ ] **Step 6: Commit**

```bash
git add docs/phase2b-galen-sample.md
git commit -m "docs(phase2b.0): stratified GALEN MISSED sample (8 pairs, prefix-clustered)"
```

---

## Task 3: Per-pair minimal HermiT-verified repros

**Files:**
- Create: `crates/owl-dl-reasoner/tests/fixtures/phase2b/pair_<NN>.ofn` (one OWX/OFN module per sampled pair, 8 files).
- Create: `crates/owl-dl-reasoner/tests/fixtures/phase2b/pair_<NN>.hermit.owx` (HermiT's classification of each, 8 files).

For EACH of the 8 sampled pairs, repeat:

- [ ] **Step 1: Build the term-file for ROBOT extract**

```bash
mkdir -p crates/owl-dl-reasoner/tests/fixtures/phase2b
echo -e "<SUB-IRI>\n<SUP-IRI>" > /tmp/p2b0-terms-NN.txt
```

(Substitute `NN` with the pair number 01..08 and the IRIs from Task 2's sample table. The IRIs go bare on each line — no quotes, no angle brackets.)

- [ ] **Step 2: Extract a ⊥-locality module via ROBOT**

ROBOT needs the source as a file. Galen is already at `ontologies/external/galen.owx` (per the existing fixture inventory).

```bash
docker run --rm \
    -v "$PWD:/work" -w /work obolibrary/robot:v1.9.6 \
    robot extract \
        --input ontologies/external/galen.owx \
        --method bot \
        --term-file /tmp/p2b0-terms-NN.txt \
        --output crates/owl-dl-reasoner/tests/fixtures/phase2b/pair_NN.owx
```

Expected: the module is much smaller than GALEN — usually < 100 axioms instead of GALEN's thousands. `wc -l crates/owl-dl-reasoner/tests/fixtures/phase2b/pair_NN.owx` should be in the 50-500 range.

If the module is huge (thousands of lines), the pair has wide reachability — that's a real finding (record it in Task 4). Continue.

- [ ] **Step 3: Convert the module to OFN (for rustdl) AND classify it with HermiT**

```bash
docker run --rm \
    -v "$PWD:/work" -w /work obolibrary/robot:v1.9.6 \
    robot convert \
        --input crates/owl-dl-reasoner/tests/fixtures/phase2b/pair_NN.owx \
        --format ofn \
        --output crates/owl-dl-reasoner/tests/fixtures/phase2b/pair_NN.ofn
sed -i -E '/^[[:space:]]*Declaration\(Datatype\(/d' \
    crates/owl-dl-reasoner/tests/fixtures/phase2b/pair_NN.ofn

docker/robot/classify-oracle.sh \
    crates/owl-dl-reasoner/tests/fixtures/phase2b/pair_NN.ofn \
    crates/owl-dl-reasoner/tests/fixtures/phase2b/pair_NN.hermit.owx
```

- [ ] **Step 4: Verify HermiT derives the entailment on the minimal module**

Search the HermiT output for the pair:

```bash
# OWX uses multi-line elements; look for SubClassOf with both classes.
python3 -c "
import xml.etree.ElementTree as ET
NS = '{http://www.w3.org/2002/07/owl#}'
tree = ET.parse('crates/owl-dl-reasoner/tests/fixtures/phase2b/pair_NN.hermit.owx')
sub_iri = '<SUB-IRI>'
sup_iri = '<SUP-IRI>'
found = False
for sc in tree.iter(NS + 'SubClassOf'):
    classes = [c.get('IRI') for c in sc.findall(NS + 'Class')]
    if len(classes) == 2 and classes[0] == sub_iri and classes[1] == sup_iri:
        found = True
        break
# Also check EquivalentClasses since HermiT may emit those for sub⊑sup ∧ sup⊑sub.
for ec in tree.iter(NS + 'EquivalentClasses'):
    classes = [c.get('IRI') for c in ec.findall(NS + 'Class')]
    if sub_iri in classes and sup_iri in classes:
        found = True
        break
print('FOUND' if found else 'NOT FOUND')
"
```

Expected: `FOUND`. If `NOT FOUND`, the locality extract was too aggressive AND the entailment isn't recoverable from the module alone. Two diagnostic moves:
- Re-run Step 2 with `--method star` instead of `--method bot` (a less aggressive module containing more axioms).
- If still not found, the entailment may depend on a global property (e.g., a chain through many intermediate classes); broaden the term-file with 5-10 additional classes that appear in axioms about SUB or SUP. Add them and re-extract.

If after two broadenings HermiT still misses on the module, record that this pair's derivation is non-local — pin it as "needs full GALEN to derive" and move on; you have 7 other pairs to analyze.

- [ ] **Step 5: Confirm rustdl misses on the minimal module**

```bash
./target/release/rustdl subclass \
    --saturation-only \
    crates/owl-dl-reasoner/tests/fixtures/phase2b/pair_NN.ofn \
    <SUB-IRI> <SUP-IRI>
echo "---"
./target/release/rustdl subclass \
    crates/owl-dl-reasoner/tests/fixtures/phase2b/pair_NN.ofn \
    <SUB-IRI> <SUP-IRI>
```

(Use the `subclass` CLI subcommand if it accepts a single-pair query. If only `classify` is available, run that with `--pair-timeout-ms 200` and grep the output for the pair.)

Expected: BOTH say "not a subclass" / `false`. That confirms the miss is real on the minimal module AND it's in saturation (the dead-end-#12 frame-test pins the layer). If `--saturation-only` misses but default classify hits, the gap is in the saturator only — the wedge/tableau covers it; that's diagnostically useful too.

Record both verdicts in the per-pair notes (Task 4).

- [ ] **Step 6: Commit ALL 8 pairs' fixtures**

After all 8 pairs are extracted, classified, and verified:

```bash
git add crates/owl-dl-reasoner/tests/fixtures/phase2b/
git commit -m "fixture(phase2b.0): 8 minimal HermiT-verified GALEN MISSED repros"
```

(Note: `crates/owl-dl-reasoner/tests/fixtures/phase2b/` is NOT under `ontologies/` so it's not gitignored. Verify with `git check-ignore -v <one-file>` before committing.)

---

## Task 4: Per-pair axiom analysis + clustering

**Files:**
- Create: `docs/phase2b-galen-pair-analysis.md` (per-pair analysis, committed).

For each of the 8 minimal modules, manually walk the axioms to identify the derivation step HermiT uses that rustdl's saturator misses.

- [ ] **Step 1: For each pair, inventory the axioms involving SUB, SUP, and their roles**

For pair NN, read `crates/owl-dl-reasoner/tests/fixtures/phase2b/pair_NN.ofn` and extract:
- All `SubClassOf` axioms mentioning SUB or SUP.
- All `EquivalentClasses` mentioning SUB or SUP.
- All `SubObjectPropertyOf`, `FunctionalObjectProperty`, `ObjectPropertyDomain`, `ObjectPropertyRange`, `TransitiveObjectProperty`, etc. mentioning any role referenced in the above axioms.

```bash
# Pair NN: extract relevant axioms.
SUB=<sub-local-name>
SUP=<sup-local-name>
grep -E "SubClassOf|EquivalentClasses" crates/owl-dl-reasoner/tests/fixtures/phase2b/pair_NN.ofn \
    | grep -E "${SUB}|${SUP}" | head -20
echo "---"
# Roles referenced in those axioms (eyeballed from the output).
grep -E "FunctionalObjectProperty|SubObjectPropertyOf|ObjectPropertyDomain|ObjectPropertyRange|TransitiveObjectProperty" \
    crates/owl-dl-reasoner/tests/fixtures/phase2b/pair_NN.ofn | head -30
```

This gives the local axiom neighborhood.

- [ ] **Step 2: For each pair, reconstruct the missing derivation**

Write out, in human-readable form:
- The "given" axioms (what SUB/SUP are defined as).
- The "needed" entailment (`SUB ⊑ SUP`).
- The DERIVATION HermiT could plausibly use: walk forward from SUB's definition, applying EL/SROIQ rules, until you reach SUP. Identify the FIRST rule application rustdl's saturator can't do.

This is the analytical heart of the diagnosis. It requires reasoning about EL/SROIQ semantics. Use these references when stuck:
- `crates/owl-dl-saturation/src/lib.rs:8-49` (the list of supported saturator rules: told subsumption, conjunctive triggers, CR5, CR9, length-2 role chains + transitivity, domain/range, Tseitin compound bodies, Bot detection, EL++ functional-role witness-merge from Phase 2a).
- `docs/fragment-completeness.md` (provably-complete fragment).
- `docs/hypertableau-dead-ends.md` (what's been tried and what fails).

If the derivation needs a rule NOT in that list, that's a candidate Phase 2b rule. Common candidates:
- `≥n + disjointness`: pairwise-disjoint `∃R.B_i` ⇒ `≥n R.⊤`.
- `(∃R.A) ⊑ B` with A defined as something more general (chain through definition unfolding).
- `R ⊑ S, R functional, S functional` (inverse-functional-related patterns).
- `∀R.C` on the LHS of a SubClassOf (universal restriction in body — outside EL).
- Role inversions composed with chains.

Write the derivation step explicitly. For example: "HermiT applies the rule `(∃R.A ⊓ ∃R.B) ⊑ ∃R.(A ⊓ B)` when R is functional AND A ⊓ B is non-empty per a disjointness analysis — rustdl's saturator has the simpler version (without the disjointness side condition) which doesn't fire here because... ."

- [ ] **Step 3: Write the per-pair analysis doc**

Create `docs/phase2b-galen-pair-analysis.md` with one section per pair (8 sections):

```markdown
# Phase 2b.0 — Per-pair GALEN MISSED analysis

For each of the 8 sampled pairs (from `phase2b-galen-sample.md`), this
doc records: the relevant axiom neighborhood, the missing derivation
step, and the candidate Phase 2b rule shape.

## Pair 01: <sub-local-name> ⊑ <sup-local-name>

**Full IRIs:** `<sub-iri>` ⊑ `<sup-iri>`
**Cluster:** <which cluster from the sample doc>
**Module size:** N axioms (`crates/owl-dl-reasoner/tests/fixtures/phase2b/pair_01.ofn`)
**rustdl --saturation-only:** false (miss)
**rustdl classify default:** false (miss) / true (hit — wedge or tableau recovered)
**HermiT on module:** true (entailment confirmed)

### Relevant axioms

<paste 5-15 lines of the most relevant axioms from Step 1>

### Missing derivation step

<the analytical writeup from Step 2: what derivation HermiT uses,
what rule rustdl needs but doesn't have>

### Candidate Phase 2b rule shape

<one sentence: the rule we'd need to add>

---

## Pair 02: ...

[repeat for all 8 pairs]
```

- [ ] **Step 4: Commit**

```bash
git add docs/phase2b-galen-pair-analysis.md
git commit -m "docs(phase2b.0): per-pair GALEN MISSED axiom analysis (8 pairs)"
```

---

## Task 5: Cluster + recommendations + close-out doc

**Files:**
- Create: `docs/phase2b-galen-diagnosis.md` (the headline diagnosis doc, committed).
- Modify: `docs/superpowers/specs/2026-05-31-soundness-completeness-perf-design.md` — append a "Landed:" pointer under the 2b.0 bullet.

This is the synthesis step. The 8 per-pair analyses (from Task 4) should reveal 2-4 distinct missing-rule patterns. Identify them, count how many of the 109 MISSED likely fall into each, and recommend the Phase 2b rule(s).

- [ ] **Step 1: Cluster the 8 analyses by rule pattern**

From `docs/phase2b-galen-pair-analysis.md`, list each pair's "candidate Phase 2b rule shape" sentence. Group sentences that name the SAME rule (even if worded differently). Typical result: 2-4 distinct rules across 8 pairs.

For each cluster, estimate its share of the 109 MISSED by re-running the IRI histograms from Task 2 against the cluster's defining pattern. For example, if Cluster A is the `*Pathology ⊑ PathologicalCondition` shape, count how many MISSED pairs match that local-name pattern.

- [ ] **Step 2: Write the headline diagnosis doc**

Create `docs/phase2b-galen-diagnosis.md`:

```markdown
# Phase 2b.0 — GALEN MISSED diagnosis

Phase 2a's EL++ functional-role witness-merge rule recovered 0 of
GALEN's 109 MISSED (see `phase2a-results.md`). The handoff's
`PathologicalCondition` trace did not describe what's actually
missing. This doc replaces that trace with an empirical analysis
based on:

- `phase2b-galen-missed-pairs.txt` — full 109-pair list.
- `phase2b-galen-sample.md` — stratified sample of 8 pairs.
- `crates/owl-dl-reasoner/tests/fixtures/phase2b/pair_NN.{ofn,hermit.owx}` — minimal modules + HermiT verdicts.
- `phase2b-galen-pair-analysis.md` — per-pair derivation analysis.

## Cluster summary

| Cluster | Pattern | Pairs in sample | Estimated share of 109 | Candidate rule |
|---|---|---|---|---|
| A | <pattern description> | <count from sample> | <estimated count> | <rule shape> |
| B | ... | ... | ... | ... |
| ... | ... | ... | ... | ... |

## Recommended Phase 2b rule order

Based on (estimated share × implementation simplicity):

1. **Cluster X — rule Y.** <one paragraph: why this is the top
   pick, what rule shape it needs, what verify-before-build canary
   to build first, what existing infrastructure can be reused.>
2. **Cluster Z — rule W.** <same shape>
3. ... etc.

## Out of scope (residual gaps)

<pairs that didn't fit any cluster, or whose derivation appears
genuinely non-local (HermiT couldn't derive even on the minimal
module). These are the residual gaps that Phase 2b's measurable
target should NOT claim to address — same honesty discipline as
Phase 2a (which honestly recorded that GALEN MISSED was NOT
recovered).>

## Honesty paragraph

Phase 2b.0's diagnosis is grounded in 8 sampled pairs out of 109.
That's representative but not exhaustive. If Phase 2b's rule based
on the top cluster lands only X-Y MISSED reduction instead of the
estimated share, the diagnosis was wrong about cluster sizes (or
the rule has its own implementation gaps) — same dead-end #12 /
#14 lesson recursing. If that happens, the recovery is: extract
the new MISSED list, re-cluster, iterate.

## Cross-references

- Phase 2a's empirical disproof: `phase2a-results.md`
- Dead-end #14 (T4 synthetic-id design): `hypertableau-dead-ends.md`
- Design spec Phase 2: `docs/superpowers/specs/2026-05-31-soundness-completeness-perf-design.md`
- Phase 2b plan (next): to be written after this diagnosis lands.
```

- [ ] **Step 3: Cross-link from the design spec**

In `docs/superpowers/specs/2026-05-31-soundness-completeness-perf-design.md`, find the `2b.0 — Re-diagnose GALEN MISSED` sub-bullet (added by Phase 2a Task 7). Append:

```
Landed: `docs/phase2b-galen-diagnosis.md`. <One-sentence summary of
the headline finding: e.g. "Top cluster (X pairs) is ≥n + disjointness;
second cluster (Y pairs) is functional-role inversion. Phase 2b proper
implements the top cluster first.">
```

- [ ] **Step 4: Commit**

```bash
git add docs/phase2b-galen-diagnosis.md \
        docs/superpowers/specs/2026-05-31-soundness-completeness-perf-design.md
git commit -m "docs(phase2b.0): GALEN MISSED diagnosis — cluster analysis + rule recommendations"
```

---

## Definition of done (Phase 2b.0)

- `docs/phase2b-galen-missed-pairs.txt` lists all 109 GALEN MISSED pairs (Task 1).
- `docs/phase2b-galen-sample.md` documents the prefix histograms + 8-pair stratified sample (Task 2).
- `crates/owl-dl-reasoner/tests/fixtures/phase2b/pair_NN.{ofn,hermit.owx}` exist for each of 8 pairs; HermiT confirms entailment on the minimal module; rustdl confirmed to miss (Task 3).
- `docs/phase2b-galen-pair-analysis.md` has the 8 per-pair derivation analyses (Task 4).
- `docs/phase2b-galen-diagnosis.md` clusters the patterns + recommends Phase 2b rule order (Task 5).
- Design spec 2b.0 bullet cross-links the diagnosis doc with a one-sentence headline (Task 5 Step 3).

The deliverable is `phase2b-galen-diagnosis.md`'s recommendations. Phase 2b proper (the implementation plan for the top-ranked rule) is the next plan, written AFTER this diagnosis lands.

## What this plan does NOT do (explicitly)

- Does NOT implement any new rule. That's Phase 2b proper.
- Does NOT optimize the existing rules. That's Phase 3.
- Does NOT claim to find ALL 109 MISSED patterns — 8 sampled pairs out of 109 is representative, not exhaustive.
- Does NOT touch saturator or reasoner code. Only the harness print limit (temporarily, reverted before commit) and the docs/fixtures.
