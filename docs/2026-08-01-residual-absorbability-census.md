# Residual-GCI absorbability census

**Date:** 2026-08-01 · rustdl v0.4.10 · **report-only — no reasoning behaviour changed**

Operationalises `docs/2026-08-01-absorption-is-the-bottleneck.md`: for every `residual_gci`
in the absorbed TBox, *which absorption technique would remove it?* The point of the exercise
is to decide what to implement **from measured population counts**, not from a grep.

New CLI (see `crates/owl-dl-core/src/residual_absorbability.rs`,
`owl_dl_reasoner::residual_absorbability_stats`, `rustdl residual-absorbability`):

```sh
rustdl residual-absorbability path/to/ont.owl          # histogram
rustdl residual-absorbability --tsv path/to/ont.owl    # one machine-readable line
```

## Buckets, and the soundness boundaries they encode

A residual body is a disjunction `d₁ ⊔ … ⊔ dₙ` standing for `⊤ ⊑ d₁ ⊔ … ⊔ dₙ`. If `dᵢ = ¬X`
the axiom reads `X ⊑ (rest)`, so the **antecedent is the negation of the disjunct**. Exactly
one bucket is assigned per residual, most-specific-first in the order below.

| disjunct (NNF) | antecedent | bucket | domain axiom? |
|---|---|---|---|
| `Max(0, R, ⊤)` = `¬(≥1 R)` | `≥1 R` | `domain_absorbable` | **yes** — identical to `ObjectPropertyDomain(R, rest)` |
| `All(R, ⊥)` = `¬∃R.⊤` | `∃R.⊤` | `domain_absorbable` | **yes** — the same axiom |
| ≥2 × `Not(Atomic)` | `A ⊓ B ⊓ …` | `binary_absorbable` | binary absorption (Hudek & Weddell) |
| `Not(Nominal)` | `{a}` | `nominal_absorbable` | nominal absorption |
| `Max(k, R, _)`, **k ≥ 1** | `≥k+1 R`, k+1 ≥ 2 | `card_antecedent_n_gt_1` | **NO — UNSOUND.** A domain rule fires at *one* successor; the antecedent needs k+1. Strictly too strong. |
| `All(R, D)`, **D ≠ ⊥**; `Max(0, R, C)`, **C ≠ ⊤** | `∃R.¬D` / `∃R.C` | `qualified_exists_antecedent` | **NO** — qualified; needs a filler check |
| none of the above | — | `genuinely_disjunctive` | the floor |

The last two exist **specifically so they cannot swell `domain_absorbable`.** An inflated
domain count would send the next implementation at an unsound fix; that is the single
load-bearing property of this tool.

Note `ObjectPropertyRange(R, C)` (`⊤ ⊑ ∀R.C`) does **not** appear anywhere here: `absorb_roles`
already rewrites a *singleton* `All(R, D)` residual into an unguarded `RoleRule`. Role
absorption for bare range axioms is shipped. `All(R, D≠⊥)` therefore only survives as a
residual *inside a multi-disjunct `Or`*, i.e. a genuinely qualified antecedent.

## Validation — run before trusting any number below

### `ore_ont_3281` — the confirming case (required: `domain_absorbable ≥ 2`)

| file | residual_gcis | domain_absorbable | others |
|---|---:|---:|---|
| `ore_ont_3281.owl` as shipped | **28** | **28** | all other buckets 0 |
| `/tmp/3281-noequiv.ofn` (the one redundant `EquivalentClasses` deleted) | **26** | **26** | all other buckets 0 |

The residual count reproduces the analysis exactly (28 → 26), and **the two residuals that
the deletion removes are both `domain_absorbable`** — well past the required ≥ 2. ✅

### `ore_ont_10019` — the negative control

**Reported: 5 residual GCIs, all 5 `domain_absorbable`.** This is **not** a false positive,
and the analysis doc's "zero axioms of this shape" is not wrong either — the two statements
are about different things, and reconciling them is the most useful finding here:

- `ore_ont_10019` contains exactly **5 `ObjectPropertyDomain` axioms** — a 1:1 match with the
  5 residuals.
- `absorb.rs:290` lowers `ObjectPropertyDomain(R, D)` to `∃R.⊤ ⊑ D`, which internalises to
  `⊤ ⊑ ∀R.⊥ ⊔ D` — no `Not(Atomic)` disjunct — so **it lands in `residual_gcis`.**

So the claim in `2026-08-01-absorption-is-the-bottleneck.md` §"The instance that proves it"
that *"`ObjectPropertyDomain(hasTarget, Relation)` is already in the file, and rustdl handles
that construct natively"* is **false as implemented.** rustdl accepts the construct but
absorbs it into a global disjunction. The doc's negative control was scanning for
`SubClassOf(≥1 R, C)` *source syntax*; the residual population is a strict superset because
every domain axiom in every ontology contributes one.

