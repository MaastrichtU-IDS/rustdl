# CB engine arc — SP-A progress record (restored)

**Why this file exists.** This is the SDD progress ledger for the consequence-based (CB) engine
arc, reconstructed and moved into git. The original lived at `.superpowers/sdd/progress.md`, which
is **gitignored** (`.superpowers/sdd/.gitignore: *`) and was **overwritten twice** during the
2026-07-30/31 session when that session ran its own SDD cycles. The content below is restored from
the session transcript; the commits it references are real and verifiable on
`feat/cb-alch-taming`.

**Hazard to avoid repeating:** `.superpowers/sdd/progress.md` is a single gitignored file that every
SDD run overwrites. Anything in it that matters beyond the current task must be copied into `docs/`.
Check whether it holds a *different* arc's ledger before starting a new one.

---

## Arc framing

Building a consequence-based engine as rustdl's **second engine**, following
Kobayashi-MaRust (KM) / Sequoia. Branch `feat/cb-alch-taming`, off `main` at `a0ce5ed`.

- Plan: `docs/superpowers/plans/2026-07-28-cb-alch-taming.md` *(on the branch, not main)*
- Spec: `docs/superpowers/specs/2026-07-28-cb-alch-second-maximal-taming-design.md` *(branch only)*
- SP-A v2 seed: `docs/superpowers/specs/2026-07-28-cb-lazy-successor-design-seed.md` *(branch only)*
- Commitment recorded at the time: full CB pursuit; **FP=0 absolute**; increments are the
  construction method.
- The prior June NO-GO was reconciled: the retired S1 engine lacked the taming, so the NO-GO did
  not rule out the taming approach.

## Task record

- **[x] Task 1 — un-stub `owl-dl-cb`.** Resurrected B1 + S1 from `feat/cb-b1-integration`. Builds,
  **92/92 tests green**, clippy clean. Entry points `classify_unordered` (B1 oracle) and
  `classify_sequoia` (S1). Commit follows `a0ce5ed`.
- **[x] Task 2 — adversarial ∀-disjunctive ALCH blowup baseline** (commit `2feb4bf`). N=13 blows up
  (>35 s), N=12 = 15.4 s, **~2.6× per step — a genuine 2ⁿ antichain**. Generator: n disjoint pairs
  + `C ⊑ ∃R.⊤` + `⊤ ⊑ ∀R.(Aᵢ ⊔ Bᵢ)`. The correct answer is `C ⊑ ⊥`, which is clash-prunable, so the
  fixture is self-checking. `agreement_on_tiny` confirms valid ALCH (B1 ≡ S1 at n=2). 93 tests green.
- **[x] Task 3 — Candidate-1 second-maximal eligibility** (commit `4aa1039`,
  `RUSTDL_CB_SECOND_MAXIMAL`, **default off**). DONE_WITH_CONCERNS. **Preserves B1 parity**
  (FP=0 / MISSED=0, non-vacuous RAII-guarded test) **but UNDER-TAMES — about 3× WORSE**: flag-ON
  n=12 takes >20 s versus 2.4 s flag-OFF. Cause: second-maximal is a *superset* of eligible
  resolutions, so it grows the `∏|supports|` cross-product. Flag-off path byte-identical (93 tests).
  **Empirical conclusion: eligibility-relaxation is the WRONG lever.**
- **[x] Task 4 — DECISION.** Candidate 1 is out (under-tames) → move to **Candidate 2: KM cap +
  splitting.** KM's `branch_ordered` *suppresses* ≥2-multi-head-premise resolvents (shrinking the
  cross-product), and splitting recovers completeness by branching a disjunctive premise down to a
  unit, which enables clash-pruning — on the adversarial fixture `C ⊑ ⊥` clashes after 2 picks, so
  polynomial rather than 2ⁿ. This is the mechanism measured to work in KM (70–310×). Note this is a
  **big** change: it adds clash-driven branching to the ordered CB engine.

## Status as of 2026-07-31: PARKED, not abandoned

