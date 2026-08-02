# Domain absorption — implementation, and the decisive experiment

**Date:** 2026-08-01 · rustdl v0.4.10 · gate `RUSTDL_DOMAIN_ABSORPTION`, **default OFF**, `=1` enables
· pinned binary `sha256 d692ea82259a5d2da6cb7074407dbe9149b1c8edfa971b93bf91326ed92a2511`
(`/tmp/…/scratchpad/bin/rustdl-domainabs-v1`, byte-identical to `target/release/rustdl` as built)

Implements step 2 of `docs/2026-08-01-absorption-is-the-bottleneck.md`, then runs the
step-2 experiment `docs/2026-08-01-residual-absorbability-census.md` called for:

> Does reaching **zero** residuals recover ontologies, while merely **reducing** residuals
> does not? Or does recovery track residual count continuously?

**Headline: neither. Recovery is essentially absent in both groups, so residual count is
not the operative variable.** Details in § The decisive experiment. This bounds the
absorption thesis to the handful of cases already demonstrated and argues against
starting qualified-`∃` absorption on the strength of the residual census.

---

## 1. What was built

`absorb_domain_residuals` — a third pass inside `absorb_roles`, run **after** the two
pre-existing rewrites so their priority is unchanged. For each surviving residual GCI it
looks for a disjunct that is `Max(0, R, ⊤)` or `All(R, ⊥)` (the NNF forms of `¬(≥1 R)` and
`¬∃R.⊤`) and, if found, replaces the whole residual with an **unguarded `RoleRule`**:

```
⊤ ⊑ ¬∃R.⊤ ⊔ rest   ≡   ∃R.⊤ ⊑ rest   ≡   ⊤ ⊑ ∀R⁻.rest
```

and `⊤ ⊑ ∀S.ψ` is exactly `RoleRule { role: S, guard: None, target_label: ψ }` — the shape
`absorb_roles` already emits for `ObjectPropertyRange`. No parallel mechanism was added.

**The role is flipped.** `apply_role_rules` adds `target_label` to the *neighbour* across a
matching edge; the neighbour across an `R⁻` edge is the R-**predecessor**, which is the node
a domain axiom constrains. Sub-role propagation comes free from the tableau's
`edge_satisfies` (an `s`-edge with `s ⊑ r` fires the `r`-domain rule). When `R` is itself
`Role::Inverse(r)` the flip yields `Role::Named(r)` and the rule correctly constrains
r-edge *targets*.

### Soundness boundaries — the whole risk

| disjunct | antecedent | absorbed? |
|---|---|---|
| `Max(0, R, ⊤)` | `≥1 R` | **yes** — identical to `ObjectPropertyDomain(R, rest)` |
| `All(R, ⊥)` | `∃R.⊤` | **yes** — the same axiom |
| `Max(k, R, _)`, k ≥ 1 | `≥ k+1 R` | **NO — UNSOUND.** A domain rule fires at the *first* successor; the antecedent needs k+1. Strictly too strong. |
| `All(R, D)`, D ≠ ⊥ · `Max(0, R, C)`, C ≠ ⊤ | `∃R.¬D` / `∃R.C` | **NO** — qualified; needs a filler check |

`(≥1 R) ⊑ C` is *logically identical* to `ObjectPropertyDomain(R, C)`, so this is sound **and
completeness-preserving**: it must change no verdict anywhere, only cost.

---

## 2. `ore_ont_3281` — the pre-registered prediction, half met

The census predicted `residual_gcis: 28 → 0` and the analysis predicted **~0.03 s** without
editing the ontology.

| | residual_gcis | `--pair-timeout-ms 1000` | `--pair-timeout-ms 1` |
|---|---:|---|---|
| flag OFF | **28** (all `domain_absorbable`) | **DNF at 300 s** | 8.84 s, **3432 timed-out pairs** |
| flag ON | **0** | **11.40 s**, 0 timed out, 224 subs | 5.40 s, **0 timed out** |

- **Residual prediction: met exactly.** 28 → 0.
- **Recovery: real.** At a non-truncating budget the ontology goes from *does not finish* to
  *finishes with the complete answer* (0 timed-out pairs, 224 subsumptions).
- **Closure identity: byte-identical**, 224 rows, at `--pair-timeout-ms 1` where both sides
  terminate.
- **Wall prediction: NOT met.** 5.40 s / 11.40 s, not 0.03 s.

