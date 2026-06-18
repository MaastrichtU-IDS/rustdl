# Wedge inverse/symmetric domain-range firing — design (SP1)

**Date:** 2026-06-18
**Status:** approved (brainstorming session 2026-06-18; root cause corrected
post-code-investigation 2026-06-18)
**Program:** "Konclude-class engine" (sub-project 1 of 3)
**Artifacts:** `docs/family-mech4-ddmin-core.ofn` (the 15-axiom inconsistent
family core, ddmin'd with the Konclude oracle — SP1's headline gate).

---

## 1. Program context (the Konclude-class engine decomposition)

The user's goal is a Konclude-class complete reasoning engine. The realization
is **"evolve the wedge"**, not rewrite: the hypertableau wedge (`hyper.rs`) is
already anywhere-blocked, Horn-fixpoint-driven, dependency-directed-backjumping,
and (since the 2026-06-15 HF3 work) handles role chains/RIAs and
forward-functional-merge. Decomposition into three gated sub-projects:

| SP | Scope | P0 gate |
|---|---|---|
| **SP1 (this doc)** | inverse + symmetric domain/range firing in the wedge | 15-axiom family core → inconsistent; corpus FP=0/MISSED=0 |
| SP2 | scale: efficient transitive closure (∀-propagation) + disjunctive-search efficiency | full family/family-stripped → inconsistent in seconds; corpus MISSED=0; no perf regression |
| SP3+ | remaining items (inverse-functional predecessor-merge; perf tail) | per-item |

### P0 gate findings (all Konclude-oracle-confirmed)

- **`family` is a calculus gap, not pure scale.** The wedge *completes and
  returns `Sat`* on a **15-axiom inconsistent core** (ddmin, 580 oracle calls).
- **The gap is inverse/symmetric domain/range firing.** Chains (H2/H3),
  forward-merge on ABox (`mech3`) and generated (`mech4a`) nodes all work.
  Minimal reproducers (Konclude inconsistent, wedge says consistent):
  - **tinv** (syntactic): `ObjectPropertyDomain(ObjectInverseOf(p), C)` + `p(a,b)` + `b:D` + `Disjoint(C,D)`.
  - **trinv**: same with `ObjectPropertyRange(ObjectInverseOf(p), C)`.
  - **H4**: declared `InverseObjectProperties(p,q)` + `ObjectPropertyDomain(q,C)` + `p(a,b)` + `b:D` + `Disjoint(C,D)`.
  - **H1a**: explicit `SymmetricObjectProperty(p)` + `ObjectPropertyDomain(p,C)` + `p(a,b)` + `b:D` + `Disjoint(C,D)`.
  - **H1**: self-inverse `InverseObjectProperties(p,p)` + `ObjectPropertyDomain(p,C)` + …
- **`family` *also* has a scale problem** (transitive closure + disjunctive
  branching depth 256) — independent, deferred to SP2.
- **Stale-doc corrections:** role-chains/RIAs (HF3) + forward-functional-merge
  already shipped and working.

---

## 2. Goal & non-goals

### Goal

Make the wedge fire `domain`/`range` (and the equivalent single-role-body
clauses) through **inverse** and **symmetric** roles, so the clause types the
correct node. Closes the calculus half of `family` (the 15-axiom core →
`inconsistent`) and a general SROIQ completeness gap.

### Reach (scoping note, added post-implementation)

SP1's firing is live on the paths that build the wedge with the role hierarchy
(`with_sub_roles`): **consistency checking** (`ConsistencyCache::decide`) and the
diagnostic subsumption probe (`hyper_subsumption_probe`). The default per-pair
**classification** subsumption oracle (`HyperCache::decide` / `classify_labels`)
currently builds its engine with `sub_roles = None`, so it does **not** carry the
inverse/symmetric domain/range firing. This is **FP-safe** (the `None` path
under-fires → fewer Unsats → never a spurious subsumption) and corpus-neutral
(MISSED=0 corpus-wide — no corpus subsumption depends on inverse/symmetric
domain/range). Extending the firing to the classify oracle (pass `Some(&hierarchy)`
in `HyperCache::build`) is a clean, FP-safe follow-up (**SP1.1**), gated on the
`with_sub_roles`/prebuilt-`base_indexes` interaction (today no caller combines
`new_with_prebuilt` + `with_sub_roles`; SP1.1 would, so the index-rebuild must
preserve the Q-clause delta). Deferred so the classify hot-path change gets its
own corpus re-validation.

### Non-goals

- **Scale.** Full `family` still stalls (SP2). SP1's gate is the 15-axiom core.
- **Main tableau, saturator, classification, justify.** Wedge-only.
- **`AsymmetricObjectProperty`** (a clash condition, handled elsewhere).
- **Inverse-functional predecessor-merge** (SP3).

