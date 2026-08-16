# Re-aim the CB arc at Horn-ELHI, not ALCH — spec

**Date:** 2026-08-16 · **Status:** ready to implement · **Branch to work on:**
`feat/cb-alch-taming` (holds the resurrected `owl-dl-cb`; `main` has only a 162-LOC stub).

**One line:** the DNF cluster that kills rustdl has **zero disjunction** and is blocked by
**inverse roles**; the parked CB engine tames disjunction. Point the existing
backward-propagation design at Horn-ELHI instead, where a published calculus with a
completeness proof already exists.

---

## 1. Why this is being reopened

`docs/superpowers/specs/2026-07-28-cb-lazy-successor-design-seed.md` set the reopen bar:
**exhibit an in-fragment ontology the hybrid cannot solve.** Two now exist, measured
2026-08-16 on v0.4.19 (single-thread, 120 s cap):

| ontology | classes | hybrid classify | `--saturation-only` |
|---|---|---|---|
| `ore_ont_11311` | 8,022 | **DNF at 120 s** | 1.1 s, 10,658 rows (incomplete — inverse dropped) |
| `ore_ont_9944` | 8,008 | **DNF at 120 s** | 0.9 s, 10,825 rows (incomplete — inverse dropped) |

Construct census of both files:

```
∀ = 0     ⊔ = 0     ¬ = 0     min/max/exact card = 0     nominals = 0
InverseObjectProperties = 3 / 7    Transitive = 3 / 3    SubObjectPropertyOf = 20 / 33
```

**They are pure Horn ELHI** — EL + role hierarchy + inverse + transitivity. No disjunction
anywhere. The ALCH engine's entire difficulty (ordered resolution, second-maximal
eligibility, the adversarial ∀-disjunctive blowup baseline) is **irrelevant to them**.

Corroborating scale: `docs/2026-08-16-dnf-tail-by-fragment-blocker.md` decomposes the
143-ontology DNF tail — **inverse blocks 71%** (102 of 143); ∀/cardinality blocks **1%**.

## 2. Why the current engine cannot be used as-is, and what IS reusable

`owl-dl-cb` (on the branch) is **ALCH**: no inverse. It cannot answer these ontologies at all.

**Reusable, and it is the valuable part:** the SP-A v2 seed
(`2026-07-28-cb-lazy-successor-design-seed.md`) specifies **backward role-clauses over a
generic successor variable + lazy successors** — the Pred rule — converging with
`2026-07-17-saturator-backward-propagation-scoping.md` (Sequoia contexts). **Backward
propagation is exactly and only what inverse roles need.** The mechanism is right; the
fragment it was aimed at is wrong.

In Horn-ELHI the job is strictly easier than the seed assumes: there is no disjunction, so
no ordered resolution, no eligibility, no branching. Each context carries a **conjunction**
of concepts and the calculus is a fixpoint — much closer to the existing EL saturator than
to a tableau.

## 3. Target calculus — DO NOT INVENT THE RULES

Use **Kazakov, "Consequence-Driven Reasoning for Horn SHIQ Ontologies", IJCAI 2009**. It is
consequence-based, handles **inverse roles and transitivity**, and ships **soundness and
completeness proofs**. Sequoia (Bate/Motik/Cuenca Grau/Simančík/Horrocks) is the SROIQ
generalisation and is the right reference for the context machinery, but full SROIQ is not
needed here and should not be attempted.

**This is the answer to the standing "do we need a proof?" question**
(`docs/2026-08-16-per-class-certification-refuted.md`): a per-class certifier for the EL
saturator would need a *new* proof, which is why three cheap approximations were refuted.
A published CB calculus **comes with the proof**. That is the single strongest argument for
this route over continuing to approximate.

**Obligation on the implementer:** derive every rule from the paper and cite it in a comment
at the rule site. Do not reconstruct rules from these notes — they are orientation, not a
specification of the calculus.

## 4. Scope

**In:** `SubClassOf` / `EquivalentClasses` over EL concepts (`⊤`, atomic, `⊓`, `∃r.C`),
`SubObjectPropertyOf` (incl. 2-step chains), `InverseObjectProperties`, `ObjectInverseOf` in
role position, `TransitiveObjectProperty`, `ObjectPropertyDomain`/`Range`, `⊥` and
`DisjointClasses`.

**Out (fall back to the existing hybrid, unchanged):** `⊔`, `∀`, `¬`, any cardinality,
nominals, datatypes, `ABox`. A file containing any of these is simply not eligible.

## 5. Integration

Mirror the shipped `RUSTDL_HORN_SHORTCIRCUIT` pattern, which is the closest precedent.

1. **`cb_eli_eligible(internal) -> bool`** — a strict allowlist over `InternalOntology`,
   modelled on `saturator_complete_fragment` in `crates/owl-dl-reasoner/src/classify.rs`.
   Allowlist, never denylist.
