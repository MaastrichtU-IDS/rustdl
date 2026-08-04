# Query-relevance / ⊥-locality module restriction of the fired rule set — scoping

**Date:** 2026-08-04
**Status:** REPORT ONLY. No code changed. Verdict **VIABLE-WEAK**, and *inert on
`ore_ont_10019`'s actual cost centre* — which is the single most decision-relevant finding here.
**Companion docs:** `docs/2026-08-04-ore-10019-rootcause.md`,
`docs/2026-08-04-absorption-on-10019-is-fully-blocked.md`.

**Binary used for every executed measurement:** `target/release/rustdl`, sha256
`c513d3ec16b92418c62b1f85fcdc4b4d4cd032c1c2c0c423acc13b3613080f94`. **This is NOT the
root-cause doc's pinned `2c7d1bbf…`.** Every comparison below is arm-vs-arm on *this one*
binary, so the comparisons are controlled; but absolute walls should not be cross-quoted
against that doc.

---

## 0. Headline

| question | answer |
|---|---|
| Is there real ⊥-locality machinery in-tree? | **Yes — in `justify.rs`, at the horned-owl layer.** `locality.rs` is a *different, weaker* algorithm and is dead in reasoning. |
| FP-safe? | **Yes, structurally.** The hazard is not FP, it is `incomplete: false` on a MISS (the D10 class) — and justify's runtime fallback net is *not* available to classify. |
| Exact (completeness-preserving)? | **Yes if the fixpoint is faithful.** `is_bot_local` errs by keeping too much, which is the safe direction. One open proof obligation (post-conversion signature). |
| Can it be applied per-probe without rebuilding `PreparedOntology`? | **Yes.** `decide` already takes `tbox: &AbsorbedTBox` per probe and already clones the pool per probe. |
| Does it help `ore_ont_10019`? | **Partly, and not where it matters.** 19 of 36 hanging per-class probes rescued; **0 of the 17 hard classes**; the one genuinely blocked subsumption still DNFs at 60 s on its own module. |
| Are most rules relevant to most probes? | **Yes for the probes that matter.** 60.6% of the 2162 ordered pairs retain **all 10** `CarbonAtom` disjunctive rules, and DNF concentrates in exactly that bucket (80% vs 56.7%). |

---

## 1. What locality machinery already exists

### 1a. `crates/owl-dl-core/src/locality.rs` — NOT ⊥-locality, and dead in reasoning

`LocalityPartition` (`locality.rs:36`) computes the **connected components of the class
co-occurrence graph** over TBox axioms via union-find (`locality.rs:48-67`): two classes are
unioned whenever any one axiom mentions both. Its only query is
`definitely_disjoint(a, b)` (`locality.rs:84`) = "different component ⇒ neither subsumes the
other". The file's own header calls it "Phase 1 scaffolding … integration with the classify
orchestrator lands in Phase 2" and cites `docs/module-extraction-plan.md`.

This is **strictly weaker than ⊥-locality** and is not the same construction: it is a
non-subsumption *filter*, not a module extraction. It cannot restrict a rule set — it can only
answer "provably unrelated".

**It is dead as a reasoning lever, verified by grep, not inferred.** `definitely_disjoint` and
`num_components` have **zero** non-test callers. The only live paths out of the module are:

- `owl_dl_reasoner::locality_stats` (`crates/owl-dl-reasoner/src/lib.rs:4812`), a pure
  diagnostic, printed by the CLI's `locality-stats` subcommand
  (`crates/owl-dl-cli/src/main.rs:2088`, declared at `main.rs:297`);
- `UnionFind` (`locality.rs:187`, `pub(crate)`), reused by the bounded DKey-disjointness
  role-component seeding at `crates/owl-dl-core/src/convert.rs:3043` — an unrelated consumer of
  the data structure only.

**And it is measurably useless on the target.** Executed:

```
$ rustdl locality-stats .../ore_ont_10019.owl/canon.owx
# classes:    47
# components: 1
# largest:    47
# singletons: 0
# dominance:  100.0%
```

One component. The partition excludes **zero** pairs on `ore_ont_10019`. (This is also the
cheapest possible confirmation that the ontology is a single tight blob — see §6.)

`docs/model-caching-plan.md` / `docs/moms-plan.md` describe the *satisfying-model* cache stub
(the `model_cache` field at `lib.rs:6012`, carrying `#[allow(dead_code)]`), which is a different
un-integrated Phase-1 stub. `docs/module-extraction-plan.md` is the doc `locality.rs` itself
cites. All three exist; none is wired.

### 1b. `crates/owl-dl-reasoner/src/justify.rs` — a genuine syntactic ⊥-locality test

This is the real thing:

