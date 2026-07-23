# Design: nominal + number-restriction realize-termination (issue #35, v4)

**Date:** 2026-07-23
**Status:** IMPLEMENTED WITH CHANGED OUTCOME — fix A (nominals-first) FALSIFIED during
implementation and DEFERRED; safety net B shipped as the fix. See § Outcome at the end.
**Scope:** main tableau (`owl-dl-tableau` `saturate`/`search`/`rules`), realize path
**Env escape hatches:** `RUSTDL_NOMINAL_FIRST` (fix A, **default OFF / opt-in** — deferred), `RUSTDL_MAX_NODES` (safety net B, default 50k), `RUSTDL_REALIZE_PAIR_TIMEOUT_MS` (safety net B, default 750 ms)

## 1. Problem

`materialize_inferred_class_assertions` (Python) / `realize` (CLI) /
`instances_of` (reasoner) **hangs (non-terminating, ~300% CPU)** on a 1-minimal
3-axiom ontology reported on issue #35 against 0.3.38:

```
Prefix(:=<http://example.org/card#>)
Ontology(<http://example.org/card>
  Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))
  Declaration(ObjectProperty(:r))
  Declaration(NamedIndividual(:x)) Declaration(NamedIndividual(:y)) Declaration(NamedIndividual(:z))
  SubClassOf(:A ObjectOneOf(:x :y :z))
  EquivalentClasses(:B ObjectIntersectionOf(:A ObjectMinCardinality(2 :r :C)))
  ObjectPropertyDomain(:r :A)
)
```

`classify` returns in ~0.55 s; `realize` never terminates. HermiT terminates in
0.5 s, `consistent=True`. So this is a **termination bug in the tableau realize
path**, not an incompleteness or soundness bug. The reporter also notes an
MIE-derived ontology `mie_dl_consistent.ofn` hangs realization on 0.3.38, likely
the same pattern.

**1-minimality (reproduced):** dropping any one of the three non-declaration
axioms makes realization terminate.

## 2. Root cause

Established by reproduction + trace + code reading (fresh 0.3.38 build):

1. `EquivalentClasses(B, A ⊓ ≥2 r.C)` contributes the `⊑`-direction
   `A ⊓ ≥2 r.C ⊑ B`. Its LHS `≥2 r.C` cannot be absorbed into an atomic/nominal
   trigger, so it becomes a **residual GCI that fires on every node**:
   `⊤ ⊑ ¬A ⊔ ≤1 r.C ⊔ B` (`apply_residual_gcis`, `saturate.rs:119`).
2. When the ⊔-search picks the **`B`** disjunct on a node, `B ⊑ A ⊓ ≥2 r.C`:
   - `apply_min` (`saturate.rs:129`) generates **2 fresh `r.C` successors** —
     each successor again receives the residual GCI and can again pick `B` →
     an **infinite generating chain**;
   - the node becomes `A`, and `A ⊑ {x,y,z}` (`ObjectOneOf`) stamps it with a
     pending nominal-covering disjunction `{x} ⊔ {y} ⊔ {z}`.
3. The deterministic rule order runs **generation (`apply_exists` step 10,
   `apply_min` step 11) before `apply_nominal_assignment` (the merge, step 13)**,
   and the merge only fires once the search driver has resolved the `ObjectOneOf`
   to an atomic `Nominal(a)` label. Generation therefore outruns nominal
   resolution: fresh chain nodes are created before any merge can collapse them.
4. **Blocking cannot cut the chain.** Both `is_blocked_ancestor` and
   `is_blocked_anywhere` (`owl-dl-tableau/src/lib.rs`) **exclude nominal-labelled
   nodes on both sides** (`lib.rs:1021` — a nominal `y` is never blocked;
   `lib.rs:1062` — a nominal candidate is never a blocker), because blocking a
   singleton is unsound in general. Every node in this cycle is nominal-tainted
   (via `A ⊑ {x,y,z}`), so blocking never fires.

**Trace evidence:** `graph_nodes` climbs 1 → 1515 → 1695 → … monotonically with
descending depth (a deepening generated chain, not ⊔-breadth); two disjunctions
fire per node — the 3-way `ObjectOneOf` (`options=3`) and the 2-way residual GCI
(`options=2`) — with heavy immediate-clash backtracking.

