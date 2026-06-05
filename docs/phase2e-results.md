# Phase 2e — functional super-role witness-merge on the body sub-role

Run 2026-06-05. Closes the notgalen residual 18 MISSED (full Konclude parity)
by fixing an order-dependent gap in the Phase 2a/2c-redux witness-merge rule.

## Headline

**notgalen MISSED 18 → 0** (rustdl_closure = konclude_closure = 32 739, FP=0).
First time notgalen reaches full Konclude parity. GALEN stays at 0. FP=0 held
across the entire corpus. The only remaining corpus MISS is SIO's 2 (out-of-EL,
separate gap).

## Root cause

The 18 reduce to one root, `Anonymous-349 ⊑ Anonymous-324`
(≡ `IntrinsicallyPathologicalBodyProcess`); the other 17 inherit via
`9 cardiac classes ⊑ IneffectiveCardiacFunction ⊑ Anonymous-349`.

```
Anonymous-349 ≡ BodyProcess ⊓ ∃hasEffectiveness.(…ineffective…)
                           ⊓ ∃hasIntrinsicPathologicalStatus.physiological
Anonymous-349 ⊑ ∃hasPathologicalStatus.pathological
Anonymous-324 ≡ ∃hasIntrinsicPathologicalStatus.pathological ⊓ BodyProcess
```

`hasIntrinsicPathologicalStatus` and `hasPathologicalStatus` are both sub-roles
of `StatusAttribute`, which is **functional**. So A349's two status witnesses
(`physiological`, `pathological`) coincide into one node = `physiological ⊓
pathological`, reached via `hasIntrinsicPathologicalStatus` ⇒ A349 ⊑
`∃hasIntrinsicPathologicalStatus.pathological` ⇒ A349 ⊑ Anonymous-324. Standard
SROIQ functional super-role merge; Konclude does it, the EL saturator did not.

**The bug.** The Phase 2a merge rule (in `process_fact`) emits the merged
synthetic on the functional super-role `R_f` and back-propagates it to the
*other* sub-roles `R_k` that the subject has witnesses on — but it **skipped the
merge-triggering role** (`other.role == fact.role`), on the rationale that CR9
hierarchy propagation already covers `R_arr`. CR9 only propagates the *original*
witness *up* to `R_f`; it never pushes the merged *synthetic* *down* to `R_arr`.
When the existential body lives on `R_arr` itself (IPBP's body is on
`hasIntrinsicPathologicalStatus`), the merged filler must land on `R_arr`. Which
sub-role is `R_arr` depends on fact-processing order — so the synthetic reached
`hasIntrinsicPathologicalStatus` in some orders and not others. On notgalen it
landed the wrong way ⇒ 18 MISSED. The Phase 2a canaries never caught it because
their `Target` body is on the super-role (`∃r_func.…`), which uses the rule's
*direct* super-role emit and never exercises the sub-role back-prop.

## The fix

Remove the `other.role == fact.role` skip in the back-prop loop
(`crates/owl-dl-saturation/src/lib.rs`). The merged synthetic now also lands on
the triggering sub-role, making the rule order-independent.

**Soundness.** By functionality of `R_f`, every sub-role witness — including
`R_arr`'s — coincides with the single `R_f`-successor carrying the full merged
atom set, so `(sub, R_arr, synthetic)` holds in every model. The back-prop
condition still requires `R_f ∈ functional_supers_of(other.role)`, i.e.
`other.role ⊑ R_f` functional. FP=0 empirically across alehif, ore-10908,
ore-15672, shoiq-knowledge, ro, sulo, sio, galen, notgalen.

## Test

`functional_role_merge_body_on_sub_role` (saturation crate): the minimal A349
shape (Subject ≡ D ⊓ ∃r_i.A; Subject ⊑ ∃r_j.B; Target ≡ ∃r_i.B ⊓ D, with
r_i,r_j ⊑ functional r_func). Red before the fix, green after. The pre-existing
super-role-body canaries still pass.

## Corpus gate

| Fixture | FP | MISSED | wall |
|---|---|---|---|
| alehif | 0 | 0 | 0.15 s |
| galen | 0 | 0 | 0.52 s |
| **notgalen** | **0** | **18 → 0** | 1.00 s |
| ro | 0 | 0 | 5.88 s |
| shoiq-knowledge | 0 | 0 | 17.77 s |
| ore-10908-sroiq | 0 | 0 | 19.89 s |
| sulo | 0 | 0 | 0.25 s |
| sio | 0 | 2 (out-of-EL) | 31.11 s |
| ore-15672-shoin | 0 | 0 | 42.89 s |
