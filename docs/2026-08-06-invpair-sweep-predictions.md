# Pre-registered predictions — `RUSTDL_INVERSE_PAIR_FUNC` two-arm sweep

Recorded **before** the sweep ran. Binary pinned
`owl-reasoner-harness/bin/rustdl-invpair-9f74f15`, sha256 `a37522841ce72493…`.
Per-arm wrapper scripts (`/tmp/invpair-arm-{off,on}.sh`) so each record's `reasoner`
field proves its own arm; propagation verified standalone on the discriminating fixture
(7-axiom core: arm-off `consistent`, arm-on `inconsistent`).

## What this sweep CAN and CANNOT see

**It cannot see the correctness effect.** The harness runs `classify`, and `classify`
does **not** observe this flag: on the 7-axiom core both arms report
`consistent: true`, while `rustdl consistent` reports `inconsistent` with the flag on.
classify's inconsistency pre-check (`abox_saturation` + `top_is_unsat`) does not reach
the tableau route the fix works through — confirmed with an unbounded
`RUSTDL_CLASSIFY_INCONSISTENCY_MS=0`.

**What it CAN see is the thing that blocks a default flip:** whether the added axioms
(derived role characteristics + materialised inverse edges) cause `ok → dnf` or answer
changes on the 1,920-ontology corpus. That is exactly the failure the v0.4.8
`RUSTDL_CLASSIFY_INCONSISTENCY` flip shipped by measuring 12 ontologies instead of the
corpus.

| quantity | prediction |
|---|---|
| `ok → dnf` | **0**, but genuinely uncertain — the flag adds ABox edges, and edge growth is the plausible regression channel. This is the measurement. |
| `dnf → ok` | **0**. The flag's effect is on `consistent`, which classify does not consult for this clash shape. |
| answer changes (digest, comments stripped) | **0 or very few.** A change is possible and would NOT automatically be a bug: derived role characteristics are entailed, so a *new* subsumption is sound. Any diff must be adjudicated against Konclude ∪ HermiT, not assumed either way. |

**Decision rule, fixed now:** recommend a default flip only if `ok → dnf` = 0 **and**
every answer change adjudicates as sound. **Independently of the sweep, the flag should
stay OFF** while `classify --json` and `consistent` disagree on the core fixture —
shipping that divergence is the defect v0.4.8 removed. That divergence, not the sweep, is
the binding blocker.

Sweep: 60 s cap, single-thread, 4-way concurrency per arm, arms sequential,
`--digest-strip-comments`.
