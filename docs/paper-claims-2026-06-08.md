# Paper plan — claims + experiment matrix (target: ISWC / ESWC research track)

Drafted 2026-06-08. Working title:
**"Knowing What You Don't Know: Sound Anytime OWL Classification with Calibrated
Incompleteness."**

## 0. Honesty constraints (these shape everything)

The 2026-06-08 benchmark (`perf-2026-06-08-konclude-vs-rustdl.md`) is the binding
reality. ISWC/ESWC reviewers will re-run Konclude/ELK in an afternoon, so:
- **We CANNOT claim general speed.** Konclude beats rustdl 2.2×–809× and beats
  *HermiT* by 1–3 orders of magnitude; rustdl DNFs wine. The head-to-head table
  goes *in* the paper, framed as a different design point — never hidden/spun.
- **We CANNOT claim guaranteed completeness.** rustdl is sound + *incomplete*.
- **The defensible contribution is the *contract*, not the speed:** a reasoner
  that is **sound by guarantee, incomplete by measurement, anytime by design, and
  self-aware** (it labels exactly which entailments are uncertain). Plus EL/Horn
  competitiveness with mature reasoners and embeddability as support.

## 1. Contribution & novelty (and the #1 risk)

**Proposed contributions:**
1. **A sound anytime classification *contract* with per-entailment certainty
   labeling.** Beyond classic "sound-but-incomplete": rustdl returns a sound
   lower bound on the class hierarchy *plus a guarantee that every non-flagged
   pair is exact and an explicit set of "uncertain" (budget-exceeded) pairs* — a
   partial oracle with **known coverage**. (System + formal contract.)
2. **An empirical characterization of the completeness/latency Pareto with a
   soundness floor**, across the ORE corpus: how much of the true hierarchy is
   recoverable per millisecond, and that the soundness guarantee (FP=0) holds at
   every budget. (Study.)
3. **EL/Horn competitiveness + embeddability**: complete classification
   competitive with HermiT on large Horn TBoxes (and tractable where HermiT
   DNFs, e.g. RO), with ~50× lower startup than JVM reasoners. (Supporting.)
4. **A benchmarking-methodology note**: reasoner wall-time comparisons are
   routinely confounded by JVM/docker startup (~1.5 s); reasoning-time must be
   isolated. We document the confound and the correction. (Methods sidebar —
   broadly reusable; we hit this ourselves, reversing our own prior numbers.)

**#1 RISK — the motivating application (must be resolved before submission).**
The reviewer's killer question: *"ELK is sound+complete+fast on EL; Konclude is
sound+complete+fast on SROIQ — who needs sound-but-incomplete-anytime?"* The
paper lives or dies on a concrete use case. Candidate framings (pick/validate
one, ideally with a real workload):
- **(a) Interactive / embedded querying** over ontologies too large or too hard
  to fully classify, where a sound answer to *one* entailment under a latency
  budget beats classifying everything. (Leans on C5/C6 + Rust embedding.)
- **(b) Sound lower bounds under SLAs** — pipelines that need a trustworthy
  partial hierarchy within a fixed time budget (serverless, CI gating, bulk
  processing of many ontologies) and can act on "known-exact + known-uncertain."
- **(c) The self-awareness itself** — applications that need to *know which
  entailments are certain* (e.g. safety-relevant subsumptions) and route only the
  uncertain few to a slow complete reasoner. rustdl as a *fast sound prefilter*
  in front of Konclude/HermiT.
**(c) is the strongest research framing** — "rustdl as a sound prefilter that
exactly partitions a hierarchy into certain/uncertain, sending only the residual
to a complete reasoner" gives a measurable end-to-end win (total time of
rustdl-prefilter + complete-reasoner-on-residual vs complete-reasoner-on-all).
*If no application survives scrutiny, retarget to ISWC In-Use or Resource track.*

## 2. Claims (each: statement / metric / falsifier / baseline)

| # | Claim | Metric | Falsified if | Baseline |
|---|---|---|---|---|
| **C1** | rustdl never asserts an unsound subsumption | FP vs oracle, all configs | any FP > 0 on any ontology | Konclude∩HermiT oracle |
| **C2** | Incompleteness is *signaled soundly*: `complete=true ⟹ MISSED=0`; uncertain pairs are explicitly enumerated | per-ontology MISSED vs the `complete` flag / timed-out set | any ontology with `complete=true` but MISSED>0, OR a MISSED pair not in the flagged set | self (oracle for MISSED) |
| **C3** | Anytime: recovered-closure % grows monotonically + soundly with budget; rustdl yields a sound partial hierarchy under deadlines where complete reasoners (killed at the same wall) yield nothing | (wall, %closure) curve per ontology; complete-reasoner recovery at matched deadline | recovered closure non-monotone in budget, or FP>0 at any budget | ELK/HermiT/Konclude killed at deadline |
| **C4** | Competitive with HermiT on EL/Horn; tractable where HermiT struggles | complete-classification wall on EL/Horn subset | rustdl slower than HermiT on the EL/Horn subset broadly (RO-type wins absent) | ELK, HermiT, Konclude |
| **C5** | ~50× lower startup + lower footprint than JVM reasoners | cold-start latency, peak RSS, trivial query | startup/ RSS not materially below JVM reasoners | HermiT, ELK (JVM); Konclude (native) |
| **C6** | Single-entailment latency ≪ full classification on large ontologies | one-query latency vs full-classify wall | per-query ≈ full-classify (no pay-per-query benefit) | self; HermiT classify-then-lookup |

