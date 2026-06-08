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

## 8. De-risking results (2026-06-08)

**(i) ELK baseline — DONE** (`perf-2026-06-08-konclude-vs-rustdl.md`). C4 refined:
rustdl is competitive with ELK on *mid-size Horn* (galen 0.59 s vs 0.85 s,
notgalen ~tie) and faster than HermiT; **ELK wins big on large pure-EL**
(go-basic ~8.5×); **Konclude beats even ELK** (go-basic ~7×). Bonus
contract-supporting point: ELK *hard-rejects* pizza and *silently drops* wine/sio
non-EL axioms (incomplete, no signal) — rustdl degrades soundly + self-aware.

**(ii) Anytime sweep — DONE** (pizza/ore-15672/sio × {25,100,1000} ms):
- **C1/soundness floor: FP=0 at EVERY budget × every ontology. Rock-solid.**
- **C3 is real but ontology-dependent, NOT a uniform monotone curve:** *pizza*
  buys completeness with budget (25 ms→MISSED=4, 100 ms→MISSED=0); *sio/ore-15672*
  are **complete at every budget** (timed-out pairs are non-subsumptions), so there
  higher budget only wastes wall (sio 17.8 s@25 ms → 72 s@1000 ms, same complete
  answer). Knob value = recover real subsumptions (pizza) *and* cap wasted work
  (sio), always sound.
- **C2 sharpened (and a self-correction):** the incompleteness signal
  (`timed_out_pairs>0`) is **SOUND but CONSERVATIVE** — `complete=true ⟹ MISSED=0`
  holds, but `complete=false` over-warns (ore-15672, sio flag incomplete yet are
  MISSED=0). So "calibrated incompleteness" is presently a *sound over-approximation*
  of the uncertain set, not a tight one. **Paper implication:** either (a) report
  it honestly as a sound conservative flag (the *certain* set is exact — still
  useful), or (b) *tighten* it (a real contribution: shrink the flagged-uncertain
  set toward the truly-uncertain) — measured by (iii).

**(C5) Startup + footprint — DONE (clean win).** `/usr/bin/time -v`, native:
- rustdl: bibtex **~0.00 s / 5.3 MB**, galen 0.57 s / 30 MB.
- Konclude (native): bibtex 0.02 s / 31 MB, galen 0.10 s / 42 MB.
- HermiT/ELK (JVM via ROBOT): ~1.6 s startup floor (measured); JVM heap RSS
  100s of MB (clean standalone-jar RSS = a small TODO).
rustdl has the **lowest startup AND smallest footprint** — ~50–160× lower startup
than JVM reasoners, ~6× smaller RSS than even native Konclude on a trivial query.
This is the most unambiguous numeric win and the backbone of the embedding story.

**(iii) Calibration confusion matrix — DONE** (full data:
`c2-calibration-matrix-2026-06-08.md`; 11 oracle ontologies × budgets × trust_sat
× label_heuristic; conservation-identity-verified). **Pivotal — and it weakens C2:**
- **C1: FP=0 every cell/budget/config.** Rock-solid.
- **TWO silent (unflagged) not-subsumed channels, not one:** `trusted_refute`
  (wedge `Sat`, gated by `trust_sat`) AND `label_pruned` (Phase-7 label heuristic,
  ungated). Both large on every SROIQ row even when `timed_out=0` (sio@1000: signal
  says "complete" while ~111k pairs — 78850 wedge + 32209 label — were never
  tableau-verified). If either is incomplete anywhere, C2's boolean breaks silently.
- **Over-warn 84% (pizza@25), 100% elsewhere** — sound but a *very* loose flag
  (flags thousands of correctly-resolved non-subs); near-useless as a *precise*
  uncertainty oracle in the fast config.
- **No realized false-negative** here — but the FN test is **vacuous** (corpus
  tuned to MISSED=0); `trust_sat=0` did not lower MISSED on any row.
- **C2 provably sound (MISSED ⊆ flagged) ONLY under `trust_sat=0` AND
  `label_heuristic=0`** (both channels → 0; demonstrated, MISSED still 0) — at
  ~20–30 s vs ~1–2 s, even heavier over-warning.
- **Conservation identities** (`timed_out(ts0)=timed_out(ts1)+trusted_refute(ts1)`,
  exact every row) make the accounting airtight.

**Consequence:** C2-as-hoped ("certain set is exact; here are the uncertain pairs")
**fails in the fast config** (two silent channels) and is *useless-as-precise* even
when sound (100% over-warn); it holds *provably* only in a slow, flooding config.
So C2 is either (a) honestly a **configurable soundness contract** (fast ↔
provably-self-aware) with the conservation-identity characterization as the
contribution, or (b) upgraded by an **unsolved signal-tightening** result (flag
only the truly-uncertain). Without (b), C2 is not a strong standalone contribution.

## 9. Post-de-risking honest standing (which claims survive)

- **Strongest numeric wins: C1 (soundness, FP=0 — a guarantee) + C5 (embedding —
  lowest startup + smallest RSS).**
- **C4** EL/Horn: honest support — competitive with HermiT, loses to ELK on big-EL
  and Konclude everywhere; + the ELK-rejects-out-of-EL graceful foil.
