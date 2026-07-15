# Phase 0 — `ore_ont_10019` instance diagnosis: NOT an architecture gap, an over-branching preprocessing bug

**Context:** the user approved building a new search architecture from first principles.
The advisor's Phase-0 gate: do NOT commit to a rewrite until an *instance-level* diagnosis
shows the stall is genuinely irreducible (large model AND near-minimal branching AND
unavoidable search). Everything prior was measured on rustdl's *own* (suspect) search
trajectory — circular. Phase 0 looked at what `ore_ont_10019` actually contains and why a
specific class stalls. **Result: the rewrite is NOT warranted. The stall is spurious
over-branching from clausification; the fix is targeted preprocessing (days).**

## The instance

211 lines, 47 classes. A chemistry functional-group ontology (dumontierlab): atoms
(pairwise-disjoint — the 55 `DisjointClasses`), bond roles (`hasBondWith` + `hasSingle/
Double/Triple/AromaticBondWith`, **all symmetric**, domain/range = `Atom`), and 29
functional groups defined by `EquivalentClasses(D, CarbonAtom ⊓ =n bond.Atom ⊓ …)`. The
only disjunctions are small covering unions (HalogenAtom 5-way, HeteroAtom 4-way,
CarbonGroup 2-way, Alkyl's H⊔C). HermiT decides it in 360 ms, Konclude 90 ms — a correct
reasoner finds a small model fast.

## The smoking gun

`hyper-sat` per-class: **`CarbonAtom` STALLS** — 7016 branches, depth 96, 336 k
`is_blocked` calls, **0 blocks fired**. `CarbonAtom` is a primitive atom (`⊑ Atom` +
disjointness, no definition, no `∃`). Proving it satisfiable should take **zero** search.

A ⊔-decision probe shows what a bare `CarbonAtom` node branches on:
```
DEC node=0 nlabels=[…,CarbonAtom,…] body=[CarbonAtom] head_classes=[AcylGroup, S59,S60,S61,S62]
```
i.e. clause `CarbonAtom → AcylGroup ⊔ ¬M1 ⊔ ¬M2 ⊔ …`, where `M1,M2` are `AcylGroup`'s
cardinality conjuncts. It is **anchored on `CarbonAtom`**, so it fires on *every* node
carrying CarbonAtom — and ~15 defined classes share `CarbonAtom` as a conjunct, each
contributing such a disjunction. Every node with a common atom branches on all of them
→ the 7000-branch, depth-95 explosion, even for a bare atom.

## Root cause (in the code)

`crates/owl-dl-core/src/clause.rs::absorb_hard_antecedent` (~line 401). For the
**sufficient (⇐) direction** of a defined class `D ≡ soft ⊓ hard₁ ⊓ hard₂` (D named;
`hardᵢ` = cardinality/`∀`/etc.), it partitions conjuncts and emits
`soft → (¬hard₁ ⊔ ¬hard₂) ⊔ D`. When `soft` is a common atom (`CarbonAtom`), this
disjunction fires on every node carrying it. The code comment acknowledges the soft
trigger is a mitigation against the ⊤-level `⊤ ⊑ ¬sub ⊔ sup` explosion (which killed SIO)
— but a *shared* soft trigger doesn't mitigate enough here.

**HermiT/Konclude avoid this** by structural transformation: the hard conjuncts become
**body surrogate atoms** (`Mᵢ ≡ hardᵢ`), so the sufficient direction is the Horn rule
`soft ⊓ M₁ ⊓ M₂ → D` — fires only when the markers are actually derived on the node, no
branch (Horn hyperresolution). rustdl flips the hard conjuncts to a negated *head*
instead → don't-know nondeterminism where HermiT has none. This is precisely the
advisor's Phase-0 Q3 ("is rustdl over-branching relative to HermiT's DL-clause /
hyperresolution reduction?"). Yes.

## Go/no-go verdict: NO rewrite. Targeted preprocessing fix.

The advisor's rewrite gate requires *near-minimal branching* — falsified: a bare atom
branches 7000×. The architecture (HermiT-style hypertableau) is already proven capable on
this exact instance in 360 ms. The gap is an **implementation/preprocessing deficiency**,
not a missing architecture. Building a new search engine would risk reproducing the same
explosion under a new name while leaving the real cause (clausification over-branching)
untouched.

**Recommended fix (targeted, days-scale):** surrogate-atom (structural / "definitorial")
transformation for hard antecedent conjuncts in `absorb_hard_antecedent`, so the
sufficient direction of a defined class clausifies **Horn** (`soft ⊓ M₁ ⊓ M₂ → D`) instead
of a disjunction anchored on a shared soft trigger. Scope:
1. Introduce a surrogate class `Mᵢ` per distinct hard conjunct; add `Mᵢ ⊑ hardᵢ`
   (necessary) and arrange `hardᵢ`-on-a-node ⟹ `Mᵢ` (so the Horn rule fires when the
   node genuinely satisfies the conjunct). The reverse-derivation of a surrogate is the
   delicate part — this is standard DL absorption theory (HermiT's approach), not novel.
2. Gate: FP=0 (curated + non-Horn `ore_ont_13723` oracle) + MISSED=0 byte-identical
   curated closures + `ore_ont_10019` classify decides the 33 within budget.
3. This is a **preprocessing** change (`owl-dl-core`), reusing the existing wedge
   unchanged — the opposite of a search rewrite.

## Secondary observations (not the primary cause)

- **Symmetric bond roles + 0 blocks fired** — worth checking once the over-branching is
  gone: whether the symmetric back-edge + blocking interaction also inflates depth. But
  the bare-`CarbonAtom` stall proves the disjunctive over-branching is the dominant,
  primary cost; fix that first and re-measure.
- All prior "levers measured out" remain correct *for the current over-branching search*;
  they don't bear on the preprocessing fix, which changes the clause set itself.

## Deeper-diagnosis spike (2026-07-16): confirms the mechanism AND scopes the fix

A throwaway flag (`RUSTDL_SPIKE_DEFER_HARD`) that DROPS the soft+hard sufficient-direction
disjunctions in `absorb_hard_antecedent`, measured on `ore_ont_10019`:

| | branches | stalled classes | sat classes | closure vs Konclude(162) | FP | MISSED |
|---|---|---|---|---|---|---|
| current (⇐ clauses on) | 160 652 | 33 | 14 | 150 | 0 | 12 |
| spike (⇐ clauses dropped) | **30** | **0** | **47** | 150 | 0 | 12 |

Three decisive conclusions:
1. **The over-branching is pure waste.** 160 652 → 30 branches, 33 → 0 stalled — yet the
   closure is IDENTICAL (150). The explosion derives nothing correct.
2. **The ⇐ clauses ARE needed** for 12 real subsumptions (`AcylGroup ⊑ CarbonylGroup`,
   … all `X ⊑ CarbonylGroup`/`OrganicSulfurGroup` — a defined class X whose *necessary*
   conditions ⊇ another defined class's definition). Konclude derives them (162); rustdl
   misses them (150). Proving `X ⊑ D` needs D's sufficient direction to expand `¬D`.
3. **Current rustdl gets the worst of both** — it HAS the ⇐ clauses (→ over-branches) and
   STILL misses the 12 (the over-branching makes those subsumption pairs stall out).

**So the sound fix must recover the 12 (→ Konclude parity 162) WITHOUT over-branching.**
That is precisely Horn surrogate-atom absorption: `CarbonAtom ⊓ Q → CarbonylGroup`
(Q a surrogate for `=1 hasDouble.O`) fires deterministically (no branch) and derives the
subsumption. **The one genuinely delicate sub-problem** the spike surfaces: the
reverse-derivation of a *cardinality* surrogate — deriving Q on a node that has
`=1 hasDouble.O` (the surrogate's ⇐ direction is itself a hard antecedent). This is the
crux the fix's design must solve (standard DL absorption territory; HermiT solves it).

## Disposition

Branch `feat/dense-sroiq-search-arch` was created for the rewrite; **the rewrite is not
warranted.** Recommend renaming/repurposing it to the absorption fix, or a fresh
`feat/hard-antecedent-surrogate-absorption` branch. No architecture code written. Probes
were throwaway (reverted). This is the honest redirect the advisor's Phase-0 gate is for:
the approved rewrite is, on the evidence, a targeted preprocessing fix.
