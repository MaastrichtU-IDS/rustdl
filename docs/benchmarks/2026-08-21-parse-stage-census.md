# The parse/front-end stage is NOT a source of DNF-tail members (2026-08-21)

**Method.** `rustdl locality-stats` (parse + convert, no reasoning) over all **1,920** ORE
ontologies, 25 s cap, P=8. Failures bucketed by their distinguishing error text. Data:
`data-2026-08-21-parse-census-1920.tsv`.

## Result

| outcome | count |
|---|---:|
| OK | **1,903** |
| TIMEOUT (25 s) | 16 |
| FAIL (parse error) | **1** |

**One** ontology in 1,920 fails to parse. This closes the front-end as a lever: the historical
"23% of ORE rejected on the anonymous-individuals converter gap" is gone, and nothing has replaced
it at any material scale.

### The single failure, and why fixing it is not worth it

`ore_ont_10860` dies in the OFN grammar on a SWRL `DLSafeRule` whose `BuiltInAtom` takes
`Variable(...)` data arguments:

```
= expected DArg
```

The grammar's `DArg` production does not admit a variable. This is a defect in the pinned
`horned-owl` fork, not in rustdl. It is *worth knowing* that SWRL axioms are **dropped gracefully
at conversion** (`DroppedAxioms`), so if the file merely PARSED, this ontology would classify — the
fix is one grammar rule for one recovery. Recorded, not queued: **1 ontology of 1,920.**

## The 16 timeouts are the more useful finding: a cheap CONVERSION-BOUND detector

`locality-stats` does parse + convert and no reasoning, so a `locality-stats` timeout localises the
wall to **before any reasoning starts**. That is strictly sharper than the phase-banner census,
whose `# wall breakdown ms:` line is only emitted *after* conversion — a conversion-bound ontology
shows up there as "all phases zero" or as no banner at all, indistinguishable from a fast pure-EL
run. **Use `locality-stats` to separate the two.**

Cross-checks that validate the detector:

* **4 of the 16** (`2504`, `4141`, `4572`, `8445`) are exactly the DKey members that the
  v0.4.21 string/group seeding fixes did **not** help — the ones that genuinely materialise 14–68 M
  concept rules, where there is no futile work to skip.
* **`10929` and `15635` are OK**, i.e. absent from the timeout list. Those are the two v0.4.21
  recovered. Their absence is the cheapest available confirmation that the shipped fix works.
* **4 of the 8** `no-banner` "TRUE_STALL" members are in this set, so a third of that bucket was
  never a reasoning stall at all.

**Consequence for the parked DKey disjointness oracle:** its addressable set continues to measure
at ~6 (`2504`, `4141`, `4572`, `8445`, `5368`, `1833`), which is the number its own spec parked it
at. This census does not move that decision; it corroborates it.
