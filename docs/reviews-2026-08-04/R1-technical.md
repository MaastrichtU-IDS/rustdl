# R1 — adversarial technical review of `docs/superpowers/plans/2026-08-04-definitorial-absorption.md`

Reviewer brief: find every way this plan FAILS or produces a WRONG result. Every claim below is
backed by `file:line` or by a command I ran in this tree.

---

## Verdict

**NO-GO.** Phase 1 is a **provable no-op on `ore_ont_10019`** — the in-tree instrument the
root-cause doc itself quotes (`..with extra ¬Atomic: 0`) *is* Phase 1's addressable-set counter,
and it reads **0 of 29** on the target; I re-ran it and confirmed, and independently confirmed by
parsing the ontology that **all 26 conjunctive definitions have exactly ONE atomic conjunct**.
Separately, the plan's "no engine change / preprocessing only" architecture is **false for the main
tableau**, which is where 84.6% of the stall lives.

---

## Blockers

### B1. Phase 1's addressable set on `ore_ont_10019` is ZERO. The plan's central bet cannot fire.

Phase 1 forms a surrogate chain over the **atomic** conjuncts of `C₁ ⊓ … ⊓ Cₙ ⊑ D`. A chain needs
**≥ 2** atomic conjuncts: with one atomic conjunct `A`, the "surrogate" is `S ≡ A`, so the trigger
is still `A` and the residual head is bit-identical to today's.

`ore_ont_10019` has **exactly one atomic conjunct in every single conjunctive definition.** Parsed
from `/data/dumontier/ore-run/pilot/ore_ont_10019.owl/canon.owx`, flattening nested
`ObjectIntersectionOf` (which `ConceptPool::and` also flattens):

```
conjunctive defs: 26   with exactly 1 atomic conjunct: 26
```

CarbonAtom ×10 (AcylBromide, Acyl, AcylHalide, Aldehyde, Alkyl, Amide, Aryl, Carbonyl, Ester,
Ketone), CarbonGroup ×6, OrganicGroup ×5, NitrogenAtom ×3, OxygenAtom ×2. The other 3 of the 29
`Or`-concluded rules are the `ObjectUnionOf` definitions (HalogenAtom 5-way, HeteroAtom 4-way,
CarbonGroup 2-way), whose `⇒` direction absorbs to `trigger → Or(members)` — also untouched by
Phase 1.

**The shipped instrument already says this, and the root-cause doc already quotes it.** I re-ran it:

```
$ ./target/release/rustdl residual-absorbability .../ore_ont_10019.owl
# concept_rules:                182
#   conclusion_is_or:           29
#   ..with extra ¬Atomic:       0  (binary-absorption candidates)
```

and `crates/owl-dl-core/src/residual_absorbability.rs:47-56` states in its own doc comment that
this counter *is* the binary-absorption (Hudek & Weddell) payoff column:

> "Two or more `Not(Atomic)` disjuncts — binary absorption (Hudek & Weddell) would fire only when
> all the atomic conjuncts are present … The place binary absorption actually pays is
> [`ResidualAbsorbabilityStats::concept_rule_or_with_extra_not_atomic`]."

`docs/2026-08-04-ore-10019-rootcause.md:133` reproduces the `0` verbatim, and the plan cites that
very line (plan:134 region, "so none of the 29 is a binary-absorption candidate") — then proposes
Phase 1 anyway. **The plan contradicts the evidence it cites, in the document it cites.**

Consequences for the plan as written:
- Task 3's first branch ("`10019` reaches 162 … ⇒ default ON") is unreachable by construction.
- Task 3's last branch ("`10019` unchanged ⇒ Phase 1 is refuted") is **guaranteed** before any code
  is written. The whole of Task 2 is a pre-determined null result.
- Task 2 Step 5 ("assert no surrogate IRI appears in classify output … on `10019`") is **vacuous** —
  no surrogate is minted there.
- Task 2 Step 6 ("measure `10019`") will report byte-identical output, which is correct behaviour
  and tells you nothing.

