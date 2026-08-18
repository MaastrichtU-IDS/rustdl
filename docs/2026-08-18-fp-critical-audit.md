# FP-critical audit: fragment gates and DKey bucket keying

**Date:** 2026-08-18 · The second half of the targeted review (the first half being flag
defaults). Scope chosen because a bug here is a **wrong answer**, not untidiness, and because
the curated corpus is documented as *inert* for the DKey area, so a green FP=0 net cannot
catch drift.

Method: the **D10 detection recipe** — for every construct a fragment gate ADMITS, check the
engine has a rule that consumes it. That recipe has found six real instances in this
repository.

## 1. `saturator_complete_fragment` — one misleading claim, no defect

Diffing the gate's admissions against the axiom variants the saturator matches leaves six
admitted-but-unconsumed:

| admitted, not consumed | verdict |
|---|---|
| `DeclareClass`, `DeclareNamedIndividual`, `DeclareObjectProperty` | no reasoning content — fine |
| `InverseObjectProperties`, `SymmetricRole` | admitted only when the role is provably unread — audited in §3 |
| **`InverseFunctionalRole`** | admitted unconditionally; saturator NEVER reads it |

The arm's comment claimed the admission existed for "the Phase-2 functional /
**inverse-functional** witness-merge", i.e. that the engine consumes it. It does not —
`grep Axiom::InverseFunctionalRole crates/owl-dl-saturation` finds nothing. That reads as a
textbook D10 defect and cost an investigation to clear.

**It is not a defect.** Inverse-functionality constrains PREDECESSORS: at most one
`r`-predecessor per node. The fragment admits no nominals, no `ABox` assertions and no inverse
role *use*, so the canonical model is a **tree** — every `∃`-witness is created by exactly one
parent and has exactly one predecessor. The constraint holds by construction and entails
nothing.

Adjudicated empirically, not only argued: three probes where it could plausibly bite (shared
filler; two sub-roles of one inverse-functional role into one filler; inverse-functional +
functional + transitive on a chain) give closures **identical to Konclude**. Comment corrected
to state the real justification and to warn that widening the fragment to nominals or `ABox`
makes the arm a genuine defect. Canaries: `inverse_functional_inert.rs`.

**Two guard attempts failed and are recorded**, because the failures are the useful part:

1. Sabotage showed the three closure tests are **blind** to a fragment widening — admitting
   `ClassAssertion`/`ObjectPropertyAssertion` left all three passing, since their fixtures
   contain no `ABox`.
2. A purpose-built tripwire also failed, for an unrelated reason: the `ABox` fixture reaches
   the fast path via **`is_pure_el`, a different gate**, and the forced `x = y` merge yields no
   CLASS subsumption — so there was no demonstrated defect to assert. Asserting "must not
   reach the fast path" would have pinned an unjustified requirement, so it was removed rather
   than shipped.

**Open question, deliberately unanswered:** whether inverse-functional + `ABox` is complete on
the `is_pure_el` path for **`realize`**, where individual identity *is* observable. `realize`
has its own gate and needs its own investigation.

## 2. `is_pure_el` — preconditions verified

Its admitted-but-unconsumed set is the same minus `InverseFunctionalRole` (which it does not
admit): declarations plus the two bare role declarations.

Two adjacent preconditions checked and **holding**:

* `tbox_only_saturator_eligible` does enforce `!ontology_uses_nominals(internal)`, which is
  what makes dropping the `ABox` verdict-safe by monotonicity.
* `is_pure_el_impl(internal, skip_abox = true)` is why an `ABox`-bearing ontology can reach the
  fast path at all — Lever 1, legitimately dropping the `ABox` for CLASS classification. This
  closes the loose end from §1's failed tripwire.

## 3. Bare role declarations — correct, and now canaried

`RUSTDL_FRAGMENT_BARE_DECL` (default ON, changed dispatch for **44 ORE ontologies**) admits a
`SymmetricObjectProperty` / `InverseObjectProperties` declaration when the role's edges are
provably unread. Since the saturator reads neither axiom, admitting one **drops its semantics
while reporting complete** — sound only if the judgement holds.

The load-bearing step is a **downward closure** in `BareRoleDecls::analyze`: `r ⊑ s` with
observable `s` makes `r` observable, because `r`-edges *are* `s`-edges and `s` is read. So a
symmetric role appearing in no concept can still be observable purely through a super-role.
Verified behaviourally — the indirect case goes hybrid, the genuinely-inert one stays on the
fast path.

**It was untested.** The three files mentioning the flag pin it *off* for unrelated reasons.
Now canaried by `bare_role_decl_observability.rs` (4 tests incl. the inert control, without
which the suite would pass with the flag doing nothing and its 44 recoveries silently lost).
Sabotage: deleting the closure fails **exactly** the indirect test with the other three
passing, so that test is provably its only coverage.

## 4. DKey bucket keying — no drift; the best-canaried surface in the tree

This is where the one real shipped FP came from: `parse_float_oneof` folded `xsd:float` and
`xsd:double` into one f64 bucket, so `∃h.DataOneOf("1.0"^^xsd:float)` and its `xsd:double` twin
were reported **EQUIVALENT**, live from v0.4.6 to v0.4.9.

The invariant is that each datatype owns a bucket namespace and edges are seeded only WITHIN a
bucket; a cross-bucket edge is an FP. Audit result:

* **13 buckets**, all covered. `parser_matrix_mutual_exclusivity` is a full N×N over the 7
  interval/string decoders (int, float, double, dec, date, dt, str);
  `numeric_oneof_parser_matrix_exclusivity` covers the 6 `DataOneOf` buckets (io, fo, dbo, deo,
  dao, dto) **and** cross-checks them against every interval/string decoder — including the
  untagged integer decoder, the riskiest because it does no tag check.
* **No drift**: every one of the 6 oneof decoders and 7 interval/string decoders has a matrix
  entry. No encoder exists without a decoder in a matrix.
* **The historical FP cannot return silently.** Re-pointing `parse_float_dkey_iri` at
  `DKEY_DOUBLE_TAG` fails **both** matrices with an exact message.

### Method note: a no-op sabotage reads as a pass

The first attempt at that sabotage edited a literal `"f:"` inside
`parse_float_dkey_iri` — which does not contain one, because it delegates to
`parse_tagged_float_dkey_iri(iri, DKEY_FLOAT_TAG)`. The replacement matched nothing, the tests
passed, and that would have been recorded as "the canary caught it" had the diff not been
checked. **Verify the sabotage actually changed behaviour before believing either outcome.**

## Overall

No FP-critical defect found. What the pass produced instead:

* one **misleading comment** on an FP-adjacent gate arm, corrected with its real justification
  and a warning about the conditions that would invalidate it;
* **two untested load-bearing judgements** now canaried (inverse-functional inertness, bare-decl
  observability), each with sabotage proving the coverage;
* two **preconditions verified** rather than assumed (nominal-freeness on the TBox-only path,
  the reason `ABox` ontologies reach the fast path);
* one **open question** stated rather than guessed (`realize` on the `is_pure_el` path);
* and confirmation that the surface which shipped the real FP is now the best-guarded one here.

The pattern worth carrying: on this codebase the recurring defect is **not** wrong logic but
**stale or misleading claims about correct logic** — which is the same finding as the flag-default
audit, and the same failure mode that made five of this session's proposals target already-shipped work.