**This strengthens the case for domain absorption and weakens the case for reading `→ 0
residuals` as a predicted speedup.** `ore_ont_10019` would reach zero residuals under domain
absorption, yet its stall is profiled at 84.6% main tableau and the doc's own analysis says
this mechanism does not explain it. Zero residuals is a *structural* result.

### End-to-end shape probes (source syntax → bucket)

| axiom | bucket |
|---|---|
| `ObjectPropertyDomain(R, D)` | `domain_absorbable` ✅ |
| `SubClassOf(ObjectSomeValuesFrom(R, owl:Thing), D)` | `domain_absorbable` ✅ |
| `SubClassOf(ObjectMinCardinality(1, R), D)` | `domain_absorbable` ✅ |
| `SubClassOf(ObjectMinCardinality(2, R), D)` | `card_antecedent_n_gt_1` ✅ *(unsound-to-absorb exclusion holds)* |
| `SubClassOf(ObjectSomeValuesFrom(R, C), D)` | `qualified_exists_antecedent` ✅ *(filler-check exclusion holds)* |
| `ObjectPropertyRange(R, D)` | 0 residuals — already a `RoleRule` ✅ |
| `EquivalentClasses(D, ObjectUnionOf(ObjectMinCardinality(1,R), C))` (the 3281 shape) | `domain_absorbable` ✅ |

## Coverage — nothing silently dropped

Conversion-only, 60 s cap, 2 concurrent workers, pinned binary
`sha256 8a2efffb303cee3e7c156f9c8314efaa8ca6a42397260da2a92062c66e99a738`
(byte-reproducible: a second `cargo build --release` produced the identical hash).

| population | requested | analysed | timed out >60 s | parse error |
|---|---:|---:|---:|---:|
| DNF survivors | 167 | **161** | 6 | 0 |
| whole pool | 1,920 | **1,913** | 6 | 1 |

- **Timed out (all 6 are survivors, all 11–59 MB — conversion-bound, not classifier-bound):**
  `ore_ont_10929` (50M), `ore_ont_15635` (59M), `ore_ont_2504` (25M), `ore_ont_4141` (11M),
  `ore_ont_4572` (54M), `ore_ont_8445` (23M).
- **Parse error:** `ore_ont_10860` — horned-owl fails on a `DLSafeRule` (SWRL) at line 1400.
  Pre-existing, unrelated to this work; the file never reaches conversion.

Independent corroboration of the source analysis: it reported *"130 of 160 measured survivors
carry residual GCIs, median 46, p90 1639, max 38,135."* This census, over 161, gets
**131 residual-bearing, median 46, p90 1639, max 38,135.**

## Bucket totals

### Population A — the 167 DNF survivors (161 analysed)

| bucket | total | share of residuals |
|---|---:|---:|
| `domain_absorbable` | 10,639 | 6.3% |
| `binary_absorbable` | 0 | 0.0% |
| `nominal_absorbable` | 0 | 0.0% |
| `card_antecedent_n_gt_1` | 42 | 0.0% |
| `qualified_exists_antecedent` | 98,128 | **58.0%** |
| `genuinely_disjunctive` | 60,274 | **35.6%** |
| **residual_gcis** | **169,083** | 100% |

`concept_rules` 26,062,572; conclusion is an `Or` 650,736; **of those, still carrying a
`¬Atomic` disjunct — the real binary-absorption candidates — 34,667.**

### Population B — the whole 1,920 pool (1,913 analysed)

| bucket | total | share of residuals |
|---|---:|---:|
| `domain_absorbable` | 97,521 | 11.4% |
| `binary_absorbable` | 0 | 0.0% |
| `nominal_absorbable` | 0 | 0.0% |
| `card_antecedent_n_gt_1` | 54 | 0.0% |
| `qualified_exists_antecedent` | 304,044 | 35.4% |
| `genuinely_disjunctive` | 456,122 | **53.2%** |
| **residual_gcis** | **857,741** | 100% |

`concept_rules` 148,714,494; conclusion is an `Or` 1,054,027; **with an extra `¬Atomic`
disjunct: 199,019.**

`binary_absorbable = 0` and `nominal_absorbable = 0` among residuals is the **expected and
correct** answer, not a bug: a residual by definition has no `Not(Atomic)` / `Not(Nominal)`
disjunct, or `as_trigger` would have consumed it. Binary absorption's payoff is in the
`concept_rule_or_with_extra_not_atomic` column above, not here.

## Headline — how many ontologies reach `residual_gcis == 0`

Zero residuals means **no global disjunctions at all**.