**Required change:** delete Phase 1 as the lead task, or re-target the plan onto an ontology where
`concept_rule_or_with_extra_not_atomic > 0`. If `10019` is to stay the motivating instance, the plan
must address the conjunct kinds that actually occur there (`∃` ×13 heads, cardinality ×13 heads),
i.e. what the plan defers to Phase 2 plus an unlisted `∃` case.

### B2. "Architecture: Preprocessing only … No engine change" is FALSE for the main tableau.

The main tableau consumes `AbsorbedTBox`. Its only concept-level rule carrier is

```rust
// crates/owl-dl-core/src/absorb.rs:144-148
pub struct ConceptRule { pub trigger: ClassId, pub conclusion: ConceptId }
```

a **single-atom** trigger, dispatched by scanning single `Atomic(cls)` labels
(`crates/owl-dl-tableau/src/rules.rs:171-179`). `absorb.rs:45` says so outright:

> "Multi-trigger absorption (`A ⊓ B ⊑ C`) is a Phase 4 refinement."

So the Horn rule Phase 1 wants — `C₁ ⊓ C₂ → S₁` — **is not expressible in `AbsorbedTBox`.** The only
encoding available with today's structures is `ConceptRule{ trigger: C₁, conclusion: Or([¬C₂, S₁]) }`
— i.e. **another disjunction**, the exact thing being eliminated.

This is not an implementation detail; it is the logical crux. **A surrogate renames a conjunction; it
does not make that conjunction derivable by a single-atom trigger.** Definitorial absorption of a
conjunctive antecedent *requires* a conjunctive trigger. Therefore Phase 1 on the main tableau needs
a new rule kind in `AbsorbedTBox` plus a new deterministic rule in `owl-dl-tableau/src/rules.rs`.
Given the root-cause doc's own attribution — "Profiling attributes **84.6% of the stall to the main
tableau** vs 15.3% `hyper::solve`" (rootcause:216) — the plan's architecture claim holds only for the
engine that is *not* the bottleneck.

**Required change:** state the engine change explicitly (new `ConceptRule` variant with a multi-class
/ role-guarded trigger + `rules.rs` firing + trail/deps handling), re-price the task, and re-scope
the canary/sabotage list to cover it. Or restrict the plan to the wedge and accept it addresses
15.3%.

### B3. The wedge ALREADY implements Phase 1's effect, and still stalls. This falsifies the plan's premise.

`crates/owl-dl-core/src/clause.rs:442-491` (`absorb_hard_antecedent`) partitions the antecedent
conjuncts into **soft** (anything `encode_antecedent` accepts) and **hard**, puts *all* soft conjuncts
into the clause **body**, and negates only the hard ones into the head. `encode_antecedent`
(`clause.rs:501-560`) accepts `Atomic`, `Nominal`, `Top`, `And`, `Or`, **and `Some`**. And when there
is no hard conjunct at all, `clausify_gci` (`clause.rs:413-436`) never reaches
`absorb_hard_antecedent` — it emits a pure Horn clause with a multi-atom body.

So in the wedge, `A ⊓ B ⊑ D` is **already** `A(X) ∧ B(X) → D(X)`, no surrogate, no disjunction — and
`A ⊓ ∃r.C ⊑ D` is already `A(X) ∧ r(X,y) ∧ C(y) → D(X)`. The wedge nonetheless burns 373,919
branches on `KetoneGroup` (rootcause:92-98). **In-tree evidence that Phase-1-equivalent absorption is
insufficient, on this very ontology.** The plan never mentions that the wedge already has it.

**Required change:** the plan must state, for each engine, what absorption already exists, and it must
not claim novelty for a transformation `clause.rs` performs today.

### B4. Wrong file path in the task list — `crates/owl-dl-tableau/src/clause.rs` does not exist.

Plan Task 2 "**Files:**" and the "Tech Stack" line both name `owl-dl-tableau` for the clausifier.

```
$ wc -l crates/owl-dl-tableau/src/clause.rs
wc: crates/owl-dl-tableau/src/clause.rs: No such file or directory
```

The clausifier is `crates/owl-dl-core/src/clause.rs` (the `head_parts` snippet the plan quotes as
"`clause.rs:471-477`" is at `crates/owl-dl-core/src/clause.rs:471-477` — verified). For an
agentic worker this is a hard stop, and it also signals the plan has not distinguished the two
independent clausification paths (`absorb.rs` → main tableau; `core/clause.rs` → wedge) that a
surrogate must be threaded through **separately**.

