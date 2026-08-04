# Peer-triage of rustdl's v0.4.14 DNF tail (151 ontologies)

**Date:** 2026-08-04 · **rustdl:** v0.4.14, pinned `bin/rustdl-v0414-986bc0f`
(sha256 `c513d3ec…`) · **Host:** 32-core / 251 GB · **Cap:** 120 s, single-thread, both
sides · **Raw data:** `owl-reasoner-harness`, `runs/triage-2026-08-04/` and
`baselines/2026-08-04-*`; retained peer hierarchies under
`/mnt/um-share-drive/dumontier/rustdl-triage-scratch-2026-08-04/`

This is the re-measure that `docs/benchmarks/2026-08-01-dnf257-characterization.md` has
listed as **blocking** its follow-on clustering since 2026-08-01. It replaces that
document's 242/15 partition, which was taken on v0.4.6 over a 257-ontology tail.

> **Headline: 138 of the remaining 151 (91.4%) are classified by a peer — 94 of them in
> under 10 s and 41 in under 1 s, median 4.42 s. The tail is still overwhelmingly a
> demonstrated algorithmic gap, not intrinsic hardness.** Set B — no peer classifies it
> either — is **13 ontologies, 8.6%**, and it is the *same* 13 that all three peers failed
> on 2026-08-01.
>
> Set A is ranked in `baselines/2026-08-04-setA-138-ranked.txt`; Set B is
> `baselines/2026-08-04-setB-13-list.txt`.

---

## 1. What was measured

The 151 are rustdl v0.4.14's genuine DNFs at a 120 s single-thread cap, from a re-run of
the 199-ontology survivor list (48 now complete, median 17.6 s, max 91.6 s). The list is
`baselines/2026-08-04-tail-v0414-list.txt`; it reproduces exactly from the remeasure JSONL
(151 `dnf` records, set-equal to the list).

**The 151 is a strict subset of the 2026-08-01 257 — zero new entrants.** So this is the
identical population, re-measured, and every ontology carries a prior verdict from an
independent peer run. That makes the comparison in §4 exact rather than approximate, and
supplies a reproducibility control (§6) that the original triage could not have.

Each ontology was given to the peers at the same 120 s cap, single-thread, using the
existing harness: `scripts/triage-leg-tail151.sh` (adapted from `triage-leg.sh`) with
outcome derived by `scripts/triage.py` from **output content, never exit code** — Konclude
exits 0 on a missing file and on junk while writing an ~896-byte Thing/Nothing hierarchy.
The partition is `scripts/partition.py`, unchanged, which is the authority on the A/B/C
definitions.

| set | definition | consequence |
|---|---|---|
| **A — gap** | ≥1 peer CLASSIFIED | algorithmic gap in rustdl; the peer's wall is the target |
| **B — intrinsic** | no peer CLASSIFIED | intrinsic for this generation of reasoners; record and stop |
| front-end | a peer returned EMPTY / NO_OUTPUT | reported **separately**, never folded into A or B |

---

## 2. The partition

| peer | CLASSIFIED | DNF | NO_OUTPUT | EMPTY |
|---|---|---|---|---|
| Konclude v0.7.0-1138 (native) | **138** | 12 | 0 | 1 |
| HermiT 1.4.3 (ROBOT/docker, ~0.56 s JVM floor) | 63 | 83 | 5 | 0 |
| KM `c6ced84` (20 GB cap) | 38 | 110 | 3 | 0 |

> **Set A = 138 (91.4%). Set B = 13 (8.6%). Set C (peers disagree) = 12** (orthogonal
> flag, size-only lower bound).
> **≥1 peer front-end failure: 9 ontologies** — its own category, per §1.

**Both other peers solve a strict subset of Konclude's set** — HermiT 63 ⊂ 138 and KM
38 ⊂ 138, verified as set containments, with **union = Konclude's 138 exactly** and **zero
peer-only solves** by either. So one peer determines the whole partition, and adding two
further independent reasoners moved it not at all. That is a much stronger statement than
"94% are tractable": the three reasoners are nested on this population, not complementary.

