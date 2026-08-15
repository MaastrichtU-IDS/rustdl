# Changelog

All notable changes to rustdl are documented here. Format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); rustdl follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added — `PreparedOntology`: reuse justification state across queries

`justify`/`justify_all` reload and re-derive per-ontology state (parse,
logical-axiom split, fragment classification) on every call. `rustdl.prepare(path)`
and `rustdl.prepare_bytes(data, format=...)` do that work once and return a
`PreparedOntology` whose `.justify(query)` / `.justify_all(query, max)` reuse it,
amortizing the setup across many justifications of the same ontology. Results are
identical to the one-shot functions. On the reasoner side this is
`justify::PreparedJustifier::{prepare, find_one, find_all}`; the existing
`find_one_justification`/`find_all_justifications` now delegate to it. The
`PreparedOntology` Python object is unsendable — use it from the creating thread.

## [0.4.17] - 2026-08-10

### Fixed — `classify` reported `consistent: true` on inconsistent ontologies

Over all 1,920 ORE ontologies, `is_consistent` finds **43** inconsistent while
`classify` agreed on only 41, reporting `consistent = true` on **2** —
`ore_ont_16372` and `ore_ont_7610`. Konclude (`Ontology … is inconsistent`) and
HermiT (`InconsistentOntologyException`) call both inconsistent independently, as
does rustdl's own `is_consistent` in 0.09–0.36 s. A confidently wrong verdict is
worse than a slow one, so this is not an opt-in.

`classify`'s inconsistency detection consulted the saturator's `⊤`-unsat signal and
the `ABox` pre-check, but never the wedge-consistency route `is_consistent` uses.
`RUSTDL_CLASSIFY_CONSISTENCY_PROBE` (**default ON**, `=0` reverts) adds three
layers, cheapest first:

1. an asserted instance of an unsatisfiable class — exact and engine-free, reading a
   set classify has already computed (`abox_check`'s P1 tests the same rule, but
   against the *saturator closure*, which knows none of these classes are unsat);
2. the wedge consistency route;
3. one **bounded** `decide(⊤)`, mirroring `is_consistent`'s fall-through after a
   wedge `Stalled`.

**A gate makes it affordable.** Running a consistency check unconditionally on the
classify path costs a mean of **5.1 s** on consistent ontologies (60 sampled, 16
over 1 s, max 30 s) — a documented dead-end. But an inconsistent KB makes `⊤` unsat
and therefore *every* class unsat, so **zero unsatisfiable classes implies
consistent**. Measured, that admits ~1.6% of ontologies.

**A second gate on the unsat FRACTION** (`RUSTDL_CLASSIFY_PROBE_MIN_FRAC_PERMILLE`,
default `2` = 0.2‰… i.e. 0.2%) was forced by a sweep. The `≥1 unsat` gate alone
admits a huge `ABox`-bearing ontology on the strength of one unsatisfiable class,
while the probe's cost scales with the `ABox`:

| ontology | classes | unsat | `ABox` | fraction | |
|---|---|---|---|---|---|
| `ore_ont_14881` | 20,485 | 1 | 98,536 | 0.005% | harmed |
| `ore_ont_1966` | 20,514 | 13 | 84,424 | 0.063% | harmed |
| `ore_ont_16372` | 744 | 3 | 108 | **0.403%** | must reach |
| `ore_ont_7610` | 91 | 91 | 0 | **100%** | must reach |

The threshold sits in that measured ~6× gap. The rationale is semantic, not
curve-fitting; and it must stay **low**, because `16372` is genuinely inconsistent at
only 0.403% — classify's own per-class unsat detection being incomplete.

### Changed — `RUSTDL_FIXPOINT_DEADLINE` now defaults ON

Shipped OFF in v0.4.16 as a documented NO-GO: both gates passed (ΔMISSED = +0; a
1,920-ontology sweep with 0 recoveries, 0 regressions, +0.5% wall) but it was
corpus-**neutral**, so there was no reason to pay for it. What changed is not the
measurement but the arrival of a caller that needs its budget honoured. With it off,
the new probe overshoots a 100 ms budget by **66–80 s** on `ore_ont_1966`; with it
on, the same probe costs **0.4–1.0 s at any budget**.

The overshoot was localised by stack-sampling, which showed
`owl_dl_tableau::hyper::*` — the **wedge**, correcting an earlier attribution to
`decide_with_deadline` on the main tableau.

### Result

| | |
|---|---|
| wrong answers fixed | **2 of 2**, both matching Konclude **and** HermiT |
| corpus regressions | **0** over 1,920 ontologies |
| FP=0 net | clean, zero `FP>0` / `MISSED>0`, all closures exact |
| workspace | 1,605 pass / 0 fail |

The combined-defaults sweep read `1750 ok / 168 dnf` vs `1749 / 169`, i.e. a nominal
`+1 recovery` and eight slowdowns. **Every one is contention** — the sweep runs
`JOBS=4`, and re-measuring on a quiet host collapses all of them (`ore_ont_13071`
33.89→52.73 s in the sweep, **20.61→20.68 s** re-measured; the "recovered"
`ore_ont_15491` completes in **both** arms at ~40 s). The change is
**outcome-neutral**, not net-positive, and the `+1` must not be quoted as a win.

### Known limitations

* `solve_at_most` in the wedge has **no deadline check at all** — the same defect
  class, not pursued because nothing measured reaches it. The `FIXPOINT_DEADLINE`
  history is the argument for waiting: a bound with no caller that needs it measures
  as a NO-GO.
* Two mechanisms were built and discarded on measurement before the shipped one:
  verifying every asserted-instance class through the main tableau (`k` **unbounded**
  probes — 58 on `wine` — which ran the FP=0 net 8h47m at 3162% CPU without
  finishing), and a flat 1000 ms probe budget (4 ontologies `ok → dnf`).


### Fixed — classify regression from the v0.4.8 inconsistency pre-check

`RUSTDL_CLASSIFY_INCONSISTENCY` (default ON since v0.4.8) puts an **unbounded**
consequence-based fixpoint over named individuals in front of every classify. On ORE
ontologies with very large `ABox`es that fixpoint costs more than the classify it
precedes. A full-corpus before/after sweep (v0.4.6 vs `main`, 1,920 ontologies, 60 s
cap, single-thread) found **four ontologies that went from `ok` to `dnf`**, and
bisected all four to that one flag.

The `ABox`-saturation half now runs under a wall-clock budget **on the classify path
only** (`RUSTDL_CLASSIFY_INCONSISTENCY_MS`, **default 3000**, `0` = unbounded).
`is_consistent` / `realize` / `materialize_*` / `diagnose` are untouched — for them
the pre-check *is* the point of the call.

| ontology | v0.4.6 | `main` (v0.4.10) | this fix |
|---|---|---|---|
| `ore_ont_10838` | 4.86 s | **DNF @60 s** | 5.43 s |
| `ore_ont_15846` | 21.73 s | **DNF @60 s** | 7.78 s |
| `ore_ont_16315` | 4.42 s | **DNF @60 s** | 4.34 s |
| `ore_ont_3087` | 4.80 s | **DNF @60 s** | 4.98 s |

**Bounding is safe by the pre-check's own contract**: it is a sound
UNDER-approximation — a clash proves inconsistency, but *no clash yields no verdict
and the caller proceeds exactly as before*. A timeout therefore returns the
no-verdict answer every caller already handles; it can never manufacture an
inconsistency. The abandoned run also drops its partial `edges` / `derived_same` so
a half-saturated closure cannot be mistaken for the fixpoint one. The clock is
sampled every 4096 worklist pops / chain steps, and a `None` deadline reads it zero
times, so the unbounded callers keep their exact previous code path.

**The default is 3000 ms because "a few hundred ms" was falsified by measurement.**
On `family.ofn` — the ontology this pre-check exists for — the `ABox` saturation
itself takes **~2.0 s** on the reference host (classify 2.67 s with the pre-check vs
0.67 s without: 506 individuals, but a 267k-edge role-chain closure). A
few-hundred-ms budget would silently break exactly the detection the flag was added
for. At 3000 ms the tax on a pathological `ABox` is ~3.0–3.5 s and `family` keeps
~1.5× headroom. Gates: `classify --json family.ofn` output **byte-identical** to
pre-change `main`; `{A ⊑ ⊥, B ⊑ ⊥}` still `consistent: true` (all-classes-unsat is
NOT inconsistency); `rustdl consistent family.ofn` unchanged at 2.63 s.

**Sabotage record — 8 sabotages, 5 caught, 3 survivors** (each run strictly
serially against `classify_inconsistency_budget.rs`; survivors reported because a
test written to guard X often does not guard X):

| # | sabotage | result |
|---|---|---|
| 1 | delete the two **worklist-drain** probes | first run **SURVIVED 8/8** → drove two new single-drain fixtures (`type_ladder` / `role_ladder`) → then **caught, 2 fail** |
| 2 | delete the two **role-chain** probes | **SURVIVED 10/10** |
| 3 | timeout returns `clash: true` | caught, 2 fail |
| 4 | timeout publishes the partial `edges` | caught, 1 fail |
| 5 | budget wired into `abox_saturation_inconsistent` (the unbounded wrapper) | **SURVIVED twice** (cheap and slow fixtures) |
| 6 | budget wired into `saturate_abox_consistency` (the unbounded saturator entry) | caught, 1 fail |
| 7 | shipped default slashed 3000 → 1 ms | **SURVIVED twice** |
| 8 | sampler `Deadline::hit` never fires | caught, 2 fail |

The three survivors are **latent, not absent**, and each has a diagnosis recorded
in the test file: (2) the chain probe fires per *outer* chain edge, and every
fixture here grows its closure across iterations, so the per-iteration probe cuts
first — a fixture whose single chain pass exceeds 4096 outer edges would be needed;
(5) `is_consistent` has a complete tableau fallback, so a leak is invisible on any
fixture the tableau can also solve — the known exception is `family.ofn` itself,
which would hang rather than fail; (7) **no in-tree canary pins the 3000 ms
number** — every cheap synthetic ABox clash is also caught by an unbudgeted route
(the slow one still reports `consistent: false` under both
`RUSTDL_CLASSIFY_INCONSISTENCY=0` and `RUSTDL_ABOX_CHECK=0`). The default is
carried by the release-CLI `family.ofn` measurement above; re-measure `family`
before changing it. Sabotage 6 is the one with teeth for leaks, because
`materialize_object_property_assertions` publishes the closure itself.

Canaries: `crates/owl-dl-reasoner/tests/classify_inconsistency_budget.rs` (12,
negatives first). The pre-existing `family_classify_agrees_with_is_consistent` now
pins the budget to `0` — `family` needs more than any sane default in the
unoptimized test profile, so under the default that canary would measure the host;
the profile-independent replacement is
`default_budget_still_detects_small_abox_inconsistency`, a small functional-role
`ABox` clash detected under the shipped default.

## [0.4.16] — 2026-08-07

### Changed — `RUSTDL_HYPER_MATCH_DEADLINE` default ON (`=0` reverts)

Shipped OFF in v0.4.15 on the strength of a wall regression **that turned out to be a
measurement artifact**. Re-measured, it flips.

**What it repairs is a contract violation, not a performance number.**
`HyperEngine::solve` checked its deadline only at *function entry* — the sole runtime
deadline check in the engine — so a frame whose work stayed inside `enumerate_matches`
never re-consulted it. Consequence: **`--pair-timeout-ms` and `--global-timeout-ms` were
silently unenforceable on that path.** On `ore_ont_16056` (309 classes) two
`classify_labels` calls ran **~17 s each against a 1 ms budget**, and `label_cache_build`
was invariant (±0.3%) across ten configurations because nothing could stop it. At the new
default that phase is **112 ms** versus **110,821 ms** at `=0`.

**Evidence for the flip** — matched arms of one pinned binary (sha `74180aca60d1bd8f`):

| | flag OFF | flag ON |
|---|---:|---:|
| MISSED (400-ontology net) | 5,194 | **5,194** |
| FP | 0 | **0** |
| onts with MISSED | 59 | 59 |
| `arm_no_closure` | 4 | 4 |

**ΔMISSED = +0, FP = 0.** The 4 `arm_no_closure` drops occur in **both** arms, so they are
not attributable to the flag — they were an artifact of comparing against the older
**v0.4.13** baseline, which bundles every change since (including domain absorption going
default ON). The 1,920-ontology sweep separately showed **0 answer changes** across 1,747
both-arm completers.

**The v0.4.15 note claiming "+40%/+47% wall on two completers" is WITHDRAWN.** Those were
single runs taken under load. Clean min-of-3 on an idle host: `ore_ont_15491` 62.8 → **62.0 s**,
`ore_ont_5617` 67.9 → 68.1 s, `ore_ont_14351` 57.9 → **57.3 s** — neutral to marginally
faster. The explanation offered for the phantom slowdown (that truncation starves the label
cache) is also refuted: label-heuristic prune rates are **identical** between arms
(13,469,242 and 16,428,289, `misses=0` both). The three sweep `ok → dnf` rows were
cap-boundary artifacts — all three complete at 57–68 s against a 60 s cap with identical
output.

**Soundness.** Truncating match enumeration derives *fewer* facts, so the failure mode is a
MISS, never a false positive — and `horn_fixpoint` converts the truncation flag into
`Stalled` **before** it can return `Sat`, which is what stops a truncated enumeration
surfacing as a trusted `Sat` (FP-safe but silently incomplete). `Unsat` is still returned
first, so a clash found before truncation remains a real clash.

**Still open, unchanged by this release:** `label_cache_build` has a per-class bound and no
*aggregate* bound, so the default per-class budget (`clamp(n × per_pair, …)`) can still be
30 s × n classes. This flag makes a budget *effective*; it does not make the default budget
*sane*. See `docs/known-limitations/label-cache-build-unbounded.md`.

### Gates

FP=0 net at the new defaults: **13 VERIFIED, zero `FP>0`/`MISSED>0`**. **1,590 tests pass**;
`fmt` and `clippy -D warnings` clean. New canary
`crates/owl-dl-tableau/tests/match_deadline_default.rs` pins all four rows of the default;
sabotaging the flip fails exactly the two discriminating rows (unset, empty).

## [0.4.15] — 2026-08-07

**One default change, two sound opt-ins that measurement declined to flip, and a materially
corrected record.** The headline is `RUSTDL_DOMAIN_ABSORPTION`; the more useful output may be the
four retractions.

### Changed — `RUSTDL_DOMAIN_ABSORPTION` default ON (`=0` reverts)

