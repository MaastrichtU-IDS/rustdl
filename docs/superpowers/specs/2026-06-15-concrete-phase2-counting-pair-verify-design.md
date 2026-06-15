# Concrete Phase 2: counting-pair verification (named⊑named data-counting subsumption) — Design

**Date:** 2026-06-15
**Status:** Approved (brainstorming), pre-plan
**Author:** rustdl (Michel Dumontier + Claude)

## Goal

Close a measured classify miss: a named⊑named subsumption entailed by data
*counting* (cardinality monotonicity), where neither class is unsatisfiable
alone — only `sub ⊓ ¬sup` is. Concretely, `C ⊑ ≥5 p.int` entails `C ⊑ D` when
`D ≡ ≥3 p.int` (because `≥5 ⟹ ≥3`), but the default classifier misses it because
it trusts the wedge's `NotSubsumed` verdict (the wedge has no `card_sat` and does
not materialise DKey cardinality).

## The measured miss (the deferral criterion, now met)

The Phase 1 spec (`2026-06-11-classify-concrete-domain-verify-design.md` §"Deferred:
in-wedge clash") gated this work on "a measured miss." Reproduced 2026-06-15 on
the default binary:

```
Ontology(
  SubClassOf(:C DataMinCardinality(5 :p xsd:integer))
  EquivalentClasses(:D DataMinCardinality(3 :p xsd:integer))
)
```
- Default `classify`: `# subsumption: tableau=0`, **no `C ⊑ D` line** → MISS.
- `RUSTDL_HYPERTABLEAU_TRUST_SAT=0`: `tableau=1`, `direct C D` → FOUND.

So the complete path (main tableau, which runs `concrete_domain_clash`) decides
it; the default trusts the wedge `Sat` and misses it.

## Why Approach C (chosen) over the deferred Approach A

The deferred design proposed Approach A: an **in-wedge clash** (thread
`dkey_ranges` into `HyperEngine`, record-but-not-materialise AtLeast/AtMost for
DKey fillers, a `card_sat` clash hook with a backjumping `DepSet`). That is
FP-critical surgery on the wedge backjumping machinery — the exact area a
soundness review caught a false-inconsistent (residual C) on 2026-06-15.

Approach C reuses the **already-sound** main-tableau `concrete_domain_clash`
(which we confirmed finds `C⊑D`), routing counting pairs to it instead of
trusting the wedge — the natural extension of the shipped Phase 1, which already
routes counting *classes* to the main tableau for the per-class unsat check.
Approach A was conceived before Phase 1's main-tableau routing existed as a
pattern; per-pair routing is now the obvious lower-risk path.

## Architecture

One guard inside `subsumes_via_tableau` (`crates/owl-dl-reasoner/src/classify.rs`,
the `HyperVerdict::NotSubsumed if trust_sat && hyper_trust_sat_enabled()` arm,
~line 1887). That arm currently returns `Ok(Some(false))` — trusting the wedge.
The change: when the pair `(sub, sup)` is **counting-relevant**, do NOT return;
fall through to the existing tableau probe (lines ~1907-1949), which builds
`sub ⊓ ¬sup` and calls `prepared.decide` / `decide_with_deadline` — the main
tableau, with `concrete_domain_clash` and `dkey_ranges` already threaded.

**This single guard fixes both call sites** of `subsumes_via_tableau` that pass
`trust_sat = true`:
1. the main top-down walk (`find_direct_parents_top_down`), and
2. the **defined-sup sweep** (`classify.rs` ~line 1449, passes `trust_sat=true`
   for cost reasons per its in-code comment) — which is where the `C⊑D` pair is
   actually tested, since `D` is a defined class.

### The counting-relevant predicate

Precompute once in the classify driver, AFTER `closure` is built and BEFORE the
top-down walk / sweep:

```rust
let counting_relevant: HashSet<ClassId> =
    if prepared.data_counting_classes.is_empty() {
        HashSet::new()
    } else {
        (0..n).map(|i| ClassId::new(i as u32))
            .filter(|&c| prepared.data_counting_classes.contains(&c)
                || closure.subsumers_of(c).iter()
                       .any(|s| prepared.data_counting_classes.contains(s)))
            .collect()
    };
```

This mirrors Phase 1's per-class predicate (`classify.rs:1195-1200`) exactly:
direct membership OR a subsumer in `data_counting_classes`. Thread
`&counting_relevant` into `find_direct_parents_top_down` and the defined-sup
sweep, which pass it to `subsumes_via_tableau`.

The guard:
```rust
let pair_counting = counting_pair_verify_enabled()
    && (counting_relevant.contains(&sub) || counting_relevant.contains(&sup));
```
When true, the `NotSubsumed if trust_sat` arm sets a **new, separate** local flag
(e.g. `counting_verified: bool`, NOT `was_fast_refuted` — that one drives the
unrelated `hyper_refuted_fast_flipped_pairs` stat and reusing it would
mis-attribute counting flips) and does NOT early-return, so execution reaches the
tableau probe. After the probe, if `counting_verified` and the tableau returned
`Subsumed`, increment a `counting_verified_pairs` stat counter (the diagnostic
banner figure for "subsumptions recovered by counting-pair verification").

**Empty-set fast path:** `data_counting_classes` is empty across the entire
corpus, so `counting_relevant` is empty, the predicate is always false, and
classify behaviour/walls are byte-identical to today.

### Env gate

`RUSTDL_COUNTING_PAIR_VERIFY` — default ON, `=0` opts out (restores the
trust-every-`NotSubsumed` behaviour). Mirrors the other feature flags; enables
the gate canary and A/B.

## Soundness (FP direction — the cardinal invariant)

The guard only swaps a *trusted wedge `Sat`* for the *complete main-tableau
verdict* on counting pairs. It cannot introduce a false subsumption:
- Fall-through → tableau `Subsumed`: sound, because `concrete_domain_clash` is
  refute-only and opus-reviewed; a tableau `Unsat` on `sub ⊓ ¬sup` is a genuine
  entailment.
- Fall-through → tableau `Sat`: not-subsumed — the same verdict the wedge gave,
  now verified.
- It touches NO backjumping/merge/wedge machinery (unlike Approach A);
  `concrete_domain_clash` already ships sound. FP=0 preserved by construction.

**Completeness.** Closes the cardinality-monotonicity miss and any counting
subsumption the main tableau decides for a counting-relevant pair. A tableau
timeout (`NoVerdict`) on a routed pair degrades to the existing sound
not-subsumed under-approximation — never an FP.

## Performance

`counting_relevant` empty corpus-wide ⇒ guard never fires ⇒ walls byte-identical
(the early-out Phase 1 relies on). On counting-bearing ontologies, each routed
pair costs a main-tableau call instead of a trusted wedge `Sat`; bounded by the
(rare) counting classes. The env gate provides an instant opt-out if a future
workload makes routing expensive.

## Testing

Canaries in `crates/owl-dl-reasoner/tests/` (a new `concrete_phase2.rs` or
appended to an existing classify test file — implementer's call, follow repo
convention). Each builds an ontology and inspects classify output via the public
API (`classify` / `is_subclass`).

### Closes-the-miss (the headline)
- `C ⊑ ≥5 p.int`, `D ≡ ≥3 p.int` ⇒ default classify reports `C ⊑ D` (was missed).

### FP guards (must NOT report subsumption)
- `C ⊑ ≥3 p.int`, `D ≡ ≥5 p.int` ⇒ `C ⊑ D` NOT reported (`≥3` ⇏ `≥5`).
- `C ⊑ ≥5 p.int`, `D ≡ ≥3 q.int` (different property) ⇒ not subsumed.
- A non-counting ontology classifies byte-identically (guard never fires).

### Subsumer-inheritance
- `C ⊑ X`, `X ⊑ ≥5 p.int`, `D ≡ ≥3 p.int` ⇒ `C ⊑ D` found (exercises the
  `counting_relevant` subsumer expansion, not just direct membership).

### Gate
- With `RUSTDL_COUNTING_PAIR_VERIFY=0`, the headline miss returns (gate disables
  cleanly).

### Corpus regression
- Full closure-diff FP=0/MISSED=0 unchanged (pizza/ro/family/wine/sulo/bibtex/
  shoiq-knowledge/sio/ore-10908/ore-15672).
- Consistent fixtures unaffected; classify walls within noise (counting_relevant
  empty corpus-wide).

## Review gate

Touches classify's trust boundary (the `NotSubsumed if trust_sat` arm), so even
though it reuses sound machinery it gets an **opus** spec+quality review before
merge — the hardened rule for soundness-adjacent classify changes.

## Out of scope (deferred, sound)
- Approach A (in-wedge clash) — not built; Approach C closes the measured miss
  without the FP-critical wedge surgery.
- Non-monotonicity counting entailments the main tableau cannot decide within
  the per-pair budget (degrade to sound not-subsumed).
- Object-property qualified-cardinality subsumption (this is the data/DKey path;
  object cardinality is the tableau's existing concern).
