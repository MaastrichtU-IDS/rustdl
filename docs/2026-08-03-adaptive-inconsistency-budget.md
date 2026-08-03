# Adaptive classify inconsistency budget (2026-08-03)

Replaces the flat `RUSTDL_CLASSIFY_INCONSISTENCY_MS=3000` default with a
two-level rule keyed on cheap structural predictors of the `ABox`-saturation
fixpoint's cost. **Default ON.** `RUSTDL_CLASSIFY_INCONSISTENCY_MS` remains an
explicit override that always wins, including `0` for unbounded.

Code: `adaptive_classify_inconsistency_budget_ms` / `AboxCostPredictors` /
`abox_cost_predictors` in `crates/owl-dl-reasoner/src/lib.rs`.
Canaries: `crates/owl-dl-reasoner/tests/adaptive_inconsistency_budget.rs`.
Measurement harness: `crates/owl-dl-reasoner/examples/abox_precheck_probe.rs`.
Raw data: `docs/data-2026-08-03-adaptive-inconsistency-budget.csv` (1145 rows —
the full population plus `family.ofn`) and
`docs/data-2026-08-03-adaptive-inconsistency-budget-axioms.csv` (the targeted
re-run carrying the lowered-axiom-count column added in §5).

## 1. The problem, with both ends re-measured

`classify_inconsistency_precheck` runs an `ABox`-saturation fixpoint over named
individuals before every classify. Both ends of the flat 3000 ms budget were
uncomfortable:

* **Too tight for the case it exists for.** `family.ofn`'s pre-check costs
  **2585–2619 ms** measured *in isolation*, and the classify-level detection
  flips between **2600 and 2700 ms** (measured by sweeping
  `RUSTDL_CLASSIFY_INCONSISTENCY_MS` on a pinned v0.4.12 binary: 2400/2500/2600
  → `consistent: true`, 0 unsat; 2700/2800/3000 → `consistent: false`, 58 unsat).
  So 3000 ms carried **~1.11–1.16× headroom**. A host 15% slower silently loses
  the detection v0.4.11 shipped to provide.
* **Too loose for big `ABox`es.** Unbounded, this pre-check took
  `ore_ont_{10838,15846,16315,3087}` from ~1.3–4.4 s to DNF at 60 s.

> **Retraction carried forward.** The earlier "`family` needs ~2.0 s / 1.5×
> headroom" figure was a confounded subtraction — classify *with* the pre-check
> (2.67 s) minus *without* (0.67 s). A clash **short-circuits** the rest of
> classify, so that difference measures "pre-check minus the classify it
> replaced", not the pre-check. Every number in this document comes from the
> isolation probe, which times `saturate_abox_consistency_bounded` directly.

## 2. Method

`examples/abox_precheck_probe.rs` parses one ontology, converts it, reads the
candidate predictors off the lowered `InternalOntology`, then times
`saturate_abox_consistency_bounded` alone. One CSV row per ontology.

Population: every `ABox`-bearing ontology in the ORE 2015 pool
(`/data/dumontier/ore-run/pool_sample/files`, 1920 files; **1144 carry
`ClassAssertion` or `ObjectPropertyAssertion`**), plus `family.ofn`. Each run
capped at a 5000 ms pre-check budget and a 60 s wall, `ulimit -v 24 GB`, serial,
`nice`. **1137 of 1144 measured**; 7 excluded — 6 exceed the 60 s wall inside
`convert_ontology` (before the pre-check runs at all: `ore_ont_{2504,4572,10929,
15635,4141,8445}`) and 1 fails to parse (`ore_ont_10860`).

Coverage argument: the pre-check is `has_abox_axioms`-guarded, so an `ABox`-free
ontology cannot be reached by this rule at all. **The `ABox`-bearing set is the
complete affected population**, and 99.4% of it was measured.

## 3. Which predictors track cost

Candidates: named-individual count, `ClassAssertion` count,
`ObjectPropertyAssertion` count, role-chain-rule count,
`TransitiveObjectProperty` count, functional/inverse-functional role count.

**Global rank correlation is the wrong statistic here and would have misled.**
Spearman against pre-check wall over all 404 first-scan rows gives
`class_assertions` **+0.863** and `individuals` **+0.856**, both *higher* than
the proxy this design ends up using (+0.462). That ranking is produced by the
mass of sub-millisecond ontologies, where "bigger is slower" holds trivially. The
decision is about the **tail**, and in the tail those two predictors are refuted
outright:

