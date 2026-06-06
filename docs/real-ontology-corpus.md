# Real-ontology corpus

The bench harness ships with synthetic EL chains (`bench synthetic-el`)
and 87 small differential fixtures (`crates/owl-dl-bench/fixtures/`).
Neither captures how rustdl behaves on *real* ontologies of the shape
the project targets — EL-heavy biomedical TBoxes plus a few SROIQ-heavy
upper ontologies that drive the gaps in
[`outperform-hermit-plan.md`](outperform-hermit-plan.md).

The "real corpus" is a small, fixed set of public ontologies fetched
on demand by [`scripts/fetch-real-ontologies.sh`](../scripts/fetch-real-ontologies.sh).
The script downloads each source, then converts it to OWL functional
syntax (`.ofn`) using ROBOT inside the `obolibrary/robot` Docker image,
because both `owl-dl-bench` and the `rustdl` CLI only parse `.ofn`
(see [crates/owl-dl-bench/src/main.rs](../crates/owl-dl-bench/src/main.rs)
and [crates/owl-dl-cli/src/main.rs](../crates/owl-dl-cli/src/main.rs)).

Files land in `ontologies/real/` which is gitignored (see
[`.gitignore`](../.gitignore)), so the corpus is reproducible from
upstream rather than vendored.

## Inventory

| Slug | Source URL | Format | Why we want it |
|---|---|---|---|
| `sio` | `https://semanticscience.org/ontology/sio.owl` | RDF/XML | Named in the baseline gap table; SROIQ-heavy with role chains (`hasParticipant ∘ inverse(hasFeature) ⊑ …`). Currently does not finish in 90 s on the tableau side — the canonical test for the saturation-inverse-chain work in [`outperform-hermit-plan.md`](outperform-hermit-plan.md). |
| `sulo` | `https://w3id.org/sulo/sulo.ttl` | Turtle | Lightweight Smart Upper Ontology; useful as a small SROIQ shape that's well outside the EL fragment. Pairs with SIO as a sanity check that the tableau path is exercised. |
| `family` | `https://www.cs.man.ac.uk/~stevensr/ontology/family.rdf.owl` | RDF/XML | Robert Stevens' family ontology. Length-3 role chains (`hasParent ∘ hasParent ∘ …`) and inverse-functional + symmetric roles — the canonical "coverage gap" workload from [`outperform-hermit-plan.md`](outperform-hermit-plan.md) (rustdl currently rejects / times out where HermiT reports inconsistent in 8 s). |
| `pizza` | `http://protege.stanford.edu/ontologies/pizza/pizza.owl` | RDF/XML | Canonical SROIQ tutorial ontology (disjointness, covering axioms, value restrictions). Small enough to classify quickly; verdicts are well-known from HermiT/ELK, so divergences are immediately suspicious. |
| `ro` | `http://purl.obolibrary.org/obo/ro.owl` | RDF/XML | The OBO Relations Ontology — pure object-property axiomatization. Exercises role hierarchy, property characteristics, and the `SubObjectPropertyOf` chain rules with no class-level complexity. |
| `go-basic` | `http://purl.obolibrary.org/obo/go/go-basic.obo` | OBO | Gene Ontology, "basic" cut. ~50k classes of canonical EL — `partOf` / `isA` heavy. Stress test for the saturation engine's scaling. |
| `ore-10908-sroiq` | ORE 2015 sample (`ore_ont_10908.owl` from `ontologies/external/ore2015_sample.zip`) | RDF/XML | Phase 0 soundness-net broadening: full SROIQ with complex role chains (R), qualified cardinality (Q), inverses, and nominals — the same fragment as SIO that produced all 38 recorded `trust_sat` false positives; 693 classes, HermiT-classified reference produced (1 738 axioms). |
| `ore-15516-alchoiq` | ORE 2015 sample (`ore_ont_15516.owl` from `ontologies/external/ore2015_sample.zip`) | RDF/XML | Phase 0 soundness-net broadening: ALCHOIQ(D) with qualified cardinality, inverses, role hierarchy, and nominals in 85 classes; **HermiT reports inconsistent** — .ofn fixture left on disk for future use (no current test references it). |
| `ore-15672-shoin` | ORE 2015 sample (`ore_ont_15672.owl` from `ontologies/external/ore2015_sample.zip`) | RDF/XML | Phase 0 soundness-net broadening: SHOIN with unqualified cardinality (N), inverses, role hierarchy, and nominals in 83 classes — covers the N-flavour number restriction clash semantics not present in the Q-carrying picks; HermiT-classified reference produced (324 axioms). |
| `wine` | W3C OWL-guide `wine` + `food` (`http://www.w3.org/TR/2003/PR-owl-guide-20031209/{wine,food}`), merged | RDF/XML | The canonical SHOIN(D) tutorial ontology — **nominal-heavy** (207 `ObjectHasValue`/`ObjectOneOf` value restrictions on regions/grapes/colors) + 39 `DisjointClasses` + 88 equivalences in 137 classes. The corpus's stressor for **nominal / value-restriction reasoning**, a class of entailment no other fixture exercised. wine imports food which circularly imports wine; `fetch_wine` strips both `owl:imports` and merges locally. **FP=0, MISSED=57 at add time → 34 after the nominal lever** (`docs/nominal-lever-scoping-2026-06-06.md`, 2026-06-06): the region/color cluster (`AlsatianWine ⊑ FrenchWine` via transitive region nominals) now closes via the saturator's NomKey fold + transitive-ABox propagation; the residual 34 are grape (`≤1` cardinality) + sugar (`∀`+nominal set), deferred. See `docs/corpus-wine-2026-06-06.md`. |

