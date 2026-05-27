# Full sound + complete hypertableau — scoping & roadmap

Drafted 2026-05-27. The standalone engine (H0–H3c + perf) is sound for
`Unsat`, corpus-complete on TBox subsumption, but its clausifier
*under-approximates* (drops cardinality in places, weakens nominals to
classes, no inverse-role propagation), so `Sat` is **unsound on the
full theory** — which is why it can't do negative refutation and can't
move the classify wall (see [`hypertableau-summary.md`](hypertableau-summary.md)
§3). This roadmap makes the engine **sound *and* complete for SROIQ**:
clauses entailment-equivalent to the ontology, a calculus that closes
the gaps, and a `Sat` verdict that is trustworthy. That unlocks
negative refutation → a complete classifier → the classify wall.

This is a **multi-month effort**, not an "implement next" turn. It is
scoped as forced-order phases, each with its own Konclude-agreement
gate that must hold *before* the phase is considered shipped.

## §0 — Why this is rustdl's job (the framing)

Konclude is faster and mature; HermiT exists. So why build it?
**Because rustdl's purpose is to *be* a native, sound, complete,
performant OWL 2 DL reasoner in Rust** — Konclude is the goalpost to
*match*, not to wrap. Shelling out to Konclude (process-out, parse the
hierarchy) would move the wall in days, but defeats the project's
reason for existing; it's noted here only as the explicit
non-goal/fallback. This is option (2) "integration matters" from the
capstone, and it is the project's raison d'être, not sunk-cost
momentum.

## §1 — Foundational decisions (commit before code)

- **Build *on* the current engine, don't rewrite.** The event
  worklist, `≤n` merge primitives, clause indexes, and branching/
  save-restore are validated, tested infrastructure. Each phase below
  touches well-defined parts. The H3c root-only-merge and anywhere-
  blocking scope cuts become explicit `// TODO(HF<n>)` markers, not
  architectural debt.
- **Match the published calculus rule-for-rule.** Soundness +
  completeness is claimed by *citing* Motik, Shearer & Horrocks 2009
  (*Hypertableau Reasoning for Description Logics*, JAIR 36) and
  demonstrating the implementation realises that calculus, **not** by
  constructing an independent proof. The safer claim; the proof
  obligation is "matches the reference rules," verified by the
  agreement gates.
- **Completeness corpus:** pizza (SHOIN), ro-stripped (SROIFV),
  sulo-stripped (SRI), sio-stripped (SRIQ), GO (EL, scale). ABox is a
  separate concern (§out-of-scope); `family-stripped` re-enters only
  if/when ABox lands.
- **Validation discipline (carried from this session):** each phase's
  Konclude-agreement gate is stated *before* the phase. If the gate
  doesn't hold, the phase didn't ship — diagnose, don't roll into the
  next phase. (The H3c `any_head_satisfied`/`AtMost` bug is the model:
  implement → validate catches the subtlety → clean fix.)

## §2 — Forced phase order

The order is forced by dependency, not preference: each phase rests on
the previous being sound.

### HF1 — Sound clausifier (no dropping, no weakening)

**Census (shipped):** `rustdl clause-stats` now prints a categorised
deferral breakdown (`deferred_census`). The HF1 target list across the
corpus:

| ontology | deferred | breakdown |
|---|---|---|
| sulo-stripped, go-basic | 0 | already at the gate |
| pizza | 7 | antecedent 6, head-cardinality 1 |
| ro-stripped | 7 | antecedent 2, head-cardinality 5 |
| family-stripped | 2 | antecedent 1, head-or-disjunct 1 |
| sio-stripped | 87 | antecedent 4, **head-cardinality 83** |

So HF1 is two categories: **head cardinality** (`≥n`/`≤n`/`Self` in a
consequent) and **antecedent** (`∀`/`¬`/cardinality on the GCI
sub-side).

