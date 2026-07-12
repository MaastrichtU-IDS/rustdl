# Advisor design: back-fold rule to close galen `TT ⊑ TICE`

**Status:** design, read-only analysis, code-grounded. **Verdict: the fix is
real, targeted, and small — but ONLY in a form that is NOT the "candidate +
verify" shape the triage assumed.** The verify direction hangs, so the fix must
be a *sound direct derivation* over the branch-free Horn sat model, added to the
hierarchy like a closure fact — never routed through `subsumes_via_tableau`.
Read §0 first; it changes the whole framing.

---

## 0. The decisive finding (measured) — why "candidate + verify" cannot work

The triage note assumed: make the label cache a sound over-approximation so the
per-pair verify filters the extra candidate. **I measured the verify and it does
not converge.**

- `rustdl subclass galen.ofn TT TICE` (= `is_subclass_of`, reduces to sat of
  `TT ⊓ ¬TICE`): **hangs > 60 s** (killed at 60 s, 99% CPU the whole time).
- `rustdl explain galen.ofn TT TICE`: **hangs > 25 s** (killed).
- `rustdl classify galen.ofn`: **0.85 s**, top-down label-pruned path;
  emits `TT ⊑ Eminence`, `TICE ⊑ Eminence`, but **not** `TT ⊑ TICE` (the MISS).

The hang is exactly the mechanism `galen-defined-class-monotonicity-residual.md`
predicts: `¬TICE` in NNF is the **disjunction**
`¬Eminence ⊔ ∀isSpecificSolidDivisionOf.¬TibialPlateau`; refuting `TT ⊓ ¬TICE`
needs disjunctive branching + `∀`-propagation + the functional merge to
interact in one branch. That search does not terminate quickly.

**Consequence:** if the back-fold merely *adds `TICE` to `labels(TT)`* and lets
the prune's pass-through call `subsumes_via_tableau(TT, TICE, …)`
(`classify.rs:1683` / `:1981`), the probe times out → returns `false` → still a
MISS (or, unbounded, it is the `DEFINED_SWEEP` wall explosion). The `¬sup`
verify is the wall; the label prune is only the gate in front of it. **Widening
the gate does nothing unless we also stop calling the verify for this pattern.**

**So the fix is a different shape:** recognize `TT ⊑ TICE` as a *sound
entailment* directly from the **forward, branch-free** `sat(TT)` model that
`classify_labels` already builds (the cheap direction — `sat(TT)` alone is Horn
and fast), and inject it into the hierarchy exactly like the existing
**defined-SUB sweep** does (`classify.rs:1833-1836`, a direct
`direct_supers[c].push(x)` with *no* tableau call). No `subsumes_via_tableau`.
This is what avoids both the MISS and the explosion.

This works because the expensive direction (`TT ⊓ ¬TICE`, disjunctive) and the
cheap direction (`sat(TT)`, Horn) are asymmetric, and the cheap direction
already contains the whole proof — see §1.

---

## 1. The back-fold rule

### 1.1 What `sat(TT)` already contains (verified against the axioms)

`classify_labels(TT)` (`reasoner/src/lib.rs:2510-2631`) runs the wedge on `TT`
alone (Q-clause `Q → TT`) and returns `satisfiability_labels(fresh_q)`
(`hyper.rs:1740-1753`) = the **root node's label set**. Trace the forward Horn
fixpoint (`horn_fixpoint`, deterministic, no `⊔`):

1. `TT ≡ Eminence ⊓ ∃isSSDivOf.Tibia ⊓ ∃isAtOtherEndOf.LigamentumPatellae`
   ⇒ root `x : Eminence`, edge `x —isSSDivOf→ w`, `w : Tibia`.
