# The three inconsistent tail members: all three ARE inconsistent (two peers each); one is fixed, two are genuine open misses

> **REVISED 2026-08-05, later the same day, after adjudicating properly.** An earlier version of this
> document called `ore_ont_4141` and `ore_ont_8445` **contested** and excluded them, and **withdrew**
> the "745/107/338 unsat classes" figures as unverified. Both calls were wrong and are reversed below:
> HermiT confirms inconsistency on both once its literal-validation crash is worked around, and the
> three class counts reproduce exactly. What follows is the corrected record.

**Date:** 2026-08-05 · **Binary:** rustdl v0.4.14 at `62013d2` (domain absorption default ON)

Follow-up on the cluster flagged in `docs/2026-08-04-tail151-peer-triage.md`: three members of the
151-ontology DNF tail that Konclude found **inconsistent** in 0.14–2.55 s while rustdl DNF'd at
120 s. Investigated one at a time against an adjudicated oracle. **The cluster is real: all three are
inconsistent, each confirmed by two independent peers.** One (`ore_ont_16372`) is already fixed; the
other two are genuine open rustdl misses.

> **CORRECTED 2026-08-05 (later) — "KM is wrong on all three" WAS MY MISREADING OF KM'S OUTPUT.**
> `km classify --lines` prints `CONSISTENT 0` / `CONSISTENT 1`, and the trailing value is a
> **boolean**, not a subsumption count — the JSON form makes it explicit
> (`{"consistent": false, …}`). So `CONSISTENT 0` means **inconsistent**. Re-run across three KM
> releases: **KM reports `ore_ont_4141` and `ore_ont_8445` as INCONSISTENT on every version**,
> including the pre-fix `c6ced84` — it agreed with Konclude and HermiT throughout. Only the
> *minimal fixture* was genuinely mis-reported, and KM `v0.2.5` now fixes that
> (`408dee4`, closing the issue filed as bio-ontology-research-group/kobayashi-marust#3). A public
> correction has been posted there.
>
> The "KM produced ordinary 444/1,927-pair hierarchies" figures came from the 2026-08-04 triage
> baseline and are **inconsistent with KM's own output** (`"subsumptions": []` alongside
> `consistent: false`), so the harness's KM reading needs its own check before those numbers are
> re-used. Treat KM-vs-peer disagreement counts from that baseline as unverified.
>
> **Followed up 2026-08-05 and it went further than expected**: the standing
> "KM 10-ontologies-FP / ~1795 spurious pairs" record is now **retracted in its FP half** —
> KM v0.2.5 is FP=0 on every cited ontology testable, and **73% of the figure was a
> `⊤`-equivalence convention artifact** (Konclude collapses `X ⊑ TopEquivClass` into
> `EquivalentClasses(Thing, C)`; the old analysis normalised only the `⊥` side). See
> `docs/2026-08-05-km-fp-claim-audit.md`.

## `ore_ont_16372` — a genuine WRONG VERDICT, fixed by the domain-absorption flip

**Oracle, independently adjudicated:** Konclude says so in its own log
(`Ontology 'http://konclude.com/test/kb' … is inconsistent`, after
`'Individual-Precomputing' processing step failed`), and **HermiT agrees** by throwing
`org.semanticweb.owlapi.reasoner.InconsistentOntologyException`. Two independent peers, so this is
not the ambiguous-Konclude-silence case. KM does not decide this one (it times out on `v0.2.3` and
exits `worker engine exited -1` on `v0.2.5`), so it neither confirms nor contradicts.

**The clash, read off the source.** Lines 1050–1051 define the *same class twice with different
enumerations*:

```
IAO_0000078 ≡ ObjectOneOf(124 125 122 123 120 121 002)              # 7 members
IAO_0000078 ≡ ObjectOneOf(124 125 423 122 123 120 428 121 002)      # 9 members
DifferentIndividuals(120 121 122 123 124 125 423 428)               # line 2404
```

`423` and `428` are in the class by the second axiom, so each must equal one of the first seven. The
`DifferentIndividuals` axiom rules out `120–125`, leaving only `IAO_0000002` for both — so
`423 = 002 = 428`, contradicting that same axiom. A **nominal-enumeration pigeonhole**.

