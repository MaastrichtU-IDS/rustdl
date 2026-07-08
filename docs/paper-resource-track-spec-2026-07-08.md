# Paper spec — rustdl Resource/system track (2026-07-08)

**Supersedes** `docs/paper-claims-2026-06-08.md` (the anytime/research-track plan, whose
novel core — anytime recovery + calibrated incompleteness — was empirically undercut:
recall is flat at 1.0 on the corpus, and the ⊤⊑C/∃R.⊤ findings showed the completeness
contract needed the fragment-gate). This is the reframed, evidence-backed plan.

**Working title:** *"rustdl: A Sound, Self-Explaining, Embeddable OWL 2 DL Reasoner."*

**Target venue:** ISWC **Resource** track (fallback: In-Use / a tools track). System +
rigorous evaluation, not a new calculus.

---

## 0. Honesty constraints (bind everything)

- **NOT a speed claim.** Konclude beats rustdl 2.2×–809× on real SROIQ; the head-to-head
  goes *in* the paper as a different design point, never hidden.
- **NOT a new calculus.** The extended EL saturation reuses known consequence-based ideas
  (ELK; Kazakov Horn-SHIQ CB; combined approach for nominals; concrete-domain CB). We
  position as sound engineering synthesis + evaluation, not novel theory.
- **NOT guaranteed-complete in general.** rustdl is sound + fragment-gated-complete
  (PureEl/Horn) + empirically near-complete on the measured SROIQ corpus.
- **The defensible contribution is the *combination*:** a reasoner that is sound by
  guarantee, self-explaining, calibrated about its own incompleteness, embeddable, and
  validated by a dual-oracle methodology that caught real bugs — a capability profile no
  single existing reasoner offers.

## 1. Contributions

- **C1 — The system + hybrid architecture.** ELK-style consequence-based EL saturation
  (extended beyond EL via sound reductions: EL⁺⁺ functional-merge, nominals/`NomKey`,
  cardinality/`MaxKey`, `∀R.OneOf`/`ForallKey`, datatype value-membership/`DKey`, `⊤⊑C`,
  `∃R.⊤`) + a hypertableau "wedge" + tableau, dispatched by a per-query orchestrator.
  Native Rust, no JVM. (Konclude-style hybrid design; HermiT-style wedge; ELK-style kernel.)
- **C2 — Built-in explanation/debugging suite.** `justify` (find-one QuickXplain +
  find-all HST, `--laconic` fragment weakening), `diagnose` (root vs derived unsat),
  `repair` (minimal hitting-set diagnoses, removal-verified), `prove` (proof tree),
  `report` (self-contained HTML). **No equivalent in Konclude or whelk-rs.** Every result
  sound by construction (verified against the reasoner).
- **C3 — Calibrated incompleteness.** `completeness_guaranteed()` — a sound contract
  (`⟹ MISSED=0`) gated on the provably-complete fragment; the classifier labels exactly
  when its "not subsumed" answers are guaranteed vs approximate. Fragment diagnostic
  (`PureEl`/`Horn`/`OutOfFragment`) surfaced in the CLI banner + API.
- **C4 — Soundness validated at scale + dual-oracle differential testing.** FP=0 vs the
  Konclude∩HermiT oracle across the curated corpus + ORE-2015 pilot. EL closure
  cross-validated against **two independent** complete/EL-complete reasoners (whelk-rs +
  Konclude) over 400 EL onts. **The validation found real soundness/completeness bugs —
  two in rustdl (`⊤⊑C`, `∃R.⊤`, fixed this session) and one in whelk-rs** (over-derivation
  on a defined-class-∃-over-subproperty pattern, Konclude-confirmed). Reasoners are buggy;
  cross-oracle testing finds it.
- **C5 (supporting) — EL/Horn competitiveness + embeddability.** rustdl's EL kernel beats
  whelk-rs on the majority (median 1.6–2.2×) and is ~1.15× Konclude / beats ELK on GALEN;
  native cold-start ≪ JVM; embeddable as a Rust library.

## 2. Claims → metric → falsifier → baseline

| # | Claim | Metric | Falsified if | Baseline |
|---|---|---|---|---|
| S1 | rustdl asserts no unsound subsumption | FP vs oracle, all configs | any FP>0 | Konclude∩HermiT |
| S2 | `completeness_guaranteed()` ⟹ MISSED=0 | flag vs oracle MISSED | guaranteed=true & MISSED>0 | self (oracle) |
| S3 | EL kernel ⊇ whelk, = Konclude on EL | closure diff, 400 EL onts | rustdl ⊊ oracle on any (non-whelk-bug) ont | whelk-rs, Konclude |
| S4 | Cross-oracle testing finds real bugs | # confirmed defects | — (demonstrated: 2 rustdl + 1 whelk) | Konclude adjudication |
| S5 | Explanation suite is sound + unique | justify/repair verified; feature matrix | any unsound justification/repair | Konclude/HermiT/whelk feature set |
| S6 | EL-competitive; embeddable | wall vs ELK/whelk/HermiT/Konclude; cold-start/RSS | rustdl broadly slower on EL; startup ≈ JVM | ELK, whelk, HermiT, Konclude |

