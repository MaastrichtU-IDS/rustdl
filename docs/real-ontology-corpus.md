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

`owl-dl-core` hard-rejects data-property and datatype axioms (and
class expressions that mention them) — see
[crates/owl-dl-core/src/convert.rs:297](../crates/owl-dl-core/src/convert.rs#L297)
and the test `unsupported_data_axioms_hard_error` that pins the
behavior. Phase 7 ([strategy v2](owl-dl-reasoner-rust-strategy-v2.md))
will lift this. Until then any real ontology that declares or uses
a data property fails conversion (SULO, family, RO, SIO all hit this
on the unmodified inputs in this corpus).

To run the bench against these inputs today, strip the data-property
signature with ROBOT and post-filter the leftover `Declaration(Datatype(...))`
lines:

```sh
cd ontologies/real
for src in sulo.ttl family.rdf.owl ro.owl sio.owl; do
    slug="${src%%.*}"
    docker run --rm -v "$PWD:/work" -w /work obolibrary/robot:v1.9.6 \
        robot remove --input "$src" --select data-properties --signature true --trim true \
                     convert --format ofn --output "${slug}-stripped.ofn"
    sed -i -E '/^[[:space:]]*Declaration\(Datatype\(/d' "${slug}-stripped.ofn"
done
```

Bench against `*-stripped.ofn`. The stripped files are *not*
classifying the original ontology — they're a Phase-7-shaped
under-approximation. Once data properties are reasoning-load-bearing
in rustdl, drop this step and use the raw `.ofn` directly.

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