| function | file:line | what it computes |
|---|---|---|
| `ce_is_bot` | `justify.rs:688` | is a class expression provably ⊥ when every name ∉ `sig` is read as ⊥ |
| `ce_is_top` | `justify.rs:717` | dual |
| `sub_ope_is_bot` | `justify.rs:741` | is a property-inclusion sub-side the empty role (chain: any component external) |
| `is_bot_local` | `justify.rs:759` | is an axiom ⊥-local w.r.t. `sig` (tautologous under the ⊥-substitution) |
| `collect_component_entities` | `justify.rs:847` | signature growth |
| `extract_bot_module` | `justify.rs:1053` | the fixpoint: keep every non-local axiom, grow `sig`, repeat |

It is a faithful **syntactic ⊥-locality (⊥-mod)** test in the Cuenca Grau / Horrocks / Sattler /
Kazakov sense, with the standard conservative bias, documented in its own doc comment
(`justify.rs:750-755`): *"returns `true` only when provably local; every unhandled construct
returns `false` (non-local ⇒ kept). This under-approximates locality, so `extract_bot_module`
yields a **superset** of the true ⊥-module and never drops a justification axiom."*

Constructs it does **not** classify (and therefore always keeps): `ReflexiveObjectProperty`,
`HasKey`, `DisjointUnion`, all ABox assertions, datatype definitions (`justify.rs:788-790`).
All of `ore_ont_10019`'s shapes *are* covered exactly.

So: **⊥-mod is entailment-preserving for the seed signature but not minimal.** "Exact" below
always means *preserves the entailments over the seed signature*, never *smallest such module*.

**It operates on `horned_owl::model::Component<A>`** — i.e. **pre-`convert_ontology`**, on the
raw parsed axioms. Its only consumer is `localized_candidates` (`justify.rs:1090`) on the
justify/repair path, gated by `RUSTDL_JUSTIFY_NO_MODULE` (`justify.rs:1096`), seeded by
`query_seed_signature` (`justify.rs:1001`) which for a subsumption query is exactly
`{sub, sup}`.

### 1c. Is either reusable for a classification probe, and at what layer?

- **`locality.rs`: no.** Wrong algorithm, and empirically inert on the target (1 component).
- **`justify.rs`: not as-is — wrong layer.** Reusing it per-probe means re-running
  `convert_ontology` → `to_nnf` → `absorb` on the module, i.e. exactly the `PreparedOntology`
  rebuild the O(n²) pair loop cannot afford. Its *algorithm* is reusable; its *call site* is not.
- **The reusable layer is `AbsorbedTBox` itself.** See §2. This is the finding that makes the
  lever architecturally cheap rather than architecturally impossible.

**Cannot determine:** whether `is_bot_local` coincides with the published ⊥-locality test on
constructs *outside* this ontology. Its bias direction is documented and safe; its exhaustiveness
is not proven anywhere in-tree, and `localized_candidates` visibly does not trust it (§5.8).

---

## 2. Where a per-probe restriction could be applied

### 2a. The call site is already parameterised — no `PreparedOntology` rebuild needed

```rust
// crates/owl-dl-reasoner/src/lib.rs:7354-7356
fn decide<F>(
    pool: &ConceptPool,
    tbox: &AbsorbedTBox,
    …
// :7387
    let mut pool = pool.clone();
// :7404
    let mut ctx = TableauContext::with_tbox_and_hierarchy(&pool, tbox, hierarchy);
```

and the tableau side takes it by reference with its own lifetime:

```rust
// crates/owl-dl-tableau/src/lib.rs:388-392
pub fn with_tbox_and_hierarchy(
    pool: &'pool ConceptPool,
    tbox: &'tbox AbsorbedTBox,
    hierarchy: &'hier RoleHierarchy,
) -> Self
```

Every `PreparedOntology` entry point (`decide` `:6606`, `decide_raw` `:6645`,
`decide_classify` `:6674`, `decide_with_deadline` `:6713`,
`decide_classify_with_deadline` `:6799`) passes `&self.tbox`. **Substituting a per-probe filtered
`AbsorbedTBox` requires no change to `PreparedOntology`'s construction and no rebuild.**

Two cost facts that matter:

1. **`decide` already clones the whole `ConceptPool` per probe** (`lib.rs:7387`). A per-probe
   O(|rules|) locality fixpoint plus a filtered-tbox clone is the same order as work already paid
   on this path.
2. `AbsorbedTBox::finalize` (`absorb.rs:110-141`) rebuilds the dispatch indices in linear time,
   and the tableau *falls back to a linear scan when the indices are empty* — documented at
   `absorb.rs:60-66` as "for performance, not correctness". So a filtered tbox is trivially
   constructible and trivially safe to hand over.

### 2b. Run the ⊥-locality fixpoint **directly over `AbsorbedTBox`**, not over horned-owl axioms

This is the key design move. Absorption produces exactly the shapes `is_bot_local` already
handles:

| absorbed form | file:line | internalises as | ⊥-local iff |
|---|---|---|---|
| `ConceptRule { trigger: A, conclusion: ψ }` | `absorb.rs:145-148` | `A ⊑ ψ` | `A ∉ sig ∨ ce_is_top(ψ)` — justify's `SubClassOf` arm verbatim (`justify.rs:762`) |
| `residual_gcis` entry `φ` | `absorb.rs:81` | `⊤ ⊑ φ` | `ce_is_top(φ)` |
| `RoleRule { role, guard, target }` | `absorb.rs:156-165` | `guard ⊑ ∀R.target` | `guard ∉ sig ∨ role ∉ sig ∨ ce_is_top(target)` |
| `NominalRule` | `absorb.rs:150-154` | `{a} ⊑ ψ` | conservatively **never** local (ABox-ish) |

I verified the two non-obvious shapes on this ontology agree with the axiom-level test:

- Pairwise disjointness is absorbed by `emit_pairwise_disjoint` (`absorb.rs:414-422`) into one
  rule `Aᵢ ⊑ ¬Aⱼ` per `i<j` pair. Its post-absorption locality test is
  `Aᵢ ∉ sig ∨ Aⱼ ∉ sig`, which is precisely justify's `DisjointClasses` arm
  (`justify.rs:767`, "at most one member non-⊥") for the binary case.
- `ObjectPropertyRange(r, Rng)` is absorbed to the residual `⊤ ⊑ ∀r.Rng`
  (`absorb.rs:402-406`); test = `r ∉ sig ∨ ce_is_top(Rng)`, coinciding with `justify.rs:770`.

So the fixpoint can run with **no re-conversion, no re-NNF, no re-absorption** — just a keep-mask
over the four existing `Vec` fields plus one `finalize()`.

### 2c. The filter points, named exactly

- `apply_concept_rules` pending-list construction: `crates/owl-dl-tableau/src/rules.rs:199-226`
  (both the linear-scan fallback arm at `:202-209` and the indexed arm at `:214-224`).
- `apply_deferred_concept_or_rules`: `rules.rs:582-616` (fallback `:586-599`, indexed
  `:601-615`).
- `apply_residual_gcis`: `rules.rs:443-452`.
- `apply_deferred_or_residuals`: `rules.rs:499-515`.

All four dispatch through `AbsorbedTBox.concept_rules_by_trigger: HashMap<ClassId, Vec<ConceptId>>`
(`absorb.rs:94`) or the flat `Vec`s. The HashMap loses the rule *index*, but the pair
`(trigger, conclusion)` is a sufficient key, so either a per-probe
`HashSet<(ClassId, ConceptId)>` drop-set carried on `TableauContext`, or a filtered clone +
`finalize()`, works. The clone is the simpler and (on the reachable population) the cheaper.

### 2d. **The cheap "signature intersection" predicate the brief floats is NOT sound-as-exact**

> *"Could the filter be a cheap per-probe predicate over an index computed once (e.g. 'does this
> rule's conclusion signature intersect the probe's relevant signature?')"*

**No — not as a completeness-preserving filter.** ⊥-locality is a **fixpoint**: the relevant
signature *grows through the rules you keep*. A one-shot intersection against a pre-computed
"relevant signature" is a different, unquantified approximation. It would still be FP-safe (it
only removes axioms) but its completeness has no theorem behind it, so a probe would return
"not subsumed" while classify reports `incomplete: false`. **That is precisely the D10 bug class**
(`docs/…/d10-bug-class-recipe.md`): a gate certifying completeness while the engine drops an
axiom, which is worse than a DNF.

The fixpoint is not expensive enough to justify the shortcut. On `ore_ont_10019` it is 150 axioms
/ 182 rules and converges in a handful of sweeps; my Python port runs 2162 full extractions in
seconds.

### 2e. Do it **per sub-class**, not per pair — and the reason is a theorem, not a heuristic

For the ⊥-module `M_C` of seed `{C}`: any name `D ∉ sig(M_C)` may be interpreted as ⊥, so
`O ⊨ C ⊑ D` would force `O ⊨ C ⊑ ⊥` — and `C ⊑ ⊥` *is* over `sig(M_C)`. Hence:

> **`M_C` decides every pair `(C, ·)`.** For `D ∈ sig(M_C)`, `M_C ⊨ C ⊑ D ⟺ O ⊨ C ⊑ D`. For
> `D ∉ sig(M_C)`, either `C` is unsatisfiable (detected inside `M_C`) or `C ⋢ D`.

This amortises one extraction over all ~46 pairs sharing a sub, and it matches the existing
per-class granularity of `HyperCache::classify_labels` (`lib.rs:3924`) rather than fighting it.

**Measured, not asserted.** Per-sub-class modules, `rustdl explain` on each of the 61
HermiT-entailed *direct* pairs:

```
per-sub-class module on 61 HermiT-entailed direct pairs: yes=60  NO(miss)=0  DNF=1
non-yes: DNF:SulfoxideGroup->SulfinicAcidGeneralGroup
```

