# Deep nominal rearch — SP-0: shadow precise-dependency probe (design, 2026-06-25)

**Status:** design / measurement gate. SP-0 of the committed deep nominal
rearchitecture. Read-only; decides which mechanism the rearch builds.

## Why this exists (the committed rearch, and the one premise it rests on)

The user committed to the **deep nominal rearchitecture**: replace the wedge's
imprecise nominal-merge dependency tracking with Konclude's per-fact dependency-node
graph (CMERGED*), so that **precise backjumping + sound UNSAT-caching + CDCL
lemma-learning + sound model-reuse** all become effective at once. The thesis: wine's
wall is **dense clash-dependency chains** — `merge_with_cause` over-folds merge
causation into `birth_deps`, and the `≤n`/NN merges drop causation entirely
(`at_most_tainted`/`nn_tainted` → `card_clash_deps` returns `DepSet::ALL`), so every
clash depends on the full ancestor context (bjgap≈1). That single shared root defeats
the whole suite together.

Committing to the rearch means building the **right** mechanism. The load-bearing
premise has never been measured directly:

> Are wine's dense clash-dependency chains an **artifact** of imprecise tracking
> (precise per-fact deps make them sparse → bjgap grows, nogoods become reusable →
> the rearch pays off), or the **genuine** semantic dependency structure (nominal
> identity really does depend on the branch context → precise deps track the *same*
> dense chains, just precisely → the suite still fails)?

