# The "three inconsistent tail members": one was a real wrong verdict and is now fixed; two are contested

**Date:** 2026-08-05 · **Binary:** rustdl v0.4.14 at `62013d2` (domain absorption default ON)

Follow-up on the cluster flagged in `docs/2026-08-04-tail151-peer-triage.md`: three members of the
151-ontology DNF tail that Konclude reportedly found **inconsistent** in 0.14–2.55 s while rustdl
DNF'd at 120 s. Investigated one at a time against an adjudicated oracle. The cluster does not
survive as stated.

## `ore_ont_16372` — a genuine WRONG VERDICT, fixed by the domain-absorption flip

**Oracle, independently adjudicated:** Konclude says so in its own log
(`Ontology 'http://konclude.com/test/kb' … is inconsistent`, after
`'Individual-Precomputing' processing step failed`), and **HermiT agrees** by throwing
`org.semanticweb.owlapi.reasoner.InconsistentOntologyException`. Two independent peers, so this is
not the ambiguous-Konclude-silence case. **KM disagrees** — it produces an ordinary hierarchy — which
is consistent with its documented concrete-domain unsoundness and is why KM is not used to adjudicate
here.

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

## `ore_ont_4141` and `ore_ont_8445` — CONTESTED, excluded rather than adjudicated

| reasoner | verdict |
|---|---|
| Konclude | inconsistent (1.36 s / 2.55 s) |
| HermiT | **no verdict** — `org.semanticweb.HermiT.datatypes.MalformedLiteralException` |
| KM | ordinary hierarchy (444 and 1,927 pairs) — i.e. *consistent* |
| rustdl (fresh binary) | `consistent` and `classify` both **TIMEOUT at 200 s** |

One peer claims inconsistency, one cannot process the file at all, and one disagrees. **A contested
oracle is not an oracle**, so per standing project practice these are *excluded* from any MISSED
claim rather than resolved by preferring a peer.

The `MalformedLiteralException` is a substantive hint rather than mere noise: an ill-typed literal —
one outside its datatype's value space — makes an OWL 2 DL KB inconsistent. That would explain all
three behaviours at once (Konclude derives ⊥ from the literal; HermiT throws on the same literal
instead of concluding; KM ignores datatypes, matching its recorded concrete-domain unsoundness). It is
a **hypothesis, not a finding** — a scan of their literals showed nothing obviously ill-typed
(`"true"^^xsd:boolean`, `"Virus"^^xsd:string`, and similar), so the offending literal, if any, was not
located. Settling it needs the specific literal HermiT chokes on, which its exception message does not
name.

## Instrument gap found: `triage.py` conflates "peer proved inconsistency" with "peer crashed"

HermiT's `NO_OUTPUT` verdict covers **both**, and they are opposites in value: an inconsistency
exception is exactly the verdict needed to adjudicate this class, while a parse/datatype crash is no
verdict at all. Of the 5 `NO_OUTPUT` rows in `baselines/2026-08-04-triage-hermit-c120.jsonl`
(`ore_ont_10949`, `16372`, `20`, `4141`, `8445`), `16372` is an **inconsistency verdict** and
`4141`/`8445` are **crashes**; `10949` and `20` were not checked. This understates HermiT's Set A and
silently discards the one signal that makes this cluster adjudicable. **`triage.py` should classify
`InconsistentOntologyException` as its own outcome**, distinct from `NO_OUTPUT`.

## Corrections to the record

- `docs/benchmarks/2026-08-01-dnf257-characterization.md`'s 2026-08-04 banner says *"three tail
  members are simply INCONSISTENT … (745/107/338 unsat classes)"*. Amended: **one** is adjudicated
  inconsistent (and now fixed), **two** are contested. The per-ontology unsat counts are **withdrawn
  as unverified** — a re-run of Konclude wrote its hierarchy to `/dev/null`, so those numbers could
  not be reproduced here and their provenance is unclear.
- `ore_ont_16372` should not be described as an open tail member; it classifies in 2.92 s.

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
