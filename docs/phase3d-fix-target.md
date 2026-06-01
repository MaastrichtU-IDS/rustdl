# Phase 3d — fix target

Per Phase 3d recon (`docs/phase3d-recon.md`), the dominant inner cost
of `apply_deferred_concept_or_rules` is the per-trigger `else` fallback
that linear-scans `&tbox.concept_rules` whenever the indexed lookup in
`tbox.concept_rules_by_trigger` returns `None`. The recon attributes
~96 % of the 18.16 % function frame (2,713 of 2,838 samples on SIO) to
the `next<ConceptRule>` + `eq<ConceptRule>` iterator-and-comparison
cluster from that scan. This doc specifies the surgical fix.

## Target code

`crates/owl-dl-tableau/src/rules.rs:576-603`, inside the `pending`
snapshot block of `apply_deferred_concept_or_rules`. Current code:

```rust
let mut out: Vec<(ConceptId, DepSet)> = Vec::new();
for (trigger, deps) in &triggers {
    let Some(conclusions) = tbox.concept_rules_by_trigger.get(trigger) else {
        // Hand-built TBox without finalize(): fall back to a
        // linear scan over concept_rules for this trigger.
        for rule in &tbox.concept_rules {
            if rule.trigger == *trigger {
                let (needs, bloom_hit) =
                    needs_deferred_or(pool, rule.conclusion, labels, label_sig);
                if bloom_hit {
                    crate::bump_counter!(ctx, needs_deferred_or_bloom_rejects);
                }
                if needs {
                    out.push((rule.conclusion, deps.clone()));
                }
            }
        }
        continue;
    };
    for &c in conclusions {
        let (needs, bloom_hit) = needs_deferred_or(pool, c, labels, label_sig);
        if bloom_hit {
            crate::bump_counter!(ctx, needs_deferred_or_bloom_rejects);
        }
        if needs {
            out.push((c, deps.clone()));
        }
    }
}
out
```

The bug: the `else` branch was documented as a fallback for hand-built
TBoxes that never invoke `finalize()`. In practice it fires every time
a label class has zero `concept_rules` in a finalized TBox — because
`finalize()` only inserts an index entry for triggers that have at
least one rule (`crates/owl-dl-core/src/absorb.rs:110-119`). On real
ontologies most class labels appearing on nodes have no Or-residual
rule, so the miss path is the common case; each such miss launches an
O(R) scan over the entire `concept_rules` vector.

## Fix shape

Gate the linear-scan path **once** at the top of the snapshot block on
`tbox.concept_rules_by_trigger.is_empty()` (the actual
"hand-built TBox without finalize()" predicate). When the index is
populated (the common case for finalized TBoxes), a missing-trigger
lookup means "no rules for this trigger" — skip with `continue`
instead of falling through to the per-trigger linear scan.

The control flow mirrors `apply_concept_rules` at
`crates/owl-dl-tableau/src/rules.rs:199-226`: an outer
`if tbox.concept_rules_by_trigger.is_empty()` / `else` split, with the
indexed branch handling missing triggers as an implicit no-op.

Surgical restructuring of the `pending` snapshot block (mutation
limited to the inner `let mut out = Vec::new(); for (trigger, deps) …`
section; everything outside is unchanged):

```rust
// Phase 3d: gate the legacy linear-scan fallback ONCE on the
// "TBox not finalized" predicate, instead of per-trigger inside
// the loop. On finalized TBoxes (the common case), an indexed
// lookup miss means "no concept_rules for this trigger" — skip.
let mut out: Vec<(ConceptId, DepSet)> = Vec::new();
if tbox.concept_rules_by_trigger.is_empty() {
    // Pre-finalize fallback (hand-built TBox without finalize()):
    // retained for compatibility. Same code as today, just hoisted.
    for (trigger, deps) in &triggers {
        for rule in &tbox.concept_rules {
            if rule.trigger == *trigger {
                let (needs, bloom_hit) =
                    needs_deferred_or(pool, rule.conclusion, labels, label_sig);
                if bloom_hit {
                    crate::bump_counter!(ctx, needs_deferred_or_bloom_rejects);
                }
                if needs {
                    out.push((rule.conclusion, deps.clone()));
                }
            }
        }
    }
} else {
    for (trigger, deps) in &triggers {
        let Some(conclusions) = tbox.concept_rules_by_trigger.get(trigger) else {
            crate::bump_counter!(ctx, apply_deferred_concept_or_skip_missing_trigger);
            continue;
        };
        for &c in conclusions {
            let (needs, bloom_hit) = needs_deferred_or(pool, c, labels, label_sig);
            if bloom_hit {
                crate::bump_counter!(ctx, needs_deferred_or_bloom_rejects);
            }
            if needs {
                out.push((c, deps.clone()));
            }
        }
    }
}
out
```

