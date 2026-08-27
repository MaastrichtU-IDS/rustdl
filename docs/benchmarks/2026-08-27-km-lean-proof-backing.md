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