### B5. The plan does not say WHERE surrogates are minted, and the two options have completely different soundness footprints.

`absorb()`'s signature makes the "in absorption" option impossible as stated:

```rust
// crates/owl-dl-core/src/absorb.rs:174
pub fn absorb(axioms_nnf: &[Axiom], pool: &mut ConceptPool) -> AbsorbedTBox
```

No `&mut Vocabulary`, and `axioms_nnf` is immutable. To mint a *named* surrogate you must either
change the signature at 6 call sites (`lib.rs:4725, 4746, 4764, 6416`, plus two tableau tests) or
fabricate a `ClassId` outside the vocabulary — and a `ClassId ≥ num_classes()` **panics** on any
IRI lookup (`crates/owl-dl-core/src/vocab.rs:36` `&self.by_id[id as usize]`) and would index out of
bounds in `told.rs` (`told.rs:109,177` size their tables at `vocabulary.num_classes()`).

The alternative — an axiom-level pass on `&mut InternalOntology` inside `convert_ontology`, on the
model of `crates/owl-dl-core/src/disjunctive_antecedent.rs:29` — *can* intern, but then the surrogate
reaches the EL saturator, the fragment gates, `told.rs`, the closure, `num_total_classes`, and every
output path. Also note the ordering hazard: `PreparedOntology::from_internal_with_deadline` **clones
the vocabulary at `lib.rs:6315`, before `absorb` at `lib.rs:6416`**, and builds `HyperCache` at
`lib.rs:6357` from the un-mutated ontology — so an absorption-time surrogate is invisible to both
`self.vocabulary` and the wedge.

**Required change:** pick one, name it, and enumerate the consequences for that choice.

### B6. Surrogates leak on the realize path, which `reportable_class_iris` does not cover.

The plan's Global Constraint says "`reportable_class_iris` already filters synthetic `DKey`/`NomKey`
classes — reuse that path." Two errors:

1. It filters **only** `DKEY_IRI_PREFIX` (`crates/owl-dl-reasoner/src/classify.rs:53`). `NomKey`
   classes are never in the vocabulary at all — they are saturator-internal
   (`owl-dl-saturation/src/lib.rs:3342,3544` `tseitin.nominal_by_ind`), so nothing filters them
   because nothing needs to. The plan mis-describes the mechanism it intends to reuse.
2. `reportable_class_iris` has exactly **3** call sites — `classify.rs:912, 945, 2059` — all in
   classify. The realize path builds its own class list with **no filter at all**:

```rust
// crates/owl-dl-reasoner/src/realize.rs:626-635   (realize_saturation_only_internal)
let class_iris: Vec<String> = (0..internal.vocabulary.num_classes()) … .collect();
// crates/owl-dl-reasoner/src/realize.rs:914-923   (realize_tableau_internal)
let class_iris: Vec<String> = (0..internal.vocabulary.num_classes()) … .collect();
```

An interned surrogate therefore appears as an **entailed type of every individual satisfying its
body** in `realize` / `realize --json` / `instances_of` / the Python `realize` wrapper, and would
break `crates/owl-dl-reasoner/tests/pseudo_model_realize.rs`'s byte-identity gate. (The
saturation *fast path* at `realize.rs:795` does filter, by `c.index() < num_user_classes` — which is
the *other* invariant, see B7.)

Also `crates/owl-dl-core/src/convert_back.rs:102` and `crates/owl-dl-cli/src/json_out.rs:502` call
`vocab.class_iri(id)` unconditionally — with an out-of-vocabulary surrogate id (B5 option A) that is
a **panic** in `justify` / `explain` / `--json` axiom rendering, not a leak.

Good news the plan should record: `crates/owl-dl-bench/src/main.rs:455,468,543,556` already excludes
`urn:rustdl-` in addition to `DKEY_IRI_PREFIX`, so a `urn:rustdl-surrogate:` naming scheme covers
the bench diff paths for free. The FP=0 closure-diff gate goes through `Classification` and so is
covered by patching `reportable_class_iris`.

