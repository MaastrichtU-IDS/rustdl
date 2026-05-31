# Phase 0 corpus candidates — inverse + cardinality + role-hierarchy

Selected from the ORE 2015 DL classification sample (`ontologies/external/ore2015_sample.zip`)
to broaden the soundness net per the Phase 0 plan
(`docs/superpowers/plans/2026-05-31-phase0-soundness-net.md`).

Selection criterion: expressivity contains **I** (inverse) + a cardinality marker
(**Q** or **N**) + **H** or **R** (role hierarchy / complex roles) — the interaction
historically responsible for the SIO false positives (see
`docs/hypertableau-dead-ends.md` §12). Where the strict criterion yielded too few
candidates, the relaxation is noted below.

> **Note:** all ORE 2015 files in `pool_sample/files/` are `approximated_dl_*`
> artefacts — they are DL approximations of the original submissions (many of which
> were OWL Full or had profile violations). The expressivity string and class count
> are those of the approximation, not the original ontology. They are still valid
> stress fixtures for the target interaction.

## Selected candidates

| Slug | ORE source | DL expressivity | Approx. classes | Approx. size | Why this stresses the interaction |
|---|---|---|---|---|---|
| ore-mf | `ore_ont_10908.owl` | SROIQ | 693 | 180 KB | Full SROIQ with complex roles (R), qualified cardinality (Q), inverses, and nominals — the same fragment as SIO, which was the sole source of all 38 recorded FPs under `trust_sat`; widest coverage of the exact interaction |
| ore-shoiq-small | `ore_ont_15516.owl` | ALCHOIQ(D) | 85 | 201 KB | Qualified cardinality (Q) + inverses (I) + role hierarchy (H) + nominals (O) in a tiny ontology; drops complex-role chains vs SROIQ so any FP here isolates cardinality+inverse+hierarchy independent of R |
| ore-shoin-design | `ore_ont_15672.owl` | SHOIN | 83 | 70 KB | Unqualified cardinality (N) + inverses (I) + role hierarchy (H) + nominals (O); covers the N-flavour clash semantics not exercised by the Q-carrying picks above, at the smallest file size in the strict match set |

## Relaxation, if any

No relaxation was needed. The strict filter (I + (Q or N) + (H or R)) matched **47
ontologies** in the ORE 2015 DL classification sample. Three small candidates with
diverse expressivity profiles were chosen from within that set. The three picks were
chosen to span the relevant variation axes: SROIQ covers complex role chains (R);
ALCHOIQ(D) covers qualified cardinality with role hierarchy but without complex chains;
SHOIN covers unqualified cardinality (N). Candidates with more than ~700 classes or
files larger than ~210 KB were not selected to keep each Konclude/HermiT diff
comfortably within minutes.

## How to extract these from the zip

```bash
unzip -o ontologies/external/ore2015_sample.zip \
  'pool_sample/files/ore_ont_10908.owl' \
  'pool_sample/files/ore_ont_15516.owl' \
  'pool_sample/files/ore_ont_15672.owl' \
  -d /tmp/ore
```