**Status: FIXED.** On a correctly-built v0.4.14 with domain absorption ON (the default as of
`62013d2`):

| | pre-flip | post-flip |
|---|---|---|
| `rustdl consistent` | **`consistent`** (wrong) | **`inconsistent`** ✓ |
| `rustdl classify` | DNF at 60 s | **2.92 s** |

So the domain-absorption flip did more than the 3 sweep recoveries credited to it: on this ontology
it **converts a wrong consistency verdict into the correct one**. That was not known when the flip
shipped and is recorded here and in `absorb.rs`.

**Soundness framing, stated precisely.** Reporting `consistent` for an inconsistent KB was a wrong
*verdict*, but it is not an FP=0 violation: an inconsistent KB entails everything, so failing to
derive ⊥ loses entailments — it is **MISS-shaped** downstream. The user-facing defect was that
`consistent` was printed with **no incompleteness signal**, so a caller could not distinguish a proof
of consistency from a failure to find the clash. That general property is unchanged by this fix and
remains a known limitation of the `consistent` surface.

## `ore_ont_4141` and `ore_ont_8445` — ADJUDICATED INCONSISTENT, and genuine open rustdl misses

| reasoner | verdict |
|---|---|
| Konclude | **inconsistent** (1.36 s / 2.55 s) |
| HermiT | **inconsistent** — once its literal crash is worked around (below) |
| KM | **inconsistent** — agrees (see the correction note above; my earlier "wrong" was a misread) |
| rustdl | `consistent` and `classify` both **TIMEOUT at 200 s** — open miss |

**How the deadlock was broken.** HermiT refused to run at all, throwing
`MalformedLiteralException` on an `xsd:anyURI` literal — `4141` on
`"http://en.wikipedia.org/wiki/Nièvre"` (non-ASCII `è`), `8445` on
`":  http://ncim.nci.nih.gov/…"` (leading colon plus two spaces). Its exception message names the
literal, which is what made this tractable. Percent-encoding just the offending `anyURI` values — 361
in `4141`, 15 in `8445`, datatypes and everything else untouched — let HermiT run, and **it reports
both inconsistent**.

**Control against the obvious objection** (that the repair *introduced* the inconsistency): Konclude
reports `is inconsistent` **18 times on both the original and the repaired file**, for each ontology.
The repair is therefore verdict-neutral for the peer that could read both, so HermiT's verdict on the
repaired file carries to the original.

**The ill-typed-literal hypothesis was tested and REFUTED as the cause.** It is true that an ill-typed
literal makes an OWL 2 DL KB inconsistent, so this looked like a clean explanation. But a positive
control — `DataPropertyAssertion(:p :a "abc"^^xsd:integer)`, unambiguously ill-typed — is reported
**`consistent` by Konclude** and merely crashes HermiT. Neither peer implements
"ill-typed literal ⟹ inconsistent", so these literals explain only HermiT's *lack* of a verdict, never
Konclude's inconsistency claim. rustdl **drops** such axioms outright ("unsupported data range"), so
they cannot affect its verdict either. **The actual source of the inconsistency in these two is still
unidentified** — worth a `diagnose`/ddmin pass, and the 108- and 339-class collapse below is the
starting point.

**Class counts, reproduced and un-withdrawn.** Reading Konclude's hierarchy directly: `owl:Thing` is
equivalent to `owl:Nothing` in all three, with **745 of 746**, **107 of 108** and **338 of 339**
declared classes collapsed (a consistent control, `ore_ont_6485`, shows **0 of 121**). The earlier
withdrawal of "745/107/338" is reversed — the figures are exact.

## Instrument gap found AND FIXED: `triage.py` conflated "peer proved inconsistency" with "peer crashed"

HermiT's `NO_OUTPUT` verdict covers **both**, and they are opposites in value: an inconsistency
exception is exactly the verdict needed to adjudicate this class, while a parse/datatype crash is no
verdict at all. Of the 5 `NO_OUTPUT` rows in `baselines/2026-08-04-triage-hermit-c120.jsonl`
(`ore_ont_10949`, `16372`, `20`, `4141`, `8445`): **`16372` and `ore_ont_20` are inconsistency
verdicts**, `4141`/`8445` are literal-validation crashes (which nonetheless *are* inconsistent, by the
work-around above), and `10949` is a genuine other-exception crash. So **4 of the 5 involve a real
inconsistency** that the triage recorded as "no output". `ore_ont_20` is a *newly surfaced* candidate —
HermiT calls it inconsistent and Konclude produces no output, so it is single-peer and **not**
adjudicated.

