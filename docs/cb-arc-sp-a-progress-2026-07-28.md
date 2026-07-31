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