| ontology | inds | ClassAssn | OPA | mult | work proxy | pre-check ms | edge adds |
|---|---|---|---|---|---|---|---|
| `ore_ont_4510` | 60,343 | 194,302 | 114,957 | 3 | 344,871 | 137 | 114,957 |
| `ore_ont_6233` | 176,043 | 176,043 | 0 | 0 | 0 | 17 | 0 |
| `ore_ont_11110` | 81,052 | 81,052 | 0 | 1 | 0 | 33 | 0 |
| `ore_ont_1579` | 129,647 | 111,578 | 78,441 | 55 | 4,314,255 | 1502 | 739,463 |
| `ore_ont_8480` | 24,910 | 19,184 | 10,462 | 55 | 575,410 | 314 | 139,295 |
| `ore_ont_3816` | 5,162 | 29,818 | 66,921 | 18 | 1,204,578 | 185 | 190,698 |
| **`family.ofn`** | **508** | **521** | **1,337** | **38** | **50,806** | **2,585** | **267,112** |

`ore_ont_4510` carries **86× more** `ObjectPropertyAssertion` than `family.ofn`
and is **19× faster**. So **`ABox` size does not track cost — and the direction
the intuition suggests (scale the budget up with size) is not merely useless but
actively harmful**: it would starve the 508-individual ontology the pre-check
exists for and subsidise the ontologies whose DNF the budget exists to prevent.

`mult` = role-chain rules + transitive roles. What separates 4510 from family is
that 4510 has no edge-*multiplying* rule, so its derived closure equals its
asserted edge set exactly (`edge_adds == opa`), while `family` turns 1337
asserted edges into 267,112. Hence the work proxy:

```text
work_proxy = asserted_edges × max(multiplying_rules, 1)
```

The `max(1)` is load-bearing: without it any chain-free ontology scores 0
regardless of size, so the 2.8 M-assertion `ore_ont_7192` would score 0.

### Separation, scored on the tail rather than by correlation

Best single threshold per predictor, requiring *every* expensive ORE ontology
strictly above the line and `family.ofn` at or below it (404-row first scan):

| predictor | best threshold | cheap ontologies wrongly put stingy |
|---|---|---|
| `individuals` | 508 | 222 |
| `class_assertions` | 521 | 222 |
| `opa` | 1,337 | 58 |
| `mult` | **none exists** | — |
| **`work_proxy`** | **50,806** | **33** |

Several predictors *can* separate; `work_proxy` separates most cheaply, and it is
the only one whose separation follows from a mechanism (closure growth) rather
than coincidence. Note every "best threshold" lands exactly on `family`'s own
value — the separation is real but has no margin at that position, which is why
the threshold is placed in the gap instead (§4).

## 4. The rule

```text
work_proxy = asserted_edges × max(multiplying_rules, 1)
budget     = work_proxy ≤ 300_000 ? 12_000 ms : 3_000 ms
```

* **`INCONSISTENCY_WORK_THRESHOLD = 300_000`.** Every ontology expensive *for
  fixpoint reasons* scores ≥ **2,047,210** (`ore_ont_16315`); `family.ofn` scores
  **50,806**; **the 40× gap between them is empty**. 300,000 is that gap's
  log-scale balance point — `family` clears it by 5.9×, the cheapest expensive
  ontology exceeds it by 6.8×. The position is biased slightly low on purpose:
  landing too low costs an ontology only *today's* behaviour (not a regression),
  while landing too high hands a runaway fixpoint the full generous budget.
* **`INCONSISTENCY_GENEROUS_MS = 12_000`.** 4.6× `family`'s measured 2585 ms
  (4.4× its 2700 ms flip point).
* **`INCONSISTENCY_STINGY_MS = 3_000`** — deliberately **identical to the
  superseded flat default**, so the rule can only ever *raise* a budget. Every
  ontology outside the low-work class is bounded bit-identically to today, which
  is what keeps the four DNF regressions at the walls the flat budget bought them.

**Why two levels and not a formula.** The proxy separates the tail but its
magnitude does not predict milliseconds, and the refutation is exact:
`ore_ont_1579` and `ore_ont_15846` have **identical** predictors (78,441 asserted
edges, 55 multiplying rules, proxy 4,314,255) and cost **1502 ms** and **>5000 ms**
respectively. A formula would read precision the measurement does not contain.

## 5. A second cost driver — found by extending the scan, and deliberately not modelled

The first version of this analysis ran on 409 ontologies (the `work/sym` subset)
and concluded that edge multiplication was **necessary** for expense. Extending
to the full 1137 refuted that:

| ontology | work proxy | pre-check ms | type adds | edge adds | lowered axioms |
|---|---|---|---|---|---|
| `ore_ont_5368` | 6,099 | >=5000 | **0** | **0** | **18,644,723** |
| `ore_ont_1833` | 10,865 | 4478 | 37,657 | 10,865 | **14,074,078** |
| `ore_ont_16632` | 9,730 | 1627 | 936 | 5,126 | **6,634,908** |
| `family.ofn` | 50,806 | 2585 | 1,455 | 267,112 | 18,873 |

`ore_ont_5368` does **zero** work and still costs 5.9 s. Its cost is the
fixpoint's **pre-indexing prelude**, which walks every lowered axiom — and it
carries 18.6 M of them (DKey disjointness flood; the same ontology that is the
`RUSTDL_DKEY_MERGING_GATE` discriminator). The rate is stable at ~0.3 µs/axiom
across all three prelude-dominated cases.

**The reflex is to add an axiom-count gate. Measurement says that would make
things worse.** The prelude runs before the first deadline probe, so its cost is
**budget-independent**:

| ontology | budget 3000 | budget 12000 | `timed_out` 3000 → 12000 |
|---|---|---|---|
| `ore_ont_1833` | 4065 ms | 4023 ms | `true` → **`false`** |
| `ore_ont_5368` | 6059 ms | 5871 ms | `true` → **`false`** |

Same wall, but the larger budget converts work already paid for and then
discarded into a completed fixpoint. Gating on axiom count would push both back
into the stingy branch and strictly worsen them. Pinned by
`prelude_dominated_predictors_stay_generous`.

**Precisely what "recovered" means here, since it is easy to overclaim:** both
ontologies are *consistent*, so the completed fixpoint returns `clash: false` —
the same value the timeout returned. The reported classify output is therefore
**unchanged** (confirmed: `ore_ont_1833` byte-identical, §10). What changes is the
epistemic status: a definite "no clash at fixpoint" instead of an indeterminate
"abandoned, no verdict". The practical value is latent — it is the case where such
an ontology *is* inconsistent that the stingy branch would silently miss.

**Honest residual:** *no budget bounds the prelude*, so an 18 M-axiom ontology
overruns any budget by ~6 s. That is pre-existing and identical at 3000 ms; it is
a separate lever (make the prelude deadline-aware), not this one.

## 6. Soundness — verified, not inherited

The pre-check is a sound under-approximation and the budget cannot touch that.
Structurally: `saturate_abox_consistency_bounded`'s abandonment path returns
`clash: false` with `edges` and `derived_same` emptied
(`abox_saturation.rs:1153`), and `clash: false` is *already* the no-verdict answer
every caller handles. `clash` cannot be `true` there — a clash breaks the loop
before the next deadline probe. So no budget, larger or smaller or absent, can
manufacture an inconsistency; changing it costs at most the detection.

Only the classify path is bounded. `is_consistent` / `realize` / `materialize_*`
/ `diagnose` call the unbounded entry point, and the existing canaries
`is_consistent_ignores_the_classify_budget` and
`materialize_ignores_the_classify_budget` still guard that.

Verified rather than assumed:

* **Soundness control** — `{A ⊑ ⊥, B ⊑ ⊥}`: `consistent: true`, 2 unsat, on
  **both** binaries. All-named-classes-unsat is not inconsistency; the test is
  that `⊤` is unsat.
* **FP=0 net** — `./scripts/run-soundness-diff.sh`: see §8.

## 7. Gate results

Binaries pinned to uniquely named paths immediately after the builds that
produced them, and sha-verified: `rustdl-BASE-v0412` `00485df997c5446c…`,
`rustdl-ADAPTIVE-final` `fd2f58dca6cec0a0…`. A third binary
(`rustdl-ADAPTIVE-v2` `9fa37a4acaccd3e6…`) was built after the only post-pin
source edit — a doc-comment backtick required by `clippy::doc_markdown` — and
re-verified as a freshness check: `family.ofn` and `ore_ont_16315` outputs
byte-identical to the pinned adaptive binary, `16315` wall 4.32 s.

**Gate 1 — `family.ofn` headroom.**

| | pre-check | budget | headroom | classify wall (min-of-3) | verdict |
|---|---|---|---|---|---|
| before | 2585 ms (flip 2600–2700) | 3000 | **1.11–1.16×** | 2.69 / 2.71 / 2.70 s | `consistent: false`, 58 unsat |
| after | 2585 ms | **12000** | **4.4–4.6×** | 2.74 / 2.71 / 2.69 s | `consistent: false`, 58 unsat |