## 3. Evaluation plan (what exists / what to produce)

**Systems:** rustdl, whelk-rs (EL), ELK (EL, complete — *still to add*), HermiT (SROIQ),
Konclude (SROIQ, ORE winner). Native binaries; reasoning-time isolated from JVM/docker
startup (document the confound).

**Corpora:** ORE-2014/2015 (public); BioPortal (911 onts, RDF/XML); the curated
characterization set (GALEN/notgalen/RO/wine/SIO/pizza/ore-*).

**Tables/figures:**
- **T1 Soundness** — FP=0 across corpus + ORE (have). *(S1)*
- **T2 Dual-oracle EL validation** — 400 EL onts: rustdl vs whelk vs Konclude; the 3 bugs
  found (have — this session). *(S3, S4)*
- **T3 Calibration** — `completeness_guaranteed` vs actual MISSED confusion (have the
  contract + fragment data; tabulate). *(S2)*
- **T4 Feature matrix** — explanation/repair/materialize/calibration/embedding across the 4
  reasoners (have). *(S5)*
- **F1 Honest head-to-head** — reasoning-time across corpus incl. rustdl's losses + the
  startup-confound methods note (have most; refresh). *(S6, §0)*
- **F2 EL kernel** — rustdl vs whelk/ELK/Konclude on the EL subset (have rustdl/whelk;
  add ELK). *(C5)*
- **T5 Startup/footprint** microbench — cold-start latency + peak RSS vs JVM (*to produce*).
- **Case study** — debug a broken ontology end-to-end with `debug()`/justify/repair (have
  the tutorial `docs/python-ontology-qa.md`; write up). *(C2)*

**To produce — status (measured 2026-07-08, `docs/paper-evidence-2026-07-08.md`):**
- **ELK baseline: DONE** (galen — rustdl `saturate()` 165 ms > ELK reasoning ~1.6 s /
  classify ~0.4 s; confirms rustdl beats ELK).
- **Startup/footprint: rustdl side DONE + measured** (trivial 30 ms / 6 MB; galen
  0.46 s / 28 MB — the embeddability headline). JVM-side RSS **not cleanly isolable under
  docker** (docker-client `time` ≠ in-container JVM; no `/usr/bin/time`; no cgroup
  `memory.peak`); bounded by the ROBOT docker wall (galen 5.84 s, ~4 s startup) + the known
  JVM floor. **Precise JVM RSS deferred to a native-JVM eval host.**
- **5-reasoner head-to-head:** rustdl's current numbers confirmed (no regression vs the
  committed controlled comparison `docs/reasoner-comparison-2026-06-21.md`). A fresh
  controlled multi-reasoner wall refresh belongs on a **native-JVM host** — re-deriving it
  under docker would be worse methodology (the startup confound §0 warns against). Setup
  task, not a missing result.

## 4. Motivating application (the #1 risk — must land)

Not speed, not novel calculus. The honest niche: **applications that need a sound answer
they can *trust and explain*, embedded, without a JVM** — e.g. an ontology-authoring/CI
tool that classifies, flags exactly which entailments are guaranteed, and shows *why* a
subsumption holds / *how* to repair an unwanted one, all in-process. Reinforced by C4: the
dual-oracle finding that a published reasoner (whelk-rs) is unsound argues that
*self-explaining, cross-validated* reasoning is a real need, not a luxury. If the reviewer
still asks "why not Konclude?": Konclude is faster but is a black box with no justification
facility and a ~1s JVM-free-but-heavy footprint; it can't tell you why, or be embedded.

## 5. Related work

ELK (EL); HermiT (hypertableau); Konclude (saturation+tableau hybrid, pay-as-you-go —
differentiate our per-entailment certainty contract from its name); whelk/whelk-rs;
consequence-based DL (Kazakov Horn-SHIQ; Cucala/Grau/Horrocks disjunctive CB; combined
approach for nominals; concrete-domain CB — position the extended EL kernel as synthesis);
justification/explanation (Horridge et al.; Glimm et al. known/possible subsumers);
reasoner differential/metamorphic testing + the ORE competition (eval methodology).

## 6. Threats to validity

- **Overfit corpus** — report ORE MISSED honestly; S1 (FP=0) is the robust claim, MISSED
  characterized not claimed-zero.
- **"Why not Konclude"** — answered only by §4; without it → In-Use track.
- **Oracle trust** — dual independent oracles (Konclude, whelk on EL; Konclude∩HermiT on
  SROIQ); disagreements reported + adjudicated (we found whelk-rs itself can be wrong).
- **Speed losses** — reported; the claim is the capability combination, not speed.

## 7. Reproducibility

Public corpora (ORE), released harness, pinned rustdl version; the `compare-whelk` +
`konclude_closure_diff` + ROBOT/Konclude oracle scripts; artifact for the repro badge.