**Required change:** enumerate every output path in the plan (classify hierarchy/unsat/`--json`,
`realize` ×3 sites, `justify`, `explain`, `prove`, `diagnose`, `repair`, `report`, Python
`materialize_*`, bench, closure diff) and add a canary per path, not one assertion on the canary
fixture.

### B7. Task 3's decision rule is wrong: ΔMISSED > 0 does **not** imply the rewrite is unsound.

Plan: "**ΔMISSED > 0** ⇒ the rewrite is not semantics-preserving as implemented. **That is a bug, not
a trade.**"

A logically conservative extension can still lose entailments here, because rustdl's classifier is
heuristic in ways that key on **class counts and ids**:

- `classify.rs:2409-2414` computes `subsumer_counts[i] = closure.subsumers_count(ClassId::new(i))`,
  and `subsumers_count` is the **raw row length** including synthetics
  (`owl-dl-saturation/src/lib.rs:2524-2526`). `classify.rs:2437-2454` then groups classes into
  **tiers of equal subsumer count**, and the tier walk **never compares same-tier classes** (the
  documented `RUSTDL_CLASSIFY_SAME_TIER` limitation). An axiom-level surrogate becomes an EL-derivable
  subsumer of every class it defines, shifting those classes between tiers and **changing which pairs
  the walk compares**. Gains and losses are both possible, from a sound rewrite.
- `classify.rs:1288-1301` and `2409-2411` identify the *filtered* index `i` with `ClassId::new(i)`,
  and `classify_pure_el` terminates its closure row scan with `if j >= n { break; // synthetic
  Tseitin/DKey id — outside user vocabulary }`. **This is a load-bearing invariant: every synthetic
  class must occupy a strict SUFFIX of the vocabulary id space.** Violate it (mint a surrogate before
  some user class) and `classes[i]` no longer denotes `ClassId(i)` — that is a silent
  **misattribution of subsumptions, i.e. an FP-shaped corruption**, not a MISS. I could not verify
  exhaustively that today's `DKey` interning always lands in the suffix (it happens inside the
  component loop, `convert.rs:2207-2213`, interleaved with user-class interning), so the invariant is
  currently *relied on but unpinned*.
- `told.rs:245-253` recognises told-disjointness **syntactically** from `And(Atomic(a), Atomic(b)) ⊑
  Bot`. If the pass fires on that shape (or on the `X ⊓ Y ⊑ ⊥` axioms
  `RUSTDL_NEG_TO_BOT_GCI`/`negation_gci.rs` produces) and rewrites it to `S ≡ A⊓B` + `S ⊑ ⊥`, the
  told-disjoint pair is **destroyed** — a completeness loss that propagates to the told closure,
  `abox_check` P2/P8, and `minimal_common_subsumers`.

**Required changes:** (a) the pass must be explicitly scoped to EXCLUDE `⊥`-headed antecedent
conjunctions; (b) the plan must state and canary the suffix invariant; (c) rewrite the Task 3 rule to
distinguish "ΔMISSED > 0 from an output/id/table defect" (a bug) from "ΔMISSED > 0 from tier-partition
perturbation" (a real trade that needs an explicit accept/reject), and require a diagnosis before
either verdict.

---

## Concerns

### C1. Task 1's census already exists, has already been run over all 1,920, and the plan does not cite it.

`docs/2026-08-01-residual-absorbability-census.md:117-145` reports, for the whole 1,913-analysed
pool: `concept_rules 148,714,494`, `conclusion is an Or 1,054,027`, **"with an extra `¬Atomic`
disjunct: 199,019"**; for the 161 DNF survivors, **34,667**. That *is* the Phase-1 population count
Task 1 Steps 1–3 propose to build a new subcommand to obtain. The existing
`rustdl residual-absorbability` subcommand emits it per ontology
(`crates/owl-dl-cli/src/main.rs:1793-1796`).

The genuinely missing number is the **per-ontology** breakdown (how many of the 1,920 have
`> 0`, and their distribution) — obtainable by re-running the shipped subcommand, with no new
instrument. Building `definitorial-census` duplicates working code.

