# Incremental reasoning P1 — handoff

**Date:** 2026-08-20
**Branch:** `feat/incremental-reasoning-p1` (23 commits, parked — not merged)
**Base:** `feat/complex-class-expression-queries-48` @ `c1f44d8` (NOT main)
**State:** all 9 tasks implemented, each independently reviewed and fix-rounded clean; whole-branch
review returned MERGE WITH FIXES, fixes applied and re-reviewed clean. Workspace gate at the tip:
**1275 tests passing, clippy clean** (`--workspace --exclude owl-dl-py`).

Spec: `docs/superpowers/specs/2026-08-18-incremental-reasoning-design.md` (v2.1)
Plan: `docs/superpowers/plans/2026-08-19-incremental-reasoning-p1.md`

This file exists because the execution ledger and the nine task reports live under
`.superpowers/sdd/`, which is **gitignored** — the analysis below would otherwise survive only
until the next `git clean -fdx`.

## What shipped

| Layer | Capability |
|---|---|
| `owl-dl-core` | index-stable axiom tombstoning (`live` bitset); live-signature computation; `convert_delta` + derived-axiom overlay over a persisted pre-pass baseline (`user_axioms`) |
| `owl-dl-saturation` | reserved id headroom (`slack`) above the user vocabulary; always-on rule→axiom provenance (6 vectors); `SaturationState` that genuinely resumes across revisions |
| `owl-dl-reasoner` | `IncrementalSession` — addition-only, rebuild-on-delete, fail-closed staging, IRI-sorted reporting through the live signature, consistency-verdict retention |
| tests | budget-free identity gate + round-trip gate, 7 mutations verified to kill it |

## The result, stated plainly

**The machinery works. It delivered no speedup on any ontology available *at the time*; GO-basic
(fetched 2026-08-20) shows 2.14× — see the RESOLVED section below.**

- `apply` p50 on galen = **4.63 ms**, *below* the measured 5.8 ms lowering floor, 99/100 additions reused.
- `classify` p50 = **882 ms**, because reuse is gated on `is_pure_el`.
- **`closure_answered = 0` on all eight local ontologies** (galen, sio, ro, mie, sulo, paper5, pizza, family). All report `mode: hybrid`.
- A synthetic pure-EL control does fire the path: **1.69×** (median of 3 × 100 revisions).
- Exit criterion **FAILED**: 886 ms vs a ≤12 ms bar, 0.99× speedup.

Full analysis: `docs/2026-08-19-incremental-p1-latency.md`.
Floor measurement: `docs/2026-08-19-incremental-lowering-floor-findings.md`.

**The criterion was also mis-pointed.** `classify --saturation-only galen` misses four real
subsumptions the hybrid finds, so galen's complete classification is not computable from the EL
closure P1 retains — ≤12 ms was unreachable on galen *by construction*. But the FAIL still earned
its keep: it surfaced that `classify` is whole-ontology on every path except `is_pure_el`.

## RESOLVED: GO-basic measured — P1 is worth keeping

**Superseded 2026-08-20.** This section previously said P1's value was unmeasured. It has now been
measured: `docs/2026-08-20-go-basic-incremental-measurement.md`.

GO-basic (51,986 classes, 19× galen, ontology IRI `http://purl.obolibrary.org/obo/go.owl`, `.ofn`
sha256 `786e9e6913fb01a8…`) reports **pure EL with `tableau=0`**. So `closure_answered = 0` across
the eight local ontologies was a property of the **corpus**, not of the `is_pure_el` gate.

| metric | value |
|---|---|
| from-scratch classify | 549.49 ms |
| per-revision p50 | 256.85 ms (`apply` 177.37 + `classify` 79.40) |
| **`closure_answered`** | **101** — first non-zero on any real ontology |
| additions reused | 99 / 100 |
| **speedup vs from-scratch** | **2.14×** (KM publishes 4.90× for comparison) |

**And a correction to `docs/2026-08-19-incremental-lowering-floor-findings.md`:** the lowering
floor's share does **not** keep falling with size. 41.5 % (101 classes) → 7.6 % (2,748) → **34.6 %
(51,986)** — the trend inverts. The extrapolated **~13× galen ceiling must not be quoted as a target
at scale**; the real ceiling at GO scale is **~2.9×**, and the measured 1.99× against
saturation-only is ≈69 % of it.

Cause: `apply` re-lowers the whole union and re-runs all four derivation passes, both
O(|ontology|). On galen that is 4.63 ms against an 882 ms classify and vanishes; on GO-basic it is
177 ms against a 79 ms classify and **dominates**. The bottleneck has inverted, which also
invalidates the floor doc's advice to deprioritise incrementalising the derivation passes.

## P2 prerequisites (blocking)