## Ground-truth (reference classification)

The closure-diff harness compares against a HermiT-inferred reference at
`ontologies/real/konclude-input/<slug>-classified.owx`, produced by
[`docker/robot/classify-oracle.sh`](../docker/robot/classify-oracle.sh):

```sh
docker/robot/classify-oracle.sh ontologies/real/wine.ofn \
    ontologies/real/konclude-input/wine-classified.owx
```

Both `ontologies/real/` and the references are gitignored; regenerate from
upstream with the fetch script + the oracle above. The closure-diff tests
(`crates/owl-dl-reasoner/tests/konclude_closure_diff.rs`, all `#[ignore]`d) SKIP
when a fixture is absent.

## Refresh

```sh
./scripts/fetch-real-ontologies.sh
```

Pin a different ROBOT image with `ROBOT_IMAGE=obolibrary/robot:v1.9.5`
(or any tag from <https://hub.docker.com/r/obolibrary/robot/tags>).
The default is `obolibrary/robot:v1.9.6`, matching
[`docker/robot/oracle.sh`](../docker/robot/oracle.sh) so the
classification harness and the differential oracle agree on tooling.

The script is idempotent: re-running it overwrites both the source and
the converted OFN, picking up any upstream updates. Downloads go
through whatever `HTTP(S)_PROXY` is set in the environment, which on
the lab's machines points at `proxy.unimaas.nl:3128`.

## Caveat: data-property stripping

> **Caveat — data property handling has changed (Phase D1, 2026-06-02):**
> Earlier versions of rustdl hard-rejected ontologies declaring
> data properties, hence the existence of stripped variants
> (family-stripped.ofn, ro-stripped.ofn, sio-stripped.ofn) that
> remove every data-related axiom. Phase D1 (commit `e34aeb6`)
> changed this to a silent drop: ontologies parse, data axioms
> are silently dropped, and the result is a sound
> under-approximation of the full classification. Phases D4 and
> D5 add a preprocessing pass that derives class axioms from
> specific data-axiom patterns (Functional+DataMin, DataMin>Max,
> DataPropertyDomain inference, SubDataPropertyOf transitivity,
> intersection-equivalence propagation, integer-range facet
> intersection). The test `data_axiom_declarations_silently_dropped`
> pins this behavior. Stripped variants remain useful for
> benchmarking the object-property fragment in isolation; the
> originals can now be classified.

## Using the corpus from a bench run

```sh
# Single ontology — uses the orchestrator's default settings.
./target/release/owl-dl-bench classify ontologies/real/sio.ofn

# Full corpus — same flags as the synthetic / 87-fixture runs.
./target/release/owl-dl-bench corpus ontologies/real --repeats 5

# Through the public CLI with the baseline timeout flag.
./target/release/rustdl classify --pair-timeout-ms 200 ontologies/real/sio.ofn
```

`owl-dl-bench corpus DIR` walks `DIR` for `.ofn` files; mixing real and
synthetic-converted ontologies in the same directory is intentional.

## What's *not* here

- **SULO from `w3id.org/sulo/`** is the file the baseline doc refers to
  as "SULO classify"; the original 466 ms baseline number predates
  this corpus pin, so refresh runs are not strictly apples-to-apples
  against that table until SULO's release date is captured here too.
- **ORE 2015 Live** — the strategy v2 plan reserves
  `xtask fetch-ore-2015` for this. That task is still
  `not implemented yet` in [xtask/src/main.rs](../xtask/src/main.rs);
  the real corpus here is the on-ramp until that lands.
