# Phase 2b — saturator trace for the compound existential-body canary

Diagnostic trace from running the Phase 2b canary
(`compound_existential_body_canary_documents_the_gap` in
`crates/owl-dl-saturation/src/lib.rs`) with temporary `eprintln!`
instrumentation. The tracing was REVERTED before commit.

## Setup

Canary axioms (4 in total, in addition to declarations):

```
SubObjectPropertyOf(:S_sub :S)
SubClassOf(:C_sub :C)
EquivalentClasses(:T   A ⊓ ∃R.(B ⊓ ∃S.C))
EquivalentClasses(:X   A ⊓ ∃R.(B ⊓ ∃S_sub.C_sub))
```

Expected entailment: X ⊑ T. HermiT confirms (Phase 2b Task 2, commit
b025031).

## IDs assigned during lowering

Resolving class/role IDs from the IRI map emitted at saturate-entry:

| ID | meaning |
|---|---|
| `ClassId(0)` | A |
| `ClassId(1)` | B |
| `ClassId(2)` | C |
| `ClassId(3)` | C_sub |
| `ClassId(4)` | T |
| `ClassId(5)` | X |
| `ClassId(6)` | M_SC — one-way marker for `∃S.C` (T's inner existential) |
| `ClassId(7)` | F_T_body — Tseitin synthetic for `B ⊓ M_SC` (= T's R-body) |
| `ClassId(8)` | M_R_FTbody — marker for `∃R.F_T_body` (allocated by LHS-And handler) |
| `ClassId(9)` | M_SsubCsub — one-way marker for `∃S_sub.C_sub` (X's inner existential) |
| `ClassId(10)` | F_X_body — Tseitin synthetic for `B ⊓ M_SsubCsub` (= X's R-body) |
| `ClassId(11)` | M_R_FXbody — marker for `∃R.F_X_body` (allocated by LHS-And handler) |
| `RoleId(0)` | R |
| `RoleId(1)` | S |
| `RoleId(2)` | S_sub |

## Synthetics + facts allocated during lowering

From the `tseitin.introduce` / `introduce_existential_marker` /
`exist_fact` / trigger logs:

```
introduce_existential_marker: role=S    body=C       -> marker=6   (NEW; trigger ∃S.C ⊑ 6)
tseitin.introduce:            body=[B,6]              -> synthetic=7   (NEW)   ; conj_trigger {B,6} ⊑ 7
introduce_existential_marker: role=S    body=C       -> marker=6   (DEDUP, from symmetric Eq direction)
tseitin.introduce:            body=[B,6]              -> synthetic=7   (DEDUP)
introduce_existential_marker: role=R    body=7       -> marker=8   (NEW; trigger ∃R.F_T_body ⊑ 8)
introduce_existential_marker: role=S_sub body=C_sub  -> marker=9   (NEW; trigger ∃S_sub.C_sub ⊑ 9)
tseitin.introduce:            body=[B,9]              -> synthetic=10  (NEW)   ; conj_trigger {B,9} ⊑ 10
introduce_existential_marker: role=S_sub body=C_sub  -> marker=9   (DEDUP)
tseitin.introduce:            body=[B,9]              -> synthetic=10  (DEDUP)
introduce_existential_marker: role=R    body=10      -> marker=11  (NEW; trigger ∃R.F_X_body ⊑ 11)
```

The **only existential facts** seeded into the worklist are the two
outer-R facts emitted by the atomic-LHS / RHS-Existential lowering
path (`lower_sub_class_of` → `atomic_existential_rhs`):

```
exist_fact[0]: sub=T  role=R  target=F_T_body (7)
exist_fact[1]: sub=X  role=R  target=F_X_body (10)
```

**No facts about the inner markers M_SC, M_SsubCsub, or about the
Tseitin bodies F_T_body, F_X_body exist.** The four `introduce_existential_marker`
NEW calls emit triggers ONLY; the marker's docstring (`lib.rs:1035-1038`)
spells out this one-way semantics ("∃R.B ⊑ F but no F ⊑ ∃R.B").

Trigger inventory:

```
exist_trigger[0]: role=S      body=C       head=6   (M_SC)
exist_trigger[1]: role=R      body=7       head=8   (M_R_FTbody)
exist_trigger[2]: role=S_sub  body=C_sub   head=9   (M_SsubCsub)
exist_trigger[3]: role=R      body=10      head=11  (M_R_FXbody)
```

```
conj_trigger[0]: {B, 6}  ⊑ 7   (F_T_body definition)
conj_trigger[1]: {A, 8}  ⊑ T   (T definition)
conj_trigger[2]: {B, 9}  ⊑ 10  (F_X_body definition)
conj_trigger[3]: {A, 11} ⊑ X   (X definition)
```

## Worklist behavior — final closure

After saturation runs to fixed point, the closure is:

```
subsumers[A]           = {A}
subsumers[B]           = {B}
subsumers[C]           = {C}
subsumers[C_sub]       = {C, C_sub}                # told ⊑ + reflexivity
subsumers[T]           = {A, T, M_R_FTbody=8}
subsumers[X]           = {A, X, M_R_FXbody=11}
subsumers[M_SC=6]      = {6}                       # NEVER picks up anything
subsumers[F_T_body=7]  = {B, M_SC=6, 7}            # via conj_trigger[0]
subsumers[M_R_FTbody=8]= {8}
subsumers[M_SsubCsub=9]= {9}                       # NEVER picks up anything
subsumers[F_X_body=10] = {B, M_SsubCsub=9, 10}     # via conj_trigger[2]
subsumers[M_R_FXbody=11]={11}
```

Facts at fixed point — identical to the seeded pair, no derived facts:

```
fact[0]: T  R   F_T_body
fact[1]: X  R   F_X_body
```

The closure does NOT contain `X ⊑ T`. (Trigger CT1 `{A,8} ⊑ T` would
fire on X if X had M_R_FTbody=8 as a subsumer; it doesn't.)

## Derivation that should fire but doesn't

What the closure WOULD need, working backward from `X ⊑ T`:

1. **`X ⊑ T`** would come from CT1 `{A, 8} ⊑ T` firing on X. X has A;
   needs `X ⊑ M_R_FTbody=8`.
2. **`X ⊑ 8`** would come from exist_trigger[1] (`∃R.F_T_body ⊑ 8`)
   firing on X. That trigger fires on any class with a fact
   `(_, R-or-super, target)` where `target ⊒ F_T_body=7`. X has a
   fact `(X, R, F_X_body=10)`. So we need `F_X_body ⊑ F_T_body`, i.e.
   `10 ⊑ 7`.
3. **`F_X_body ⊑ F_T_body`** would come from CT0 `{B, M_SC=6} ⊑ F_T_body=7`
   firing on F_X_body. F_X_body has B as a subsumer; needs
   `F_X_body ⊑ M_SC=6`.
4. **`F_X_body ⊑ M_SC=6`** would come from exist_trigger[0]
   (`∃S.C ⊑ M_SC=6`) firing on F_X_body. That trigger fires when some
   class has a fact `(F_X_body, S-or-super, target)` where `target ⊒ C`.
   **F_X_body has no facts about it at all — facts_by_sub[10] is empty
   in the trace.** This is the first step that DOESN'T fire and SHOULD.
5. The fact that would unblock step 4 is `(F_X_body=10, S_sub, C_sub=3)`,
   or equivalently `(M_SsubCsub=9, S_sub, C_sub=3)` plus the
   conjunctive subsumer `F_X_body ⊑ M_SsubCsub` (which already holds).
   With that fact present, the worklist would:
   - Process fact `(10, S_sub, 3)`: target_subsumers(3) = {C, C_sub};
     existential_triggers_by_body[C=2] contains exist_trigger[0] with
     role S. S is in supers_of(S_sub) (via the SubObjectPropertyOf),
     so the trigger MATCHES. CR9 + CR5 fire jointly: F_X_body gains
     head=6 as a subsumer. (Mechanically this is `process_fact` at
     `lib.rs:644-662`.)
   - Conjunctive trigger CT0 then fires on F_X_body (now has both B
     and M_SC=6), giving F_X_body ⊑ F_T_body=7.
   - `process_subsumer` then fires exist_trigger[1] for the pre-existing
     fact `(X, R, F_X_body)` whose target now has 7 as a subsumer,
     deriving X ⊑ M_R_FTbody=8.
   - Conjunctive trigger CT1 then fires on X, deriving X ⊑ T. Done.

The entire downstream chain is gated on emitting that one missing
fact at lowering time.

## Diagnostic conclusion

**Hypothesis CONFIRMED.** `introduce_existential_marker` emits the
trigger `∃R.B ⊑ F` but no fact `(F, R, B)`. The marker 9 (M_SsubCsub)
is then placed into the body of the Tseitin synthetic F_X_body=10 by
`atomic_classes_with_existential_markers` (`lib.rs:1502`), so the
closure correctly derives `F_X_body ⊑ M_SsubCsub`. But since
M_SsubCsub has no outgoing existential fact, the closure cannot
propagate that "F_X_body has an S_sub-witness in C_sub" through
CR9 (sub-property `S_sub ⊑ S`) + CR5 (sub-class `C_sub ⊑ C`) into the
trigger `∃S.C ⊑ M_SC`. The chain breaks at step 4 of the derivation
above. Concretely: the missing fact is `(M_SsubCsub=9, S_sub, C_sub=3)`,
which would also be sound to emit as `(F_X_body=10, S_sub, C_sub=3)`
since `F_X_body ⊑ M_SsubCsub` already holds and `process_subsumer`
would propagate the fact through `subs_of_class`.

The same gap exists symmetrically for T's chain: `(M_SC=6, S, C=2)`
is also missing. T happens to derive its subsumers without needing
that propagation (because T's outer trigger `∃R.F_T_body ⊑ 8` is the
one tied to T's own definition, not a cross-class lookup), but ANY
ontology that needs to MATCH T's body against a structurally similar
X's body via sub-property/sub-class would hit the same gap.

Cross-site dedup check (advisor concern): markers 6 and 9 are
allocated NEW only from `atomic_classes_with_existential_markers`
(the in-body site at `lib.rs:1514` / `:1523`). Markers 8 and 11 are
allocated from the LHS-And handler at `lib.rs:1291` (top-level
existential operand within a conjunction LHS). In this canary the
in-body and LHS-And sites use disjoint `(role, body)` keys, so the
proposed fix can safely swap only the in-body call sites without
touching LHS-And semantics. (If a future ontology had two
existentials `∃R.B` where one appears in a Tseitin body AND the other
appears as an LHS-And operand at the SAME `(R, B)` key, the shared
marker would now carry equivalent semantics in both contexts — which
is sound: an LHS trigger `∃R.B ⊑ F` paired with a fact `(F, R, B)` is
just F ≡ ∃R.B, and the LHS-And handler currently uses F to mean
"holds in any class with an R-edge to a B", which is exactly what
the new equivalence-semantic marker means as well.)

## Proposed fix

Add a sibling method on `TseitinAllocator` named
`introduce_equivalent_existential_marker(role, body, rules)` that
calls the existing `introduce_existential_marker` and then also
pushes an `ExistentialFact { sub: marker, role, target: body }` into
`rules.existential_facts`. Call this new method from BOTH call sites
inside `atomic_classes_with_existential_markers` (`lib.rs:1514` and
`:1523`) — the two arms that lower nested `∃R.body` / `≥n R.body`
operands inside a Tseitin synthetic body. Leave the existing one-way
`introduce_existential_marker` AND its callers in the LHS-And handler
(`lib.rs:1291`, `:1297`) and in the top-level `Some` / `Min` arms of
`atomic_or_tseitin_body_with_extras` (`lib.rs:1464`, `:1475`) UNCHANGED
— LHS-trigger semantics ARE one-way, and top-level existential bodies
already get a fact at the outer atomic-existential-RHS lowering, so
duplicating one there would be redundant but not unsound.

**Soundness:** emitting `(F, R, B)` is sound because the in-body
context defines F to mean "has an R-witness in B" — the surrounding
Tseitin conjunction `B' ⊓ F ⊓ …` is exactly the encoding of a class
whose instances each carry an R-edge to a B-instance, so F entails
∃R.B by construction. The one-way trigger `∃R.B ⊑ F` was already
asserting the other direction; together they make F ≡ ∃R.B in this
construction, which is the intended semantics. No new triggers fire
on classes that aren't already subsumed by F.

**Termination:** the marker's `(role, body)` key is dedup'd by
`by_existential`, so the fact is emitted at most once per
distinct nested-existential shape; the `seen_facts` HashSet in the
worklist further dedups identical facts at push time.

**Expected closure effect on the canary:** with `(9, S_sub, 3)` (and
symmetrically `(6, S, 2)`) seeded as told facts, the trace's step 4
gates open and the closure derives `X ⊑ T` as the narrative above
walks through. Verified by inverting the canary assertion in Task 5.
