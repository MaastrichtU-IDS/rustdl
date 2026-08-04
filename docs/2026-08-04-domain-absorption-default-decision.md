# `RUSTDL_DOMAIN_ABSORPTION` — the default decision, by two-arm corpus sweep

**Date:** 2026-08-04 · rustdl **v0.4.14** · gate `RUSTDL_DOMAIN_ABSORPTION`
**Question:** flip the default to ON, or keep it OFF?
**Answer:** _(see § Recommendation)_

This settles the one measurement `docs/2026-08-01-domain-absorption-results.md` left open. That
document established the feature is sound, verdict-identical everywhere it looked, FP=0 net
flag-ON, and 6/6 sabotage-caught, then declined to flip the default for exactly one reason:

> Flipping it default-ON needs a broader wall check first: it is *not* free — the census shows
> **1,030 of 1,913** pool ontologies carry at least one domain-absorbable residual.

Absorption is on the path every ontology traverses, so the risk is not soundness — it is a wall
or DNF regression on ontologies that complete today. That is what an `ok → dnf` sweep detects
and what no other gate in this repo can see: `run-soundness-diff.sh` is FP-shaped and the
MISSED net's frame is drawn from *completers*, so neither can observe an ontology that stops
finishing.

---

## 1. The flag's semantics and default, confirmed from source

Not taken from the docs. `crates/owl-dl-core/src/absorb.rs:274-279`:

```rust
/// `RUSTDL_DOMAIN_ABSORPTION` — opt in to domain absorption
/// ([`absorb_domain_residuals`]). **Default OFF**; `=1` enables.
#[must_use]
pub fn domain_absorption_enabled() -> bool {
    std::env::var_os("RUSTDL_DOMAIN_ABSORPTION").is_some_and(|v| v == "1")
}
```

This is the **opt-in** idiom (`is_some_and(|v| v == "1")`), not the house default-ON idiom
(`is_none_or(|v| v != "0")`). So today: unset ⇒ OFF, and *only* the exact string `"1"` enables.
Called once, at `absorb.rs:245`, guarding `absorb_domain_residuals(tbox, pool)`.

The recognizer is the whole soundness surface, `absorb.rs:307-319`:

```rust
fn as_domain_trigger(cid: ConceptId, pool: &ConceptPool) -> Option<Role> {
    match pool.get(cid) {
        // `≤0 R.⊤` = `¬∃R.⊤`. Qualified (`filler ≠ ⊤`) must NOT match.
        ConceptExpr::Max(0, role, filler) => {
            matches!(pool.get(*filler), ConceptExpr::Top).then_some(*role)
        }
        // `∀R.⊥` = `¬∃R.⊤`. `∀R.D` with `D ≠ ⊥` must NOT match.
        ConceptExpr::All(role, inner) => {
            matches!(pool.get(*inner), ConceptExpr::Bot).then_some(*role)
        }
        _ => None,
    }
}
```

Both admitted shapes are `¬∃R.⊤`, so the rewritten residual `⊤ ⊑ ¬∃R.⊤ ⊔ rest` is *literally*
`ObjectPropertyDomain(R, rest)` — hence the claim that the change is verdict-preserving, not
merely sound. `absorb_domain_residuals` (`:340-370`) emits
`RoleRule { role: role.flip(), guard: None, target_label: Or(rest) }` and drops the residual.

**A consequence worth stating, because it bounds what this sweep can even detect:** `clause.rs`
clausifies from `InternalOntology` **directly, not from the absorbed TBox**, so the hypertableau
wedge is *blind* to this flag. Only main-tableau-answered queries can change behaviour. That is
why the prior document's verdict canaries must force `RUSTDL_HYPERTABLEAU=0`, and it is also why
a wall effect at all is notable: `ore_ont_16372` moving DNF → 8.34 s is the main tableau's work
changing, not the wedge's.

---

## 2. Provenance — one binary, pinned, verified against a discriminating input

The flag is env-gated, so one binary serves both arms; the arms are env settings.

| | |
|---|---|
| binary | `/tmp/rustdl-domabs-v0414-2026-08-04` |
| sha256 | `98f801474c1a8d0d7f3d1776ea36fe1940eba7a001b1df1f26cb9ead36475c46` |
| version | `rustdl 0.4.14` (matches `Cargo.toml` `version = "0.4.14"`) |
| built | `RUSTUP_TOOLCHAIN=stable cargo build --release --workspace`, then copied and `chmod 555` |
| identity | sha256 equal to `target/release/rustdl` at build time |
| arm OFF | `/tmp/domabs-arm-off.sh` → `exec env RUSTDL_DOMAIN_ABSORPTION=0 <binary> "$@"` |
| arm ON | `/tmp/domabs-arm-on.sh` → `exec env RUSTDL_DOMAIN_ABSORPTION=1 <binary> "$@"` |

Per `[[pin-binaries-per-configuration]]`, the pin was verified against **discriminating inputs**
— ontologies where the arms must measurably differ — before any sweep was trusted:

| probe | flag OFF | flag ON | agrees with prior record? |
|---|---|---|---|
| `ore_ont_16372` `tbox-stats` residual_gcis | **49** | **5** | yes (49 → 5) |
| `ore_ont_3281` `tbox-stats` residual_gcis | **28** | **0** | yes (28 → 0) |
| `ore_ont_16372` `classify`, 60 s cap, 1 thread | **rc 124, DNF @60.01 s** | **8.34 s, 2236 rows** | yes |

So the pinned binary carries the feature, the env gate moves it, and the arms diverge in the
quantity actually being measured — not merely in a diagnostic counter.

---

## 3. Method

| | |
|---|---|
| corpus | `/data/dumontier/ore-run/pool_sample/files`, **1920** `.owl` (functional syntax) |
| command | `classify <ont>` (no per-pair budget) |
| cap | **60 s** wall, per ontology |
| threads | `--threads 1` (rustdl pinned single-thread) |
| concurrency | **4 concurrent chunks** of 480 ontologies (`sweep-arm.sh`), one output file per chunk |
| arms | run **sequentially** — OFF first, then ON — so the two never share the host |
| tooling | `owl-reasoner-harness run` via `scripts/sweep-arm.sh`; comparison by `compare` + `scripts/wall-delta.py` |

`sweep-arm.sh` writes **one JSONL per chunk** and concatenates at the end; concurrent appends to a
single `--out` previously produced 40 unparseable records and 73 silently missing ontologies.

### 3a. The answer-identity instrument had to be fixed first

`compare`'s answer-identity check reads `out_sha256`, which the harness computed over **raw
stdout including `#` banner lines** — and those banners carry wall-clock timings and a
millisecond-bucketed `# wedge-cost-histogram`. Measured on the pre-existing v0.4.14 early-abandon
arms, a raw OFF-vs-ON comparison reports **1133 of 1745 completers as DIFFERENT**. That signal is
noise-dominated and cannot support (or refute) a verdict-identity claim, and re-checking ~1100
ontologies by hand is not a plan.

So the harness gained `--digest-strip-comments` (hash only non-`#` lines), the `Header` gained
`digest_strip_comments` so a run records which regime it is in, and `compare` now **says** which
regime a reading is in and refuses to interpret a mode mismatch. `out_lines` still counts full
stdout. No reasoner code was touched.

**The instrument was proved to fire** before use — one binary, one flag setting, the same
ontology run twice:

| ontology | raw digests (2 runs) | stripped digests (2 runs) | banner lines differing |
|---|---|---|---|
| `ore_ont_10006` | `761aa30a2abd` ≠ `70e100852345` | `50455c97a513` = `50455c97a513` | 2 |
| `ore_ont_1539` | `15e5d02d2990` ≠ `6905ab9a1f11` | `c27af4649942` = `c27af4649942` | 2 |

This *is* the OFF-vs-OFF control the trap calls for, at the level that matters: it shows the raw
digest is nondeterministic with the flag held constant, and that stripping removes exactly that
nondeterminism. Both sweep arms ran with `--digest-strip-comments`, so every digest comparison
below is banner-stripped and strict.

### 3b. Pre-registered predictions

Written to `owl-reasoner-harness/runs/2026-08-04-domabs-PREDICTIONS.txt` **before either arm was
launched**, and reproduced here verbatim in substance:

- **P1** OFF-arm DNF count in 150–175. (Amended before launch: the tail-151 list was measured at
  a **120 s** cap, not 60 s, so the 60 s figure must be higher; the state of play records ~157.)
- **P2** `dnf → ok` = **3**, exactly `{ore_ont_16372, ore_ont_6132, ore_ont_9899}`. `ore_ont_3281`
  — the 4th prior recovery — has left the tail via v0.4.14 early-abandon and should complete in
  **both** arms.
- **P3** `ok → dnf` = **0**. *The blocking finding.*
- **P4** answer changes among both-completing ontologies = **0**, banner-stripped.
- **P5** wall delta median ≈ 0, p90 ≈ 0.
- **P6** ΔMISSED = 0.

### 3c. Verification of the prior record's recovery set

R4's correction checked independently against
`baselines/2026-08-04-tail-v0414-list.txt` (151 entries) and `2026-08-04-setA-138-ranked.txt`:

| ontology | in the v0.4.14 tail? | fastest peer | Set |
|---|---|---|---|
| `ore_ont_3281` | **no — has left the tail** | — | — |
| `ore_ont_16372` | yes | konclude **0.14 s** | A |
| `ore_ont_6132` | yes | konclude **0.97 s** | A |
| `ore_ont_9899` | yes | konclude **1.04 s** | A |

**R4 is confirmed: 3 expected recoveries, not 4, all Set A at 0.14 / 0.97 / 1.04 s peer walls.**

---

## 4. Results

_(filled in from the sweep)_

---

## 5. Recommendation

_(filled in from the sweep)_
