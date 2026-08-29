# `chainrange` adjudicated: a real gap, and Konclude misses it too

**Question.** Issue #81's reproducer `chainrange.ofn` (`Chain(t,u) ⊑ r`, `Range(r,F)`, `F ⊑ G`,
`C ⊑ ∃t.∃u.A`, `∃t.∃u.F ⊑ D` ⊨ `C ⊑ D`) survived the #81 fix unchanged. Is it the same
mechanism as `cascade.ofn`, and is the entailment even real?

**Answer: a different mechanism, and the entailment is real — but Konclude does not derive it,
so the adjudication had to go to HermiT.**

## Why this needed adjudicating at all

Konclude reports **nothing** on `chainrange.ofn` — the same answer rustdl gives. Read naively
that is peer agreement, i.e. "no gap". But Konclude is documented to under-report, so silence is
ambiguous. The discriminating control settles that: with the chain removed and `Range` put
directly on `u`, **Konclude does report `C ⊑ D`** (and so does post-#81 rustdl). Konclude reports
the row when it can derive it, so its silence here is not a reporting convention.

## The probe ladder

| probe | shape | derivation | HermiT | Konclude | rustdl |
|---|---|---|---|---|---|
| control | `Range(u,F)`, no chain | `C ⊑ D` | — | **`C ⊑ D`** | **`C ⊑ D`** (via #81) |
| p1 | chain, no range, `∃r.A ⊑ D` | `C ⊑ D` | — | **`C ⊑ D`** | **`C ⊑ D`** |
| p2 | chain + range, `∃r.F ⊑ D` | `C ⊑ D` | — | *nothing* | *nothing* |
| p3 | chain + range + `Disjoint(A,F)` | `C` unsat | **`Nothing ≡ C`** | `C ⊑ Thing` | *nothing* |
| p4 | p3 with **asserted** `t(a,b)`, `u(b,c)` | inconsistent | — | **inconsistent** | **inconsistent** |
| p5 | p3 with `x : C` (**generated** witnesses) | inconsistent | **inconsistent** | *consistent* | **inconsistent** |
| chainrange | the issue's fixture | `C ⊑ D` | **`C ⊑ D`** | *nothing* | *nothing* |

p1 shows both reasoners apply the chain to generated witnesses. p4 shows both propagate a range
onto a chain-derived edge between **asserted** individuals. p2/p3/p5 isolate the gap to exactly
one combination: **a range propagating onto a chain-derived edge between GENERATED witnesses.**

## Findings

1. **`C ⊑ D` is entailed.** HermiT derives it directly on `chainrange.ofn`, and independently
   reports `EquivalentClasses(Nothing, C)` on p3 and `InconsistentOntologyException` on p5. That
   matches the hand derivation: `t(x,y) ∧ u(y,z) ⟹ r(x,z)` by the chain, `z ∈ F` by the range,
   so `x ∈ ∃t.∃u.F ⊑ D`.

2. **Konclude is incomplete on this pattern.** It misses p2, p3, p5 and `chainrange` while
   getting p1 and p4 right. This is the 5th recorded Konclude under-report in this project — but
   note it is a *stronger* claim than the previous four, because p5 is a **consistency** question
   where Konclude answers `consistent` and both HermiT and rustdl answer `inconsistent`.

3. **rustdl's gap is confined to the TBox classification path.** rustdl gets p4 AND p5 right
   (its `abox_saturation` materialises chains and applies ranges), and **beats Konclude on p5**.
   What it misses is the classification form: no chain-induced range reaches a generated witness
   during saturation.

4. **It is NOT the `cascade` mechanism**, so the #81 comment that said it "needs a range folded
   into a nested witness, exactly as `cascade` does" is wrong. `cascade` needed
   `effective_ranges` threading into two lowering sites; that shipped, and `chainrange` did not
   move. Folding into `effective_ranges[u]` would be **unsound** here: a bare `u`-successor with
   no `t`-predecessor is not an `r`-successor and must not inherit `Range(r)`. Closing this needs
   a chain-aware derivation rule, not an extras fold.

## Method note

The `grep -o … | head -1 || echo none` idiom silently reported "no result" because the pipeline's
exit status is `head`'s, not `grep`'s — the same class of error as the recorded `grep -c` trap,
and it nearly turned Konclude's real answer into a fabricated one. Both Konclude output files
were checked for size and content before any verdict was read from them, and p1 (a case Konclude
gets right) served as the in-band control proving the harness worked.