Note: **C1 is the robust headline** (FP=0 is a guarantee we hold corpus-wide);
**C2+C3 are the novel core**; **C4–C6 support the niche.** MISSED=0 is *not*
claimed in general (it would overfit our tuned corpus — see §5).

## 3. Experiment matrix

**Systems** (all sound+complete except rustdl): **rustdl** (configs below),
**ELK** (EL, complete — *essential* baseline, currently missing), **HermiT**
(SROIQ, complete), **Konclude** (SROIQ, ORE winner). Optional breadth: JFact /
Pellet for the correctness table only.

**rustdl configs**: `--pair-timeout-ms ∈ {0(unbounded), 25, 100, 1000, 5000}`;
`--saturation-only`; default. (The sweep drives C3.)

**Corpus**:
- **Primary: the ORE 2014/2015 competition corpus** (public, hundreds of
  ontologies, profile-labeled) — required for credibility and to avoid the
  "tuned-on-26-ontologies" critique. Stratify by profile (EL vs rest) and size.
- **Secondary: our characterization set** (GALEN, notgalen, RO, wine, SIO,
  pizza, ore-10908, ore-15672, …) for deep per-ontology analysis + the case
  studies (wine anytime curve, RO-vs-HermiT-DNF).

**Metrics** (per system × ontology × config; ≥3 repeats, median; native binaries;
reasoning-time isolated from startup):
- Correctness: FP, MISSED vs oracle (oracle = Konclude∩HermiT; disagreements flagged & excluded).
- Time: total wall **and** isolated reasoning time; startup floor.
- Anytime: % true-closure recovered; wall — at each budget.
- Calibration: pair-level precision/recall of the "uncertain" flag vs actual MISSED; ontology-level `complete ⟹ MISSED=0`.
- Resources: peak RSS, cold-start latency.
- Single-query: sampled-entailment latency vs full-classify.
- Prefilter (if framing (c)): wall of (rustdl certain/uncertain partition + complete reasoner on uncertain only) vs complete reasoner on all.

**Outputs (figures/tables):**
- **T1 Correctness** — FP=0 across all systems-as-oracle-checks; rustdl MISSED distribution. (C1, C2)
- **F1 Anytime Pareto** — wall vs %closure recovered, rustdl budget sweep, with complete-reasoner-at-deadline points. *The headline figure.* (C3)
- **T2 Calibration** — confusion matrix of `complete`/uncertain flag vs MISSED. (C2)
- **F2 EL/Horn** — complete-classification time, rustdl vs ELK/HermiT/Konclude, on the EL/Horn subset; call out RO. (C4)
- **T3 Startup/footprint** microbench. (C5)
- **F3 Query vs classify** latency across sizes. (C6)
- **T4 Honest head-to-head** — reasoning-time across the corpus incl. our losses + the methods note. (§0)
- **F4 Prefilter** (framing c) — end-to-end time of the sound-prefilter pipeline. (application)

## 4. Methodology rigor (ISWC reproducibility expectations)
- Native binaries; reasoning-time isolated from JVM/docker startup (document the
  confound — §0/T4); ≥3 repeats, median, idle host, hardware reported.
- Oracle = agreement of two independent complete reasoners (Konclude, HermiT);
  ontologies where they disagree are reported and excluded from MISSED.
- Public corpus (ORE) + released harness + the rustdl version pinned; artifact
  for the reproducibility badge.

## 5. Threats to validity (state them; reviewers will)
- **Overfit corpus**: our FP=0/MISSED=0 was achieved *on* the 26-ontology set we
  tuned. The full ORE suite will show nonzero MISSED — we report it; C1 (FP=0) is
  the robust claim, MISSED is characterized not claimed-zero.
- **"Why not ELK/Konclude"**: answered only by §1's application; without it the
  paper is weak (→ In-Use/Resource track).
- **Anytime value is tail-concentrated**: most ontologies classify cheaply; the
  anytime benefit shows on the hard minority. Be explicit about *when* it matters.
- **We lose on speed**: reported honestly; the claim is the contract, not speed.
- **Oracle trust**: HermiT built our existing oracle; using Konclude∩HermiT
  agreement de-risks single-reasoner bias.

## 6. Related work to position against
ELK (Kazakov et al., EL); HermiT (Motik/Shearer/Horrocks, hypertableau);
Konclude (Steigmiller et al., saturation+tableau, "pay-as-you-go" — *note the
name clash; differentiate our per-entailment certainty contract from Konclude's
pay-as-you-go*); the ORE competition (eval methodology); anytime/approximate DL
reasoning (e.g. approximate ELK, screech, role/concept approximations); the
known/possible-subsumer optimization (Glimm et al. 2010).

## 7. What we need before submission (ordered)
1. **A validated motivating application** (§1) — the gating item.
2. **ELK baseline** — cheap, essential (ROBOT bundles it); run first.
3. **The anytime Pareto (F1)** on the secondary set — the headline figure;
   cheap to produce now (budget sweep we already partly have).
4. **Full ORE corpus** runs for T1/T2/C1/C2 credibility — the heaviest item.
5. Startup/RSS (T3), query-vs-classify (F3) microbenches — cheap.
6. Prefilter pipeline (F4) if framing (c).

**Recommended first concrete steps (cheap, high-value, de-risk the thesis):**
(i) ELK baseline on the EL/Horn subset; (ii) the anytime Pareto figure on the
secondary set; (iii) the signal-calibration confusion matrix. These three
validate C2/C3/C4 — the novel core — before investing in the full ORE sweep.