Absorbs domain residuals into triggered role rules. Full 1,920-ontology two-arm sweep, 60 s cap,
single-thread, arms sequential, per-arm wrapper scripts recorded in each record's provenance:

| | OFF | ON |
|---|---:|---:|
| ok | 1,751 | **1,753** |
| dnf | 168 | **166** |

Three recoveries — `ore_ont_16372` 60 s → **8.36 s**, `6132` → 32.46 s, `9899` → 32.86 s, all
peer-solvable — with **0 answer changes across all 1,750 both-arm completers** and a median wall
delta of **+0.000 s**. FP=0 net: 11 VERIFIED, closures exact.

**It also closed the completion-graph half of issue #35 v4.** The load-bearing gate
`issue35_v4_completion_graph_is_bounded` had been `#[ignore]`d *for failing* since 2026-07-23; it
is now live and passing. With the node cap off, `=0` does not terminate (killed at 300 s) and the
new default passes in **0.00 s**. This is causal: v4's root cause is `ObjectPropertyDomain(r,A)`
absorbing to an **untriggered** residual, and domain absorption is precisely the pass that makes
it triggered. The 2026-07-23 conclusion that this needed "a nominal-aware-blocking / NN-rule
redesign" was **wrong about the required mechanism** — a pass that already existed, behind a
flag, sufficed. `RUSTDL_NOMINAL_FIRST` remains falsified and OFF.

**Known cost, recorded rather than buried** (all with byte-identical output, re-verified serially
at min-of-3): `ore_ont_7011` 5.05 → 17.53 s (3.5×), `ore_ont_13545` 5.35 → 15.47 s (2.9×), and
`ore_ont_14351` crossing a 60 s cap (59.96 → 61.47 s). Net **+2 completions at a 60 s budget**,
+3 above ~62 s.

### Added — `RUSTDL_INVERSE_PAIR_FUNC` (default **OFF**)

Derives functionality across a declared inverse pair (`Functional(R)` ⟹ `InverseFunctional(S)`
and the three symmetric cases) and materialises the entailed inverse `ABox` edge where the
partner is functional. Closes a genuine **wrong-verdict** defect found by delta-debugging
`ore_ont_4141` from 67,143 axioms to a **7-axiom core**: a 5-axiom `ABox` that Konclude and
HermiT both call inconsistent was reported `consistent`.
See `docs/known-limitations/inverse-pair-functionality-not-derived.md`.

**Stays OFF:** a three-arm sweep found one real `ok → dnf` (`ore_ont_16372`, 8.28 s → dnf,
reproduced serially). It also has **no corpus-level completeness gain** — the +17-pair oracle
parity on `ore_ont_13859` measured on 2026-08-06 was lost when the derivation was reordered to
run after `derive_functional_max_cardinality` (which repaired 3 of 4 earlier regressions). That
trade was made without measuring the benefit side; recovering it by emitting the enforcement GCI
*selectively* is the recorded follow-up.

### Added — `RUSTDL_HYPER_MATCH_DEADLINE` (default **OFF**)

`HyperEngine::solve` checked its deadline **only at entry** — the sole runtime deadline check in
the engine — so a frame whose work stayed inside `enumerate_matches` never re-consulted it. On
`ore_ont_16056` (309 classes) `label_cache_build` was ~34 s and **invariant across ten
configurations**; two `classify_labels` calls ran ~17 s each against a **1 ms** budget. With the
flag on that phase falls to **102 ms** (~800×) and the ontology classifies in 16.9 s.
See `docs/known-limitations/label-cache-build-unbounded.md`.

`horn_fixpoint` converts the truncation flag into `Stalled` **before** it can return `Sat` — the
load-bearing line, without which a truncated enumeration would be FP-safe but *silently
incomplete*.

**Stays OFF:** it recovers **1 of 12** cluster members (predicted 3–8), needs non-default budgets
to do so, and adds **+40%/+47%** wall on two completers.

### Added — diagnostics

`residual-absorbability` gains a guard-manufacturable predicate (plus a 1,920-ontology census).
CI now runs `owl-dl-py`'s Rust unit tests, which previously had **zero** coverage: `ci.yml`
excludes the crate and `pytest` only exercises the Python surface.

### Fixed — the record

Four of the project's own claims were withdrawn on evidence:

- **The KM soundness comparison.** "KM 10-ontology FP / ~1795 spurious pairs" is retracted: KM
  v0.2.5 is **FP=0 on every cited ontology testable**, and **73% of the figure was a
  `⊤`-equivalence convention artifact** (Konclude collapses `X ⊑ TopEquivClass` into
  `EquivalentClasses(Thing, C)`; the old analysis normalised only the `⊥` side). Version-independent,
  so it was never sound methodology. rustdl's own FP=0 record is unaffected.
- **Defect 7** (`Instant::now()` batching) is refuted — the 11.28% belonged to an
  already-fixed loop in `owl-dl-saturation`.
- **Konclude's mechanism**, corrected by reading its own absorbed-TBox dump: its `⇐` direction is
  **not** Horn; it carries the same 4-way disjunctive head, and differs by being demand-driven
  (all 47 absorbed rules have two guards; **zero** fire on a bare node).
- **`CLAUDE.md`'s "dead code" label** on `label_cache_timeout_ms` — it is live and load-bearing,
  and that label would have deterred the investigation that found the deadline defect.

The DNF tail was also re-triaged: **151 ontologies, 138 peer-solvable**, superseding the
257/242 figures, with HermiT's and KM's sets proven to be strict **subsets** of Konclude's.

### Gates

FP=0 net at pure defaults: **13 VERIFIED, zero `FP>0`/`MISSED>0`**, every reference closure exact
(galen 27997, notgalen 32739, sio 8904, ore-10908 6001, pizza 499, alehif 247, ro 158,
ore-15672 142, sulo 51, bibtex 16). **1,586 tests pass**; `fmt` and `clippy -D warnings` clean.

## [0.4.14] — 2026-08-04

**Adaptive early-abandon in the main tableau, default ON** — plus the measurement capability that
made it decidable. `RUSTDL_TABLEAU_EARLY_ABANDON=0` reverts.

### The lever, and why it was blocked until now

`MAX_SEARCH_DEPTH = 256` binds on **27 of the 33** ontologies that reach the main tableau, with zero
headroom on every one. Lowering it recovers DNFs and gives large speedups — but **loses pairs** (4 of
162 on `ore_ont_10019`), and iterative deepening was already **refuted** here: at depth 8 versus 256
the speedup cases show `tableau=0` and identical rows, because the entire gain is on probes that
reach **no verdict at either depth**. The win is *giving up sooner*, and deepening is defined not to.

So the right shape is an adaptive early-abandon — a **completeness/performance trade**, which this
project had **no way to price**. Every gate here is FP-shaped (`run-soundness-diff.sh`) or
outcome-shaped (a sweep counts `dnf → ok`). Nothing measured *entailments lost across the corpus*.

### Added — a corpus-scale MISSED net (`owl-reasoner-harness`)

Per-ontology MISSED against a **Konclude ∪ HermiT** oracle over a 400-ontology seeded, stratified
population (**189, or 47%, exercise a per-pair search**), with a committed v0.4.13 baseline of
**MISSED = 5,198** over 60 of 393 scored and **FP = 0 on all 393** against a 14.0M-pair union.
Where the two peers disagree the ontology is **excluded, not adjudicated** — a contested oracle is
not an oracle (1 case: `ore_ont_15682`, Konclude under-reporting, the third instance of that
pattern). A later arm costs **~10 minutes**.

**Its sensitivity is proven, not assumed.** A prediction was committed *before* the validating arm
ran: a 1 ms per-pair budget must give ΔMISSED > 0, concentrated in search-exercised rows, FP still 0.
Result: **+80 pairs over 13 ontologies, 13/13 search-exercised, 0 losses in the 140 fast-path rows,
FP 0** — direction, magnitude and location all as predicted. A net that reports zero for everything
is indistinguishable from a broken one.

Selection note worth keeping: `tableau > 0` would have been the **wrong** stratifying predicate — it
holds on **2 of 546** completers, because the label heuristic prunes 96–100%. The binding counter is
`label … pass_through > 0`. Choosing the intuitive proxy would have produced a net vacuous for its
primary purpose.

### The trade, priced

| | |
|---|---|
| **ΔMISSED vs the 5,198 baseline** | **0** (0 ontologies lost pairs, 0 gained) |
| FP | **0**; 400/400 closures byte-identical |
| corpus sweep, 1,920 ontologies, two arms | **6 recoveries, 0 regressions, 14 faster, 0 slower, −5.5% aggregate** |

Recoveries: `ore_ont_3281`, `3250`, `904`, `7204`, `9800`, `14351`. Speedups include `ore_ont_13545`
46.31 → 11.88 s and `ore_ont_2826` 7.31 → 4.85 s, both with **identical closures**. The new closures
are FP=0 **and** MISSED=0 against both Konclude and HermiT.

**Both gates were required.** The corpus sweep is not redundant with the MISSED net: the net's frame
is drawn from **completers** and structurally cannot observe an `ok → dnf` in the DNF tail — which is
exactly the failure mode a 12-ontology benchmark missed in v0.4.8, taking four ontologies from ~5 s
to DNF.

### Design notes

The criterion abandons a probe once it has bottomed out at `MAX_SEARCH_DEPTH` **32 times in total**,
latched, returning `Ok(None)` exactly like a deadline cut or `NodeCap` — so it can only **suppress**
an `Unsat`, never manufacture one. A bottom-out means the frame above cannot return
`Unsat(combined)`, and "needs more depth" is already refuted (`ore_ont_10019` wants 459–460 and still
DNFs at cap 2048).

**The obvious criterion was built first and refuted by its own calibration.** An `is_diverging`-style
run of cap hits reset by progress fails because the doomed probes are locally *productive* throughout
— `ore_ont_2826` makes 229 definite decisions in 1,074 trials with a longest run of **2**.

`ore_ont_3281` is why this is adaptive rather than a retuned constant: it recovers here
(DNF → 20.1 s) and is the ontology a *fixed lower* cap makes two orders of magnitude **worse**.

Sabotage: 10 run, 8 caught, **2 survivors recorded as uncaught** rather than explained away; two
earlier survivors forced new canaries, one catching the criterion being silently reverted to the
refuted form. Two fixture traps were found by the controls (the `⊔`-rule literal prune and
`ConceptPool::or` flattening each made the naive fixture depth-free).

### Gates

FP=0 net **11 VERIFIED, closures exact**; **1,565 tests**; fmt and clippy clean.

## [0.4.13] — 2026-08-03

Two correctness levers taken off the shelf, one fragile constant made adaptive, and a
well-evidenced NO-GO. FP=0 net 11 VERIFIED closures exact; 1,541 tests.

### Enabled — two DKey levers that were built, gated, and dormant

Both were finished work waiting only on a volume scan, and the scan (1,920 ontologies × 4 arms,
7,680 runs) says the cost is nil:

- **`RUSTDL_DKEY_EMIT_ORDER`** — fixes a **live, non-monotonic** defect: `∀p.[0,5] ⊓ ∃p.{9}` is
  unsatisfiable alone, but adding an *unrelated* property that merely mentions the same keys made
  it satisfiable again. Adding an axiom must never remove an entailment.
- **`RUSTDL_DKEY_ONEOF_SEED`** — the sixth D10-class bug (a gate certifies completeness while the
  engine drops the axiom).

Corpus-wide delta: **+1 concept rule and +1 told-disjoint pair**, in one ontology
(`ore_ont_9303`). Zero over the growth threshold; per-arm conversion timeouts 7/7/7/7 with an
**identical stem set**, so zero flag-induced. `ore_ont_5368` — the DKey discriminator — reads
**18,620,251 in all four arms** (`ore_ont_9347` is reported but never judged on: it reads 113
under both a correct gate and a no-op build). The single mover's verdict is confirmed
independently by **Konclude** and **HermiT**.

**Honest scope:** `ONEOF_SEED` moving *nothing* corpus-wide means the corpus shows the flip is
**free**, not that it is **right** — the numeric `DataOneOf` pattern is absent from ORE. The FP=0
net is non-regression only here, by `datatype_value_membership.rs`'s own admission.

### Fixed — the inconsistency pre-check budget was one constant serving two conflicting needs

The v0.4.11 budget was measured at **~13% headroom**, not the ~50% previously recorded. (That
figure was a **confounded subtraction** — detecting the inconsistency short-circuits the rest of
classify, so `with − without` understates the pre-check. Swept directly, `family.ofn`'s detection
flips between **2600 and 2700 ms** against a 3000 ms default.) A host 15% slower silently lost the
detection v0.4.11 shipped to provide. Raising the constant would have reintroduced the
four-ontology DNF regression the budget exists to prevent.

**Both size models were wrong, including the one this release's author proposed.** ABox size does
not predict cost in *either* direction — `ore_ont_4510` has 114,957 `ObjectPropertyAssertion` and
costs **136 ms**; `family` has 1,337 and costs **2,585 ms**; `ore_ont_6233` has 176,043
`ClassAssertion` and costs 17 ms. A global Spearman correlation would also have misled, preferring
`class_assertions` (+0.863) because it is dominated by sub-millisecond ontologies while the
decision is entirely about the tail.

What tracks the tail is `work_proxy = OPA × max(chain + transitive roles, 1)` — the only separator
with a mechanism (closure growth). Rule: `work_proxy ≤ 300_000` gets **12_000 ms**, else **3_000
ms**, with the threshold inside a **measured empty 40× gap** (`family` 50,806; cheapest expensive
case 2,047,210) and biased low. The stingy branch is bit-identical to the old flat default, so the
rule **can only ever raise a budget** — which is why no corpus sweep was required.

`family`'s headroom: **1.16× → 4.6×**, wall unchanged (2.70 → 2.71 s). The four v0.4.11 regressions
stay byte-identical (5.43 / 8.24 / 4.39 / 4.90 s). 18-ontology ABox spot-check outcome-identical,
worst case +9.6% (0.98 s).

Two further premises were refuted during the work: "edge multiplication is necessary for expense"
(held at 409 ontologies, broke at 1,137 — `ore_ont_5368` performs **zero** type/edge additions and
still costs 5,936 ms, via a second driver: the per-axiom pre-indexing prelude), and the reflex to
gate on axiom count (the prelude runs **before** the first deadline probe, so its cost is
budget-independent and gating would have made those cases strictly worse). **Honest residual,
pre-existing and unchanged: nothing bounds the prelude.**

