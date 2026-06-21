# `rustdl justify --laconic` — fine-grained (laconic) justifications (design)

**Date:** 2026-06-21
**Status:** approved (brainstorming) → ready for implementation plan
**Branch:** `feat/laconic-justifications`

**Sub-feature C** of the explanation/debugging suite (after `justify`/`prove` and
the `diagnose` command — sub-project A — shipped). Companion sub-features still out
of scope here: **B** repair suggestions, **D** visual rendering, and the
precise/atomic-split mode.

## Goal

Pinpoint the responsible *part* of each axiom, not just the whole axiom. A regular
justification is a minimal set of ontology axioms; a laconic justification weakens
each to its responsible fragment — e.g. when `C ⊑ B ⊓ D ⊓ ∃r.E` is in a
justification of `C ⊑ B`, report just `C ⊑ B`, dropping the superfluous conjuncts.
This is Horridge/Parsia/Sattler-style laconic justification, restricted to a sound,
bounded set of structural weakening operators (the "structural splitting" flavor).

## Soundness framing

FP=0 is sacred. A laconic justification reports *weakened* axioms that are not
literally in the ontology, so the contract is:

1. **Every reported fragment is entailed by an original ontology axiom** (e.g.
   `C ⊑ B ⊓ D` entails `C ⊑ B`). Hence the laconic set is a set of genuine
   consequences of the ontology, and it genuinely explains `q`. `diagnose`/`justify`
   add no new entailments *about* the ontology; FP=0 is untouched by construction.
2. **The result actually entails `q`.** QuickXplain re-verifies `background ∪ subset
   ⊨ q` through the same oracle, so a laconic justification is never a spurious
   explanation.

## Architecture & placement

- New module `crates/owl-dl-reasoner/src/laconic.rs` — the axiom **weakening**
  operators plus the laconic algorithm. Public entry points:
  `find_laconic_justification(onto, q)` and
  `find_all_laconic_justifications(onto, q, max)`.
- **Reuses, does not reinvent:** `justify::{find_one_justification,
  find_all_justifications, quickxplain, logical_axioms, ontology_from, Entailment,
  Justification}`. The weakening operators are the only new logic; QuickXplain does
  the minimization and the oracle does entailment.
- CLI: a `--laconic` flag on the existing `justify` handler in `owl-dl-cli`,
  composing with `--all` / `--max` / `--labels`.

Units with one responsibility each: `weaken(axiom) -> Vec<fragments>` is a pure
function of a single axiom; the laconic driver is a pure function of (regular
justification, background); the CLI formats. Each is independently testable.

## Algorithm

```
1. J ← regular justification(s) of q          (existing find_one / find_all)
2. for each axiom a ∈ J: fragments(a) ← weaken(a)   [entailed weaker pieces]
3. background ← the non-logical fixed set (DECLARATIONS ONLY)
4. candidates ← ⋃ fragments(a) for a ∈ J
5. laconic ← quickxplain(background, candidates, q)  [minimal responsible fragments]
```

**Background = declarations only (not the rest of the ontology).** `J` is a minimal
regular justification, so `J` alone (with declarations) entails `q`; we explain `q`
using only weakenings of `J`'s axioms. Putting the *other* logical axioms in the
background would let an alternative derivation entail `q` on its own, collapsing the
laconic result to `∅`. This also mirrors how `find_one_justification` itself calls
`quickxplain` (fixed = non-logical declarations, candidates = the axioms under
consideration). Every supported weakening operator is **entailment-preserving** (the
fragment set is *set-equivalent* to `J` — see below), so `background ∪ candidates ⊨
q` holds and QuickXplain's precondition is met. A debug-only assertion re-checks this
invariant so any future non-preserving operator is caught immediately.

For `find_all_laconic_justifications`, apply steps 2–5 to each regular justification
returned by `find_all_justifications` (cap by `max`), de-duplicating identical
laconic results.

## Weakening operators (v1)

Each operator emits only fragments **entailed by** the original axiom (the soundness
contract). `weaken` recurses structurally.

The operators are **entailment-preserving**: for each, the *set* of fragments is
set-equivalent to the original axiom (not merely individually entailed by it). This
is what guarantees the candidate set still entails `q`.

| Axiom (input) | Weakens to | Set-equivalent because |
|---|---|---|
| `C ⊑ D₁ ⊓ … ⊓ Dₙ` | `{ C ⊑ Dᵢ }` | `X ⊑ D₁⊓…⊓Dₙ` iff `X ⊑ Dᵢ` for all `i` |
| `C ≡ D₁ ⊓ … ⊓ Dₙ` (general equiv) | all ordered pairs `Cᵢ ⊑ split(Cⱼ)` | equivalence ⇒ pairwise subsumptions; conjunction split as above |
| `DisjointClasses(C₁ … Cₙ)`, n>2 | pairwise `DisjointClasses(Cᵢ, Cⱼ)` | n-ary disjointness ≡ all pairwise disjoint |
| nested `⊓` on the RHS | recurse the conjunction split | composition |

