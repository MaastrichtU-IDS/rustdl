# Diagnosis: notgalen's residual 18 MISSED (2026-06-05)

**Goal.** Pursue the "actual completeness path" — make the 18/20 corpus MISSES
that are notgalen recoverable. This doc is the diagnosis gate before any code.

**Verdict: stop and report. These are NOT the 2d-proven mechanism.** The
residual is the functional-role-witness-merge completeness frontier (Phase 2a
territory, 0 prior corpus impact), soundness-sensitive, multi-phase. My earlier
"same shape GALEN recovered, proven forward-saturator approach, +2.7% wall"
framing was **over-optimistic** — see §4.

## 1. The 18 reduce to one root

All 18 = 9 cardiac classes × 2 supers (`Anonymous-324` ≡
`IntrinsicallyPathologicalBodyProcess`, the same concept). The 9 classes are all
`⊑ IneffectiveCardiacFunction (ICF)`, and `ICF ⊑ Anonymous-349` (told). So the
single root gap is:

```
Anonymous-349 ⊑ Anonymous-324
```

everything else inherits via `⊑ ICF ⊑ Anonymous-349`. Konclude infers it
directly — no decomposing named intermediate (checked `notgalen-classified.owx`).

## 2. Why the saturator misses it

```
Anonymous-349 ≡ BodyProcess
              ⊓ ∃hasEffectiveness.(∃hasState.ineffective ⊓ Effectiveness)
              ⊓ ∃hasIntrinsicPathologicalStatus.physiological
Anonymous-349 ⊑ ∃hasPathologicalStatus.pathological          (told)

Anonymous-324 ≡ ∃hasIntrinsicPathologicalStatus.pathological ⊓ BodyProcess
```

`BodyProcess` conjunct: derived (`saturation-only` → yes). Missing conjunct:
`Anonymous-349 ⊑ ∃hasIntrinsicPathologicalStatus.pathological`.

The blocker: `hasIntrinsicPathologicalStatus` is **functional** (line 8903), and
Anonymous-349's only successor on that role is filled `physiological`.
`physiological ⋢ pathological` (siblings under
`PathologicalOrPhysiologicalStatus`, not disjoint). So deriving the missing
conjunct requires **forcing that single functional successor to also be
`pathological`** — i.e. functional-role witness-merge of `physiological` with a
`pathological` derived from elsewhere — plus the GCI chain that supplies the
`pathological`. There is **no** direct GCI deriving
`∃hasIntrinsicPathologicalStatus.pathological` from the ineffective-effectiveness
body (the full RHS-list is unrelated named classes), so it is **not** a simple
compound-existential-body lowering (Phase 2b) either.

## 3. Bounded checks that confirm "research-grade" (not assumed)

| Check | Result | Implication |
|---|---|---|
| `grep -c ObjectPropertyChain` galen / notgalen | **0 / 0** | not a role-chain delta |
| Konclude inferred supers of Anonymous-349 | only `IPBP`, direct | no droppable told-step intermediate |
| Direct GCI `…ineffective-body… ⊑ ∃hasIntrinsicPathStatus.pathological` | **absent** | not Phase-2b compound-body lowering |
| `hasIntrinsicPathologicalStatus` functional | **yes** | mechanism is functional-merge |
| `physiological ⊑ pathological` | **no** | can't be a trivial filler-subsumption |

## 4. Correction to the "do it" pitch

- notgalen is pure Horn (9377/9377) → a polynomial classification exists, and
  the per-pair backward proof timing out (>3 min) is a search artifact, not
  fundamental hardness. **This part holds.**
- BUT the recovery mechanism is **not** Phase 2d's existential-fact-on-subclass
  propagation (which took GALEN — a *different* ontology, `factkb#` IRIs — to
  full parity). It is **functional-role witness-merge** (Phase 2a), which had
  **0 GALEN corpus impact** and is the soundness-sensitive branch. So "proven
  approach, +2.7% wall" does **not** transfer to this cluster.
- GALEN (`factkb#`) vs notgalen (`galen.org#`) are genuinely different
  ontologies; the analogous axiom is an anonymous GCI in GALEN (galen.ofn:7515)
  vs a named equivalence in notgalen. Why GALEN's machinery closes its cardiac
  cluster but notgalen's stays open is the precise open question — likely the
  named-vs-anonymous structural difference interacting with 2d's fact
  propagation.

## 5. Recommendation

The 18 are a **functional-role-merge completeness** project: extend the
saturator (or the Horn hyper fixpoint) to merge functional-role witnesses across
GCI-derived fillers, with FP=0 corpus gating (functional merge can introduce
unsound positives if mis-fired). Multi-phase, soundness-critical — a deliberate
project, not an increment. The SIO 2 (out-of-EL) remain separately deferred.
This refines the handoff's "research-grade" with a precise root cause:
**`Anonymous-349 ⊑ Anonymous-324`, blocked on functional-merge of the
`hasIntrinsicPathologicalStatus` successor.**