**Head cardinality (shipped).** `emit_head` now clausifies
`Min`→`AtLeast` (new atom), `Max`→`AtMost`, `≥0`→trivial,
`ExactCardinality`→`Min⊓Max` (via `And`), and `∃R.Self`→a self-loop
`Role(x,x)` head; qualifiers name compound fillers. `AtLeast`/`Self`
heads are no-ops in the engine (`TODO(HF3)` — generation/self-edges),
which keeps `Unsat` sound (an unenforced head only weakens the theory);
`Max`→`AtMost` is *enforced* by the existing H3c merge. Census drop:
**SIO 87→4, ro 7→2, pizza 7→6**, head-cardinality bucket now 0
corpus-wide. Re-validated: SIO 1585 sat/0 unsat, pizza 695/695, **0
false positives** unchanged.

**`head_atom_for` naming + `¬∃R.Self` (shipped).** Compound head
disjuncts (`∀`/`≥n`/`≤n`/nested) are now *named* (`Q ⊑ disjunct`) and
`¬∃R.Self` becomes `Q ∧ R(x,x) → ⊥`, clearing the `head-or-disjunct`
bucket (family 2→1). Pizza/SIO agreement unchanged (0 FP).

**Antecedent absorption (shipped) — HF1 COMPLETE, `deferred == 0`
corpus-wide.** Internalizing a hard-antecedent GCI as `⊤ ⊑ ¬sub ⊔ sup`
is sound but the `⊤`-headed clause fires at *every* node and `¬∀ → ∃`
generates successors everywhere — **measured to explode (SIO 0.45 s →
did-not-finish)**. The fix is *partial absorption*: split the
antecedent conjuncts into **soft** (encodable as body atoms — a
trigger) and **hard**, emitting `soft ⊑ (⊔¬hard) ⊔ sup` — e.g.
`A ⊓ ∀R.C ⊑ D` → `A(x) → ∃R.¬C(x) ⊔ D(x)`, triggered by `A`, so it
*doesn't* fire everywhere. A purely-hard antecedent (no soft trigger —
a handful, e.g. ro's 2) falls back to `⊤`-internalization, which
doesn't explode at that scale.

Result: **`deferred == 0` on the whole corpus** (pizza/SIO/ro/sulo/
family/GO), engine still fast (SIO bare-sat 452 ms, ro 0.7 ms), and
agreement preserved — pizza 695/695, ro 158/158, sulo 51/51 (all
100 %, 0 FP), SIO 1585 sat / 0 unsat. HF1's gate is met: the clausifier
now produces clauses for every SROIQ construct in the corpus.

(Note: the engine's inverse-role/blocking handling is still HF2 — the
clauses are emitted soundly, but ro's inverse-role *reasoning*
soundness is not yet guaranteed by the calculus, only empirically
agreeing here.)


Every SROIQ construct produces clauses **entailment-equivalent** to
the source axiom. This is the foundation: until the clause set equals
the ontology (not a weakening), `Sat` agreement against Konclude is
meaningless. Mostly mechanical structural-transformation work, but
*weeks*, not an afternoon — it covers every antecedent/consequent
position of `∀`/`∃`/`⊔`/`¬`/`≥n`/`≤n`/`{a}`/`Self`, including the
nested shapes the current clausifier defers.
- **Nominal representation (the HF1↔HF4 hinge):** encode the singleton
  constraint, not nominal-as-class. `{a}` carries
  `∀x,y. {a}(x) ∧ {a}(y) → x = y` — represented via an `Equal` atom /
  a `≤1`-style constraint the HF4 NN-rule consumes. Pick the
  representation here and hold it.
- **Gate:** `clause-stats` **`deferred == 0`** on the entire
  completeness corpus. Do not pass HF1 until this holds everywhere.

### HF2 — Double-blocking + inverse-role propagation