**Fixed:** `triage.py` now emits a distinct `INCONSISTENT` verdict, via two routes because the peers
signal differently — a hierarchy asserting `owl:Thing ≡ owl:Nothing` (Konclude, which still writes a
full 128 KB hierarchy and so read as `CLASSIFIED`), or an `InconsistentOntologyException` in a captured
log (HermiT, which writes nothing and so read as `NO_OUTPUT`, i.e. indistinguishable from a crash —
its opposite in value). A new `--log-dir` supplies the logs.

**The predicate is `owl:Thing`, not "all classes", and that is load-bearing.** `{A ⊑ ⊥, B ⊑ ⊥}` empties
every named class yet has a model, so a ratio heuristic would be unsound. Verified by a negative-control
fixture of exactly that shape (reads `CLASSIFIED`, correctly) and by **two sabotages, both caught**:
dropping the `Thing` requirement makes the negative control false-positive to `INCONSISTENT`, and
dropping the HermiT exception check turns its verdict back into `NO_OUTPUT`.

## Corrections to the record

- `docs/benchmarks/2026-08-01-dnf257-characterization.md`'s 2026-08-04 banner said *"three tail
  members are simply INCONSISTENT … (745/107/338 unsat classes)"*. **That statement is CORRECT and is
  restored.** My first amendment to it — downgrading two to "contested" and withdrawing the counts —
  was itself wrong and has been reversed. The counts reproduce exactly (745/746, 107/108, 338/339
  classes collapsed, against 0/121 on a consistent control); the first re-run simply wrote Konclude's
  hierarchy to `/dev/null`, so *my measurement* lacked the file, not the original claim.
- `ore_ont_16372` should not be described as an open tail member; it classifies in 2.92 s.

## Second trap, caught only by a control: `XML parsing error` in Konclude's log is BENIGN

Mid-investigation I found `{error} XML parsing error at 1:1: 'Start tag expected.'` in Konclude's log
for **all three** ontologies and concluded it had never parsed them — making its inconsistency claims
parse artifacts. **Wrong.** Konclude probes formats, so it emits that error for *every*
functional-syntax `.owl` file and then reads it successfully: a definitely-classified control
(`ore_ont_6485`, 40 KB hierarchy) logs it identically. The diagnostic pair is
`processing step failed` + `is inconsistent` — **0/0** on the control versus **18/18** on each
inconsistent case. Recorded in `triage.py`'s docstring so the next reader does not repeat it.

## Method failure worth recording: ~10 measurements on a stale binary

The first pass of this investigation concluded that **the wedge returns `Sat` on an inconsistent KB**,
supported by route isolation (`RUSTDL_WEDGE_CONSISTENCY=0` → timeout, while `RUSTDL_MAX_NODES=0` and
`RUSTDL_ABOX_SATURATION=0` still returned `consistent`) and by a narrowing showing the clash was
detectable in a 3-axiom extract but masked in the full file. **All of it was measured on a
pre-flip `target/release/rustdl`** — built 2026-08-04 20:00, against an `absorb.rs` edited
2026-08-05 02:25 — so it was characterising the *old* default, and the conclusion is withdrawn.

Two things make this cheap to avoid next time:

1. **`cargo test` does not rebuild the CLI binary.** The flip's own gates are unaffected and remain
   valid, because `run-soundness-diff.sh` runs `cargo test … --release`, which compiles from source;
   only the standalone `target/release/rustdl` was stale. Worth knowing which of the two a given
   measurement depends on.
2. **A disagreement with a prior *pinned* measurement is a staleness signal first.** The tell was
   `classify` taking 189 s where a pinned-binary harness run had recorded 8.36 s. That contradiction
   should have triggered an mtime check immediately; instead it was read as a `--json`-versus-plain
   difference and the investigation continued for several more measurements.
