# Closing the SAO/BFO disjunctive-domain chain (2026-06-10)

The last **genuine-calculus** incompleteness on the ORE-2015 sample
(`docs/ore-2015-rerun-2026-06-10.md` stage-2): 10 pairs across two
ontologies — `ore_ont_15273` (5) and `ore_ont_6437` (5), both the SAO
neuroscience ontology over BFO 1.0. rustdl returned `no` even under the
full accelerators-off tableau; the Konclude∩HermiT oracle says `yes`.

## The miss

Headline pair: `sao1785599611 ⊑ snap#Continuant`. The 5 missed supers per
ontology are exactly the **common ancestors** of a disjunctive data-property
domain:

```
SubClassOf(sao1785599611, DataHasValue(sao841626191, "multipolar"^^xsd:string))
DataPropertyDomain(sao841626191,
    ObjectUnionOf(sao1224657022 sao1355908696 sao1813327414))
```

`sao1785599611`'s only *named* told-super is `sao-943768255`, which is
parentless — so there is no told route to Continuant. The real inference is:

1. `DataHasValue(p, v)` ⟹ the class **uses** `p` (`∃p.{v} ⊑ ∃p.⊤`).
2. `DataPropertyDomain(p, D₁⊔D₂⊔D₃) ≡ ∃p.⊤ ⊑ (D₁⊔D₂⊔D₃)`, so
   `sao1785599611 ⊑ (D₁⊔D₂⊔D₃)`.
3. All three domain disjuncts chain to Continuant:
   - `sao1224657022 ⊑ sao1813327414 ⊑ Object ⊑ IndependentContinuant ⊑ Continuant`
   - `sao1355908696 ⊑ sao197110912 ⊑ sao1813327414 ⊑ …`
   - `sao1813327414 ⊑ Object ⊑ …`
   so their **common told-subsumers** are exactly
   `{sao1813327414, Object, IndependentContinuant, Continuant, Entity}` — the 5 missed pairs.
4. `(D₁⊔D₂⊔D₃) ⊑ E` for each common subsumer `E` ⟹ `sao1785599611 ⊑ E`.

## Root cause

`data_axioms.rs` *did* record the C-side (`class_some` captures
`DataHasValue`/`DataSomeValuesFrom`/`DataMin≥1`/`DataExact≥1` — all
mandatory-filler), and Pattern 3 (`emit_domain_inferences`) already turns
`DataPropertyDomain(p, D) + C-uses-p` into `C ⊑ D`. **But it only fired for
an *atomic* domain `D`**: the scan at `DataPropertyDomain` recorded a domain
only when `class_iri(ce)` succeeded, so a `ObjectUnionOf(...)` domain returned
`None` and was silently dropped. The whole inference chain never started — even
the full tableau never saw that `sao1785599611` uses a domain-restricted
property, because data-property domains aren't lowered into the IR at all
(`convert.rs` drops `DataPropertyDomain`).

## Fix (two sound, composable pieces)

1. **`data_axioms.rs`** — capture disjunctive domains
   (`union_domains: Vec<(dp, [D…])>`), recorded **only when every disjunct is
   atomic** (a non-atomic member is invisible to the told tables, so a
   common-subsumer over a partial set would be unsound — advisor gate). New
   `pub fn derive_data_domain_unions(src, vocab) -> Vec<(ClassId, Vec<ClassId>)>`
   returns `(C, [D…])` for each class C using such a `dp`.

2. **`convert.rs`** — for each `(C, [D…])`, build the bare disjunctive GCI
   `SubClassOf(C, ObjectUnionOf(D…))` in the IR (it owns the `ConceptPool`;
   `data_axioms` only returns resolved ids). Runs right before
   `derive_disjunction_existentials`.

3. **`disjunction_existential.rs`** — extend `collect_from_sup` to also fold a
   **bare** `X ⊑ (D₁⊔…⊔Dₙ)` super (not just `∃R.(union)`) into `X ⊑ E` for each
   minimal common told-subsumer `E` (reusing the existing
   `minimal_common_subsumers`). Emits to the saturator directly — no ∃ wrapper.

The reduced `C ⊑ E` is what closes SAO via the **saturator fast path**
(`explain` reports "answered by saturation"); the bare GCI also lets the full
tableau case-split natively if no common subsumer exists.

## Soundness

`DataPropertyDomain(p,D) ≡ ∃p.⊤ ⊑ D`; `class_some` only captures
mandatory-filler restrictions (never Max/All), so `C ⊑ ∃p.⊤ ⊑ D` is genuine.
Common told-subsumer of a union: `Dᵢ ⊑ E ∀i ⟹ (⊔Dᵢ) ⊑ E ⟹ C ⊑ E`. Both steps
sound; FP impossible. The bare-`Or` fold is unconditionally sound for *any*
disjunctive super (regardless of origin) — on covering axioms
(`C ⊑ Ind⊔Dep` with both `⊑ C`) the minimal common subsumer is `C` itself, so
it emits `C ⊑ C` (skipped) — inert there.

## Verification

- Both ontologies: all 5 pairs now `yes` (via `.ofn` copies — see panic note),
  answered by saturation. **Genuine-calculus incompleteness on the ORE-2015
  sample: 10 → 0.**
- No tuned corpus fixture has a disjunctive data-property domain → the
  data-domain emission is a **strict no-op** on the corpus.
- 3 negatives-first canaries in `disjunction_existential.rs`:
  common-subsumer-yields-`C⊑E`, no-common-subsumer-is-silent,
  non-atomic-member-emits-nothing (the soundness gate).
- Corpus closure-diff gate (`RUSTDL_TEST_PAIR_MS=25`): FP=0 / MISSED=0
  preserved corpus-wide.

## Separate robustness bug (not fixed here)

Both ORE files are OWL Functional Syntax with a `.owl` **extension**. The CLI's
`parse_ofn` routes `.owl` → RDF/XML by extension and **panics** (unwrap on the
oxrdf parse error) instead of erroring gracefully. Worked around with `.ofn`
copies. A content-sniff (or graceful error) is worth a follow-up — tracked
separately from this completeness work.