Parked by an explicit redirect during the 2026-07-30 session ("spec the atomic-negation lever, park
CB"), recorded in branch commit `4019a94` *"spec: conjunctive-unsat + RHS-negation canonicalization;
park the CB arc"*. The redirect was productive — the atomic-negation lever it produced became
`RUSTDL_NEG_TO_BOT_GCI`, shipped in **v0.4.6**, flipping 13 ORE ontologies to pure-EL (5 of them
previously DNF).

**Where things stand:**

- `crates/owl-dl-cb` **is on `main`** (the crate survived), but **every 2026-07-28 CB doc and the
  Candidate-1 commit are only on `feat/cb-alch-taming`** (`be5314f`, `2650001`, `4aa1039`,
  `4019a94`, `2ede386` — none is an ancestor of `main`).
- The branch is backed up: `refs/archive/heads/feat/cb-alch-taming` at `2ede386`. It does **not**
  appear in GitHub's branch list — see the branch-backup section of CLAUDE.md.
- CLAUDE.md (line ~112) lists `feat/cb-alch-taming` as **deliberately parked, not stale**, with its
  park record in its own tree. Do not prune it.

**To resume:** check out the branch, read the SP-A v2 seed spec plus
`2650001` (KM vs rustdl classify decision-procedure analysis + runtime profile), and start at
Candidate 2 (KM cap + splitting). Do **not** re-attempt Candidate-1 second-maximal eligibility — it
was measured at ~3× worse and the negative result is the reason Candidate 2 was chosen.

---

## CLOSED ON EVIDENCE — 2026-07-31 (v0.4.6)

**The arc's own GO criterion now has zero candidates.** The park record specified the resumption
test as: run the existing B1/S1 over the in-fragment slow/DNF ORE set, requiring *"completes at least
one ontology the shipped path DNFs"*. That set was
`12012, 10016, 10032, 2397, 9318, 15703, 6212, 3524`.

Re-measured on v0.4.6 — **all eight complete on the shipped hybrid:**

| ontology | v0.4.6 | mode | note |
|---|---|---|---|
| `ore_ont_9318` | 0.89 s | pure EL | |
| `ore_ont_2397` | 1.18 s | pure EL | was DNF at 150 s pre-`NEG_TO_BOT_GCI` |
| `ore_ont_10032` | 2.19 s | pure EL | was DNF |
| `ore_ont_10016` | 3.83 s | pure EL | was DNF |
| `ore_ont_6212` | 17.65 s | pure EL | was DNF |
| **`ore_ont_12012`** | **60.45 s** | hybrid | **the only member with genuine ALC content** (48,103 rows) |
| `ore_ont_15703` | >120 s, <400 s | pure EL | exit 0, 266,832 rows |
| `ore_ont_3524` | >120 s, <400 s | pure EL | exit 0, 266,832 rows |

(`15703`/`3524` were initially misread as complete-at-120 s from truncated streamed output; exit codes
at a 400 s budget settle it. They are slow, not DNF — and both are pure-EL, so an ALCH CB engine is
not the relevant tool for them regardless.)

The park record's slow set went the same way: `33, 6870, 7275, 7726, 11906, 16299` all flipped to
pure-EL in v0.4.6.

**The mechanism is ironic and worth stating plainly:** most of these were recovered by
`RUSTDL_NEG_TO_BOT_GCI` — the atomic-negation lever that exists *because* the CB arc was parked in
favour of it. The redirect that paused this arc is what removed its market.

**Market summary.** The park record measured 8 of 289 ORE DNF ontologies (2.8%) as in-fragment, with
only `12012` carrying genuine ALC content. That is now **0 of 289**. The two ontologies where KM beats
rustdl 70–310× (`ore_ont_9053`, `ore_ont_10197`) remain rejected by `normalize.rs` on three counts
each (ABox, datatypes, transitive/inverse), so the engine still cannot load the cases that motivate
it. Nothing was found that a CB engine would win.

**Status: CLOSED, on payoff-vs-cost, with numbers** — the stop the park record explicitly admitted
("payoff-vs-cost must remain an admissible stop; 'only a demonstrated genuine impossibility counts' is
an unfalsifiability clause"). Not closed on fatigue, and not blocked on any unresolved technical
question.

**What to keep regardless — the durable technical findings:**

1. **The 2ⁿ is in the CONTEXT space, not the clause space.** `RUSTDL_CB_DEBUG=1` context counts:
   n=9 → 531, n=10 → 1045, n=11 → 2071, n=12 → 4121, at ~25 clauses per context throughout.
2. **Why the engine has no choice.** `DerivedClause.premise` is *always empty* (`model.rs:86-91`,
   "core held implicitly") and `intern_context` seeds every core atom as an unconditional unit
   (`engine.rs:91-114`). The core *is* the hypothesis set, so a hypothesis can only be recorded in
   the context key — and hypotheses in the key make the key space the powerset of the definers.
3. **The reviewed formulation, if it ever matters: "Stage 1′"** — batch *all* ∀s (conditional
   included) into one successor core and add a `used`-provenance field that **only back-prop may
   read**. Hypotheses stay in the core, so the `⊓core` invariant stays correct by construction: one
   FP surface instead of thirteen, and a provenance bug degrades to a MISS rather than an FP. Pair
   with a provenance-size cap degrading to an `ALL` sentinel (the `RUSTDL_PRECISE_CARD_DEPS`
   discipline). Note `seq_model.rs::SeqClause` has no premise field, so this is **B1-only** as
   designed.
4. Measured negatives not to repeat: Candidate-1 second-maximal eligibility (~3× *worse*); the
   seed's "backward propagation over a generic successor variable" mechanism claim (**unconfirmed**
   against KM's source); "B1 and S1 both hang >30 s" (a debug-build artefact — in release S1 n=13 is
   6.4 s and B1 is the worse engine).

**To reopen, the burden is now:** exhibit an ontology inside `owl-dl-cb`'s fragment that the shipped
hybrid does not solve. As of v0.4.6 no such ontology is known in the ORE corpus.
