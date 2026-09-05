# Widening `verify-el` past `is_pure_el` to Horn buys TWO shapes — do not build it

`2026-09-05-verify-el-cap-is-a-weak-lever.md` closed by naming the fragment as the binding
constraint (**1,392 of 1,920 unresolved, 72.5%**, against 124 lost to the cap) and recommending
"extending `verify-el` past `is_pure_el` to the Horn fragment" as the lever.

**That recommendation is retracted here.** The market is not the 1,392 `unresolved` count, nor
anything close to it: **all but TWO structural shapes** are refused by the evaluator regardless of
what the gate does, and the argument is a two-list comparison that takes minutes, not a census.

**It also turned up a live, peer-confirmed engine defect** — see the last section. That was
incidental to the scoping and is the more valuable half of the day.

## The claim

Widening the CLI gate at `crates/owl-dl-cli/src/main.rs:2558` from `PureEl` to `PureEl | Horn`
can reach a verdict **only** on an ontology whose sole departures from `is_el_axiom` are
**non-atomic `DisjointClasses` members** and **non-atomic `ObjectPropertyDomain` / `Range`
fillers**. Every other route out of EL that stays Horn is refused by `eval.rs`, and one refusal
sinks the whole verdict.

Both holes are the same mechanism: the EL gate demands an **atomic** concept in a position where
`eval.rs` accepts anything it can evaluate, and `CE::And` is something it can evaluate.

> **An earlier draft of this file claimed the market was ZERO and carried a `∎`.** That claim was
> written over five unread `eval.rs` arms, and **two** of them were holes. It was caught by
> reading the arms, not by any test — which is why all 25 `Axiom` variants are now enumerated
> rather than summarised.

## The proof

Three facts compose.

**(1) `Verified` requires ZERO unresolved axioms.** `verify` (`crates/owl-dl-verify/src/lib.rs:494-520`)
returns `Violated` if any violation exists, then `Unresolved` if any `unresolved` reason exists,
and only reaches `Verified` when both lists are empty. One refused axiom sinks the whole verdict.

**(2) Every construct that makes an ontology Horn-but-not-EL is refused by the evaluator.**
`Horn-but-not-EL` means `is_el_axiom` (`classify.rs:2143-2195`) says no while
`clausify_with_stats` reports `disjunctive == 0 && deferred == 0`. Enumerating the generators
against `eval.rs`:

| generator of Horn ∖ EL | source | `eval.rs` |
|---|---|---|
| `∃r⁻.C` | `is_el_concept` requires `!role.is_inverse()` | `Unresolved("Some(Inverse)")` :82 |
| `∀r.C` | `_ => false` | `Unresolved("All")` :87 |
| `≥n r.C` | `_ => false` | `Unresolved("Min")` :88 |
| `≤n r.C` | `_ => false` | `Unresolved("Max")` :89 |
| `{a}` | `_ => false` | `Unresolved("Nominal")` :83 |
| `Self(r)` | `_ => false` | `Unresolved("SelfRestriction")` :84 |
| `FunctionalRole` | axiom `_ => false` | `unhandled` :566 |
| `InverseFunctionalRole` | axiom `_ => false` | `unhandled` :567 |
| `ReflexiveRole` | axiom `_ => false` | `unhandled` :564 |
| `IrreflexiveRole` | axiom `_ => false` | `unhandled` :565 |
| `AsymmetricRole` | axiom `_ => false` | `unhandled` :563 |
| `DisjointObjectProperties` | axiom `_ => false` | `unhandled` :562 |
| `DisjointUnion` | axiom `_ => false` | `unhandled` :561 |
| `ClassAssertion` / `ObjectPropertyAssertion` / `NegativeOPA` / `SameIndividual` / `DifferentIndividuals` | axiom `_ => false` | `unhandled` :568-574 |

`¬C` and `⊔` are absent from this table on purpose: they clausify DISJUNCTIVE, so they generate
`OutOfFragment`, not `Horn`. They are refused too, but they are not part of the widening's market.

The remaining ways `is_el_axiom` says no are role-shaped, and `eval.rs` refuses each of them at
exactly the sub-case that makes it non-EL — verified by reading the arms, not inferred:

| generator of Horn ∖ EL | `is_el_axiom` | `eval.rs` |
|---|---|---|
| inverse `sub`/`sup` in `SubObjectPropertyOf` | :2169 requires `!r.is_inverse()` | `unhandled("SubObjectPropertyOf(Role, Inverse)")` :425 |
| a chain with an inverse leg | :2170-2174 | `unhandled("SubObjectPropertyOf(Chain, Inverse)")` :448 |
| a chain of length ≠ 2 | :2170-2174 | `unhandled("SubObjectPropertyOf(Chain, len != 2)")` :445 |
| inverse member of `EquivalentObjectProperties` | :2175 | `unhandled("EquivalentObjectProperties(Inverse)")` :474 |
| inverse `TransitiveRole` | :2176 | `unhandled("TransitiveRole(Inverse)")` :500 |
| inverse role in `ObjectPropertyDomain` / `Range` | :2183-2188 | `Unresolved("ObjectPropertyDomain(Inverse)")` :367, `Range` :393 |

**(3) The two arms that LOOK like exceptions are guarded on exactly the case that matters.**
`SymmetricRole` (:527) and `InverseObjectProperties` (:543) are the only Horn ∖ EL role axioms
`eval.rs` has arms for — and both return `Holds` **only when the role has no edges**, otherwise
`Unresolved(GuardedRoleHasEdges)`. An edge-free symmetric/inverse declaration is precisely the
*bare declaration* that `RUSTDL_FRAGMENT_BARE_DECL` already admits to `is_pure_el`. The moment the
role is genuinely read — the only way it pushes an ontology out of EL — the guard fires.

## The two counterexamples — measured, not argued

### Hole 1 — a non-atomic `DisjointClasses` member

`is_el_axiom:2163` requires every `DisjointClasses` member to be
**atomic**, so `DisjointClasses(ObjectIntersectionOf(A B), C)` leaves the EL fragment. It
clausifies Horn (`A(x) ∧ B(x) ∧ C(x) → ⊥`, no disjunctive head). And `eval.rs:342`'s
`DisjointClasses` arm has **no atomicity check at all** — it calls `eval_concept` on each member,
and `CE::And` is handled at :43 without complaint.

`crates/owl-dl-verify/tests/horn_widening_market.rs` calls `build_model`/`verify` directly,
bypassing the CLI gate, and reports on that shape:

```
fragment = Horn
verdict  = Verified { axioms_checked: 8, domain_size: 4 }
```

### Hole 2 — a non-atomic `ObjectPropertyDomain` / `Range` filler

`is_el_axiom:2183-2188` requires an atomic (or `⊤`/`⊥`) filler — the D10 "Bug B" tightening, whose
own comment records that the engine's `role_domains` accepts **only** `Atomic` fillers and
silently drops a conjunctive one. `eval.rs:366` checks only that the role is not inverse, then
calls `eval_concept`. So `ObjectPropertyDomain(r, P ⊓ Q)` is Horn, non-EL, and reachable —
the same test file reports `fragment = Horn`, `verdict = Violated`.

So the market is **non-empty**, and the honest form of the claim is the one stated above: two
shapes, not zero, and not 1,392.

**Corpus reach, by grep SUPERSET: ≤8 ontologies for hole 1, ≤14 for hole 2.** Eight carry a
`DisjointClasses` with a non-atomic member (a same-line grep and a whitespace-insensitive
balanced-paren scan agree at 8); fourteen carry a conjunctive `ObjectPropertyDomain`/`Range`
filler (4 domain, 13 range, union 14).

**Do not add these to 22.** An ontology reaches a verdict only if **every** departure from
`is_el_axiom` is one of the two holes, so each count is a superset of its own conjunct and the
real market is smaller than either — a member of the 14 that also contains a single `∀` anywhere
is refused on the `∀`. `ore_ont_10908` is the demonstration: it is in the 14, it is a curated
corpus fixture, and it classifies at MISSED=0 against its Konclude oracle, so the shape does not
bite there. For comparison, the CB arc was deferred at a market of 16 *measured by gate*, not by
grep.

