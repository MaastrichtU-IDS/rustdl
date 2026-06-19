# Classify-completeness (SP1.1) — design

**Date:** 2026-06-19
**Status:** approved (brainstorming session 2026-06-19)
**Program:** "Konclude-class engine" (sub-project 1.1 — completeness follow-on to SP1)
**Predecessors:** [SP1 spec](2026-06-18-wedge-declared-inverse-symmetric-design.md)
(shipped, `ae13521`). Layer A POC: branch `wip/sp1.1-classify-oracle-reach`
(commit `a2b3014`, FP=0/MISSED=0, N²-test-proven). Memory:
`konclude-class-engine-sp1`.

---

## 1. Context — the two-layer classify-completeness gap

SP1 made the wedge fire `domain`/`range` through inverse + symmetric roles, but
**only on the paths that build the wedge with the role hierarchy** (consistency +
the diagnostic probe). The default per-pair **classification** oracle does not
carry it, so a subsumption derivable only via inverse/symmetric-domain firing is
**missed in `classify`**. Driver (Konclude-confirmed, `rustdl classify` misses):

```
C ⊑ ∃p.G ;  ObjectPropertyDomain(ObjectInverseOf(p), H) ;
G ⊓ H ⊑ K ;  ∃p.K ⊑ D     ⟹  C ⊑ D    (only via inverse-domain on the generated successor)
```

The SP1.1 investigation found the miss has **two independent causes**, and both
must be closed for the subsumption to surface end-to-end:

- **Layer A — oracle reach.** `HyperCache::{decide, classify_labels}`
  (`reasoner/src/lib.rs`) build their `HyperEngine` with `sub_roles = None`, so the
  classify oracle (and the labels it produces) don't carry the inverse/symmetric
  firing. **Already built + validated** on `wip/sp1.1-classify-oracle-reach`
  (FP=0/MISSED=0 corpus-wide, an N²-pairs test proves `C ⊑ D` is then found via the
  exhaustive `classify_n2` path).
- **Layer B — tier routing.** The top-down classifier
  (`classify.rs::classify_top_down_internal`) groups classes into **tiers of equal
  closure-subsumer count** and assumes **same-tier classes are mutually
  incomparable** (a more-specific class has *more* subsumers, so `C ⊏ D` ⟹
  `count[D] < count[C]` ⟹ different tier). That holds only if the closure captures
  *all* subsumers — but it's the EL/told closure (under-approximate). An
  engine-derived `C ⊑ D` the closure can't see leaves `C`, `D` at **equal count →
  same tier → never compared → missed**. This is why Layer A alone (verified) does
  not surface `C ⊑ D` in default `classify`: the pair is never asked.

## 2. Goal & non-goals

### Goal

Make inverse/symmetric-domain-derived subsumptions appear in **default
`classify`**, by (A) giving the classify oracle the role hierarchy and (B) testing
the same-tier pairs the tier walk currently skips, gated by the now-complete label
heuristic so the cost stays bounded.

### Non-goals

- **Reducing the disjunctive-search cost** (SP2 — measured NO-GO). SP1.1 may
  *surface* a few new same-tier pairs that are themselves SP2-hard; those are
  deadline-capped by `--pair-timeout-ms` like any other hard pair (see §6 perf).
- **New calculus.** SP1's firing is the calculus; SP1.1 only routes it into the
  classify oracle + tier walk. Wedge/consistency paths are unchanged.

### Ship/revert criterion

Ships iff: (1) the driver `C ⊑ D` appears in default `classify`; (2) corpus
closures are **MISSED-reduced-or-unchanged with FP=0** (no closure loses entries,
none gains a spurious one); (3) no wall blowup on the corpus (the same-tier testing
+ any newly-surfaced hard pairs stay within tolerance, deadline-capped). Any FP →
revert.

## 3. Architecture

### Layer A (land the validated POC)

Port `wip/sp1.1-classify-oracle-reach` (`a2b3014`) onto the SP1.1 branch:
- `HyperCache` (`reasoner/src/lib.rs`) gains a `sub_roles: RoleHierarchy` field,
  built once in `HyperCache::build` via the existing `build_role_hierarchy(&internal)`.
- `HyperCache::build` builds the shared amortized index with the hierarchy
  (`build_clause_indexes(&clauses, Some(&sub_roles))`) instead of `None`; the
  Q-clause delta block is unchanged (the Q-clause has no role atom).
- `HyperCache::decide` adds `.with_sub_roles(self.sub_roles.clone())` (rebuilds the
  per-pair index from its own clauses — correct, self-contained).
- `HyperCache::classify_labels` adds `.with_sub_roles_keep_index(self.sub_roles.clone())`
  — a new `HyperEngine` method that sets `sub_roles` **without rebuilding** the
  prebuilt amortized index (the index is already hierarchy-aware from `build`), so
  the per-probe amortization + Q-clause delta are preserved.

### Layer B (same-tier pairwise testing, label-gated)

In `classify_top_down_internal`, after the existing tier walk places cross-tier
parents, add a **same-tier subsumption pass**:

- For each tier, for each ordered pair `(C, D)` of *distinct* same-tier members
  where **`D ∈ labels(C)`** (the Layer-A-completed label heuristic — a cheap bitset
  membership check, the same `LabelOracle` Phase 7 already builds), run the oracle
  `subsumes_via_tableau`/`HyperCache::decide(C, D)` exactly as the cross-tier walk
  does.
