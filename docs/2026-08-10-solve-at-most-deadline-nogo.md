# `solve_at_most` has no deadline check — and needs none (NO-GO)

2026-08-10. Investigated as the last item in the deadline-overshoot arc, and closed
**before** building anything, on the discipline the `RUSTDL_FIXPOINT_DEADLINE`
history established: a bound with no caller that needs it measures as a NO-GO.

## The claim, and how it was overstated

Mapping the wedge's deadline consultation sites turned up three
(`solve` entry, the strided `enumerate_matches` probe, and `decide_with_deadline`
setting it) and I recorded that `solve_at_most` and `partition_rec` have **zero**
deadline references. That is literally true — and it overstates the exposure.

**`partition_rec` recurses through `self.solve(depth - 1)`, and `solve` checks the
deadline at entry.** So the at-most path is not unguarded the way `horn_fixpoint`
was, where a `while worklist.pop()` drain loop could run without limit. The only
unguarded work here is what happens *between* `solve` calls: the merge loop and
partition enumeration on a partition that clashes (which skips the recursion).

## Addressability: the set is empty

Two counters answer it. The existing `RUSTDL_AT_MOST_EXHAUST_PROBE` reports
`at_most_exhaust_total`, but only when partitions are **exhausted** — that is "never
failed", not "never entered", and the unguarded region runs on entry regardless. So a
temporary entry counter was added, measured, and reverted.

| population | ontologies with entries > 0 | max entries |
|---|---|---|
| 40 sampled ORE ontologies | **0** | 0 |
| `wine`, `ore_ont_15672`, `ore_ont_1966`, `ore_ont_16372` | **0** | 0 |
| curated fixtures | 2 | **21** (`pizza`), 7 (`sio`) |

`at_most_exhaust_total` was **0** on all seven cardinality-heavy / hard / DNF-tail
ontologies probed, so the path is not merely rarely entered — it essentially never
exhausts either.

**`wine` returning 0 is the most informative row.** It is the corpus's most
nominal- and cardinality-heavy ontology and the documented hard frontier for `≤n`
reasoning; if any input were going to drive this path, it is that one.

The two ontologies that do reach it, `pizza` (21 entries) and `sio` (7), classify in
0.19 s and 0.55 s. Twenty-one entries on a sub-second ontology cannot be a
performance problem, and no amount of deadline precision would change their walls.

## Verdict

**NO-GO. Do not add a deadline check to `solve_at_most` without first exhibiting an
ontology that enters it at volume.** The mechanism is sound to build and would be
cheap, but it has no caller: adding it now would produce exactly the corpus-neutral
result that made `RUSTDL_FIXPOINT_DEADLINE` a NO-GO on 2026-08-08 — and unlike that
flag, there is no known future caller waiting for it.

The reopen criterion is concrete: an ontology with a high `solve_at_most` entry count
whose wall is dominated by it. The entry counter used here is ~6 lines
(`AtomicU64` + increment + a gated `eprintln!` in `decide_with_deadline`) and was
deliberately not kept, since a diagnostic with no live question is dead code; the
recipe above is enough to rebuild it in minutes.
