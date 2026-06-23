# Incremental saturation on the structural engine (production classify)

`RUSTDL_INCREMENTAL_FIXPOINT` now also drives the legacy structural tableau
(`TableauContext` / `saturate`), which `PreparedOntology::decide` — the engine
the production `classify` orchestrator actually uses — runs on. (The same flag
already drives the `HyperEngine` wedge; see `repr-cache-increment2.md` /
increment 3.)

## What changed

`saturate()` called `mark_all_dirty()` on every entry — once per branch — so it
re-scanned the whole graph each frame (pizza: 60M node-scans / 3,187 distinct =
100% re-saturation, mirroring the HyperEngine wedge). Now:

- **`saturate()`** skips `mark_all_dirty` after the *initial* saturation
  (`mark_initial_saturate_done`) and drains only the deltas the per-mutation
  dirty hooks raised.
- **`add_label`** additionally dirties the node's **predecessors** — the
  back-propagation the per-mutation hooks lacked (qualified `≤n` counting a
  qualifier-bearing successor, the EL `R(x,y)∧C(y)→D(x)` shape, inverse-role
  propagation). This is the gap that broke the 2026-05-25 persistent-dirty
  attempt ("rules not re-firing on nodes affected indirectly by a merge").
- **`merge`** dirties reparented children (blocking-status change); its moved
  labels go through `add_label`, so target-predecessor back-prop is automatic.

Default OFF ⇒ legacy behaviour is byte-identical.

## Validation

- 128 tableau + 138 reasoner lib tests pass with it ON.
- The merge fixtures that killed the 2026-05-25 attempt — `48_max_merge…`,
  `65_functional_role_merges`, `66_inverse_functional_merges` — give identical
  verdicts ON vs OFF.
- Corpus classify output BYTE-IDENTICAL ON vs OFF: sulo/sio/mie/paper5/anatomy
  (sio = 1617 subsumptions).

## Result

End-to-end classify wall (best of 3):

| ontology | OFF | ON | speedup | vs Konclude |
|---|---|---|---|---|
| paper5 | 232 ms | **41 ms** | **5.70×** | now beats Konclude (82 ms) |
| sio | 386 ms | **243 ms** | **1.59×** | now beats Konclude (318 ms) |
| sulo | 4.9 ms | 3.8 ms | 1.26× | (already ahead) |
| anatomy / mie | ~2–8 ms | same | ~1× | EL closure, never hits the tableau |

Both paper5 and sio — cases rustdl previously *lost* to Konclude — now win.

## Pizza: still stalls (needs routing, not just speed)

Pizza classify is unchanged (706 vs 708 stalled pairs, same wall): incremental
makes each `saturate()` ~3× cheaper, but the structural engine's *search* for
those pairs is too large to complete within budget even so. The `HyperEngine`
wedge WITH increment-3 *does* finish pizza's worst pair (Stalled>2000ms → Sat
146ms). So the remaining pizza fix is routing — send hard residual pairs to the
(now fast) incremental wedge — not further speeding the structural saturation.

## Promotion

The change is sound (corpus-identical, merge fixtures preserved) and a pure win
on structural workloads. It is gated pending broader bench-corpus validation
before flipping the default on.