### C2. Task 1 Step 2's acceptance criterion is factually wrong and would reject a CORRECT instrument.

Plan Step 2: "it must report ~29 defined classes, **~15 heads sharing `CarbonAtom`**, and cardinality
conjuncts present. **If it does not, the instrument is wrong, not the root-cause doc.**"

The true count is **10**, not ~15: AcylBromideGroup, AcylGroup, AcylHalideGroup, AldehydeGroup, Alkyl,
AmideGroup, Aryl, CarbonylGroup, EsterGroup, KetoneGroup. The remaining 16 conjunctive heads are
triggered by CarbonGroup (6), OrganicGroup (5), NitrogenAtom (3), OxygenAtom (2). The "~15 of the 29"
figure originates at `docs/2026-08-04-ore-10019-rootcause.md:157` and the plan repeats it twice
(plan:20, plan:51). A correct instrument reporting `10` would be discarded under this rule.

Also note `absorb_gci` picks the "first" `Not(Atomic)` in **sorted `ConceptId`** order, not source
order, because `ConceptPool::or` does `v.sort_unstable(); v.dedup()`
(`crates/owl-dl-core/src/ir.rs:413-414`). Any design that assumes control over trigger choice must
account for that. (Harmless on `10019` — one candidate per head.)

### C3. The plan's "Konclude, measured" claim is a misattribution.

Plan:22: "**Konclude, measured:** definitorial surrogate body atoms make `⇐` the Horn rule
`soft ⊓ M₁ ⊓ … → D`."

What was measured is Konclude's **wall** (`rootcause:43` "Konclude 0.04 s"; the plan says "20 ms" —
a second, unexplained factor-2 drift from `rootcause:284`). The **mechanism** is inferred from
rustdl's own comments: `rootcause:284-291` sources it to "the code's own comment (§5) and the
2026-07-16 Phase-0 analysis" — both rustdl documents. No Konclude source, paper, or trace is cited.
Stating an unmeasured mechanism as "measured" is exactly the class of error this project's method
notes exist to prevent.

**On the substance (review question C): the claim does not survive the cardinality conjuncts.** Every
one of `10019`'s 26 conjunctive definitions has one atomic conjunct plus `∃`/cardinality
(B1). So whatever Konclude does, it must be handling the **`∃` and cardinality** conjuncts as body
atoms. Phase 2 (deferred) is therefore the mechanism, and Phase 1 alone provably cannot reproduce
Konclude's result on this ontology. B3 independently confirms it: rustdl's wedge already does the
atomic-and-`∃` half and still stalls.

### C4. The "11 curated closures exact" gate is measured INERT for this change. I ran it.

```
$ for f in bibtex pizza ro sio sulo wine family go-basic; do
    ./target/release/rustdl residual-absorbability ontologies/real/$f.ofn | grep 'extra ¬Atomic'
  done
bibtex 0   pizza 0   ro 0   sio 0   sulo 0   wine 8   family 0   go-basic 0
```

**7 of 8 curated fixtures cannot fire Phase 1 at all.** The only one that can is `wine`, with 8
candidate rules — and `wine` is the ontology with the documented pathological wall. So "11 VERIFIED,
closures exact" would be an inertness check on 10 fixtures and a single genuine test on the hardest
one. This is the precise pitfall the review brief names ("byte-identical can mean never fired"), and
the plan does not acknowledge it.

**Required addition:** measure `concept_rule_or_with_extra_not_atomic` on every gate fixture **before**
running the gate, and report the firing/non-firing split alongside the VERIFIED count. A gate that
cannot fire is not evidence.

### C5. The MISSED net cannot see `ore_ont_10019` — it is not in the population.

```
$ grep -x ore_ont_10019 /data/dumontier/owl-reasoner-harness/baselines/2026-08-03-missed-net-population.txt
(no match, exit 1)
```

