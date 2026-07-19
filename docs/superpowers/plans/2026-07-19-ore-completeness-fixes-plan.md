# Plan: fix the completeness errors found in the full-ORE 4-reasoner analysis (2026-07-19)

Source analysis: `docs/superpowers/specs/2026-07-19-rustdl-ore-reasoning-misses-analysis.md`.
All errors are **completeness** (rustdl ⊆ Konclude everywhere; 0 FP). This plan fixes
them in value order. **Non-negotiable invariant for every step: FP=0 corpus-wide,
verified by closure-diff against the curated gate + a Konclude/HermiT ORE oracle on
the touched fixtures. A completeness fix that introduces a single FP is reverted.**
Framing is by correctness/soundness/scope, not calendar time.

## Prioritization

| # | error | value | tractability | touches a *proven* guarantee? |
|---|---|---|---|---|
| **P1** | EL compound-∃ GCI (`∃r.C ⊑ ∃s.D`) not propagated | high (confirmed bug, minimal repro) | medium | **YES** — violates "complete on EL" |
| **P2** | Missed TBox unsatisfiability (`∀+⊔`, `≤n+Disjoint`) | medium (3 onts, concentrated) | medium | no (documented trust_sat gap) |
| **P3** | Defined-class / ∀ recognition misses (6.7k subs) | medium (spread thin) | low–medium | no |
| **P4** | SWRL/`DLSafeRule` parse gap (1 ont → r=0) | low | low (mostly a clean-skip) | no |
| **P0** | Benchmark-harness equivalent-IRI normalization | (not a reasoner fix) | easy | n/a — enables clean re-measure |

Recommended order: **P0 → P1 → P2 → P3 → P4** (P0 first so every subsequent fix's
corpus delta is measured against a de-artifacted baseline).

---

## P0 — Harness hygiene: equivalence-canonicalized ORE diff (prerequisite)

**Why first.** ~13k of the 27k raw "misses" are equivalent-IRI/frag collisions on
pure-EL onts (verified `2612` 3341→0). Until the benchmark harness canonicalizes
classes through their entailed equivalences, we cannot cleanly read the corpus
impact of P1–P3.

**Steps**
1. Fold `verify_eq.py`'s logic into the standing ORE harness: before diffing,
   union-find every class over (a) the ontology's named `EquivalentClasses` axioms
   **and** (b) each reasoner's own reported equivalences; compare closures on
   canonical representatives. Unicode-aware IRI parsing (accents — the `1325` bug).
2. Add unsat-normalization to the diff (strip classes each reasoner reports `≡ Nothing`
   / `unsat` from both sides) so "missed unsatisfiability" is reported as a *detection*
   count, not inflated pair counts.
3. Re-run the full ORE diff to get the **de-artifacted baseline**: expected genuine
   miss ≈ 9.4k (6.7k defined-class + 2.7k the P1 bug), 0 FP.

**Verification.** `2612`, `13224` (pure-EL) must show 0 miss after P0; the FP/soundness
verdict must be byte-identical to the current run.

**Risk.** None to the reasoner (harness only).

---

## P1 — EL compound-existential GCI propagation (the confirmed bug)

**Root cause (localized).** GCIs of the form `∃r.C ⊑ ∃s.D` (compound existential on
*both* sides). `absorb.rs` can turn `∃r.C ⊑ Atomic` into a working saturator path
(Pizza test `∃hasTopping.EdibleThing ⊑ FoodItem` passes), but a **compound-∃
consequent** falls to `residual_gcis` and the EL saturator never materializes the
RHS `∃s.D` as an existential fact on the classes that have `∃r.C`. On a pure-EL ont
the residual never reaches the tableau either → silent miss, and `explain` still
reports the closure "complete". Repro fixture:
`docs/known-limitations/fixtures/el-existential-gci-defined-class-gap.ofn`.

**Approach (two candidates; pick by the failing test + soundness).**
- **A (preferred — lower risk): Tseitin the compound-∃ consequent at conversion.**
  Rewrite `∃r.C ⊑ ∃s.D` into `∃r.C ⊑ S'` + `S' ⊑ ∃s.D` with a fresh atomic surrogate
  `S'` (same mechanism the saturator already uses for compound `∃` *bodies* and for
  NomKey/DKey fillers). Then the existing atomic-RHS path (Pizza) fires `X ⊑ S'`,
  and `S' ⊑ ∃s.D` feeds CR5 as a normal existential fact. Purely additive lowering;
  surrogate filtered from reported classes like DKey/NomKey.
- **B: extend CR5 to emit an existential fact directly** when an ∃-antecedent GCI's
  consequent is `∃s.D`. More invasive in the hot loop; keep as fallback if A leaves
  a residual case.

**Files.** `crates/owl-dl-core/src/convert.rs` (or `absorb.rs`) for the Tseitin
rewrite; `crates/owl-dl-saturation/src/lib.rs` only if approach B. Surrogate-class
filtering in `reportable_class_iris` (reasoner).

**TDD.**
1. RED: add the minimal repro as a saturation unit test (`X ⊑ GOCHE` expected) +
   the fixture as a reasoner-level classify test. Watch it fail (rustdl misses).