2. `Tibia ⊑ ∃hasSpecificSolidDivision.TICE` ⇒ `w —hSSD→ v`, `v : TICE`.
3. `hSSD ≡ inv(isSSDivOf)` ⇒ inverse edge `v —isSSDivOf→ w` materialised.
4. `v : TICE` unfolds forward ⇒ `v : Eminence`, `v —isSSDivOf→ u`,
   `u : TibialPlateau`. `Functional(isSSDivOf)` on `v` has two isSSDivOf-succs
   `{w, u}` ⇒ the **incremental `≤1` merge** (`RUSTDL_INVERSE_FUNC_MERGE`,
   default ON, `enforce_at_most_one`, `hyper.rs:~2510`) merges them
   deterministically ⇒ survivor carries `{Tibia, TibialPlateau}`, i.e.
   **`w : TibialPlateau`**.

After step 4 the graph has: root `x : Eminence`, edge `x —isSSDivOf→ w`,
`w : TibialPlateau`. **Everything above is Horn/deterministic — no `⊔`, no
`≤n>1` branch merge — so `branches_taken == 0`.** All of it is entailed
(`SearchStats` docstring, `hyper.rs:441` — "`branches_taken == 0` was decided by
pure Horn propagation"; every `DepSet` is `EMPTY`, `hyper.rs:67`).

What the wedge does **not** do: fire the *backward* direction of `TICE`'s
definition. `TICE ≡ Eminence ⊓ ∃isSSDivOf.TibialPlateau` clausifies forward only
(`Eminence ⊓ ∃…TibialPlateau ⊑ TICE` has an `∃` in the body ⇒ non-clausal ⇒ not
a Horn clause). So `TICE ∉ labels(x)`, the pair is pruned at `classify.rs:1697`,
MISS. **The back-fold is precisely that missing backward `∃`-composition step,
run over this already-built graph.**

### 1.2 The rule (a completion/label rule over the sat graph)

For a defined class `D ≡ A₁ ⊓ … ⊓ A_k ⊓ ∃r₁.C₁ ⊓ … ⊓ ∃r_m.C_m` (m ≥ 1) and a
graph node `x`:

```
BACKFOLD:
  if  (∀i) Aᵢ ∈ labels(x)
  and (∀j) ∃ a resolved rⱼ-successor y of x with Cⱼ ∈ labels(y)
  then add D to labels(x).
```

Concretely at the root node, reusing existing engine machinery:

- atomic check: `Aᵢ ∈ node.labels` (`HyperNode.labels`, `hyper.rs:160`).
- existential check: `!self.distinct_role_succ(x, rⱼ, Some(Cⱼ)).is_empty()`
  (`hyper.rs:2456-2492`) — this already resolves the union-find, honours the
  role hierarchy via `role_matches`, and (when `inverse_func_merge` is on)
  scans `preds` too, so the merged survivor `w` is found and its
  `has(TibialPlateau)` is checked. **No new traversal code needed.**

For `TT`: `Eminence ∈ labels(x)` ✓ and `distinct_role_succ(x, isSSDivOf,
Some(TibialPlateau))` returns `{w}` ✓ ⇒ add `TICE`. Done.

**Optional recall widener (defer):** accept `C' ∈ labels(y)` with
`C' ⊑_closure Cⱼ` (a closure subclass of the filler), not just `Cⱼ` literally.
Not needed for the target (the merge puts `TibialPlateau` on `w` verbatim), and
it only widens recall — leave it off in v1.

**Optional fixpoint (defer):** the rule is monotone; iterating it over *all*
nodes to a fixpoint captures nested defined classes. For the target a **single
pass at the root** suffices (the filler label `w:TibialPlateau` is already
materialised by the Horn merge). v1 = root-only, one pass; re-run cheaply until
no new atomic conjunct fires if you want to catch `D` whose `Aᵢ` is itself a
back-folded name (bounded by #defined classes).

### 1.3 Does saturation already do this? (honest answer: half of it, in the
wrong engine)

Yes and no — and the "no" is the whole bug:

- **`owl-dl-saturation` (ELK-style) already implements `∃`-composition into
  defined classes** — that is its CR5 + Tseitin machinery for compound `∃`
  bodies (`saturation/src/lib.rs:36`, `atomic_or_tseitin_body`,
  `TseitinAllocator`, `lib.rs:2213+`). It *would* derive `TT ⊑ TICE` — **but
  only if `Tibia ⊑ TibialPlateau` were in the EL closure.** It is not:
  `Tibia ⊑ TibialPlateau` requires the functional/`≤1` merge across the declared
  inverse, which is **out of EL** and not a saturation consequence. So the
  saturator's `closure.contains(TT, TICE)` (`classify.rs:1969/1631`) is false —
  correctly, given its inputs.
- **The wedge's `horn_fixpoint` (`hyper.rs`) has the merge** (that is exactly
  what `RUSTDL_INVERSE_FUNC_MERGE` added, default ON) — it derives
  `w : TibialPlateau`. **But it has no backward `∃`-composition** (non-clausal),
  so it never labels `x : TICE`.