### Why the 0.03 s figure does not reproduce — the calibration was confounded

The 300× calibration came from deleting one `EquivalentClasses` axiom. That deletion removes
**both directions at once**: the `⟸` direction (which produces the two residuals) *and* the
`⟹` direction `Relation ⊑ (≥1 hasTarget) ⊔ (≥1 hasSource)`, a **disjunctive concept rule**
that absorption does not touch. Splitting the axiom into its two halves (line 257 replaced by
one `SubClassOf`, everything else untouched), all at `--pair-timeout-ms 1`:

| variant | residual_gcis (OFF) | wall OFF | wall ON |
|---|---:|---|---|
| `⟸` only — `(≥1 hT) ⊔ (≥1 hS) ⊑ Relation` | 28 | **0.02 s** | 0.01 s |
| `⟹` only — `Relation ⊑ (≥1 hT) ⊔ (≥1 hS)` | 26 | **0.02 s** | 0.02 s |
| both (as shipped) | 28 | **8.84 s** | 5.40 s |
| neither (the doc's deletion) | 26 | 0.01 s | 0.02 s |

All four produce the same 224 subsumptions.

**28 residuals alone cost nothing (0.02 s). The disjunctive concept rule alone costs nothing
(0.02 s). Only their interaction is expensive.** So *"two extra residual disjunctions cost
300×"* is a mis-attribution: the controlled deletion changed two things, and the residual
count was the one that got the credit. Domain absorption breaks the interaction by removing
one side of it — which is why `3281` recovers at all — but the residual count was never the
governing quantity, and this is the single point the whole extrapolation rested on.

---

## 3. Correctness gates

| gate | result |
|---|---|
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | clean |
| `cargo test --workspace --exclude owl-dl-py` | **1467 passed, 0 failed** |
| `./scripts/run-soundness-diff.sh` **with `RUSTDL_DOMAIN_ABSORPTION=1`** | **11 VERIFIED, all closures exact**; 22 tests passed, 0 failed |

FP=0 net, flag ON: galen 27997, notgalen 32739, sio 8904, ore-10908 6001, wine 653,
pizza 499, alehif 247, ro 158, ore-15672 142, sulo 51, bibtex 16 — every one
`FP=0 MISSED=0`. Three NOT VERIFIED for want of fixtures (`ro-stripped`, `sulo-stripped`,
`sio-stripped`), the documented pre-existing absences.

### Verdict identity — the core correctness claim

**Curated fixtures, `classify --pair-timeout-ms 1000`, `RAYON_NUM_THREADS=1`, ON vs OFF:
closure rows byte-identical on 13/13.**

pizza 314, sio 1617, ro 49, sulo 15, bibtex 15, wine 0 (DNF at 600 s on *both* sides),
family 59, go-basic 57803, galen 3309, notgalen 4111, alehif 51, ore-10908 759,
ore-15672 75.

Three fixtures showed residual **banner** differences (pizza, sio, ore-15672). All are
telemetry: `# wall breakdown ms`, the `# wedge-cost-histogram` data line, `# label heuristic`,
`# pairs-per-sub`, `# hyper-proven pairs`. An **OFF-vs-OFF control on the same binary**
reproduces differences of exactly this kind — on pizza it moves the histogram line
(`180 → 181`) *and* `hyper-proven pairs` (`103 → 104`) with the flag held constant. The
histogram buckets by whole milliseconds, so a bucket-boundary shift is a timing artefact.
No closure row moved on any fixture.

**ORE ontologies that complete, `--pair-timeout-ms 1000`, with an ON-vs-ON control arm** (the
control exists because at a *truncating* budget the hierarchy is not run-to-run deterministic
on hard ontologies — see the v0.3.39/`CLASSIFY_LABELS_AMORTIZE` measurement warning):

| ontology | residual_gcis | domain_absorbable | ON-vs-ON | ON-vs-OFF |
|---|---:|---:|---|---|
| `ore_ont_10019` | 5 | 5 | SAME | **SAME** |
| `ore_ont_12433` | 13 | 2 | SAME | **SAME** |
| `ore_ont_1270` | 12,705 | 12,702 | SAME | **SAME** |
| `ore_ont_12836` | 206 | 206 | SAME | **SAME** |
| `ore_ont_15030` | 758 | 538 | SAME | **SAME** |
| `ore_ont_16274` | 16 | 16 | SAME | **SAME** |
| `ore_ont_1853` | 109 | 96 | SAME | **SAME** |
| `ore_ont_2360` | 73 | 73 | SAME | **SAME** |
| `ore_ont_6727` | 46 | 46 | SAME | **SAME** |
| `ore_ont_4049` | 2,204 | 2,197 | SAME | **SAME** |
| `ore_ont_3281` | 28 | 28 | SAME | DIFF — **OFF does not finish** (rc 124 at 240 s); ON gives 224 rows |

**10 of 10 comparable ORE ontologies are identical**, spanning `domain_absorbable` from 2 to
12,702. The eleventh, `ore_ont_3281`, is the recovery case: there is nothing to compare
because the flag-OFF side produces no answer.

### Sabotage — 6 of 6 caught

Per `[[sabotage-your-own-guard-tests]]`. Strictly serial (one build at a time in one
worktree). Each row names the canary that failed; each sabotage failed *its* canary, not
merely "some test".

| # | sabotage | unit canary | verdict canary |
|---|---|---|---|
| S1 | accept `Max(k ≥ 1, R, ⊤)` as domain — **the unsound one** | `max_k_ge_1_is_not_domain_absorbed` ✅ | `min_two_antecedent_with_one_successor_stays_consistent` ✅ |
| S2 | accept `All(R, D)` for any `D` — **unsound** | `all_non_bot_filler_is_not_domain_absorbed` ✅ | `qualified_antecedent_with_other_filler_stays_consistent` ✅ |
| S3 | drop the `filler = ⊤` check on `Max(0, R, filler)` — **unsound** | `max_zero_qualified_is_not_domain_absorbed` ✅ | `qualified_max_zero_antecedent_stays_consistent` ✅ |
| S4 | forget `Role::flip` (constrain the target, not the source) | 4 canaries ✅ | 7 canaries ✅ |
| S5 | drop `rest`, always emit `⊥` | 3 canaries ✅ | 2 canaries ✅ |
| S6 | ignore the env var (feature always on) | `flag_off_leaves_domain_axiom_as_residual` ✅ | — (flag-default is a unit-level fact) |

**Honest note on how this list was reached:** on the first pass S3 was caught by the *unit*
canary only — there was no verdict-level false-positive canary for the `Max(0, R, C≠⊤)` shape,
even though it is the same soundness class as S2. `qualified_max_zero_antecedent_stays_consistent`
was written afterwards and S3 re-run to confirm it fails. The gap was found by running the
sabotage, not by reading the test list.

The verdict canaries run on the **main tableau** (`RUSTDL_HYPERTABLEAU=0`,
`RUSTDL_WEDGE_CONSISTENCY=0`, both ABox pre-checks off). This is load-bearing: `clause.rs`
clausifies from `InternalOntology` **directly, not from the absorbed TBox**, so a
wedge-answered query is blind to this feature and a canary that let the wedge answer would
guard nothing.

---

## 4. The decisive experiment

**Question.** Does reaching **zero** residuals recover ontologies, while merely **reducing**
residuals does not? Or does recovery track residual count continuously?

**Method.** Census re-run with this binary over the 167 DNF survivors
(`/data/dumontier/owl-reasoner-harness/baselines/2026-08-01-survivors-167-list.txt`) — it
reproduces the published census exactly: **54** reach `residual_gcis == 0` under domain
absorption (**Group Z**), **77** keep residuals (**Group N**), 30 already had zero, 6 time out
in conversion. Group N is the full 77, not a sample, and spans 1 to 1,380 remaining residuals.

`classify` (no per-pair budget), **120 s cap**, `RAYON_NUM_THREADS=1`,
`ulimit -v $((24*1024*1024))`, flag ON, 3 concurrent workers. **Every non-DNF outcome was then
re-run strictly serially on an idle host, with its flag-OFF twin**, plus a 10-ontology
flag-OFF control drawn from the DNF set.

### Recovery rate

| group | n | completes | recovery | memory-abort | still DNF |
|---|---:|---:|---:|---:|---:|
| **Z** — residuals → **0** | 54 | **1** | **1.9%** | 2 | 51 |
| **N** — residuals **remain** | 77 | **3** | **3.9%** | 1 | 73 |

### Every recovery, serially re-verified ON vs OFF

| ontology | group | residuals before → after | ON | OFF |
|---|---|---|---|---|
| `ore_ont_3281` | Z | 28 → **0** | **11.49 s**, 224 subs | DNF @120 s |
| `ore_ont_16372` | N | 49 → **5** | **6.66 s**, 2237 subs | DNF @120 s |
| `ore_ont_6132` | N | 114 → **4** | **33.34 s**, 394 subs | DNF @120 s |
| `ore_ont_9899` | N | 114 → **4** | **33.16 s**, 487 subs | DNF @120 s |

All four are genuine DNF→completes, same binary, uncontended. The 10 flag-OFF controls all
DNF at 120 s, so the survivor baseline reproduces on this host.

### Dose–response within Group N

| residuals remaining | n | completes |
|---|---:|---:|
| 1–2 | 21 | 0 |
| **3–9** | **14** | **3** |
| 10–49 | 13 | 0 |
| 50–99 | 8 | 0 |
| 100+ | 21 | 0 |

Not monotone, and the one non-empty cell is **not** the smallest bucket: nothing recovers at
1–2 residuals, three things recover at 3–9.

### The answer

**Neither. Recovery does not track residual count at all, in either form.**

- **Zero is not a threshold.** 53 of 54 ontologies reach zero global disjunctions and still do
  not finish. "No residuals" is a structural property of the absorbed TBox, not a statement
  about tractability — exactly what the census warned when it flagged `ore_ont_10019` (zero
  residuals under domain absorption, stall profiled at 84.6% main tableau).
- **Nor is recovery continuous in residual count.** Group N recovers at 3.9% against Z's 1.9%
  — if anything *higher*, and at n = 54/77 the two are statistically indistinguishable. Every
  Group-N recovery left 4–5 residuals standing, and the 21 ontologies left with only 1–2
  residuals recovered nothing.
- **Third possibility, and the one the data supports:** recovery is driven by *which* axiom is
  absorbed and how it interacts with the rest of the TBox, not by how many residuals remain.
  This is the same conclusion the `ore_ont_3281` attribution in §2 reaches from the other
  direction — 28 residuals alone cost 0.02 s; it took their *interaction* with a disjunctive
  concept rule to cost 300×.

**So the absorption thesis is bounded to the four cases demonstrated here.** 4 of 131
survivors (3.1%) recover. Domain absorption is real, sound, cheap and worth having — but it is
not a lever on the DNF tail, and the residual census cannot be used to predict which
ontologies a further absorption technique would rescue.

### Threats to validity, stated

- The ON arm ran 3-way concurrent; every *outcome-changing* row was re-measured serially. A
  contended run could in principle have pushed a marginal completer over 120 s, which would
  understate recovery — but the 10 serial flag-OFF controls all DNF, and 127 of 131 rows DNF
  with ≥ 68 s of headroom unused.
- **Three ontologies aborted at the 24 GB address-space cap** (`ore_ont_10621`, `11270`,
  `11085`, all ~17 GB RSS). **They abort identically with the flag OFF** — walls and RSS match
  to within 1% (e.g. `10621` 93.72 s / 17,648,260 kB ON vs 93.56 s / 17,648,172 kB OFF), so
  this is the pre-existing RSS tail, **not** a regression introduced by domain absorption. They
  are counted separately above and excluded from both recovery numerators.
- 6 survivors (all 11–59 MB) never reach classify — they time out in *conversion* — and are
  outside both groups.
- 120 s is a budget, not a limit: an ontology counted DNF here might finish at 600 s. The
  question asked was recovery *at a fixed budget*, which is what a user experiences.

---

## 5. Recommendation on qualified-`∃` absorption

**Do not start it on the strength of the residual census. NO-GO as currently motivated.**

The census's own case for it was: qualified-`∃` is 58% of survivor residual *volume*, and it
would raise the zero-residual survivor count from 54 to 85. Both facts are true and now known
to be **irrelevant to the outcome anyone cares about**, because the experiment shows the
zero-residual predicate has no recovery value: reaching zero rescued **1 of 54**. Adding 31
more ontologies to a class with a 1.9% recovery rate predicts ~0.6 further recoveries.

That is the whole argument, and it is worth taking seriously because qualified-`∃` absorption
is the expensive one: it needs a **backward** role rule (target label → source label), which
`RoleRule` cannot express, so it is a new mechanism in the tableau rather than a rewrite into
an existing one. A large project justified by a metric that has just been measured not to
predict the outcome.

Three narrower things the data *does* support:

1. **Keep domain absorption, default OFF for now.** It is sound and completeness-preserving by
   logical identity, verdict-identical everywhere measured, and it converts 4 DNFs into
   answers. Flipping it default-ON needs a broader wall check first: it is *not* free — the
   census shows 1,030 of 1,913 pool ontologies carry at least one domain-absorbable residual,
   so this pass changes the absorbed TBox of the majority of the corpus, and only the 13
   curated + 11 ORE ontologies here have been checked for wall regression.
2. **The four recoveries are the specimens to study**, not the population. `ore_ont_3281`,
   `6132`, `9899`, `16372` are cases where removing *one particular kind* of global disjunction
   unblocked the search. Understanding what those four share is a cheaper and better-aimed
   question than "remove more residuals".
3. **Re-attribute `ore_ont_3281` in the analysis doc.** Its 300× is not "two residual
   disjunctions"; it is the interaction between the `⟸` residuals and the `⟹` disjunctive
   concept rule, neither of which costs anything alone (§2). The one calibration point the
   absorption thesis rested on does not say what it was read as saying.

`genuinely_disjunctive` — 36% of survivor residuals — remains the floor no absorption technique
in this family touches, and on this evidence the tail is not primarily an absorption problem.

---

## Raw data

`exp-on.tsv` (131 ON rows), `verify.tsv` (serial re-verification + 10 OFF controls),
`census167.tsv` (this binary's census over the survivors) — scratchpad only, not committed;
the tables above reproduce every number that carries an argument.


---

## Follow-up (2026-08-02): the NO-GO's weak step, tested

The NO-GO was challenged on a fair point, so the extrapolation behind it was tested rather
than restated.

**The weak step.** "31 more ontologies reach zero residuals × a 1-in-54 recovery rate ≈ 0.6
recoveries" applies a rate measured on **Group Z** — median **36** residuals, all
domain-shaped — to a set that is materially different:

| | Group Z (where 1/54 was measured) | the 31 qualified-∃ extras |
|---|---|---|
| median residuals | 36 | **80** |
| max | 2,452 | **13,511** |
| ≥100 residuals | — | **15 of 31** |

`ore_ont_2874` and `ore_ont_2738` carry **13,511 / 13,509 residuals that are 100%
qualified-∃ with ZERO domain-absorbable** — precisely the ontologies qualified-∃ absorption
exists to serve, and ones domain absorption cannot touch. The experiment never drove any
high-residual ontology to zero, so the transfer was untested.

> **CORRECTION 2026-08-02 — THE ONE-DIRECTIONAL INFERENCE BELOW IS UNSOUND.** An adversarial
> review caught it. Deletion is semantically *weaker*, not computationally *stronger*: removing
> axioms turns cheaply-proved subsumptions into non-subsumptions that must be **refuted**
> (proved satisfiable), and removes clashes that would have terminated branches early. Since
> refutation is where this reasoner's cost lives, the deleted arm can DNF for work the absorbed
> arm would never do. **So "no rescue under deletion" does NOT imply "no rescue under
> absorption", and the 2874/2738 result below does not carry the weight assigned to it.**
>
> The qualified-`∃` NO-GO still stands, on its two *independent* legs — residual count scoring
> **AUC 0.480 (below chance)** against a completing contrast group, and the Group Z/N experiment
> run with a **real absorption implementation** (1/54 vs 3/77). Neither uses the deletion
> inference. But this leg is retracted, and any future plan proposing a deletion-based
> falsification must first show the cut arm's *cost profile* improved (timed-out pairs or branch
> counts down); otherwise a non-rescue is uninformative rather than confirming.

**The test.** Deletion is strictly STRONGER than absorption — absorption removes the residual
*while preserving semantics* — so if deleting the axioms does not rescue, absorption provably
cannot. One-directional, and immune to the two-things-changed confound that invalidated this
document's original calibration.

| ontology | classes | residuals cut | uncut | cut |
|---|---:|---|---|---|
| `ore_ont_2874` | 51,810 | **13,511 → 42** | DNF @120 s | **DNF @120 s** |
| `ore_ont_2738` | 45,756 | **13,509 → 40** | DNF @120 s | **DNF @120 s** |

**At the extreme the NO-GO holds by measurement, not extrapolation.** Caveat: both are very
large, so scale may dominate independently of residuals.

**The mid-range is NOT tested, and the attempt failed honestly.** Four extras with 136–339
residuals were probed and **the intervention did not fire** — residuals were unchanged
(339→339, 148→148, 136→136, 136→136), so `cut` was byte-equivalent to `uncut` and those runs
are evidence of nothing. Their qualified-∃ residuals arise from a shape the grep-level cut does
not match. Recorded rather than reported as four more confirmations, which is what they would
have looked like at a glance.

**Status of the NO-GO.** It stands on the balance of evidence — the motivating argument
(residual volume, zero-reachers) is refuted, the extreme cases are measured, and no instance
has yet been shown to recover from removing qualified-∃ residuals. But it is **weaker than
"documented NO-GO" implies**: the 136–339 residual band remains untested. What would settle it
is a precise cut (or the implementation itself) applied to `ore_ont_14551` — 2,755 classes, the
only small one in that band, and therefore the one case where a recovery could not be dismissed
as scale.

---

## Two-factor test (2026-08-02): residuals are NOT the discriminator — disjunctive concept rules are

The `3281` split showed cost came from the *interaction* of residual disjunctions and
disjunctive concept rules (either alone: 0.02 s; both: 8.84 s). That suggested coupling domain
absorption with a second technique. Tested on a population instead of an instance.

**Design.** All 167 DNF survivors (161 measurable) against a **contrast group of 180 ontologies
that COMPLETE** (seeded sample of the 1,607 `ok` at 60 s). A correlation within the DNF set
alone would have been meaningless — every member has the same outcome by construction.

**AUC = P[DNF value > OK value]; 0.5 is no separation.**

| factor | AUC | OK median | DNF median |
|---|---:|---:|---:|
| `residual_gcis` | **0.585** | 28 | 36 |
| **`concept_rule_or`** | **0.849** | **0** | **294** |
| product (resid × rule_or) | 0.759 | 0 | 3046 |
| `concept_rules` (size) | 0.783 | 3,121 | 41,940 |

**Controlled for size**, since DNF ontologies are ~13× larger and would have more of everything:

| normalised factor | AUC |
|---|---:|
| `rule_or / concept_rules` | **0.787** |
| `residual_gcis / concept_rules` | **0.480** — *below chance* |

And size-matched within bands, where the effect must survive if it is real:

| band (`concept_rules`) | n (OK/DNF) | `rule_or` AUC | `residuals` AUC |
|---|---|---:|---:|
| 1k–10k | 74 / 24 | **0.808** | 0.603 |
| 10k–60k | 27 / 45 | **0.815** | 0.766 |
| 60k+ | 11 / 70 | **0.880** | 0.722 |

**56% of completing ontologies have ZERO disjunctive concept rules, against 15% of DNF.**

### Conclusions

1. **Residual GCIs are definitively not the discriminator.** Per unit size the signal is
   *0.480* — literally none. This closes the absorption-of-residuals thesis on population
   evidence, independently of the instance-level refutation above, and retires
   qualified-`∃` absorption as a target: it optimises a variable that does not separate.
2. **Coupling does not help.** The product (0.759) is **worse** than `concept_rule_or` alone
   (0.849). Multiplying a strong factor by a null one dilutes it. The interaction seen on
   `3281` is real for that instance but is not the population mechanism.
3. **`concept_rule_or` is the discriminator**, and it survives every size control
   (0.81–0.88 within bands). That is the target.

### What this implies for what to build

**Binary absorption** (Hudek & Weddell) is precisely the technique that reduces
`concept_rule_or`: `as_trigger` picks one `¬Atomic` from `A ⊓ B ⊑ C` and leaves
`A → (¬B ⊔ C)` firing as a disjunction on every `A`-node. The census already sized the
population — **34,667** `Or`-conclusion concept rules still carrying a `¬Atomic`, of 199,019.
It is also a far smaller build than qualified-`∃`, needing no backward role rule.

**Do not build it yet.** AUC 0.85 is discrimination, not causation, and this project has just
been burned by exactly that inference. The next step is the same one-directional falsification
that worked before: on a high-`rule_or` ontology, **delete** those rules (strictly stronger
than absorbing them) and see whether it completes. No rescue under deletion ⇒ no rescue under
absorption, and binary absorption dies cheaply too.