2. GREEN: implement approach A; the minimal test passes.
3. Regression: `2658`/`3406` recover their ~1350 each (equivalence-canonicalized,
   via P0); byte-identical elsewhere.
4. Canaries: nested case `∃r.C ⊑ ∃s.(∃t.E)`; role-hierarchy case (`s⊑r` present);
   a negative control (GCI absent ⟹ no subsumption — already in the repro).

**Soundness gate.** Approach A only *adds entailed* subsumptions (the surrogate is
`≡`-neutral). Full FP=0 corpus closure-diff (galen/notgalen/sio/wine/ore-10908/
ore-15672/pizza/alehif/ro/sulo) + the ORE Konclude∩HermiT oracle on `2658`/`3406`.

**Expected win.** Recovers 2,700 subs on `2658`/`3406`; restores the "complete on EL"
guarantee on this axiom shape; likely helps some out-of-EL onts whose defined classes
sit behind such GCIs. Gate behind a default-ON env flag (`RUSTDL_EXISTENTIAL_GCI`
or similar) for A/B, per project convention.

---

## P2 — Sound TBox unsatisfiability pre-check (`∀+⊔`, `≤n+Disjoint`)

**Root cause.** On `5014` (`∃hasRoom.Guestroom ⊓ ∀hasRoom.(Guestroom ⊔ …)`) and
`4198`/`16321` (`≤1` cardinality + pairwise `DisjointClasses`), the class is
unsatisfiable but rustdl's saturator/`trust_sat` reports `unsat=0`. It misses a few
*detections*; the large pair counts are just the `≡ Nothing`-block enumeration.

**Approach.** A sound, terminating TBox unsat pre-pass — the TBox analogue of the
shipped `abox_saturation.rs` consequence-based pre-check. For each named class C,
seed `{C}` and run the deterministic Horn closure already available (type
propagation + `∀`-elim on derived successors + `≤n`/functional merge + disjoint/⊥
clash); a derived ⊥ ⟹ `C ⊑ ⊥`. **Under-approximate and sound** (only marks a class
unsat when a clash is *derived*; never the converse). Feed discovered `C ⊑ ⊥` into
the class hierarchy exactly like the label-cache back-fold (branch-free, no tableau
blow-up risk).

**Files.** New `crates/owl-dl-reasoner/src/tbox_unsat.rs` (mirrors `abox_check.rs` /
`abox_saturation.rs`); wire into `classify.rs` before the per-pair loop; gate
`RUSTDL_TBOX_UNSAT` default-ON.

**TDD.** RED: `5014`/`4198` minimal cores (ddmin the specific unsat class) as
`#[ignore]`-able fixtures + synthetic `∀+⊔` and `≤1+Disjoint` canaries. GREEN: the
pre-check marks them unsat. Corpus: `5014`/`4198`/`16321` misses → 0 (via P0's
unsat-normalized metric this shows as detections recovered).

**Soundness gate.** The pre-check derives ⊥ only from entailed clashes → sound by
construction. FP=0 corpus closure-diff; must be byte-identical on all
already-consistent fixtures (no spurious unsat). This is the highest-FP-risk step
(a wrong unsat = C ⊑ everything = many FPs) — adversarially test with
consistent-but-near-clash onts.

---

## P3 — Defined-class / ∀ sufficient-direction recognition (the 6.7k genuine misses)

**Root cause.** `X ⊑ C` where `C ≡` compound (`∃/⊓`, often `+∀/¬`). The existing
defined-SUB sweep misses compound/negation/∀ RHS; the hardest (`778`) are per-pair
**budget-bound** (`explain` >90 s under the 1000 ms per-pair cap + adaptive cut).
Two sub-causes, addressed separately:

- **P3a — sweep coverage.** Extend the defined-class recognition sweep to compound
  RHS with `∀`/`¬` conjuncts (the `15167` `EncompassedArea ≡ LargeArea ⊓ ¬Continent
  ⊓ ¬Sea` shape needs disjointness/¬ reasoning; the SWEET 28-cluster and `9534` are
  `∃/⊓` defined-class lattices). Investigate whether P1's fix already recovers the
  `∃/⊓` subset (re-measure after P1 — several of these may vanish for free).
- **P3b — budget for hard pairs.** For the small residue that is genuinely hard
  (`778` BioTop): these are SROIQ, correctly out of the saturator's complete
  fragment. Options: (i) a targeted higher per-pair budget for defined-class-RHS
  candidate pairs only; (ii) accept as documented `trust_sat` incompleteness. Prefer
  (ii) unless a cheap sound sweep closes them — do **not** raise the global budget
  (regresses wall on the whole corpus).

**Sequencing note.** Run P3 **after** P1 + re-measure — P1 may absorb the `∃/⊓`
defined-class subset, shrinking P3's real target. Scope P3a precisely against the
post-P1 residual before building it.

**Soundness gate.** Same FP=0 discipline; the sweep must be a sound sufficient-
direction check (entailed-only), like the existing defined-SUB sweep.

---