T4 implements the surgical version above. The counter name
`apply_deferred_concept_or_skip_missing_trigger` mirrors the Phase 3a
`needs_deferred_or_bloom_rejects` shape and gives T4's structural
canary a positive signal (it bumps on every indexed miss, which is the
hot path being eliminated relative to the linear scan).

## Soundness invariant

The change preserves semantics. In the finalized-TBox case, the
indexed `concept_rules_by_trigger.get(trigger)` returns the SAME
conclusions as a linear scan of `concept_rules` filtered by
`rule.trigger == trigger`, because the index is built **from**
`concept_rules` by `finalize()`. From
`crates/owl-dl-core/src/absorb.rs:110-119`:

```rust
pub fn finalize(&mut self) {
    self.concept_rules_by_trigger.clear();
    self.concept_rules_by_trigger
        .reserve(self.concept_rules.len());
    for rule in &self.concept_rules {
        self.concept_rules_by_trigger
            .entry(rule.trigger)
            .or_default()
            .push(rule.conclusion);
    }
    …
}
```

Every `ConceptRule` in `concept_rules` is reflected in the index by
trigger; the index value for any given trigger is exactly the set
`{ rule.conclusion | rule in concept_rules, rule.trigger == trigger }`.
A `None` from `get(trigger)` is therefore equivalent to "no rules with
this trigger exist in `concept_rules`," which is exactly what the
linear scan would discover after iterating the entire list and finding
no match. Skipping with `continue` on `None` reaches the same `out`
vector that the scan would produce.

`finalize()` is documented as "Idempotent — safe to call after any
mutation of the rule lists" and is always invoked by
`owl_dl_core::absorb::absorb`, which is the canonical entry point used
by the reasoner (the recon notes `absorb.rs:263` calls it). The
pre-finalize fallback is retained (now in the outer `if` branch) for
hand-built TBoxes used in unit tests that bypass `absorb`.

DepSet propagation is unchanged: `deps.clone()` happens on the same
push site, with the same `deps` value (the trigger's own
`label_deps[pos]`), so the back-jumping driver receives identical
`DepSet`s on Or-materialisation. The `needs_deferred_or` semantic — Or
already in labels or one of its disjuncts already in labels ⇒ skip —
is unchanged: it is called on exactly the same `(c, labels, label_sig)`
triples in either control path.

## Predicted impact

- SIO `apply_deferred_concept_or_rules` flame frame:
  **18.16 % → ~0.80 %** (~17.4 pp drop), per recon flame-attribution
  math (2,713 of 2,838 samples removed from the dominant frame; the
  ~125 remaining "alloc + drop" samples are independent of the fix).
- GALEN classify wall: sub-proportional to the flame delta due to
  rayon overlap and concurrent work. Honest range estimate: **5–15 %
  wall reduction** from the ~12.2 min Phase 3c baseline (i.e. ~10.4 to
  ~11.6 min). Should be the next-largest single-phase delta after
  Phase 3c's ~50 % reduction.
- FP=0 + MISSED=17 unchanged on GALEN. The change is a pure
  performance refactor with no semantic delta vs the pre-fix code
  (per the soundness section).
- Phase 0 net (`alehif_closure_matches_konclude`, `ore_10908_sroiq`,
  `ore_15672_shoin`) unchanged: FP=0 / MISSED=0 across all three.

## What this design does NOT change

- `apply_role_rules` (Phase 3e candidate, 16.36 % on SIO).
- `apply_max` (14.34 % on SIO; already attacked by Phase 3b on the
  inverse-roles path).
- The Phase 3a `needs_deferred_or` bloom prefilter (orthogonal; still
  invoked per conclusion inside the indexed branch).
- The `from_iter` / `collect` heap-alloc cluster (6.51 % on SIO; Phase
  3e target).
- The pre-finalize linear-scan fallback branch (retained intentionally
  for hand-built TBoxes used in unit tests that bypass
  `owl_dl_core::absorb::absorb`).
- Trigger-snapshot collection (`Vec<(ClassId, DepSet)>` build at
  `rules.rs:563-571`): ~0.80 % of total per recon, too small for this
  phase.
- DepSet sharing strategy (Rc/Arc): out of scope; clone cost is not
  visible as a child of the dominant frame post-3c.
- The wedge / hypertableau / saturation engines and the env-flag
  defaults (`RUSTDL_HYPERTABLEAU*`).

## Cross-references

- Phase 3d plan: `docs/superpowers/plans/2026-06-01-phase3d-apply-deferred-concept-or-rules.md`
- Phase 3d recon: `docs/phase3d-recon.md`
- Sibling rule reference shape: `crates/owl-dl-tableau/src/rules.rs:199-226`
  (`apply_concept_rules`)
- finalize() semantics: `crates/owl-dl-core/src/absorb.rs:110-119`
- Phase 3c results (prior baseline): `docs/phase3c-results.md`
- Post-3c flame findings: `docs/flamegraphs/sio-classify-2026-06-01-post-phase3c-findings.md`
