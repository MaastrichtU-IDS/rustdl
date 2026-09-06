# D10 arc — long-term roadmap with pre-registered acceptance and kill conditions

**Status:** roadmap, 2026-09-06. Four INDEPENDENT workstreams. Per the writing-plans scope
check, each gets its own plan; only WS1 is planned in detail today
(`2026-09-06-conjunctive-domain-range-filler-110.md`) because only WS1's premise has been
verified against current `main`.

**Why a roadmap and not one plan:** WS1 is an engine fix, WS2 a bounded feasibility probe,
WS3 a corpus acquisition, WS4 a triage. They share a theme (D10 / silent incompleteness) and
nothing else. Bundling them would produce a plan whose tasks cannot be independently
rejected — the failure the scope check exists to prevent.

---

## Standing conditions — every workstream, no exceptions

These are this repo's own hard-won rules. A workstream that violates one is not done, however
good its headline number looks.

| # | Condition | Why it is here |
|---|---|---|
| S1 | **Re-verify the premise against current `main` before planning against it.** | The 2026-08-18 audit: five proposals targeted already-shipped work; three named targets had evaporated. This check has retired more work than it cost, every time. |
| S2 | **Pin the binary per configuration**, named after the configuration, immediately after the build, and verify the pin against a **discriminating** input. | A 2 h scan once measured a sabotage build. `ore_ont_9347` cannot discriminate DKey; `5368` can. |
| S3 | **Declare the numeric pass/fail criterion BEFORE running.** | A probe silently failed to fire on 4 of 6 targets and would have read as 4 confirmations. |
| S4 | **Sabotage every guard.** Break the guarded code; confirm the test fails. Report survivors as findings. | Three sabotages of a differential test all passed 6/6. A guard that cannot be made to fail is not a guard. |
| S5 | **Oracle-adjudicate with a DISCRIMINATING control.** Konclude under-reports (9 recorded); pair every negative probe with a case where the oracle *does* report. | The v0.4.6–v0.4.9 float/double FP. |
| S6 | **Compare the TRIPLE** (`direct_subsumptions`, `unsatisfiable`, `equivalent_groups`), never row counts. | `direct_subsumptions` is the Hasse relation: proving a class unsat ELIDES rows, so a correct fix reads as a regression. 4 recorded instances. |
| S7 | **Alternate arm order** in any wall comparison; a fixed order buys a ~3.4% phantom. | The #70 sweep. |
| S8 | **Adjudicate every reported loss sequentially on an idle host, ≥3 runs**, and **raise the cap before recording a one-sided timeout as a loss**. | Contention manufactures `ok → dnf`; and `ore_ont_12451` (2026-09-05) was 3.4× slower, not lost. |
| S9 | **State what the evidence does NOT cover.** A green FP=0 net over the curated corpus shows INERTNESS for any area the corpus does not exercise. | `datatype_value_membership.rs` says so itself. |
| S10 | **Check whether a fixture's behaviour is a DEFECT or a documented DESIGN DECISION** before reporting it, and before pinning a test to it. | `topwitness.ofn` (2026-09-06 near-miss); `tests-that-pin-the-bug`. |

---

## WS1 — Close #110: conjunctive `ObjectPropertyDomain` / `Range` filler

**Question.** `Domain(r, P ⊓ Q)` + `X ⊑ ∃r.B` entails `X ⊑ P`, `X ⊑ Q`. `classify` returns zero
rows with `incomplete: false` while Konclude, HermiT and KM all derive both. Same for `Range`.
Close it at root.

**Premise, VERIFIED 2026-09-06 (S1).** `collect_el_rules`' `ObjectPropertyDomain` /
`ObjectPropertyRange` arms (`owl-dl-saturation/src/lib.rs:3457-3487`) handle `Bot` (poison) and
`Atomic` (push), and **silently fall through on `And`**. `role_domains` and `role_ranges` are
already `HashMap<RoleId, Vec<ClassId>>` — a conjunction is *already representable*. Both
fragment gates (`is_el_axiom:2183/2186`, `is_saturator_axiom:2574/2577`) refuse via one shared
predicate `is_atomic_or_trivial_concept:2132`.

