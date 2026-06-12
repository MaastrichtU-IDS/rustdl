# Justification (explanation) — foundation design (2026-06-12)

## Goal

Give rustdl **justifications**: for an entailment it reports, return a *minimal
set of the user's axioms* responsible for it ("why does `C ⊑ D` hold?"). This is
the capability neither rustdl nor Konclude offers natively, and it lives in
rustdl's strength zone (sound, embeddable, EL/Horn-complete).

This is **Spec 1 of 3** in a decomposed project (each its own spec→plan→build):
1. **Foundation (this spec):** black-box find-one engine + 6 class/individual
   query types + find-all (phase 2) + CLI/API.
2. **Property/individual query reductions** (`P⊑Q`, `(a P b)`, `a=b`, `a≠b` via
   probe-symbol encodings) — new query variants on this engine.
3. **Refinements** — laconic/precise justifications + root-vs-derived
   unsatisfiability.

## Approach (decided in brainstorming)

**Black-box, reasoner-agnostic** (approach A): minimization runs over the loaded
ontology's *own axioms* via the public entailment API (`is_subclass_of`,
`is_class_satisfiable`, `is_consistent`, `is_instance_of`). Justifications come
back as the user's axioms; no engine internals are touched; subsumption,
unsatisfiability, inconsistency, and instance entailments are handled uniformly.
(Rejected: glass-box EL proof-tracing — EL-subsumption-only, doesn't meet the
broad-coverage requirement; IR-level minimization — faster but justifications in
IR terms + fragile snapshot reuse.)

## The soundness / correctness contract (load-bearing, honest)

rustdl is **sound everywhere** but **complete only on EL/Horn** (SROIQ uses the
trust_sat wedge, which can miss). For justification-finding this means:

- **Any returned justification genuinely entails the query** (soundness of the
  oracle: if rustdl says `J ⊨ q`, it holds). This is unconditional.
- **Minimality** is established by *removing* an axiom and confirming the
  remainder no longer entails. On SROIQ, rustdl might *miss* that the remainder
  still entails → the axiom is kept → the result is **guaranteed-entailing but
  possibly non-minimal**. On **EL/Horn** (rustdl complete), the result is
  **exact (minimal)**.
- Every result carries `fragment: FragmentClassification` and
  `minimal_guaranteed: bool` (= `fragment ∈ {PureEl, Horn}`). Consumers/CLI must
  surface this so a SROIQ "justification" is never mistaken for provably minimal.

## Architecture

New module `crates/owl-dl-reasoner/src/justify.rs` (orchestration level — drives
the public reasoner API, no engine internals). A `justify` CLI subcommand wires
it.

### Query type
```rust
pub enum Entailment {
    SubClassOf { sub: String, sup: String },
    EquivalentClasses { a: String, b: String },
    DisjointClasses { a: String, b: String },
    Unsatisfiable { class: String },
    InstanceOf { individual: String, class: String },
    Inconsistent,
}
```

### Entailment oracle (one dispatcher, reused per candidate check)
```rust
fn entails<A: ForIRI>(onto: &SetOntology<A>, q: &Entailment) -> Result<bool, ReasonError>
```
| variant | reduction |
|---|---|
| `SubClassOf{sub,sup}` | `is_subclass_of(onto, sub, sup)` |
| `EquivalentClasses{a,b}` | `is_subclass_of(a,b) && is_subclass_of(b,a)` |
| `DisjointClasses{a,b}` | add probe `Declaration(X) + EquivalentClasses(X, ObjectIntersectionOf(a,b))`, then `!is_class_satisfiable(onto', X)` |
| `Unsatisfiable{class}` | `!is_class_satisfiable(onto, class)` |
| `InstanceOf{individual,class}` | `is_instance_of(onto, individual, class)` |
| `Inconsistent` | `!is_consistent(onto)` |

The `DisjointClasses` probe (`X` a fresh IRI `urn:rustdl-justify-probe`) is
injected by `entails` into the tested ontology — it is query encoding, NOT a
candidate axiom, so it appears in every tested subset and never in the output.

### Axiom partition
- `fixed`: `Declaration`, `AnnotationAssertion`, ontology/import metadata —
  retained in *every* tested ontology (no effect on entailment; keep it
  well-formed).
- `candidates`: all logical axioms (TBox `SubClassOf`/`Equivalent`/`Disjoint`,
  RBox `SubObjectPropertyOf`/domain/range/characteristics, data-property axioms,
  ABox `ClassAssertion`/`ObjectPropertyAssertion`/`Same`/`Different`, …). The
  minimizer is axiom-type-agnostic, so object/data-property axioms and ABox
  assertions appear in justifications whenever responsible.