`weaken` for `EquivalentClasses(C₁ … Cₙ)` emits, for every ordered pair `(i, j)`,
`Cᵢ ⊑ f` for each conjunction-split fragment `f` of `Cⱼ` — so `C ≡ D` →
`{C ⊑ D, D ⊑ C}` and `C ≡ D⊓E` → `{C ⊑ D, C ⊑ E, (D⊓E) ⊑ C}` uniformly.

An axiom with no applicable operator (a plain `C ⊑ D`, a `DisjointUnion`, a
domain/range or property axiom) **passes through unchanged** — it is its own only
fragment.

**Deliberately NOT weakened in v1 (a sound subset of Horridge's operators):**
- **Existential-filler narrowing** (`∃r.(D⊓E) → ∃r.D`). Although each narrowed
  fragment is *individually* entailed by the original, the fragment **set** is NOT
  set-equivalent to it (`{C⊑∃r.D, C⊑∃r.E}` is strictly weaker than `C⊑∃r.(D⊓E)` —
  the successors need not coincide). A non-set-equivalent operator can make the
  candidate set fail to entail `q`, violating QuickXplain's precondition. So
  `split_sup` splits conjunctions ONLY; existential fillers are atomic. (A future v2
  could re-introduce narrowing behind a real precondition-check fallback.)
- The **LHS** of a subsumption. Dropping a left conjunct *strengthens* the axiom
  (`C₁ ⊑ D` is stronger than `C₁ ⊓ C₂ ⊑ D`), which is **not entailed** by the
  original — so LHS splitting would be unsound. LHS stays whole.
- **Cardinality weakening** (`≥3 r.C → ≥1 r.C`), `∀`-fillers, property-hierarchy
  weakening, datatype-range weakening. Sound in principle but deferred.

Consequence: the result is **laconic (structural)** — sound and minimal among the
supported weakenings, but not provably maximally-weak. This limitation is stated in
the output (see honesty flag).

## Output & honesty flag

The CLI prints the weakened fragments in Manchester syntax (reusing the justify
renderer and `--labels` glossing) under a `# laconic justification (N axioms)`
header. The result carries an honesty note analogous to the existing
`Justification::minimal_guaranteed`:

- fragments are **sound** (genuine consequences of the ontology), and
- **minimal among the supported weakening operators** (QuickXplain), but
- because the operator set is bounded, a fragment may still contain a non-minimal
  sub-part outside the supported operators → "laconic (structural)", not provably
  maximally-weak.

The `Justification` struct (or a thin `LaconicJustification` wrapper) keeps the same
`fragment` / `minimal_guaranteed` fields plus a marker that this is a laconic result,
so the renderer can label it accordingly.

## Testing

- **Unit — weakening operators** (`laconic.rs`): each operator on synthetic axioms —
  RHS-conjunction split, `∃`-filler split, `C ≡ D` → two subsumptions, `C ≡ ⊓Dᵢ`
  split, pairwise `DisjointClasses`, pairwise `EquivalentClasses`, and **nested**
  `C ⊑ E ⊓ ∃r.(F ⊓ G)`. **Negative controls** that must pass through unchanged: a
  plain `C ⊑ D`; an LHS conjunction `C₁ ⊓ C₂ ⊑ D` (LHS not split); a cardinality
  axiom (`≥n r.C` not weakened); a `∀`-filler.
- **Soundness property test:** for every fragment emitted by `weaken(a)`, assert
  `{a} ⊨ fragment` (each fragment is genuinely entailed by its source axiom) via the
  oracle.
- **Integration:** a crafted ontology where `A ⊑ B ⊓ C ⊓ ∃r.D` is in the
  justification of `A ⊑ B` → `find_laconic_justification` returns exactly `{A ⊑ B}`
  (superfluous conjuncts dropped); an equivalence case (`C ≡ D ⊓ E`, query `C ⊑ D` →
  laconic `{C ⊑ D}`); a disjointness case.
- **End-to-end re-verification:** for every laconic result, assert
  `ontology_from(background, laconic) ⊨ q` (it must still entail `q`).
- **Corpus:** `justify --laconic` on a fixture with real multi-conjunct axioms (e.g.
  pizza `unsat CheeseyVegetableTopping`, or a SIO subsumption). Assert (a) the
  laconic result still entails `q`, (b) it is no larger in "logical content" than the
  regular justification (each laconic fragment is entailed by some regular-J axiom),
  (c) FP=0 / no crash, (d) classification closure byte-identical (read-only).

## Out of scope (v1)

- Full Horridge operator set (cardinality / LHS / property weakening + the OPlus
  minimization) — the result is the sound structural subset.
- `diagnose --laconic` composition — `diagnose` keeps whole-axiom root
  justifications for now; a later change can route its root justification through the
  laconic path.
- Precise/atomic-split mode and visual rendering (sub-feature D).