| | survivors (161) | whole pool (1,913) |
|---|---:|---:|
| already at 0 residuals | 30 | 791 |
| have ≥1 residual | 131 | 1,122 |
| have `domain_absorbable > 0` | **121** (75%) | **1,030** (54%) |
| **→ 0 residuals under DOMAIN absorption alone** | **54** (41% of the 131) | **532** (47% of the 1,122) |
| → 0 residuals under DOMAIN + BINARY | 54 | 532 |
| → 0 residuals if qualified `∃`-absorption were *also* built (ceiling) | 85 | 722 |
| have `genuinely_disjunctive > 0` (irreducible floor) | 45 | 396 |

Domain + binary adds **nothing** over domain alone — binary contributes 0 residuals by
construction, so the two columns are necessarily identical.

`domain_absorbable` distribution over the ontologies that have any —
survivors: min 1, p25 11, **median 35**, p75 59, p90 218, max 2,452 (sum 10,639);
pool: min 1, p25 16, **median 51**, p75 77, p90 137, max 12,702 (sum 97,521).

Both validation ontologies are in the 54: `ore_ont_3281` **and** `ore_ont_10019` reach zero.

## Top 15 by `domain_absorbable` — whole pool

| ontology | domain_absorbable | residual_gcis | genuinely_disjunctive | →0 under domain? | in the 167? |
|---|---:|---:|---:|:--:|:--:|
| `ore_ont_1270` | 12,702 | 12,705 | 3 | no | no |
| `ore_ont_16286` | 7,003 | 7,003 | 0 | **yes** | no |
| `ore_ont_10949` | 2,452 | 2,452 | 0 | **yes** | **yes** |
| `ore_ont_16853` | 2,208 | 2,233 | 25 | no | no |
| `ore_ont_4049` | 2,197 | 2,204 | 7 | no | no |
| `ore_ont_11085` | 1,341 | 2,073 | 732 | no | **yes** |
| `ore_ont_9577` | 906 | 914 | 8 | no | no |
| `ore_ont_16420` | 883 | 891 | 8 | no | no |
| `ore_ont_9151` | 864 | 872 | 8 | no | no |
| `ore_ont_12191` | 625 | 633 | 8 | no | no |
| `ore_ont_11378` | 622 | 629 | 7 | no | no |
| `ore_ont_1886` | 604 | 604 | 0 | **yes** | no |
| `ore_ont_3077` | 599 | 606 | 7 | no | no |
| `ore_ont_699` | 599 | 606 | 7 | no | no |
| `ore_ont_5059` | 550 | 550 | 0 | **yes** | no |

### Top 15 restricted to the DNF survivors (the actionable population)

| ontology | domain_absorbable | residual_gcis | qualified | genuinely_disj | →0 under domain? |
|---|---:|---:|---:|---:|:--:|
| `ore_ont_10949` | 2,452 | 2,452 | 0 | 0 | **yes** |
| `ore_ont_11085` | 1,341 | 2,073 | 0 | 732 | no |
| `ore_ont_15687` | 358 | 358 | 0 | 0 | **yes** |
| `ore_ont_14572` | 337 | 339 | 2 | 0 | no |
| `ore_ont_7361` | 337 | 339 | 2 | 0 | no |
| `ore_ont_9724` | 337 | 339 | 2 | 0 | no |
| `ore_ont_8273` | 334 | 341 | 2 | 5 | no |
| `ore_ont_4796` | 303 | 305 | 1 | 1 | no |
| `ore_ont_3794` | 275 | 276 | 0 | 1 | no |
| `ore_ont_10621` | 265 | 265 | 0 | 0 | **yes** |
| `ore_ont_11270` | 265 | 265 | 0 | 0 | **yes** |
| `ore_ont_8475` | 231 | 1,199 | 844 | 124 | no |
| `ore_ont_1123` | 218 | 262 | 9 | 34 | no |
| `ore_ont_12432` | 151 | 151 | 0 | 0 | **yes** |
| `ore_ont_13859` | 143 | 143 | 0 | 0 | **yes** |

A recurring near-miss pattern is visible: `337 / 339`, `334 / 341`, `303 / 305`, `275 / 276`
— domain absorption clears all but a handful, and those two-to-seven stragglers are what keep
the ontology out of the zero-residual set. Whether that residue still costs 300× is
**unmeasured** and is the obvious follow-up experiment.

## Verification of "report-only"

- `classify --pair-timeout-ms 1000` on `bench-corpus/pizza.ofn`, `bench-corpus/mie.ofn`,
  `docs/family-mech4-ddmin-core.ofn`, pre-change binary vs post-change binary: **byte-identical
  on all three** once the nondeterministic `# wall breakdown ms:` line is stripped (that line
  differed by 2 ms on two of the three; nothing else differed by a byte).
  (`ontologies/real/` is gitignored and not provisioned in this worktree, hence the
  `bench-corpus` fixtures.)