## The model source is mismatched, and that is what a `Violated` here would mean

Suppose someone "fixed" the guard so a read symmetric role could be checked. `build_model`
(`lib.rs:324-328`) constructs its model from `owl_dl_saturation::saturate_with_exists_facts` —
the **EL saturator**, which has no symmetry and no inverse rule at all (a grep of
`crates/owl-dl-saturation/src/` for `SymmetricRole` / `InverseObjectProperties` returns
**nothing**, confirming the note in CLAUDE.md's `BareRoleDecls` doc). The model would therefore be
missing exactly the backward edges the symmetric axiom asserts, and the check would report
`Violated`, and it would say nothing about the engine that answers the query — because
`saturator_complete_fragment` routes this ontology to the HYBRID path, not the saturator.
That is a **LEAD requiring adjudication**, which is exactly what the crate's own design spec
says a violation is until spurious violations are proven zero.

The same holds structurally for the whole widening: post-D10, `saturator_complete_fragment`
deliberately routes Horn-not-EL away from the saturator to the hybrid path. On the ontologies a
widened gate would admit, `build_model` would be reading a closure from an engine that is **not
the one answering the query** and is known-incomplete there. A `Violated` from that is not a D10
detection — D10 is "the gate certifies complete while the engine drops the axiom", and here the
gate correctly declines.

**Closing the gap properly means a different model source** — building the interpretation from
the wedge's Horn fixpoint completion graph rather than a subsumer closure. That is a sub-project
wanting its own spec, not a gate relaxation, and its precedent is the CB arc: real work went in
before the market was sized, and it reached 16 ontologies and was deferred.

## Method note

The two-list comparison is a **read**, not a sweep: three files, minutes. It was run *before* any
1,920-ontology census, on the advice that a constraint which can bound an arc analytically should
be checked before compute is spent. Had the census run first it would have produced a Horn count —
a number that looks like a market and, per the table above, is not one.

But the read is only as good as the arms actually read. The first draft's `∎` covered fourteen
arms and skipped five; one of the five was the hole. **Enumerate every arm the claim quantifies
over, or do not write a universal.**

## THE VALUABLE PART: adjudicating hole 2 found a live D10 defect

Hole 2's `Violated` looked like an instrument artifact. It was adjudicated anyway, on the crate's
own rule that a violation is a lead. It is a **real, silent, peer-confirmed engine defect**.

Fixture (`ObjectPropertyDomain(r, P ⊓ Q)` + `X ⊑ ∃r.B`; `X ⊑ P` and `X ⊑ Q` are entailed):

| | verdict |
|---|---|
| **rustdl `classify --json`** | **`direct_subsumptions: []`**, `incomplete: false`, `dropped: {}` |
| banner | `# mode: hybrid`, `# fragment: Horn (trust_sat sound by construction; hyper Horn fixpoint is complete)` |
| Konclude v0.7.0-1138 | `X ⊑ P`, `X ⊑ Q` (1,880 bytes — not the 896-byte failure stub) |
| HermiT 1.4.3 | `X ⊑ P`, `X ⊑ Q` |
| rustdl `subclass X P` | **`yes`** |

The D10 shape exactly: the gate certifies the fragment complete, the engine drops the axiom, and
the answer is reported with `incomplete: false`. `dropped` is empty, so it is a SILENT miss, not
a degradation.

**Root-caused, and it is NOT the calculus.** `subclass` proves the pair, and no flag on the
subsumption path recovers it — `RUSTDL_HYPERTABLEAU_TRUST_SAT=0`, `RUSTDL_HYPERTABLEAU=0`,
`RUSTDL_CLASSIFY_VERIFY_REFUTATIONS=1` and `RUSTDL_DOMAIN_ABSORPTION=0` all still return zero
rows. **`RUSTDL_CLASSIFY_SAME_TIER=1` recovers both.** This is the documented tier-walk
limitation: the walk groups classes by EL/told subsumer count and never compares same-tier
classes. Dropping the conjunctive domain leaves `X` with **no** EL subsumer, so it lands in the
same tier as `P` and `Q` and is never compared against them.

**The discriminating control is the atomic filler.** `ObjectPropertyDomain(r, :P)` derives
`X ⊑ P` correctly at the default. So the cause is the conjunctive filler, not the domain
mechanism, and not the fixture's shape in general.

**What this does and does not say about `RUSTDL_CLASSIFY_SAME_TIER`.** That flag ships OFF partly
because it is *corpus-invisible* — "the pattern occurs only in the `sp11sub` synthetic". This is a
**second synthetic pattern** it closes, arrived at independently from the `verify-el` direction.
**Its corpus reach was then MEASURED and it is ZERO** — see the two-arm run at the end of this
file. So the flag's corpus-invisible justification stands **unqualified**, and nothing here argues
for flipping it: its ~2× wall cost is unchanged, and the narrower fix is to make the tier
assignment aware of a domain/range filler the saturator drops, or to stop dropping it.

**The defect is independent of the widening.** It lives in `classify`'s tier walk, not in
`verify-el`; it would have been findable from the classification side at any time. The spike is
what happened to point at the shape, not an instrument that detected it — `verify-el` cannot even
run on this ontology today.

**No third hole.** `SubClassOf` / `EquivalentClasses` recurse into `is_el_concept`, which is where
the same atomic-vs-`And` asymmetry could have hidden one. Printed rather than eyeballed:
`eval_concept` accepts exactly `Top`, `Bot`, `Atomic`, `And`, `Some(Role::Named)` (:41-80) and
`is_el_concept` accepts exactly `Top`/`Atomic`/`Bot`, `And`, `Some(!is_inverse)` (:2207-2210) —
identical sets, no guard on either side the other lacks.

## THE FULL 1,920 DECOMPOSITION — the builder-refusal half is 9 ontologies, not a population

The `unresolved` bucket was suspected of hiding a second, engine-change-free lever: the
`UnresolvedReason` variants `ChainRangeOutOfProfile` / `LabelNotClosed` / `BoundTripped` /
`RunDelta` are **builder** refusals that fire on ontologies which ARE `is_pure_el`, so that
population would need no fragment work at all. Measured over all 1,920 at a 60 s cap (pinned
binary, first line for the gate refusals, full captured stdout for the builder reasons):

| bucket | count |
|---|---:|
| `verified` (exit 0) | 361 |
| **refused by the CLI GATE** (`analyze_fragment != PureEl`) | **1,351** — 662 `Horn`, 689 `OutOfFragment` |
| **passed the gate, refused by the builder/evaluator** | **9** |
| timeout at 60 s — UNMEASURED | 199 |
| I/O or parse error | 1 |

**The builder-refusal half is 9 ontologies** — 6 `AxiomsDroppedAtConversion` and 3
`BoundTripped`. `ChainRangeOutOfProfile`, `LabelNotClosed` and `RunDelta` fire on **zero** ORE
ontologies. So that lever is measured out too, and the "off-fragment/**refused**" slash in the
72.5% headline is 99.3% the first word.

**662 Horn is a real population and still not a market**, which is the point of the two-list
comparison above: the constraint is not how many ontologies the gate refuses, it is how many the
evaluator could judge if it stopped refusing them.

## The `RUSTDL_CLASSIFY_SAME_TIER` corpus question is now answered: 0 of 12

Two-arm run over the 14 conjunctive-filler candidates, `RUSTDL_CLASSIFY_SAME_TIER` `0` vs `1`,
arm order alternated, comparing (rows, unsat, equivalence groups): **12 IDENTICAL, 0 gained, 0
lost, 2 UNMEASURED** (`ore_ont_10080`, `ore_ont_12451` — cap, not disagreement; `ore_ont_10080`
re-run sequentially on an IDLE host still DNFs at a **600 s** cap in the `=0` arm, so it is
genuinely out of reach rather than a contention artifact).

So the flag's **corpus-invisible default-OFF justification stands unqualified**. The defect filed
as #110 is a second *synthetic* pattern, exactly as `sp11sub` was, and nothing here argues for
flipping the flag.
