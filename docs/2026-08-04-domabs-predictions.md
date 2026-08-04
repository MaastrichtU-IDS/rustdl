# Pre-registered predictions — RUSTDL_DOMAIN_ABSORPTION default decision

Recorded 2026-08-04 **before** the two-arm sweep was run. Binary pinned
`owl-reasoner-harness/bin/rustdl-domabs-2df70fc`, sha256 `98f801474c1a8d0d…`,
verified against a discriminating input (`ore_ont_16372` `residual_gcis` 49→5 flag OFF→ON)
and verified to propagate through the harness (`ore_ont_16372` dnf@60s → ok 8.34 s).

Flag is opt-in, `absorb.rs:278` (`is_some_and(|v| v == "1")`), default OFF. Confirmed from source.

| quantity | prediction |
|---|---|
| `dnf → ok` (recoveries) | **≥3** — `ore_ont_16372`, `6132`, `9899`. (`3281` has left the tail on v0.4.14, so the prior record's "4" is stale; R4 predicted this and it is confirmed.) |
| `ok → dnf` (regressions) | **unknown — this is the measurement.** The flag alters absorption on 1,030 of 1,913 ontologies, and absorption is on every ontology's path. |
| genuine answer changes | **0.** Domain absorption is a logical identity with `ObjectPropertyDomain`, so a real digest difference (comments stripped) is a **bug and a hard stop**, not a trade. |
| ΔMISSED vs 5,198 | **0**, for the same reason. |

**Decision rule, fixed now:** flip to default ON iff `ok → dnf` = 0 **and** genuine answer changes = 0
**and** the wall distribution over both-arm completers shows no material regression. Any `ok → dnf`
keeps it OFF pending diagnosis — that is the failure the v0.4.8 `CLASSIFY_INCONSISTENCY` flip shipped
by measuring on 12 ontologies instead of the corpus.

Sweep: 60 s cap, single-thread, 4-way concurrency per arm, **arms run sequentially** to keep
cap-boundary noise symmetric. `--digest-strip-comments` set, because rustdl's `#` banners carry
timings and a raw digest reports ~65% of completers as different from noise alone.
