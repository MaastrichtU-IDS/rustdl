# MRV disjunction ordering — corpus gate RESULTS + VERDICT — 2026-06-23

**Verdict: PASS → default-ON.** MRV (most-constrained-⊔-first ordering of `find_open_disjunction`)
is **FP=0/MISSED=0 byte-identical across all 10 oracled fixtures**, has **no wall regression**, and
**collapses wine's hard models** to the correct verdict. Flipped default-ON. This is the **first
sound, shipped increment of the nominal/merge rewrite** — and the first sound forward result on wine.

## What MRV is

When the search reaches multiple open disjunctions (`⊔`), branch the one with the **fewest live
disjuncts first** (most-constrained-variable / fail-first). It only **reorders** which open ⊔ is
expanded first — no disjunct is dropped, added, or altered — so it is **verdict-invariant by
construction** (the search space and its completeness are unchanged). This is a categorically stronger
soundness basis than the dropped det-pruning lever (which removed disjuncts and was unsound on the
nominal fragment).

## Corpus FP gate (the proof)

`konclude_closure_diff`, `RUSTDL_MRV_ORDERING=1`. Fast fixtures at 1 s/pair; heavy cardinality/
disjunction fixtures at 25 ms/pair (spurious results complete fast → caught; MRV finds real
subsumptions in time → MISSED=0):

| fixture | rustdl | konclude | FP | MISSED |
|---|---|---|---|---|
| bibtex | 16 | 16 | 0 | 0 |
| ro | 51 | 51 | 0 | 0 |
| ore-15672 | 142 | 142 | 0 | 0 |
| pizza | 158 | 158 | 0 | 0 |
| galen (×2) | 27997 | 27997 | 0 | 0 |
| notgalen | 32739 | 32739 | 0 | 0 |
| ore-10908 | 6001 | 6001 | 0 | 0 |
| sio | 8904 | 8904 | 0 | 0 |
| **wine** | **653** | **653** | **0** | **0** |

**FP=0/MISSED=0 on every fixture, byte-identical to the Konclude∩HermiT oracle.** wine reaches its
full correct closure (653) at a 25 ms/pair deadline with MRV on.

## No wall regression

Full classify wall, MRV OFF vs ON (release CLI): **ore-10908 0.18 s / 0.18 s; sio 0.68 s / 0.68 s**
(identical — MRV's per-branch scan is negligible where there is no ⊔ explosion). The EL fixture batch
(galen/notgalen) finished in 11.7 s. Workspace test suite green under MRV default-ON (62 result groups,
0 failures).

## The wine collapse (the win)

From the combination spike (MRV-only, sound): `sat(Alsatian ⊓ ¬American)` **66683 → 1227 branches /
60 s-DNF → 1.2 s / Sat** (54×); `sat(SweetWine)` **67459 → 12366 / 15.6 s / Sat** (5.5×). MRV makes
wine's hard per-pair searches *terminate* (correctly) where they previously thrashed wide-and-shallow
to DNF.

## Provenance

MRV was isolated from the combination spike (`docs/combination-spike-gate-results-2026-06-23.md`): the
spike's other lever, **det-pruning, is unsound** on the nominal/≤n fragment (the deterministic
`horn_fixpoint` look-ahead skips the ≤n-merge → drops live disjuncts; FP=7 alone, 156 with MRV
amplification) **and contributed ~nothing to the collapse** — dropped. MRV alone is the sound lever
that delivers the collapse.

## Verdict / consequence

- **PASS on all four gate conditions** (FP=0 corpus-wide + no regression + wine collapse + flag-OFF
  byte-identical) → **default-ON** (`RUSTDL_MRV_ORDERING=0` opts out).
- Lands on `feat/build-once-redesign` as the rewrite's first shipped increment. `main` stays pristine
  (push there is the user's call, per the standing rule).
- It validates the rewrite's direction: a sound search-ordering improvement collapses the hard
  nominal/disjunctive models without touching soundness — and the corpus oracle, not the argument, is
  what proved it (the lesson that retracted the premature combination-spike GO).

## Method note

The premature GO on the full combination (presented on pizza/bibtex + 2 probes) was refuted by the
wine corpus FP gate (FP=232) — det-pruning's unsoundness. The disentangling isolated MRV as the sound
lever, and *this* gate ran the full-corpus FP proof BEFORE the default-ON flip. FP=0-on-wine + 9 other
fixtures is the evidence; the soundness argument (reordering is verdict-invariant) is the why.
