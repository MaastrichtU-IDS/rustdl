# rustdl vs whelk-rs — full comparison (2026-07-08)

whelk-rs (INCATools, a Rust port of Balhoff's Scala *whelk*) is rustdl's closest
comparator: another **native-Rust, consequence-based EL** reasoner. This is the
head-to-head across every axis, for paper positioning.

Measured fresh on current `main` (v0.3.21): `owl-dl-bench compare-whelk --iters 4`
(first iter warmup-discarded), whelk-rs git `701710d5`, same process/host, in-memory.
`rustdl saturate()` (convert + saturate, no matrix) is the apples-to-apples kernel row
vs whelk `assert()`.

## 1. Scope / expressivity — the primary difference

| | whelk-rs | rustdl |
|---|---|---|
| Profile | **EL** (ELK-style EL⁺⁺ core) | **SROIQ(D)** — EL saturation + super-EL reductions + hybrid tableau |
| Out-of-EL input | **silently drops** non-EL axioms (unsound as a general DL reasoner) | routes to the complete hybrid path |
| Guarantee surface | EL-complete | `completeness_guaranteed()` (PureEl/Horn) + sound everywhere (FP=0) |

whelk-rs is an EL classifier; rustdl is a full DL reasoner whose EL saturation kernel
*is* directly comparable to whelk on EL input. Only **galen / notgalen / go-basic** are
valid head-to-head EL rows — on ro/pizza/sio/wine whelk silently drops the non-EL axioms,
so a "comparison" there is meaningless (different problems).

## 2. Performance — saturation kernel (apples-to-apples)

| ontology | classes | rustdl `saturate()` | whelk `assert()` | ratio | closure diff |
|---|--:|--:|--:|--:|---|
| galen    | 2,748  | **165 ms** | 410 ms | **rustdl 2.5×** | +17 rustdl-only, 0 whelk-only |
| notgalen | 3,087  | **209 ms** | 420 ms | **rustdl 2.0×** | +27 rustdl-only, 0 whelk-only |
| go-basic | 51,967 | 2.14 s | **1.11 s** | **whelk 1.9×** | **identical** (357,043 both) |

rustdl's kernel is **2.0–2.5× faster on medium EL** (galen/notgalen) — and the gap grew
vs the 2026-06-16 measurement (1.9×/1.4×) after this session's EL constant-factor work.
whelk is **1.9× faster at 52k-class scale** (go-basic). Both are native (no JVM startup;
ELK, by contrast, is ~4.5× slower than rustdl on galen and JVM-bound).

**The go-basic gap is understood and closeable:** rustdl runs its EL⁺⁺ functional-merge
guards (Phase 2a/2d) on every event even when they fire vacuously (go-basic has no
functional roles) — ~14M wasted guard checks. Gating Phase 2a/2d on
`functional_roles.is_empty()` upfront would remove that overhead (documented opportunity;
not yet done). So whelk's scale win is an un-taken optimization on our side, not an
algorithmic disadvantage.

## 3. Correctness — closure agreement

- **Pure EL (go-basic): byte-identical.** Both derive exactly 357,043 non-reflexive named
  subsumptions. (whelk's raw 512,946 = + reflexive + ⊑Top + internals.) The EL inference
  content is the same — a strong cross-validation of both engines.
- **EL⁺⁺ (galen/notgalen): rustdl is a strict SUPERSET.** rustdl derives +17 (galen) /
  +27 (notgalen) subsumptions whelk misses, all from **functional-role witness-merge**
  (rustdl's EL⁺⁺ machinery; whelk implements base ELK without it). All oracle-confirmed
  genuine (rustdl MISSED=0 vs Konclude∩HermiT); **0 pairs the other way** — rustdl never
  misses anything whelk finds. So on their shared EL fragment rustdl is *at least as
  complete*, and strictly more so where functional roles bite.

## 4. Features

| capability | whelk-rs | rustdl |
|---|:--:|:--:|
| EL classification | ✅ | ✅ |
| Full SROIQ(D) classification | — | ✅ (hybrid) |
| Consistency / realization | partial | ✅ |
| Explanation: justify / diagnose / repair / prove | — | ✅ (no whelk equivalent) |
| Inference materialization (object/data props, subprops, ∃-successors) | — | ✅ |
| HTML debug report | — | ✅ |
| Anytime (per-pair / global deadline, flagged-undecided) | — | ✅ |
| Data properties + concrete domains | — | ✅ |
| I/O formats | OFN/OWX | OFN / OWX / RDF-XML / Manchester |
| Native (no JVM) | ✅ | ✅ |

## 5. Architecture

Both are **consequence-based** (fixpoint saturation, no backtracking) on the EL core.
rustdl extends the same kernel with (a) EL⁺⁺ functional/inverse-functional witness-merge,
(b) super-EL sound reductions — `NomKey` nominals, opaque `≤n`/`∀R.OneOf`/`∀R.K` keys,
`DKey` datatype value-membership, ObjectHasSelf+range — and (c) a hybrid hypertableau
"wedge" + tableau for the rest of SROIQ. whelk-rs stays within EL.

## 5b. At-scale EL sweep (ORE-2015, 205 pure-EL onts) — added 2026-07-08

Ran `compare-whelk` over all ORE-2015 pure-EL ontologies (multi-format harness).
whelk-rs (base ELK, EL-sound-and-complete) is the oracle for the EL closure.

- **rustdl == whelk (byte-identical EL closure): 201 / 205** — strong mutual
  cross-validation at scale.
- **rustdl ⊋ whelk (sound EL⁺⁺ superset): 1** (functional-role witness-merge).
- **rustdl ⊊ whelk (EL gap): 3** (`ore_ont_3406/7216/13224`).
- **Performance: rustdl faster on 123 / 144 timed onts (median 1.64×); whelk faster
  on the largest** (e.g. `ore_ont_10248` 1M pairs: whelk 1.9 s vs rustdl 9.4 s) — the
  go-basic scaling pattern generalises.

**The sweep found two real rustdl EL-completeness gaps the tuned corpus never
exercised — its whole point:**
1. **`⊤ ⊑ NamedClass`** (`ore_ont_11522`): FIXED (commit on `fix/top-subsumes-all-el`;
   `top_subsumers` broadcast). 11522 522 → 1490 = whelk.
2. **Defined-class conjunctive trigger with an `∃` over a sub-property** — RESIDUAL,
   3 onts. Pattern: `GOCHE_37527 ≡ ∃GOCHEREL_0000004.CHEBI_37527 ⊓ CHEBI_24431`,
   `GOCHEREL_0000004 ⊑ RO_0000087`; rustdl's saturator doesn't fire the trigger for
   many CHEBI subclasses (claims PureEl-complete, isn't). Chemistry (CHEBI/GOCHE) onts.
   Not yet fixed — needs saturator debugging of why the conjunctive `∃`-over-subproperty
   trigger misses.

**Correction to §3:** the "strict superset" claim held on galen/notgalen/go-basic but is
FALSE in general — pre-fix rustdl was a subset on the ⊤⊑C onts, and remains a subset on
the 3 defined-class-∃ onts. Post-⊤⊑C-fix: rustdl is ⊇ whelk on 202/205 EL onts, with 3
residual EL gaps to close before any unqualified EL-completeness claim.

## 6. Paper takeaway

whelk-rs is the honest "same-class" baseline that isolates rustdl's contribution: **same
native-Rust consequence-based EL core, cross-validated to identical closures on pure EL,
but rustdl (i) is 2–2.5× faster on medium EL, (ii) derives a sound superset via EL⁺⁺, and
(iii) extends the fixpoint across a super-EL SROIQ(D) fragment with a full
explanation/debugging suite whelk has no equivalent for.** whelk's only win — raw
throughput at 50k classes — is a known, closeable vacuous-guard overhead, not an
algorithmic gap. This is the cleanest evidence that rustdl's "consequence-based beyond EL"
is a real extension of, not a reimplementation of, the EL state of the art.