Zero completeness loss. The single DNF is the pair rustdl already misses on the **full**
ontology (`docs/2026-08-04-ore-10019-rootcause.md:79`).

### 2f. The wedge is a separate path and is NOT covered

`HyperCache::build` (`lib.rs:3116-3119`) clausifies from `InternalOntology` via
`owl_dl_core::clause::clausify_with_stats` — **not** from `AbsorbedTBox`. Filtering
`AbsorbedTBox` therefore reaches **only the main tableau**. On `ore_ont_10019` that is the right
target (84.6% main tableau vs 15.3% `hyper::solve`, per the root-cause doc §5), but it means the
label cache / wedge oracle keeps the unrestricted clause set. Extending the filter to the wedge
collides head-on with the clause-index amortisation — see §5.1.

---

## 3. The addressable set, measured

### 3a. Method (stated, so it can be checked and re-run)

1. Ported `ce_is_bot` / `ce_is_top` / `sub_ope_is_bot` / `is_bot_local` /
   `collect_component_entities` / `extract_bot_module` from `justify.rs` to Python **arm-for-arm**
   for every construct occurring in `canon.owx`; everything else falls into the same
   `_ => false` (keep) arm the Rust uses.
2. Parsed `/data/dumontier/ore-run/pilot/ore_ont_10019.owl/canon.owx` → **150 logical axioms**
   (55 `DisjointClasses`, 47 `SubClassOf`, 29 `EquivalentClasses`, 5 `SymmetricObjectProperty`,
   5 `ObjectPropertyDomain`, 5 `ObjectPropertyRange`, 4 `SubObjectPropertyOf`), 47 declared
   classes.
3. Ran the fixpoint per probe with `seed = {sub, sup}` (matching `query_seed_signature`,
   `justify.rs:1001`) or `seed = {C}`.
4. Materialised selected modules back to OWL/XML (all `Declaration`s retained, module axioms only)
   and ran the real binary on them.

**Instrument acceptance, declared and met.** Two independent prior observations had to reproduce
before I trusted any new number:

- the definition census: **26** conjunctive definitions, **10** `CarbonAtom`-triggered,
  **0** with ≥2 atomic conjuncts — matches
  `docs/2026-08-04-absorption-on-10019-is-fully-blocked.md:14-21` exactly;
- the `sat` partition: my unrestricted arm reads **11 sat / 36 HANG**, matching the root-cause
  doc §4b exactly, including *which* 11.

Trigger census (full, for the record): `CarbonAtom` 10, `CarbonGroup` 6, `OrganicGroup` 5,
`NitrogenAtom` 3, `OxygenAtom` 2.

### 3b. Per-pair modules over all 2162 ordered class pairs

| | value |
|---|---|
| module size (of 150 logical axioms) | min 0, **mean 102.1**, max 140 |
| `CarbonAtom`-triggered rules surviving (of 10) | min 0, **mean 6.48**, max 10 |
| histogram of survivors | **{0: 380, 1: 40, 2: 432, 10: 1310}** |
| all 26 conjunctive defs surviving | mean 17.19, max 26 |
| **probes retaining ALL 10** | **1310 / 2162 = 60.6%** |

Restricted to the 61 HermiT-entailed direct pairs: mean 4.36/10, 39.3% retain all 10.

**The distribution is bimodal — 0, 1, 2, or 10, nothing between.** That is the whole result. There
is no gradual reduction to exploit: either the module misses `CarbonAtom` entirely, or it has all
ten rules.

### 3c. Per-class modules split exactly along the hang/complete line

| set | n | mean \|module\| | mean CarbonAtom rules kept | retaining all 10 |
|---|---:|---:|---:|---:|
| classes that **HANG** unrestricted | 36 | 82.7 / 150 | **5.25 / 10** | **17 / 36** |
| classes that **complete** unrestricted | 11 | 29.3 / 150 | **0.00 / 10** | **0 / 11** |

**The reduction is concentrated on the classes that already complete.** The 17 hard classes keep
126–127 of 150 axioms and 25 of 26 conjunctive definitions: `AcylBromide/Chloride/Fluoride/Iodide`,
`Acyl`, `AcylHalide`, `Aldehyde`, `Amide`, `CarboxylicAcid`, `Ester`, `Imine`, `Ketone`,
`SulfinicAcidGeneral`, `SulfonicAcidDerivative`, `SulfonicAcid`, `SulfonylHalide`, `Sulfoxide` —
i.e. exactly the cardinality-heavy definitions.

### 3d. Why `CarbonAtom` leaks in

Top axioms that first pull `CarbonAtom` into the signature, counted over all 2162 probes:

| probes | axiom |
|---:|---|
| **532** | `EquivalentClasses(Alkyl, CarbonAtom ⊓ ∃hSBW.(H ⊔ C))` |
| 260 | `EquivalentClasses(AcylHalideGroup, …)` |
| 90 | `EquivalentClasses(AcylBromideGroup, …)` |
| 88 | `EquivalentClasses(AcylGroup, …)` |