**Approach.** `Domain(r, P ⊓ Q) ≡ Domain(r,P) ∧ Domain(r,Q)` is a **logical identity**, so
decomposition is sound and completeness-preserving by construction — the same argument shape as
`flatten_union_of_oneofs` (#42 item 1) and #81's range fold.

**Detailed plan:** `2026-09-06-conjunctive-domain-range-filler-110.md`.

**Acceptance (all required):**
- A1. Both reproducers (domain + range) derive the entailed pairs; both atomic controls still do.
- A2. `# fragment:` moves `Horn` → `pure-EL` on both reproducers — the gate and engine moved together.
- A3. A **partially**-decomposable filler (`Domain(r, P ⊓ ∃s.C)`) derives `P` **and is REFUSED by both gates** (a fresh D10 otherwise).
- A4. Konclude ∪ HermiT ∪ KM agree on every probe, each paired with a discriminating control (S5).
- A5. **≥5 sabotages run, every survivor reported** (S4).
- A6. FP=0 net: 11 VERIFIED, closures exact. **Recorded as INERTNESS, not correctness** (S9) — corpus reach is measured zero.
- A7. Two-arm sweep over the **14** conjunctive-filler ORE ontologies plus ≥20 non-bearing controls: triple-identical or adjudicated (S6, S7, S8).
- A8. **Fragment-routing sweep** — the fix moves ontologies onto the pure-EL fast path, which is a behaviour change: 0 `ok → dnf`, 0 answer changes.

**Kill condition.** If A3 cannot be satisfied without the two predicates drifting apart, stop and
re-plan: the fix has become a gate/engine coupling problem, which is the D10 generator itself.

**Expected corpus reward: ZERO.** Measured — 12 of 14 IDENTICAL under `SAME_TIER=1`, 0 gained.
This is a correctness fix. **Do not sell it as a completeness win**, and do not let a flat sweep
be read as the fix not working (S9).

---

## WS2 — `VarCap`: are any of the 39 addressable?

**Question.** `RUSTDL_TRACE_BODY_VARS` census (2026-09-04) found **39 ontologies with
`VarCap`-only refusals** — clauses silently discarded for exceeding `MAX_BODY_VARS = 8`, the same
silent-drop shape as the `NotTree` bug that turned out to be real. Recorded, never diagnosed.

**What is already measured out — do not re-run.** Raising the cap 8 → 16 recovers **nothing** and
**destroys three completers** (`ore_ont_16461`, `7775`, `15491`; 9,773 sound pairs lost), because
the withheld clause is **disjunctive** in each case. No fixed value works: binders need 9, 11, 12,
16, 25 and 133.

**The one unexplored shape**, named in `docs/2026-08-03-max-body-vars.md` and never built: admit
wide **Horn** bodies while continuing to refuse wide **disjunctive** ones.

**Step 1 is a measurement, not a build.** Of the 39, how many refuse a body that is **Horn**?

**Acceptance:**
- B1. Per-ontology Horn/disjunctive split of the 39 refused bodies, by the ENGINE probe, smoke-tested against a known value first (S3).
- B2. For any Horn-bodied member, confirm the withheld clause changes an ANSWER (oracle-adjudicated), not merely that it is withheld.

**Kill condition, pre-registered.** If **fewer than 3** of the 39 refuse a Horn body whose
withheld clause changes an answer, **stop and record the negative**. The CB arc was deferred at a
market of 16; a market under 3 does not justify a new branching-cap policy in the wedge's hot path.

---

## WS3 — Point `verify-el` at a corpus that is not ORE

**Question.** Post-#87 the instrument is clean (0 violations over 403 checkable). It has therefore
found **nothing** on ORE — and ORE is heavily mined by this project. Its value as an unattended
D10 hunter is untested on fresh ground.

**Prerequisite, and it is a real cost.** F1 and F3 remain live builder false-positive mechanisms,
so every `Violated` is a LEAD requiring adjudication (~1 h each with peer oracles). Budget that
before starting.

**Acceptance:**
- C1. Corpus obtained, licensed, and its size + provenance recorded.
- C2. Coverage reported as the same five-bucket split used for ORE (verified / gate-refused by fragment / builder-refused by reason / timeout / error), so it is comparable.
- C3. Every `Violated` adjudicated against Konclude ∪ HermiT ∪ KM with a discriminating control (S5), and classified as engine defect / builder FP / unresolved.

**Kill condition.** If coverage is again ~20% **and** violations are 0 after adjudication,
`verify-el` is measured out as a *hunter*. Reposition it as a **regression gate** (run it on the
curated fixtures in CI, where a new `Violated` means a fresh engine defect) and stop investing in
coverage.

---

## WS4 — The 9 builder-refusal ontologies

**Question.** Of 1,920, exactly **9** pass the CLI fragment gate and are then refused by the
builder: **6 `AxiomsDroppedAtConversion`, 3 `BoundTripped`**. The 6 are the interesting ones — a
dropped axiom is a conversion gap, and `dropped` is *visible*, so these are cheap to characterise.

**Acceptance:**
- D1. Per-ontology, the `dropped` map by kind, from the CLI's own reporting (not a grep).
- D2. Each distinct kind classified: known-and-deliberate (SWRL, `HasKey`) vs an unrecorded gap.

**Kill condition.** If all 6 are known-and-deliberate kinds, record the fact in the `owl-dl-verify`
section and close. This is a ~1 h triage, not a project; **do not** let it grow into a conversion
workstream without a fresh decision.

---

## Ordering and rationale

1. **WS1** — a real defect, root cause known, premise verified, fix sized.
2. **WS4** — ~1 h, and it closes out the decomposition finding.
3. **WS2** — cheap probe with a hard pre-registered kill condition.
4. **WS3** — largest, gated on obtaining a corpus, and its value is contingent on WS2/WS4 not
   turning up something closer to hand.

**What this roadmap deliberately excludes**, each already measured out — re-propose only with new
evidence: raising `verify-el`'s 60 s cap (5× buys +2.1 points, finds nothing); widening
`verify-el` past `is_pure_el` to Horn (two admissible shapes; see
`docs/benchmarks/2026-09-05-verify-el-horn-widening-market.md`); flipping
`RUSTDL_CLASSIFY_SAME_TIER` (corpus-invisible, and measured at 3.4× wall on `ore_ont_12451` for
identical answers).
