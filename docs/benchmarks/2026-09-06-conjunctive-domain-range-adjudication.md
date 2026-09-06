# Conjunctive `ObjectPropertyDomain`/`Range` filler (#110) — three-oracle adjudication and sabotage battery

Date: 2026-09-06. Branch `fix/conjunctive-domain-range-filler-110`, Task 3.
Binaries pinned per configuration:

| arm | commit | sha256 (prefix) |
|---|---|---|
| BEFORE | `0abc21b` (pre-#110) | `ac6477fc` |
| AFTER | `2fa8d0f` (Tasks 1+2) | `e9f11f43` |

The AFTER binary was **rebuilt from the tree after the last sabotage revert and is
byte-identical to the pin** (`e9f11f43…` both times). That identity is the evidence that
all five sabotages are fully reverted, not an assertion that they are.

---

## 1. What was wrong, stated precisely

`collect_el_rules`' `ObjectPropertyDomain`/`Range` arms handled `Bot` (poison) and `Atomic`
(push) and **fell through silently on `And`**. `Domain(r, P ⊓ Q)` therefore reached the EL
saturator as *nothing at all*.

The EL gate was not the liar — `is_el_axiom` correctly refused the axiom, so the ontology
routed to the hybrid path. **The false certification is the `Horn` banner plus the tier
walk**: dropping the filler leaves `X` with no EL subsumer, so the tier walk (which groups
by EL/told subsumer count and never compares same-tier classes) never compares `X` against
`P`, and `classify` returns **zero rows with `incomplete: false` and `dropped: {}`** under
`# fragment: Horn (trust_sat sound by construction; hyper Horn fixpoint is complete)`.

That is the D10 shape: a gate certifying complete over an engine that dropped the axiom.

**This closes a defect the design record already carries as live and UNFIXED.** CLAUDE.md's
`owl-dl-verify` section records exactly this shape — *"`ObjectPropertyDomain(r, P ⊓ Q)` …
`direct_subsumptions: []` with `incomplete: false`"*, recovered only by
`RUSTDL_CLASSIFY_SAME_TIER=1`. It now closes **at the default, with no flag**.

---

## 2. Oracle adjudication — 6 of 6

**Comparison, as defined:** the set of **named** subsumption pairs, excluding every row whose
superclass is `owl:Thing` and every row mentioning `owl:Nothing`. This exclusion is not
cosmetic — Konclude emits **6 `Thing` rows on `dom_conj`, of 8 `<SubClassOf>` blocks total**,
so a raw `<SubClassOf>` count compares nothing. Konclude was judged from **content, never exit
code** (it exits 0 on a missing file, writing an ~896-byte `Thing`/`Nothing` stub); every
output here is 2414–2537 bytes, so every run genuinely classified.

The Konclude parser was validated independently rather than trusted: a raw
`grep -c '<SubClassOf>'` reproduces the parsed block totals exactly (8/7/7/7/8/7), and the
files contain **zero** `abbreviatedIRI` attributes, so the IRI regex dropped nothing.

| probe | rustdl BEFORE | rustdl AFTER | Konclude | HermiT | KM | agree? |
|---|---|---|---|---|---|---|
| `dom_conj` | **∅** | `X⊑P`, `X⊑Q` | `X⊑P`, `X⊑Q` | `X⊑P`, `X⊑Q` | `X⊑P`, `X⊑Q` | ✅ |
| `dom_atomic` | `X⊑P` | `X⊑P` | `X⊑P` | `X⊑P` | `X⊑P` | ✅ |
| `rng_conj` | **∅** | `X⊑W` | `X⊑W` | `X⊑W` | `X⊑W` | ✅ |
| `rng_atomic` | `X⊑W` | `X⊑W` | `X⊑W` | `X⊑W` | `X⊑W` | ✅ |
| `dom_partial` | **∅** | `X⊑P`, `X⊑Z` | `X⊑P`, `X⊑Z` | `X⊑P`, `X⊑Z` | `X⊑P`, `X⊑Z` | ✅ |
| `dom_disj` | ∅ | ∅ | ∅ | ∅ | ∅ | ✅ |

**Post-fix rustdl, Konclude, HermiT and KM produce the identical named set on 6 of 6.**
Every AFTER row carries `consistent: true`, `incomplete: false`, `dropped: {}`, `unsat: []`.

Fragment banner, BEFORE → AFTER:

| probe | BEFORE | AFTER |
|---|---|---|
| `dom_conj` | `Horn` (mode hybrid) | **`pure-EL`** |
| `rng_conj` | `Horn` (mode hybrid) | **`pure-EL`** |
| `dom_atomic` / `rng_atomic` | `pure-EL` | `pure-EL` (unchanged control) |
| `dom_partial` | `Horn` | `Horn` (gate correctly refuses; hybrid recovers) |
| `dom_disj` | `out-of-EL` | `out-of-EL` (unchanged) |

### 2.1 `dom_partial` — the brief's replacement probe, and why it strengthens the fix

The brief's original `dom_partial` asked whether the oracles report `X ⊑ Q` where `Q` does not
occur in the axiom — trivially "no", so no implementation could fail it. The version used here
is `Domain(r, P ⊓ ∃s.S)` plus `SubClassOf(∃s.S, Z)`, asking whether **`X ⊑ Z`** is derived —
i.e. whether the conjunct the *engine did not process* still reaches an answer.

It does, and that is the point of Task 2. `decompose_role_filler` returns `false` on this
filler, `is_processed_role_filler` therefore **refuses the fragment**, and the ontology routes
to the complete hybrid path, which derives both `X ⊑ P` and `X ⊑ Z`. Partial decomposition is
sound to *derive from* but must not be *certified complete* — and the probe shows the refusal
is not a loss.

### 2.2 `dom_disj` is a discriminating control, not an ambiguous silence

Konclude is documented to under-report, so its silence needs licensing. It is licensed
affirmatively here: on `dom_disj` Konclude places `X` in the **`Thing` list** (7 `Thing` rows,
0 named) — an explicit "no named parent" — while on `dom_atomic` it emits `X ⊑ P` and `X` is
*absent* from the `Thing` list. Same for HermiT: its `dom_disj` output is 1 byte while
`dom_atomic`, through the same wrapper on the same file family, is 61 bytes containing the
domain-derived row. Both oracles are demonstrably reasoning about `ObjectPropertyDomain` on
these files and declining `dom_disj` on the merits.

`Domain(r, P ⊔ Q)` entails `X ⊑ P ⊔ Q` and **neither disjunct**, so a reasoner reporting
`X ⊑ P` here has a false positive. The fix is **provably inert** on `dom_disj`: BEFORE and
AFTER produce identical output. That inertness is what shows the fix is not over-broad in the
FP direction.

### 2.3 Two probe limits, stated rather than implied

- **`rng_conj` observes only the `P` conjunct.** Its consuming axiom is `∃r.(B ⊓ P) ⊑ W`;
  nothing in the probe consumes `Q`. So the oracle set does **not** demonstrate that both
  range conjuncts reach a witness — only `dom_conj` does that, via two separate rows. The
  canary `conjunctive_range_filler_derives_every_conjunct` is what covers the range half.
- **No integration probe can observe sabotage 5** (see §3.5).

---

## 3. Sabotage battery — 6 run, 5 caught, 1 survived

Each sabotage was applied to source, evaluated with `cargo test` (a *pinned binary cannot see
a source sabotage*), then reverted and the tree confirmed clean with `git diff --exit-code`
before the next. Failures are reported **by test name**; the granularity is the finding.

Baseline, unsabotaged: `conjunctive_domain_range_filler` 11/11, `owl-dl-saturation --lib
decompose` 7/7, `horn_widening_market` 3/3.

**Direction of risk differs and is not uniform.** #3 is the only sabotage whose survival would
mean **unsoundness** (a real FP); #1/#2/#4/#5 are completeness-direction.

### 3.1 Sabotage 1 — revert both engine arms to the `Atomic`-only branch → **CAUGHT**

Predicted: the 2 bug canaries + the partial canary. **Observed 5 failures** — the 3 predicted
plus both provenance canaries:

```
conjunctive_domain_filler_derives_every_conjunct                  FAILED
conjunctive_range_filler_derives_every_conjunct                   FAILED
partially_decomposable_filler_still_derives_its_atomic_conjunct   FAILED
prove_attributes_the_conjunctive_domain_subsumption_to_its_axiom  FAILED
prove_attributes_the_conjunctive_range_folded_subsumer_to_its_axiom FAILED
```

**The first reading of this was wrong, and the correction is the useful part.** The patch
asserted a unique match on each pattern, so it hit only the *real* Pass-1 arms
(`lib.rs` ~3519/~3532); the two provenance mirrors in `collect_el_rules_with_provenance`
(~3157/~3225) were left decomposing. The two `prove` canaries therefore failed only because
the underlying subsumption had vanished — **nothing to do with the mirrors**. Calling that an
"over-catch" would have recorded coverage this sabotage does not provide, leaving those
canaries' *stated purpose* (guarding the mirror against drift from the real pass) unverified.
See §3.6, which runs the sabotage that actually tests it.

### 3.2 Sabotage 2 — `decompose_role_filler`'s `And` arm returns `true` unconditionally → **CAUGHT**

Predicted `a_partially_decomposable_filler_is_refused_by_the_gate`; that failed, plus two unit
tests:

```
a_partially_decomposable_filler_is_refused_by_the_gate            FAILED
decompose_reports_incomplete_but_still_pushes_the_atomic_half     FAILED
decompose_does_not_short_circuit_when_the_bad_conjunct_sorts_first FAILED
```

### 3.3 Sabotage 3 — add a `ConceptExpr::Or` arm that decomposes like `And` → **CAUGHT**

The unsoundness-direction sabotage. Both predicted canaries failed, plus three unit tests:

```
a_disjunctive_filler_does_not_decompose      FAILED  (panic: "FP: a disjunct is not a domain")
a_disjunctive_filler_is_refused_by_the_gate  FAILED
decompose_declines_a_disjunction_and_pushes_nothing FAILED
decompose_reports_incomplete_but_still_pushes_the_atomic_half FAILED
decompose_does_not_short_circuit_when_the_bad_conjunct_sorts_first FAILED
```

The panic message is the finding: under this sabotage `Domain(r, P ⊔ Q)` yields `X ⊑ P` — a
genuine false positive, caught with a control (`dom_atomic`/`rng_atomic`) that shows the
oracles *do* report in the entailed case.

### 3.4 Sabotage 4 — re-implement `is_processed_role_filler` locally → **SURVIVED (predicted)**

Replacing the delegation with a local `matches!(Atomic | Bot | Top | And(all atomic))` leaves
**the entire workspace green — 1960 passed, 0 failed, 85 ignored**, not merely the 21 tests of
the three targeted binaries. The frame matters here and only here: catches can only accumulate
as the frame grows, so #1/#1b/#2/#3/#5 are frame-insensitive, but #4 is the one sabotage whose
*headline could flip*, and two files that reference the gate by name
(`conjunctive_unsat.rs:878`, `flag_defaults.rs`) sit outside the narrow frame. They do not
catch it either. This was predicted; recording it is the point.

**But the survivor statement is stronger than "they agree today", and the extra probe is what
established that.** The obvious distinguisher is a *nested* conjunction —
`Domain(r, P ⊓ (Q ⊓ W))`, where the recursive decomposer returns `true` and pushes all three
while `And(all atomic)` returns `false`. Built as `dom_nested.ofn` and run under a
**sabotage-4 build**: it still reports `# fragment: pure-EL` and derives `X⊑P`, `X⊑Q`, `X⊑W`,
identical to the correct binary.

The reason is structural, not luck: **`ConceptPool::and` flattens nested `And` at intern time**
(`crates/owl-dl-core/src/ir.rs:374`, `ConceptExpr::And(inner) => v.extend_from_slice(inner)`),
also dropping `Top` operands and short-circuiting `Bot`. So an `And` operand can never itself
be an `And`, `Top`, or `Bot`, and on **every concept the interner can construct** the two
predicates are extensionally equal on the boolean the gate reads.

That "every constructible input" is enforced at a **single chokepoint, by privacy**, which was
checked rather than assumed: `intern_raw` is private to `ir.rs`, and a workspace-wide grep for
`ConceptExpr::And(` finds exactly one **construction** site — `ir.rs:389`, inside
`ConceptPool::and` — every other hit being a destructuring pattern.

So the honest limit is not "the canaries missed a divergence" but: **these canaries pin
behaviour, not the no-drift property.** No-drift rests on the shared call site plus an
invariant maintained in a *different crate*. If `ConceptPool::and` ever stopped flattening,
the local and delegated forms would silently diverge and nothing here would notice. That
coupling is untested.

### 3.5 Sabotage 5 — short-circuit the `And` loop on the first `false` → **CAUGHT, by exactly one test**

```
decompose_does_not_short_circuit_when_the_bad_conjunct_sorts_first  FAILED
```

The integration suite is **completely blind**: 11/11 pass. This is the honest limit the brief
predicted, and it is structural — `ConceptPool::and` **sorts operands by `ConceptId`**
(`v.sort_unstable()`), so an integration fixture cannot choose which conjunct comes first;
that ordering is an interning artefact. The unit test, which builds the `And` directly with
the non-decomposable conjunct sorting first, is the **entire net** for this mutation — the
same shape of limit CLAUDE.md records for the `IntSet::disjoint` relaxation.

### 3.6 Sabotage 1b — revert **only** the provenance mirrors → **CAUGHT**

Added after §3.1's misreading was caught. The real Pass-1 arms are left intact and only
`collect_el_rules_with_provenance`'s two mirrors are reverted to `Atomic`-only, so the
derivations survive and *only* the attribution can break. Predicted: the two `prove` canaries
fail, the three derivation canaries pass. Observed exactly that:

```
prove_attributes_the_conjunctive_domain_subsumption_to_its_axiom    FAILED
prove_attributes_the_conjunctive_range_folded_subsumer_to_its_axiom FAILED
9 passed (incl. all three derivation canaries); saturation 7/7; horn_widening_market 3/3
```

So the mirrors **are** guarded, and by the canaries written for them — verified, not assumed.
Without this run the branch would have shipped two guard tests whose stated purpose was
untested, which is exactly [[sabotage-your-own-guard-tests]].

---

## 4. Gates

- **Full workspace suite: 1960 passed, 0 failed, 85 ignored**, run after the final revert.
  `owl-dl-py` is excluded: its *lib test* fails to **link** in this environment with undefined
  CPython symbols (`PyExc_Exception`, `PyErr_Fetch`, …) — a missing libpython for the pyo3
  extension, environmental and untouched by #110, which changes no Python surface.
- **Tree clean** (`git diff --exit-code`) after each of the **six** reverts, and the rebuilt
  release binary is **byte-identical** to the pre-sabotage pin (`e9f11f43…`), re-confirmed
  after the final revert of the second sabotage-4 run.

---

## 5. What this evidence does and does not license

### 5.1 The published superset figure was wrong, and the corpus REWARD is ZERO

**The superset is 27 ORE ontologies, not the ≤14 recorded for this shape elsewhere in the
design record.** That smaller figure
came from a scan whose `break` fired after the first axiom body matching a broad construct
regex, so any ontology whose *first* `Domain`/`Range` axiom happened to be non-conjunctive was
wrongly excluded. Two independent instruments now agree at 27 — a corrected balanced-paren
scan, and

```sh
grep -lE 'ObjectProperty(Domain|Range)\([^)]*ObjectIntersectionOf' \
  /data/dumontier/ore-run/pool_sample/files/* | wc -l   # 27
```

re-derived here. **Prefer 27; the ≤14 is stale wherever it still appears.**

> **SUPERSEDED 2026-09-06 by Task 4 — see §3 of
> `docs/benchmarks/2026-09-06-conjunctive-filler-sweep.md` for the corrected attribution.**
> The `ore_ont_4796` gain below is real and KM-confirmed, but it is **not attributable to
> #110**: the pre-fix binary gains the same two pairs under `RUSTDL_CLASSIFY_SAME_TIER=1`, and
> BEFORE and AFTER are triple-identical under *either* flag setting. The comparison here crossed
> a flag boundary (default vs `SAME_TIER`) within one binary rather than comparing arms under
> the same flag. Corpus reward for #110 is measured **zero**. The analysis below is otherwise
> unchanged.

**The shape occurs in the wild, but the observation below is FLAG-ATTRIBUTABLE — see the box
above before reading it as reward.** On `ore_ont_4796` (DOLCE-Lite), `RUSTDL_CLASSIFY_SAME_TIER=1`
gains exactly `DOLCE-Lite#agent ⊑ DOLCE-Lite#endurant` and `DOLCE-Lite#agent ⊑
DOLCE-Lite#particular` — **transitive closure 1,224 → 1,226, lost 0** — stable at both the 5 ms
default and `--pair-timeout-ms 1000`, and **KM derives both**. Since the flag is the documented
workaround for precisely the tier-walk half of this defect, that is the shape biting a real
ontology rather than only a synthetic.

**Measure this on the CLOSURE, not the Hasse relation.** The `direct_subsumptions` diff shows
only `gained=1`: the second pair is transitively implied and therefore has no direct row. A
row-count comparison understates the gain — the fifth recorded instance of the direct-vs-closure
trap in this repository.

**The bound that survived this task — no full ORE sweep was run here — is now CLOSED by Task 4,
and it closed against the expectation.** `ore_ont_4796` corroborates that the shape occurs in the
wild; it never measured the population, and the population measurement came back **zero**: 45
IDENTICAL / 0 DIFFER / 2 UNMEASURED / 0 lost entailments over 25 measured bearing + 20 controls,
and 0 fragment-routing movers across all 1,920 (§3 of
`docs/benchmarks/2026-09-06-conjunctive-filler-sweep.md`). So the evidence for #110 is **the
canaries and this three-oracle adjudication** — the corpus shows inertness, not confirmation.

### 5.2 What is established

On six probes spanning the fixed shape, its atomic control, the partially-decomposable fallback
and the disjunctive FP guard, **four independent reasoners agree exactly**; the pre-fix binary
silently returned nothing on three of them while certifying `Horn`; and **five of six mutations
to the fix are caught by name**, the sixth being a survivor provably unobservable through the
gate's own interface — bounded over the full 1960-test workspace, not a narrow frame.
