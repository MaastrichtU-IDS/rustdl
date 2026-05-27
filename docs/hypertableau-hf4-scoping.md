# HF4 — nominals as true singletons (scoping)

Drafted 2026-05-27. HF1–HF3 done. HF4 makes `{a}` a true **singleton**
(the NN-rule + nominal merging), the deepest SROIQ interaction. Part of
[`hypertableau-full-scoping.md`](hypertableau-full-scoping.md) §HF4.

## §0 — Current state

The clausifier treats a nominal `{a}` as an ordinary atomic class in the
reserved region `[nominal_base, nominal_base + num_individuals)` — a
sound under-approximation that **loses the singleton constraint**
(`{a}(x) ∧ {a}(y) → x = y`). `Atom::Equal` is a no-op. So a constraint
that two `{a}`-fillers must coincide is silently dropped (sound for
`Unsat`, incomplete).

## §1 — HF4a (shipped): the NN-rule

`apply_nn_rule`, run in the Horn fixpoint on the triggering `Label`
event: when node `n` gains a singleton nominal `c`, any *other* node
(canonical, `≠ n`) carrying `c` is the same individual and is merged
into `n` via the existing `merge` — which clashes if they are `≠`. The
engine learns the nominal range via `with_nominals(start, count)` /
`is_nominal` (mirroring HF2's `with_sub_roles`); the probe passes
`[num_classes, num_classes + num_individuals)`.

**Composes with HF3a.** `≥2 R.{o}` generates two `≠` successors both
`{o}`; the NN-rule merges them; the `≠` clashes ⇒ unsat. That is the
canary `hyper_subsumption_probe_nominal_singleton_cardinality`
(`A ⊑ ≥2 R.{o}` ⊨ `A ⊑ B` because `A` is unsat). Over-merge guard:
`≥1 ⊓ ≤1 R.{o}` is Sat (one successor, no merge) — the NN-rule fires
only on *distinct* same-nominal nodes.

**Soundness:** merging same-nominal nodes is semantically forced, so it
only *adds* clashes — `Unsat` stays sound. **Termination:** each merge
drops node count; finite individuals bound nominal classes; count-based
`≥n` honors the merged count, no regen. Verified: unsat + sat canaries
pass; pizza 695 / ro 158 / sulo 51 unchanged, 0 FP (pizza's
`RealItalianPizza ⊑ ∃hCO.{Italy}` path — single `{Italy}`-successor
reused via `∃` witness — is undisturbed); SIO 0.92 s, 1585 sat/0 unsat.

## §2 — HF4b (deferred): the hard cousins

Marked `TODO(HF4b)` in `apply_nn_rule`, not built (corpus doesn't
exercise them):
- **Nominal-under-`∀` propagation:** `∀R.{o}` seeding `{o}` onto an
  *existing* successor, then NN-merging it. The HF4a canary seeds the
  nominal directly via the `≥n` qualifier, not via `∀`-propagation.
- **Nominal-aware blocking:** nominal nodes represent fixed individuals
  and shouldn't be blocked; the canary's generated nominal successors
  aren't blocked (their labels aren't a subset of the root's), so the
  gap doesn't bite here.
- **Multi-predecessor in-edge redirect:** NN-merging two nominal nodes
  that each have their own predecessor leaves stale in-edges (the same
  gap `merge`'s doc already disclaims, and that HF2 deferred). The
  canary is tree-shaped (both successors' sole predecessor is the
  root).

## §3 — Out of scope / next

HF5 wires the engine as the complete classifier (trust `Sat`,
both-direction Konclude agreement) — depends on HF4b + HF2 double-
blocking for full `Sat` soundness. Datatypes, SWRL: separate.

## §4 — Honesty

This is **HF4a**, the first valid increment, not all of HF4. The full
"nominal merging is a fixpoint" capstone (HF4b) plus its interaction
with cardinality and inverses is the remaining depth.
