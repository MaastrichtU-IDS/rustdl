# `verify-el` corpus re-scan after the #87 fixes (2026-09-04)

Closes #87. Before: **8 `Violated`, all eight the instrument's own false positives** — a
0-for-8 record against the crate's headline claim that a `Violated` is a real engine
defect. After both mechanisms are fixed: **0 `Violated` across all 1,920**.

## Result, with the deltas as the consistency check

| verdict | before (#87) | after | delta |
|---|---:|---:|---|
| **violated** | **8** | **0** | **−8** |
| verified | 358 | 362 | +4 |
| unresolved (off-fragment / refused) | 1,357 | 1,361 | +4 |
| **timeout at 60 s — UNMEASURED, not passing** | **196** | **196** | 0 |
| I/O or parse error | 1 | 1 | 0 |
| total | 1,920 | 1,920 | |

The deltas are what make this trustworthy rather than merely green: **+4 verified is exactly
the four F4 ontologies, +4 unresolved is exactly the four F5 ontologies, −8 is their sum, and
nothing else moved.** The timeout count reproducing at 196 on the same cap is an independent
stability signal. Per-ontology rows: `2026-09-04-verify-el-corpus-rescan.tsv`.

The eight, individually:

| ontology | mechanism | before | after |
|---|---|---|---|
| `ore_ont_11522` | F4 | violated, 8 violations | verified, 0 |
| `ore_ont_14128` | F4 | violated, 2 | verified, 0 |
| `ore_ont_14826` | F4 | violated, 19 | verified, 0 |
| `ore_ont_7270` | F4 | violated, 2 | verified, 0 |
| `ore_ont_12317` | F5 | violated, 25,369 | unresolved, exit 3 |
| `ore_ont_13220` | F5 | violated, 20,292 | unresolved, exit 3 |
| `ore_ont_15249` | F5 | violated, 54,304 | unresolved, exit 3 |
| `ore_ont_283` | F5 | violated, 92,573 | unresolved, exit 3 |

The single `ERR` is `ore_ont_10860`, the known `horned-owl` SWRL `BuiltInAtom` grammar gap —
it does not parse, and is unrelated.

## What this licenses, and what it does not

**Licensed:** `verify-el` can now be run **unattended** as a D10 hunter. That is the actual
prize. The loop that produced most of this project's recent engine fixes has been *reactive* —
an oracle disagreement or a hand-built probe surfaces something, then it gets root-caused. A
clean instrument makes it *systematic*.

**Not licensed:** "no engine defects exist". Three limits, all measured:

* **Coverage is 19%.** 362 of 1,920 are checkable. The binding constraint is the `is_pure_el`
  fragment restriction, not defect density — extending to the Horn fragment is the obvious
  follow-on and a materially bigger piece of work.
* **196 ontologies are UNMEASURED**, not passing. They hit the 60 s cap, which is "no verdict
  reached" — a different thing from an exit-3 `Unresolved`.
* **F1 and F3 are still live** builder false-positive mechanisms
  (`verify-two-expansion-paths-split-a-witness.md`). So a future `Violated` is a **lead
  requiring adjudication**, not a proof. What changed is that it is no longer *guaranteed* to be
  spurious.

## The fixes, in one line each

**F5** (`8bbadb7`) — `fold_build_reasons` kept `Violated` when a build reason said the model was
TRUNCATED. A truncated model is smaller than the axioms describe, so an axiom that would have
been witnessed cannot be, and the violations are artifacts of the cut. Deliberately narrower
than "any reason": a *localized* gap leaves a violation meaningful.

**F4** (`5a9ea3a`) — `required_atoms` on `⊤` returns an EMPTY antecedent, and
`expand_from_axioms` read empty as "no rule" when it means "fires on every element". Fixed at
two points: a label floor applied at `FiniteModel::intern` (the single chokepoint), plus letting
a universal antecedent fire as a rule so `⊤ ⊑ ∃r.C` is materialised.

## Three things this cost, worth keeping

**F4 and F2 were ONE mechanism and neither write-up said so.** F2 was filed as a builder
imprecision, F4 as a corpus-scale instrument bug; both reduce to "the Tseitin marker's label is
never closed under `⊤ ⊑ C`". It surfaced because F2's test **failed** — it asserted the false
positive, and had been written to trip on a fix rather than being `#[ignore]`d. That is the only
reason it did not pass unnoticed.

**The obvious fix was the dangerous one.** `required_atoms` returns empty both for `⊤` and for
every shape it cannot decompose (`Some`/`Or`/`Not`/`Min`/`All`). Reading empty as universal
would apply consequents to every element unconditionally, and the failure direction is not a
spurious violation but a **false all-clear** — the one outcome this crate exists to prevent. The
predicate therefore tests for `⊤` structurally, never for emptiness.

**The first over-broadness guard did not guard.** It asserted only that a baseline verifies, and
under the sabotage (flipping `_ => false` to `_ => true`) it **passed** — the sabotage was caught
by an unrelated cascade test instead. Rewritten around a `DisjointClasses` that makes the
over-broad labelling observable; it now fails by name.

## A trap that outlives this crate

Any oracle comparison that counts `Thing` rows reports spurious MISSED on any ontology asserting
`⊤ ⊑ C` — and this project has already been bitten once, when 73% of an apparent ~1,795-row gap
against Kobayashi-MaRust turned out to be the same ⊤-convention artifact. Worth auditing in the
comparison tooling independently of anything here.
