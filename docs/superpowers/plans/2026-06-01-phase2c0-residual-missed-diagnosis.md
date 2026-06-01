# Phase 2c.0 — Residual MISSED Diagnosis Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Confirm what calculus pattern(s) the 17 GALEN + 27 notgalen residual MISSED actually need post-Phase-2b/3, and write the Phase 2c implementation recommendation — either reaffirming Phase 2b.0's EL+ Option 3 (functional-role + covering / sibling-collapse) or refining it if the residual shape has shifted.

**Architecture:** This is forensic, not implementation. Tight scope — much of Phase 2b.0's infrastructure (pair_06 + pair_07 minimal modules, the per-pair analysis doc, the cluster mapping) is reusable. Output is `docs/phase2c-galen-diagnosis.md` (the recommendation). Steps: extract the current 17 GALEN MISSED + 27 notgalen MISSED list; histogram by super-class to spot post-Phase-2b cluster shifts (e.g. the `AbnormalBodyStructure` pairs visible in the Phase 3c log weren't in Phase 2b.0's original 5-cluster map); sample 2-3 representative pairs (re-using pair_06 + pair_07 where applicable); if any new shape, build 1-2 fresh minimal modules; analyze; write the Phase 2c rule-shape recommendation.

**Tech Stack:** No code changes. Bash/grep/python + ROBOT extract (Phase 0 oracle for any new minimal module), reusing existing Phase 2b.0 fixtures + tooling.

---

## Background the executor needs