**This is the same two-axiom minimal set the root-cause doc's delta-debugging isolated**
(`{AcylHalideGroup, Alkyl}`, `docs/2026-08-04-ore-10019-rootcause.md:185-202`) — reached
independently by a completely different instrument. Once either enters the module, `CarbonAtom`
enters `sig` and all ten rules follow in the next sweep.

### 3e. Executed rescues — the settling measurement

**`sat`, all 47 classes, 5 s cap, one process per class, same binary both arms:**

| arm | sat | HANG |
|---|---:|---:|
| full 150-axiom ontology | 11 | **36** |
| per-class ⊥-module | **30** | **17** |

**19 of 36 hangs rescued.** And the 17 survivors are *exactly* the 17 classes §3c predicted
("retains all 10") — a perfect predictor, no exceptions.

Spot checks:

| probe | full | its ⊥-module |
|---|---|---|
| `sat CarbonAtom` | **HANG >25 s** | module = **1 axiom** → `sat`, **0.00 s** |
| `sat KetoneGroup` | **HANG >25 s** | module = **126 axioms** → **HANG >25 s** |
| `explain SulfoxideGroup ⊑ SulfinicAcidGeneralGroup` | DNF (>3 min per root-cause doc) | module = 126 axioms → **DNF at 60 s** |

The bare-atom symptom the root-cause doc flags as "the single sharpest piece of evidence" (§4b)
**is** fixed by locality. The hard classes are not, and the one genuinely blocked subsumption is
not.

### 3f. Pair-level rescue rate, sampled

Per-pair modules, `rustdl explain`, 3 s cap, both arms same binary, seed `20260804`:

| bucket | size | n sampled | DNF on full | DNF on module | **rescued** | verdict disagreements |
|---|---:|---:|---:|---:|---:|---:|
| **REDUCED** (<10 rules kept) | 852 | 60 | 34 | **0** | **34** | **0** |
| **INERT** (all 10 kept) | 1310 | 40 | 32 | 32 | **0** | **0** |

Two things to take from this:

1. **When locality reduces, it works — completely.** 34 of 34 sampled DNF probes in the reduced
   bucket became decided. And 0 verdict disagreements across all 100 sampled pairs is empirical
   support for exactness in *both* directions on this ontology.
2. **DNF concentrates in the bucket locality cannot touch**: 80% DNF rate in the inert bucket vs
   56.7% in the reduced one. Weighting the sampled rates by bucket size, roughly **32% of DNF
   probes sit in the reducible bucket and ~68% do not.**

**Frame caveat, stated plainly.** Classify's tier walk does not probe all 2162 pairs, so the
32%/68% split cannot be mapped directly onto the reported "154 pairs hit the 1000 ms per-pair
timeout". The exact list *is* recorded internally — `stats.timed_out_pair_ids`, surfaced by
`ClassificationStats::undecided_pairs()` (`crates/owl-dl-reasoner/src/classify.rs:706`) — but it
is **not exposed on the CLI**, and dumping it needs a code addition, which is out of scope for a
report-only task. **Cannot determine** the exact per-bucket split of the 154 without that.

### 3g. Answer to the brief's question

> *"If the answer is 'most rules are relevant to most probes', the lever is dead and that is the
> single most valuable thing you can tell me."*

**Most rules are relevant to most of the probes that cost anything, and irrelevant to most of the
probes that are already cheap.** 60.6% of pairs retain all 10; the 17 classes that generate the
wall retain 126/150 axioms and 10/10 rules; the reduction lands on the 11 classes that already
complete in milliseconds plus 19 mid-tier classes. So the lever is **not dead**, but it is *not
the fix for `ore_ont_10019`* and must not be gated on it.

---

## 4. Does the deferred-Or mechanism already do part of this? — Yes, and it is the mechanism that already failed

**The 10 `CarbonAtom` disjunctions do go through the deferred path.** Confirmed against code and
against the instrument:

- `apply_concept_rules` **explicitly skips `Or(_)` conclusions** in both arms —
  `rules.rs:204` (`!matches!(pool.get(rule.conclusion), ConceptExpr::Or(_))`) and `rules.rs:217`
  — with the rationale spelled out at `rules.rs:190-197`.
- `apply_deferred_concept_or_rules` (`rules.rs:546`) materialises them at saturate stable-state,
  driven from `crates/owl-dl-tableau/src/saturate.rs:168` and `:247`.
- `rustdl tbox-stats` on `canon.owx`: `concept_rules: 182`, **`concept_rule_or: 29`**,
  `residual_gcis: 5` (`residual_or: 5`). So all 29 `EquivalentClasses`-derived ⇐ rules are
  `Or`-concluded and all take the deferred path; the 10 `CarbonAtom`-triggered ones are among them.

**Why deferral does not save them — confirmed against the code, as the brief asked.** The
deferral test is `needs_deferred_or` (`rules.rs:642`), and its semantic core is *the same
syntactic membership test* as the search's open-test:

```rust
// rules.rs:661  (is the Or itself already a label?)
if c_maybe_present && labels.binary_search(&c).is_ok() { return (false, false); }
// rules.rs:689-692  (is any disjunct already a label?)
let any_disjunct_present = args.iter().any(|d| … && labels.binary_search(d).is_ok());
```

versus

```rust
// search.rs:502-503
if let ConceptExpr::Or(args) = pool.get(c)
    && !args.iter().any(|d| labels.binary_search(d).is_ok())
```

Identical predicate. The disjuncts of these heads are `Max(0, r, C)` and `Min(2, r, C)` — the NNF
of `¬(=n r.C)`, flattened into the head by `ConceptPool::or` — plus the defined class `D`. **None
of them is ever syntactically a label**, even though the `Max` disjuncts are semantically true on
almost every node. So `needs_deferred_or` returns `true`, the `Or` is materialised on every
`CarbonAtom` node, and `first_open_disjunction` (`search.rs:484`) then finds it open by the very
same test.

Deferral **relocates** the branching from the inner saturation loop to stable-state; it does not
eliminate it. The Phase-3 bloom prefilter (`rules.rs:650-683`) makes the *test* cheap, not the
*outcome* different.

This is the sharpest argument for why locality is a genuinely different axis: it drops the rule
**before it can fire at all**, which is why `sat CarbonAtom` flips from HANG to 0.00 s while
deferral never could. It is also the sharpest warning: the tree already contains one
relevance-shaped mechanism that fires on this pattern and does not help, so a second one needs
measured rescues (§3e/§3f), not an argument from plausibility.

---

## 5. Interaction risks

### 5.1 Clause-index amortisation — the hard conflict

`RUSTDL_CLASSIFY_AMORTIZE_IDX` (`lib.rs:2085`, **default ON**) shares one
`Arc<ClauseIndexes>` base (`lib.rs:2957`, built `:3394-3419`) across every pair, plus an
**append-only** per-pair delta (`HyperEngine::new_with_prebuilt_extras`, `lib.rs:3792-3796`).
**A per-probe axiom subset is a removal, and removal cannot be expressed as an append.** Either:

- abandon the amortisation for locality-restricted probes → give back the measured 11–13% on
  `ore_ont_1508` / `ore_ont_12698`; or