### NO-GO — iterative deepening does not transplant to the main tableau

`RUSTDL_TABLEAU_ITERATIVE_DEEPENING`, shipped **default OFF** as a documented negative result.

v0.4.12 fixed the *wedge's* fixed depth cap this way. A constant audit then found
`MAX_SEARCH_DEPTH = 256` in the main tableau binding on **27 of the 33** ontologies that reach the
tableau, with depth 8 recovering 3 DNFs and making 2 completers **~14× faster with identical rows**.
The transplant looked obvious. **It is refuted, and the reason is the mechanism, not the code:** at
depth 8 versus 256 those cases show `tableau=0`, *identical* timed-out-pair counts, and identical
rows — the entire 14.7× is `unsat_probe` time (30,014 → 562 ms) on probes that reach **no verdict
at either depth**. The audit's win is **giving up sooner**; deepening is defined not to, and must
re-run every undecided probe at ≥256.

Measured: **0 of 3 DNFs recovered, 0 of 2 speedups reproduced.** The positive control is what makes
that trustworthy — 44 and 7 probes entered with **`shallow_decided = 0`**, so the code is live and
simply decides nothing. Superset check 26/26 byte-identical over a **census** of all 1,920
ontologies (the driver is reachable on 26, decides on 3) rather than an inert random sample.

**No sweep queued**, deliberately: a sweep bounds population risk when instance evidence is good,
and here the result is a null on its own addressable set. What the data supports instead is an
adaptive **early-abandon** — a *completeness* trade (a lower cap loses 4 of 162 pairs on
`ore_ont_10019`, including `KetoneGroup ⊑ CarbonylGroup`) that must be gated on a MISSED net rather
than a wall measurement.

### Also

- **Five constants closed by the audit**: `FIXPOINT_ITERS` (the prior "structurally true"
  reading **refuted** — it *does* bind on 11/368, and halving it breaks a healthy ontology, so it
  is correctly sized), `DIV_WINDOW` (null — disabling rescues nothing it cuts), `RUSTDL_MAX_NODES`
  (does not bind), `ID_SHALLOW_BUDGET_DIVISOR` (flat over 16×), and `label_cache_timeout_ms`
  (**dead code, zero callers**).
- **Logged, not fixed:** `MAX_BODY_VARS = 8` may be a silent-MISS source — `ore_ont_10140` needs
  12, and raising it *adds* entailments, so it is asymmetric and needs an oracle.
- Doc drift fixed: `dkey_post_nnf_enabled`'s header claimed "Default OFF" while the code has been
  default-ON since 0.4.8.

## [0.4.12] — 2026-08-02

**Iterative deepening for the wedge search, default ON.** `HYPER_WEDGE_DEPTH = 256` was a fixed
constant, and a fixed constant is wrong in both directions at once.

**Curated `wine` now classifies by default in 73.9 s.** It previously did not finish — and the
project's own record closed that case as needing a nominal re-architecture, *"no cheap entry,
DON'T reopen w/o new evidence."* This is that evidence, and it came from the depth schedule
without touching nominals at all.

### The defect

| | `ore_ont_10407` | `ore_ont_2182` |
|---|---|---|
| genuine proof depth | **319** — 256 too LOW | **≤7** — 256 is 36× too HIGH |
| at a better depth | 512 → 10.0 s, `stalled=0` | 8 → 1.01 s, *more* subsumptions than 256 gave |
| cost of the wrong cap | **4.4× more work than completing** — a capped branch cannot conclude, so the search re-descends through sibling disjuncts | 2,357 stalled pairs |

**FP-safe by construction:** a depth cap can only *suppress* an `Unsat`, never manufacture one,
so deepening only ever adds entailments.

### Measured, ORE-wide (1,920 ontologies, two sequential arms, 60 s cap, single-thread)

| | |
|---|---|
| recoveries (`dnf` → `ok`) | **16** |
| regressions (`ok` → `dnf`) | **0** |
| materially faster / slower (>25% and >2 s) | **12 / 0** |
| aggregate wall over 1,730 both-completing | −2.1% |
| closure losses | **0** (superset verified on the fixed build, 26 ontologies) |

**Budgeted runs improve most**, because the shallow level *decides* rather than re-works: on
`wine` at `--pair-timeout-ms 25`, **108.8 s → 4.60 s** with identical subsumptions. At a 5 ms
budget the OFF arm has 3,340 pairs burning their full budget then stalling; the ON arm has 3,454
resolved at depth 8 in the **0 ms** bucket.

### A regression caught, and the fix it forced

The first corpus sweep found **one** `ok → dnf` — `ore_ont_13991`, 3,119 classes, 32.79 s → DNF.
A rule fixed *before* the numbers blocks a flip on any such transition, so the default stayed OFF
until it was understood. Cause, confirmed by dose–response rather than arithmetic: the shallow
budget was a **per-pair constant**, so its cost scaled with the pair count — quadratic in classes.
At 56,760 pairs, 5 ms each is 284 s of pure tax. (5 ms → DNF; 1 ms → completes at 90.31 s, an
overhead of 57.5 s against a predicted 57 s.)

`RUSTDL_ID_SHALLOW_WASTE_MS` (default 1000, `0` reverts) now budgets **wall wasted by shallow
phases that did not decide** — metering the harm in its own units. The obvious alternative, a
consecutive-miss counter, was implemented and **refuted by measurement**: `ore_ont_13991`'s
shallow phase *does* decide 84 pairs while missing 200, so interleaved successes keep resetting
it. Verified at the boundary — `WASTE_MS=0` reproduces the regression exactly.

### How it was found

Two independent root-cause investigations, each of which **refuted its own starting hypothesis**
before converging here. Cardinality: `ore_ont_10407` has **zero** `Max`/`Exact` axioms — 41 of its
"50 cardinality axioms" are `MinCardinality(0 R)` tautologies. Blocking: 81.82 s vs 81.96 s with
anywhere-blocking toggled, and double-blocking already firing on 74.5% of calls.

Also corrected: CLAUDE.md claimed nominal-tainted nodes are excluded from **both** blocking
predicates. Only `is_blocked_anywhere` excludes them; `is_blocked_ancestor` does not — and
classify uses ancestor-blocking by default, so the deferred issue-#35 v4 redesign never applied
to the classify path.

### Gates

FP=0 net **11 VERIFIED, closures exact**; 1,510 tests; fmt and clippy clean; sabotage 8/8.
`RUSTDL_ITERATIVE_DEEPENING=0` reverts.

## [0.4.11] — 2026-08-02

The release that a **full-corpus regression sweep** made possible — and the one it caught a
regression in. 1,920 ontologies measured v0.4.6 vs this tree, single-thread, 60 s cap:

| | |
|---|---|
| recoveries (`dnf` → `ok`) | **91** |
| regressions (`ok` → `dnf`) | **4 — all fixed below** |
| answer changes among completing ontologies | **0** |
| materially slower (>25% and >2 s) | **0** |
| peak-RSS growth >2× and >500 MB | **0** |

FP=0 net 11/11 VERIFIED, closures exact; 1479 tests pass.

### Fixed — a regression this project shipped in v0.4.8

`RUSTDL_CLASSIFY_INCONSISTENCY` was flipped to default ON after a cost benchmark over **12**
ABox-bearing ORE ontologies showed −1.5%, "a wash". Twelve was not enough. It put an
**unbounded** named-individual fixpoint in front of every classify, and on ontologies carrying
60k–110k ABox assertions that fixpoint dominates the entire run:

| ontology | v0.4.6 | v0.4.10 | **v0.4.11** |
|---|---|---|---|
| `ore_ont_10838` | 4.86 s | DNF @60 s | **6.43 s** |
| `ore_ont_15846` | 21.73 s | DNF @60 s | **8.24 s** |
| `ore_ont_16315` | 4.42 s | DNF @60 s | **4.35 s** |
| `ore_ont_3087` | 4.80 s | DNF @60 s | **5.01 s** |

Now bounded by `RUSTDL_CLASSIFY_INCONSISTENCY_MS` (**default 3000**, `0` = unbounded), on the
**classify path only** — `is_consistent`, `realize`, `materialize_*` and `diagnose` keep the
unbounded pre-check, because for them it *is* the point of the call. Safe by the pre-check's own
contract: it is a sound under-approximation, so a clash proves inconsistency while no clash
yields no verdict and the caller proceeds exactly as before. A timeout costs at most the
detection, never correctness. The motivating fix is intact — `classify --json family.ofn` still
reports `consistent: false` with 58 unsatisfiable classes and still agrees with `rustdl consistent`.

**The 3000 ms default was measured, not guessed** — and the guess was wrong. "A few hundred ms"
was proposed and falsified: `family.ofn`'s ABox saturation alone takes **~2.0 s** (506
individuals, a 267k-edge role-chain closure), so a sub-2.5 s budget would have silently
re-broken the very detection the flag exists for — a fix quietly reintroducing the bug it repairs.

### Added — `--global-timeout-ms` now bounds preparation (`RUSTDL_PREP_DEADLINE`, default OFF)

`saturate()` and `from_internal()` ran before any deadline check, so the global timeout bounded
only the *search*. Under a **1 ms** budget, 77 of 252 DNF ontologies still burned ≥10 s and 26
never finished `from_internal` (`ore_ont_10926`: 84.9 s against a 1 ms promise). A caller asking
for a bounded classify got an unbounded one. `ore_ont_1028` at a 3000 ms budget: 9.38 s → 3.19 s,
flagged **incomplete** — an early abort degrades to the EL closure read-off and can never present
itself as complete. Known residual: `convert_ontology` and `collect_el_rules` remain uninterruptible.

### Fixed — the attribution banner was lying by ~1500×

`tier_walk_wall_ms` was a **residual subtraction**, so all unbudgeted prep was charged to the
tier walk: `ore_ont_1028` reported **8964 ms for a 6 ms walk**. This is the instrument that
falsified an earlier taxonomy of this corpus. Phases are now measured directly across nine line
items with the leftover explicitly named `unattributed_ms`, so a residual can never masquerade
as a phase again. The real cost is finally visible (`prepare=4486`, `saturate=2215`).

### Added — domain absorption (`RUSTDL_DOMAIN_ABSORPTION`, default OFF)

`(≥1 R) ⊑ C` is logically identical to `ObjectPropertyDomain(R, C)` but fell into
`residual_gcis` — a global disjunction on every node. Note `absorb.rs` lowers *every*
`ObjectPropertyDomain` that way too, so this was universal. Sound and completeness-preserving;
recovers 4 DNFs (`ore_ont_3281`: 28 residuals → 0, DNF@300 s → 11.4 s). Default OFF: it alters
the absorbed TBox of 1,030 of 1,913 pool ontologies without a wall check at that scale.

**Its motivating thesis was refuted and is documented as such** — see
`docs/2026-08-01-absorption-is-the-bottleneck.md`, which carries a retraction banner. Residual
count does *not* predict tractability: driving 54 ontologies to **zero** residuals recovered
**1**, while 77 that kept residuals recovered **3**, and per unit size the signal is
**AUC 0.480 — below chance**. Qualified-`∃` absorption is therefore a documented **NO-GO**.
The discriminator is `concept_rule_or` (AUC 0.849, holding 0.81–0.88 within every size band;
56% of completing ontologies have zero disjunctive concept rules against 15% of failing ones).

### Also

- Guarded the shipped per-pair amortizer's delta arithmetic, which a misdirected sabotage
  panicked on `ore_ont_10019` with no test catching it.
- **Unguarded, recorded:** sabotage showed nothing pins the `CLASSIFY_INCONSISTENCY_MS` value —
  slashing it to 1 ms passes every canary, because cheap synthetic clashes are also caught by
  unbudgeted routes. The substance of that fix is untested.

## [0.4.10] — 2026-08-01

Throughput release, and the first one chosen by a **pre-registered experiment** rather than by
a profile. `RUSTDL_CLASSIFY_LABELS_AMORTIZE`, **default ON** (`=0` reverts). FP=0 net 11/11
VERIFIED with exact closures; 1418 tests pass.

### The fix

`classify_labels` obtained a `HyperEngine` per **class**, full-rebuilding `ClauseIndexes` every
time: SP2.1/SP3 seed clauses are absent from `base_indexes` while `RUSTDL_SAT_SEED` defaults ON,
so v0.3.39's per-**pair** amortization never reached the label-cache build. The amortizer already
existed at `hyper.rs:1349/1167` and was simply unwired for this path.

Measured on the merged binary at a non-truncating `--pair-timeout-ms 1000`, **closures
byte-identical both ways**:

| ontology | OFF | ON | |
|---|---|---|---|
| `ore_ont_12698` | 100.46 s | **5.40 s** | −94.6%, 21,313 rows identical |
| `ore_ont_1508` | 202.79 s | **98.47 s** | −51.4%, 13,926 rows identical |

`label_cache_build` alone: 163.3 → 59.9 s and 95.7 → 2.05 s.

### Why it was adjudicated rather than simply built

Two independent reviewers reached **opposite verdicts** on this exact code. One reported it
CONFIRMED and DNF-scale; the other reported SUSPECTED and flagged its own measurement as
confounded — because `RUSTDL_SAT_SEED=0` removes the seed *clauses* as well as the rebuild, so
less work of every kind is done. That objection was correct, and it applied to **both** reviewers'
numbers.

So a third arm was built and the design, predictions and decision rule were committed *before*
running it (`docs/superpowers/specs/2026-08-01-clauseindex-per-class-adjudication.md`):

| arm | `ore_ont_1508` | `ore_ont_12698` |
|---|---|---|
| A stock | 197.83 s | 98.78 s |
| B `SAT_SEED=0` (the confounded arm) | 114.12 s | 49.80 s |
| **C seeds kept, index amortized** | **94.89 s** | **5.33 s** |

**Arm C beats arm B on both.** Keeping the clauses and removing only the rebuild is *better*
than removing both, so the cost was the rebuild. The methodological objection was right; the
substantive suspicion that "wiring the amortizer buys little" is refuted.

Method notes that make the number trustworthy: min-of-3 **interleaved** (drift over minutes is
comparable to the effect size); one probe at a time on an idle host; each binary pinned to a
uniquely named path and `sha256`-verified; and **arm C was proven to fire** via a marker with
both branches demonstrated able to print — 6 of 6 runs took the delta path, 0 fell back. A
4-row closure diff at a *truncating* budget was correctly diagnosed as budget noise, not an arm
effect: timed-out pairs vary 57–68 across runs of a single binary and the same 4 rows differ
*within* one flag setting.