- Phase 2b.0 (commits e871e13..dbf1782) diagnosed the original 109 GALEN MISSED across 5 clusters (A=paired anatomy, B=hollow, C=pathological process, D=digestive pathology, E=joint stability) + a 25-pair F tail. The 8-pair stratified sample is in `docs/phase2b-galen-sample.md`; the per-pair analysis is in `docs/phase2b-galen-pair-analysis.md` (pairs 06+07 are the cluster C/D representatives).
- Phase 2b + 2b.5 (compound existential-body lowering) recovered 92 of 109 → 17 GALEN MISSED. The 17 are presumed to be cluster C/D (~24 pairs in Phase 2b.0's estimate) plus possibly some F tail.
- The Phase 3 arc (a/b/c, perf only — NO completeness change) didn't touch MISSED counts. GALEN MISSED stayed at 17 throughout.
- The Phase 3c GALEN log (`/tmp/p3c-galen.log` if present, otherwise re-derivable) captured the current 17 MISSED pairs. Sample lines (from the recent Phase 3c work):
  - `CardiacInsufficiencyDueToProsthesis ⊑ IntrinsicallyPathologicalBodyProcess`
  - `Cheyne-StokesRepiration ⊑ IntrinsicallyPathologicalBodyProcess`
  - `CongestiveCardiacFailure ⊑ IntrinsicallyPathologicalBodyProcess` (= pair_06 in Phase 2b.0 sample)
  - `Diverticulum ⊑ AbnormalBodyStructure` ← NEW super-class not in Phase 2b.0's 5-cluster map.
- The 27 notgalen MISSED have not been characterized by per-pair analysis (Phase 2b.0 was GALEN-only). This diagnosis includes a brief notgalen pattern check.
- Phase 2b.0's per-pair analysis for pair 06 (`docs/phase2b-galen-pair-analysis.md`) lists THREE candidate Phase 2c implementations:
  1. Full functional-role inference + negation/case-splitting (hypertableau extension).
  2. Disjointness propagation through the merged witness (extension of Phase 2a's atom-set merge).
  3. **An EL+ approximation:** materialise `∃hasIntrinsicPathologicalStatus.pathological` directly when the relevant GCI's antecedent fires AND the `physiological` alternative is provably-disjoint via the `PathologicalOrPhysiologicalStatus` covering. Saturator-side; saturator-amenable.
- Existing fixtures on disk (reusable for this diagnosis):
  - `crates/owl-dl-reasoner/tests/fixtures/phase2b/pair_06.{ofn,owx,hermit.owx}` — `CongestiveCardiacFailure ⊑ IntrinsicallyPathologicalBodyProcess`.
  - `crates/owl-dl-reasoner/tests/fixtures/phase2b/pair_07.{ofn,owx,hermit.owx}` — `AcuteGastricUlcer ⊑ DigestiveSystemPathology`.

---

## Task 1: Extract the current 17 GALEN MISSED + 27 notgalen MISSED list

**Files:**
- Create: `docs/phase2c-galen-missed-pairs.txt` (the 17-pair list, committed).
- Create: `docs/phase2c-notgalen-missed-pairs.txt` (the 27-pair list, committed).

The Phase 3c GALEN log (`/tmp/p3c-galen.log`) may still have the full MISSED block. If not, re-run the harness (the diff bumps the print limit was reverted in Phase 2b.0 T1 — we need to re-bump temporarily).

- [ ] **Step 1: Check for the Phase 3c log**

```bash
ls -la /tmp/p3c-galen.log 2>&1
grep -c "MISSED:" /tmp/p3c-galen.log 2>&1
```

If the file exists AND has ≥17 MISSED lines: skip Step 2 and proceed to Step 3.
If the file is missing OR has <17 MISSED lines (the harness limit caps at 50, so 17 should fit, but the limit may be back to 50 post-Phase-2b.0): go to Step 2.

- [ ] **Step 2: Re-extract via harness re-bump (if needed)**

Temporarily bump the harness limit at `crates/owl-dl-reasoner/tests/konclude_closure_diff.rs:262` (the same site Phase 2b.0 T1 bumped):

```bash
sed -i 's|let missed_limit = if missed.len() <= 50 { missed.len() } else { 50 };|let missed_limit = missed.len();  // P2c.0 diagnosis: print all MISSED|' crates/owl-dl-reasoner/tests/konclude_closure_diff.rs
```

Run GALEN + notgalen:

```bash
timeout 2400 cargo test -p owl-dl-reasoner --test konclude_closure_diff --release \
    galen_closure_matches_konclude notgalen_closure_matches_konclude \
    -- --ignored --nocapture 2>&1 | tee /tmp/p2c0-residual.log
```

REVERT the harness change:
```bash
git checkout crates/owl-dl-reasoner/tests/konclude_closure_diff.rs
git diff crates/owl-dl-reasoner/tests/konclude_closure_diff.rs
```
Expected: empty (file restored).

- [ ] **Step 3: Extract the GALEN pair list**

```bash
# From whichever log has the MISSED lines:
LOG="/tmp/p3c-galen.log"  # or /tmp/p2c0-residual.log if Step 2 ran
awk '/--- galen/,/--- notgalen|^test result|^running/' "$LOG" | grep "^ MISSED:" | sed 's/^ MISSED: //' > docs/phase2c-galen-missed-pairs.txt
wc -l docs/phase2c-galen-missed-pairs.txt
```
Expected: 17 lines.

If the awk pattern doesn't isolate GALEN's section (different log shape), simpler fallback: extract every MISSED line and verify by class IRI patterns:
```bash
grep "^ MISSED:" "$LOG" | sed 's/^ MISSED: //' | head -50 > /tmp/p2c0-all-missed.txt
wc -l /tmp/p2c0-all-missed.txt
```
Then manually split into galen vs notgalen by IRI namespace if both share `factkb#`.

- [ ] **Step 4: Extract the notgalen pair list (if Step 2 captured it)**

```bash
awk '/--- notgalen/,/^test result|^running/' "$LOG" | grep "^ MISSED:" | sed 's/^ MISSED: //' > docs/phase2c-notgalen-missed-pairs.txt
wc -l docs/phase2c-notgalen-missed-pairs.txt
```
Expected: 27 lines.

If only the GALEN log is available, defer notgalen to Phase 2c proper (record as "not separately measured this diagnosis"). The cluster C/D recommendation likely applies to notgalen too per Phase 2b.0's diagnosis.

- [ ] **Step 5: Commit the pair lists**

```bash
git add docs/phase2c-galen-missed-pairs.txt docs/phase2c-notgalen-missed-pairs.txt
git commit -m "docs(phase2c.0): GALEN (17) + notgalen (27) residual MISSED pair lists"
```

(If notgalen wasn't captured, `git add` only the GALEN file and note that in the commit message body.)

---

## Task 2: Histogram + cluster comparison

**Files:**
- Create: `docs/phase2c-cluster-shift.md` (the comparison doc, committed).

The Phase 2b.0 5-cluster map (A=paired, B=hollow, C=pathological-process, D=digestive-pathology, E=joint-stability) characterised 109 pairs. The 17 residual should mostly be C+D (the un-recovered Phase 2b.0 estimate ~24 pairs), but the recent Phase 3c log surfaced `AbnormalBodyStructure` which wasn't in the original cluster map. This task confirms the residual shape.

- [ ] **Step 1: Super-class histogram on GALEN residual**

```bash
awk '{print $3}' docs/phase2c-galen-missed-pairs.txt | sed 's|.*#||' | sort | uniq -c | sort -rn
```

Capture. Expected: a small set of distinct super-classes (the 17 residual pairs likely converge on 2-3 super-classes).

- [ ] **Step 2: Compare to Phase 2b.0's super-class histogram**

The Phase 2b.0 super-class top values were `MirrorImagedBodyStructure` 20, `ExactlyPairedBodyStructure` 20, `IntrinsicallyPathologicalBodyProcess` 12, `DigestiveSystemPathology` ~12 (per `docs/phase2b-galen-sample.md`'s "Visible clusters" section).

The Phase 2b/2b.5 fix should have recovered cluster A (paired/mirror — ~40 pairs) and cluster B (hollow — 15) and cluster E (joint stability — 5) — these were the compound-existential-body shape. Cluster C (12) + D (12) — functional-role + covering — should still be MISSED. Plus the ~25 F tail.

Expected from Step 1: top super-classes are now `IntrinsicallyPathologicalBodyProcess` + `DigestiveSystemPathology` + possibly `AbnormalBodyStructure` (the new one).

- [ ] **Step 3: Check the notgalen histogram (if captured)**

```bash
awk '{print $3}' docs/phase2c-notgalen-missed-pairs.txt | sed 's|.*#||' | sort | uniq -c | sort -rn
```

Phase 2b.0 didn't analyse notgalen. The Phase 3c log mentioned `IntrinsicallyPathologicalBodyProcess` and `AbnormalBodyStructure` super-classes — same shape as GALEN's residual, suggesting the same cluster C/D pattern.

- [ ] **Step 4: Write `docs/phase2c-cluster-shift.md`**

```markdown
# Phase 2c.0 — cluster shift from Phase 2b.0 to post-Phase-3 residual

Compares the original Phase 2b.0 5-cluster characterization
(`docs/phase2b-galen-sample.md`) against the current 17 GALEN + 27
notgalen residual MISSED (per `docs/phase2c-galen-missed-pairs.txt` +
`docs/phase2c-notgalen-missed-pairs.txt`).

## GALEN residual super-class histogram (top 5)

<paste Step 1 output>

## Phase 2b.0 super-class histogram (recall)

| Super-class | Phase 2b.0 (of 109) | Phase 2b.0 cluster | Post-2b/2b.5 status |
|---|---|---|---|
| MirrorImagedBodyStructure | 20 | A (paired anatomy) | recovered |
| ExactlyPairedBodyStructure | 20 | A | recovered |
| IntrinsicallyPathologicalBodyProcess | 12 | C (pathological process) | <residual count from Step 1> |
| DigestiveSystemPathology | 12 | D (digestive pathology) | <residual count from Step 1> |
| JointStability | 5 | E (joint stability) | recovered |
| HollowStructure / ActuallyHollowBodyStructure (cluster B) | 15 | B (hollow) | recovered |

## New super-class appearance

<if `AbnormalBodyStructure` shows up in the GALEN residual but
NOT in Phase 2b.0's top histogram, note it here. Could be:
1. A pair that was in the unsampled F tail in Phase 2b.0.
2. A pair newly visible because Phase 2b.0 only printed up to 50
   MISSED — the F tail's distribution wasn't fully characterised.
3. A pair newly created by the Phase 2b/2b.5 fix (unlikely; the
   fix preserved verdicts).
Investigate which by counting `AbnormalBodyStructure` occurrences in
the original 109-pair list (`docs/phase2b-galen-missed-pairs.txt`):
```
grep -c "AbnormalBodyStructure" docs/phase2b-galen-missed-pairs.txt
```
If the count is > 0, it was in the F tail. Document.>

## notgalen residual histogram (top 5)

<paste Step 3 output, or "not captured this diagnosis">

## Cluster mapping for Phase 2c

| Residual cluster | Pairs (GALEN + notgalen) | Phase 2b.0 origin | Phase 2c rule shape |
|---|---|---|---|
| C — IntrinsicallyPathologicalBodyProcess | <n_galen> + <n_notgalen> | original C | functional-role + covering / sibling-collapse |
| D — DigestiveSystemPathology | <n_galen> + <n_notgalen> | original D | same as C |
| <new — AbnormalBodyStructure?> | <n> | F tail or new | TBD per per-pair analysis |
| F tail (remaining) | <n> | F tail | TBD |
```

- [ ] **Step 5: Commit**

```bash
git add docs/phase2c-cluster-shift.md
git commit -m "docs(phase2c.0): residual MISSED cluster shift analysis"
```

---

## Task 3: Per-pair analysis for any new shape

**Files:**
- Possibly: `crates/owl-dl-reasoner/tests/fixtures/phase2c/pair_<NN>.{ofn,owx,hermit.owx}` (only if Task 2 found a new shape not in Phase 2b.0).

If Task 2 confirmed the residual is cluster C+D (matches Phase 2b.0's existing pair_06 + pair_07 analysis), SKIP this task — we have enough data already. Go to Task 4.

If Task 2 found a NEW shape (e.g. `AbnormalBodyStructure` is genuinely distinct from C+D), build one fresh minimal module via the Phase 2b.0 oracle pipeline.

- [ ] **Step 1: Determine whether new shapes exist**

Re-read Task 2's `docs/phase2c-cluster-shift.md`. If the "Cluster mapping for Phase 2c" table has a NEW row beyond cluster C+D+F (e.g. "AbnormalBodyStructure" as a distinct fourth cluster), proceed. Otherwise SKIP to Task 4.

- [ ] **Step 2: Pick one representative pair for the new shape**

From `docs/phase2c-galen-missed-pairs.txt`, pick a pair whose super-class is the new shape. Record the full IRIs.

- [ ] **Step 3: Build the minimal HermiT-verified module (reuse Phase 2b.0 oracle pipeline)**

```bash
mkdir -p crates/owl-dl-reasoner/tests/fixtures/phase2c
SUB="<full sub iri>"
SUP="<full sup iri>"
printf "%s\n%s\n" "$SUB" "$SUP" > /tmp/p2c0-terms-01.txt

docker run --rm \
    -v "$PWD:/work" -w /work obolibrary/robot:v1.9.6 \
    robot extract \
        --input ontologies/external/galen.owx \
        --method bot \
        --term-file /tmp/p2c0-terms-01.txt \
        --output crates/owl-dl-reasoner/tests/fixtures/phase2c/pair_01.owx

docker run --rm \
    -v "$PWD:/work" -w /work obolibrary/robot:v1.9.6 \
    robot convert \
        --input crates/owl-dl-reasoner/tests/fixtures/phase2c/pair_01.owx \
        --format ofn \
        --output crates/owl-dl-reasoner/tests/fixtures/phase2c/pair_01.ofn
sed -i -E '/^[[:space:]]*Declaration\(Datatype\(/d' \
    crates/owl-dl-reasoner/tests/fixtures/phase2c/pair_01.ofn

docker/robot/classify-oracle.sh \
    crates/owl-dl-reasoner/tests/fixtures/phase2c/pair_01.ofn \
    crates/owl-dl-reasoner/tests/fixtures/phase2c/pair_01.hermit.owx
```

- [ ] **Step 4: Verify HermiT derives the entailment**

```bash
python3 <<EOF
import xml.etree.ElementTree as ET
NS = '{http://www.w3.org/2002/07/owl#}'
tree = ET.parse('crates/owl-dl-reasoner/tests/fixtures/phase2c/pair_01.hermit.owx')
sub = "$SUB"
sup = "$SUP"
# BFS over transitive SubClassOf edges (HermiT may not emit the direct edge).
from collections import defaultdict
edges = defaultdict(set)
for sc in tree.iter(NS + 'SubClassOf'):
    classes = [c.get('IRI') for c in sc.findall(NS + 'Class')]
    if len(classes) == 2:
        edges[classes[0]].add(classes[1])
seen, queue = {sub}, [sub]
while queue:
    cur = queue.pop(0)
    if cur == sup:
        print("FOUND"); break
    for nxt in edges.get(cur, ()):
        if nxt not in seen:
            seen.add(nxt); queue.append(nxt)
else:
    print("NOT FOUND")
EOF
```

Expected: `FOUND`. If `NOT FOUND` even with star-locality fallback, this pair is genuinely non-local — note in Task 4, skip.

- [ ] **Step 5: Walk the relevant axioms**

```bash
SUB_LOCAL=$(echo "$SUB" | sed 's|.*#||')
SUP_LOCAL=$(echo "$SUP" | sed 's|.*#||')
grep -E "Class.*${SUB_LOCAL}\b|Class.*${SUP_LOCAL}\b" \
    crates/owl-dl-reasoner/tests/fixtures/phase2c/pair_01.ofn | head -10
echo "---"
grep -E "EquivalentClasses|SubClassOf" crates/owl-dl-reasoner/tests/fixtures/phase2c/pair_01.ofn \
    | grep -E "#${SUB_LOCAL}\b|#${SUP_LOCAL}\b" | head -20
```

Identify the SUB and SUP definitions. Compare to Phase 2b.0's pair-06 analysis structure — is the derivation the SAME (functional-role + covering / sibling-collapse) or a DIFFERENT pattern?

- [ ] **Step 6: Commit the new fixture**

```bash
git add crates/owl-dl-reasoner/tests/fixtures/phase2c/
git commit -m "fixture(phase2c.0): minimal module for <new-cluster> sample pair"
```

---

## Task 4: Phase 2c diagnosis + rule recommendation

**Files:**
- Create: `docs/phase2c-galen-diagnosis.md` (the headline diagnosis, committed).
- Modify: `docs/superpowers/specs/2026-05-31-soundness-completeness-perf-design.md` (Phase 2 section close-out continuation).

- [ ] **Step 1: Synthesize Tasks 1-3 into the diagnosis doc**

Create `docs/phase2c-galen-diagnosis.md`:

```markdown
# Phase 2c.0 — Residual MISSED diagnosis + Phase 2c rule recommendation

Post-Phase-2b/2b.5/3 state: GALEN 17 MISSED + notgalen 27 MISSED = 44
residual pairs unrecovered. Phase 2b.0's diagnosis pointed at cluster
C+D (functional-role + covering / sibling-collapse) with three
candidate implementations. This Phase 2c.0 diagnosis confirms the
residual shape and picks the Phase 2c implementation target.

## Residual MISSED inventory

- GALEN: 17 pairs (`docs/phase2c-galen-missed-pairs.txt`).
- notgalen: 27 pairs (`docs/phase2c-notgalen-missed-pairs.txt`).
- Total: 44.

## Cluster characterization

Per `docs/phase2c-cluster-shift.md`:

<summarize the cluster table from Task 2>

## Rule recommendation

Phase 2b.0's pair-06 analysis (`docs/phase2b-galen-pair-analysis.md`)
listed THREE candidate Phase 2c implementations:

1. **Full functional-role inference + negation/case-splitting**
   (hypertableau extension). Most general but most invasive — requires
   hypertableau-wedge engine changes outside the saturator.
2. **Disjointness propagation through merged witness** (extension of
   Phase 2a's atom-set merge). Possible but requires the witness
   to participate in classical-disjointness reasoning the saturator
   doesn't currently do.
3. **EL+ approximation:** materialise
   `∃hasIntrinsicPathologicalStatus.pathological` directly when the
   relevant GCI's antecedent fires AND the `physiological` alternative
   is provably-disjoint via the `PathologicalOrPhysiologicalStatus`
   covering. Saturator-side; pattern-matching on the specific
   axiom shape; no negation needed.

**Recommended for Phase 2c: Option 3** (EL+ approximation).

Rationale:
- Option 1's hypertableau changes are months of work; deferred.
- Option 2's full classical-disjointness is a calculus extension
  to the saturator that may have its own perf cost.
- Option 3 is pattern-matching: recognize the specific
  `∃R_i.X ⊑ … ⊓ ∃R_f.Y` triangle where R_i ⊑ R_f, R_f functional,
  X ⊑ Z, Y ⊑ Z, AND Z has a covering (e.g.
  `Z ⊑ X ⊔ Y` or equivalent covering axiom). Lower the triangle
  to the entailment `∃R_i.X ⊑ ∃R_f.Y` directly.

The pattern is identifiable from absorbed-TBox shape alone (no
runtime witness merging needed); the lowering is mechanical given
the matched triangle.

## Phase 2c scope estimate

If Phase 2c lands Option 3:
- Estimated coverage: 24-44 pairs (all of cluster C+D, plus possibly
  some F tail depending on shape match).
- Phase 2b's per-pair analysis confirmed pair 06 + pair 07 match
  the triangle pattern; the other 15 GALEN + 27 notgalen pairs are
  empirically inferred to share the shape (untested at per-pair level
  but the histograms in Task 2 support it).
- Honesty: if Option 3 lands and recovers far fewer than 44, the
  residual is a different shape requiring Phase 2d (Option 1 or 2).

## Phase 2c implementation outline (for the Phase 2c plan)

- T1: build a synthetic canary mirroring pair-06's structure
  (`∃R_i.X` with R_i ⊑ functional R_f, plus covering on the
  R_f-target's range). Verify HermiT derives; verify rustdl misses.
- T2: pattern detection in absorption — find the triangle in
  the absorbed-TBox shape.
- T3: TDD canary for the rule firing (structural counter).
- T4: implement the lowering (emit the materialised existential).
- T5: corpus measurement on GALEN + notgalen + Phase 0 net.
- T6: results doc + Phase 2 close-out update.

## Cross-references

- Phase 2b.0 diagnosis: `phase2b-galen-diagnosis.md`
- Phase 2b.0 per-pair analysis: `phase2b-galen-pair-analysis.md` (pairs 06, 07 are the canonical cluster C/D representatives).
- Phase 2 close-out: `phase2-closeout.md`
- Design spec: `docs/superpowers/specs/2026-05-31-soundness-completeness-perf-design.md` §"Phase 2".
```

- [ ] **Step 2: Update the design spec Phase 2c.0 bullet**

In `docs/superpowers/specs/2026-05-31-soundness-completeness-perf-design.md`, find the Phase 2 section. After the existing Phase 2 close-out paragraph (added during Phase 2 close-out commit 7ae8be9), append:

```
**Phase 2c.0 landed (2026-06-01):** `docs/phase2c-galen-diagnosis.md`
+ `docs/phase2c-cluster-shift.md`. Confirmed the 17 GALEN + 27 notgalen
residual MISSED are predominantly cluster C+D (functional-role +
covering / sibling-collapse) per Phase 2b.0's analysis, with `<additional
shape if found>` as a possible new shape. Phase 2c proper targets Option
3 (EL+ approximation) per the diagnosis — pattern-matching the
`∃R_i.X ⊑ ∃R_f.Y` triangle under R_i ⊑ R_f functional + covering, no
calculus extension needed. Estimated coverage: 24-44 of 44.
```

- [ ] **Step 3: Commit**

```bash
git add docs/phase2c-galen-diagnosis.md \
        docs/superpowers/specs/2026-05-31-soundness-completeness-perf-design.md
git commit -m "docs(phase2c.0): residual MISSED diagnosis + Phase 2c rule recommendation

17 GALEN + 27 notgalen residual MISSED confirmed predominantly
cluster C+D (functional-role + covering / sibling-collapse) per
Phase 2b.0 analysis. Phase 2c proper targets Option 3 (EL+
approximation, pattern-matching the triangle in absorbed-TBox).
Estimated coverage: 24-44 of 44 residual pairs."
```

---

## Definition of done (Phase 2c.0)

- `docs/phase2c-galen-missed-pairs.txt` lists the 17 GALEN MISSED.
- `docs/phase2c-notgalen-missed-pairs.txt` lists the 27 notgalen MISSED (or notes "not captured" if Task 1 deferred).
- `docs/phase2c-cluster-shift.md` characterizes the residual cluster shape vs Phase 2b.0's original 5-cluster map.
- `docs/phase2c-galen-diagnosis.md` recommends Phase 2c's implementation target (Option 3, EL+ approximation).
- Design spec's Phase 2 section has the Phase 2c.0 close-out note.
- Any new fixtures (Task 3) committed under `crates/owl-dl-reasoner/tests/fixtures/phase2c/`.

This unblocks Phase 2c proper (the EL+ approximation implementation), which is the next plan.

## What this plan does NOT do

- Does NOT implement any new rule — that's Phase 2c.
- Does NOT touch saturator or reasoner code (except possibly the temporary harness limit bump in Task 1, reverted before commit).
- Does NOT promise the 44-pair recovery — the diagnosis estimates 24-44 depending on shape variation; Phase 2c's empirical measurement will settle the actual number.
- Does NOT extend to clusters not visible in the histograms (e.g. if the residual is partly an F-tail shape, that becomes a future Phase 2d).