### Result
```rust
pub struct Justification<A: ForIRI> {
    pub axioms: Vec<Component<A>>,        // the user's responsible axioms
    pub fragment: FragmentClassification,
    pub minimal_guaranteed: bool,         // fragment ∈ {PureEl, Horn}
}
```

## find-one (phase 1) — QuickXplain

Junker (2004) divide-and-conquer minimization. Given background `B = fixed` and
`candidates C` with `B ∪ C ⊨ q` (precondition; verified first — if the full
ontology does not entail `q`, return `Ok(None)` "not entailed, nothing to
justify"), compute a minimal `C' ⊆ C` with `B ∪ C' ⊨ q`. ~O(|C'|·log(|C|/|C'|))
`entails` checks; each check rebuilds a `SetOntology` from `B ∪ subset`. Minimal
result on EL/Horn (rustdl complete); guaranteed-entailing on SROIQ.

A naive single-pass linear contraction (O(|C|) checks) is the simpler correct
fallback (monotonicity makes it minimal too); QuickXplain is the default for the
log-factor speedup. **Noted optimization, not phase 1:** ⊥-locality syntactic
module extraction (reuse `locality.rs` if applicable) to shrink `candidates`
before minimization — large speedup on big ontologies; deferred to keep phase 1
focused.

## find-all (phase 2 of this spec) — Hitting Set Tree

Reiter HST over find-one: find `J₁`; for each axiom `a ∈ J₁`, recurse on the
ontology with `a` removed to find a justification avoiding `a`; collect all
minimal justifications, **capped** (default ≤10, configurable). `find_all_justifications(onto, query, max) -> Vec<Justification<A>>`.
Same correctness contract per justification.

## API / CLI

- `pub fn find_one_justification<A: ForIRI>(onto: &SetOntology<A>, q: &Entailment) -> Result<Option<Justification<A>>, ReasonError>`
- `pub fn find_all_justifications<A: ForIRI>(onto: &SetOntology<A>, q: &Entailment, max: usize) -> Result<Vec<Justification<A>>, ReasonError>` (phase 2)
- CLI: `rustdl justify <file> <QUERY>` where QUERY ∈
  `subclass <S> <T>` | `unsat <C>` | `instance <I> <C>` | `equivalent <A> <B>` |
  `disjoint <A> <B>` | `inconsistent`. Prints the responsible axioms (rendered,
  e.g. OFN), the count, the `fragment`, and a one-line guarantee note
  (`minimal (EL/Horn)` vs `entailing; minimality not guaranteed (SROIQ)`).
  Phase 2: `--all [--max N]` prints every minimal justification. The existing
  engine-attribution `explain` command is left as-is (distinct diagnostic).

## Testing (negatives-first + correctness invariants)

- **Known-justification canaries:** `A⊑B, B⊑C ⊢ A⊑C` ⟹ exactly `{A⊑B, B⊑C}`;
  irrelevant noise axioms must be excluded; one canary per query type
  (subclass, equivalent, disjoint, unsat, instance, inconsistent).
- **Correctness invariants (asserted in tests):** (a) the returned `axioms` are
  a subset of the ontology's logical axioms; (b) they re-entail `q`
  (`entails(fixed ∪ axioms, q)` is true); (c) on an EL/Horn fixture, minimality
  — removing any single axiom breaks entailment; (d) `minimal_guaranteed` is
  true on an EL fixture, false on a SROIQ fixture.
- **Multiple-justification (phase 2):** an ontology with two independent
  derivations of `C⊑D` → find-one returns one valid minimal; find-all returns
  both; cap respected.
- **Not-entailed:** querying a non-entailment returns `Ok(None)`.
- **Corpus smoke:** on a real fixture, pick a known entailment, assert the
  justification is a subset that re-entails (no oracle for "the" justification,
  so check the invariants, not an exact set).

## Scope / non-goals (this spec)

- **In:** find-one + find-all; the 6 class/individual queries; CLI + API;
  fragment/minimality flagging.
- **Out (committed follow-on specs):** property/individual *query* reductions
  (Spec 2); laconic/precise + root/derived (Spec 3); glass-box EL proofs;
  IR-level minimization; module-extraction speedup (a noted optimization,
  added only if corpus runs prove find-one too slow).
- **Performance:** find-one is interactive-scale (tens–hundreds of `entails`
  checks, each an EL/Horn classify ≈ ms). No hard budget; if a real ontology
  makes it slow, module extraction is the lever. Not gated on wall.