### Scope

Phase 2 attribution of the 199 remaining DNF ontologies puts **123 of them (62%) in the
`after_prepared` phase — which is the label-cache build**, i.e. exactly this code. How many that
converts into completions is measured separately and is not claimed here.

### Also

- Empty-value semantics aligned with the seven other default-ON flags: only an explicit `=0`
  reverts. The prior assertion required an empty value to *disable*, which was right under
  default-OFF but would have let `VAR=$UNSET` silently revert a 52–95% improvement.
- **Recorded, not fixed:** a misdirected sabotage landed in the *shipped* per-pair amortizer and
  panicked on `ore_ont_10019` with no in-tree test catching it — an unclaimed coverage gap around
  live delta arithmetic.
- Sabotage of the new canaries: 4 run, 2 caught, **2 survived**, with both survivors and their
  reasons recorded in the canaries' doc comments — one of which stated a rationale the sabotage
  disproved and was corrected.

## [0.4.9] — 2026-08-01

**Soundness release. Fixes a false-positive subsumption present since at least v0.4.6.**
FP=0 is this project's absolute invariant and it was being violated. FP=0 net 11/11 VERIFIED,
closures exact; 1413 tests pass.

### Fixed — a FALSE POSITIVE across `xsd:float` / `xsd:double` (unflagged)

`parse_float_oneof` folded `xsd:float` and `xsd:double` into a single f64-keyed `fo:` DKey
bucket, so

```
EquivalentClasses(:AF DataSomeValuesFrom(:h DataOneOf("1.0"^^xsd:float)))
EquivalentClasses(:AD DataSomeValuesFrom(:h DataOneOf("1.0"^^xsd:double)))
```

were reported **equivalent**. They are not: the two datatypes have distinct value spaces.

**Reproduced independently before the fix was accepted.** rustdl emitted `equiv AD AF`;
Konclude declared both classes and reported **no** relation between them. The discriminating
control is what makes that conclusive — on a float-vs-**float** variant Konclude *does* report
mutual subsumption, so its silence on float-vs-double is a genuine non-entailment rather than
the under-reporting Konclude is documented to do elsewhere (`ore_ont_10407`, `ore_ont_9540`).
HermiT produced an empty output on this fixture and is **not** evidence here.

**Pre-existing, not a regression** — a pinned v0.4.6 binary reproduces it. It escaped the FP=0
net because the curated corpus is **inert** for the DKey area, exactly as
`datatype_value_membership.rs` warns: *"the corpus has NO such clash, so these canaries are the
ENTIRE safety net."* This is what that warning looks like when it comes true.

Fix: split into `fo:` (f32-rounded) and a new `dbo:` (f64) bucket. **Unflagged and on by
default** — a soundness fix is not opt-in. It is also a prerequisite: the seeding below would
be FP-unsound on `fo:` without it.

### Added — numeric `DataOneOf` bucket seeding (`RUSTDL_DKEY_ONEOF_SEED`, default ON since 2026-08-03)

The sixth D10-class bug: the five numeric `DataOneOf` DKey buckets were never collected into
`seed_dkey_subsumptions`, so they received no told `DKey ⊑ DKey` edges and no disjointness,
while `is_pure_el` still certified the saturator complete. Recovers 3 of the 5 oracle-confirmed
results (`F ⊑ D`, `D ⊑ E`, and the `∀p.DataOneOf(1,2) ⊓ ∃p.{3}` unsatisfiability). The other
two (`C ≡ F` in both directions) turned out to be **already held** — identical sets intern to
one DKey — and are reported as not-reproducible rather than claimed.

11 canaries; **7 sabotages, 7 caught, 0 survivors**. Konclude and HermiT agree exactly on both
fixtures. Both DKey discriminators unmoved at each flag setting (`ore_ont_9347` = 113,
`ore_ont_5368` = 18,620,251 — and `9347` alone cannot validate this area).

Shipped default OFF pending a cost measurement: the disjointness half is O(k²) per component,
the same shape that caused the v0.3.29 conversion DNFs.

**FLIPPED ON 2026-08-03** after that measurement — `rustdl tbox-stats` over all 1,920 ORE
ontologies in four arms, recording `concept_rules`, told-subsumer edges, told-disjoint pairs
AND conversion wall, with timeouts counted per arm. This flag moves **nothing**: zero
ontologies change any count, zero gain a conversion timeout, `ore_ont_5368` unmoved at
18,620,251. The numeric `DataOneOf` pattern does not occur in ORE at all, so the corpus shows
only that the flip is free; the evidence that it is right is the canaries plus a
Konclude ∪ HermiT adjudication of the ladder fixture (ON reproduces the oracle union exactly,
FP=0/MISSED=0; OFF misses three subsumptions). See `docs/2026-08-03-dkey-volume-scan.md`.

### Added — DKey disjointness emit ordering (`RUSTDL_DKEY_EMIT_ORDER`, default ON since 2026-08-03)

`seed_disjoint_bucket::try_emit` ran `emitted.insert(pair)` **before** the droppable test, so a
pair spanning two role components was permanently consumed by whichever component the
`BTreeMap` reached first — even when the *other* component was the one that could consume it.

**The defect is live at default settings and is NON-MONOTONIC:** `∀p.[0,5] ⊓ ∃p.{9}` is
unsatisfiable alone, but adding an *unrelated* property `q` that merely mentions the same two
keys makes it satisfiable again. Adding an axiom must never remove an entailment.

Resolves the previously unexplained anomaly where `RUSTDL_DKEY_MERGING_GATE=0` found *fewer*
entailments than the default, with no fourth mechanism. The one-line reordering was necessary
but not sufficient — the `RUSTDL_DKEY_SPLIT_STATS` counters were per-`(pair, component)` and
double-counted once declining stops spending a pair.

Shipped default OFF pending an ORE volume scan: this lever emits *more* axioms, so the risk is
both FP and volume re-inflation — precisely what the 2026-07-30 non-merging-component gate
exists to prevent.

**FLIPPED ON 2026-08-03.** The scan (1,920 ontologies × 4 arms) found **exactly one ontology
whose numbers move**: `ore_ont_9303`, `concept_rules` 8886 → 8887 and told-disjoint pairs
6669 → 6670 — a corpus-wide total of **+1 axiom**. No ontology over the >2× / >100k threshold,
none gained a conversion timeout, none >2× slower, `ore_ont_5368` unmoved. Because the risk
direction is a FALSE POSITIVE the mover was adjudicated, not just counted: its classify output
is byte-identical ON vs OFF, and its verdict (inconsistent, 726/726 unsat) is confirmed by
**both** Konclude (727/728 classes ≡ owl:Nothing) and HermiT (InconsistentOntologyException).
FP=0. See `docs/2026-08-03-dkey-volume-scan.md`. Sabotage reported as run: **3 of 4 caught, 1 survived** (ignoring the
collapse/broadcast split left all six canaries green, so they pin the split's FP-safety but not
its volume bound).

## [0.4.8] — 2026-08-01

Correctness release. Three wrong answers fixed, all flipped **default ON** (`=0` reverts
each). FP=0 soundness net **11/11 VERIFIED, closures exact**; workspace tests 1396 passed,
0 failed.

### Fixed — `classify` and `consistent` no longer contradict each other

`RUSTDL_CLASSIFY_INCONSISTENCY`. On `family.ofn` — which HermiT and Konclude both call
inconsistent in under a second — `classify --json` reported `"consistent": true` with
`"unsatisfiable": []`, while `rustdl consistent` on the **same file** reported
`inconsistent`. Two CLI surfaces disagreeing, and the classify answer was simply wrong.

The `top_is_unsat` test existed only in `classify_pure_el`, and `abox_saturation` was absent
from `classify.rs` entirely. The fix extracts `abox_saturation_inconsistent` so
`is_consistent` and `classify` share **one** function, and routes a positive verdict through
the existing A1 `classify_inconsistent` rather than adding a second mechanism.
Now: `consistent=false`, 58 unsatisfiable, agreeing with `rustdl consistent`.

**The soundness subtlety is respected and tested:** all-named-classes-unsatisfiable is *not*
inconsistency — `{A⊑⊥, B⊑⊥}` empties every named class yet has a non-empty domain and is
consistent. The test is that **⊤** is unsatisfiable. Verified: that ontology still reports
`consistent=true` with 2 unsatisfiable classes, and sabotaging the inference fails the test.

**Cost measured before flipping**, because the flag adds an ABox pass to the classify path:
over 12 ABox-bearing ORE ontologies, **−1.5% aggregate** (a wash). Six initially appeared to
change output; an **OFF-vs-OFF control reproduced the same diffs**, and with banner lines
stripped all six bodies were byte-identical — nondeterministic timing banners, not answers.

### Fixed — two D10-class bugs (gate certifies complete, engine drops the axiom)

Both were found independently by two reviewers. This is the shape that has shipped three
times here: the reasoner returns a wrong answer *and* reports `incomplete: false`, so the
user gets no signal.

- **`RUSTDL_EL_BOT_FILLER`** — `is_el_concept` admitted `Bot` as an ∃-filler while the
  lowering dropped the axiom, so `X ⊑ ∃r.⊥` was certified pure-EL-complete and reported
  satisfiable. Konclude and HermiT both say `X ≡ Nothing`. Fixed in the **saturator**, not by
  tightening the gate — tightening only relocates the MISS to the hybrid path, whereas the
  existing `directly_unsat` / `process_unsat` machinery derives it for free. Made **total**
  (a recursive predicate) rather than a `Bot` match arm, which would still have missed
  `∃r.∃s.⊥`.
- **`RUSTDL_DKEY_POST_NNF`** — `dkey_components` ran **pre-NNF**, so a `∀p.DKey` that only
  exists *after* NNF (from `¬∃q.¬DKey`, legal OWL 2 DL) marked neither `merge_inducing` nor
  collapse/broadcast and its disjointness pair was dropped. Konclude says `Negated ≡ Nothing`;
  rustdl said satisfiable under every flag. This was a **completeness regression** introduced
  by the 07-20/07-30 DKey gates, which *either gate alone* loses. Fixed by an NNF-aware scan
  rather than by moving the pass, because `NEG_TO_BOT_GCI` needs pre-NNF in the same function.

**Evidence discipline:** the curated corpus is inert for the DKey area by its own admission,
so the FP=0 net shows *non-regression* only — the canaries and Konclude ∪ HermiT adjudication
are what carry the DKey fix. Sabotage results are reported as run, not as hoped: 8 of 9 caught
for the Bot fix and 4 of 6 for the DKey fix, with the survivors explained in-code rather than
the tests overclaiming.

### Explained

The previously unexplained anomaly where `RUSTDL_DKEY_MERGING_GATE=0` found *fewer*
entailments than the default — "adding sound disjointness loses entailments" — is now
understood: `seed_disjoint_bucket::try_emit` runs `emitted.insert(pair)` **before** the
droppable test, so when a pair spans two components whichever the `BTreeMap` reaches first
consumes it permanently. The merging gate was accidentally masking this.

### Known, not fixed

That `emitted`-before-`droppable` ordering is a **third latent completeness defect** with a
one-line fix. Deliberately left out of this release to keep it a separate, separately-gated
change rather than muddying two correctness commits.

## [0.4.7] — 2026-08-01

Classification-throughput release, driven by the first peer-triaged
characterization of rustdl's did-not-finish tail. Two levers flip default ON;
two more ship default OFF. FP=0 soundness net **11/11 VERIFIED, closures exact**
(galen 27997, notgalen 32739, sio 8904, ore-10908 6001, wine 653, pizza 499,
alehif 247, ro 158, ore-15672 142, sulo 51, bibtex 16).

### Why this release exists

Of 1,920 ORE ontologies, rustdl fails to classify 257 within 120 s single-thread.
Given the same ontologies at the same cap, **Konclude classifies 242 of them (94%)
at a median wall of 3.47 s**; HermiT and KM each rescue none of the remaining 15.
Three independent reasoners failing exactly the same 15 is the strongest available
evidence that only ~6% of the tail is intrinsically hard — **the rest is rustdl's
gap.** That reverses the account carried in `CLAUDE.md`, which described this tail
as intrinsic SROIQ hardness with "no cheap entry"; the defensible version of that
claim is narrower — the mechanisms previously *investigated* were measured out, and
no peer had ever been asked. The rustdl side was re-validated uncontended (0 of a
seeded 20 complete sequentially), so 242 is measured, not inferred.
See `docs/benchmarks/2026-08-01-dnf257-characterization.md`.

### Added — default ON (`=0` reverts)

- **`RUSTDL_FAST_DIRECT_SUBSUMERS`** — `direct_subsumers` recomputed an O(k²)
  transitive reduction **per class, inside the output loop**. `ore_ont_10125`
  finished *classifying* in ~15 s and then spent **≥385 s emitting**:
  **DNF at 900 s → 14.70 s complete (>61×)**. Output is byte-identical including
  ascending order; an ordering sabotage is one of four canaries. No regression off
  the hot path (`ore_ont_9498`, 305 k classes: 12.81 → 12.85 s).
- **`RUSTDL_FRAGMENT_BARE_DECL`** — `is_el_axiom` / `is_saturator_axiom` fell
  through to `_ => false` for bare `SymmetricObjectProperty` and
  `InverseObjectProperties` **declarations**, so merely *naming* such a property
  refused the ontology the saturation fast path. **44 of the 257 now classify**
  (`# mode: pure EL`, median 1.87 s; `ore_ont_8470` 132.76 s → 0.53 s).
  A declaration is admitted **only when the role's edge set is provably unread** —
  no concept occurrence, no domain/range, not a chain part, no characteristic, no
  ABox assertion, and not below an observable role (closed to a fixpoint).
  Admitting them unconditionally would be a D10 bug; the blocked set is 76 and this
  admits 44, because the other 32 have a genuinely *read* role.

### Added — default OFF (opt-in)

- **`RUSTDL_SAT_ENQUEUE_DEDUP`** — records a derived subsumer pair at *enqueue*
  rather than *pop*, giving the worklist an in-queue membership test. On
  `ore_ont_11085` the queue reached 1.07 G entries (8 GB), with ≥414 M of 927 M
  pushes provably duplicates. **OOM-abort at 16.96 GB → completes at 491 MB (35×).**
  Default OFF because it does **not** recover the ontology at a production budget —
  687 s, still DNF at 120 s. It removes the memory wall, not the compute cost.