`classify --json` output **byte-identical** between binaries, and `rustdl
consistent ontologies/real/family.ofn` reports `inconsistent` — the two surfaces
agree. The wall does not change because a completing pre-check spends its cost,
not its cap.

**Gate 2 — the four DNF regressions**, 60 s cap, `RAYON_NUM_THREADS=1`:

| ontology | base | adaptive | classify `--json` |
|---|---|---|---|
| `ore_ont_10838` | 5.65 s | 5.43 s | byte-identical |
| `ore_ont_15846` | 8.12 s | 8.24 s | byte-identical |
| `ore_ont_16315` | 4.34 s | 4.39 s | byte-identical |
| `ore_ont_3087` | 5.03 s | 4.90 s | byte-identical |

All complete; walls within run-to-run noise, as they must be — all four score
above the threshold and receive exactly the old 3000 ms.

*Threat to validity:* the four are distinct files but produce **identical**
classification output (1919 direct subsumptions each). They share a TBox and
differ only in `ABox`, so they are effectively one TBox with four `ABox`es, not
four independent data points.

**Gate 3 — soundness control:** §6. **Gate 4 — override:** `MS=1` starves the
pre-check (`consistent: true`, 0 unsat — a sound MISS); `MS=0` runs it unbounded
(`consistent: false`, 58 unsat); `MS=77` is read back verbatim. **Gate 5 —
sabotage:** §8. **Gate 7 — spot check:** §8.

## 8. Verification

* `cargo fmt --all -- --check` — clean.
* `cargo clippy --workspace --all-targets --all-features -- -D warnings` — clean.
* `cargo test --workspace --exclude owl-dl-py --no-fail-fast` — **129 groups,
  1523 passed, 0 failed, 81 ignored** (`owl-dl-py` excluded for the pre-existing
  Python link failure; `--no-fail-fast` because fail-fast previously reported 18
  groups where the real figure is 129).
* `./scripts/run-soundness-diff.sh` — rc=0, 22 tests pass, **11 distinct closures
  VERIFIED and all exact**: galen 27997, notgalen 32739, sio 8904, ore-10908 6001,
  wine 653, pizza 499, alehif 247, ro 158, ore-15672 142, sulo 51, bibtex 16 —
  identical to the documented reference values. The 3 NOT VERIFIED
  (`ro-stripped`, `sulo-stripped`, `sio-stripped`) are the documented absent
  fixtures, unchanged by this work.

  *Evidence caveat:* the four DNF-regression ontologies and `family.ofn` are not
  in that net, so FP=0 here demonstrates **non-regression / inertness**. The
  gates that actually carry this change are §7 (the walls and byte-identity) and
  the sabotage table below.