- rebuild the base per module → if modules are per-class, that is *exactly* the
  O(#clauses)-per-class rebuild `RUSTDL_CLASSIFY_LABELS_AMORTIZE` (`lib.rs:2107-2112`) was built
  to eliminate (`ore_ont_1508` 197.83 → 94.89 s; `ore_ont_12698` 98.78 → 5.33 s).

**Restricting the wedge by locality would undo that work.** Restricting only the main tableau's
`AbsorbedTBox` sidesteps this entirely (§2f) — and on `ore_ont_10019` the main tableau is 84.6% of
the stall — at the cost that the label oracle is then computed on the *unrestricted* KB.

### 5.2 Label cache / `LabelOracle`

Built once per class (`HyperCache::classify_labels`, `lib.rs:3924`), consumed **subtractively** at
`classify.rs:3155-3178` (`D ∉ labels(C)` ⇒ skip `subsumes_via_tableau`). A per-sub-class module is
structurally compatible (same granularity), the error direction is safe (smaller KB ⇒ fewer labels
⇒ more pruning ⇒ MISS not FP), and the §2e theorem makes the pruning exact **provided the
extraction is a faithful fixpoint**. If it is an approximation, this is the amplifier — see 5.3.

### 5.3 Top-down walk and Hasse reduction — the amplifier

`find_direct_parents_top_down` (`classify.rs:3091`) pushes a candidate's children onto the
frontier **only on a positive verdict**. One locality-induced negative therefore silently removes
an entire subtree, and `direct_supers` presumes a *transitive* subsumption relation, which
per-probe-varying incompleteness can break (`A⊑B`, `B⊑C` decided, `A⊑C` missed ⇒ an inconsistent
Hasse diagram). Exact ⊥-locality cannot do this; the §2d shortcut can. This is the concrete
reason to insist on both the fixpoint *and* the per-sub-class formulation — one module per sub
means the whole row of the matrix is decided against one consistent axiom set.

### 5.4 `RUSTDL_CLASSIFY_BACKFOLD`

`inject_backfold_derived_sups` (`classify.rs:3040`, called `:2917`) reads
`LabelOracle::Sat::derived_sups`, i.e. names proved over the **per-class merge-enriched wedge
graph** (`classify.rs:331-337`). A restricted label cache shrinks that graph, so the branch-free
∃-composition can lose the galen pair the flag exists for
(`docs/known-limitations/galen-defined-class-monotonicity-residual.md`). **Must be re-gated on
galen, not on `ore_ont_10019`.**

### 5.5 Incremental `horn_fixpoint`

`RUSTDL_HYPER_INCREMENTAL_FIXPOINT` (`lib.rs:2067`; `hyper.rs:2079`) seeds the root graph **once**
in `decide_with_deadline` and thereafter drains only the per-branch worklist delta. Per-probe
clause-set variation is compatible in principle (each probe owns its engine), but the seeding runs
against the shared base — the same conflict as 5.1, not an independent one.

### 5.6 Tier walk / `RUSTDL_CLASSIFY_SAME_TIER`

`classify_top_down_internal` (`classify.rs:2049`) groups classes by EL/told-subsumer count taken
from the **full** closure. If locality changes which subsumptions are found, tier membership can
shift. Note the two levers already interact on this exact ontology: `RUSTDL_CLASSIFY_SAME_TIER=1`
recovers 2 of `ore_ont_10019`'s 3 misses (root-cause doc §3), so they must be measured together,
not additively.

### 5.7 `abox_irrelevant_to_classify`

`decide_classify` (`lib.rs:6674`) already drops the ABox for classify when
`abox_irrelevant_to_classify` (`lib.rs:5990`) holds. `is_bot_local` **keeps** every ABox assertion
(`justify.rs:788-790`), so an IR-level port must not start dropping them, or two independently
justified subtractions would compose in an unaudited way.

### 5.8 The obligation this lever creates that justify does not have

`localized_candidates` does **not** trust its own locality classifier:

```rust
// justify.rs:1115-1118
if entails(&ontology_from(fixed, &module), q)? { Ok(Some(module)) }
else if entails(&ontology_from(fixed, all_candidates), q)? {
    Ok(Some(all_candidates.to_vec())) // locality bug — safe fallback
```

**A classify probe cannot have that net.** Detecting "the module wrongly failed to entail"
requires running the full probe, which is the cost being avoided. So a classify-side locality
lever must be gated by a *differential identity test* — module-restricted vs unrestricted
classification closures, byte-identical, on fixtures where the restriction demonstrably fires —
rather than by a runtime fallback. Per
`docs/…/sabotage-your-own-guard-tests.md`, that gate must then be sabotaged (break the fixpoint;
confirm the test fails) before it is claimed to protect anything, and per
`docs/2026-08-04-absorption-on-10019-is-fully-blocked.md` §Method-notes it must be checked that
the gate *can* fire on its fixtures at all.

### 5.9 FP-safety and `incomplete`, verified as asked

- **Structural FP-safety holds.** The probe is `sat(X ⊓ ¬D)`; removing axioms weakens the KB, so
  the probe can only become *more* satisfiable ⇒ fewer reported subsumptions and fewer unsat
  classes ⇒ MISS, never FP.
- **`trust_sat` does not break it.** With `RUSTDL_HYPERTABLEAU_TRUST_SAT` on (default), a wedge
  `Sat` is trusted as "not subsumed". A restricted KB makes `Sat` strictly *more* likely — still
  only more MISSes. No new FP surface.
- **The one FP-shaped mechanism in the tree is not fed by this.** `RUSTDL_SNAPSHOT_CAPTURE` asserts
  a *positive* from one model and is FP-unsound by construction — it is **default OFF** since
  2026-06-08 and locality does not feed it. `RUSTDL_PSEUDO_MODEL` is subtractive (`Ok(false)`
  only), same safe direction.
- **The real risk is `incomplete: false` on a MISS.** ⊥-locality is entailment-preserving, so the
  honest `incomplete` claim survives *only if* the implementation is a faithful fixpoint of a
  correct locality test. `is_bot_local`'s bias (unhandled ⇒ keep) is the safe one.
- **One open proof obligation.** §2b applies the fixpoint **post-conversion**, over `AbsorbedTBox`
  and therefore over synthetic names (Tseitin, `DKey`, `NomKey`). The internalised axiom set is
  logically equivalent to the source modulo a definitional extension, but ⊥-locality of a
  definitional extension is not *automatically* ⊥-locality of the source. **Cannot determine
  without a proof.** The conservative alternative is to run the fixpoint over
  `InternalOntology.axioms` (pre-absorb, post-convert) and map the surviving axioms to rules,
  which costs an absorb per probe.

---

## 6. Verdict

**VIABLE-WEAK.**

**What is genuinely established:**

- The lever is **FP-safe structurally** and **exact by the ⊥-locality theorem**, and the in-tree
  ⊥-locality test (`justify.rs`) is faithful-with-a-safe-bias rather than an approximation. Zero
  verdict disagreements over 100 sampled pairs plus 60/61 oracle-entailed pairs recovered with
  0 misses is real empirical support on this ontology.
- It is **architecturally cheap**: `decide` already takes `tbox: &AbsorbedTBox` per probe
  (`lib.rs:7356`) and already clones the pool per probe (`lib.rs:7387`), and the fixpoint can run
  directly over `AbsorbedTBox` with no re-conversion (§2b). No `PreparedOntology` rebuild.
- It **does rescue real probes**: 19 of `ore_ont_10019`'s 36 hanging per-class probes, including
  the bare-atom symptom the root-cause doc calls its sharpest evidence, and 34 of 34 sampled DNF
  pairs in the 39.4% reducible bucket.

**Why it is nonetheless WEAK, and specifically not the fix for `ore_ont_10019`:**

- **60.6% of pairs retain all 10 disjunctive rules**, and DNF concentrates there (80% vs 56.7%).
- The **17 hard classes retain 126/150 axioms and 10/10 rules**; `sat KetoneGroup` hangs
  identically on its own module.
- The **one genuinely blocked subsumption** (`SulfoxideGroup ⊑ SulfinicAcidGeneralGroup`) still
  **DNFs at 60 s** on its 126-axiom module. The lever recovers **no** completeness on this file.
- Best case it shaves roughly a third of the 97 s wall. Konclude does this file in 0.04 s. The
  root-cause doc's conclusion stands unchanged: the fix shape for `ore_ont_10019` is
  **Horn/definitorial absorption of the ⇐ direction**, because only that removes the branching and
  the speculative generation together.
- `ore_ont_10019` is a **single connected component at 100% dominance** — a tight blob. That is
  *why* the lever is weak here, and it is also the reason `ore_ont_10019` says nothing about the
  lever's value on a loosely-coupled ontology.

### The single measurement that would settle it

**A corpus-scale per-class ⊥-module reduction census**, over the ~1,913 OK ORE ontologies and
especially the ~157 v0.4.13 DNF survivors: for each ontology, the distribution of
`|M_C| / |axioms|` and of surviving `concept_rule_or` count.

**Cheapest way to run it: no build, no reasoner.** The fixpoint is a pure syntactic pass over
`canon.owx`; the Python port used here runs 2162 extractions on a 150-axiom file in seconds. Point
it at the ORE canon corpus and cross-reference the DNF list. An afternoon, zero risk to the tree.

**Decision rule, declared in advance:**

> Build the lever only if a material number of the DNF survivors show **median per-class module
> ≤ ~50% of axioms** *and* a **>50% drop in surviving `concept_rule_or`**.

`ore_ont_10019` fails both criteria on its hard classes (84% of axioms retained, 0% drop) and
**must not be the gating ontology**. That is the same mistake
`docs/2026-08-04-absorption-on-10019-is-fully-blocked.md` records: *"run the instrument that
counts the mechanism's addressable set on the target, and put the number in the plan next to the
decision rule."* This document is that instrument's output for one target; the census is the
population version.

If the census passes, scope the build as: (1) `AbsorbedTBox`-level ⊥-locality fixpoint, per
**sub-class**; (2) main tableau only, wedge untouched (avoids §5.1); (3) gated by a
module-vs-full closure byte-identity test that is *sabotaged before being trusted* (§5.8); (4)
re-gated on **galen** for `RUSTDL_CLASSIFY_BACKFOLD` (§5.4) and on the MISSED net
(baseline 5,198) for completeness, plus a full 1,920-ontology two-arm sweep for the DNF tail,
since the MISSED net's frame is drawn from completers and structurally cannot see an
`ok → dnf`.

---

## 7. Method notes

- **The instrument was accepted against two prior direct observations before any new number was
  read** — the 26/10/0 definition census and the 11-sat/36-HANG partition. Both reproduced
  exactly. Per the root-cause doc's own §10, a result that contradicts an earlier direct
  observation is a harness bug until proven otherwise; here nothing contradicted, which is what
  licensed trusting the 19/36.
- **A second, independent instrument re-derived a known result.** The top axiom pulling
  `CarbonAtom` into the signature (`Alkyl`, then `AcylHalideGroup`) is the same two-axiom minimal
  set the root-cause doc found by delta-debugging. Two unrelated methods agreeing is worth more
  than either alone.
- **The bimodal histogram is the finding, not the mean.** `mean 6.48 of 10` reads like "a third of
  the rules are dropped on average", which would be encouraging and would be wrong. The
  distribution is `{0, 1, 2, 10}` with 60.6% at 10. A mean over a bimodal distribution would have
  produced an optimistic plan.
- **`locality-stats` was the cheapest disconfirmation available and should have been run first.**
  One component, 100% dominance — three seconds of work that predicts most of §3 and immediately
  disqualifies the in-tree `locality.rs` as the machinery to reuse.
- **The brief's suggested cheap predicate is unsound-as-exact and the fixpoint is not expensive.**
  Worth stating because the cheap version is the tempting one and its failure mode is silent
  (`incomplete: false` on a MISS), not loud.
- **`cannot determine`, recorded twice rather than inferred:** the exact bucket split of the 154
  timed-out pairs (needs `undecided_pairs()` exposed, i.e. a code change), and whether
  post-conversion ⊥-locality over synthetic names preserves the theorem (needs a proof).
