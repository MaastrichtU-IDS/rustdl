# `rustdl report` — HTML debugging report (design)

**Date:** 2026-06-21
**Status:** approved (brainstorming) → ready for implementation plan
**Branch:** `feat/html-report`

**Sub-feature D** of the explanation/debugging suite — the capstone. The other three
shipped: `diagnose` (A), `repair` (B), `justify --laconic` (C), plus `justify`/`prove`.
This ties them into one navigable HTML report. Mockup:
`/tmp/.../scratchpad/report-mockup.html` (published artifact during brainstorming).

## Goal

`rustdl report <ontology>` produces one self-contained HTML page answering "what is
wrong with this ontology, why, and how to fix it": a summary, the diagnose
roots/derived lattice, each root's minimal justification, and the repair suggestions
— in a single file the user opens in a browser.

## Soundness framing

`report` is **presentation-only** over already-sound reasoner output. It runs no new
reasoning of its own — it orchestrates the shipped `diagnose` / `find_one_justification`
/ `find_repairs`, each already sound (and each repair already verified). There is no
new false-positive surface; classification is untouched (read-only). The bar here is
**faithful rendering**: the HTML must accurately reflect the data and must not corrupt
it (all dynamic text HTML-escaped) or pull in external content.

## Architecture & placement

- New module `crates/owl-dl-cli/src/report.rs` (the CLI's first submodule) — holds the
  `Report` data model, the orchestration (`build_report`), and the pure renderer
  (`render_html`). `main.rs` gets a thin `Report` command handler.
- **Rendering lives in the CLI, not the reasoner library** — it is presentation, and
  keeps the library HTML-free. It reuses the shipped library API only:
  `diagnose::diagnose`, `justify::{find_one_justification, find_repairs, Entailment,
  component_entities}`, and the CLI's existing `as_manchester_with_prefixes` /
  `build_label_map` / `local_name`.
- Two units, each independently testable:
  - `build_report(onto, max_roots) -> Result<Report>` — orchestration (calls the
    reasoner; no HTML).
  - `render_html(report, pm, labels) -> String` — pure; HTML from the `Report` value.

## Data model

```
struct Report {
    ontology_path: String,
    class_count: usize,
    consistent: bool,
    fragment: String,            // e.g. "SHOIN" / "Horn" — from the classification
    // when inconsistent:
    inconsistency: Option<Section>,   // justification + repairs for `Inconsistent`
    // when consistent with unsat classes:
    roots: Vec<RootEntry>,            // each: iri, justification axioms, repairs, derived[]
    derived: Vec<(String, Vec<String>)>,  // derived iri ⇐ root iris
    // counts for the summary grid:
    n_unsat: usize, n_root: usize, n_derived: usize,
    repairs_complete: bool,           // from find_repairs completeness flag
    truncated_roots: usize,           // roots beyond max_roots, listed without detail
}
```
A `RootEntry`/`Section` carries the `Component` axioms (justification + each repair's
removal set) so `render_html` can Manchester-render them with the prefix map.

## Data flow (`build_report`)

```
1. diagnose(onto)                                  [A — consistency + roots/derived]
2. if !consistent:
       inconsistency = { justify: find_one_justification(Inconsistent),
                         repairs: find_repairs(Inconsistent, max) }
   else for each root (capped at max_roots):
       justification = find_one_justification(Unsatisfiable{root})
       repairs       = find_repairs(Unsatisfiable{root}, max)
3. class_count / fragment from classify (diagnose already classifies; expose via a
   classify call or reuse — see plan), derived list straight from the Diagnosis.
```
`max_roots` (CLI `--max-roots`, default 50) bounds the per-root reasoning cost; roots
beyond it are listed in the derived/summary area without justify+repair detail, and
`truncated_roots` records the count (surfaced in the report, never silent).

## The rendered report (matches the mockup)

- **Summary grid:** classes · consistency (green/red) · # unsatisfiable · root/derived
  · fragment.
- **Inconsistent:** a section with the responsible axioms (justification) and repairs.
- **Root section** ("fix these first"): each root a native-collapsible `<details>` —
  *Why it's unsatisfiable* (justification axioms, Manchester; `--labels` adds glosses)
  + *How to fix* (each repair = a labelled "remove N axiom(s)" block) + *Causes* (the
  derived classes it makes unsatisfiable).
- **Derived table:** derived class ⇐ root(s).
- **Coherent:** "No problems found — N classes, all satisfiable."
- **Footer:** soundness + completeness ("repairs complete (Horn)" / "w.r.t. found
  justifications") + "self-contained, read-only" note.

## Self-contained & safe

- A complete standalone document: `<!doctype html><html><head><meta charset><style>…
  </style></head><body>…</body></html>`. **Inline CSS only, no JavaScript**
  (collapsibility via `<details>`/`<summary>`). **No external resources** — no CDN
  script/stylesheet/font, no remote image, no `<link>`/`<script src>`.
- **All dynamic text HTML-escaped** via a `html_escape` helper (`& < > "` →
  entities) — applied to every IRI, label, and Manchester axiom string. Faithful and
  injection-free.

## CLI

```
rustdl report <file> [-o <out.html>] [--labels] [--max-roots N]
```
- Output defaults to **stdout** (pipeable / redirectable); `-o PATH` writes a file.
- `--labels` glosses entities with their `rdfs:label` (reuses justify's label map).
- `--max-roots N` (default 50) caps roots given full detail.

## Testing

- **`html_escape` unit:** `a < b & "c" > d` → fully escaped; ordinary text unchanged.
- **`build_report` + `render_html` on small in-memory ontologies** (declarations
  included, per the prior sub-features' convention):
  - broken (one root, e.g. `X ⊑ A ⊓ ¬A`, `Y ⊑ X`): HTML contains the root IRI, the
    string "Root unsatisfiable", a repair axiom, and the derived `Y`.
  - inconsistent (ABox clash): HTML contains "INCONSISTENT" and the responsible axioms.
  - coherent: HTML contains "No problems found".
- **Self-contained assertion** (on a generated report): the output starts with
  `<!doctype html`, contains `<head>`/`<body>`, and contains **no** `<script`,
  `<link `, `src=`, or `https?://…` *resource* reference (IRIs appear only as escaped
  text content, never in `href`/`src` attributes — assert there are no such attributes).
- **Escaping integration:** an ontology whose IRI/label contains `<`/`&` → the report
  contains the escaped form, not the raw character inside markup.
- **Corpus** (`#[ignore]`d, SHOIN cost): `report` on pizza → non-empty HTML naming the
  unsatisfiable classes; runs without panic.

## Out of scope (v1)

- `prove` proof-tree rendering inside the report (the report's narrative is
  diagnose + justify + repair; proof trees are a future add).
- Interactive JavaScript (search / filter / collapse-all), theming/dark-mode toggle.
- Non-HTML output formats (JSON, PDF), and per-command `--html` flags (the `report`
  command is the single surface).
- Laconic fragments per root (kept to justify + repair for v1, per the brainstorm).
