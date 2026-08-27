# How much of Kobayashi-MaRust is backed by Lean proofs? (2026-08-27)

Measured against KM **v1.1.0** (`/tmp/km-v110`), not the v0.2.32 tree audited earlier
(913 commits stale). Competitive-intelligence note for rustdl's own assurance story.

## The corpus is real and it compiles

| quantity | value |
|---|---:|
| Lean files | 425 |
| Lean lines | 119,825 |
| `theorem` declarations | 2,664 |
| actual `sorry` | **0** |
| custom `axiom` declarations | **0** |
| `lean_exe` checker targets | **105** |
| `KM_*` certification env vars | 130 |

`lake build` completes: **8,616 jobs, exit 0**, on `leanprover-community/mathlib4`
`v4.30.0-rc2`. `#print axioms` output shows theorems depending only on
`[propext, Classical.choice, Quot.sound]` — the three standard Lean kernel axioms.
So the theorems genuinely typecheck; this is not aspirational text.

**My earlier "13 `sorry`" was wrong** — all 13 hits were doc comments *asserting*
`sorry`-freeness. Grep the token, not the line.

## The pipeline closes end-to-end — verified, not inferred

`km classify` on a 2-axiom EL ontology with
`KM_ELC_LEAN_REQUIRED=1 KM_ELC_LEAN_CERT_CHECKER=<lean exe> KM_ELC_LEAN_CERT_OUT=…`:
a 1,518-byte certificate is written, the Lean-compiled checker **accepts** it silently,
and the published answer is correct (`A⊑B, A⊑C, B⊑C`).

Critically, on acceptance the code path is `return Some(certificate.verified_result())`
(`engine/src/elcomplete.rs:~6180`) — **the published answer is read out of the checked
certificate**, not merely accompanied by it.

### The checker verifies EXACTNESS, and 4/4 sabotages were caught

| sabotage | verdict |
|---|---|
| inject a false subsumption (`C⊑A`) | REJECTED |
| **delete an entailed subsumption** (`A⊑C`) | REJECTED |
| delete one derivation trace step | REJECTED |
| truncate the bound source ontology | REJECTED |

The second row is the strong one: the checker rejects an **incomplete** answer, so it
certifies the taxonomy as exact in both directions, bound to the source ontology.

**Two earlier "sabotages" of mine were no-ops** — certificate cells are dicts
(`{"sub":2,"sup":3}`), and an `isinstance(x, list)` guard silently skipped the mutation,
so an unmodified certificate was "accepted" and read as a checker failure. Assert the
mutation applied before interpreting the verdict. ([[calibrate-instrument-against-known-value]])

## The boundary — where it does NOT reach

1. **Off by default.** Plain `km classify` emits `{consistent, subsumptions,
   unsatisfiable, dropped}` with **no certificate key**. Certification is opt-in per
   `KM_*` env var, and only 5 of 41 routes are `certified_*`.
2. **Fails closed by DECLINING, and silently.** A rejected certificate returns `None`,
   which makes the EL route decline — an *uncertified* engine then answers, and stdout is
   byte-identical to the uncertified baseline. Sound (no unchecked EL answer is
   published) but the loss of coverage is invisible to the caller. Same shape as rustdl's
   own silent-sound-underapproximation problem.
3. **Setting the env vars does not mean they were used.** On `pizza.ofn` (disjunction ⇒
   not EL-routed) the certified arm wrote no certificate, printed nothing, and returned
   output identical to the plain arm. Absence of a certificate is not evidence of a
   certified run.
4. **Coverage is shape-limited.** A 3-axiom EL ontology adding
   `SubClassOf(ObjectSomeValuesFrom(:r,:C), :B)` was **rejected** — compound-LHS
   existential is outside the producer's certified shapes. It declined rather than
   overclaiming, which is correct, but the certified fragment is narrower than "EL".
5. **KM's own docs are candid about the rest.** `docs/CB-CERTIFICATION.md` §"Not yet
   established": the refinement between the Rust engine's actual context stores /
   blocking / termination state and the abstract calculus in the completeness theorems
   **remains open**; the blocked/global document "is still supplied externally rather
   than generated from the production run"; "omitted taxonomy cells also remain
   unproved"; negative cells are closed only where a **one-element** model suffices, and
   SROIQ lacks the finite-model property in general. Their own line:
   *"Benchmark agreement with another reasoner is regression evidence only."*

## Honest summary

KM has a **large, real, kernel-checked Lean formalisation** (2,664 theorems, 0 `sorry`,
0 custom axioms) and a **working proof-carrying pipeline** whose checker enforces
exactness and rejects tampering. What it does not yet have is that pipeline covering the
**default** classification of **arbitrary SROIQ** input: it is opt-in, shape-limited,
declines silently outside its fragment, and the production-engine-to-abstract-calculus
refinement is explicitly unfinished.

For rustdl: `rustdl prove` / `justify` occupy adjacent ground with a different trade —
no formal kernel behind them, but they run on the default path over the full supported
fragment. The two assurance stories are complementary, not ranked.

