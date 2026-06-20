# Session report — 2026-06-20

Engineering session on `rustdl` (sound OWL 2 DL/SROIQ reasoner). Method throughout:
**measure-first** (every lever gated on a cheap measurement before building),
**FP=0 is sacred** (no false-positive subsumptions; corpus closure-diff vs the
Konclude∩HermiT oracle is the gate), **branch + spike + validate before merge.**

---

## 1. What shipped to `origin/main`

Seven landed changes (all FP=0-validated, pushed through `a34ae67`):

| Area | Change | Result |
|---|---|---|
| **Correctness** | **ABox-saturation consistency pre-check** (`RUSTDL_ABOX_SATURATION`, default-ON) — consequence-based fixpoint over named individuals | **family inconsistency closed** (1.6s; the last open correctness gap), FP=0/MISSED=0 corpus-wide |
| **Soundness** | **FP fix: disjunctive `∃R.(A⊔B)` marker corruption** — union markers reused the singleton `∃R.A` marker | latent **FP=0 violation closed** (`ore_ont_7499` FP 1060→0) |
| **Completeness** | **domain-as-GCI** (`∃R.⊤ ⊑ C` routed to `role_domains`) | SWEET 13621/14450 → full parity (652 pairs) |
| **Completeness** | **ObjectHasSelf → domain/range** + **disjunctive object-property domain** + **`minimal_common_subsumers` equiv-cycle fix** | ore_ont_4827 MISSED 461→79 |
| **Perf** | **saturation-only oracle for inconsistency justification** | `justify inconsistent` 488s → ~64s (7.6×), variance eliminated |

Cumulative on the ORE-2015 pilot: **C2 silent-MISSED 1315 → 273 pairs (1042 recovered, ~80%), FP_strict=0 across all 197 diffed ontologies, zero regressions.**

Two earlier-session wins also on main: the **family pre-check** (above) and the
**justify** speedup.

---

## 2. Engine state (end of session)

- **Sound at scale:** FP=0 on the curated corpus **and** across all 197 diffed
  ORE-2015 pilot ontologies (including 11 newly-diffed onts that DNF'd before and
  hid the 7499 FP). The masked-FP risk is retired.
- **Correctness:** the family inconsistency gap is closed.
- **Completeness:** ~80% of the ORE completeness frontier recovered this session.
- **EL perf:** ~1.15× Konclude (galen 0.31s); SROIQ outliers fixed (ore-15672 via
  the earlier blocked-⊔ fix) except wine (combinatorial, NO-GO'd).

---

## 3. The ORE-2015 sweep arc (the session's spine)

The curated corpus was "solved" (FP=0/MISSED=0), so cheap levers on it came back
zero. Pointing the engine at the **ORE-2015 pilot** (233 onts, cached
Konclude∩HermiT oracle) surfaced real, sound engine gaps the curated corpus never
exercised — **one FP and four completeness fixes** (table §1). Each was found by the
same loop: diff vs oracle → minimal repro → root-cause → sound fix → FP=0 gate
(corpus + ORE pilot). The FP (7499) had been hidden for the whole project because
that ontology DNF'd and was never diffed.

---

## 4. The DL tail — fully investigated, no viable architectural shortcut

After the completeness fixes, **273 silent-MISSED pairs** remain (16 onts). These
were exhaustively characterized:

**Bottleneck = branch-wide disjunctive explosion** (e.g. 394: 66k branches, 7339:
30k, 778: 36k), with **76% of branches non-clashing ∃-generation descents to the
depth-256 cap** — model-building depth, not disjunct redundancy.

Every cheap, FP-free lever was **measured out** (not generalized — tested across the
ORE branch-wide set):

| Lever | Verdict |
|---|---|
| L1 — scalar max-branching-tag deps | dead (P0: wine bjgap genuine) |
| L2 — disjunction-priority ordering | **inert** (778: 35933→35919) |
| SP1 — semantic branching | **structurally inert** (394/7339/12698 have *zero* complements; 778's aren't disjunct-complements) |
| Anywhere/double blocking | **already on** in the wedge; explosion persists |
| 1-UIP conflict-learning | viable on the merge-free fragment but **ROI-NO-GO** (59 pairs in ONE ontology; bulk is wine-class merge fragment) |
| Within-search caching | 0% headroom (Konclude-confirmed) |

**Konclude does these in 1–220ms** (whole-ontology classification) — so the gap is
real, but it is **architectural/engineering-maturity, not a missing technique**:

- The **whole-ontology consequence-based engine** (`owl-dl-cb`, B1/B2 + ordered
  Sequoia S1) was **built, benchmarked against Konclude, and retired**
  (`docs/superpowers/specs/2026-06-16-cb-konclude-investigation-verdict.md`):
  CB-saturation is **fundamentally worse on ∀-rich disjunction** (combinatorial
  cross-product over an antichain of incomparable disjunctions, intrinsic; ordering
  prunes only ~20%). Verdict: CONSOLIDATE — do not build it out to SROIQ.
- Konclude's speed comes from a **mature clash-driven tableau** (clash→branch-unwind
  + dependency backjumping + common-disjunct extraction) — which rustdl **already
  has** in the wedge; the gap is 15 years of tableau optimization, not architecture.

**Conclusion:** "whole-ontology clash-driven classification" splits into (a) the
pure whole-ontology/CB approach — built and retired as fundamentally worse on the
exact disjunctive input we want to close; and (b) the clash-driven tableau — which
rustdl is, just less optimized. There is **no architectural rewrite that helps the
DL tail**; the residual is the engineering-maturity parity bet to Konclude, with no
cheap FP-free entry remaining.

---

## 5. Status of branches / open items

- **`origin/main` (`a34ae67`)** — all production work; sound, validated.
- **local `main`** is one docs commit ahead (`6523d6a`, the 1-UIP scoping spec) —
  not yet pushed.
- **`feat/sp1-ore-dl`** — SP1 cherry-picked onto main; measured inert on the ORE
  branch-wide set; **do not merge.**
- **`feat/abox-sat-A-gated`** — kept as the record for a future inverse-aware
  bake-off (the consistency-side inverse work).
- **`owl-dl-cb`** — retired CB engine; durable role is EL/easy-ALCH accelerator +
  differential oracle.

**Recommendation: bank.** The engine is sound, the family gap is closed, completeness
is ~80% up on the ORE frontier, and the remaining DL tail has been investigated to a
definitive verdict — no viable architectural shortcut; the only path is the
multi-year tableau-maturity parity bet, which warrants its own explicit
cost-accepted decision, not a tail-of-session build.

---

## 6. Specs / records written this session
- `docs/superpowers/specs/2026-06-20-abox-saturation-consistency-design.md`
- `docs/superpowers/specs/2026-06-20-1uip-cdcl-nonnominal-dl-design.md` (scoping → ROI-NO-GO)
- this report; memory updated throughout (`ore-revalidation-2026-06-20`,
  `saturator-fp-disjunction-existential-marker`, `inverse-aware-classification-no-win`,
  `justify-inconsistency-perf`, `conflict-learning-simple-is-weak`).