1. **Tombstones are invisible to the reasoner.** Across all four crates there is exactly ONE
   non-doc consumer of `live_axioms` (`core/src/signature.rs`). `collect_el_rules`, `saturate` and
   `PreparedOntology` all read `internal.axioms` wholesale. So `kill_axiom` buys index stability,
   **not retraction** — the session achieves deletion by re-lowering the mirror through an
   id-preserving path. Pinned by `owl-dl-saturation/tests/rule_axiom_index.rs`
   (`a_tombstoned_axiom_still_compiles_its_rules`). **P2 must teach the engines to filter on `live`
   before a session can prune instead of re-lower.**
2. **Multi-source provenance.** Four rule shapes are jointly entailed by two or more axioms but
   carry one provenance slot each. Documented as `INVARIANT A CONSUMER MUST RESPECT` on `ElRules`,
   with five `// MULTI-SOURCE:` markers (`grep -n 'MULTI-SOURCE' crates/owl-dl-saturation/src/lib.rs`).
   **A deletion phase that assumes provenance is complete ships a false positive.**
3. **`NO_AXIOM` on Tseitin clauses means deletion retains them.** Argued inert; must be re-checked
   when `apply_delta` lands.
4. **The optimisation target has moved (new, 2026-08-20).** Closure reuse is already working; the
   dominant cost at scale is `apply`'s O(|ontology|) re-lowering plus the four derivation passes.
   Further speedup must come from making those sub-linear, not from the closure.
5. **P0 (edit-locality measurement) still un-run.** The spec defers P2's algorithm choice to it.
   `INITIAL_SLACK = 64` should be sized against its output. Demonstrably too small for a 52k-class
   ontology: it exhausts in 64 edits and costs a full rebuild (the 445.97 ms p-max on GO-basic).

## Pre-existing production defects found by this work

All three documented with reproducers; none caused by this branch.

- `docs/known-limitations/dkey-id-aliasing-classify-fp.md` — **false positives** in `classify()`.
  The highest-priority fix in this list: ~20 call sites plus a corpus FP=0 re-validation.
- `docs/known-limitations/top-down-classify-misses-equivalences.md` — the default path misses
  equivalence classes off-EL (13 vs 23 lines on `paper5.ofn`).
- `docs/known-limitations/galen-off-the-fast-path.md` — galen is off the fast path on this machine;
  `classify.rs:1252` cites a test that does not exist; `CLAUDE.md` contradicts itself eight lines
  apart. **Regression framing withdrawn** — the local galen has no ontology IRI, no versionIRI, and
  no entry in the fetch script, so cross-machine comparison is unsupportable.

Also measured, previously untracked: **`is_consistent` costs ~10 s/call on `paper5.ofn`** (release).

## Residual risks if this merges as-is

- **`paper5` has no consistency coverage in the identity gate by default** — the `RUSTDL_GATE_FULL=1`
  escape costs ~25 min, so treat the out-of-fragment consistency-retention path as untested-by-CI.
- **The mirror/baseline desync guard is a `debug_assert!`**, so release CI has no executable check on
  it. Its two `#[should_panic]` tests are now correctly `ignore`d in release.
- **The C1 fix costs latency:** an annotation-only or unrecognized-form edit no longer retains the
  consistency verdict. On an `OutOfFragment` ontology that is a ~10 s recompute per such edit.
  Slower, never wrong — **not yet reflected in the latency doc.**
- **Closing the C1 class rests on `Declaration` staying invisible to source-reading passes.** True
  today (`derive_data_axioms` and `derive_data_domain_unions` have no `Declaration` arm). If a
  future pass reads the mirror for declarations, `additions_are_inert` is the single line to
  re-audit — and it is not annotated in-source with that obligation.

## Follow-up items (triaged non-blocking by the whole-branch review)

- Aux-role IRIs are keyed on a *positional* axiom index (`convert.rs:2428`), so a deletion shifts
  them and the session degrades to rebuild-always. Key on chain content instead.
- `#[non_exhaustive]` on `SessionStats` and `SaturateConfig` before the incremental API ships.
- `push_live_axiom` is a baseline-bypassing back door — narrow its visibility or rename it.
- `ElRules` and four rule structs are now fully `pub` and not `#[doc(hidden)]`.
- `signature::mark_concept` has no visited set (latent, not live).
- Cosmetic: assertion messages at `incremental_identity_gate.rs:634/649/654/673` use `\`
  continuations without a trailing space, so words fuse in the output.

## Process note worth carrying forward

Eight tests specified in the plan turned out **vacuous** — green while proving nothing — and every
one was caught only by someone constructing and *running* a mutation, never by reading. Two were in
the identity gate itself. The practice that worked: for every load-bearing assertion, build the
mutation that should break it and show that it does; treat a test that cannot fail as a finding.