- **`RUSTDL_LAZY_ABOX_SATURATION`** — elides an EL saturation that is provably dead
  on ABox-free input. Correct, but its **premise was refuted as a performance
  lever**: measured 0.02–0.14 s (0.2–0.3%), RSS flat, because every large ABox-free
  ontology takes the pure-EL fast path and never reaches that call site. The
  "~62% of prep" estimate should not be quoted.

### Behaviour change worth knowing

A bare `InverseObjectProperties` declaration was a common in-tree idiom for forcing
an ontology *out* of the EL fragment in tests. `RUSTDL_FRAGMENT_BARE_DECL` dissolves
that idiom: three canaries used it as a device and now take the fast path. In every
case the **verdict** assertions still passed and only the telemetry assertions
failed, confirming a dispatch change rather than a regression; those canaries now
pin the flag off. New tests should not rely on a bare declaration to leave the
fragment.

### Root causes established

- **`ore_ont_11085`'s ≥21.7 GB is the subsumer worklist**, not the D4 dense
  matrices (re-refuted: both `IdMatrix`es stay at 62 MB) and not Tseitin minting
  (`next_id` never moves). Counterfactual: deleting only its 732
  `SubClassOf(owl:Thing, C)` axioms gives 0.72 s / 166 MB.
- **The hard tail is not the wedge.** `ore_ont_10019` profiles **84.6% main
  tableau** vs 15.3% `hyper::solve`; `ore_ont_1508` is 22,454/22,456 `match_body`
  samples under the *Horn* path. `ore_ont_10019` is a **47-class** ontology using
  0.01 GB that Konclude does in 0.06 s.
- **The "94% unattributed" budget overrun** is `saturate()` and `from_internal()`
  running *before* any deadline check (so `--global-timeout-ms` bounds only the
  search) plus `tier_walk_wall_ms` being a residual subtraction that mis-attributes
  the unbudgeted prep. Not yet fixed.

### Known, not fixed

Two new D10-class correctness bugs (`is_el_concept` admits `Bot` as an ∃-filler
while the lowering drops the axiom; `dkey_components` runs pre-NNF so a post-NNF
`∀p.DKey` is missed — a completeness regression from the 07-20/07-30 DKey gates),
and `classify --json` reporting `"consistent": true` on `family.ofn` where
`rustdl consistent` reports `inconsistent`. See `docs/reviews-2026-08-01/`.

## [0.4.6] — 2026-07-31

Completeness and conversion-scale release. Four ORE ontologies go from
did-not-finish to classifying, and five axiom shapes the EL saturator silently
dropped are now handled. FP=0 verified on the release candidate (soundness net
22/22, closures exact).

### Fixed

- **`⊑ ⊥` completeness — five silently-dropped axiom shapes.** The EL saturator
  admitted these through its fragment gate and then discarded them, so a closure
  could be reported complete while the axiom was ignored: `A ⊓ B ⊑ ⊥` (new
  `ConjunctiveUnsat` rule), `⊤ ⊑ ⊥` (marks every user class unsat at seed time),
  `∃r.A ⊑ ⊥`, and the "poisoned role" family `∃r.⊤ ⊑ ⊥` /
  `ObjectPropertyDomain(r, ⊥)` / `ObjectPropertyRange(r, ⊥)`. Inconsistency is now
  reported from `⊤` being unsatisfiable, **not** from all named classes being
  unsatisfiable — `{A ⊑ ⊥, B ⊑ ⊥}` empties every named class yet has a non-empty
  domain, so the old signal was wrong. Known remaining gap: role-chain-induced
  poison (`SubObjectPropertyOf(Chain(t,u), r)` + `Domain(r) = ⊥`) is still missed.
- **Nominal-forcing range is a `DKey` merge source.** A range filler that forces
  every successor to be the *same* individual collapses them via the o-rule, so two
  distinct data values share one node label. Detected by adversarial review; the
  filler test was replaced by treating any range / `∀` as merge-inducing, because
  `ObjectPropertyRange(p, C)` with `C ⊑ ObjectOneOf(o)` is not syntactically
  detectable. Regression: `tests/dkey_nominal_range_merge.rs`.
- **`realize` no longer hangs on an unbounded internal classify**, and a per-pair
  `NoVerdict` degrades to a sound MISS instead of erroring the whole call.
- **Soundness net fails loudly on missing fixtures.** Absent REQUIRED fixtures used
  to skip while the suite still reported `ok` — of 22 fixture blocks, 9 could skip
  silently. They now fail with the path and the fetch hint.

### Added

- **`RUSTDL_NEG_TO_BOT_GCI`** (default ON, `=0` reverts) — canonicalizes
  `X ⊑ ¬Y` to `X ⊓ Y ⊑ ⊥` pre-NNF, so the EL fast path can take ontologies that
  previously fell to the hybrid path. Logically equivalent, hence FP-safe by
  construction. Measured over ORE: **13 ontologies flip to pure-EL, 5 of which were
  DNF at 60 s**; `ore_ont_9318` 23.93 s hybrid → 0.90 s pure-EL with a
  byte-identical 19,470-row closure.
- **`RUSTDL_DKEY_MERGING_GATE`** (default ON) — skips `DisjointClasses(DKey, DKey)`
  seeding for role components containing no merge-inducing role, where the pairs can
  never be consumed. `ore_ont_9347`: 49,571,087 axioms → 113; classify
  **DNF @703 s / 70.7 GB → 10.72 s / 227 MB**. `ore_ont_11287` also recovered.
- **`RUSTDL_DKEY_COLLAPSE_SPLIT`** (default ON) — distinguishes COLLAPSE merge
  sources (functional / inverse-functional / `≤n`, which force two successors onto
  one node) from BROADCAST sources (a `DKey`-bearing range or `∀`, which put one key
  on every successor), and omits value×value pairs where only broadcast applies.
  **`ore_ont_7607` and `ore_ont_1685`: DNF → complete** (5.4M axioms → ~9k, 7.9 GB →
  0.26 GB); `ore_ont_12182` 46.7 s → 2.4 s with byte-identical answers. Verified
  answer-identical across all **286** affected ontologies.
