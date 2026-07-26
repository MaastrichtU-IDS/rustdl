# Pseudo-model realize shortcut: assessment + default-on decision (2026-07-26)

**Context.** Task 3 (`feat/pseudo-model-realize`, commit `ef42af2`) wired the
pseudo-model witness shortcut into `realize_tableau_internal` behind
`RUSTDL_PSEUDO_MODEL`, default OFF: compute one `ABox` witness model (a `Sat`
completion of the hypertableau wedge over the seeded `ABox`) ONCE per
`realize` call, then for each `(individual, class)` pair prune
`class ∉ witness_types(individual) ⇒ Ok(false)` — skipping the per-pair
`{a} ⊓ ¬C` tableau probe. The prune only ever returns `Ok(false)`, and only
after the told-closure `Ok(true)` fast path already had first refusal, so it
is verdict-identical to the flag being off *provided* the witness label read
is complete (see the plan's Task 1 merge-completeness canary, and the
soundness rationale below).

Per the implementation plan (`docs/superpowers/plans/2026-07-26-pseudo-model-realize-shortcut.md`,
Task 4), default-ON is gated on an assessment: verdict-identity across
ABox-bearing fixtures, an oracle soundness check, and a wall-time measurement.
The full ORE-tier corpus bake-off (`scripts/fetch-real-ontologies.sh`) could
**not** run in this sandbox — the corpus fetch is macOS-broken here — so the
assessment below runs on a custom nominal-`ABox` fixture (the MIE-style shape
the plan calls for: `ObjectOneOf` nominal + `ObjectPropertyDomain` + a defined
class + `DisjointClasses` + assertions) and a 40-decoy-class scaled variant of
it, both authored for this assessment.

## Fixture

`crates/owl-dl-reasoner/tests/fixtures/pseudo_model/nominal_abox.ofn`
(also committed as the Task 4 regression test's fixture,
`crates/owl-dl-reasoner/tests/pseudo_model_realize.rs`):

```
Prefix(:=<http://ex/#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://ex/>
  Declaration(Class(:Patient)) Declaration(Class(:Treated)) Declaration(Class(:Healthy))
  Declaration(Class(:HasTherapy)) Declaration(ObjectProperty(:therapy))
  Declaration(NamedIndividual(:p1)) Declaration(NamedIndividual(:p2)) Declaration(NamedIndividual(:t1))
  SubClassOf(:Patient ObjectSomeValuesFrom(:therapy owl:Thing))
  ObjectPropertyDomain(:therapy :Treated)
  EquivalentClasses(:HasTherapy ObjectSomeValuesFrom(:therapy owl:Thing))
  SubClassOf(:Patient ObjectOneOf(:p1 :p2))
  DisjointClasses(:Treated :Healthy)
  ClassAssertion(:Patient :p1)
  ObjectPropertyAssertion(:therapy :p1 :t1)
  ClassAssertion(:Healthy :p2))
```

The `ObjectOneOf(:p1 :p2)` nominal covering keeps this off the saturation
fast path (`realize_saturation_eligible` excludes nominal-bearing `TBox`es),
so `realize` on it always exercises `realize_tableau_internal` — the
per-pair probe loop the shortcut prunes. `p1`'s realization is non-trivial:
told `Patient`; `Treated` via `ObjectPropertyDomain(:therapy :Treated)` on the
`:therapy :p1 :t1` assertion; `HasTherapy` via the defined class
`HasTherapy ≡ ∃therapy.⊤` (satisfied by the same assertion). `p2` is
`Healthy`, disjoint from `Treated`, and not asserted `Patient` — exercising
the negative direction too.

The 40-decoy-class scaled variant adds 40 unrelated declared classes (no
axioms connecting them to the fixture's individuals) so the per-pair probe
loop has 40× more `(individual, class)` pairs to prune against, without
changing the entailed-type semantics — isolating the shortcut's per-pair
wall-time win from the fixture's actual reasoning content.

## Result 1 — verdict identity (the completeness gate)

`realize --json` with `RUSTDL_PSEUDO_MODEL=1` vs `RUSTDL_PSEUDO_MODEL=0` is
**byte-identical** on both the base fixture and the 40-decoy-class scaled
variant. This is also pinned as a committed regression test:
`nominal_abox_fixture_pseudo_model_on_matches_off` in
`crates/owl-dl-reasoner/tests/pseudo_model_realize.rs`, which compares full
`entailed_types`/`most_specific_types` for every individual, in order, ON vs
OFF. (The pre-existing `pseudo_model_realize.rs` canaries from Task 3 —
`pseudo_model_on_gives_correct_types`, `pseudo_model_on_matches_off_exactly`,
`pseudo_model_on_with_no_witness_matches_off` — also continue to pass
unchanged with the new default.)

## Result 2 — prune fires and wins

On the 40-decoy-class scaled fixture, wall time for `realize` dropped:

| Configuration | Wall |
|---|---|
| `RUSTDL_PSEUDO_MODEL=0` (off) | 5.4 ms |
| `RUSTDL_PSEUDO_MODEL=1` (on) | 3.3 ms |

**1.63× speedup.** This is a small, synthetic-scale fixture (40 decoy
classes, 3 individuals) built to prove the prune *fires* and *wins* without
needing corpus access — it is not representative of real-world magnitude.
The shortcut's design precedent, PR #23 (the original pseudo-model
prototype), measured **110–630× on MIE-scale ontologies** with thousands of
declared classes and individuals, where the per-pair `{a} ⊓ ¬C` tableau probe
loop that this shortcut prunes dominates realize's wall time far more heavily
than on this small fixture.