- **Double-blocking, not pair-blocking.** Anywhere blocking (current)
  and pair-blocking (sound for SHIQ) are **unsound** once inverses
  interact with nominals. Go straight to the published double-blocking
  condition (Motik et al. §3.4) — no fragile intermediate.
- **Inverse-role propagation:** edges are matched both directions;
  `match_body`/`fire_exists` do inverse-aware lookups (the existing
  `RoleHierarchy` helps). `∀R⁻` / `∃R⁻` fire across the reverse edge.
- **Gate:** a small `R⁻` ontology derives the correct hierarchy under
  the new blocking; ro-stripped (SROIFV) classify matches Konclude
  (this is where the current engine + HermiT both fail/hang).

### HF3 — Cardinality fully in the calculus

Drop the H3c root-only scope cut. General `≤n` merge anywhere; `≥n`
generation creating `n` pairwise-**distinct** successors (explicit
inequality/`≠` tracking); qualified `≤n.C` / `≥n.C`.
- **Termination via `≤`-before-`≥` rule ordering** (Motik et al.): the
  `≥n` rule generates distinct successors, the `≤n` rule may re-merge;
  without the ordering the calculus loops. Termination is argued in
  the doc and pinned by a regression test (a cyclic `∃R.∃R…` + `≤n`
  ontology that terminates under the right order, loops under the
  wrong one).
- **Gate:** pizza `InterestingPizza` subsumptions correct **without**
  the H3c `¬sup` shortcut (i.e. via the real calculus); a qualified
  `≤n.C` test ontology matches Konclude.

### HF4 — Nominals as singletons (the NN-rule)

`{a}` is a true singleton. The NN-rule and nominal merging — where
nominals + cardinality interact (an `≥n` generation under a nominal
can force new nominals). This is the hardest interaction in SROIQ; the
"nominal merging is a fixpoint" obstacle from the capstone is now a
*calculus rule*, with the termination argument extended to cover
nominal-introduced successors.
- **Gate:** pizza `RealItalianPizza`/`hasValue` correct via the real
  nominal calculus (not nominal-as-atomic); a nominal+cardinality test
  ontology matches Konclude.

### HF5 — Wire as the complete classifier

`Sat` is now sound, so the orchestrator trusts **both** directions.
Replace `subsumes_via_tableau` with the new engine on the workloads
where it passes the *both-directions* agreement check; the H4 wedge's
`Unsat`-only restriction is lifted.
- **Gate (the payoff):** full **classify** Konclude agreement on the
  corpus, **both directions** — 0 false positives *and* 0 misses — and
  the classify wall moved (pizza/ro classify complete in reasonable
  time where they currently time out / hang).

## §3 — Out of scope (named, so they don't bloat)

Datatypes (`xsd:*`, the `owl-dl-datatypes` crate is separate); SWRL
rules; OWL 2 RL/QL/EL profile shortcuts; multi-threading; ABox/
consistency *as part of this roadmap* (the calculus extends to ABox,
but it is its own phase after HF5 if wanted — `family-stripped`'s
inconsistency is the re-entry test). Performance tuning beyond what's
needed to pass the gates (the perf arc already gives a fast Horn core).

## §4 — Risk & honest calendar

Each phase is weeks; HF4 (nominal+cardinality) is the deepest and
riskiest. The total is **months**. The mitigations are the forced
order (no building on unsound foundations) and the per-phase Konclude
gates (no phase ships unvalidated). If the calendar reality shifts the
§0 framing — e.g. a native complete reasoner stops being the priority
— revisit before committing to HF2+, since HF1 (sound clausifier) is
independently useful (it makes the existing `Unsat` accelerator cover
more) even if the calculus phases are deferred.

## §5 — Recommended entry point

**HF1, the sound clausifier.** It's the foundation everything rests
on, it's mostly mechanical (lower risk than the calculus phases), its
gate (`deferred == 0`) is crisp and measurable, and it delivers
standalone value (a more complete `Unsat` accelerator) even if HF2+
are deferred. Start there.