## P4 — SWRL / `DLSafeRule` parse gap (`10860`)

**Root cause.** rustdl's OFN parser errors on `DLSafeRule(...)` (a
`DataPropertyAtom`/`BuiltInAtom` arg form) → whole ont fails → r=0.

**Approach.** SWRL rules are outside OWL 2 DL reasoning scope; the sound behaviour is
to **parse-and-drop** `DLSafeRule` axioms (like other unsupported constructs) rather
than abort the whole file — a sound under-approximation (ignoring rules can only
lose entailments, never add). Fix the parser to skip/accept `DLSafeRule` and continue.

**Files.** `convert.rs` (horned-owl → IR) — treat `Rule` components as no-ops with a
one-line `# dropped: N SWRL rules` diagnostic.

**TDD.** RED: `10860` (or a 3-line `DLSafeRule` fixture) currently errors. GREEN:
classifies, returning the rule-free entailments (matches Konclude's class hierarchy,
which also doesn't use the rules for class subsumption here). Verify no FP.

---

## EXECUTION OUTCOME (2026-07-19)

**P1 — SHIPPED.** Root-caused to `lower_sub_class_of`'s `ConceptExpr::Some` branch:
`∃r.C ⊑ sup` only emitted triggers for `atomic_operands_on_right(sup)`, so a
compound-∃ consequent (`∃s.D`) produced no trigger and the GCI was dropped. Fixed by
mirroring the `And`-branch's Phase-2b.5 handling — allocate a two-way equivalent
marker `M ≡ ∃s.D` and emit `∃r.C ⊑ M` triggers (no env flag, consistent with the
sibling unflagged EL fixes ⊤⊑C / ∃R.⊤ / domain-GCI in the same function). TDD:
positive + negative-control unit tests (`existential_gci_compound_consequent_*`).
**Gate: 78 saturation tests green; workspace green (except a pre-existing missing
fixture `ontologies/regression/funcmerge-cyclic.ofn`, fails identically on
baseline); 5 curated SROIQ/EL oracles byte-identical (galen/notgalen/alehif/
ore-10908/ore-15672, FP=0/MISS=0); `2658`/`3406` recovered to exact Konclude match
(124447=124447, overshoot=0); broad ORE FP sweep 182+ onts TRUE-FP=0.** Restores the
"complete on EL" guarantee on `∃r.C ⊑ ∃s.D`.

**P2 — DOCUMENTED (do not build), per the discriminating probe.** rustdl's OWN
complete hypertableau does not derive these unsats: `5014` (`∀+⊔`) `explain` diverges
>90 s; `4198`/`16321` (`≤n+Disjoint`) classify with trust_sat OFF + unbounded still
reports 0 unsat. A sound pre-check that out-powers the hypertableau on disjunctive
unsat is not plausible to build (and is the highest-FP-risk item for 3 onts). This is
the same disjunctive-reasoning wall the project already retired (wine-wall / reuse-
trap / conflict-learning NO-GOs). Recorded as a known-limitation, not attempted.

**P3 — PARTIAL by P1 + DOCUMENTED residual.** P1 covers the `∃/⊓` defined-class
subset. The re-measured residual (`13647` 1281, `4911` 933, `16457` 636, canonicalized,
FP=0) is `∀`-into-defined-class recognition — the EL/ALC boundary (the saturator has
no complete `∀`-rule; the hybrid tableau handles it but trust_sat/budget skips it).
This is the retired architectural frontier; documented, not force-fixed.

**P4 — DOCUMENTED (external).** The `10860` failure is a horned-owl OFN *parser*
grammar bug (`expected DArg` on a SWRL `DataPropertyAtom`), not rustdl — which already
skips `Rule` axioms at conversion (`convert.rs:2068 C::Rule(_) => Ok(None)`). The
parse aborts before conversion. A robust fix belongs upstream in horned-owl; a loader
strip-and-retry for one ontology is too fragile to add. Documented.

**Net:** the one guarantee-violating bug (P1) is fixed and gated; P2–P4 are honestly
scoped — P2/P3 hit the project's already-measured disjunctive/`∀` architectural
frontier (not worth a risky new engine for the count), P4 is external. Genuine
reasoning-completeness recovered: 2,700 subs (P1) + the `∃/⊓` defined-class subset;
residual is the documented `trust_sat`/EL-boundary incompleteness.

## Cross-cutting

- **Order:** P0 (baseline) → P1 (ship, re-measure) → P2 → re-measure → P3 (scoped to
  post-P1/P2 residual) → P4.
- **Every engine step** ships behind a default-ON env gate (`=0` reverts), carries a
  negatives-first canary suite, and passes the full FP=0 corpus closure-diff + the
  ORE Konclude∩HermiT oracle on its target fixtures before merge — the standing
  soundness contract.
- **Re-measure after each** with the P0 de-artifacted harness so the genuine-miss
  number moves visibly (target: 9.4k → the irreducible SROIQ-hard residue).
- **Subagent-driven execution** per project preference: each P-step is a self-
  contained unit (spec already exists) suitable for a subagent with the TDD + gate
  checklist.
