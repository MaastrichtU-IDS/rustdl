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

## RESOLVED — the trace says (A), and 1-UIP is the wrong lever for wine

2026-06-06, second measurement. Instrumented every `clash_deps = DepSet::ALL`
site (temp, reverted) on two stalled wine classes under `trust_sat=0`.

**Step 1 — which site produces the overflow that defeats backjumping?** Not the
one I assumed. The `merge()` `≠`-violation site (the `wedge-merge-deps-defeat-
backjumping` note's nominal/NN suspect) fires **0 times** on these classes. The
overflow is **100% cardinality (`≤n`)**:

| class | site | events (20 s) |
|---|---|---|
| Sancerre | `forced_distinct_exceeds` pre-check | 72 000+ |
| CabernetFranc | `solve_at_most` partition fallback | 16 000+ |

CabernetFranc is **not** merge-free at the tableau level — it stalls on `≤n`
cardinality, same as Sancerre. Wine's intractability is cardinality, not
nominals.

**Step 2 — the ceiling (does precise deps even help?).** At each cardinality
clash, compute `over` = a **sound over-approximation** of the true clash deps
(over-approx ⟹ `over ⊇ true_deps` ⟹ `!over.contains(d)` ⟹ backjumping fires
*soundly* past the deepest decision `d`). Built up incrementally:

| `over` = | Sancerre backjumpable | CabernetFranc backjumpable |
|---|---|---|
| `⋃ birth_deps(succ)` | 100 % | 100 % |
| `+ ⋃ label_deps(succ)` (disjointness) | 100 % | 73–79 % |
| `+ parent(birth ∪ label)` (the `≤n` itself) | 100 % | 73–79 % |

`over_overflow = false` throughout — the over-approx is a precise small bitset,
**not** degenerate. So the deepest decision `d` is genuinely **outside** the
cardinality clash's real dependency set in **73–100 %** of clashes. Today those
clashes report `DepSet::ALL`, so backjumping fires **0 %** of the time
(`d_out = 0`).

**Step 2b — the sound (shippable) rate, not the optimistic one.** The `over`
above captures only one of the two distinctness channels:
`must_be_distinct = are_neq || labels_disjoint`. `label_deps` captures the
disjoint-label channel; the **`≠`-forced channel is uncaptured** (an `are_neq`
asserted under a decision is in neither birth nor label deps). So the 73–100 %
is *option-(i)* teeth — assumes `≠` is captured, hence an over-estimate of the
sound rate. The shippable *option-(ii)* fix falls back to `DepSet::ALL` whenever
a participating pair is distinct **only** via `≠` (`are_neq && !labels_disjoint`),
so its sound realized rate = the pure-disjoint-label fraction. Measured:

| | opt1 (assume ≠ captured) | **opt2 = SHIP (sound)** | pure-label |
|---|---|---|---|
| Sancerre (pre-check, disjoint grapes) | 100 % | **100 %** | 100 % |
| CabernetFranc (`solve_at_most`) | 73–79 % | **2.7 %** | 2.7 % |
| **wine aggregate (484 k clashes)** | 80 % | **43 %** | 63 % |

The two classes split exactly as the channel analysis predicts: Sancerre's
distinctness is 100 % disjoint-label → option (ii) backjumps soundly on all of
it; CabernetFranc's is 97 % `≠`-forced → option (ii) is inert there (its 73 %
opt1 was almost entirely the unsound channel). **Aggregate, the sound fix
converts ~43 % of wine's cardinality clashes from "block backjump" to
"backjump"** — vs 0 % today. That is the lever's real, sound teeth.

**This is (A): backjumping is artificially blocked.** A precise (over-approx)
dep on the cardinality clash unblocks it in the large majority of cases —
**without 1-UIP at all**. The multi-week asserting-clause build is the wrong
lever for wine; the right lever is **precise `≤n`-clash deps** (the deferred
`wedge-merge-deps-defeat-backjumping` lever, now *measured* to have teeth).

## The sound over-approx + its one soundness hole

Ship, at both cardinality `DepSet::ALL` sites, in place of `DepSet::ALL`:

```
over = ⋃_{s ∈ succs} (birth_deps(s) ∪ label_deps(s))
     ∪ birth_deps(parent) ∪ label_deps(parent)   // the ≤n constraint's own deps
```