Neither engine has *both* the merge and the `∃`-composition. The fix adds the
`∃`-composition **over the wedge's merge-enriched Horn model** — i.e. it runs
the saturator's CR5 idea against the graph that finally contains the merge fact.
This is why routing through the *saturation closure* (option B below) does not
work without also feeding the merge fact back into saturation; running the rule
in the wedge's label path (option A) is strictly smaller.

---

## 2. Soundness — the rule can only ADD, and only when entailed

Two independent soundness claims:

**(a) As a derivation (what we actually do): sound because the model is
branch-free.** Gate the derivation on the sat having `branches_taken == 0`
(`SearchStats`, `hyper.rs:445-458`) — equivalently, on every label used having
`deps_of(...) == DepSet::EMPTY` (`hyper.rs:282-287`). In a branch-free run there
are no decisions, so the least Horn model *is* the canonical model and every
label/edge is a genuine consequence: `TT ⊑ Eminence`, `TT ⊑ ∃isSSDivOf.TibialPlateau`
both hold in every model. Hence `TT ⊑ Eminence ⊓ ∃isSSDivOf.TibialPlateau = TICE`.
This is the standard EL `∃`-composition soundness, applied to a model that
happens to also contain the merge fact. **FP-free by construction** — it is the
same class of reasoning as the closure seed (`classify.rs:1857`) and the
defined-SUB sweep (`classify.rs:1767` "genuine entailment — added directly, no
tableau/wedge call").

*Blocking is not a soundness threat:* blocking can only make a node's labels
*incomplete*, never add a spurious label. The rule requires a *positive*
membership `Cⱼ ∈ labels(y)`, so blocking can only cost recall (a MISS), never
create an FP.

*The merge is deterministic:* `≤1`/functional merge (`enforce_at_most_one`)
fires inside the Horn fixpoint with `DepSet::EMPTY`; it is not a `≤n>1` branch
merge (`find_open_at_most`) and does not increment `branches_taken`. So the
merge fact is genuinely entailed, not branch-relative.

**(b) As a candidate widener (the fallback view): also sound, but useless
here.** If we instead only added `D` to the `Sat(labels)` set, the prune
(`classify.rs:1677-1700`, `:1975-1997`) can only move a pair from `pruned`
(false) to `pass_through` → `subsumes_via_tableau`. The tableau makes the final
call, so FP=0 is preserved regardless. But per §0 that verify hangs, so this
view yields a timeout MISS, not a fix. **We therefore adopt (a), not (b).**

Either way the candidate/derived set is a **superset** of true subsumers: the
rule is pure monotone addition; it never removes a label, never prunes, never
flips a `pass_through` to `pruned`.

---

## 3. Why it does NOT explode like `DEFINED_SWEEP`

`RUSTDL_CLASSIFY_DEFINED_SWEEP` bypasses the prune and calls
`subsumes_via_tableau(cand, sup, trust_sat=false)` (`classify.rs:1656-1675`) for
**every** (candidate × defined-sup) pair. galen has **681 defined-`∃` classes**
(of 699 `EquivalentClasses`; 2748 classes total) — so it fires the *hanging*
`¬sup` tableau probe hundreds-of-thousands of times. That is the >6:40 /
>3:20 wall.

The back-fold makes **zero** tableau/wedge calls. It is a structural scan over
the `sat(c)` graph that `classify_labels` **already builds** for the label
cache. Cost per class = O(#defined-`∃` bodies × #successors-per-`∃`) ≈
681 × (small) label/edge lookups; over 2748 classes that is a few million
`Vec::contains`/`distinct_role_succ` calls — sub-second, dwarfed by the sat
itself. No new search, no new deadline pressure.

**How many extra subsumptions does it surface on galen?** Essentially **one**
(the target family), possibly a tiny handful of siblings. Reason: the EL closure
already contains *every* defined-`∃` subsumption whose filler subsumption is an
EL consequence (the saturator's CR5 does exactly this). The back-fold adds only
the defined-`∃` subsumptions whose filler subsumption is **merge-derived and
therefore absent from the EL closure** — on galen that is the
`Tibia ⊑ TibialPlateau`-gated `TT ⊑ TICE` pattern. So the delta over the closure
is ~1 derivation, not ~681 verifies. That asymmetry is the entire point.

---

## 4. Where to implement (smallest correct change)

Three candidate sites; the smallest correct one is **A**.

**A. In `classify_labels` / a new `HyperEngine` graph method (RECOMMENDED).**
The graph is live only inside `classify_labels` (`lib.rs:2510-2631`); it is
discarded after `satisfiability_labels` returns a flat root-label set. So the
back-fold must run there, while `self.nodes`, `resolve`, `distinct_role_succ`
are in scope. Concretely:

1. **Precompute once** in `HyperCache::build` (`lib.rs:2016+`) a
   `defined_exists_bodies: Vec<(ClassId, SmallVec<ClassId>, SmallVec<(Role, ClassId)>)>`
   — one entry per `EquivalentClasses`-defined name whose body is a conjunction
   with ≥1 `∃`-conjunct. `owl_dl_core::definitions::extract_definitions` is
   **already called at `lib.rs:2020`** and returns `Definitions::body_of(c)`
   (`definitions.rs:34`); walk each body `ConceptExpr::And` into atomic
   conjuncts + `(role, filler)` `∃`-conjuncts. Store on `HyperCache` beside
   `sat_seed`/`exists_seed` (`lib.rs:1987-1995`). Purely-atomic defined bodies
   are excluded (they already fire via Horn clauses).
2. **New engine method** `HyperEngine::backfold_derived(&self, root, &bodies,
   sub_roles) -> Vec<ClassId>` (in `hyper.rs`, near `distinct_role_succ`
   `:2456`): only when `self.stats().branches_taken == 0`, run BACKFOLD (§1.2)
   at `self.resolve(HNode-of-root)` and return the derived `D`s. (Finer gate:
   drop the whole-run branch-free requirement and instead require every `Aᵢ` and
   the witness `Cⱼ` label to have `deps_of == EMPTY`; keeps the rule alive on
   classes whose sat branched elsewhere. v1 can use the coarse whole-run gate —
   galen's per-class forward sats are branch-free, consistent with the 0.85 s
   classify wall.)
3. **Carry the result out** without corrupting the candidate semantics: extend
   `LabelOracle::Sat` (`lib.rs:1871-1882`) to
   `Sat { labels: HashSet<ClassId>, derived_sups: Vec<ClassId> }`, or add a
   sibling map returned alongside the cache. `derived_sups` are *entailed*, not
   candidates.
4. **Inject into the hierarchy** in `classify.rs`, mirroring the defined-SUB
   sweep's direct push (`classify.rs:1833-1836`): after the label cache is built
   (`classify.rs:1256-1301`), for each class `c` and each `D ∈
   derived_sups(c)`, if `!closure.contains(c, D)` and `!direct_supers[c]
   .contains(D)`, do `direct_supers[c].push(D); direct_children[D].push(c);`
   and bump a `stats.backfold_recovered` counter. **No `subsumes_via_tableau`.**
   The transitive-closure BFS at `classify.rs:1866-1887` then propagates it into
   the entailment matrix like any other direct super.

Change footprint: 1 new precomputed field + its builder; 1 new engine method
(reuses `distinct_role_succ`); 1 enum-variant field; ~10 lines of injection in
`classify.rs`. All flag-gated (§6).

**B. Feed the merge fact into the saturation closure and re-saturate.** Then the
saturator's existing CR5 derives `TT ⊑ TICE` and it is found by
`closure.contains` (`classify.rs:1631/1969`) with no label machinery at all —
elegant, but requires extracting global merge subsumptions (`Tibia ⊑
TibialPlateau`) from the wedge and re-running saturation. Bigger, cross-engine,
and the merge is computed per-sat-graph, not globally. **Not smallest.** Note
for the future, not v1.

**C. In the prune gate itself.** Rejected: the gate's only lever is
"prune vs. verify", and the verify hangs (§0). Widening the gate cannot help.

The filler subsumption (`Tibia ⊑ TibialPlateau`) is **merge-derived**, so the
recognition MUST run after the merge facts are on the graph — i.e. after the
Horn fixpoint of `sat(c)` completes. Site A satisfies this by construction (it
reads the post-sat graph). This is also why B needs the merge fact *first*.

---

## 5. Termination & confluence

- **Termination:** BACKFOLD is monotone label addition over a *finite,
  already-completed, blocking-bounded* graph and a finite set of defined bodies.
  Root-only single pass is trivially terminating. The optional all-nodes
  fixpoint terminates because each iteration only adds labels from a finite
  universe and never removes; bounded by #nodes × #defined-classes. It runs
  *after* the sat fixpoint, so it cannot perturb the sat's own termination
  (blocking, deadline) — it is strictly post-hoc read-only-plus-derive.
- **Confluence:** the rule adds `D` whenever the structural precondition holds;
  the precondition is monotone in the label set, so the least fixpoint is unique
  and independent of application order (standard monotone-closure confluence).
  Adding `D` at `x` can enable another body whose `Aᵢ = D` — handled by
  iterating to fixpoint; order-independent. No interaction with backjumping
  because the derivation runs only on `branches_taken == 0` (no decisions to be
  confluent *with*).

---

## 6. Risk + test strategy

**Regression risk — could it over-generate and blow up some other ontology's
wall?** Low, and bounded by construction: the back-fold makes **zero** search
calls, so it cannot burn deadlines the way `DEFINED_SWEEP` does. Its only cost
is the O(#defined-`∃` × successors) structural scan per class. The one real risk
is an ontology with *many* defined-`∃` classes where the scan itself is
non-trivial (galen's 681 is already near the top of the corpus) — mitigate by
indexing bodies by a required atomic conjunct (the "genus") so only bodies whose
genus is in `labels(x)` are scanned. FP risk is nil under the branch-free /
`EMPTY`-deps gate (§2a); if that gate is ever wrong, the failure is an FP, so the
gate is the load-bearing invariant and must be unit-tested directly.

**Gating.** Ship behind `RUSTDL_CLASSIFY_BACKFOLD` (default OFF for the first
landing; flip to ON only after the corpus gate below is green). Flag-off path
must be byte-identical: no `defined_exists_bodies` build, no engine method call,
`LabelOracle::Sat` behaves exactly as today.

**Gates (all must hold before default-ON):**
1. **galen MISSED 1 → 0** (the target; `TT ⊑ TICE` now `direct`/entailed).
2. **corpus FP = 0** — closure-diff vs the Konclude∩HermiT oracle across
   galen, notgalen, sio, wine, ore-10908, ore-15672, alehif, ro, pizza,
   shoiq-knowledge. This is THE gate (the back-fold's soundness is the whole
   risk surface).
3. **no new MISSED** anywhere (byte-identical closures except galen's +1).
4. **wall not materially worse** — galen classify stays ~sub-second (was
   0.85 s); no ontology regresses beyond ±10%. If any does, it is the scan cost,
   fixed by the genus-index. Explicitly: it must **not** behave like
   `DEFINED_SWEEP` (>6:40).

**Unit canaries (negatives-first):**
- The two minimal repros from the residual doc (told-filler and merge-derived
  filler) as positive `#[test]`s that assert `TT ⊑ TICE` via the classify path
  (not just the single-pair API), so we lock in that the *label cache* now
  carries it.
- A branch-free-gate canary: an ontology where `sat(c)` **branches** and the
  structural precondition holds in the chosen branch but the subsumption does
  **not** hold — assert the back-fold does **not** derive it (guards §2a; this
  is the FP tripwire).
- A blocking canary: deep `∃`-chain where the witness filler is on a blocked
  node — assert no crash and (acceptable) MISS, never FP.

---

## 7. Step outline (for the implementation plan)

1. **Precompute** `HyperCache.defined_exists_bodies` in `HyperCache::build`
   (`lib.rs:2016+`) from `extract_definitions` (already at `:2020`); only
   ≥1-`∃`-conjunct conjunctive bodies. Add a genus index (required atomic
   conjunct → body list).
2. **Engine method** `HyperEngine::backfold_derived(root, &bodies, sub_roles)`
   in `hyper.rs` near `:2456`, gated on `branches_taken == 0` (or per-label
   `EMPTY`-deps), reusing `resolve` + `distinct_role_succ`. Returns `Vec<ClassId>`.
3. **Wire into `classify_labels`** (`lib.rs:2622-2628`): on `HyperResult::Sat`,
   call `backfold_derived`, return `LabelOracle::Sat { labels, derived_sups }`.
4. **Extend `LabelOracle::Sat`** (`lib.rs:1871`) with `derived_sups`; update the
   two prune sites (`classify.rs:1678`, `:1976`) to keep reading `labels` exactly
   as today (they ignore `derived_sups`).
5. **Inject** into the hierarchy in `classify.rs` right after the label cache
   build (`~:1303`), mirroring the defined-SUB sweep push (`:1833-1836`): direct
   `direct_supers`/`direct_children` edges for each entailed `derived_sups`,
   guarded by `!closure.contains` and dedup; add `stats.backfold_recovered`.
6. **Flag** `RUSTDL_CLASSIFY_BACKFOLD` (`reasoner/src/lib.rs`, alongside
   `classify_defined_sweep_enabled` `:1621`); default OFF → ON after gates.
7. **Tests**: canaries in §6; run the corpus closure-diff gate (§6.2/3) and the
   galen MISSED gate on a freshly built `--release` binary (per the toolchain
   gotcha in CLAUDE.md).

---

## 8. Honest bottom line

The fix **is** targeted and small — but the triage's framing ("sound
over-approximation → per-pair verify filters it") is **wrong about the verify**:
the `¬TICE` verify hangs (§0, measured), so a wider candidate set + verify is
either a timeout MISS or the `DEFINED_SWEEP` explosion. The *correct* fix is a
**sound direct derivation** — EL `∃`-composition run over the wedge's
already-built, branch-free, merge-enriched `sat(c)` graph — injected into the
hierarchy with **zero** tableau calls, exactly like the existing defined-SUB
sweep. That is what makes it both close the pair and stay sub-second. The
load-bearing soundness invariant is the branch-free / `EMPTY`-deps gate; the
load-bearing test is the corpus FP=0 closure-diff. Everything else is
mechanical. Recommend implementing option **A** behind
`RUSTDL_CLASSIFY_BACKFOLD`.