* **Sabotage: 9 run serially, 8 caught on the first pass, 1 survived.** The
  survivor (`INCONSISTENCY_GENEROUS_MS` blown up to 1,000,000 ms — reintroducing
  an effectively unbounded pre-check through the low-work door) was closed by
  adding `generous_budget_is_bounded_above`, and the replay then failed. Counted
  as run **including the survivor**, per the discipline that a test written to
  guard a risk often does not guard it.

  | # | mutation | result |
  |---|---|---|
  | S1 | `GENEROUS` 12000 → 3000 (starve family) | caught (1) |
  | S2 | `THRESHOLD` 300k → 10k (below family's 50,806) | caught (2) |
  | S3 | swap the branches (budget grows with work) | caught (6) |
  | S4 | drop `max(1)` from `work_proxy` | caught (1) |
  | S5 | adaptive value shadows the env override | caught (2) |
  | S6 | score plain `SubObjectPropertyOf` as a multiplier | caught (1) |
  | S7 | drop `TransitiveRole` from the multiplier count | caught (1) |
  | S8 | `GENEROUS` → 1,000,000 ms | **SURVIVED** → guard added → caught (1) |
  | S9 | add an axiom-count gate (the §5 reflex) | caught (1) |

* **Release-only value test.** `real_family_classify_is_inconsistent` is
  `#[ignore]`d **with both reasons stated in the attribute**: the corpus is
  gitignored, *and* `family`'s pre-check costs **37.9 s in the unoptimized test
  profile** against ~2.6 s in release, so in a debug run it would measure the
  profile rather than the rule. Run it with
  `cargo test --release -p owl-dl-reasoner --test adaptive_inconsistency_budget -- --ignored`.
  `real_family_predictors_match_the_hard_coded_ones` is corpus-gated only and
  passes, so the constants cannot drift from the ontology they describe.

## 9. Population effect, and why no further sweep is needed

| | count | budget | effect |
|---|---|---|---|
| low-work (`proxy ≤ 300,000`) | 1102 | 12,000 ms | 1089 cost <500 ms — cap unobservable; 11 of the remaining 13 complete in ≤1627 ms — cap never binds; 2 are the prelude cases, same wall, verdict recovered |
| high-work (`proxy > 300,000`) | 35 | 3,000 ms | bit-identical to today; slowest *completing* member 2973 ms |

**Net effect over the whole measured population: no wall change and no outcome
change, except that `ore_ont_{1833,5368}` stop discarding a pre-check they had
already paid for.**

The rule *can* grant more than 3000 ms, so the caveat is stated plainly rather
than waved away — but the 1137-ontology `ABox`-bearing scan **is** the sweep for
this lever, because `has_abox_axioms` makes the `ABox`-bearing set the complete
affected population. A broader ORE-wide classify sweep would re-measure 776
`ABox`-free ontologies this rule provably cannot reach.

**Gate 7 spot check** (`RAYON_NUM_THREADS=1`, 120 s cap, base vs adaptive,
`classify --json` compared byte-for-byte) — 18 `ABox`-bearing ontologies chosen
as the highest-information set: the 12 low-work members whose pre-check costs
≥500 ms (where a budget difference could show at all), 4 large-`ABox` stingy-class
controls, and 2 clash-bearing ontologies to check that detection is preserved.
Results in §10.

## 10. Gate 7 results

18 ontologies, `RAYON_NUM_THREADS=1`, 120 s cap, `ulimit -v 24 GB`, serial,
base then adaptive back-to-back. Threshold for a "material slowdown": >25% **and**
>2 s.

| ontology | base | adaptive | delta | `classify --json` |
|---|---|---|---|---|
| `ore_ont_16632` | 17.15 s | 17.93 s | +4.5% | identical |
| `ore_ont_11126` | 15.99 s | 16.43 s | +2.8% | identical |
| `ore_ont_1270` | 3.68 s | 3.77 s | +2.4% | identical |
| `ore_ont_15680` | 2.77 s | 2.80 s | +1.1% | identical |
| `ore_ont_4609` | 2.92 s | 2.76 s | −5.5% | identical |
| `ore_ont_15655` | 34.26 s | 34.54 s | +0.8% | identical |
| `ore_ont_10425` | 22.90 s | 21.49 s | −6.2% | identical |
| `ore_ont_13052` | 10.18 s | 11.16 s | +9.6% | identical |
| `ore_ont_10125` | 16.63 s | 16.79 s | +1.0% | identical |
| `ore_ont_5857` | DNF@120 | DNF@120 | — | both DNF (pre-existing) |
| `ore_ont_4412` | DNF@120 | DNF@120 | — | both DNF (pre-existing) |
| `ore_ont_1833` | 83.59 s | 87.35 s | +4.5% | identical |
| `ore_ont_4510` | 2.68 s | 2.67 s | −0.4% | identical |
| `ore_ont_6233` | 6.66 s | 6.53 s | −2.0% | identical |
| `ore_ont_11110` | 1.53 s | 1.51 s | −1.3% | identical |
| `ore_ont_1579` | 3.98 s | 4.23 s | +6.3% | identical |
| `ore_ont_8989` | 1.91 s | 1.91 s | +0.0% | identical (clash-bearing) |
| `ore_ont_5753` | 68.38 s | 68.84 s | +0.7% | identical (clash-bearing) |

**18/18 outcome-identical. Worst delta +9.6% (0.98 s absolute) — no row meets the
material-slowdown threshold on either half of the conjunction.** The two DNFs are
pre-existing and identical on both binaries. `ore_ont_1833`, the low-work member
whose pre-check now completes rather than timing out, is +4.5% / +3.8 s — well
inside tolerance, and its output is unchanged for the reason given in §5.

*Threat to validity:* single runs, not min-of-3, so the ±10% band is
run-to-run noise rather than a measured effect. The directional split (7 up,
6 down) is what one expects from noise; the design's own prediction is exactly
zero effect, since 16 of the 18 have pre-checks that either complete under 3000 ms
or score above the threshold.

## 11. Default recommendation

**ON.** It replaces a shipped default whose only measured failure mode is losing
the `family.ofn` detection on a moderately slower host, and it cannot lower any
budget below the value the four DNF regressions were validated under. `=0` and
any explicit `RUSTDL_CLASSIFY_INCONSISTENCY_MS` remain available.