- A confirmed `C ⊑ D` is recorded in the entailment matrix / hierarchy with the same
  bookkeeping the cross-tier walk uses (it does **not** change tier membership — the
  result is a cross-classification edge added post-walk, consistent with the
  existing "closure-seed step recovers cross-tier subsumption the walk can't see"
  comment).
- **Asymmetry guard:** same-tier `C ⊑ D` and `D ⊑ C` can't both hold for distinct
  satisfiable classes (would be equivalence, handled separately); test ordered
  pairs and let the oracle reject the false direction.
- **Gating keeps it bounded:** the label heuristic prunes 96–100% (Phase 7), so the
  oracle is invoked only on genuine same-tier candidates. The `D ∈ labels(C)` check
  is O(1) per pair; per tier it's O(T²) bitset checks (cheap), with oracle calls
  only on label-hits.

### Files

- `crates/owl-dl-reasoner/src/lib.rs` — Layer A (HyperCache hierarchy threading) +
  `HyperEngine::with_sub_roles_keep_index` in `crates/owl-dl-tableau/src/hyper.rs`.
- `crates/owl-dl-reasoner/src/classify.rs` — Layer B (the same-tier pass in
  `classify_top_down_internal`).
- Tests: `crates/owl-dl-reasoner/tests/classify_inverse_domain.rs` (extend the
  Layer-A POC tests with a **default-`classify`** assertion that `C ⊑ D` is found —
  the end-to-end gate, currently failing).

## 4. Soundness

- **Layer A** is FP=0 by construction (SP1's firing is sound; threading the
  hierarchy only adds genuinely-entailed matches) and already corpus-validated on
  the POC branch.
- **Layer B** is FP=0 by construction: it only *adds candidate pairs to test*; every
  recorded subsumption is **confirmed by the oracle** (`HyperEngine` `Unsat` of
  `C ⊓ ¬D`, sound for the full ontology). The label gate only *prunes* what's tested
  — a label miss is a sound MISS, never an FP. So Layer B can only **add real
  subsumptions or do nothing** — it cannot create a false one.
- The whole-corpus gate is closure comparison vs the Konclude oracle: **FP=0**, and
  closures **MISSED-reduced-or-unchanged** (no closure should *lose* an entry — that
  would signal a bug, not a completeness gain).

## 5. Testing & gates

1. **End-to-end driver test** (the headline): `classify_inverse_domain.rs` — assert
   `C ⊑ D` (`http://e#C ⊑ http://e#D`) appears in **default `classify`** of the
   `sp11sub` ontology (currently MISSED). Plus the existing N² + FP-control tests
   (kept green).
2. **Same-tier FP control:** a synthetic where two same-tier classes are *not*
   subsumption-related but are label-heuristic-adjacent — assert default `classify`
   does **not** report a subsumption between them.
3. **Corpus closure net** (`konclude_closure_diff`): **FP=0**, and every fixture's
   closure **≥ its pre-SP1.1 closure** (MISSED-reduced-or-unchanged), byte-compared.
   Any closure that *shrinks* → bug → revert.
4. **Perf:** classify walls on galen/notgalen/sio/alehif/ore-10908 within tolerance
   (the same-tier pass adds O(T²) bitset checks + a few oracle calls); **ore-15672 /
   wine watched specifically** — Layer B may surface same-tier hard pairs that hit
   the SP2 disjunctive-timeout. Confirm the wall stays bounded (deadline-capped) and
   record any new timed-out pairs.
5. Workspace suite green; clippy `--all-features -D warnings`; fmt.

## 6. The Layer-B perf risk (explicit)

Layer B tests same-tier pairs the walk skipped. On most fixtures these are few
(label-gated) and cheap. On the SROIQ-hard fixtures (ore-15672, wine) some
same-tier candidates may be SP2-hard (disjunctive-search timeout). Mitigation: the
testing is **label-gated** (only candidates the complete oracle flags) and
**deadline-capped** (`--pair-timeout-ms`), so the worst case is a few extra
deadline-burns, not unbounded growth. The §5.4 perf gate quantifies it; if a
fixture's wall blows up, the ship criterion fails → revert or gate Layer B behind a
flag.

## 7. Decomposition

1. **Layer A** — port `a2b3014` (HyperCache hierarchy threading +
   `with_sub_roles_keep_index`); re-validate FP=0/MISSED=0 corpus-wide. (Mostly a
   cherry-pick of validated work.)
2. **Layer B** — the same-tier label-gated pass in `classify_top_down_internal` +
   the end-to-end driver test + the same-tier FP control.
3. **Corpus closure net + perf gate** — FP=0, MISSED-reduced-or-unchanged, no wall
   blowup; accept/revert.
4. Docs (CLAUDE.md) + memory.

## 8. Open questions for implementation

- **Equivalence vs same-tier subsumption:** confirm how the existing walk handles
  equivalent classes (equal closure-count *and* mutual subsumption) so Layer B's
  ordered-pair test composes (it should only add strict edges; equivalences are
  pre-handled).
- **Where to place the same-tier pass:** during each tier's processing vs a single
  post-walk pass over all tiers. Prefer post-walk (after `direct_supers` is built)
  to avoid mutating tier state mid-parallel-walk; confirm it composes with the
  entailment-matrix builder's existing closure-seed step.
- **`with_sub_roles_keep_index` invariant:** the prebuilt index must already be
  hierarchy-aware (built `Some(&h)` in `HyperCache::build`) — assert/document that
  this setter is only used on such an index (else it's a silent MISS).
