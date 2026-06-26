# NomKey distinctness from DifferentIndividuals — saturation increment (M2) design

**Status:** design. The first concrete engine increment from the Konclude representation study
(`docs/stage4-konclude-representation-study-2026-06-26.md`). The study's elimination argument
established that Konclude's wine speed comes from **saturation-driven deterministic expansion**, and
the code comparison identified a **specific determinism rustdl's EL saturator lacks**: Konclude's
global saturation-time ATMOST-insufficiency fires on `≤1 R` over distinct nominals; rustdl's
functional-merge→unsat machinery is present and fully propagated, but **`DifferentIndividuals` is
never fed into the saturator's NomKey `disjoint_pairs`**, so the nominal value-partition case
misfires at the class level. This spec adds exactly that rule.

## Premise (verified)

- Wine asserts the needed distinctness: `DifferentIndividuals(vin:Delicate vin:Moderate vin:Strong)`
  (body), `DifferentIndividuals(vin:Dry vin:OffDry vin:Sweet)` (sugar), etc. — so the increment is
  applicable, not inert.
- rustdl already lowers `ObjectHasValue(R,a)` → `∃R.NomKey(a)` (`introduce_nominal`), and already
  has the functional-witness-merge → `C ⊑ ⊥` path **plus** full ancestor+subclass unsat
  propagation. The only missing link is the NomKey-disjointness that makes the merge of two
  distinct-nominal witnesses unsatisfiable.

## The rule

In the EL saturator's axiom collection (`owl-dl-saturation`, `collect_el_rules` /
`collect_el_rules_with_provenance`), add a case for `Axiom::DifferentIndividuals(set)`:

- For every unordered pair `(a, b)` in `set` with `a ≠ b`, register
  `DisjointClasses(NomKey(a), NomKey(b))` into the saturator's `disjoint_pairs` (the same registry
  `DisjointClasses` axioms populate), where `NomKey(x)` is the synthetic class id from the existing
  per-individual nominal map (`introduce_nominal` / `nominal_to_ind` reverse lookup).
- Only register pairs for individuals that **have** a NomKey (i.e. appear in some
  `ObjectHasValue`/`∃R.{x}`); individuals never used as nominal fillers have no NomKey and are
  irrelevant (no `∃R.{x}` fact can clash). Allocation order is handled in the plan (process
  `DifferentIndividuals` after nominals are introduced, or look up lazily).

Once registered, the **existing** machinery does the rest unchanged: the functional/`≤1`
witness-merge builds the synthetic `S = NomKey(a) ⊓ NomKey(b)`; `S` gains both subsumers; the
existing disjointness check derives `S ⊑ ⊥`; `process_unsat` propagates to every class with `S` in
its facts and up/down the told hierarchy. The B2a forced-disjunct rule then resolves more
value-disjunctions deterministically (a disjunct whose synthetic `C ⊓ Dᵢ` is now unsat is dropped),
which enriches the derived ∃-facts the ∃-seed feeds into the wedge.

## Soundness

`DifferentIndividuals(a,b)` asserts `a ≠ b`, so `{a} ⊓ {b} = ⊥`. rustdl's `NomKey(x)` is a 1:1
opaque representative of identity-with-`x` (CLAUDE.md: "1:1 individual identity"), so
`NomKey(a) ⊓ NomKey(b)` is unsatisfiable exactly when `a ≠ b`. Registering the disjoint pair only
under an asserted `DifferentIndividuals` (never UNA-wide — OWL has no UNA) **adds only entailed
disjointness**. Sound by construction. The increment is *additive* (more derived unsats /
forced-disjuncts), in the same family as B1–B2c and the ∃-seed.

**But** — every nominal-fragment pruning lever this project has built has been unsound on first
cut three times (det-pruning FP=232, marker-saturator 23, precise-merge 232). A
sound-by-construction argument is **not** sufficient evidence here; only the wine oracle is.

## Gate (pre-committed, non-negotiable)

1. **Wine FIRST.** `konclude_closure_diff` on wine with the increment ON must be **FP=0 / MISSED=0,
   byte-identical (653=653, unsat:0)**. Run before trusting any wall number. A single spurious
   subsumption or spurious-unsat class ⇒ **NO-GO, revert** (the increment is wrong as built).
2. **Full corpus FP=0.** Same `konclude_closure_diff` byte-identical across the oracled fixtures
   (sio 8904, galen 27997, notgalen 32739, ore-10908 6001, ore-15672 142, pizza, alehif, ro,
   shoiq-knowledge) — DifferentIndividuals appears elsewhere; the rule must not regress them.
3. **Then wall.** Measure wine classify wall + label-cache misses + the 8 genuinely-hard classes
   (Burgundy, Chardonnay, Gamay, Meursault, PinotBlanc, Port, Tours, WhiteBurgundy) vs increment-OFF.
   Report whether any of the 8 now label/collapse.

GO only if FP=0 corpus-wide AND the wall measurably improves (≥1 of the 8 collapses, or a material
miss reduction). If FP=0 but no wall improvement ⇒ the rule is sound-but-inert on the wall (evidence
the determinism gap is elsewhere — M1 absorption, or the dense wall (B)); document and stop, do not
ship an inert default-on. If FP>0 ⇒ NO-GO.

## Scope / non-goals

- **M2 only.** M1 (nominal-value disjunction → deterministic absorption) is a separate, more
  entangled candidate (needs trigger/priority machinery per the study) — deferred to a follow-up
  iff M2 under-delivers.
- **DifferentIndividuals only** — no UNA-wide distinctness (unsound under OWL), no `SameIndividual`
  interaction beyond what exists.
- EL-saturator class-level only; `abox_saturation` already handles the ground-individual version
  (do not duplicate).
- Gated behind an env flag (e.g. `RUSTDL_NOMKEY_DIFF`, **default OFF**) until the gate passes; flag-
  OFF path byte-identical. `main` stays pristine; work on a branch off `main`. Flip default-ON only
  on a clean GO, as the controller's call.

## Global constraints

- Toolchain: `RUSTUP_HOME=/home/dumontier/.rustup CARGO_HOME=/home/dumontier/.cargo` + stable bin on
  PATH.
- `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets --all-features -- -D warnings`
  (pedantic, warnings = errors); `cargo test --workspace` green.
- Commit only when asked; trailers `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>` +
  `Claude-Session: https://claude.ai/code/session_01HSzon7V2wkhrudxBNAJduh`.
- Soundness of the measured config is verified by the wine/corpus closure-diff, not by unit tests
  alone (the three prior FP escapes all passed their unit canaries).