Sound **iff** `over ⊇ true_deps`. Captured: succ existence (`birth_deps`), why
they're pairwise-distinct *when distinctness is disjoint-label-based*
(`label_deps`), the parent's existence + the `≤n` label. **Hole:** an explicit
`≠` (DifferentIndividuals / nominal `ObjectOneOf` distinctness) asserted *under a
decision* is in neither birth nor label deps — `over` would miss it →
**unsound backjump → false subsumption**. In wine all distinctness is told
(disjoint labels + told nominal distinctness → EMPTY/captured), which is why the
over-approx is sound *here* and the corpus FP=0 gate validates it. For general
soundness the shipped fix must either (i) track deps on the `≠` relation, or
(ii) conservatively fall back to `DepSet::ALL` whenever a participating distinct
pair is `≠`-forced rather than disjoint-label-derived (sound by construction,
still captures wine's pattern). Option (ii) is the cheap, safe first cut.

## BUILT + the merge-redirect PROOF outcome

2026-06-06. Built behind `RUSTDL_PRECISE_CARD_DEPS` (default OFF):
`HyperEngine::with_precise_card_deps()` + `card_clash_deps(parent, succs)`, gated
by `hyper_precise_card_deps_enabled()` at the three wedge-construction sites in
`reasoner/src/lib.rs`.

The first cut narrowed **both** cardinality sites and got wine 34→27 (FP=0
corpus-wide). Then we **pursued the soundness proof for the merge-redirect / edge-
provenance concern** — and it does NOT go through for both sites. Two distinct
holes:

- **Hole A — `solve_at_most` partition-exhaustion (fallback site).** It returns
  Unsat when *every* partition into ≤n mergeable blocks fails via the deeper
  `solve(depth-1)`. Those deeper failures can depend on broader-graph decisions
  (notably via inverse-role back-propagation) that are **not** in the local
  `succs`/`parent` — so `card_clash_deps(parent, succs)` can under-report there.
  Unbounded by a local set; **not provably sound. Reverted this site to
  `DepSet::ALL`.**
- **Hole B — merge-redirected edges.** `merge()` pushes `s_j`'s out-edges onto
  `s_i` without updating target `parent` fields, and a merge-gained successor's
  `birth_deps` predates the merge. A counted successor not *directly generated by*
  the canonical parent can carry a decision `over` misses.

**Where the proof goes through — the pre-check site with an own-successor
guard.** At `forced_distinct_exceeds` the clash is purely structural ("`>n`
pairwise-must-distinct R-successors of `parent`", no deeper search). With the
guard `∀ s ∈ succs: s.parent == Some(parent)` (closing Hole B by forcing the
`DepSet::ALL` fallback on any redirected edge) plus the `≠`-only fallback
(closing the disjoint-label-vs-`≠` channel), `⋃(birth ∪ label of succs) ∪
parent(birth ∪ label)` covers three of the four contributors: succ existence +
R-membership (`birth_deps`, since each succ was generated directly by `parent`)
and pairwise distinctness (`label_deps`, or fallback). This is the shipped
version.

**Hole C — the `≤n` constraint's own provenance (found in review, NOW CLOSED).**
The fourth contributor is "`parent` carries the `≤n`". That constraint lives in
`parent.at_most` as a **dep-less** `(role, qual, n)` tuple (`fire_head`'s
`AtMost` arm previously pushed it and *dropped* `deps`). So `over` captured the
constraint's provenance only *transitively*, via `parent.label_deps`, when the
clause that derived `≤n` is triggered by a **class atom on `parent`** (told
`A ⊑ ≤n R` → body `A(x)`; the wine case). It did **not** cover a `≤n` derived
under a decision by a **role-body** clause: `AtMost` is always Horn (never
disjunctive — clausifier emits a singleton head), but a domain/range axiom with a
cardinality filler (`clause.rs:296/303` pass `body=[Role(R,X,y)]` into
`emit_head`; the `Max` arm emits, no defer) or an absorbed `∃S.E ⊑ ≤n R` yields
`R(x,y) → ≤n(x)`, whose trigger dep is `y`'s label (`y` is not a counted R-succ)
— missed by `over`.

**Closed (commit below):** two new per-node fields, both backtracking-safe (the
`save`/`restore` whole-node clone preserves them; `from_snapshot` defaults them
to EMPTY, and cardinality seeds are never snapshot-replayed):
- `at_most_dep: DepSet` — union of the `body_deps` of every `AtMost` head fired
  onto the node (set at the push site). `card_clash_deps` seeds `over` with
  `parent.at_most_dep`, so a decision-derived `≤n` contributes its decision.
  (Closes the **derivation** half — the role-body case.)
- `at_most_tainted: bool` — set when `merge()` redirects another node's `at_most`
  onto this one. `card_clash_deps` returns `DepSet::ALL` when set, because a
  merge's *causation* dep (why the two nodes coincide) is untracked. (Closes the
  **merge-inheritance** half.)

With these, the pre-check `over = parent.at_most_dep ∪ ⋃(birth ∪ label of succs)
∪ parent(birth ∪ label)` is a superset of the true clash deps for **all four
contributors** (succ-existence, R-membership, distinctness, `≤n`-provenance) ⟹
**sound by construction.** Validated: wine MISSED still 34→31 (the taint guard
does not fire on wine — its `≤1 madeFromGrape` is told, `at_most_dep` EMPTY),
FP=0 across wine / ore-10908 / ore-15672 / shoiq-knowledge / sio / alehif.

Corpus closure-diff, sound version, flag ON (FP=0 = sound, MISSED = completeness):

| fixture | FP | MISSED | budget-indep? | note |
|---|---|---|---|---|
| **wine** | **0** | **34 → 31** | **yes** (31 at both 200 ms & 2000 ms) | recovered 3; closure 619 → 622; wall **neutral** (see note) |
| ore-10908 (SROIQ Q) | 0 | 0 | — | 6001 = 6001 |
| ore-15672 (SHOIN N) | 0 | 0 | — | unchanged |
| shoiq-knowledge | 0 | 0 | — | unchanged |
| sio | 0 | 0 | — | 8904 = 8904 |
| alehif | 0 | 0 | — | unchanged |

**The recovery is algorithmic, not speed** (the discriminating control): OFF is
flat at MISSED=34 at both 200 ms and 2000 ms — 10× the budget recovers nothing —
while ON sits at 31 at both. Backjumping reaches subsumed verdicts the un-pruned
wedge never reaches at any time budget. The 4 extra recoveries the first cut got
(31→27) came from the now-reverted fallback site (Hole A); they are **not**
provably sound and are not shipped. Residual 31 are likely wedge *incompleteness*
(`Sat`-when-should-be-`Unsat`), unfixable by backjumping (verdict-preserving).

**Wall is neutral — `−25%` retracted.** Earlier drafts of this doc and the 0.3.5
commit messages cited a `−25%` wine wall, from a single closure-diff OFF-vs-ON
pair (311 s vs 232 s). A clean 5-run re-measurement (`docs/perf-2026-06-06-
konclude-vs-rustdl.md`) shows the 232 s was a light-load outlier: OFF and ON both
sit at ~311 s. The gain is **completeness only** (the 3 recovered pairs were
capped at the 200 ms budget anyway, so resolving them faster is ≈0.6 s — noise on
a 311 s wall). No regression; the perf is neutral.

### Promotion path

The shipped (pre-check-only) version recovers 3 wine MISSED budget-independently
with FP=0 across wine/ore-10908(Q)/ore-15672(N)/shoiq/sio/alehif, and is now
**sound by construction for all four clash contributors** (Hole C closed). The
default-ON gate's soundness requirement is **met**.

**Regression guard.** All other 96 tableau tests run flag-OFF, so the flag-ON
path needs its own coverage. Added two non-ignored verdict-preservation tests
(`precise_card_deps_preserves_unsat_verdict` / `…_sat_verdict`): a synthetic
disjoint-label cardinality ontology classified flag-ON vs flag-OFF, asserting
**identical verdicts** — directly pinning the soundness property ("the
over-approx never changes a verdict, only prunes") so a future `card_clash_deps`
refactor can't silently rot it. (Per the advisor: this test, not the
`corpus_closure_long_timeout` bake, is the meaningful gate — GALEN/notgalen are
Horn-shortcircuited and never enter the wedge cardinality path. The
cardinality-bearing fixtures wine/ore-10908/ore-15672/shoiq are the real corpus
coverage and are already green.)

**Flipped to default ON (2026-06-06).** By explicit decision, after: sound by
construction (4-contributor proof + advisor sign-off), FP=0 across all six
cardinality/nominal fixtures, verdict-preservation regression tests in CI, inert
on the EL/Horn corpus (Horn-shortcircuited), and no test regressions (reasoner +
tableau suites identical with the flag forced OFF vs the new default). Set
`RUSTDL_PRECISE_CARD_DEPS=0` to revert to the conservative `DepSet::ALL`.

Recovering the extra 4 (the `solve_at_most` fallback site / Hole A) needs precise
deeper-search provenance — real conflict analysis (1-UIP-style) or `≠`-relation
dep tracking — a larger effort, deferred.

## Original next-step note (superseded by the build above)

1. Add a `clash_deps_card(succs, parent)` helper returning the `over` above,
   with the option-(ii) fallback to `DepSet::ALL` on any `≠`-forced pair.
2. Replace `DepSet::ALL` at the two cardinality sites (`forced_distinct_exceeds`
   pre-check + `solve_at_most` fallback) with it. Gate behind an env flag
   (`RUSTDL_PRECISE_CARD_DEPS`, default OFF) for A/B + safe rollout.
3. Gate hard: **corpus closure-diff FP=0 + MISSED-unchanged, ON vs OFF**, plus
   wine `d_out` and stall count. The FP=0 gate is the soundness net for the `≠`
   hole; option (ii) makes it sound by construction regardless.
4. If FP=0 holds and wine stalls drop, promote to default ON.

This supersedes the PR-20 1-UIP plan **for the cardinality classes**. 1-UIP may
still matter for pure-disjunction stalls, but wine's stalls are cardinality and
this is the cheap fix.
