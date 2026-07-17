# Saturator backward propagation (symmetric/inverse) — scoping (2026-07-17)

Phase A inc 2+ (`docs/2026-07-17-deficiency-roadmap.md`). This is a **scoping** document, not an
implementation design: it defines the minimal sub-increment, the measure-first go/no-go, and the
honest cost, so the build-vs-defer decision is made with full information. It exists because the
cheap "inert-symmetric gate" was refuted (see below) and backward propagation is the only sound
path to the {disjoint, symmetric} / inverse giant-Horn tail.

## Why this, and why it's hard

The saturator is a consequence-based engine complete on **EL forward** only — it has **no
backward/predecessor propagation**. Symmetric (R⁻ ≡ R) and inverse roles need it. The cheap gate —
"a symmetric role used only in forward `∃` is inert" — is **unsound**, proven by a 4-axiom
counterexample (advisor; confirmed by rustdl `explain`):

```
X ⊑ ∃R.C ;  ∃R.X ⊑ D ;  ∃R.D ⊑ E ;  Symmetric(R)   ⟹  X ⊑ E
```

Forward-only never builds the symmetric back-edge `c→x`, never fires the antecedent trigger
`∃R.X ⊑ D`, and misses `X ⊑ E`. A symmetric role is inert only when **write-only** (never an
antecedent `∃R` trigger, no domain) — which is the classification-irrelevant case. So there is no
cheap sound gate; the fix is real backward propagation (the CB **Pred rule**; Simančík et al. AIJ
2014, Bate et al. JAIR 2018, `[[sequoia-cb-sroiq-paper]]`).

## Target & tractability (measured)

The {disjoint, symmetric} tail is ~29–31 giant Horn onts (median 58k classes), all DNF on the
O(n²) per-pair path. One-pass **forward** saturation (symmetric ignored, incomplete) is the
tractability proxy: **4.2 s @ 41k classes (`ore_ont_10008`) but 58 s @ 12k classes
(`ore_ont_3914`)** — cost varies ~19× with per-class density, and the dense onts are already near
the edge *before* backward propagation adds facts. So: one-pass is tractable for most, marginal for
the dense few; backward propagation will add cost and its **termination is the crux** (back-edges
create cycles `X→witness→X`, requiring blocking/dedup on generated backward structure).

**No FP safety net on the target onts:** they DNF on the hybrid path and no oracle scales to
58k–981k classes. So soundness of any admission rests on a by-construction argument — with the
counterexample as the standing proof that the naive shortcut is wrong.

## Minimal sub-increment (the seed to build first)

**Symmetric-only back-edge materialization**, gated + flagged, in `owl-dl-saturation`:
- When `X ⊑ ∃R.C` is processed and R is symmetric, also record the back-fact that the C-witness has
  an R-edge to an X-instance (materialize `witness ⊑ ∃R.{X-marker}` / a predecessor fact), so the
  existing antecedent-`∃R` triggers and `process_unsat` fire on the witness's back-edge.
- **Termination:** the back-edges must be blocked/deduped (equality-blocking on witness labels, the
  CB analogue of double-blocking) or the fact set grows unboundedly on cyclic symmetric structure.
  This is the hard, soundness-and-termination-critical core.
- Symmetric-first (not general inverse) because R⁻ ≡ R avoids tracking a separate inverse role —
  the narrowest form of the Pred rule.

## Go/no-go gate (build the minimal sub-increment behind a flag, then measure — in order)

1. **Completeness proof-of-life:** the 4-axiom counterexample now yields `X ⊑ E` on the
   saturation path (the closure witnesses it, no tableau). If not, the back-edge rule is wrong.
2. **Termination + tractability at scale:** on `ore_ont_3914` (12k, the dense one) and
   `ore_ont_10008` (41k), the backward-prop saturation **terminates** and stays within a sane wall
   (target: same order as forward-only + a constant factor; if it blows up / doesn't terminate →
   **NO-GO**, blocking insufficient).
3. **Soundness — FP=0:** curated oracle net byte-identical/FP=0; a synthetic + the smallest real
   symmetric onts classify == the hybrid path (the only onts where hybrid completes as an oracle);
   by-construction argument on the giants (no scale oracle).
4. **Recovery:** the {disjoint, symmetric} onts that are otherwise in-fragment now classify one-pass
   (the actual payoff — ~29 onts).

**Lead with gate 2** (the advisor's point): if backward-prop doesn't terminate tractably at 12–41k
scale, the whole path is a NO-GO and the rest of the design is moot.

## Honest framing (per `[[frame-by-correctness-not-time]]`)

This is a **research-grade engine extension** — the genuinely hard part of lifting rustdl's EL-CB
engine toward SRIQ-CB, where Sequoia lives. It is a **multi-increment build** (symmetric back-edge
+ termination-blocking → general inverse → …), soundness-and-termination-critical, with **no FP net
on the target giants**. It should be entered as a **deliberate, focused effort with explicit cost
acceptance** — not tacked onto an omnibus session. The two shipped increments (anon coverage;
disjointness foundation) are the safe wins; this is the frontier.

**Recommendation:** treat this scoping as the entry artifact. Build the minimal sub-increment
(symmetric back-edge + blocking) behind a flag as a dedicated first step, gated on #2 (termination
at 12–41k scale) BEFORE investing in the general rule. If #2 fails, it's a documented NO-GO and the
giant symmetric tail is accepted as out of reach for the CB path (redirect to the anytime/
calibrated research thread, `[[paper-iswc-eswc-plan]]`).
