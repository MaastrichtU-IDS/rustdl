# Guard-manufacturability census: how large is the population Konclude's second guard would reach?

**Date:** 2026-08-04
**Status:** report-only instrument + corpus census. **No reasoning behaviour changed.**
**Reads on:** `docs/2026-08-04-konclude-cardinality-mechanism.md` (the measured peer mechanism),
`docs/2026-08-04-absorption-on-10019-is-fully-blocked.md` (whose correction banner says the existing
census predicate measures the wrong population),
`docs/benchmarks/2026-08-01-residual-absorbability-census.tsv` (the old predicate's population).

Instrument: `crates/owl-dl-core/src/residual_absorbability.rs`, surfaced by
`rustdl residual-absorbability [--tsv]`.

---

## 0. What was wrong with the shipped counter

`concept_rule_or_with_extra_not_atomic` counts a disjunctive-conclusion `ConceptRule` as a
binary-absorption candidate only if its conclusion still carries a **second `Not(Atomic)` disjunct**
— i.e. only if a second guard was **already written** as an atomic conjunct of the definition. On
`ore_ont_10019` that reads **0 of 29**.

Konclude does not *find* a second guard; it **manufactures** one. For a body conjunct `∃r.F` it mints
a fresh marker `T` and emits `F ⊑ ∀r⁻.T`, so `T` reaches exactly the nodes having an `r`-successor in
`F`; the absorbed rule then takes `T` as its second guard, and the `≤n` halves stay negated in the
head. Measured consequence: **all 47** of Konclude's absorbed rules have 2 guards and **0** fire on a
bare node, against rustdl's 1 guard and **10** firing on every bare `CarbonAtom`.

So the counter to build asks a different question: **did the definition body carry a conjunct that
entails `∃r.F`?**

---

## 1. The instrument, and the one place the brief's predicate had to be extended

Post-NNF, in the absorbed `Or` conclusion, the body conjunct appears negated. The instrument
(`GuardManufacturability::classify`) sorts each disjunct:

| body conjunct | negated, NNF | tier |
|---|---|---|
| `∃r.F`, `F` a **named atomic** class | `All(r, Not(Atomic F))` | **A** — the headline |
| `≥1 r.F` (incl. the `≥` half of `=1 r.F`), `F` named atomic | `Max(0, r, F)` | **A** |
| `≥k r.F`, `k ≥ 2`, `F` named atomic | `Max(k-1, r, F)` with `k-1 ≥ 1` | **B** |
| `∃r.F` / `≥k r.F` with `F` **complex** (e.g. `SulfurAtom ⊓ ∃hDBW.O ⊓ ≥2 hDBW.O`) | `All(r, Or(…))`, `Max(k, r, And(…))` | **C** |
| `≤n r.F` (incl. the `≤` half of `=n r.F`) | `Min(n+1, r, F)` | **never qualifies** |
| `∃r.⊤` / `≥k r.⊤` | `All(r, ⊥)` / `Max(k, r, ⊤)` | **never counted** |

**The `Min` exclusion is the load-bearing part.** `≤n r.F` does not entail `∃r.F` — a node with no
`r`-successor satisfies it — so no marker can reach a node from it and no guard is manufacturable.
Getting this wrong is what would produce a too-optimistic population; see the sabotage in §2.

`Max(k, r, ⊤)` and `All(r, ⊥)` are excluded from every tier because the marker rule would be
`⊤ ⊑ ∀r⁻.T`, which marks every node with an `r`-predecessor and guards nothing.

**Named vs synthetic.** A class is **synthetic** iff its vocabulary IRI starts with `urn:rustdl-`
(the reserved namespace covering `urn:rustdl-dkey:` concrete-domain keys, `urn:rustdl-anon:`
anonymous individuals, `urn:rustdl-aux-role:` chain aux roles). Everything else is a source-ontology
class. **All counts below are named-only**; a rule qualifying only through a synthetic filler is
counted in its own column. That column is **0 on `ore_ont_10019`** but not corpus-wide — 196 rules
across 12 pool ontologies, 32 rules across 2 tail ontologies (§3.2) — so the distinction was
necessary rather than decorative, even though its volume is negligible (0.02% of rules).

### Why three tiers rather than one number

The brief specified a single predicate — `All(r, ¬F)` or `Max(0, r, F)` with `F` a **named** class.
Implemented exactly as specified, that reads **15** of 29 on `ore_ont_10019`, not the expected 26.
The expected 26 is **not wrong about Konclude** — Konclude does give all 26 conjunctive definitions a
second guard — but the predicate as written cannot reach it, for two reasons that are visible in the
ontology itself and that the brief's own mapping table elides:

1. **`=n`/`≥n` with `n ≥ 2` does not produce `Max(0, …)`.** `=2 hSBW.Alkyl` lowers to
   `Min(2) ⊓ Max(2)`, whose negation is `Max(1, …) ⊔ Min(3, …)`. So `EtherGroup`
   (`OxygenAtom ⊓ =2 hSBW.Alkyl`) and `TertiaryAmineGroup` (`NitrogenAtom ⊓ =3 hSBW.CarbonAtom`)
   have **no** `Max(0, …)` disjunct. They are nonetheless manufacturable: `≥2 r.F ⊨ ∃r.F`. That is
   **tier B**, 2 rules.
2. **9 of the 26 definitions have a *complex* filler**, not a named class — e.g.
   `SulfoxideGroup ≡ CarbonGroup ⊓ ∃hSBW.(SulfurAtom ⊓ ∃hDBW.O ⊓ ∃hSBW.CarbonGroup ⊓ ≥2 hSBW.CarbonGroup ⊓ =1 hDBW.O)`,
   and `Alkyl ≡ CarbonAtom ⊓ ∃hSBW.(CarbonAtom ⊔ HydrogenAtom)`. A guard is still manufacturable
   (`F ⊑ ∀r⁻.T` for the complex `F`), but the marker rule then needs a **multi-guard, recursive**
   absorption of its own — which is exactly the `TRIG283 ⊓ TRIG329 → TRIG331` marker composition
   measured in Konclude's absorbed TBox (`…-konclude-cardinality-mechanism.md` §2b). That is
   **tier C**, 9 rules.

15 + 2 + 9 = **26**. The tiers are kept separate because they cost different things to build: tier A
needs only single-triggered marker rules (a shape `absorb.rs` already emits) plus a multi-guard
`ConceptRule` at the consuming end; tier C additionally requires the guard-minting to recurse into
the filler, i.e. it presupposes the machinery it feeds.

---

## 2. Acceptance gate: PASSED, with the decomposition stated

Target: **26** guard-manufacturable of the 29 `Or`-conclusion rules on
`/data/dumontier/ore-run/pilot/ore_ont_10019.owl/canon.owx`, with a max shared-trigger count of
**10** (`CarbonAtom`).

```
$ rustdl residual-absorbability ore_ont_10019.owl/canon.owx
# concept_rules:                182
#   conclusion_is_or:           29
#   ..with extra ¬Atomic:       0  (binary-absorption candidates)   ← the OLD counter
#   ..guard_mfg tierA:          15  (∃r.F / ≥1 r.F, F named atomic)
#   ..guard_mfg tierB only:     2  (≥k r.F, k≥2, F named atomic)
#   ..guard_mfg tierC only:     9  (complex filler — recursive minting)
#   ..synthetic filler only:    0
#   ..guard_mfg any tier:       26                                  ← the GATE
# all_or_manufacturable tierA:  false
# all_or_manufacturable any:    false
# distinct_shared_triggers:     5  (≥2 disjunctive rules)
#   ..with ≥5:                  3
# max_rules_per_trigger:        10                                  ← the GATE
```

**26 of 29, max shared trigger 10 — both gate values reproduce.** The residual 3 are independently
confirmed to be the three `ObjectUnionOf` definitions (`CarbonGroup`, `HalogenAtom`, `HeteroAtom`),
read directly out of `canon.owx`: they are not conjunctive definitions at all, so their `Or`
conclusion is a genuine covering disjunction with nothing to guard on. `all_or_manufacturable` is
therefore `false` for `ore_ont_10019` under both readings — the ontology cannot reach "0 rules firing
on bare nodes" by this mechanism alone, which is a **stronger and more honest** statement than the
gate asked for.

The old-vs-new contrast on this one ontology: **0 → 26**.

### The negative control was sabotaged and it caught the sabotage

`min_k_ge_2_alone_is_not_guard_manufacturable` (in
`crates/owl-dl-core/src/residual_absorbability.rs`) asserts that a rule whose only cardinality
disjunct is `Min(k ≥ 2, r, F)` does **not** count. Sabotage: add a `ConceptExpr::Min` arm routing to
the same filler classifier as `Max`.

- **Result: caught.** That test, and only that test, failed (22 passed / 1 failed).
- The sabotaged build's reading on `ore_ont_10019` is **tierA 17, tierB 0, any 26** — the same total
  by coincidence, but with two tier-B rules mis-promoted into tier A. So on this ontology the
  sabotage is invisible in the headline total and visible only in the tier split; the *unit test* is
  what discriminates, not the acceptance gate. Worth recording: the gate alone would have passed a
  broken instrument.

Nine other unit tests cover the tier boundaries: `∀r.¬F`, `Max(0,r,F)`, `Min` not vetoing a
qualifying sibling (the `KetoneGroup` shape), tier B held out of the headline, tier C held out,
`⊤`/`⊥` fillers counted in **no** tier, synthetic fillers counted apart, and the shared-trigger
arithmetic. `cargo fmt`, `cargo clippy --workspace --all-targets --all-features -- -D warnings` and
`cargo test -p owl-dl-core` are clean (23 tests in this module).

---

## 3. The census

Binary pinned at `scratchpad/rustdl-guardmfg-CENSUS` immediately after the build, and the acceptance
gate re-verified **against the pin** before the sweep. 120 s cap per invocation, 8-way concurrency.
Raw data committed as `docs/benchmarks/2026-08-04-guard-manufacturable-census.tsv` (one row per
ontology; columns after the stem and status:
`residual_gcis, domain, binary, nominal, card_n_gt_1, qualified, genuinely, concept_rules, or,
extra_not_atomic, mfg_A, mfg_B_only, mfg_C_only, mfg_synth_only, shared_ge2, shared_ge5,
max_per_trigger`).

Populations: the **whole pool** (`/data/dumontier/ore-run/pool_sample/files`, 1,920 `.owl`) and the
**v0.4.14 DNF tail** (`baselines/2026-08-04-tail-v0414-list.txt`, 151, all of which are in the pool
— so one sweep serves both).

### 3.0 Conversion outcomes — reported, not dropped

| | pool (1,920) | tail (151) |
|---|---:|---:|
| conversion OK | **1,914** | **146** |
| conversion TIMEOUT @120 s | **5** | **5** |
| parse FAIL | **1** | 0 |

All 5 timeouts (`ore_ont_10929`, `2504`, `4141`, `4572`, `8445`) are **tail** members — unsurprising,
since a conversion-bound ontology is a DNF candidate for that reason alone. The single parse failure
(`ore_ont_10860`) is a pre-existing horned-owl limitation, not a census defect: the file contains
SWRL `DLSafeRule` axioms and the parser rejects `BuiltInAtom`'s `DArg`. Every percentage below is
over the OK rows.

### 3.1 Population counts, old predicate vs new

| | pool | tail |
|---|---:|---:|
| `concept_rule_or > 0` (the mechanism can apply at all) | **888** | **120** |
| …of which `or == 0`, mechanism structurally N/A | 1,026 | 26 |
| **OLD** `extra ¬Atomic > 0` | **285** | **55** |
| **NEW** tier A `> 0` | **455** | **116** |
| **NEW** any tier `> 0` | **489** | **116** |
| …newly visible (any tier > 0 **and** OLD == 0) | **332** | **62** |
| `or > 0` but any tier == 0 (genuinely inert) | **399** | **4** |

The OLD pool figure of **285** reproduces the committed
`docs/benchmarks/2026-08-01-residual-absorbability-census.tsv` count exactly — the cheapest available
check that this sweep is over the same population with the same old counter.

**The under-count is large and it is worst exactly where it matters.** On the tail the old predicate
saw 55 of 120 candidates; the new one sees **116 of 120**. Counted in *rules* rather than ontologies
the gap is starker still:

| | pool | tail |
|---|---:|---:|
| total `Or`-conclusion rules | 1,054,027 | 648,373 |
| covered, any tier | **841,355 (79.8%)** | **603,819 (93.1%)** |
| covered, OLD predicate | 199,019 (18.9%) | **34,591 (5.3%)** |

Per-ontology coverage (share of that ontology's `Or` rules that are manufacturable): pool median
**33.3%**, tail median **98.3%**; ≥90% coverage on **301 of 888** pool and **81 of 120** tail
ontologies.

### 3.2 Distribution of the manufacturable count

Over ontologies with a non-zero count:

| | min | median | p90 | max |
|---|---:|---:|---:|---:|
| pool, tier A (n=455) | 1 | 196 | 4,313 | 30,481 |
| pool, any tier (n=489) | 1 | 144 | 4,311 | 30,481 |
| pool, OLD (n=285) | 1 | 6 | 563 | 30,509 |
| tail, tier A (n=116) | 2 | 2,269 | 15,928 | 30,481 |
| tail, any tier (n=116) | 2 | 2,269 | 15,928 | 30,481 |
| tail, OLD (n=55) | 1 | 5 | 874 | 12,881 |

Tier mix, in rules: pool A 827,201 / B-only 154 / C-only 14,000 / synthetic-only 196; tail A 592,840
/ B-only 147 / C-only 10,832 / synthetic-only 32. **Tier A carries ~98% of the volume**, which is the
useful result for scoping — the cheap tier is the one that pays. Tier C is nonetheless present on
**157** pool / **39** tail ontologies, and the synthetic-only column is small but **not** zero (12
pool / 2 tail ontologies), so keeping named and synthetic apart was necessary rather than decorative.

### 3.3 Shared triggers — the quantity that makes the cost quadratic

| | pool | tail |
|---|---:|---:|
| has a trigger with **≥2** disjunctive rules | **548** | **114** |
| has a trigger with **≥5** disjunctive rules | **422** | **101** |
| max rules on one trigger (median / p90 / max, over the ≥2 set) | 30 / 678 / **25,022** | 166 / 6,102 / **18,323** |
| distinct shared triggers (median / p90 / max, over the ≥2 set) | 27 / 389 / 3,773 | 268 / 1,168 / 3,773 |

`ore_ont_10019`'s `CarbonAtom` at **10** is at the very mild end of this distribution. The tail
contains ontologies where a **single** class opens 6,102 and 18,323 disjunctions on a bare node.

### 3.4 THE HEADLINE — how many go from "N firing on bare nodes" to "0"

`guard_manufacturable == concept_rule_or`, i.e. every disjunctive rule gets a second guard:

| | pool (of 888 with or>0) | tail (of 120 with or>0) |
|---|---:|---:|
| **any tier** | **208** | **26** |
| tier A only | 188 | 18 |
| **OLD predicate** | 128 | **1** |
| any tier **and** a shared trigger ≥2 | 190 | 26 |
| any tier **and** a shared trigger ≥5 | 171 | 26 |

**On the DNF tail the old predicate found 1 ontology; the corrected predicate finds 26** — and all 26
also carry a shared trigger with ≥5 disjunctive rules, so in every one of them the mechanism both
applies completely and applies to the pattern that costs. Their `Or`-rule counts run from 197 to
15,928 (median 3,609).

`ore_ont_10019` is **not** in that set: 26 of 29, the residual 3 being the `ObjectUnionOf`
definitions. Consistent with §2 and with the standing instruction that a build must be justified on a
population rather than on that ontology.

### 3.5 The gated set (tail, full coverage, shared trigger ≥5)

```
ore_ont_11495  or= 7682  A= 7682  C=    0  shared≥2= 292  max/trig=6102
ore_ont_4669   or= 7515  A= 7515  C=    0  shared≥2= 268  max/trig=6102
ore_ont_7127   or=11598  A=11598  C=    0  shared≥2= 284  max/trig=6102
ore_ont_7581   or= 7515  A= 7515  C=    0  shared≥2= 268  max/trig=6102
ore_ont_7956   or= 7515  A= 7515  C=    0  shared≥2= 268  max/trig=6102
ore_ont_3575   or=15928  A=15928  C=    0  shared≥2= 605  max/trig=4704
ore_ont_5218   or=15928  A=15928  C=    0  shared≥2= 605  max/trig=4704
ore_ont_11745  or=14846  A=14846  C=    0  shared≥2= 548  max/trig=4619
ore_ont_11037  or= 3046  A= 3046  C=    0  shared≥2=  59  max/trig= 850
ore_ont_3794   or=11194  A= 9426  C= 1768  shared≥2=1296  max/trig= 512
ore_ont_14572  or= 9966  A= 7656  C= 2310  shared≥2=1168  max/trig= 506
ore_ont_7361   or= 9966  A= 7656  C= 2310  shared≥2=1168  max/trig= 506
ore_ont_9724   or= 9966  A= 7656  C= 2310  shared≥2=1168  max/trig= 506
ore_ont_11629  or= 3608  A= 3395  C=  213  shared≥2= 396  max/trig= 399
ore_ont_9855   or= 3611  A= 3396  C=  215  shared≥2= 397  max/trig= 397
ore_ont_8194   or=  315  A=  315  C=    0  shared≥2=   3  max/trig= 259
ore_ont_2111   or= 1173  A= 1173  C=    0  shared≥2=  77  max/trig= 234
ore_ont_6608   or= 2110  A= 2110  C=    0  shared≥2= 325  max/trig= 149
ore_ont_9944   or= 2110  A= 2110  C=    0  shared≥2= 325  max/trig= 149
ore_ont_11311  or= 1877  A= 1877  C=    0  shared≥2= 268  max/trig= 148
ore_ont_239    or= 1877  A= 1877  C=    0  shared≥2= 268  max/trig= 148
ore_ont_9739   or= 1877  A= 1877  C=    0  shared≥2= 268  max/trig= 148
ore_ont_12432  or= 1044  A=  632  C=  412  shared≥2= 160  max/trig= 105
ore_ont_4412   or=  642  A=  642  C=    0  shared≥2= 131  max/trig=  78
ore_ont_13846  or=  281  A=  281  C=    0  shared≥2= 112  max/trig=  16
ore_ont_15687  or=  197  A=  154  C=   43  shared≥2=  40  max/trig=   8
```

**17 of these 26 need tier A only** (`C == 0`), i.e. only single-triggered marker rules plus a
multi-guard `ConceptRule` — no recursive minting.

Just outside full coverage, and worth carrying because they are the largest instances, are the
≥99%-coverage tail ontologies: `ore_ont_2874` 22,578/22,627, `2738` 17,342/17,356, `9835`
17,927/17,953, `9663` 13,470/13,488, `13242` 13,205/13,223, `7646` 12,198/12,219, `2302` 9,168/9,169.
Each would go from tens of thousands of bare-node disjunctions to a double-digit residue.

### 3.6 Where the mechanism is inert

**399 pool ontologies have `or > 0` and nothing manufacturable at all** — for those, no variant of
this lever helps, and 1,026 pool ontologies have no disjunctive-conclusion rule in the first place.
On the tail the inert set is only **4** (`ore_ont_10949` or=322, `8475` or=272, `934` or=6, `20`
or=5) plus **26** with `or == 0`.

---

## 4. Threats to validity

- **This is a static count of an absorbed TBox, not a measurement of reasoning.** It says a second
  guard is *derivable*, not that deriving it makes any of these 26 ontologies classify. A rule with
  two guards still opens its disjunction when both guards land, and Konclude's own numbers are
  consistent with the disjunction remaining — what changes is *how often* it opens. A build must be
  gated on a real two-arm sweep plus the corpus-scale MISSED net, per this project's standing rule;
  nothing here substitutes for either.
- **The mechanism has a cost this census does not price.** Minting a marker per `∃r.F` conjunct grows
  the class signature. On `ore_ont_3575` that is up to 15,928 new markers and 15,928 new
  `∀r⁻.T`-propagating rules. Konclude mints 89 on `ore_ont_10019` against rustdl's 26 heads, so the
  ratio is not 1:1 in its implementation either, but the direction is clear: this trades label-space
  and role-propagation work for branching. **On this population that trade is unmeasured.**
- **Soundness of the marker rules is asserted, not verified here.** `F ⊑ ∀r⁻.T` needs the *inverse*
  role in general (`∀r.T` suffices only when `r` is symmetric, which happens to hold for all five
  roles of `ore_ont_10019` and is not general), and the id-space / told-table / fragment-gate /
  output-leak hazards flagged in `docs/reviews-2026-08-04/R1-technical.md` for surrogate minting are
  untouched by this census.
- **Tier C is counted as manufacturable on a structural argument, not a measurement.** It presupposes
  the multi-guard machinery it feeds. If tier C turned out not to work, the tail headline drops from
  26 to 18 and the pool from 208 to 188 — so the conclusion does not rest on it.
- **`max_per_trigger` counts rules per trigger, not firings.** An ontology whose 6,102-rule trigger is
  never labelled pays nothing. Which triggers are actually reached is a runtime question.

## 5. Method notes

- **The acceptance gate alone would have passed a broken instrument.** The `Min`-counting sabotage
  left the gate's headline total at 26 and moved only the tier split. The unit test caught it. If a
  gate is a single aggregate, it cannot discriminate compensating errors inside the aggregate.
- **Reproducing an existing number is the cheapest validation available.** The old predicate reading
  **285** on the pool — matching the committed census byte for byte — is what makes the new numbers
  from the same sweep believable.
- **A specified predicate can be right in intent and unreachable as written.** The brief's `Max(0,…)`
  mapping is correct only for `n = 1`; `=2`/`=3` conjuncts produce `Max(1,…)`/`Max(2,…)`, and 9 of
  `ore_ont_10019`'s definitions have complex fillers that no named-class predicate can see. Rather
  than adjust the target to the output or the output to the target, both are reported with the
  decomposition that reconciles them.
- **Report conversion failures as a row.** 5 of the 6 non-OK ontologies are tail members; silently
  dropping them would have inflated every tail percentage.


---

## 6. Verdict

**The addressable population justifies building the lever — on the strength of the tail numbers, not
the pool numbers.**

The pool picture is mixed: 489 of 1,914 ontologies have something manufacturable, but only 208 reach
full coverage and 1,425 are either inert or have no disjunctive rule at all. That is a lever with a
population, not a lever with a mandate.

The **tail** picture is not mixed. Of the 120 v0.4.14 DNF-tail ontologies that have a disjunctive
absorbed rule, **116 have manufacturable guards**, **93.1% of their 648,373 disjunctive rules are
covered**, the median ontology is at **98.3% coverage**, and **26 reach 100% with a shared trigger of
≥5** — against **1** under the shipped predicate. 101 of them have a class that opens ≥5 disjunctions
on a bare node, some as many as 18,323. This is the first mechanism in the recent record whose
addressable set on the DNF tail is measured in the dozens rather than in single digits, and it is the
mechanism the peer reasoner is measured to use.

**Gate it on the 26 in §3.5**, prioritising the 17 that need tier A only (`C == 0`) — in particular
`ore_ont_3575`/`5218` (15,928 rules), `11745` (14,846), `7127` (11,598), `11495` (7,682) and
`4669`/`7581`/`7956` (7,515). Carry the seven ≥99%-coverage ontologies in §3.5's second paragraph
(`2874`, `2738`, `9835`, `9663`, `13242`, `7646`, `2302`) as secondary targets, since they are the
largest instances even though a small residue survives.

**Do not gate it on `ore_ont_10019`** (26 of 29; its residual 3 are covering disjunctions this
mechanism cannot touch, and its unique remaining completeness prize is one subsumption), and do not
treat this census as evidence the lever *works*: it establishes only that the input the mechanism
consumes is present at scale. The go/no-go still needs a two-arm 1,920-ontology sweep for `ok → dnf`
regressions plus the corpus-scale MISSED net, and the soundness work on inverse-role marker
propagation that §4 lists as untouched.