**Confirmation of the mechanism:** replacing `ObjectOneOf(:x :y :z)` with a plain
named superclass `SubClassOf(:A :D)` (nodes no longer nominal-tainted) →
anywhere-blocking fires → **terminates**. This isolates the nominal label as
exactly what disables blocking. Dropping the domain axiom or the equivalence also
terminates, consistent with the mechanism above.

**Relation to the prior #35 fix.** Commits `df65f0f` / `2a449ac` (0.3.35) switched
the deadline-free paths to anywhere-blocking, fixing the earlier
nominal-anchored ∃-cycle (`hang_v3`). That fix is live and correctly enabled here
(`decide` selects anywhere-blocking when `deadline.is_none()`; forcing
`RUSTDL_ANYWHERE_BLOCKING=1` changes nothing), but anywhere-blocking carries the
**same nominal exclusion**, so the `≥-cardinality + ObjectOneOf + domain` pattern
slips past it. This is a distinct, deeper case: the earlier fix bounded a cycle
whose nodes were not all nominal; here every cycle node is nominal-tainted.

## 3. Fix A — nominals-first scheduling

**Idea:** collapse each `A`-node into one of the ≤k canonical nominal nodes
**before** it can generate a successor, so the graph is bounded to
(#nominals + their immediately-merged successors) — the same finite 3-nominal
model HermiT builds. This is a **scheduling change only**: generation is
deferred (never skipped), the disjunction is still branched, and the collapse is
the existing `apply_nominal_assignment` merge. No blocking-soundness change.

### A.1 Generation guard (`rules.rs`) — must be TBox-aware

**Critical ordering fact (advisor concern 1, verified against the code).** The
nominal covering `Or` is NOT an eager label. `absorb` turns `A ⊑ {x,y,z}` into
`ConceptRule { trigger: A, conclusion: Or([Nominal x, Nominal y, Nominal z]) }`,
and `apply_concept_rules` **explicitly skips `Or(_)` conclusions** (`rules.rs:204`
and `:217`). The nominal `Or` is materialized only by
`apply_deferred_concept_or_rules`, which runs **only in the stable-state sweep**
(`saturate.rs:158`, gated on `if !changed`). The residual GCI
`⊤ ⊑ ¬A ⊔ ≤1 r.C ⊔ B` is likewise a deferred-Or residual. But `apply_min` /
`apply_exists` run in the per-node rule block (`saturate.rs:128-129`) **before**
that sweep. So on the pass where a node acquires `A` + `≥2 r.C`, generation fires
while the nominal `Or` is still only a *pending, unmaterialized* concept-rule
conclusion. A predicate that scans only materialized `Or` labels is therefore
**inert exactly when it must fire** — this was the fatal flaw in the first draft.

The predicate must consult the TBox. Add on `TableauContext`:

```
fn has_pending_nominal_disjunction(&self, node: NodeId) -> bool
```

Returns true iff **either**:
- (materialized) `node` carries an open `Or` label (no disjunct yet in the label
  set) with at least one `Nominal(_)` disjunct; **or**
- (pending, not-yet-materialized) `node` carries an `Atomic(class)` label whose
  `tbox.concept_rules_by_trigger[class]` (or the linear `concept_rules` fallback
  when the index is empty) has an `Or` conclusion that contains a `Nominal(_)`
  disjunct and none of whose disjuncts is already in the node's label set.

The pending branch is what fires on the bug (node has `A`, `Or` not yet
materialized). TBox access is via `ctx.tbox() -> Option<&AbsorbedTBox>`;
`concept_rules_by_trigger` is keyed by `ClassId`.

`apply_exists` and `apply_min` return `RuleOutcome::NoChange` — **defer, do not
generate** — while `has_pending_nominal_disjunction(node)` holds. Everything else
on the node still fires; the pass stabilizes without generation, the sweep
materializes the nominal `Or`, and the search driver resolves it (A.2). Deferral
is **revisitable, never a skip**: `add_label_with_deps` re-dirties the node
(`lib.rs:1123`) and `merge` re-dirties the survivor (`lib.rs:1166-1168`), so
generation resumes on the canonical node after the merge (completeness-preserving;
verified with the advisor).

Cost: cheap-false on ontologies with no nominal-covering concept rule (the
`Or`-with-`Nominal` conclusion never exists); only adds a TBox lookup on
nominal-covering inputs. Gated by `RUSTDL_NOMINAL_FIRST` (default ON); `=0`
reverts to unconditional generation.

### A.2 Search-driver priority (`search.rs`)

`first_open_disjunction` (and the choose-rule selection) **prefers a pending
nominal-covering disjunction** over other open disjunctions when one exists on any
node. This guarantees the search resolves the `ObjectOneOf` first; the subsequent
`apply_nominal_assignment` merges the node into the canonical nominal; on the next
saturation round `has_pending_nominal_disjunction` is false on the survivor and
the deferred `≥/∃` generation fires — **once**, on the bounded canonical node.

Priority-only; does not change WHICH branches exist, only their order, so it is
verdict-neutral by construction (it can change perf and which model is found
first, not satisfiability).

### A.3 Why it terminates — argument + mandatory empirical gate

With A.1 TBox-aware (so generation is genuinely deferred *before* a node's
nominal `Or` materializes) and A.2 resolving that `Or` first: a node that picks
`B` acquires `A`, the guard fires (pending nominal `Or` via the TBox),
`apply_min`/`apply_exists` defer, the pass stabilizes, the sweep materializes the
`Or`, A.2 selects it, `apply_nominal_assignment` merges the node into one of the
`{x,y,z}` canonical nodes, and only *then* does generation fire — once, on the
canonical node. Every prospective generator collapses into one of the ≤k
canonical nominals before spawning a successor, so the completion is bounded by
(#named individuals + a bounded transient frontier).

**This is an argument, not a proof — and it is gated empirically (advisor concern
2).** The `≥2`-distinctness constraint (the two `r.C` successors must be distinct)
interacts with only 3 available nominals via `apply_max` / the choose rule
(`rules.rs` `apply_max`, `first_open_choose`), and clash-and-backtrack cycling is
not ruled out a priori. Therefore acceptance **requires a hard, deterministic
node-count gate run through the real `saturate`/`search` driver** on the
reproducer with the **cap disabled** (`RUSTDL_MAX_NODES=0`, `RUSTDL_NOMINAL_FIRST=1`):
assert `ctx.graph().len()` stays under a small constant AND the verdict is
`Sat`/`Unsat`. Cap-disabled is essential — a cap-based check is cap-invariant
(§5.1). This is the acceptance criterion, NOT deferred to the corpus bake-off. A separate risk is that a deferred node's nominal `Or` is never
selected by A.2 (generation never resumes) → a *new* stall producing wrong realize
output rather than a hang; the same node-count gate plus a correct-verdict
assertion catches it, and safety net B (§4) prevents any hang regardless.

### A.4 Soundness & completeness

- **Sound:** no new blocker/merge logic; `apply_nominal_assignment` is unchanged;
  deferring a rule cannot introduce a spurious entailment.
- **Completeness-preserving:** generation is deferred, not dropped — it resumes on
  the survivor after the merge; the disjunction is still fully branched. A.2 only
  reorders branch selection. The claim is validated empirically by the corpus
  bake-off (byte-identical closures, MISSED unchanged), consistent with how every
  scheduling/perf change in this repo is gated.

## 4. Safety net B — deterministic node/inference cap

Independent of A, guarantee **no input can hang** even on an unforeseen variant.

**Correct mapping matters (advisor concern 3, verified).** A naïve
`SaturationResult::Stalled` on cap-exceed does NOT yield a sound
under-approximation. `saturate` → `Stalled` maps to `SearchVerdict::DepthLimit`
(`search.rs:113`), and on the **deadline-free** path `decide` maps
`DepthLimit if deadline_reached() => Ok(None)` else `=> Err(NoVerdict)`
(`lib.rs:5312-5313`). With no deadline set, that is `Err(ReasonError::NoVerdict)`,
which `realize.rs:198` (`Ok(!prepared.decide(build)?)`) propagates via `?` — the
whole `realize` call **fails hard**, not a MISS. Two internal probes also
`.expect("no deadline ⇒ search always returns Some(_)")` (`lib.rs:4572` and
`:4605`) and would **panic** once a deadline-free search can stall.

Design that yields the intended semantics:

- Add a distinct **node-cap signal** — `SaturationResult::NodeCapped` →
  `SearchVerdict::NodeCap` — separate from the rule-bug `DepthLimit`, so a cap
  trip is unambiguous. **It must stay distinct through `search::branch`**
  (`search.rs:238-294`): `branch` treats `DepthLimit` *softly* (sets a flag, falls
  back to returning `DepthLimit`), so folding `NodeCap` into that path would return
  `DepthLimit` and `decide` would map it to `Err(NoVerdict)` — re-creating this very
  bug. `branch` needs its own `node_capped` flag and a final-return `NodeCap` arm,
  and `SearchVerdict::to_option` needs a `NodeCap => None` arm. (No `SearchVerdict`
  match uses a `_` wildcard, so the compiler forces every site to handle it.)
- Track live node count on the deadline-free `saturate`/`search` path
  (`ctx.graph().len()`), checked at the same cadence as `check_deadline()`. On
  exceeding `RUSTDL_MAX_NODES` (default ~50 000 — far above any real corpus
  completion; tune during the bake-off), return `NodeCapped`.
- In `decide`/`decide_with_deadline`, map `NodeCap => Ok(None)` on **both** paths
  (a cap trip is a clean "no verdict", exactly like a spent deadline). Then every
  caller already treats `Ok(None)` soundly:
  - realize / `is_instance_of` / `instances_of` — `None → unwrap_or(true)` at the
    probe (`realize.rs:196-198` already does this for the deadline case) → **do
    not assert the type** (sound MISS);
  - `is_consistent` — `None → consistent` (the existing MISSED-inconsistency
    under-approximation).
- Fix the two `expect("no deadline ⇒ Some")` sites (`lib.rs:4572`, `:4605`,
  label-oracle probes) to treat a `None` from a cap trip as `LabelOracle::
  NoVerdict` instead of panicking.

Deterministic (node count, not wall clock) — same result every run, matching the
repo's preference for count/branch bounds over timeouts (cf. adaptive-budget's
branch window). `RUSTDL_MAX_NODES=0` disables the cap.

**Scope limit of B (be honest):** the cap bounds *instantaneous* live node count
(`graph().len()`). It does NOT bound a clash-and-backtrack / `≥n`-distinctness
explosion that churns nodes under rollback at bounded instantaneous size — that
mode terminates only via the deadline-free search-depth ceiling
`DEEP_SEARCH_DEPTH = 1_000_000` (`lib.rs:5302`), i.e. as a very slow
`DepthLimit → Err(NoVerdict)`, not a clean MISS. The reported #35 trace is
monotone node-growth, which B *does* cover; the residual `apply_max`-cycling mode
is why fix A (not B) is the actual fix, and why A's acceptance is the cap-disabled
bounded-graph gate in §5.1, not a cap-based check.

**B is the belt; A is the fix.** With A working, the corpus never approaches the
cap; B only ever fires on a pattern A doesn't yet handle, converting a hang into a
sound MISS + a diagnosable signal.

## 5. Testing / verification

Use the real reasoner APIs (verified): `owl_dl_reasoner::realize(&onto)` →
`Realization::{entailed_types, most_specific_types}(iri) -> &[String]`; build
ontologies in tests via `parse(&format!("{HEADER}…"))` as the existing realize
tests do (`realize.rs`, incl. the v3 `realize_terminates_on_issue35_reproducer`
at `realize.rs:1239`). There is no `load_ontology`/`types_of`/`AbsorbedTBox::
empty`/`RoleHierarchy::empty`/`intern_atomic`/`testkit` — those were fabrications
in the first draft.

### 5.1 Canaries (new)
- **Cap-DISABLED bounded-node gate (the termination acceptance criterion,
  concern 2).** Run the reproducer through the actual `saturate`/`search` driver
  with `RUSTDL_MAX_NODES=0` (cap OFF) and `RUSTDL_NOMINAL_FIRST=1`, and assert
  `ctx.graph().len()` stays ≤ a small constant (e.g. 64) **and** the verdict is
  `Sat`/`Unsat` (not `NodeCap`/`DepthLimit`). Built at the `owl-dl-tableau` layer
  by feeding the reproducer's absorbed TBox (via `owl-dl-core` convert+absorb)
  into the `decide`-style probe. **This is the load-bearing gate.** A
  realize-level "does not trip a low cap" check is NOT sufficient: it is
  cap-invariant — a divergent run that trips the cap returns
  `NodeCap → Ok(None) →` "not an instance", the *same* answer as a genuinely
  bounded run, so it cannot distinguish the two. The realize-level test is kept
  only as a no-hang/no-error + verdict smoke check, plus a positive-entailment
  canary (where a capped divergence would give the WRONG answer, making it
  observable). HARD gate, not deferred to the bake-off.
- The reporter's 3-axiom core through `realize`: terminates and matches HermiT
  (consistent; `x,y,z` carry no spurious `B`/`C` types). Committed fixture.
- `mie_dl_consistent.ofn` (reporter-supplied, if obtainable) terminates.
- The `no_nominal` (plain named superclass) and `no_domain` minimality variants
  stay correct — guards against the guard over-firing.
- Predicate unit test: `has_pending_nominal_disjunction` true for the pending
  (TBox concept-rule) case AND the materialized-open-`Or` case; false once a
  disjunct is present or on a plain atomic node.
- Safety-net test: with `RUSTDL_NOMINAL_FIRST=0` and a low `RUSTDL_MAX_NODES`, the
  reproducer returns a sound result (`realize` succeeds with `Ok`, no panic, no
  hang) — proving the `NodeCap → Ok(None)` mapping, not a hard error.

### 5.2 Correctness gate (the standard engine-change bake-off)
Full corpus — galen, notgalen, sio, wine, ore-10908, ore-15672, alehif, ro,
pizza, bibtex — **FP=0 / MISSED unchanged, byte-identical classify closures, no
reproducible wall regression**, run on a freshly-built release binary (per the
toolchain gotcha). A/B via `RUSTDL_NOMINAL_FIRST=0`.

### 5.3 Realize oracle
Where an oracle is available, `realize` output on the new fixtures matches
HermiT/ROBOT realization (types per individual).

## 6. Risks

- **A.1 predicate fires too late (the flaw the first draft had).** If the
  predicate scans only materialized labels it is inert at generation time
  (concern 1). Mitigation: the TBox-aware pending branch (§A.1); the real-driver
  bounded-node gate (§5.1) fails loudly if generation is not actually deferred.
- **A.1 predicate too broad** → defers generation when it shouldn't, changing a
  verdict or stalling a legitimate model. Mitigation: restrict strictly to an
  `Or` (materialized or TBox-pending) containing a `Nominal` disjunct; the
  bake-off byte-identity gate catches any verdict drift.
- **Safety-net B mapping (concern 3).** A cap trip must map to `Ok(None)`, not
  `Err(NoVerdict)`, on the deadline-free path, and the two `expect("no deadline ⇒
  Some")` sites must be made graceful — otherwise B produces hard errors/panics
  instead of sound MISSes. Mitigation: the distinct `NodeCap` verdict + the
  safety-net test in §5.1.
- **A.2 priority interacts with backjumping / branch ids.** Reordering disjunction
  selection must not corrupt `branch_id`/`DepSet` bookkeeping. Mitigation:
  selection order is independent of dependency tracking; verify the
  verdict-preservation regression tests in `owl-dl-tableau` stay green.
- **Deferral + no eventual resolution = stall.** If a node has a pending nominal
  disjunction that the search never selects (bug in A.2), generation never
  resumes → a new stall. Mitigation: the model-shape canary + the B cap backstop.
- **Wedge parity.** Classify uses the hyper wedge, which has its own generation
  path; this fix is scoped to the main tableau (the realize/consistency path).
  The wedge is out of scope but must be checked for the same latent pattern as a
  follow-up (§8).
- **Perf.** Deferral + re-saturation could add rounds on nominal-heavy ontologies
  (wine's nominal cluster). Bake-off wall gate covers it.

## 7. Out of scope (follow-ups)
- Sound nominal-aware **blocking** (relaxing the `lib.rs:1021/1062` exclusion) —
  the more general but higher-soundness-risk mechanism; deferred unless a pattern
  appears that nominals-first does not bound.
- The **NN-rule** (Horrocks–Sattler) for number-restrictions-meet-nominals — the
  textbook-complete solution; only if a nominal-covered `≥n` *filler* case surfaces.
- Wedge (`hyper.rs`) generation path — verify/port if it exhibits the same hang.

## 8. Rollout
- `RUSTDL_NOMINAL_FIRST` default ON; `RUSTDL_MAX_NODES` default ~50k.
- CHANGELOG entry; CLAUDE.md tableau section note (issue #35 v4).
- Reply on the GitHub issue with the fix + the immediate workaround
  (`RUSTDL_REALIZE_PAIR_TIMEOUT_MS`) for users on the released 0.3.38.

---

## Outcome (2026-07-23, post-implementation)

The plan was executed via subagent-driven development. **The result differs from
the design's central bet.**

### Fix A (nominals-first scheduling) — FALSIFIED, deferred
Implemented as Tasks 1–4 (`RUSTDL_MAX_NODES` cap infra, TBox-aware
`has_pending_nominal_disjunction`, the `apply_exists`/`apply_min` guard, and the
`first_open_disjunction` nominal priority). The Task 5 acceptance gate — the
cap-disabled, real-driver bounded-node assertion this design insisted on (§5.1,
per advisor concern 2) — showed **A does not bound the reproducer**: with the cap
disabled and `RUSTDL_NOMINAL_FIRST=1`, the completion graph grows without bound
(`graph().len()` tracks `cap−1` exactly at every cap value = genuine divergence,
not convergence), verified independently with the release binary (`realize` hangs).

**Why the design was wrong.** The root-cause analysis in §2 was incomplete. The
generating cycle is driven not only by the equivalence's residual but by
`ObjectPropertyDomain(r,A)`, which absorbs to an **untriggered universal residual
GCI** `⊤ ⊑ ¬∃r.⊤ ⊔ A` — a *residual-GCI disjunction*, not a `concept_rule`.
`has_pending_nominal_disjunction` (keyed on `concept_rules_by_trigger`) never sees
it; choosing its `A` disjunct on a fresh `≥2 r.C` witness re-opens the covering
nominal disjunction, and the o-rule merge folds the witness into the constraint's
own owner, forcing endless regeneration. Dropping either the domain axiom or the
nominal covering alone terminates in <1 s (matching the reporter's 1-minimality);
only the combination diverges. **Conclusion: nominals-first *scheduling* is the
wrong mechanism for this cycle.** The machinery is retained, dormant behind
`RUSTDL_NOMINAL_FIRST` (default OFF, opt-in), as scaffolding for a proper
redesign — sound nominal-aware **blocking** (relaxing the `lib.rs:1021/1062`
nominal exclusion) or the **NN-rule** — to be scoped as a separate spec.

### Safety net B — shipped as the fix
B is now the delivered fix. At default settings `realize` on the reproducer
terminates in ~0.75 s with a sound result (a MISS — `x,y,z` are not reported as
`B`/`C`; matching this is the deferred A work). B is two sound, deterministic
bounds:
- **`RUSTDL_REALIZE_PAIR_TIMEOUT_MS` defaults to 750 ms** (was unbounded since
  0.3.18; `=0` opts out). Bounds each per-individual realize probe → sound MISS.
  Affects only realize, never classify/consistency.
- **`RUSTDL_MAX_NODES` cap** (default 50000, `0` disables) on the deadline-free
  tableau path, returning a distinct `NodeCap` verdict → `Ok(None)` (sound MISS /
  consistent under-approximation) with a **hard early-return**. Never `Err`, never
  a panic.

### Not done (deliberately)
- The full corpus bake-off (planned Task 7) was **not run**: with A deferred and
  `RUSTDL_NOMINAL_FIRST` default OFF, no default-ON engine scheduling changed, so
  the merged branch's only default behavior change is safety net B (sound bounds
  on realize). A bake-off is a prerequisite before ever enabling nominal-first by
  default.

### Pre-existing, unrelated bugs surfaced (file separately)
1. `debug_assert_eq!` in `TableauContext::remove_edge_recorded` (`owl-dl-tableau/src/lib.rs:1529`)
   **panics** on this reproducer in debug builds past ~10–19 nodes, reachable via
   the CLI; reproduced at the base commit (predates this work).
2. `realize`'s `entailed_types` misses an entailment that `is_instance_of` finds
   — a realization scoping gap.