Everything downstream works iff *artifact*, fails iff *genuine*. Wine's existing data
("of 451k clashes, ZERO context-independent"; "66–82% of ⊔ nondeterministic after
complete Horn propagation") *leans genuine* — but it was measured under the **current
imprecise** tracking, so it is exactly the number that could be an artifact. SP-0
remeasures it under **precise** deps, read-only, before any build. Building the wrong
mechanism twice (cf. precise-merge-deps FP=232; per-fact-dep-graph 1/77, which
measured one consumer — ≤n backjump alone — of the foundation) is the failure mode
this gate prevents.

The "~14 states ×~40k revisits" datum is from ORE-15672's *e-interaction*, **not
wine** (and ore-15672 is already handled by the adaptive-budget early-cut) — it does
not transfer. SP-0 measures wine directly.

## What SP-0 builds: a read-only shadow precise-dependency layer

A **shadow** dependency layer computed alongside the real wedge search but **never
consulted for any search decision or verdict** — so search behavior, soundness, and
termination are byte-identical to flag-off. Only the recorded numbers differ.

The shadow mirrors the three imprecision sites, computing what a precise per-fact
graph *would* track:

- **Merge causation (the core).** Where the real engine sets `at_most_tainted` /
  `nn_tainted` (and `card_clash_deps`/merge then return `DepSet::ALL`), the shadow
  instead records the **actual decision level(s) that caused the merge** and unions
  that precise causation into the shadow deps of the merged labels — the precise
  causation the CMERGED* graph is designed to carry. (`u128` decision-level `DepSet`;
  wine's `max_branch_depth≈30` ≪ 128, so no overflow → the precise set is exact, never
  forced to `ALL`.)
- **Per-label shadow deps.** A shadow `DepSet` per `(node, label)` mirroring
  `label_deps`, propagated through derivations using the precise merge causation above
  instead of the taint fallback.
- **Birth deps.** Shadow `birth_deps` carrying only the precise causation of the
  node's creation, not the conservative ancestor fold.

At every clash the wedge detects, record a pair: `(clash_deps_real,
clash_deps_shadow)` — the real (taint→`ALL`) set and the shadow-precise set.

Implementation note: the shadow layer is exactly the data the CMERGED* rearch will
build. On GO it is the foundation, not throwaway. On NO-GO it is the proof. It is
gated behind `RUSTDL_SHADOW_DEP_PROBE` (default OFF); flag-off is byte-identical.

## The three measures (one instrumentation pass captures all)

Run on wine's **hard classes** — at minimum `sat(SweetWine)` and
`sat(AlsatianWine ⊓ ¬AmericanWine)`, plus a handful more of the ~19 hard classes for
distribution — with adaptive budget OFF, big-stack thread, depth 256.

1. **bjgap distribution, real vs shadow.** For each clash, `bjgap = current_branch_
   depth − highest_decision_level_in(clash_deps)` (levels the backjump would skip; 1 =
   useless). Report the distribution of `bjgap_real` and `bjgap_shadow`.
   - `bjgap_shadow` stays pinned at ≈1 → chains are **genuine** → the per-fact-dep
     graph is the wrong mechanism (NO-GO), proven without building it.
   - `bjgap_shadow` shifts off 1 (a regime change — e.g. median ≥ 3, or a substantial
     fraction with `bjgap_shadow ≥ 5`) → **artifact** → backjumping would fire → GO
     signal.
2. **Nogood reusability under precise deps.** Express each clash's `clash_deps_shadow`
   as a **context-independent nogood** — the set of node-label facts (not the path)
   that are jointly inconsistent. Count, of the clashes encountered, how many distinct
   reusable nogoods recur across ≥2 different branch paths. Current baseline (imprecise)
   is ~0 of 451k. A substantial reusable fraction → caching/CDCL would fire (GO);
   ~0 → they cannot, regardless of how clean the graph is (NO-GO for the caching/CDCL
   consumers).
3. **Revisited-state context-sharing (resolves the mechanism fork + the FP-trap).**
   Hash node label-sets; count revisits on wine. For revisited states, record whether
   they share the same in-scope nominal merges (**cacheable** → state-memoization is
   viable) or differ (**the reuse-trap** → state/unsat caching on nominals is the
   snapshot-cache FP, off-limits). This decides path-dep-pruning (CMERGED* backjump/
   CDCL) vs semantic-state-memoization as the foundation, and flags the FP risk if
   memoization looks tempting.

## Decisive outcomes

- **GO** — `bjgap_shadow` regime-changes off 1 **and** a non-trivial reusable-nogood
  fraction appears. → Build the CMERGED* per-fact dependency-node graph as the
  de-risked foundation (the shadow layer becomes real), then layer the integrated
  suite (all-channel precise backjump → sound caching → CDCL → model-reuse) as
  successive sub-projects. Measure 3 also names which foundation (path-dep vs
  state-memo) and whether memoization is FP-safe.
- **NO-GO** — `bjgap_shadow` stays ≈1 / reusability ≈0. → A **proven mechanism-floor**:
  wine's nondeterminism is semantic, the per-fact-dep-graph is the wrong mechanism.
  This is not "floor"; it is the specific, evidenced input for the user's remaining
  choice — committing to Konclude's *fully-integrated* engine reimplemented from
  scratch (where the disjunctive search itself is the target, not the dep tracking),
  or stopping the wine chase. The verdict doc presents that fork explicitly.

## Soundness / risk

- **Zero FP/termination risk:** the shadow layer is never consulted for a decision or
  verdict. Flag-off byte-identical (verified by the existing wedge suite + a wine
  closure spot-check). The only cost is wall (slower wine search under the flag) — and
  wall is not the gate.
- This is the 7th wine check but the cheapest: read-only instrumentation, no behavior
  change, and its NO-GO branch *discharges* the "must reach Konclude-class" directive
  by naming the one remaining route rather than looping into an 8th forward-lever gate.

## Scope / non-goals

- SP-0 is the **instrument + measurement + verdict** only. It does NOT wire the shadow
  deps into backjumping/caching/CDCL (that is the GO build). No default change.
- Branch `feat/nominal-rearch-sp0` off `feat/build-once-redesign`. Spike code stays on
  the branch; the durable deliverable is the verdict doc
  `docs/nominal-rearch-sp0-shadow-dep-probe-results-2026-06-25.md`. The shadow layer is
  retained on the branch (reused on GO).

## Global constraints

- FP=0 sacred — but here it is structural (read-only shadow; flag-off byte-identical),
  not a closure-diff gate. Still confirm flag-off wine closure unchanged.
- `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets --all-features
  -- -D warnings` (pedantic) clean; `cargo test --workspace` green (flag-off).
- Toolchain `RUSTUP_HOME=/home/dumontier/.rustup CARGO_HOME=/home/dumontier/.cargo` +
  stable bin on PATH.
- Commit only when asked; trailers `Co-Authored-By: Claude Opus 4.8
  <noreply@anthropic.com>` + `Claude-Session:
  https://claude.ai/code/session_01HSzon7V2wkhrudxBNAJduh`.