---

## What transfers to rustdl (measured, 2026-08-27)

### MEASURED OUT: KM's "drop the parse tree before saturation" idiom

`engine/src/elcomplete.rs` carries an explicit memory idiom — *"Drop it BEFORE
saturation so the parse tree never coexists with the peak saturation state"* — and
rustdl structurally does the opposite: every one of ~30 CLI commands does
`let onto = parse_ofn(&file)?` and passes `&onto` into reasoning, so the horned-owl
`SetOntology` stays resident for the whole run (and `dropped_block` re-reads it after).

**It has no addressable set in rustdl.** Peak RSS, `tbox-stats` (parse+convert, no
reasoning) vs full `classify`, `RAYON_NUM_THREADS=1`:

| ontology | file | convert-only peak | classify peak | conversion share |
|---|---:|---:|---:|---:|
| `ore_ont_12898` | 564 MB | 7,633,432 kB | 7,633,208 kB | **100%** |
| `ore_ont_10926` | 558 MB | 7,562,580 kB | 7,562,416 kB | **100%** |
| `ore_ont_2121` | 172 MB | 2,803,736 kB | 2,803,884 kB | **100%** |
| `ore_ont_11395` | 166 MB | 2,429,280 kB | 2,429,376 kB | **100%** |
| `ore_ont_9347` | 8.2 MB | 123,792 kB | 228,620 kB | 54% |
| `ore_ont_1508` | 3.8 MB | 63,284 kB | 181,976 kB | 35% |
| `ore_ont_10019` | 32 KB | 6,612 kB | 15,044 kB | 44% |

The correlation is **inverse to need**: on every multi-GB ontology, 100% of peak is
reached *before reasoning begins*, so freeing the parse tree afterwards cannot lower the
peak — it has already happened. Where the lever would bite (35–54% conversion share),
absolute peak is already 15–228 MB, so it saves tens of MB. A lever that applies only
where there is no problem.

**Corollary worth more than the lever:** rustdl's multi-GB RSS tail is a **conversion**
problem, not a reasoning problem. This independently agrees with
`2026-08-22-parse-is-the-wall-on-the-largest-files.md` and with the mechanism partition
(conversion-bound work converts 1:1 to wall because no budget bounds it).

### The idea actually worth taking: NEGATIVE certificates

rustdl can prove **positives** (`justify`, `prove`) but has **no artifact for a
negative** — a reported non-subsumption is backed only by "the engine did not derive it".
Consequently every completeness claim rustdl makes rests on an **external oracle**
(Konclude ∪ HermiT), which is why the FP=0 net is FP-shaped and the MISSED net needs a
400-ontology oracle population and ~10 min per arm.

KM's exact-taxonomy design is the shape of the fix: a checker demands a cell for **every**
pair, where a positive cell cites a derivation and a **negative cell carries a checked
finite countermodel**; a cell it cannot close is marked `unresolved` and the checker
**rejects** that, so the obligation is counted rather than assumed. Their producer closes
negatives by reducing the clause set to SAT under a **one-element** interpretation and
emitting concept/role/constant tables that Lean re-checks against every source clause,
with explicit search bounds and `unresolved` on exhaustion.

Two reasons this fits rustdl specifically:

* The wedge already builds a `Sat` completion. rustdl also already knows (see
  `known-limitations/realize-drops-derived-individual-equality.md`) that such a completion
  is a **pre-model**, not a model — which is exactly why it cannot today be used as
  evidence. A countermodel emitter is the missing step that turns it into evidence.
* "Bounded search, `unresolved` on exhaustion" is rustdl's existing
  sound-under-approximation idiom, so it needs no new soundness argument — only a new
  *observability* one.

Payoff: a self-verifying completeness check that needs no peer reasoner, and which would
make `RUSTDL_HYPERTABLEAU_TRUST_SAT`'s soundness — today an empirical corpus claim —
checkable per pair.

### Also transferable

* **Publish the answer OUT of the checked artifact.** KM's accept path is
  `return Some(certificate.verified_result())`. rustdl's `prove`/`justify` are *separate
  surfaces* from the classify answer, so a proof cannot gate publication. A `--verify`
  mode whose answer is read from a validated structure is a strictly stronger contract.
* **An exactness checker catches MISSES structurally.** Deleting an entailed subsumption
  from a KM certificate is rejected. Every rustdl gate today is FP-shaped or oracle-shaped.
* **A standing `## Not yet established` doc section**, plus their line *"Benchmark
  agreement with another reasoner is regression evidence only."* rustdl's own
  best-documented failure mode is the design record drifting optimistically ahead of the
  engine; a standing falsifiable-claims section is a cheaper guard than scattered
  corrections.

### Do NOT copy

**105 `lean_exe` checker targets and 130 `KM_*` env vars.** rustdl already pins 43 flag
defaults behaviourally *because* they drifted. KM's surface is why their own boundary
takes an 803-line document to state.
