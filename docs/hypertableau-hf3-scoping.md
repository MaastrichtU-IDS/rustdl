# HF3 — cardinality fully in the calculus (scoping)

Drafted 2026-05-27. HF1 (sound clausifier) and HF2 (inverse roles, RBox
inverse pairs, role hierarchy) are done. HF3 makes **cardinality** a
first-class calculus citizen, dropping the H3c root-only scope cut.
Part of [`hypertableau-full-scoping.md`](hypertableau-full-scoping.md) §HF3.

## §0 — Current state

- **`≤n` works** (H3c): `Atom::AtMost` is stored on the node
  (`at_most`), `find_open_at_most` flags a node with more distinct
  `role`-successors than `n`, and `solve` branches by `merge`-ing one
  pair per branch (union-find `representative`). Scope cut: this is
  sound, but merge does not redirect in-edges and the `≥n` side is
  absent.
- **`≥n` is a NO-OP** (`Atom::AtLeast` → `FireOutcome::NoChange`). The
  clausifier *emits* `AtLeast` (HF1), but the engine never generates
  successors. So `≥n` constraints are silently dropped — sound for
  `Unsat` (an unenforced head only weakens the theory) but incomplete.
- **No `≠` (distinctness) tracking.** Successors created by `∃`/`≤n`
  are merge-able freely; nothing records "these two must stay distinct."

## §1 — The pieces (forced sub-order)

### HF3a — `≥n` generation + `≠` tracking (this phase's core)

- `Atom::AtLeast(role, qual, n, var)` asserted at node `x` ⇒ ensure `x`
  has `n` pairwise-**distinct** `role`-successors satisfying `qual`.
  Generate fresh successors (edge `x —role→ new`, seed `qual`) up to
  `n`, and record them pairwise `≠`.
- **`≠` store:** an explicit inequality relation between nodes (a
  symmetric set of `(HNode, HNode)` pairs, resolved through the merge
  union-find). Generated `≥n` successors are pairwise `≠`.
- **Soundness (Unsat-only, preserved):** `≥n` generation creates
  successors that exist in *every* model; marking them `≠` is sound
  (`≥n` requires `n` distinct). So this only *adds* genuine clashes —
  `Unsat` stays sound. (Same discipline as inverse propagation.)
- **`≤n` ⋈ `≠`:** the merge rule must refuse to merge two `≠`-related
  nodes — a forced such merge is a **clash** (can't fit `n+1` pairwise-
  distinct fillers under `≤n`). This is what makes `≥2 ⊓ ≤1` unsat.

### HF3b — termination (`≤`-before-`≥` ordering)

- Cyclic `≥` (e.g. `A ⊑ ≥2 R.A`) must terminate. Two guards:
  1. **Generate only when needed:** `≥n` fires only if `x` lacks `n`
     distinct `qual`-successors — no regeneration.
  2. **Blocking gates generation:** a *blocked* node generates no
     successors (the existing `is_blocked` check that already gates
     `fire_exists`).
  3. **Rule priority:** apply `≤n` merge before `≥n` generation so a
     generate→merge→regenerate loop can't form (Motik et al. ordering).
- **Gate:** a cyclic `∃R.∃R… + ≤n` ontology terminates under the right
  order and loops under the wrong one (a pinned regression test).

### HF3c — qualified `≤n.C` / `≥n.C`

- Qualifiers on both sides (the `qual: Option<ClassId>` is already
  threaded; `≥n.C` seeds `C`, `≤n.C` counts only `C`-successors).
- **Gate:** a qualified `≤n.C` ontology matches Konclude; pizza
  `InterestingPizza` (`≡ Pizza ⊓ hasTopping min 3`) derives correctly
  **via the real calculus**, not the H3c `¬sup` shortcut.

## §2 — First increment + canary

**HF3a, gated by the minimal `≥2 ⊓ ≤1` clash.** Canary (sat probe, since
it's an unsatisfiable class, not a subsumption):
`A ⊑ ≥2 R.⊤`, `A ⊑ ≤1 R.⊤` ⊨ `A` unsat. Today `A` is reported **sat**
(≥n dropped); HF3a must report **unsat** (2 distinct successors, ≤1
merge forced, `≠` clash). Write it failing first.

## §2.5 — HF3a shipped

`≥n` generation lives in `generate_at_least` (called from
`apply_head_atom` — deterministic, in the Horn fixpoint, **not**
`solve`). `≠` is an engine-level `neq` store keyed through the merge
union-find (`add_neq`/`are_neq`), captured by save/restore; `merge`
returns a clash on a `≠` pair and the `≤n` solve-loop skips `≠` pairs.

**Generation guard — count-based, not fire-once (a measured fix).**
The first cut gated generation by fire-once only. That **regressed
pizza 695 → 682**: the 13 lost were exactly the cardinality pairs
(`X ⊑ InterestingPizza`/`NonVegetarianPizza`), all moved `Unsat →
Stalled` (stalled 2 → 15). Cause: `InterestingPizza` already has its
≥3 toppings via `∃`, so fire-once generated 3 *redundant* fresh
successors → the `≤2` merge tree ballooned past the depth/branch
budget. Fix: gate by **`distinct_role_succ(x, role, qual).len() >= n`**
(don't generate if already satisfied), keeping fire-once as a
secondary regen guard. Pizza restored to 695 (stalled back to 2), SIO
0.45 → 0.91 s (bounded, no blowup), all corpus 0 FP. **Lesson: a
sound rule can still regress completeness via search blowup → budget
→ Stalled; the corpus diff is the guard.** Canaries: `≥2 ⊓ ≤1` unsat,
`≥2 ⊓ ≤2` sat (off-by-one pin), cyclic `≥2 R.A` sat (termination).

## §3 — Out of scope

Nominals/NN-rule (HF4 — where `≥n` under a nominal forces new nominals);
double-blocking (HF2 SAT lever, deferred); datatypes. The `≠` store and
generation built here are what HF4's nominal cardinality will reuse.

## §4 — Risk

`≥n` generation + `≠` + termination is the second-hardest interaction in
SROIQ (after nominals). The mitigations are the Unsat-only soundness
(generation only adds clashes), the forced sub-order (HF3a clash before
HF3b termination before HF3c qualified), and a termination regression
test pinned before HF3b ships.
