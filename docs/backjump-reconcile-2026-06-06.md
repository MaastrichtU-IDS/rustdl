# Clean-start measurement: backjumping vs conflict structure on wine

2026-06-06. First measurement of the 1-UIP spike (PR #20). Reconciles the
contradiction between two earlier probes and **sharpens the spike's core question
— which changes whether the lever is cheap or multi-week.**

## Measured (clean baseline = main, no learning; temp counter, reverted)

`hyper-sat` on wine, 1 s/class:

```
d_in = 1 097 710    d_out (backjumps) = 0
clashes = 167 046   levels_mean = 2.73   spread_mean = 10.18   single_level = 24
```

- **Backjumping never fires** (`d_out = 0`) — confirmed (both earlier probes now
  agree).
- **Conflicts depend on only ~2.73 decision levels**, and those levels are spread
  **~10 apart** (deepest − 2nd-deepest).

## The reconciled puzzle → the spike's real question

A conflict depending on levels `{2, 12}` *appears* to leave the ~9 levels between
irrelevant — so backjumping *should* skip them — yet it never fires. Tracing
`clause_body_deps`: a derived label inherits a decision's level whenever its
derivation descends from that decision's asserted disjunct (via body-label deps +
successor `birth_deps`). So `d_out = 0` has **two readings with opposite
conclusions**:

- **(A) Backjumping is artificially blocked** — clashes carry levels they
  needn't (e.g. a coarse `birth_deps` that over-attributes). Then a *cheap* fix to
  dep-precision restores backjumping and may close wine **without 1-UIP at all**.
- **(B) `d_out = 0` is correct** — each disjunct genuinely enables the subtree
  that later clashes, so the clash truly depends on it; backjumping legitimately
  cannot fire. Then 1-UIP (asserting clauses) — or nothing — is the only lever.

This is the **first thing the spike must settle**, and it determines the whole
cost: (A) = a small dep-precision fix; (B) = the multi-week 1-UIP build; or stop.

## Next step (the actual spike entry point, revised)

Pick one stalled wine class (e.g. CabernetFranc), one recurring conflict, and
**trace its dep provenance**: for each level in `clash_deps`, which label/clause/
`birth_deps` put it there, and is that attribution *necessary* or *spurious*?
- Spurious attribution found → fix it (cheap), re-measure `d_out` and wine stalls.
- All attributions necessary → (B) holds; proceed to the 1-UIP build (PR #20 plan).

This supersedes "implement antecedent recording first": the dep-provenance trace
is cheaper and tells us whether antecedent recording / 1-UIP is even needed.

## Status

The 1-UIP undertaking (PR #20) stands, but its first milestone is now this
dep-provenance trace, not antecedent recording — because it may reveal a cheap
backjumping fix that makes the multi-week build unnecessary. Measure before
building, again.