400 ontologies, `10019` absent. So the ΔMISSED gate is blind to any gain **or loss** on the target,
and the plan must not present ΔMISSED = 0 as evidence about `10019`. (This is on top of the recorded
structural limitation that the net's frame is drawn from completers.)

### C6. The full-corpus sweep compares an output DIGEST — a surrogate leak makes it uninterpretable.

`/data/dumontier/owl-reasoner-harness/scripts/sweep-arm.sh` invokes the harness with
`--digest-output`. If a surrogate reaches any output byte, **every ontology where the pass fires
reads as "answer changed"**, and the sweep's `ok → dnf` / answer-change signal is destroyed. The plan
must either prove the digest is taken over the filtered class list, or add a surrogate-stripping step
to the arm, **before** the sweep is run.

### C7. The plan does not engage with the prior negative result on absorption, or with the prescribed cheap falsification.

`CLAUDE.md` (v0.4.13 "Closed by measurement — do not re-propose without new evidence") and
`docs/2026-08-01-domain-absorption-results.md:400-441` are directly on point. That document's
conclusion §"What this implies for what to build" names **binary absorption** as the target, sizes it
at exactly the numbers in C1, and then says:

> "**Do not build it yet.** … The next step is the same one-directional falsification that worked
> before: on a high-`rule_or` ontology, **delete** those rules … and see whether it completes. No
> rescue under deletion ⇒ no rescue under absorption, and binary absorption dies cheaply too."

The plan skips that step. Complicating matters, two in-tree records push the other way:
`rootcause:229-232` retracts the 2026-07-16 deletion spike, and `CLAUDE.md`'s method notes say
"**Deletion is NOT computationally stronger than absorption**". **The plan must state which
prescription it follows and why**, rather than silently proceeding past a documented "do not build it
yet".

### C8. Fragment gates: no D10 instance found, but the check must be in the plan.

I checked both directions and could not construct a flip. `is_el_axiom`
(`classify.rs:1621-1622`) requires *all* `EquivalentClasses` members EL, and `is_el_concept`
(`classify.rs:1670-1682`) rejects `Max`/`Min`/`Not`/`All`. Splitting
`D ≡ A ⊓ =1 r.F` into `S ≡ A ⊓ B` (EL) + `D ≡ S ⊓ =1 r.F` (still non-EL) leaves the verdict
unchanged; a pure-EL input stays pure-EL. So no gate is newly *admitted* and no engine newly *drops*
an admitted axiom.

**But this is conditional on two properties the plan must guarantee explicitly:** (i) the pass is
**additive** (original axiom retained) or, if replacing, the replacement is shape-preserving for
every gate predicate; (ii) it does **not** fire on `⊥`-headed antecedents (B7c) or on
`DisjointClasses`-derived shapes. Without (i)/(ii) written down, a later refactor reopens a D10
instance. Add a gate-verdict-identity canary (`analyze_fragment` / `# fragment:` banner, ON vs OFF)
over a fixture set that includes a pure-EL, a Horn, and an out-of-fragment input.

### C9. Memory: one surrogate per head, on 148 M concept rules, is not obviously safe.

The plan flags this ("check the memory consequence of minting a surrogate per head") but does not
bound it. Concretely: an axiom-level surrogate grows `num_user_classes`, hence the saturator's
`O(num_total_classes²)` structures (`owl-dl-saturation/src/lib.rs:605-673`, `subsumed_by =
IdMatrix::with_capacity(num_total_classes)`), which is the D4 dense-matrix regime. The corpus-wide
Phase-1 candidate count is 199,019 rules (C1). **The plan needs a pre-declared RSS budget and a
worst-case ontology named in advance** (`ore_ont_3914`-class, 570 k classes), not a "check".

### C10. `justify` / `explain` meaningfulness is unaddressed.

The plan's D-question list is not in the plan. If a surrogate participates in a derivation,
`justify`'s output either (a) panics on the IRI lookup (B5 option A), (b) prints
`urn:rustdl-surrogate:…` axioms the user never wrote, or (c) must **unfold** surrogates back to their
definition. None of the three is specified. `justify`/`repair` are user-facing and `repair`'s
axiom-removal sets must refer to **original** axioms or the repair is not applicable to the source
file.

---

## Confirmations — verified correct, do not re-litigate

1. **`absorb.rs:461-468` is the right site and the description is accurate.** The loop takes the
   first `as_trigger` match and `break`s; `absorb_gci:470-491` puts the remaining disjuncts into
   `pool.or(rest)` as the `ConceptRule` conclusion. (Caveat C2 on "first" = sorted order.)
2. **`search.rs:502-503` is quoted correctly** and is genuinely a purely syntactic open-test:
   `crates/owl-dl-tableau/src/search.rs:502-503`, `!args.iter().any(|d| labels.binary_search(d).is_ok())`.
3. **`clause.rs:471-477` is quoted correctly** (modulo the crate path, B4) — the `head_parts`
   negation of hard conjuncts is at `crates/owl-dl-core/src/clause.rs:471-477`.
4. **`absorb` runs AFTER NNF**, at `crates/owl-dl-reasoner/src/lib.rs:6415-6416`
   (`nnf_axioms` then `absorb`). A pass that needs pre-NNF shapes must run in `convert_ontology`, as
   `negation_gci` does and as its doc explains.
5. **The `29`/`182`/`5`-residual figures in the root-cause doc reproduce exactly** on the current
   binary (my `residual-absorbability` run above). The root-cause investigation is careful and its
   numbers hold up; the problem is the plan's inference from them, not the measurements.
6. **The FP=0 closure-diff gate is covered by patching `reportable_class_iris`** — it goes through
   `Classification`, whose class list comes from `classify.rs:2059`. And `owl-dl-bench` already
   excludes `urn:rustdl-` (`main.rs:455,543`), so a `urn:rustdl-`-prefixed surrogate name is the right
   choice.
7. **The soundness argument in the abstract is correct.** A fresh atom defined by an equivalence is a
   conservative extension; it adds no entailments over the original signature. Every failure mode I
   found is *implementational* (id-space, output filtering, syntactic pattern-matching tables,
   count-keyed heuristics), which is exactly why the plan is right to say "must be verified, not
   asserted" — B6/B7 are the specific verifications.
8. **The "success is decided classes and wall, never branch count" constraint is correct and
   well-supported** by the refuted `RUSTDL_OR_CARD_SATISFIED` result (`rootcause:237-265`). Keep it.

---

## The single highest-value finding, restated

**Phase 1 is a no-op on `ore_ont_10019`, and the codebase already told us so before the plan was
written.** Phase 1 needs ≥ 2 atomic conjuncts per head; `ore_ont_10019` has exactly **one** in all 26
of its conjunctive definitions (verified by parsing the ontology), and the shipped
`residual-absorbability` counter — whose own doc comment at
`crates/owl-dl-core/src/residual_absorbability.rs:47-56` identifies it as *the* binary-absorption
payoff column — reports **`..with extra ¬Atomic: 0` of `conclusion_is_or: 29`**. That exact zero is
quoted in the root-cause doc the plan is built on (`docs/2026-08-04-ore-10019-rootcause.md:133`) and
in the plan's own "defect" section.

So the plan's phase ordering is **inverted**: everything that is actually disjunctive on this
ontology lives in the conjuncts Phase 1 excludes — 13 heads whose only non-atomic conjunct is `∃`
(yielding the 2-disjunct head `Or([All(r,¬C), D])`, consistent with the trace's dominant
`options=2` bucket, 11,839 of 20,002 at node 9, `rootcause:127`) and 13 whose non-atomic conjuncts
are `=n` cardinality (yielding the `options=3/5/7` buckets). Phase 1 addresses **neither**.

The evidence-supported next move is not a surrogate over atomic conjuncts. It is to **port
`core/clause.rs`'s existing soft/hard antecedent partition (`clause.rs:442-491`) into `absorb.rs`**,
which would convert those 13 `∃`-only heads from disjunctions into deterministic rules with **no new
named class at all** — the "multi-trigger absorption" `absorb.rs:45` already names as unbuilt. That
is a bounded engine change in `AbsorbedTBox` + `rules.rs`, it is aimed at the 84.6% (main tableau)
rather than the 15.3% (wedge), and it does not inherit any of B5/B6/B7's id-space, output-leak, or
told-table hazards, because it introduces no vocabulary entry. The remaining cardinality half is the
genuinely open sub-problem, and B3 is the reason to expect it to be the binding one.