**Set B, all 13:** `ore_ont_11196 ore_ont_1123 ore_ont_11553 ore_ont_1194 ore_ont_12451
ore_ont_15344 ore_ont_15695 ore_ont_20 ore_ont_3215 ore_ont_4669 ore_ont_5548
ore_ont_7646 ore_ont_8475`. Every one of these was **also** unsolved by all three peers on
2026-08-01. Three independent reasoners failing the same 13 twice, in independent runs, is
the strongest evidence available at this scale that Set B is genuinely hard.

**Front-end failures (9), not counted as peer successes:** `ore_ont_12451` (Konclude EMPTY —
the 896-byte degenerate signature, on a valid 1.37 MB functional-syntax file; this is a
genuine Konclude front-end failure and is *also* the only Set B member with a non-DNF
cause); `ore_ont_10949`, `ore_ont_16372`, `ore_ont_20`, `ore_ont_4141`, `ore_ont_8445`
(HermiT NO_OUTPUT); `ore_ont_15687`, `ore_ont_2738`, `ore_ont_2874` (KM NO_OUTPUT, the
signature of its mandatory 20 GB allocation cap). Seven of these nine are nonetheless in
Set A because Konclude classified them — which is exactly why this category is reported
separately rather than folded into B.

---

## 3. Wall distribution on Set A