2. **Dispatch** in `classify_top_down_internal`, *after* the pure-EL fast path and *before*
   the hybrid path: if eligible and `RUSTDL_CB_ELI` is on, classify with the CB engine and
   return; else fall through untouched.
3. **Flag `RUSTDL_CB_ELI`, DEFAULT OFF.** Flag-off must be byte-identical to today.

## 6. Gates — all required

**G1 — completeness oracle.** On every eligible ontology, the CB closure must **equal**
Konclude's. `ore_ont_11311` and `ore_ont_9944` are the headline targets; Konclude's
`ore_ont_11311` taxonomy has 10,667 `SubClassOf`. Konclude ∪ HermiT adjudicates any
disagreement; where the two peers disagree, **exclude the ontology rather than picking a
side**.

**G2 — FP=0.** `./scripts/run-soundness-diff.sh`, flag ON. Grep `^[fp0]`.
**Do not treat a green net as evidence of completeness** — it is FP-shaped.

**G3 — flag-off byte-identity.** ≥10 curated fixtures, `classify` output identical with the
flag off. Strip `#` banner lines (they carry timings and a nondeterministic
`wedge-cost-histogram`); run an OFF-vs-OFF control first to learn what is nondeterministic.

**G4 — no `ok → dnf`.** Full 1,920-ontology ORE sweep, both arms, before any default flip.
The MISSED net **cannot** see this: its frame is drawn from completers.

**G5 — D10 audit.** *This is the gate this project most often fails.*
`memory/d10-bug-class-recipe.md`: six recorded instances of a fragment gate certifying
COMPLETE while the engine silently drops the axiom — a wrong answer carrying
`incomplete: false`. **For every construct `cb_eli_eligible` admits, grep the engine for the
rule that consumes it, and write a canary that fails if the rule is deleted.** Watch
recursion-reachable arms (a `Bot` match arm still misses `∃r.∃s.⊥`) and pass order.

## 7. Milestones

Each ends green with tests committed.

| # | deliverable | done when |
|---|---|---|
| 1 | `cb_eli_eligible` + canaries | accepts `11311`/`9944`; rejects ⊔/∀/card/nominal/ABox; each rejection has a test |
| 2 | ELH core (no inverse) — contexts, `⊓`, `∃`, role hierarchy, chains, `⊥` | closure == EL saturator on 5 pure-EL fixtures |
| 3 | **Inverse via the Pred rule** (the real work) | a hand-built ELI fixture where saturation MISSES and CB gets it — `tests/fixtures/eli/inverse-trigger-probe.ofn` already exists and is exactly this shape |
| 4 | Transitivity + domain/range | closure == Konclude on `11311` and `9944` |
| 5 | Dispatch behind `RUSTDL_CB_ELI` | G2 + G3 pass |
| 6 | ORE sweep | G4; default-flip decision |

**Stop at milestone 3 and report if the Pred rule does not terminate or does not close the
probe fixture.** That is the load-bearing risk; everything after it is mechanical.

## 8. Traps specific to this repo

- **Build with `RUSTUP_TOOLCHAIN=stable cargo …`**, or add cargo to the 1.95.0 toolchain.
  A skipped build silently reuses a stale binary. Freshness canary: `wine` classifies in
  ~2.7 s at defaults.
- **Pin a binary per configuration immediately after building it**, and verify the pin
  against a discriminating input. `memory/pin-binaries-per-configuration.md` — this cost two
  bad measurements, one a 2-hour scan of a sabotage build.
- **Sabotage your own guard tests.** Break the guarded code and confirm the test fails. In
  this project 3 of 4 sabotages have passed a test written to catch them.
- **`classify --pair-timeout-ms 1` is the addressability pre-check** for any per-pair-search
  lever. Not needed here — these ontologies stall in `label_cache_build`, not per-pair search
  — but do not skip it if scope widens.
- **The `sweeps` phase varies 1.4–2.4× run-to-run single-threaded.** One run per arm cannot
  establish a per-ontology wall claim.
- **`pkill -f` kills the agent's own shell.** Kill by PID.

## 9. What success is worth

`label_cache_build` on these two is 230.9 s and 599.9 s, and
`docs/2026-08-16-label-cache-reproduces-the-closure.md` shows **100% of the classes it
completes reproduce the saturation closure exactly**, with the rest of the wall producing
`NoVerdict`. A complete CB engine for this fragment does not need the label cache at all: it
answers the classification directly, in the ~1 s range saturation already demonstrates.

Addressable population: the inverse-blocked share of the DNF tail is **102 of 143 (71%)**.
Not all of those are Horn — the Horn subset is the target, and **sizing it is milestone 1's
by-product**: run `cb_eli_eligible` across the ORE corpus and report the count. If that count
is under ~20, say so and stop; the mechanism would be correct but the reward too small, and
this project has repeatedly found that **a shape census sizes a population but does not
predict a rescue**.
