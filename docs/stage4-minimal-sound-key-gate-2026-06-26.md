# Stage-4 — minimal-sound-key gate (engine go/no-go)

**Status:** verdict (durable). The gating measurement for the deep-engine program. It
decides whether the proven Gamay collapse (451k→43 branches) can be made **sound** by a
sparse re-keying — i.e. whether a Konclude-style "sound completion-graph reuse" engine has a
viable core in rustdl's calculus, or whether the only path is a different nominal calculus
(don't generate the branches).

## Why this gate

Part-4 of the engine characterization proved the *payoff* (label-keyed Unsat memo collapses
`sat(Gamay)` 451175 branches/30s/Stalled → 43 branches/3ms) but returned a **spurious Unsat**
(Gamay is satisfiable). The open question — which defines what the engine *is* — was whether
that unsoundness is fixable by a **sparse** key (capture a little more local context → sound
*and* still collapsed) or whether soundness requires near-full context (→ no reuse, dense).

The advisor reframed the design step as this measurement: *for the false positives, what is the
minimal key extension that makes the verdict sound while preserving the collapse?* This is
distinct from SP-0 (which measured backjump dep density): the reuse key is "what must match for
the Unsat to *recur*," a potential subset of the backjump deps.

## Method

Extended the throwaway UNSAT-memo probe (`hyper.rs::solve`) with a leveled key
(`memo_node_key`, env `RUSTDL_MEMO_KEY`), monotone in information:

- **0** — own labels only (the part-4 label-keyed memo)
- **1** — + own outgoing edge-roles `(role-id, inverse)`, no neighbour labels
- **2** — + immediate-successor labels & their edge-roles (1 hop)
- **3** — + 2-hop successor structure
- **9** — full reachable **descendant subtree** (cycle-guarded) — models Konclude-style
  sub-completion-graph reuse
- **99** — whole **completion-graph** fingerprint (every node's labels + edges; captures
  ancestor/sibling context) — must be sound; tests whether identical global states recur

Driver: `stage4_minkey_gamay` (ignored), one fresh 2 GB-stack thread per level (per-thread memo
resets), `RUSTDL_UNSAT_MEMO=1`, `RUSTDL_ADAPTIVE_BUDGET=0`, depth 256, 30 s budget. Ground
truth = **Sat** (wine ∃-seed classification is MISSED=0/FP=0; Gamay satisfiable there).

## Result

| `RUSTDL_MEMO_KEY` | verdict | branches | wall | sound? |
|---|---|---|---|---|
| 0 labels | Unsat | 43 | 3 ms | ❌ reuse-trap |
| 1 +edge-roles | Unsat | 43 | 3 ms | ❌ |
| 2 +succ labels (1-hop) | Unsat | 1255 | 76 ms | ❌ |
| 3 +2-hop | Unsat | 1255 | 76 ms | ❌ |
| 9 full descendant subtree | Unsat | 1255 | 75 ms | ❌ |
| 99 whole completion-graph | **Stalled** | **431591** | 30000 ms | ✅ (no reuse) |

## Verdict — DENSE / NO-GO for a sound-reuse engine

**There is no sparse sound re-keying for Gamay.** Two facts pin it:

1. **Every descendant-local key is unsound** — even level 9 (the *full reachable subtree*, which
   is exactly what Konclude-style sub-completion-graph reuse would key on) returns spurious
   Unsat. So the verdict is **not** determined by the node's subtree; the unsoundness comes from
   the **ancestor / nominal-merge context above** the node. Enriching the descendant key only
   reduced spurious memo hits (43→1255 branches), never restored soundness.
2. **The only sound key gives no reuse** — keying on the whole graph (which captures the
   ancestor context) is sound, but identical global states essentially never recur, so the memo
   collapses nothing: 431591 branches/Stalled, indistinguishable from the 451175 no-memo
   baseline.

The 43–1255-branch collapse is therefore **irreducibly tied to unsoundness** in rustdl's
calculus — it exists *only* because unsound keys conflate distinct ancestor contexts. Sound
state reuse cannot deliver the wine speedup here.

This is the empirical confirmation of `wine-wall-bjgap1-genuine.md`: *Konclude is fast because
its nominal architecture does not create the dense ancestor-dependent dependency chains, not
because it caches.* `merge_with_cause` folds nominal-merge causation into every downstream
node's `birth_deps`, so a node's satisfiability depends on the full ancestor context — defeating
backjumping (SP-0, bjgap≈1), sound caching, and lemma-learning alike.

## Implication for the engine program

The engine I was about to design — a *sparse-dependency representation enabling sound
completion-graph reuse* — targets a lever that **does not exist** in this calculus. There is no
sound reuse to exploit. The only path to ms on wine is a **different nominal calculus** that
does not generate the branches in the first place (Konclude's integrated nominal/merge
handling), a from-scratch reimplementation of nominal reasoning with **no incremental
sound-reuse entry point**.

Honest framing of that target: a **wall-only** win (MISSED=0 — correctness is already perfect),
on **one fixture** (wine), against an engine that already solves it in ~114 ms. Every
reuse/caching attempt in this codebase has been FP-unsound (A1 FP=100, snapshot cache
default-OFF), and this gate shows why structurally. Whether to commit a from-scratch nominal
calculus to that is a pure-value call, not a technical-viability one — the technical viability
of the *cheap* path is now closed: NO-GO.

## Scope / provenance

- Branch `feat/stage4-engine-characterization` (off the ∃-seed merge `ee6904c`). `main`
  untouched. The leveled key + `stage4_minkey_gamay` are throwaway probes (gated, `#[ignore]`d),
  alongside the part-1–4 probes (`RUSTDL_TREE_LOG`/`REVISIT_PROBE`/`UNSAT_MEMO`/`MEMO_KEY`).
- Measurement integrity: levels show distinct behaviour (43 / 1255 / 431591), so the binary is
  live and the knob works — not the stale-binary artifact that bit the Phase-2 wall measurement.