CLASSIFIED members only. "ratio" is 120 s (rustdl's cap, which it did not finish within)
over the peer wall — a lower bound on the gap, since rustdl's true wall is unknown and
larger.

| peer | n | median | p90 | max | < 10 s | < 1 s |
|---|---|---|---|---|---|---|
| Konclude | 138 | **5.11 s** | 28.56 s | 92.47 s | **94** | 39 |
| HermiT | 63 | 25.42 s | 86.32 s | 110.99 s | 22 | 0 |
| KM | 38 | 33.52 s | 93.29 s | 114.13 s | 16 | 11 |
| **fastest-of-any** | **138** | **4.42 s** | **28.56 s** | **92.47 s** | **94** | **41** |

Konclude also finishes **125 of 138 under 30 s**. Its closures on Set A are substantial —
**median 83,281 pairs, max 2,155,032** — so this is real classification work, not empty
output (see §7 for the eight small-closure members, each individually accounted for).

**The headline number:** of rustdl's 151 remaining unclassifiable ontologies, **138 (91.4%)
are a demonstrated algorithmic gap** — a peer does them at the same cap on the same host —
with a **median fastest-peer wall of 4.42 s**, **94 under 10 s**, and **41 under one
second** (a ≥120× gap against rustdl's cap). Only **13 (8.6%)** are plausibly intrinsic.

---

## 4. Comparison to the 2026-08-01 partition

| | 2026-08-01 (v0.4.6) | 2026-08-04 (v0.4.14) |
|---|---|---|
| tail size | 257 | **151** |
| Set A (≥1 peer classifies) | 242 (**94.2%**) | 138 (**91.4%**) |
| Set B (no peer classifies) | 15 (5.8%) | 13 (8.6%) |
| median fastest-peer wall on Set A | 3.47 s | 5.11 s |

**The peer-solvable fraction went slightly DOWN (94.2% → 91.4%), and Set B's share rose
from 5.8% to 8.6%. rustdl has been harvesting the ontologies that were easier for peers
too — but only mildly.** The direct evidence:

- Of the **106** ontologies rustdl recovered, **104 (98.1%)** were Set A on 2026-08-01 and
  only 2 were Set B. The recoveries came almost entirely out of Set A, which is exactly
  what shrinks Set A's *share* while leaving its absolute dominance intact.
- On the 2026-08-01 measurement, Konclude's median wall was **3.08 s on the ontologies
  rustdl went on to recover** versus **5.06 s on those that survived** — the survivors were
  already the harder half by the peer's own yardstick. HermiT shows the same ordering
  (12.91 s recovered vs 19.84 s surviving).

**Interpretation.** The enrichment toward Set B is real but small: an 8.6% intrinsic share
after removing 106 ontologies means the residual tail is *still* ~91% demonstrably
tractable by a peer. This does **not** support a return to the "intrinsic SROIQ hardness"
framing that the 2026-08-01 triage overturned. The correct reading is that the gap remains
overwhelmingly algorithmic, while the per-ontology price of closing it is rising — the
median target moved from 3.47 s to 5.11 s, and the sub-second cohort thinned from a larger
absolute base. Nine easy-harvest cycles have not exhausted the algorithmic surface.

---

## 5. Set A ranked by fastest peer wall — the candidate targets

**The 20 fastest. A peer doing in well under a second what rustdl cannot do in 120 s is
the most diagnostic evidence in this document.** 18 of the 20 are Konclude-fastest; KM is
fastest on two.

| # | ontology | peer | peer wall | ≥ ratio vs 120 s | closure pairs |
|---|---|---|---|---|---|
| 1 | `ore_ont_10019` | konclude | **0.05 s** | **2400×** | 162 |
| 2 | `ore_ont_14272` | konclude | 0.06 s | 2000× | 4,137 |
| 3 | `ore_ont_6485` | konclude | 0.08 s | 1500× | 240 |
| 4 | `ore_ont_10109` | konclude | 0.09 s | 1333× | 481 |
| 5 | `ore_ont_7828` | konclude | 0.09 s | 1333× | 4,804 |
| 6 | `ore_ont_8429` | konclude | 0.09 s | 1333× | 5,049 |
| 7 | `ore_ont_16371` | konclude | 0.10 s | 1200× | 4,491 |
| 8 | `ore_ont_10807` | konclude | 0.11 s | 1091× | 4,789 |
| 9 | `ore_ont_6923` | konclude | 0.11 s | 1091× | 5,236 |
| 10 | `ore_ont_9864` | konclude | 0.11 s | 1091× | 4,443 |
| 11 | `ore_ont_16372` | konclude | 0.14 s | 857× | 0 — **inconsistent** (745 unsat) |
| 12 | `ore_ont_5764` | konclude | 0.18 s | 667× | 5,678 |
| 13 | `ore_ont_6333` | konclude | 0.18 s | 667× | 351 |
| 14 | `ore_ont_10460` | konclude | 0.19 s | 632× | 2,859 |
| 15 | `ore_ont_4827` | konclude | 0.27 s | 444× | 5,463 |
| 16 | `ore_ont_16056` | **km** | 0.28 s | 429× | 2,454 (Konclude 606 — Set C) |
| 17 | `ore_ont_10517` | konclude | 0.29 s | 414× | 2,187 |
| 18 | `ore_ont_1707` | konclude | 0.31 s | 387× | 8,350 |
| 19 | `ore_ont_4141` | **km** | 0.33 s | 364× | 444 (Konclude 0 / **inconsistent** — Set C) |
| 20 | `ore_ont_8273` | konclude | 0.33 s | 364× | 1,925 |

The full 138-member ranking is reproducible with
`scripts/analyse-tail151.py` (§ 6 of its output) and is in
`runs/triage-2026-08-04/triage-table.jsonl`.

**Three sub-clusters in this list are worth naming, because they are cheap to attack:**

- **`ore_ont_10019` is still #1, at 0.05 s.** It is already the design record's canonical
  case (47 classes, 182 concept rules, 0.01 GB RSS, 84.6% of its stall in the **main
  tableau**, attributed to over-branching from defined-class clausification —
  `[[dense-sroiq-root-cause-overbranching]]`). Konclude does it 2400× faster than rustdl's
  cap. The diagnosis exists and the fix (surrogate-atom absorption) is unbuilt.
- **Three tail members are simply INCONSISTENT ontologies** — `ore_ont_16372` (0.14 s),
  `ore_ont_4141` (1.36 s), `ore_ont_8445` (2.55 s). Konclude reports `owl:Thing`
  unsatisfiable with 745 / 107 / 338 unsat classes. rustdl DNFs at 120 s on all three.
  Given that classify's inconsistency detection is a documented sound *under*-approximation
  that cannot reach a tableau-only inconsistency (see the `RUSTDL_CLASSIFY_INCONSISTENCY`
  residual in `CLAUDE.md`), these three are a targeted, self-contained cluster.
- **Two are ~140 k-class ontologies with a genuinely FLAT hierarchy** — `ore_ont_16744`
  (52.17 s, 142,884 `SubClassOf` axioms, **all** `X ⊑ owl:Thing`) and `ore_ont_8737`
  (46.58 s, 136,612, likewise). The entailed non-trivial closure is empty; the work is
  scale, not reasoning. rustdl cannot produce that answer in 120 s.

Two ontologies already named as known problems in `CLAUDE.md` also sit in Set A with small
peer walls, which corroborates the list: **`ore_ont_5368`** (the 27 GB DNF and the DKey
merging-gate discriminator) at **0.68 s**, and **`ore_ont_11085`** (the 16.96 GB OOM) at
**2.69 s**.

---

## 6. Reproducibility control

Because the 151 is a strict subset of the 257, each ontology has two independent peer
measurements a few days apart. This bounds how much of the partition is run-to-run noise:

| peer | identical verdict | changes |
|---|---|---|
| Konclude | **151 / 151** | none |
| HermiT | 148 / 151 | 2 × DNF→CLASSIFIED, 1 × CLASSIFIED→DNF |
| KM | **151 / 151** | none |

Two of the three peers are **perfectly** reproducible on this population, and Konclude's
Set A median moved only 5.06 s → 5.11 s across the two runs. HermiT's three flips are
cap-boundary timing jitter, and since HermiT's set is a subset of Konclude's they change no
partition member. **The headline 138/13 split is not sensitive to run-to-run variation.**

---

## 7. Threats to validity

1. **Concurrency, and its direction.** Legs ran **sequentially** (one peer at a time), but
   **4 batches ran concurrently within a leg** — matching the 2026-08-01 baseline exactly,
   so the comparison is apples-to-apples. Concurrency biases a peer *toward* DNF
   (contention inflates its wall against a fixed 120 s cap). **So Set A = 138 is a LOWER
   BOUND and the reported peer walls are UPPER bounds** — both in the safe direction for
   the claim being made. An uncontended re-run could only move ontologies from B into A and
   only shorten the walls in §3 and §5. Set A walls should be re-measured in isolation
   before any individual one is quoted as a hard target.
2. **No degenerate output survives in Set A — checked individually, not assumed.** Eight of
   the 138 have a Konclude closure under 100 pairs, and each was inspected: three are
   genuinely **inconsistent** (`16372`/`4141`/`8445`, `owl:Thing` unsat); two are genuinely
   **flat** (`16744`/`8737`, ~140 k classes all directly under `owl:Thing` — verified by
   counting `SubClassOf` blocks with a non-`Thing` superclass: **0** of 142,884 and 0 of
   136,612, against 3,855 of 3,867 on a control); three have small but real closures
   (`10929` 1 pair, `4572` 31, `9540` 66). `pairs` is deliberately not the verdict
   predicate — `triage.py` documents that an ontology may legitimately entail nothing — and
   these five zero/near-zero cases are exactly that case, not empty output.
3. **The one genuinely degenerate peer output is `ore_ont_12451`** — Konclude's 896-byte
   Thing/Nothing hierarchy on a valid 1.37 MB file, i.e. a Konclude front-end failure. It
   is reported as EMPTY, kept out of both A and B by the partition's own rule, and is the
   only Set B member whose cause is not a timeout.
4. **HermiT carries a ~0.56 s docker+JVM floor** (its fastest Set A wall here is 2.54 s).
   Its walls are end-to-end and are not comparable to Konclude's native walls without
   subtracting that floor. This does not affect the partition, since HermiT's set is a
   subset of Konclude's.
5. **Outcome only, no adjudication — by design.** This document answers *did a peer produce
   a hierarchy*, not *was it right*. Konclude is documented in this project to under-report.
   Two Set C findings are worth carrying forward even though adjudication was out of scope:
   - **`ore_ont_9540`** (Konclude 66 pairs vs HermiT 71) is **one of the three
     already-recorded Konclude under-reporting cases, independently reproduced here.**
   - **KM disagrees on 11 of the 12** Set C members and always reports *fewer* pairs than
     Konclude/HermiT (e.g. `10807` 4,338 vs 4,789), consistent with its documented
     unsoundness. More sharply: on **`ore_ont_4141`** and **`ore_ont_8445`** Konclude
     reports `owl:Thing` unsatisfiable (inconsistent) while KM returns an ordinary
     444-/1,927-pair hierarchy — **KM appears to miss the inconsistency outright.** Since
     both are in Set A on Konclude's verdict anyway, this does not affect the partition.

   A Set A membership therefore means "tractable for a peer", not "the peer's answer is the
   oracle"; any MISSED comparison on these must adjudicate against Konclude ∪ HermiT.
6. **Closure-size equality is not proof of agreement.** Most both-classified pairs have
   equal closure sizes, but size is invariant under relabelling and two normaliser bugs in
   this repo previously corrupted pairs while leaving counts intact. Set C = 12 is therefore
   a **lower bound**, as `partition.py` states; the true set needs `normalise.py compare`
   over the retained hierarchies.
7. **The rustdl side is single-measurement.** The 151 comes from one 120 s pinned-binary
   run. The 2026-08-01 work validated its analogue with a strictly-sequential idle-host
   re-run of a seeded 20-ontology sample (completed = 0); that control has **not** been
   repeated here, so a small number of the 151 could be contention artifacts rather than
   genuine DNFs. This would shrink the tail, not change the A/B ratio's direction.

---

## 8. What this changes

- The characterization doc's blocked follow-on clustering is **unblocked**; its 242/15
  partition should be marked superseded by 138/13.
- **Set A = 138 is the work set.** It is large enough that the 2026-08-01 plan's stopping
  rule (`|Set A| < 20` ⇒ stop) is nowhere near triggered.
- **Set B = 13 is closed on evidence for now.** Three reasoners have failed the same 13
  twice across independent runs. Engine work aimed there has no evidence behind it; revisit
  only if a peer later classifies one.
- The three highest-value clusters, by peer wall and by self-containment, are: the
  `ore_ont_10019` over-branching case (diagnosis in hand, fix unbuilt), the **three
  inconsistent ontologies** rustdl cannot detect within 120 s, and the **two flat
  ~140 k-class** ontologies.

---

## 9. Reproducing this

```sh
H=/data/dumontier/owl-reasoner-harness
$H/scripts/triage-all-legs-tail151.sh                  # 3 legs, sequential, ~2h50m total
python3 $H/scripts/partition.py \
  --dnf-list $H/baselines/2026-08-04-tail-v0414-list.txt \
  --konclude $H/runs/triage-2026-08-04/konclude-triage.jsonl \
  --hermit   $H/runs/triage-2026-08-04/hermit-triage.jsonl \
  --km       $H/runs/triage-2026-08-04/km-triage.jsonl \
  -o $H/runs/triage-2026-08-04/triage-table.jsonl
python3 $H/scripts/analyse-tail151.py                   # wall distributions, comparison, full ranking
```

Leg walls on this host: Konclude 23 min, HermiT 64 min, KM 81 min. Retained peer
hierarchies (1.8 GB) are under
`/mnt/um-share-drive/dumontier/rustdl-triage-scratch-2026-08-04/` and are the input to any
later FP/MISSED comparison on Set A. The 2026-08-01 baselines were not touched; the new
per-peer JSONLs should be copied to `baselines/2026-08-04-triage-{konclude,hermit,km}-c120.jsonl`.