- `cargo fmt --all -- --check` clean.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` clean.
- `cargo test --workspace --exclude owl-dl-py`: **1,431 passed, 0 failed.**

## Sabotage — 6 of 6 caught

Per `[[sabotage-your-own-guard-tests]]`: a test written to guard X often does not guard X.
Each guard was broken in turn and the suite re-run.

| # | sabotage | expected canary | result |
|---|---|---|---|
| S1 | drop the `Max(0,R,filler)` `filler = ⊤` check → treat every `≤0 R.C` as domain | `max_zero_qualified_is_not_domain_absorbable` | **CAUGHT** |
| S2 | treat `Max(k≥1, R, _)` as domain (**the unsound one**) | `max_k_ge_1_is_not_domain_absorbable` | **CAUGHT** |
| S3 | drop the `All(R, ⊥)` check → treat every `∀R.D` as domain (**unsound**) | `all_non_bot_is_not_domain_absorbable` | **CAUGHT** |
| S4 | invert the domain/binary priority | `domain_wins_priority` | **CAUGHT** |
| S5 | binary threshold `≥2` → `≥1` | `one_not_atomic_is_not_binary_absorbable` | **CAUGHT** |
| S6 | drop the non-`Or` singleton-body handling | `singleton_body_is_classified` | **CAUGHT** |

**6/6.** Each sabotage failed the *specific* canary written for it, not merely "some test".
13 unit tests total in `residual_absorbability::tests`, all passing on the unsabotaged source.

## Read — is domain absorption worth implementing?

**Yes, but as the cheap first step of a two-step programme, and not on the strength of the
300× calibration alone.**

For it:

- It is **sound and complete by logical identity** with `ObjectPropertyDomain` — the one
  technique in the list with no completeness or FP exposure, and a rule form
  (`RoleRule`) rustdl already has.
- The addressable set is large and not speculative: **1,030 of 1,913** ontologies carry at
  least one, **121 of 161** survivors do, and **532 pool-wide / 54 survivors** would drop to
  **zero global disjunctions**.
- The mechanism is broader than the source analysis assumed. Every `ObjectPropertyDomain`
  axiom in every ontology currently becomes a global disjunction. That is a very common
  construct being handled in an expensive way, and it explains why `domain_absorbable > 0` is
  near-universal rather than exotic.

Against, and worth stating plainly:

- **Domain absorption removes only 6.3% of survivor residuals** (11.4% pool-wide). The mass is
  elsewhere: `qualified_exists_antecedent` 58% and `genuinely_disjunctive` 36% in the
  survivors. Reaching *zero* is an all-or-nothing predicate that a small absolute count can
  satisfy; the bulk of the residual *volume* is untouched.
- Median `residual_gcis` among survivors is 46 and median `domain_absorbable` is 35, so the
  typical survivor is left with ~10 residuals, not 0. If two really cost 300×, ten is not
  obviously better than forty-six — **the relationship between residual count and wall is
  calibrated at exactly one point (28 → 26 on `ore_ont_3281`) and should not be extrapolated.**
- `ore_ont_10019`, the negative control, reaches zero residuals and is still expected to
  stall. Zero residuals ≠ fast.

Recommended order:

1. **Build domain absorption** (unqualified, n = 1). Cheap, sound by identity, and the
   pre-registered prediction is sharp: `ore_ont_3281` should reach ~0.03 s **without** editing
   the ontology. If it does not, this whole line of attack is refuted at low cost.
2. **Then measure the 54 survivors that reach zero**, and separately the near-miss cluster
   (`337/339`-style) — that comparison is what tells you whether "zero residuals" or "fewer
   residuals" is the operative variable.
3. **Only then consider qualified `∃`-absorption**, which is where the volume actually is
   (58% of survivor residuals) and which would raise the zero-residual survivors from 54 to
   **85**. It is materially harder — it needs a *backward* role rule (target label → source
   label), which `RoleRule` does not currently express — so it should be gated on step 2.
4. **Binary absorption is a separate lever with its own population** (199,019 `Or`-conclusion
   concept rules pool-wide still carrying a `¬Atomic`). It removes **zero** residuals and so
   cannot be justified by the residual census; justify it on the concept-rule column or not
   at all.

`genuinely_disjunctive` — 36% of survivor residuals, present in 45 of 161 survivors — is the
floor none of these three techniques touches.

## Raw data

`docs/benchmarks/2026-08-01-residual-absorbability-census.tsv` — one row per pool ontology,
including the 6 `TIMEOUT` and 1 `ERROR` rows (nothing filtered out).