### Ship/revert criterion

Ships iff: the expanded motif passes, the 15-axiom core is `inconsistent`, the
corpus closure-diff net holds at FP=0/MISSED=0, and `role_matches`/edge-add
shows no measurable perf regression on GALEN/SIO. Any corpus FP → immediate
revert (FP=0 is sacred).

---

## 3. Root cause (confirmed by code investigation 2026-06-18)

The bug is in the **event-triggering layer**, not matching. `enumerate_matches`
(`hyper.rs:2182-2209`) already matches inverse first-leg atoms — it checks the
node's `preds` (incoming edges) via `role_matches(er.flip(), role, hier)`. The
defect is that the clause is **never fired at the node where the match lives**.

Domain/range clausify (clause.rs:303-316) to a single role-body clause anchored
at the home variable `X`:

- `domain(R,C)` → body `[Atom::Role(R, X, y)]`, head `[Atom::Class(C, X)]`
- `range(R,C)` → body `[Atom::Role(R, X, y)]`, head `[Atom::Class(C, y)]`

`canon_role` preserves an inverse role (`Inverse(p)` stays `Inverse(p)`), and a
declared `InverseObjectProperties(p,q)` rewrites `q → Inverse(p)`. So an inverse
domain becomes body `[Atom::Role(Inverse(p), X, y)]`.

Triggering (`hyper.rs:585-591` index, `1126-1159` `Event::Edge`): when edge
`src —p→ tgt` is added, the wedge fires `role_trigger` clauses **at `src`**
(forward first-leg) and `role_back_trigger` clauses at `preds(src)` (HF3
non-first legs, indexed only when `u != X`). A clause with an **inverse
first-leg** atom must fire at **`tgt`** (which now holds the matching incoming
edge) — and nothing triggers that. Hence the silent miss.

Two distinct sub-gaps fall out:

1. **Inverse first-leg** (`Inverse(p)(X,y)`): not triggered at the target.
   Covers tinv, trinv, and (via `canon`) declared `InverseObjectProperties(p,q)`
   (H4).
2. **Symmetric / self-inverse**: `SymmetricObjectProperty(p)` is not in
   `canon`, and self-inverse `InverseObjectProperties(p,p)` is mishandled by
   `canon` (it degenerately maps `p → Inverse(p)`). The family core's
   `hasAuntInLaw` is **self-inverse and load-bearing**, so this sub-gap must be
   closed too (H1a, H1).

---

## 4. Architecture

### 4.1 Part 1 (shared) — inverse first-leg triggering

`crates/owl-dl-tableau/src/hyper.rs`:

- New `ClauseIndexes` field `inverse_first_trigger: Vec<Vec<usize>>` (beside
  `role_back_trigger`).
- In the index builder (585-591): when a body atom is `Atom::Role(r, u, _)`
  with `u == X` and `r.is_inverse()`, push `ci` into
  `inverse_first_trigger[role_id_index(r)]`.
- In `Event::Edge(src, role, tgt)` (1126-1159): after the existing `role_trigger`
  / `role_back_trigger` firing, fire each `inverse_first_trigger[role_id_index(role)]`
  clause **at `tgt`** (`fire_clause(ci, tgt)`), with the same `FireOutcome::Clash`
  propagation.

This is sound (additive: a clause that should fire now does) and mirrors the
proven `role_back_trigger` mechanism.

### 4.2 Part 2 — symmetric / self-inverse (TWO variants, compared)

Both variants first need **symmetric-role detection** in
`build_role_hierarchy` (reasoner `lib.rs`): collect a `symmetric: Vec<bool>`
(indexed by `role_id`) from every `Axiom::SymmetricRole(p)` **and** every
self-inverse `Axiom::InverseObjectProperties(p, p)`. Store it inside
`RoleHierarchy` (owl-dl-core `role_hierarchy.rs`) so it rides the existing
`with_sub_roles` plumbing.

The variants differ in how a symmetric edge's reverse direction becomes
visible:

**Variant R — `role_matches` symmetric-awareness (no new edges).**
- `RoleHierarchy::is_symmetric(role_id) -> bool`.
- `role_matches(edge, wanted, hier)`: in addition to the existing logic, return
  `true` when `edge.role_id() == wanted.role_id()` **and** that role is
  symmetric — i.e. ignore the `is_inverse()` mismatch for symmetric roles. This
  makes the existing `preds`-flip path in `enumerate_matches` match an incoming
  symmetric edge against a forward symmetric body atom.
- Triggering: extend Part 1's index condition so a first-leg atom
  `Atom::Role(r, X, _)` is also added to `inverse_first_trigger` when
  `is_symmetric(r.role_id())` (whether `r` is `Named` or `Inverse`) — so a
  symmetric `domain(p,C)` clause fires at the target too.