- **Pseudo-model realize shortcut** (`RUSTDL_PSEUDO_MODEL`, default ON, #57) — prunes
  per-(individual, class) probes against one witness model. Subtractive only, so
  completeness-preserving and FP-safe.
- **Diagnostics.** `RUSTDL_TRACE_RSS` emits per-phase RSS markers (the right tool for
  localising a stall, since the wall-breakdown banner only prints on completion), and
  `RUSTDL_DKEY_SPLIT_STATS` reports how many `DKey` pairs the collapse/broadcast split
  would drop, without changing emission.

### Changed

- **Classify fast paths no longer build a full `PreparedOntology` to read one
  verdict.** Both fast-path sites previously constructed the whole object —
  second EL saturation, `HyperCache`, `ConsistencyCache`, NNF, absorb — solely to
  consult the ABox inconsistency check, then discarded it. They now build only the
  eight fields that check reads. **16.7–35.5% wall** on fast-path ABox ontologies
  (`ore_ont_10073` 9.74 s → 6.28 s), with the hybrid path untouched.

### Notes

- The `Bucket A / Bucket B` characterisation of the did-not-finish tail is
  **falsified** — it was an artefact of which budget each phase honours, not two
  mechanisms. The tail is 12 ontologies and one mechanism. Three candidate causes are
  now eliminated by measurement (axiom volume, deadline enforcement inside the Horn
  fixpoint, and the per-class clause clone at ≤6.3%). See
  `docs/2026-07-30-dnf-tail-is-one-bucket-not-two.md` and
  `docs/handoff-2026-07-31.md`.

## [0.4.5] — 2026-07-26

### Added

- **Explanation surface in Protégé.** rustdl's justifications and proofs are now
  exposed in the Protégé UI:
  - The Explanation ("?") dialog offers **rustdl** (minimal justifications) and
    **rustdl (laconic)** as explanation sources, via the OWL Explanation API.
    Minimal justifications are fail-hard verified against the source ontology
    (anti-fabrication); laconic fragments are sound-by-construction weakenings.
  - The proof view shows rustdl step-level EL proof trees (degrading to a
    single-step justification outside the EL fragment), via the liveontologies
    proof-service extension point. Requires Protégé's proof-explanation plugin
    installed; justifications work without it.
- **`rustdl justify --json`** and **`rustdl prove --json`** — machine-readable
  justifications (with `minimal` / `laconic` / `enumeration_complete` honesty
  flags) and EL proof trees (or a justification fallback), each axiom rendered as
  a self-contained OWL Functional Syntax document. Schemas in
  `docs/json-schema.md`; these are the bridge the plugin consumes.
- **Dropped-axiom surfacing + graceful degradation** (#43): the `classify` /
  `consistent` / `realize` `--json` outputs (and the Python surface) now report
  a `dropped` block of per-kind counts for axioms outside the supported fragment,
  so a sound under-approximation is visible rather than silent.

## [0.4.4] — 2026-07-25

### Added

- **Complex (anonymous) class-expression queries** (#48): satisfiability of an
  arbitrary class expression, subclass-entailment between two class expressions,
  and instance retrieval for a class expression — exposed on the reasoner API,
  Python (`class_expression_satisfiable` / `entailed_subclass` / `instances`),
  and the CLI (`sat-expr` / `subclass-expr` / `instances-expr`, each with
  `--json`). Entailment-backed and sound; the `incomplete` flag marks a sound
  under-approximation. HermiT-oracle-tested.
- **Protégé plugin shows its version in the Reasoner menu** — the reasoner now
  appears as `rustdl <version>` (like ELK/HermiT), via `${project.version}`
  resource-filtering of `plugin.xml`.

### Fixed

- **Wedge clausifier: `DisjointUnion` covering direction** (#40) — the covering
  constraint of `DisjointUnion(C, D₁ … Dₙ)` is now clausified in both directions,
  closing a wedge-classify completeness gap.

## [0.4.3] — 2026-07-25

### Added

- **Protégé plugin now answers the inferred query surface** exposed by the
  reasoner in 0.4.2's groundwork (#44–#47): object/data **property hierarchy**,
  **disjoint** classes & properties, **same/different individuals**, and
  **object/data property values** — previously the plugin returned empty node
  sets for these. Wired to the new `disjoint` / `property-hierarchy` /
  `individuals` / `property-values` `--json` subcommands; the plugin advertises
  all nine OWLAPI `InferenceType`s and surfaces each query's `incomplete` flag as
  a logged sound-under-approximation warning. Only complex-class-expression
  queries remain unbacked.

### Fixed

- **Inferred-query completeness honesty (`--json` `incomplete`).** `disjoint` /
  `individuals` no longer report `incomplete:false` when a per-pair probe hits
  the node-cap on the unbounded path; `inferred_object_property_values` now gates
  `incomplete` on the ABox saturator's actual edge-complete fragment (a sound
  over-approximation) instead of a mismatched proxy — so `incomplete:false` is a
  genuine "no entailed edge missed" guarantee. Plus doc/test hardening across the
  new query surface.
- **Protégé plugin: inconsistent ontologies** now surface as
  `InconsistentOntologyException` (Protégé's standard inconsistency state) rather
  than a generic reasoner error, for the new query families too.

## [0.4.2] — 2026-07-24

### Added

- **Protégé plugin auto-update.** The plugin bundle now advertises an
  `Update-Url`, and the repo hosts an `update.properties` descriptor that the
  release workflow keeps current on every tag — so an installed rustdl plugin
  offers new versions through Protégé's **Check for updates**. (Registration in
  Protégé's in-app "Check for plugins" catalog is a separate one-time step with
  the Protégé maintainers.)

_Plugin packaging only; the Rust reasoner, wheels, and CLI binaries are
identical to 0.4.1._

## [0.4.1] — 2026-07-24

### Fixed

- **Protégé plugin now appears in the reasoner menu.** The v0.4.0 plugin jar's
  `plugin.xml` used a `<reasonerFactory name=.. factoryClass=../>` element, but
  Protégé's `org.protege.editor.owl.inference_reasonerfactory` extension point
  reads `<name value=../>` and `<class value=../>` **child elements** (as ELK and
  HermiT do). With the wrong schema Protégé loaded a null class name, threw an
  NPE, and dropped rustdl from the reasoner list. Verified in Protégé 5.6.9.
- gson is now embedded as a nested jar (`inline=false`, matching the ELK/km
  packaging) rather than unpacked, so no stray `module-info.class` lands loose
  in the OSGi bundle.

_No Rust/reasoner changes; this release only corrects the Protégé plugin
packaging (the wheels and CLI binaries are identical to 0.4.0)._

## [0.4.0] — 2026-07-24

### Added

- **Machine-readable `--json` output** on `classify`, `consistent`, and
  `realize` (a versioned `schema_version: 1` contract, golden-tested) — a stable
  bridge for tooling that needs to parse rustdl's results rather than scrape the
  human tab/`#`-comment output. See `docs/json-schema.md`.
- **Standalone cross-platform CLI binaries**, built in CI on every release and
  attached to the GitHub Release with `SHA256SUMS`: `x86_64`/`aarch64` Linux
  (fully static, musl), Apple-Silicon macOS, and Windows x86_64 (static CRT).
  See `docs/cli-binaries.md`.
- **Protégé reasoner plugin** (`protege/`): rustdl as a first-class Protégé
  reasoner — consistency, class hierarchy, unsatisfiable classes, and class
  assertions (types/instances) — with the platform binary bundled in the plugin
  jar, so there is no separate binary install or PATH setup. It is an OWLAPI
  `OWLReasoner` (BUFFERING) whose flag-driven `precomputeInferences` maps
  `CLASS_HIERARCHY`→`classify` and `CLASS_ASSERTIONS`→`realize` over the `--json`
  bridge, with a per-pair classify budget (`rustdl.pair.timeout.ms`) so hard
  ontologies degrade to a sound incomplete classification rather than hang. See
  `docs/protege-plugin.md`.

### Fixed

- Test and documentation hardening for the #38 completion-graph merge fix and
  the #39 nominal-cardinality realize typing.

## [0.3.41] — 2026-07-24

### Fixed

- **Completion-graph merge could corrupt the edge set (`remove_edge_recorded`).**
  `TableauContext::merge_into_with_deps`, when re-anchoring a merged node's
  incoming edges, located the forward edge on the union-find representative
  `y_eff = resolve(y)` but searched the mirror in-edge for the *unresolved*
  snapshot node `(role, y)`. When `y` had since merged into `y_eff`,
  `source.in_edges` can hold both a stale orphaned `(role, y)` and the live
  `(role, y_eff)`, so the wrong mirror was removed — a `debug_assert_eq!` panic
  in debug builds, and in release a silent wrong-edge removal (edge-set
  corruption). Now searches the mirror for `(role, y_eff)`, consistent with the
  forward-edge lookup and the `add_edge` forward/mirror pairing invariant.
  Surfaced by the issue #35 v4 reproducer; fixes #38. (The symmetric outgoing
  re-anchor was reviewed and is unaffected — it disambiguates by the per-entry
  stored endpoint.)

## [0.3.40] — 2026-07-23

### Fixed

- **`realize` / `materialize_inferred_class_assertions` no longer hangs on the
  issue #35 v4 nominal + number-restriction pattern** (`ObjectMinCardinality` +
  `ObjectOneOf` covering + `ObjectPropertyDomain`). At default settings realize
  on the 3-axiom reproducer now completes in ~0.75 s (previously non-terminating
  / >300 s). The fix is a deterministic **safety net**, not a completeness fix —
  realize returns a sound under-approximation (a MISS, never a false type) on
  inputs it cannot bound:
  - **Default per-pair realize bound restored.** `RUSTDL_REALIZE_PAIR_TIMEOUT_MS`
    now defaults to **750 ms** when unset (was: unbounded since 0.3.18); `=0`
    opts back into unbounded. Bounds each per-individual instance probe → realize
    terminates fast with a sound MISS. Affects only `realize`/`instances_of`/
    `is_instance_of` — not `classify` or `is_consistent`.
  - **Deterministic node cap.** `RUSTDL_MAX_NODES` (default 50000, `0` disables)
    caps the deadline-free tableau search; a trip yields a distinct `NodeCap`
    verdict that maps to `Ok(None)` (sound MISS / consistent under-approximation),
    with a **hard early-return** so it stops promptly. Never `Err`, never a panic,
    never a hang.

### Deferred / known-limitation

- The intended **complete** fix for issue #35 v4 — "nominals-first scheduling"
  (`RUSTDL_NOMINAL_FIRST`, **default OFF / opt-in**) — was implemented but
  **proven not to bound the reproducer**: `ObjectPropertyDomain(r,A)` absorbs to
  an untriggered residual disjunction that the concept-rule-keyed guard never
  intercepts, and the covering-disjunction + cardinality merge regenerates
  without bound. The scheduling machinery is left in place, dormant behind the
  opt-in flag, as the foundation for a proper redesign (sound nominal-aware
  blocking or the NN-rule). A HermiT-matching realization on this pattern remains
  a known limitation. See
  `docs/superpowers/specs/2026-07-23-nominal-cardinality-realize-termination-design.md`
  (§ Outcome) and the plan
  `docs/superpowers/plans/2026-07-23-nominal-cardinality-realize-termination-plan.md`.

## [0.3.39] — 2026-07-23

### Performance

- **Classify subsumption oracle: amortize the per-pair `ClauseIndexes` rebuild
  (wedge-heavy converging classify ~11–13% faster).** `HyperCache::decide_with_stats`
  cloned the full clause vector and rebuilt the entire `ClauseIndexes` on every
  decided pair — measured 13,772 rebuilds × ~34.6k clauses on `ore_ont_1508`, and
  11–15% of self-time on converging wedge-heavy classify. It now reuses the
  shared base `Arc<ClauseIndexes>` (built once in `HyperCache::build`) and applies
  only an O(#appended-clauses) per-pair delta: a `ClauseIndexSink` trait routes
  the base build and the per-pair delta through one shared `index_one_clause`
  (so `x_trigger`, `match_plans`, nonhorn/empty-body, and disjoint-pairs entries
  can never diverge), the engine branch-routes `clause(ci)`/`match_plan(ci)`
  between the shared base slice and a small per-pair extras slice, disjointness
  is read via a base+per-pair overlay (no HashSet clone), and the pair-invariant
  value-disjoint clauses are folded into the base once. **Verdict-preserving by
  construction:** classify closures byte-identical with the optimization on vs
  off (`RUSTDL_CLASSIFY_AMORTIZE_IDX=0`) and vs the pre-change baseline across
  ro/sio/sulo/pizza/galen + `ore_ont_1508`/`12698`; full suite green; a delta-vs-
  full-`build_clause_indexes` equivalence unit test guards the per-clause routine.
  `RUSTDL_CLASSIFY_AMORTIZE_IDX=0` restores the old clone+rebuild path. (Also
  fixes a latent OOB: the pre-existing prebuilt-index path pre-applied the
  Q-clause's `x_trigger` entry but not its `match_plans` entry.) Plan + advisor
  review: `docs/superpowers/plans/2026-07-23-classify-clauseindex-amortization-plan.md`.

## [0.3.38] — 2026-07-23

### Performance

- **Wedge `HyperEngine::is_blocked`: drop the per-call candidate-bucket clone.**
  The wedge's double-blocking check (the hottest self-time leaf in a wedge
  search, exercised by every classify pair and consistency query) cloned the
  parent-role candidate bucket out of `block_index` on every call, just to
  release the borrow before mutating `stats`. It now iterates the bucket in
  place (the immutable `block_index` borrow coexists with the `nodes` borrows;
  `block_compares`/`blocks_fired` accumulate in loop-locals written after the
  loop), removing a per-call heap allocation on the hot path. Behaviour-identical
  — same candidates, order, subset-pair block decision, and stats; full
  tableau+reasoner suite green, verdicts unchanged. (Profiling note: the residual
  ORE consistency cost, e.g. `ore_ont_9899` ~21 s, is a *convergence* wall — both
  the wedge and the tableau fall-through run their full deadline and return the
  correct-but-timeout-derived `consistent`, Konclude-confirmed — not a hot loop
  this addresses; see `docs/superpowers/plans/2026-07-23-wedge-isblocked-labelsig-prefilter-plan.md` §0/§8.)

## [0.3.37] — 2026-07-23

### Performance

- **ABox-saturation disjoint-clash checks indexed (ORE ABox consistency: e.g.
  `ore_ont_9899` pre-check ~27.5 s → ~0.5 s).** After the 0.3.36 chain index,
  `saturate_abox_consistency`'s Rule 8 (disjoint clash) was still
  `O(|disjoint_pairs| × |individuals|)` re-scanned every fixpoint iteration, and
  Rule 7b (functional existential-marker clash) did the same via a linear
  `disjoint_pairs` scan inside a fillers² loop — the dominant cost on ORE ABox
  ontologies that declare disjoint classes. Now a `disjoint_of` symmetric class
  adjacency + a normalized membership set are built once before the fixpoint;
  Rule 8 is type-driven (for each individual, for each of its types, check a
  told-disjoint partner — `O(Σ types × partners)`, guarded so disjoint-free
  ABoxes stay zero-cost) and Rule 7b uses O(1) membership. Verdict-preserving by
  construction (Rules 7b/8 write only the `clash` bool — no types/edges, no
  downstream consumer): 79/79 ORE ABox ontologies verdict-identical
  indexed-vs-brute (incl. all 10 inconsistent), corpus consistency unchanged.
  `RUSTDL_ABOX_DISJOINT_BRUTE=1` restores the pre-fix scans for A/B. Broadly
  applicable (any ABox ontology with disjoint classes), unlike the family-scoped
  0.3.36 chain index. The residual ORE consistency cost is now the hybrid
  tableau, not the pre-check. Plan + advisor review:
  `docs/superpowers/plans/2026-07-23-abox-saturation-disjoint-index-plan.md`.

## [0.3.36] — 2026-07-23

### Fixed

- **`realize` / `materialize_inferred_class_assertions` no longer hang on an
  inconsistent ontology.** Off the saturation-eligible fragment, realize ran a
  `{a} ⊓ ¬C` tableau probe per (individual, class); on an inconsistent KB every
  such membership holds vacuously, so this was both wrong to report as a
  meaningful realization and — on a *deep* inconsistency the ABox-saturation
  pre-check catches but classify's pattern checks do not (e.g. `family.ofn`) — a
  multi-minute-to-hours stall over an ABox that never cheaply clashes. Pre-fix it
  could even return a degenerate `Ok` (an individual typed into mutually-disjoint
  classes). `realize_internal` now returns `Err(Inconsistent)` when
  `saturate_abox_consistency` reports a clash, matching the sibling
  `materialize_{object,data}_property_assertions`. Sound under-approximation
  (clash ⇒ genuinely inconsistent), so consistent ontologies are unaffected.

### Performance

- **ABox-saturation role-chain closure: index the inner leg (family
  `is_consistent`/`realize` ~20 s → ~1.3 s, ~15×).** `saturate_abox_consistency`'s
  Rule-4 role-chain phase found "the r-edges leaving node `b`" with an O(E)
  linear rescan of the edge snapshot, nested under the outer edge loop, per chain
  rule, every fixpoint iteration — on `family.ofn` (508 individuals, ~267 k-edge
  transitive closure) that was ~21 s of the ~21.6 s total. It now looks the inner
  leg up in `(role, src)`/`(role, dst)` indexes (O(fan-out)), byte-identical to
  the old scan by construction (same snapshot, same candidate order, same
  derivation schedule). Verdict- and closure-preserving: 79/79 ABox-bearing ORE
  ontologies verdict-identical indexed-vs-brute, closure edge-set byte-identical
  on a transitivity+inverse-in-chain+symmetric fixture. `RUSTDL_ABOX_CHAIN_BRUTE=1`
  restores the pre-fix scan for A/B. The win is scoped to chain-closure-dominated
  inputs (family and its class); broad ORE consistency cost lives in the tableau,
  not this phase (separate follow-up). Plan +
  advisor review: `docs/superpowers/plans/2026-07-23-abox-saturation-chain-index-plan.md`.

## [0.3.35] — 2026-07-23

### Fixed

- **Realize / consistency / satisfiability hang on defined-class ontologies with
  a nominal-anchored `∃`-cycle ([#35], `hang_v3` core).** A `{a} ⊓ ¬C` instance
  probe over `Person ⊑ ∃hasMother.Woman` + `Woman ⊑ Person` (from `Woman ≡ …`) +
  the covering `Person ≡ Man ⊔ Woman` + a property domain, with an `ABox` edge
  `isMotherOf(a,b)`, builds an infinite maternal `∃`-cycle **anchored at a
  nominal root**. Ancestor-scoped pair-blocking cannot block that chain — the
  pairwise parent-subset condition never holds near the nominal anchor — so the
  completion graph grows near-unbounded and each probe is pathologically slow
  (~70 s for `realize` on the 6-axiom core; `realize` /
  `materialize_inferred_class_assertions` run one probe per (individual, class),
  aggregating that into a multi-minute-to-hours stall on the source KG). The
  0.3.34 deep-cap fix only let each probe eventually finish, slowly; this is the
  underlying blocking condition behind the successive #35 cores. Fixed by
  enabling **anywhere-blocking** (Motik/Shearer/Horrocks) on the deadline-free
  query paths (`is_consistent` / `is_class_satisfiable` / un-timed
  `realize` / `instance`), which blocks the chain against any earlier
  non-nominal node and bounds the graph. `hang_v3` `realize`: ~70 s → 0.04 s,
  verdicts matching HermiT (`a`, `b` are just `owl:Thing`; consistent).
  Deadline-bounded paths (classification pairs, timed realize probes) keep
  ancestor-blocking — a 152-ontology ORE + curated-corpus bake-off confirmed
  anywhere-blocking is verdict-identical there (152/152 + corpus byte-identical,
  0 panics, no reproducible wall regression), so the classify hot loop is left
  untouched. Env override: `RUSTDL_ANYWHERE_BLOCKING=1` forces it on everywhere
  (incl. classify), `=0` forces the pre-fix ancestor-only behaviour.

[#35]: https://github.com/MaastrichtU-IDS/rustdl/issues/35

## [0.3.34] — 2026-07-22

### Fixed

- **Realization / consistency / satisfiability hang on disjunction-dense
  ontologies with defined classes ([#35]).** `is_consistent`,
  `is_class_satisfiable`, and un-timed `realize` (the query paths with no
  deadline) could hang indefinitely (>300 s) on a small ontology whose
  completion graph is bounded by pair-blocking (~200–330 nodes) but whose
  `EquivalentClasses` reverse-directions plus a class-union absorb into several
  open `⊔`s per node. A clash-free model there needs hundreds of sequential
  `⊔`/`choose` decisions — more than the old `MAX_SEARCH_DEPTH = 256` recursion
  cap. A depth cutoff returns `DepthLimit`, which carries no clash dependencies,
  so dependency-directed back-jumping could not prune and the driver enumerated
  the exponential `⊔`-space without terminating. Termination on these paths now
  rests on pair-blocking (a finite graph ⇒ a finite decision count): the search
  runs with a deep cap on a dedicated large-stack thread, so blocking does the
  bounding and the recursion stays stack-safe. Deadline-bounded paths
  (classification pairs, timed realize probes) are unchanged — they check the
  deadline at every recursive entry, cannot hang, and a cap hit is a sound MISS —
  so the classify hot loop and its benchmark walls are untouched. This is the
  tableau counterpart to the 0.3.31 realize saturation fast-path: that one keeps
  realize off the tableau on the EL/Horn fragment; this one fixes the tableau
  itself for the off-fragment case.

[#35]: https://github.com/MaastrichtU-IDS/rustdl/issues/35

## [0.3.33] — 2026-07-22

### Fixed

- **`--global-timeout-ms` now actually bounds the reasoning wall** (it didn't on
  large out-of-EL ontologies in 0.3.32). Root cause (profiler-identified): `decide`
  cloned the entire `ConceptPool` (~200k concepts) plus set up the tableau context
  *before* checking the deadline, so under a global budget the tens of thousands of
  post-deadline probes each paid a full pool clone even though the search would
  instant-timeout. Now `decide` fast-exits when the deadline is already spent,
  before the clone. Also bounded the post-deadline `timed_out_pair_ids`
  materialization (was O(n²): ore_ont_3215 built a 3.3-billion-tuple vector) — the
  sweep/tier-walk record one marker per class and break; the entailment-matrix BFS
  uses a generation-stamped visited buffer. **ore_ont_3215 (54,973 classes) at
  `--global-timeout-ms 30000`: 317 s / 26 GB → 42 s / 3 GB**; several other
  disjunctive giants recover from timeout. **Verdict-preserving: 1630/1630 ORE
  ontologies byte-identical before-vs-after on default (no-deadline) runs** (the
  fast-exit only fires when the deadline is already expired). See
  `docs/2026-07-22-global-timeout-fastbail.md`.

### Note

- The 0.3.32 description of `--global-timeout-ms` as bounding "total time" was
  overstated and is corrected: it bounds the *reasoning/probing* wall, not the
  fixed saturation + preprocessing overhead (which is not deadline-gated).

## [0.3.32] — 2026-07-22

### Added

- **`classify --global-timeout-ms N`** (CLI). A total wall-clock budget for the
  whole classification (`0` = unbounded, the default), complementing the existing
  `--pair-timeout-ms`. Each probe is cut at the smaller of the per-pair budget and
  the time left on the global deadline; pairs still undecided at the deadline
  default to "not subsumed" — a sound under-approximation (FP=0, real subsumptions
  may be missed). Wires the reasoner's existing `classify_with_budget` entry point
  to the CLI; the `INCOMPLETE` warning now names whichever bound was hit.

### Changed

- **Python `classify` / `classify_bytes`: timeout kwarg renamed
  `global_deadline_ms` → `global_timeout_ms`** for parity with the CLI flag and
  with `per_pair_timeout_ms`. `global_deadline_ms` is kept as an accepted
  **deprecated alias** (emits `DeprecationWarning`), so existing code keeps working
  — no breaking change.

## [0.3.31] — 2026-07-21

### Fixed

- **Realize saturation fast path — ends the issue #35 realization hang.**
  `realize` / `is_instance_of` / `instances_of` (and the Python
  `materialize_inferred_class_assertions`) ran the full SROIQ tableau
  (`{a} ⊓ ¬C` probe) for every (individual, class) pair. On an EL/Horn ontology
  with a defined class (`≡` + `∃`) + property domain + a property assertion, the
  two non-absorbable GCIs from the `≡` definitions let every node speculatively
  pick the defined class → generate a `∃` successor → recurse; the ⊔-search
  burned 100k+ branches over a blocked ~134-node graph and never terminated
  (> 300 s hang), while `classify` on the same file was instant (saturation fast
  path). realize had no such gate. New `owl_dl_saturation::saturate_for_realize`
  materializes each named individual as a nominal class `N_a` and seeds
  `N_a ⊑ C` (ClassAssertion), `N_a ⊑ ∃r.N_b` (ground edge ⇒ domain +
  existential-LHS firing) and `N_b ⊑ Rng` (ground range); entailed types are
  `subsumers_of(N_a) ∩ named classes`. `realize_saturation_eligible` gates the
  three individual queries onto this path (TBox in the saturator-complete
  fragment **and** every ABox axiom a shape the seeding captures — atomic/⊓
  `ClassAssertion`, non-inverse `ObjectPropertyAssertion`; `SameIndividual` /
  inverse fall back). **Complete == the tableau on the fragment** (incl. the
  conjunctive-LHS `x:D1, x:D2, D1 ⊓ D2 ⊑ E ⊨ x:E` case), **sound by
  construction**, terminating (no tableau). Off-fragment keeps the identical
  tableau path plus an opt-in per-pair deadline `RUSTDL_REALIZE_PAIR_TIMEOUT_MS`
  (default unset ⇒ no bound; restores the caller-side bound removed in 0.3.18).
  `RUSTDL_REALIZE_SATURATION=0` reverts. FP=0 preserved; `classify` untouched.
  See `docs/2026-07-21-realize-saturation-fast-path.md`.

## [0.3.30] — 2026-07-21

### Changed

- **Sparse `Classification.entailed` matrix** — closes the giant-ontology memory
  + hierarchy-print wall (the D4 residual). The class-subsumption result was a
  dense `Vec<FixedBitSet>` n×n matrix allocated up front (n²/8 bytes regardless of
  content): on `ore_ont_868` (981,151 classes) that is 112 GB, and the O(n²)
  hierarchy print (`equivalent_classes`/`direct_subsumers` scanning `0..n` per
  class) did not finish in 20 minutes. New adaptive `EntailmentMatrix`:
  `Dense(Vec<FixedBitSet>)` for ≤ 60k classes (every curated fixture — byte-
  identical to the old path) / `Sparse(Vec<Vec<u32>>)` above (ascending-sorted
  subsumer rows, unsatisfiable rows elided). A single `entails(i,j)` choke-point
  reintroduces `⊥ ⊑ *` for unsatisfiable subjects; the accessors iterate the
  sparse row O(k) instead of scanning `0..n`. **`ore_ont_868` classify:
  TIMEOUT (> 20 min) / 116 GB peak → 69 s / 3.3 GB, full 981,153-line hierarchy.**
  Verdict-preserving: dense-vs-sparse byte-identical on galen/sio; corpus
  FP=0/MISSED=0 unchanged; galen wall unchanged (stays on the dense path). See
  `docs/2026-07-21-sparse-classification-results.md`.

## [0.3.23] — 2026-07-18

### Added

- **`render_manchester(path) -> list[str]`** (Python binding). Renders every
  logical axiom of the ontology at `path` as a Manchester syntax string,
  reusing the same `AsManchester` renderer `justify`/`justify_all`/`repair`
  already use — declarations, imports, and ontology-level annotations are
  filtered out as non-logical noise. Lets downstream consumers (e.g. a
  reasoner service that only has whelk/rdflib axioms) render an ontology to
  Manchester without going through a query/justification call.

## [0.3.21] — 2026-07-02

### Added

- **Opt-in defined-sup VERIFY sweep** (`RUSTDL_CLASSIFY_DEFINED_SWEEP`, **default
  OFF**). For a class `D` defined via a non-EL body (`¬`/`⊔`/`∀`), the wedge's
  label countermodel is an unreliable counterexample, so the label heuristic can
  prune a true `cand ⊑ D`. When enabled, the defined-sup sweep bypasses the label
  prune for defined sups and verifies each candidate with the full tableau. Sound
  (FP=0 by construction — only tableau-confirmed edges). Closes complement/
  disjunction-defined subsumptions the closure-guided walk can't see
  (ORE `ore_ont_15167` 42→34 MISSED). Default OFF: corpus-invisible and ~2× wall.
  See `docs/ore-sweep-2026-07-01.md`.

## [0.3.20] — 2026-07-01

### Added

- **`entails(ObjectPropertyAssertion)` / `justify property` now confirm RBox-derived
  edges** ([#28]). The oracle unions the RBox-complete `materialize` edge set
  (transitivity / symmetry / inverse / `SameIndividual` / `ObjectHasValue`) with the
  existing NegOPA inconsistency probe — sound by composition (the materialize set was
  validated FP-free vs HermiT in #26), strictly more complete.
- **`materialize_data_property_assertions` now folds `SameIndividual`** (`a≡b`,
  `dp(a,v)` ⟹ `dp(b,v)`), guarded by a new HermiT/ROBOT data-property-assertion
  oracle (`materialize_data_matches_hermit_oracle`).

### Fixed

- **EL saturator: `ObjectHasSelf` self-loop now propagates super-role range.**
  `X ⊑ ∃R.Self`, `R ⊑* S`, `range(S)=C` ⟹ `X ⊑ C` (the rule previously read only the
  direct `range(R)`). Sound — the self-loop successor coincides with `x`, so the range
  obligation lands on `x` itself. Closes an ORE-2015 completeness gap (`ore_ont_4827`
  MISSED 79→0); FP=0 / MISSED=0 corpus-wide.

## [0.3.19] — 2026-06-29

### Fixed

- **`materialize_inferred_property_assertions` now materializes symmetric,
  `SameIndividual`, and `ObjectHasValue` entailments** ([#26] audit follow-up to
  the v0.3.18 transitive-closure fix). The ABox edge-saturator backing the
  materializer was a consistency-pre-check clash-finder that under-computed these
  ground entailments while the docstring over-claimed them:
  - **Symmetric** (`SymmetricObjectProperty(R)`, `a knows b` ⟹ `b knows a`) —
    was claimed but never implemented, same as transitivity had been.
  - **`SameIndividual`** (`a≡a'` ⟹ edges/types fold across the class).
  - **`ObjectHasValue`** (`a : ∃R.{b}`, asserted or via `C ⊑ ∃R.{b}` + `a:C`,
    ⟹ ground edge `R(a,b)`).

  All sound (corpus closure-diff byte-identical, FP=0/MISSED=0) and they also
  close matching gaps in the ABox consistency pre-check. Adds the materialize
  regression coverage the issue noted was missing.

## [0.3.18] — 2026-06-29

### Fixed

- **`materialize_inferred_property_assertions` now includes the transitive (and
  chained) closure of object-property assertions** ([#26]). For
  `TransitiveObjectProperty(R)` it previously returned only one-step edges
  (`a→b`, `b→c`) and omitted the composed `a→c`. Root cause: the ABox
  edge-saturation backing the materializer handled sub-property / inverse /
  symmetric / declared role-chains but had no transitivity rule. Transitivity is
  now registered as the self-chain `R ∘ R ⊑ R` and closed in the same fixpoint,
  so it composes with the hierarchy/inverse/chain rewrites. Sound (every edge is
  entailed; corpus closure-diff byte-identical, FP=0/MISSED=0) — also closes a
  completeness gap in the ABox consistency pre-check.

[#26]: https://github.com/MaastrichtU-IDS/rustdl/issues/26
[#28]: https://github.com/MaastrichtU-IDS/rustdl/issues/28

## [0.3.17] — 2026-06-28

### Changed

- **`classify` is now bounded by default — it can't hang on wine-class
  ontologies.** The Python default ran each pair at 1000 ms with no global
  bound, so total time grew with the pair count and could hang on combinatorial
  nominal+cardinality+disjunction inputs. New defaults:
  - **`per_pair_timeout_ms` 1000 → 100.** Measured completeness floor: at 100 ms
    the full Konclude-oracle corpus is FP=0/MISSED=0 byte-identical (real corpus
    28 s → 12.8 s, ~2.2×). 25 ms was rejected — it flakily drops 4–8 real
    subsumptions on the standard pizza fixture.
  - **new `global_deadline_ms` = 60000** — bounds the *total* wall (the backstop
    the per-pair cap can't provide). Each probe is cut at
    `min(per_pair, remaining global)`.

  Set either bound to `0` to disable it; both `0` = unbounded/complete. Sound at
  every setting — cut pairs default to "not subsumed" (FP=0); the
  `IncompleteClassificationWarning` and `Classification.complete` /
  `.timed_out_pairs` report any incompleteness.

### Added

- `owl_dl_reasoner::classify_with_budget(ontology, per_pair, global)` — public
  combined per-pair + global-deadline classify entry point.
- `owl-dl-bench corpus --pair-timeout-ms <N>` — per-pair cap for the corpus
  harness (it previously ran unbounded and hung on wine-class inputs);
  `bench-corpus/` fixtures added.

## [0.3.16] — 2026-06-27

### Internal

- **Saturation-only closure-diff mode** (`ORE_ONE_SAT_ONLY=1`) in the
  `konclude_closure_diff` test harness: computes rustdl's closure from the
  saturation-only path (a global fixpoint, no per-class wedge) and diffs it
  against the Konclude oracle. Diagnostic for whether the sound-but-under-
  approximate saturator is already complete on ontologies whose per-class wedge
  classify is too slow to finish. Test-only and env-gated; no engine change.

## [0.3.15] — 2026-06-27

### Performance

- **Restrict the MRV disjunction scan to anchored clauses.**
  `find_open_disjunction` (the wedge's most-constrained-variable ⊔-scan) used to
  examine every `(node × non-Horn clause)` pair per call — up to ~35k pairs/call
  on large ORE ontologies. A disjunctive clause can only be an *open* ⊔ at a node
  when its X-class atoms are in that node's label, so the scan now iterates a
  precomputed ascending `(clause, first-X-class anchor)` list and skips an
  anchored clause via an O(1) `has(anchor)` check (the residual role-only/empty
  bodies are always scanned). Soundness is termination-only here — MRV's choice
  is verdict-invariant and no open ⊔ is missed; ascending order keeps the
  tie-break identical (closures byte-identical, FP=0/MISSED=0 corpus-wide).
  **sio −12%, ore-15672 −10%, wine@1ms −18%, ore-10908 −5%**; flat on EL.

## [0.3.14] — 2026-06-27

### Performance

- **Precompute per-clause match plan (matcher hot loop).** `match_body` — the
  wedge's #1 hotspot by profiling — used to re-partition the clause body
  (role/class/X-class atoms) and re-run the variable-tree `eval_order` on every
  `(clause, node)` call, even though both depend only on the immutable clause
  body. Each clause's `(role_atoms, other_classes, X-classes, eval_order)` (or
  its "unsupported" verdict) is now computed once at index-build and stored in
  the Arc-shared `ClauseIndexes`. Sound by construction (memoizes immutable
  clause-derived data — matches and verdicts byte-identical). **ore-10908
  −8…−11%** (the matcher-heaviest classification), sio −2%; flat on EL and
  tier-walk-bound workloads. The companion to the v0.3.12 `SmallVec`/membership
  matcher wins (which cut allocation; this cuts the recompute).

## [0.3.13] — 2026-06-27

All changes below are **sound** — FP=0/MISSED=0 byte-identical against the
Konclude∩HermiT oracle across the full corpus (wine, galen, notgalen, sio,
ore-10908, ore-15672, pizza, alehif, ro, sulo, bibtex).

### Performance

- **Coupled-saturation precompletion seed (`RUSTDL_SAT_SEED`, default-ON).** The
  classify path now seeds the per-class wedge search with the saturator's named
  subsumers (SP2) and derived existential facts (SP3) — Konclude-style coupled
  saturation. Collapses wine's hard nominal/disjunctive model builds:
  **wine classify 49s → 3.2s (~15×)**, sound. Pairs with MRV ⊔-ordering and the
  adaptive label-cache deadline.
- **Value-derived type-disjointness + tautology-skip (default-ON).** Types forced
  to distinct nominal values on a functional role are treated as disjoint, and
  `a ⊔ ¬a` complement disjunctions are skipped — together they make wine's
  descriptor value-partition fragment tractable (the 8 previously-hardest classes
  now resolve in milliseconds).
- **Label-cache deadline floor 1000ms → 50ms.** The per-class label-build budget
  is `n × per_pair` (the refute-the-row break-even); the old 1s floor sat above
  that and over-invested at tight per-pair caps. Lowering it gives **wine
  −39% at `--pair-timeout-ms 1`** (1.54s → 0.94s). Inactive at the default cap.

### Completeness

Two sound completeness levers (default-ON; pure gains — close real subsumptions,
never introduce false ones):

- **Nominal-filler typing (`RUSTDL_NOMINAL_TYPING`).** `ClassAssertion(C, a)` now
  types the nominal filler so `∃R.{a} ⊑ ∃R.C` is derived (object value-membership,
  the analog of the data D6 lever). DMOP MISSED 31→0.
- **ObjectOneOf common-subsumer (`RUSTDL_ONEOF_SUBSUMER`).** For an enumerated
  class `X ≡ ObjectOneOf(a₁…aₙ)`, `X ⊑ C` is seeded when every member `aᵢ` is
  typed `C` (LHS ⊔-elimination). ORE `ore_ont_5107` MISSED 6→0.

## [0.3.12] — 2026-06-22

### Performance

Three FP-safe constant-factor wins in the hypertableau **wedge** matcher hot loop
(found by profiling; each sound and complete-preserving — closures **byte-identical
corpus-wide**, verified across the whole arc):

- **Matcher allocation reduction (`SmallVec`).** The per-call / per-recursion scratch
  in the wedge's clause matcher (`hyper.rs`) — `match_body` / `enumerate_matches`
  buffers (`role_atoms`, `other_classes`, `targets`), the `Binding` type itself (kills
  the per-match `clone`), and `eval_order`'s scratch — now stays inline instead of
  heap-allocating. Allocator self-time on the wedge hot path dropped from ~35% to ~1%.
- **Linear-scan label membership.** `HyperNode::has` was a binary search over a sorted
  label slice; profiling showed nodes carry only ~5 labels over a universe of hundreds
  to thousands, so the binary search was mostly branch-misprediction. Replaced with a
  branch-predictable linear scan + early-exit (labels stay sorted, same result).

Cumulative effect (vs the pre-arc baseline): **~10–19% faster on out-of-EL SROIQ
classification** (ore-10908 −18.6%, sio −13.9%, ore-15516 −13.2%) and **~2× on the
matcher-bound `family` inconsistency case** (125s → 63s, verified). Flat on EL
ontologies (they route to the saturator, untouched). A `FixedBitSet` label
representation was scoped and rejected by a profiling study (a dense per-node bitset
would bloat the wedge's frequent node-clones 2–65× on sparse-wide nodes — net-negative;
see `docs/wedge-label-bitset-p0-results.md`).

## [0.3.11] — 2026-06-22

### Added

- **Manchester syntax (`.omn`) input.** rustdl now *reads* OWL Manchester syntax,
  not just writes it — `classify`/`debug`/`diagnose`/`justify`/`repair`/`report`
  accept `.omn` files (CLI auto-detects by content sniff + extension; Python
  auto-detects by extension, or `classify_bytes(data, format="omn")`). Front-end
  only — no engine change, FP=0 structurally untouched. rustdl already rendered
  explanations in Manchester; input completes the symmetry. The reader is the
  conformance-tested `horned_owl::io::omn::reader` from the pinned fork rev (no
  dependency on the upstream PR merging).
- **Python QA tutorial** (`docs/python-ontology-qa.md`) — an end-to-end "diagnose
  and fix a broken ontology" walkthrough (classify → `debug()` → justify/repair →
  fix → read inferred facts), fully Manchester-faced and CI-guarded against rot.
  Linked from the main and PyPI READMEs.

[0.3.12]: https://github.com/MaastrichtU-IDS/rustdl/releases/tag/v0.3.12
[0.3.11]: https://github.com/MaastrichtU-IDS/rustdl/releases/tag/v0.3.11

## [0.3.10] — 2026-06-21

> Note: the changelog was not maintained for 0.3.2–0.3.9; this entry covers the
> user-facing additions landing in 0.3.10. See `CLAUDE.md` and
> `docs/superpowers/specs/` for the full engineering record of intermediate work.

### Added

- **Explanation & debugging suite** — rustdl is now an ontology-*debugging* tool, a
  niche the fast reasoners (Konclude has no built-in justification/explanation) don't
  serve. All sound by construction; read-only (FP=0 untouched).
  - `rustdl justify --laconic` — weakens each justification axiom to its responsible
    *fragment* (Horridge-style laconic justifications, sound structural weakening).
  - `rustdl diagnose` — partitions unsatisfiable classes into **root** causes vs
    **derived** collateral, and justifies the roots ("where to start fixing");
    detects global inconsistency too.
  - `rustdl repair` — minimal axiom-removal sets to break an unwanted entailment
    (Reiter diagnoses = minimal hitting sets over all justifications), each **verified**.
  - `rustdl report` — a self-contained HTML debugging report (summary + diagnose
    roots/derived + per-root justification + repairs), no external resources.
- **Inference materialization** (Python + reasoner API):
  - `materialize_inferred_property_assertions` — inferred **object** property
    assertions over named individuals (hierarchy / inverse / symmetric / role chains /
    transitivity); also CLI `rustdl realize --properties`.
  - `materialize_inferred_data_property_assertions` — inferred **data** property
    assertions (5-tuple incl. datatype + language tag).
  - `materialize_inferred_subobjectproperty_axioms` /
    `materialize_inferred_subdataproperty_axioms` — the inferred property-hierarchy
    closure (object: told + equivalent + inverse; data: told + equivalent).
  - `materialize_existential_successors` — a blank-node representation of named
    individuals' entailed existential successors (one row per entailed `a : ∃R.C`;
    *not* entailed ground triples — witnesses are model-relative).
- `rustdl.debug()` now returns a typed `Diagnosis` result object (attribute access +
  dict-compatible `Mapping`; `to_dict()` for JSON).

### Fixed

- **family inconsistency** is now detected (a consequence-based ABox-saturation
  pre-check) — the last open correctness gap.

[0.3.10]: https://github.com/MaastrichtU-IDS/rustdl/releases/tag/v0.3.10

## [0.3.1] — 2026-06-05

### Added

- **Bundled example ontologies** for a zero-setup, offline demo:
  `rustdl.examples.pizza()`, `.sulo()`, `.sio()` return local paths to
  ontologies shipped *inside the wheel* (gzip-compressed, decompressed
  into a per-user cache dir on first use — no network, works behind a
  proxy / air-gapped). Companion `PIZZA_NS` / `SULO_NS` / `SIO_NS`
  namespace constants. `pizza()` is the SULO-aligned ontostart pizza
  (88 classes, classifies instantly and completely).

[0.3.1]: https://github.com/MaastrichtU-IDS/rustdl/releases/tag/v0.3.1

## [0.3.0] — 2026-06-05

### Changed

- **Classification now bounds each subsumption test by default**
  (`pair_timeout_ms` / `per_pair_timeout_ms` defaults to **1000 ms**;
  previously unbounded). Pathological SROIQ ontologies (e.g. pizza) no
  longer hang by default. A timed-out pair is recorded as "not
  subsumed" — **sound** (never a false subsumption), but the result may
  be **incomplete**. Pass `0` for the complete, unbounded classification.
  1000 ms is the empirical knee on pizza: higher budgets buy no extra
  completeness (the remaining pairs are intractable at any reasonable
  bound) but cost proportionally more wall time.
- **Incompleteness is now surfaced loudly**, so a bounded run can't
  silently hand back a hierarchy missing real edges:
  - CLI `rustdl classify` prints a prominent `⚠ INCOMPLETE: N pair(s)
    exceeded the timeout …` warning to stderr when any pair times out.
  - Python `rustdl.classify` / `classify_bytes` emit an
    `IncompleteClassificationWarning` (filterable via the `warnings`
    module), and the `Classification` object exposes `.complete` (bool)
    and `.timed_out_pairs` (int).

### Added

- **CLI multi-format input.** `rustdl <cmd> file.{owx,owl,rdf}` now
  works — the reader is chosen by file extension (.owx → OWL/XML,
  .owl/.rdf → RDF/XML, .ofn/other → OWL Functional). Previously the CLI
  read OFN only; the Python bindings already auto-detected. Verified on
  the owlcs pizza ontology (RDF/XML) — 99 classes, 2 unsatisfiable
  (CheeseyVegetableTopping, IceCream), matching the canonical result.

## [0.2.2] — 2026-06-05

### Changed

- **PyO3 0.22 → 0.25.** Clears 35 build warnings: 28 from PyO3 0.22's
  macro-generated code tripping the `unsafe_op_in_unsafe_fn` lint
  (default-warn under Rust edition 2024) and 5 `gil-refs` cfg
  warnings — both fixed by PyO3's edition-2024-clean codegen. The only
  source change required was `get_type_bound::<T>()` →
  `get_type::<T>()`. `cargo build -p owl-dl-py` is now warning-free.
- Silenced 7 `dead_code` warnings on `ClashReason` (fields read only
  via the derived `Debug` impl for `RUSTDL_TRACE` output).

### CI

- **Linux aarch64 wheels now build on a native ARM runner**
  (`ubuntu-24.04-arm`) instead of QEMU emulation on an x86 host. Build
  time drops from ~50 min to ~4 min. QEMU setup step removed.

## [0.2.1] — 2026-06-05

### Changed

- **Python package documentation.** The `rustdl` PyPI page now ships a
  complete README: install, quick-start, full API reference (classify +
  Classification members, one-shot queries, inference materialization,
  exception hierarchy), and the soundness/coverage contract. The 0.2.0
  wheel shipped a four-line placeholder README; this release replaces it.
- Re-added the Linux aarch64 wheel to the release matrix (dropped in 0.2.0
  to speed up release-workflow iteration).

## [0.2.0] — 2026-06-04

### Added

- **Python bindings** (`rustdl` on PyPI). PyO3 + maturin. ABI3 wheel
  for Python 3.10/3.11/3.12/3.13. Top-level API one-to-one with the
  Rust public API (`classify`, `classify_bytes`, `is_consistent`,
  `is_class_satisfiable`, `is_subclass_of`, `is_instance_of`,
  `instances_of`, `realize`) plus inference materialization helpers
  (`materialize_inferred_subclass_axioms`,
  `materialize_inferred_class_assertions`). Auto-detects OFN/OWX/RDF-XML
  format from file extension. 5-platform wheel matrix (Linux x86_64 +
  aarch64, macOS x86_64 + arm64, Windows AMD64) + sdist. PyPI publish
  via trusted publisher (OIDC, no token in CI).
- New GitHub Actions workflows: `python-ci.yml` (PR/dispatch gate) and
  `release-python.yml` (cibuildwheel + maturin publish on `v*.*.*` tag).

### Deferred to roadmap

- owlready2 / omny integration (separate brainstorm queued).
- Black-box `rustdl.explain(path, sub, sup)` axiom-justifications.
- `rustdl.Reasoner(path)` stateful class for batch queries.
- Native pyhornedowl `Ontology` pass-through.
- See the spec at `docs/superpowers/specs/2026-06-04-python-bindings-design.md`
  for the full deferred-feature list.

## [0.1.0] — 2026-06-04

First tagged release. The engine is sound on every measured workload
and competitive (or winning) against HermiT and Konclude on most.

### Added

- Sound OWL 2 DL (SROIQ) classifier with hybrid saturation+tableau
  orchestrator.
- Hypertableau wedge accelerator (default engine since 2026-05-29).
- Per-class label heuristic (Phase 7) — sound non-subsumption pruner
  via per-class wedge satisfiability.
- Cache-deadline decoupling (Phase 8) — independent deadline for the
  label-cache build, so SROIQ classes needing a few hundred ms of
  wedge satisfiability no longer get cut off at NoVerdict.
- Horn-shortcircuit fast path (Phase 2b) — Horn-fragment ontologies
  dispatch straight to saturation, skipping the per-pair tableau loop.
- ABox consistency check (Phase A1) — seven sound clash patterns:
  direct-Bot assertion, disjoint types per individual, NegOPA vs OPA
  with role-hierarchy propagation, SameAs ∩ DifferentFrom (transitive
  via union-find), Functional + two distinct witnesses,
  Asymmetric / Irreflexive violations, domain/range disjointness.
- Datatype preprocessing (D1–D5) — sound under-approximation for data
  axioms not directly supported; recognized patterns derived as TBox
  axioms (Functional + DataMin, DataMin > DataMax, DataPropertyDomain
  inference, SubDataPropertyOf transitivity,
  intersection-equivalence propagation, integer-range facet
  intersection).
- 9-corpus closure-diff regression harness — FP=0 invariant gated
  against Konclude on every commit.

### Performance

Compared with the May 2026 baseline:

- **GALEN**: 445 s → **0.49 s** (now beats Konclude — 0.24× ratio).
- **notgalen**: 1168 s → **0.78 s** (now beats Konclude — 0.35× ratio).
- **alehif**: 2.28 s → **0.16 s** (0.08× Konclude).
- **ORE-10908**: 17× Konclude → **3.1×** (under the ≤5× target).
- **sio-stripped**: 4.3× absolute wall improvement (still 13.6×
  Konclude — out-of-EL fragment, timeout-bound; see dead-end §18).

### Known limitations

- Data-axiom patterns outside the D4/D5 recognizers are silently
  dropped (sound under-approximation; missed positives possible).
- `HasKey` not supported (errors at parse time).
- SWRL rules silently skipped.
- Role chains of length > 2 error at parse time.
- family-class workloads need ABox saturation (open scoping target
  per dead-end §21).
- ore-15672 has a 3-class intrinsic intractability cluster — sub-model
  caching is the only known path (multi-month research-engineering;
  dead-end §18).

### Dead-ends documented

21 entries in [`docs/hypertableau-dead-ends.md`](docs/hypertableau-dead-ends.md)
covering soundness traps, perf optimizations that didn't materialize,
and design decisions that recon ruled out before implementation. The
ledger is the canonical record of "we tried X; here's what killed it."

### Soundness contract

FP=0 vs Konclude verified on every release. The closure-diff tests in
[`crates/owl-dl-reasoner/tests/konclude_closure_diff.rs`](crates/owl-dl-reasoner/tests/konclude_closure_diff.rs)
are the soundness tripwire — any change that introduces a false-positive
subsumption fails CI.

[0.3.0]: https://github.com/MaastrichtU-IDS/rustdl/releases/tag/v0.3.0
[0.2.2]: https://github.com/MaastrichtU-IDS/rustdl/releases/tag/v0.2.2
[0.2.1]: https://github.com/MaastrichtU-IDS/rustdl/releases/tag/v0.2.1
[0.2.0]: https://github.com/MaastrichtU-IDS/rustdl/releases/tag/v0.2.0
[0.1.0]: https://github.com/MaastrichtU-IDS/rustdl/releases/tag/v0.1.0