- **C3** anytime: a *property*, not a comparative win here (Konclude all <0.3 s).
- **C2** (intended novel core): **weaker than hoped** — sound-conservative, two
  silent channels, provable only in a slow config; needs signal-tightening
  (unsolved) to be strong.
- **Net:** the data supports a **sound, lightweight, embeddable reasoner with a
  configurable soundness contract** (Resource/In-Use strength) more than a
  novel-algorithm research-track result. Research-track needs EITHER the
  signal-tightening contribution (b) OR a validated application (sound prefilter)
  OR full-ORE evidence where the self-aware/anytime behavior delivers a measurable
  win. **This is the framing decision now on the table.**

## 10. ORE 2015 at-scale run — DONE (`ore-2015-results-2026-06-08.md`)

Fetched ORE 2015 (Zenodo 18578, 1920 ontologies); pilot 232 (187 full-oracle =
Konclude∩HermiT). Stratified: 256 PureEl / 233 Horn / 190 OutOfFragment (so the
C2 hunt is non-vacuous, unlike the tuned corpus). **Two defects the tuned corpus
could not surface:**
- **(FIXED) Default-config SOUNDNESS bug:** the snapshot cache emits spurious
  subsumptions on ≥5 ontologies (`ore_ont_13723`: 30 FP vs oracle, silent). Root:
  `BackPropRisk::Safe` ignores disjunction → disjunctive inv/nom/card-free
  ontologies pass Safe → A1-unsound reuse. **Fixed: `RUSTDL_SNAPSHOT_CAPTURE`
  default→OFF** (its sound domain is Horn-shortcircuited anyway). Verified: 13723
  FP gone, tuned corpus byte-identical FP=0/MISSED=0, 96 tests pass.
- **(CHARACTERIZED) C2 silent false-negatives — the headline at-scale result:**
  ≥72 pairs across 4 ontologies are genuine *calculus* false-negatives (subclass
  =NO, oracle=yes, **no timeout flag**), validated with all silent channels off +
  unbounded tableau. Largest: `ore_ont_9054` (60 pairs) = the Phase-D1 datatype
  under-approximation surfacing as a *silent* incompleteness; + SAO BFO-chain;
  xenc. Lower bound (183 MISSED pairs unresolved at the 15 s subclass cap). **So
  C2's "self-aware" signal provably misses real subsumptions silently at scale.**
- **C3 comparative anytime: NOT supported even at scale** — 4 ontologies where
  HermiT hit 300 s but rustdl finished, yet Konclude does them in 1–2 s (never
  "complete reasoners give nothing"). No all-complete-DNF case.
- **C1 calculus soundness held at scale:** FP=0 at the `subclass`/calculus level
  across 187 (modulo 1 DNF ontology). The FP above was a snapshot *integration*
  bug, not a calculus bug.
- **Headline distribution (187): 86.1% sound+complete.** EL/Horn: 0 FP/MISSED.
  Median wall Konclude 0.2 s / rustdl 0.5 s / HermiT 5.0 s; rustdl beats HermiT on
  EL/Horn, loses to Konclude; 30 SROIQ DNF at 300 s. Plus an inconsistency-
  detection gap (11 ontologies: HermiT classify-crashes on incoherent KBs; rustdl
  returns an unflagged hierarchy).

**UPDATE 2026-06-08 — datatype gap (the largest silent-C2-FN source) PARTIALLY
CLOSED.** Phase D6 (DataKey value-membership reduction, integer facets) recovers
`ore_ont_9054` MISSED 79→37 (42 pairs), FP=0/MISSED=0 re-verified corpus-wide.
So the biggest concrete silent incompleteness is now a *sound* completeness gain
for integer facets; the 37 residuals are out-of-scope-by-construction (31 float +
6 bare-integer — both straightforwardly closeable by extending the same reduction
to float ranges / bare-`xsd:integer`). This turns the C2-silent-FN finding from a
*defect* into a *characterized, progressively-closeable* gap — a better paper
story (a sound concrete-domain completeness lever with a clear frontier).

**Consequence for framing:** ORE did NOT rescue a clean research-track win
(C3 unsupported; C2-as-self-aware *fails* — silent FNs demonstrated). It DID (a)
find+fix a real default-config soundness bug, and (b) characterize a concrete,
fixable incompleteness (the datatype under-approximation). The publishable shape
is **characterization + fixes** (resource/experience), or a research contribution
only if the datatype gap is closed and/or the signal is tightened to catch the
silent channels. The robust positives stand: **C1 (calculus FP=0 at scale) +
C5 (embedding) + EL/Horn-beats-HermiT.**

**Honest reframe from de-risking:** the comparative anytime claim ("sound partial
where complete reasoners give nothing") has **no support on this corpus** —
Konclude completes everything in <0.3 s. C3's defensible form is a *property*
(tunable sound completeness/latency with an FP=0 floor) valuable for **embedded**
use (couples to C5), plus the *signal* (C2). The comparative version needs the
full ORE suite's reasoner-DNF ontologies. This pushes the paper's center of
gravity firmly onto **C1 (soundness guarantee) + C2 (self-aware incompleteness) +
C5 (embedding)**, with C3 as the property and C4 as honest support.