- Cost: one extra branch in the hot `role_matches`; no graph growth.

**Variant M — symmetric edge materialization (no `role_matches` change).**
- When `add_edge(src, p, tgt)` is applied and `is_symmetric(p.role_id())` (and
  `src != tgt`), also add the reverse edge `add_edge(tgt, p, src)` through the
  sanctioned `TableauContext`/trail interface so it is undone on backjump. The
  existing forward triggering + matching then fire `domain`/`range` at both
  endpoints with no further change.
- Cost: extra edges (bounded — only for symmetric roles, non-transitive) +
  trail entries; must avoid infinite re-materialization (guard: do not
  re-add if the reverse already present; the symmetric of the reverse is the
  original, already present).

### 4.3 Touched files

- `crates/owl-dl-tableau/src/hyper.rs` — Part 1 (both variants); Variant R also
  edits `role_matches`; Variant M edits the `add_edge` path.
- `crates/owl-dl-core/src/role_hierarchy.rs` — symmetric set + accessor (both
  variants).
- `crates/owl-dl-reasoner/src/lib.rs` — `build_role_hierarchy` ingests
  `SymmetricRole` + self-inverse `InverseObjectProperties` (both variants).

---

## 5. Soundness argument (FP=0)

Additive and sound by construction. `InverseObjectProperties(p,q)` is
`p(x,y) ⟺ q(y,x)`; `SymmetricObjectProperty(p)` (and self-inverse) is
`p(x,y) ⟹ p(y,x)`. Firing a clause through an edge these entailments make real
adds only true atoms — the soundness shape of the shipped HF3 chain-edge
derivation. No model is trusted; no refutation weakened. Variant M materializes
only genuinely-entailed edges; Variant R matches only genuinely-entailed
relationships. Residual risk is implementation (over-trigger / over-match /
re-materialization loop), caught by negative-control motif tests + the corpus
closure-diff net.

---

## 6. Testing & gates

### 6.1 Expanded motif (new tests; each positive Konclude-oracle-confirmed)

Inverse (Part 1): tinv, trinv (syntactic), H4 (declared), inverse+range,
inverse × sub-role.
Symmetric (Part 2): H1a (explicit symmetric + domain), H1 (self-inverse +
domain), symmetric + range, symmetric × chain-leg.
Negative controls (must stay `consistent`): two unrelated roles with the same
domain shape but no inverse/symmetric declaration; domain on `p` with an edge on
an unrelated role `r`.

### 6.2 Family core gate

`docs/family-mech4-ddmin-core.ofn` (15 logical axioms) → `inconsistent`.
(The full `family.ofn` remains a sound MISS until SP2 — its stall is scale.)

### 6.3 Corpus closure-diff net (sacred)

FP=0 / MISSED=0 across galen, notgalen, sio, wine, ore-10908, ore-15672,
shoiq-knowledge, alehif, ro, sulo, pizza. Any FP → revert.

### 6.4 Perf

No measurable regression on GALEN/SIO (wall + flamegraph). Variant R: watch
`role_matches`; Variant M: watch edge count / graph size.

---

## 7. The bake-off (the user-requested comparison)

Part 1 (triggering) is shared and built first. Part 2 is built **twice** in
isolated git worktrees — Variant R and Variant M — each passing 6.1 (symmetric
motif) + 6.2 (family core) + 6.3 (corpus net) + 6.4 (perf). Compare on:

1. **Correctness** — both must pass the symmetric motif + family core
   identically (they are logically equivalent; a divergence is a bug).
2. **FP=0/MISSED=0** corpus net — both must hold; report any difference.
3. **Perf** — GALEN/SIO wall + the variant-specific signal (R: role_matches
   hot-path; M: graph/edge growth).
4. **Complexity / risk** — Variant M's trail/backjump interaction vs Variant R's
   hot-path branch.

Pick the winner on (FP=0 first, then perf, then simplicity); merge it; discard
the other worktree. If they tie on everything, prefer **R** (no graph growth,
better aligned with SP2's scale goal).

---

## 8. Open questions for implementation

- Inverse-equivalence + symmetry interaction with `canon` (the self-inverse
  `InverseObjectProperties(p,p)` is currently fed to `canon`): ensure the
  symmetric-set path takes precedence so the degenerate `p → Inverse(p)` canon
  rewrite for self-inverse roles is suppressed (or harmless). Pin with the
  self-inverse motif (H1).
- Variant M only: confirm the reverse-edge add routes through the trail so
  backjumping undoes it, and that the no-duplicate guard prevents a
  re-materialization loop.
