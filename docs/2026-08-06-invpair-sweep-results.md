# `RUSTDL_INVERSE_PAIR_FUNC` two-arm sweep — DO NOT FLIP, but the flag has real value

**Date:** 2026-08-06 · **Binary:** pinned `bin/rustdl-invpair-9f74f15`, sha `a37522841ce72493…`
**Predictions pre-registered before the run:** `docs/2026-08-06-invpair-sweep-predictions.md`
**Arms:** per-arm wrapper scripts recorded in each record's `reasoner` field; propagation
proven on a discriminating fixture *before* launch (arm-off `consistent`, arm-on
`inconsistent`); 1,920 ontologies each, 60 s cap, single-thread, arms sequential,
`--digest-strip-comments`. Both arms 1,920 cases, identical ontology sets.

**Setup validated independently:** arm-off reproduced `ok=1753, dnf=166, err_reject=1`,
*identical* to the 2026-08-04 domain-absorption ON arm measured with a different pinned
binary — so the harness, corpus, cap and concurrency match a run already trusted, and the
flag-off path is inert.

## Result

| | off | on |
|---|---:|---:|
| ok | 1753 | **1749** |
| dnf | 166 | **170** |

### Correctness: a genuine GAIN, no false positive

One ontology changed its answer, and it changed for the better. `ore_ont_13859`:

| | closure |
|---|---:|
| rustdl flag OFF | 6247 |
| **rustdl flag ON** | **6264** |
| Konclude | 6264 |
| HermiT | 6264 |

The flag adds **17 pairs and loses 0**, and **all 17 are present in both Konclude and
HermiT** — so they are entailed, and flag-ON reaches **exact parity with both oracles**
where flag-OFF was 17 short. Together with the 7-axiom `ore_ont_4141` core now being
decided, the correctness case for the mechanism is established, not merely argued.

### Performance: four severe regressions, and this is the blocker

**`ok → dnf` = 4, `dnf → ok` = 0.** All four reproduce serially on an idle host and are
*worse* than the sweep showed — none completes even at a 120 s cap:

| ontology | flag OFF | flag ON (120 s cap, serial) |
|---|---:|---|
| `ore_ont_16372` | 3.50 s | **dnf** |
| `ore_ont_7532` | 1.40 s | **dnf** |
| `ore_ont_9662` | 1.06 s | **dnf** |
| `ore_ont_9786` | 5.07 s | **dnf** |

Ontologies that classify in **1–5 seconds become non-terminating**. This is precisely the
failure mode the v0.4.8 `RUSTDL_CLASSIFY_INCONSISTENCY` flip shipped by measuring 12
ontologies instead of the corpus — and precisely why this sweep was worth its cost.

**`ore_ont_16372` is an interaction between two of my own changes**: domain absorption
(default ON since 2026-08-05) *rescued* it from a DNF, and this flag un-rescues it. Any
future work here must re-check it.

The rest of the corpus is unaffected: median wall delta **+0.000 s**, p90 **+0.020 s**;
the worst non-DNF regression is `ore_ont_13071` 33.22 → 40.56 s.

## Decision: keep default OFF

The pre-registered rule was *"flip only if `ok → dnf` = 0 and every answer change
adjudicates as sound"*. The second condition passed handsomely; the first failed 4 ways.
**Not flipping**, and unlike the domain-absorption case there is no argument for departing
from the rule — fast ontologies becoming non-terminating is the exact harm the rule exists
to catch.

**A second, independent blocker stands regardless of the sweep:** with the flag ON,
`rustdl consistent` reports `inconsistent` while `classify --json` reports
`consistent: true` on the 7-axiom core (confirmed with an unbounded
`RUSTDL_CLASSIFY_INCONSISTENCY_MS=0`, so not a budget artifact). Shipping two consistency
surfaces that contradict each other is worse than shipping a known MISS.

**But this is not dead scaffolding.** The flag is a sound, oracle-validated opt-in that
closes a real wrong-verdict defect and recovers 17 entailed subsumptions. It earns its
place as `=1`.

## Follow-ups, in dependency order

1. **Root-cause the 4 regressions.** Bounded and named. The likely channel is the
   materialised inverse edges — added `ABox` edges feeding the tableau — but that is a
   hypothesis, not a measurement. A per-ontology `--pair-timeout-ms 1` probe would say
   whether the new cost is per-pair search or elsewhere.
2. **Teach classify's pre-check the tableau route**, which removes the divergence blocker
   and is the same open item that would let the full `ore_ont_4141`/`8445` be decided.
3. Only then re-sweep for a flip.

**Not run, so not claimed:** a MISSED-net arm. The one adjudicated answer change was a
gain, but that is a single ontology, not a corpus-scale completeness measurement.