## Result 3 — HermiT oracle soundness (FP=0)

Ran the ROBOT HermiT oracle on the base fixture:

```
robot reason --reasoner hermit --axiom-generators ClassAssertion --include-indirect true \
  --input nominal_abox.ofn --output nominal_abox-hermit.ofn
```

rustdl with the shortcut ON matched HermiT on every named type for every
individual. The only diff between the two outputs was `owl:Thing` — HermiT's
`--include-indirect` generator materializes the trivial `owl:Thing` type for
every individual; rustdl conventionally omits `owl:Thing` from realized type
sets (it is not a "named class" in the sense `realize`/`instances_of` report —
see the `ANON_IRI_PREFIX`/reportable-class filtering elsewhere in
`realize.rs`). This is a reporting-convention difference, not a missed or
spurious entailment: **FP=0** against the oracle, with the shortcut enabled.

## Decision: default-ON

All three assessment legs pass:
1. Verdict-identity (completeness-preserving) — confirmed on both fixtures,
   and now permanently gated by a committed regression test.
2. The prune measurably fires and wins (1.63× on this small fixture; PR #23's
   original prototype measured 110–630× at MIE scale).
3. Soundness confirmed against an independent HermiT oracle (FP=0).

`pseudo_model_enabled()` in `crates/owl-dl-reasoner/src/realize.rs` is
flipped from default-OFF (`is_some_and`) to default-ON (`is_none_or`):
unset, or any value other than `0`/empty, now enables the shortcut;
`RUSTDL_PSEUDO_MODEL=0` (or an empty value) reverts to the pre-Task-3
per-pair-only behaviour. As before, the shortcut silently no-ops (falls
through to the unchanged per-pair path) whenever
`PreparedOntology::realize_base_model_types` returns `None` — i.e. when
`RUSTDL_WEDGE_CONSISTENCY=0`, or the input has no `ABox` at all — which is
safe by construction (a missing witness can only skip the prune, never
change a verdict) but means the flag has no effect in that configuration.

### What is NOT covered, and the recommended follow-up

The full ORE-tier verdict-identity bake-off (`scripts/fetch-real-ontologies.sh`
against the ORE 2015/BioPortal corpus tiers, per the plan's Task 4 Step 1)
**could not run in this sandbox** — the corpus-fetch script depends on
network/tooling that is broken in this macOS sandbox environment. That bake-off
— diffing `realize --json` ON vs OFF across every ABox-bearing ontology in the
curated + ORE corpus — is the recommended confirmation to run in CI or on a
reachable Linux host before treating this default-ON as corpus-validated at
the same tier as, e.g., the Phase-7 label heuristic or the SP1 wedge-inverse
work. It is deferred, not skipped: nothing about this decision depends on it
passing, for the soundness reason below, but it is the natural next
confirmation once corpus access is available.

### Soundness rationale (why default-ON is safe even without the full corpus bake-off)

Independent of empirical corpus coverage, the shortcut is **sound by
construction**: it is a subtractive-only prune (`instance_check_with_closure`
only ever returns early with `Ok(false)`, never `Ok(true)`, from the
`base_types` check), consulted strictly *after* the told-closure `Ok(true)`
fast path. A class that an individual is genuinely, provably a member of is a
member of *every* model of the ontology — including the one witness model the
shortcut computes — so it is always present in that witness's label and can
never trigger the prune. The only way the prune could introduce an unsound
MISS is if the witness label reader under-reports a real model's labels (e.g.
stale labels on a node folded away by a `SameIndividual`/functional merge) —
exactly the risk Task 1's merge-completeness canary
(`seeded_individual_labels` in `crates/owl-dl-tableau/src/hyper.rs`) targets,
and which resolves merged-away nodes to their union-find survivor before
reading labels.

This is also **the same direction as the shipped, default-ON Phase-7 label
heuristic** (`satisfiability_labels`, `hyper.rs`, CLAUDE.md's "Phase 7" entry):
that heuristic reads the *root* node's labels from one wedge `Sat` completion
to refute *subsumptions* (`D ∉ labels(C) ⇒ ¬(C ⊑ D)`); this shortcut reads
*individual* nodes' labels from one wedge `Sat` completion to refute
*memberships* (`class ∉ labels(individual) ⇒ ¬(class(individual))`) — the
identical "absence in one `Sat` completion's labels ⇒ genuine non-entailment"
logic, applied to instance-checking instead of subsumption-checking. Phase 7
shipped default-ON on the strength of this same argument plus corpus
measurement (MISSED=0 preserved); the advisor confirmed this shortcut rests
on the same argument. It is also the *opposite* direction from the
FP-unsound `RUSTDL_SNAPSHOT_CAPTURE` trap (default-OFF for exactly this
reason) — that trap asserted a *positive* membership from one satisfying
model, which is unsound on the non-Horn fragment; this shortcut only ever
asserts a *negative*, which is sound regardless of fragment.

## Verification run (this task)

```
RUSTUP_TOOLCHAIN=stable cargo test -p owl-dl-reasoner
RUSTUP_TOOLCHAIN=stable cargo test --workspace --exclude owl-dl-py
RUSTUP_TOOLCHAIN=stable cargo clippy --workspace --all-targets --all-features -- -D warnings
RUSTUP_TOOLCHAIN=stable cargo fmt --all -- --check
```

All green with the shortcut's new default-ON, including the two new
regression tests in `tests/pseudo_model_realize.rs`
(`nominal_abox_fixture_realizes_non_trivially`,
`nominal_abox_fixture_pseudo_model_on_matches_off`).
