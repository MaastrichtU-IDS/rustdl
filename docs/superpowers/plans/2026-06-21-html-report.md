# `rustdl report` (HTML debugging report) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `rustdl report <ontology>` — produces one self-contained HTML page (summary + diagnose roots/derived + per-root justification + repairs) for debugging an ontology.

**Architecture:** A new CLI module `crates/owl-dl-cli/src/report.rs` with a `Report` data model, an orchestrator `build_report` (calls the shipped `diagnose`/`find_one_justification`/`find_repairs`/`classify`), and a pure `render_html`. The `report` CLI command wires args → build → render → write. Presentation-only; no new reasoning; read-only.

**Tech Stack:** Rust (edition 2024), horned-owl model + `PrefixMapping`, `owl-dl-reasoner` crate (`diagnose`, `justify::{find_one_justification, find_repairs, Entailment, component_entities}`, `classify`).

**Spec:** `docs/superpowers/specs/2026-06-21-html-report-design.md`
**Branch:** `feat/html-report`

---

## Key facts (verified against the codebase)

- `owl_dl_reasoner::diagnose(onto) -> Result<Diagnosis, ReasonError>`; `Diagnosis { consistent: bool, roots: Vec<String>, derived: Vec<DerivedClass>, all_unsat: Vec<String>, root_derives: BTreeMap<String, Vec<String>> }`; `DerivedClass { iri: String, roots: Vec<String> }`.
- `owl_dl_reasoner::classify(onto) -> Result<Classification, ReasonError>`; `Classification::classes() -> &[String]`; `Classification::stats() -> ClassificationStats`; `ClassificationStats { fragment: FragmentClassification, .. }`; `FragmentClassification: Display`.
- `owl_dl_reasoner::justify::find_one_justification(onto, &Entailment) -> Result<Option<Justification<A>>, ReasonError>` with `Justification { axioms: Vec<Component<A>>, .. }`.
- `owl_dl_reasoner::find_repairs(onto, &Entailment, max) -> Result<Repairs<A>, ReasonError>`; `Repairs { entailed, repairs: Vec<Repair<A>>, complete: bool, dropped_unverified }`; `Repair { remove: Vec<Component<A>> }`.
- `Entailment::Unsatisfiable { class: String }`, `Entailment::Inconsistent`.
- `owl_dl_reasoner::justify::component_entities(&Component<A>) -> BTreeSet<String>`.
- In the CLI (`main.rs`): the type alias `RcStr`; helpers `parse_ofn_with_pm(&Path) -> Result<(SetOntology<RcStr>, PrefixMapping)>`, `build_label_map(&onto) -> <map>` (a `HashMap<String, String>`-like; `lm.get(&iri) -> Option<&String>`), `local_name(&str) -> String`; the method `Component::as_manchester_with_prefixes(&PrefixMapping) -> String`. These are free items in `main.rs`, reachable from a submodule as `crate::local_name` / `crate::build_label_map`, or pass them in.
- `main.rs` currently has NO submodules; adding `mod report;` is the first.

## File structure

- **Create** `crates/owl-dl-cli/src/report.rs` — `Report`/`RootEntry`/`Section` types, `html_escape`, `render_html`, `build_report`, unit tests.
- **Modify** `crates/owl-dl-cli/src/main.rs` — `mod report;`, the `Report` command variant + handler.
- **Modify** `README.md`, `CLAUDE.md` — document (final task).

---

### Task 1: Module skeleton + data model

**Files:** Create `crates/owl-dl-cli/src/report.rs`; Modify `crates/owl-dl-cli/src/main.rs`

ENVIRONMENT: cargo may not be on PATH — prefix shells with:
```bash
export RUSTUP_HOME=/home/dumontier/.rustup
export PATH="/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"
```

- [ ] **Step 1: Branch**

```bash
cd /data/dumontier/rustdl
git checkout main
git checkout -b feat/html-report
```

- [ ] **Step 2: Create `crates/owl-dl-cli/src/report.rs`**

```rust
//! `rustdl report`: a self-contained HTML debugging report (summary + diagnose
//! roots/derived + per-root justification + repairs). Presentation-only over the
//! shipped reasoner output; runs no new reasoning; read-only.

use anyhow::Context;
use horned_owl::model::{Component, RcStr};
use horned_owl::ontology::set::SetOntology;
use horned_owl::io::ofn::writer::AsFunctional; // not used; placeholder removed in Step build if absent

use crate::RcStr as _RcStrAlias; // see note: prefer the crate's RcStr if defined; else horned_owl's

/// One root unsatisfiable class with its explanation and fixes.
pub struct RootEntry {
    pub iri: String,
    pub justification: Vec<Component<RcStr>>,
    pub repairs: Vec<Vec<Component<RcStr>>>,
    pub derives: Vec<String>,
}

/// The inconsistency explanation (when the whole ontology is inconsistent).
pub struct Section {
    pub justification: Vec<Component<RcStr>>,
    pub repairs: Vec<Vec<Component<RcStr>>>,
}

/// Everything the HTML renderer needs — assembled by `build_report`.
pub struct Report {
    pub ontology_path: String,
    pub class_count: usize,
    pub consistent: bool,
    pub fragment: String,
    pub inconsistency: Option<Section>,
    pub roots: Vec<RootEntry>,
    pub derived: Vec<(String, Vec<String>)>,
    pub n_unsat: usize,
    pub n_root: usize,
    pub n_derived: usize,
    pub repairs_complete: bool,
    pub truncated_roots: usize,
}
```

NOTE ON `RcStr`: the CLI already uses a `RcStr` type alias (it is `horned_owl::model::RcStr`). Use **the same `RcStr` the rest of `main.rs` uses** so `Component<RcStr>` matches `parse_ofn_with_pm`'s output. Drop the confusing placeholder imports above; the correct minimal import is `use horned_owl::model::{Component, RcStr};` plus what later steps need. Verify by checking `main.rs`'s `RcStr` definition/import and matching it exactly. Remove the `AsFunctional`/`_RcStrAlias` lines — they are placeholders to delete.

- [ ] **Step 3: Wire the module into `main.rs`**

Add near the top of `crates/owl-dl-cli/src/main.rs` (with the other top-level items, after the `use` block):
```rust
mod report;
```

- [ ] **Step 4: Build**

Run: `cargo build -p owl-dl-cli`
Expected: compiles. Dead-code warnings for the unused types are fine for now. If `RcStr` import is wrong, fix it to match `main.rs`. Do NOT run clippy yet.

- [ ] **Step 5: Commit**

```bash
git add crates/owl-dl-cli/src/report.rs crates/owl-dl-cli/src/main.rs
git commit -m "feat(report): module skeleton + Report data model"
```

---

### Task 2: `html_escape`

**Files:** Modify `crates/owl-dl-cli/src/report.rs`

- [ ] **Step 1: Write the failing test** — append to `report.rs`:

```rust
#[cfg(test)]
mod escape_tests {
    use super::*;

    #[test]
    fn escapes_markup_chars() {
        assert_eq!(
            html_escape(r#"a < b & "c" > d"#),
            "a &lt; b &amp; &quot;c&quot; &gt; d"
        );
    }

    #[test]
    fn plain_text_unchanged() {
        assert_eq!(html_escape("PolarBear ⊑ Animal"), "PolarBear ⊑ Animal");
    }
}
```

- [ ] **Step 2: Run** — `cargo test -p owl-dl-cli --lib escape_tests` → FAIL (`html_escape` undefined).

- [ ] **Step 3: Implement** — add to `report.rs` (before the test module):

```rust
/// HTML-escape text for safe, faithful embedding as element content.
/// `&` first (so it doesn't double-escape the entities it introduces).
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
```

- [ ] **Step 4: Run** — `cargo test -p owl-dl-cli --lib escape_tests` → 2 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/owl-dl-cli/src/report.rs
git commit -m "feat(report): html_escape helper"
```

---

### Task 3: `render_html` (pure renderer)

**Files:** Modify `crates/owl-dl-cli/src/report.rs`

- [ ] **Step 1: Write the failing tests** — append to `report.rs`:

```rust
#[cfg(test)]
mod render_tests {
    use super::*;
    use horned_owl::model::{Build, ClassExpression as CE, SubClassOf};

    fn pm() -> horned_owl::ontology::set::SetOntology<RcStr> {
        // not used directly; render takes a PrefixMapping — build a default one below
        horned_owl::ontology::set::SetOntology::new()
    }
    fn prefixes() -> curie::PrefixMapping {
        curie::PrefixMapping::default()
    }
    fn sc(b: &Build<RcStr>, s: &str, t: &str) -> Component<RcStr> {
        Component::SubClassOf(SubClassOf {
            sub: CE::Class(b.class(s)),
            sup: CE::Class(b.class(t)),
        })
    }

    fn coherent_report() -> Report {
        Report {
            ontology_path: "ex.ofn".into(),
            class_count: 12,
            consistent: true,
            fragment: "EL".into(),
            inconsistency: None,
            roots: Vec::new(),
            derived: Vec::new(),
            n_unsat: 0,
            n_root: 0,
            n_derived: 0,
            repairs_complete: true,
            truncated_roots: 0,
        }
    }

    #[test]
    fn coherent_says_no_problems_and_is_self_contained() {
        let html = render_html(&coherent_report(), &prefixes(), None);
        assert!(html.starts_with("<!doctype html"), "must be a full document");
        assert!(html.contains("<head>") && html.contains("<body>"));
        assert!(html.to_lowercase().contains("no problems"));
        // self-contained: no external resources
        assert!(!html.contains("<script"), "no scripts");
        assert!(!html.contains("<link "), "no external stylesheets");
        assert!(!html.contains("src="), "no external src");
        assert!(!html.contains("http://") && !html.contains("https://"), "no external URLs");
    }

    #[test]
    fn root_report_shows_why_and_fix() {
        let b = Build::new_rc();
        let report = Report {
            roots: vec![RootEntry {
                iri: "urn:Bad".into(),
                justification: vec![sc(&b, "urn:Bad", "urn:A")],
                repairs: vec![vec![sc(&b, "urn:Bad", "urn:A")]],
                derives: vec!["urn:SubBad".into()],
            }],
            n_unsat: 2,
            n_root: 1,
            n_derived: 1,
            derived: vec![("urn:SubBad".into(), vec!["urn:Bad".into()])],
            ..coherent_report()
        };
        let html = render_html(&report, &prefixes(), None);
        assert!(html.contains("urn:Bad"), "names the root");
        assert!(html.to_lowercase().contains("root unsatisfiable"));
        assert!(html.to_lowercase().contains("repair"));
        assert!(html.contains("urn:SubBad"), "lists the derived class");
    }

    #[test]
    fn escapes_dynamic_text() {
        // a class IRI containing markup must appear escaped, never raw inside markup.
        let b = Build::new_rc();
        let report = Report {
            roots: vec![RootEntry {
                iri: "urn:a<b".into(),
                justification: vec![sc(&b, "urn:a<b", "urn:A")],
                repairs: Vec::new(),
                derives: Vec::new(),
            }],
            n_unsat: 1,
            n_root: 1,
            ..coherent_report()
        };
        let html = render_html(&report, &prefixes(), None);
        assert!(html.contains("urn:a&lt;b"), "root IRI must be escaped");
        assert!(!html.contains("urn:a<b"), "raw < must not leak into markup");
    }
}
```

NOTE: `curie::PrefixMapping` is horned-owl's prefix-map type (the same one `parse_ofn_with_pm` returns). If the path differs, check `main.rs`'s import for the `pm` type returned by `parse_ofn_with_pm` and use that exact path in the test (and in `render_html`'s signature). Remove the unused `pm()` helper if it does not compile; only `prefixes()` is needed.

- [ ] **Step 2: Run** — `cargo test -p owl-dl-cli --lib render_tests` → FAIL (`render_html` undefined).

- [ ] **Step 3: Implement `render_html`** — add to `report.rs`. Use the prefix-map type that `parse_ofn_with_pm` returns (shown here as `curie::PrefixMapping`; adjust to match `main.rs`). `LabelMap` is the type `build_label_map` returns (a `HashMap<String, String>`).

```rust
use curie::PrefixMapping;
use owl_dl_reasoner::justify::component_entities;
use std::collections::HashMap;

const REPORT_CSS: &str = r#"
:root{--ink:#1a1a1a;--muted:#6b7280;--line:#e5e7eb;--root:#b91c1c;--derived:#b45309;--fix:#047857;--chip:#f3f4f6}
body{font:15px/1.55 -apple-system,system-ui,sans-serif;color:var(--ink);max-width:920px;margin:0 auto;padding:28px 20px 80px}
h1{font-size:22px;margin:0 0 2px}.path{color:var(--muted);font:13px ui-monospace,monospace;margin:0 0 18px}
.summary{display:grid;grid-template-columns:repeat(auto-fit,minmax(130px,1fr));gap:10px;margin:0 0 26px}
.stat{border:1px solid var(--line);border-radius:8px;padding:10px 12px}.stat .n{font-size:20px;font-weight:700}
.stat .l{font-size:12px;color:var(--muted);text-transform:uppercase;letter-spacing:.03em}
.ok{color:var(--fix)}.bad{color:var(--root)}
h2{font-size:15px;text-transform:uppercase;letter-spacing:.04em;color:var(--muted);border-bottom:1px solid var(--line);padding-bottom:6px;margin:30px 0 12px}
details{border:1px solid var(--line);border-left:3px solid var(--root);border-radius:0 6px 6px 0;margin:8px 0;padding:0 14px}
details[open]{padding-bottom:12px}summary{cursor:pointer;padding:11px 0;font-weight:600}
summary .tag{display:inline-block;font:11px/1 ui-monospace,monospace;background:var(--root);color:#fff;border-radius:4px;padding:3px 6px;margin-right:8px}
.cls{font:14px ui-monospace,monospace}.block .h{font-size:12px;font-weight:700;text-transform:uppercase;color:var(--muted);margin:10px 0 4px}
.why .h{color:var(--root)}.fix .h{color:var(--fix)}
.ax{font:13px ui-monospace,monospace;background:var(--chip);border-radius:5px;padding:5px 9px;margin:3px 0}
.ax .lbl{color:var(--muted);font-size:11.5px}
.repair{border:1px solid var(--line);border-radius:6px;padding:6px 10px;margin:6px 0}.repair .cap{font-size:12px;color:var(--fix);font-weight:600;margin-bottom:3px}
.derives{font-size:12.5px;color:var(--muted);margin-top:8px}
table{border-collapse:collapse;width:100%;font-size:13.5px}td,th{text-align:left;padding:6px 9px;border-bottom:1px solid var(--line)}
th{color:var(--muted);font-size:12px;text-transform:uppercase}.derived-cls{border-left:3px solid var(--derived)}
.foot{color:var(--muted);font-size:12px;margin-top:36px;border-top:1px solid var(--line);padding-top:10px}
"#;

/// Render one axiom as an `.ax` div (Manchester text, escaped; optional labels).
fn ax_html(ax: &Component<RcStr>, pm: &PrefixMapping, labels: Option<&HashMap<String, String>>) -> String {
    let man = html_escape(&ax.as_manchester_with_prefixes(pm));
    let mut s = format!("<div class=\"ax\">{man}");
    if let Some(lm) = labels {
        let glosses: Vec<String> = component_entities(ax)
            .into_iter()
            .filter_map(|iri| {
                lm.get(&iri)
                    .map(|l| format!("{} = \"{}\"", html_escape(&crate::local_name(&iri)), html_escape(l)))
            })
            .collect();
        if !glosses.is_empty() {
            s.push_str(&format!("<span class=\"lbl\"> · {}</span>", glosses.join("; ")));
        }
    }
    s.push_str("</div>");
    s
}

fn repairs_html(repairs: &[Vec<Component<RcStr>>], pm: &PrefixMapping, labels: Option<&HashMap<String, String>>) -> String {
    if repairs.is_empty() {
        return "<div class=\"derives\">no verifiable repair found</div>".to_string();
    }
    let mut s = String::new();
    for (i, rep) in repairs.iter().enumerate() {
        s.push_str(&format!(
            "<div class=\"repair\"><div class=\"cap\">repair {} — remove {} axiom(s)</div>",
            i + 1,
            rep.len()
        ));
        for ax in rep {
            s.push_str(&ax_html(ax, pm, labels));
        }
        s.push_str("</div>");
    }
    s
}

/// Render the full self-contained HTML document for `report`.
pub fn render_html(report: &Report, pm: &PrefixMapping, labels: Option<&HashMap<String, String>>) -> String {
    let mut h = String::new();
    h.push_str("<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">");
    h.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">");
    h.push_str(&format!("<title>rustdl report — {}</title>", html_escape(&report.ontology_path)));
    h.push_str(&format!("<style>{REPORT_CSS}</style></head><body>"));

    h.push_str("<h1>rustdl debugging report</h1>");
    h.push_str(&format!("<p class=\"path\">{} · generated by rustdl</p>", html_escape(&report.ontology_path)));

    // summary grid
    let consistency = if report.consistent {
        "<span class=\"n ok\">consistent</span>"
    } else {
        "<span class=\"n bad\">INCONSISTENT</span>"
    };
    h.push_str("<div class=\"summary\">");
    h.push_str(&format!("<div class=\"stat\"><div class=\"n\">{}</div><div class=\"l\">classes</div></div>", report.class_count));
    h.push_str(&format!("<div class=\"stat\">{consistency}<div class=\"l\">consistency</div></div>"));
    h.push_str(&format!("<div class=\"stat\"><div class=\"n bad\">{}</div><div class=\"l\">unsatisfiable</div></div>", report.n_unsat));
    h.push_str(&format!("<div class=\"stat\"><div class=\"n\">{} / {}</div><div class=\"l\">root / derived</div></div>", report.n_root, report.n_derived));
    h.push_str(&format!("<div class=\"stat\"><div class=\"n\">{}</div><div class=\"l\">fragment</div></div>", html_escape(&report.fragment)));
    h.push_str("</div>");

    if let Some(sec) = &report.inconsistency {
        h.push_str("<h2>Inconsistent ontology — responsible axioms</h2>");
        h.push_str("<div class=\"block why\"><div class=\"h\">Why it is inconsistent (minimal justification)</div>");
        for ax in &sec.justification {
            h.push_str(&ax_html(ax, pm, labels));
        }
        h.push_str("</div><div class=\"block fix\"><div class=\"h\">How to fix (minimal repairs — remove one set)</div>");
        h.push_str(&repairs_html(&sec.repairs, pm, labels));
        h.push_str("</div>");
    } else if report.n_unsat == 0 {
        h.push_str(&format!(
            "<h2>No problems found</h2><p>All {} classes are satisfiable and the ontology is consistent.</p>",
            report.class_count
        ));
    } else {
        // roots
        h.push_str("<h2>Root unsatisfiable classes — fix these first</h2>");
        for root in &report.roots {
            h.push_str(&format!(
                "<details><summary><span class=\"tag\">ROOT</span><span class=\"cls\">{}</span></summary>",
                html_escape(&root.iri)
            ));
            h.push_str("<div class=\"block why\"><div class=\"h\">Why it's unsatisfiable (minimal justification)</div>");
            for ax in &root.justification {
                h.push_str(&ax_html(ax, pm, labels));
            }
            h.push_str("</div><div class=\"block fix\"><div class=\"h\">How to fix (minimal repairs — remove one set)</div>");
            h.push_str(&repairs_html(&root.repairs, pm, labels));
            h.push_str("</div>");
            if !root.derives.is_empty() {
                let list: Vec<String> = root.derives.iter().map(|d| format!("<code>{}</code>", html_escape(d))).collect();
                h.push_str(&format!("<div class=\"derives\">Causes {} derived class(es): {}</div>", root.derives.len(), list.join(", ")));
            }
            h.push_str("</details>");
        }
        if report.truncated_roots > 0 {
            h.push_str(&format!("<p class=\"derives\">… and {} more root(s) not detailed (raise --max-roots).</p>", report.truncated_roots));
        }
        if !report.derived.is_empty() {
            h.push_str("<h2>Derived unsatisfiable classes — likely resolve once roots are fixed</h2>");
            h.push_str("<table><tr><th>derived class</th><th>depends on root</th></tr>");
            for (d, roots) in &report.derived {
                let rs: Vec<String> = roots.iter().map(|r| format!("<code>{}</code>", html_escape(r))).collect();
                h.push_str(&format!("<tr><td class=\"derived-cls cls\">{}</td><td>{}</td></tr>", html_escape(d), rs.join(", ")));
            }
            h.push_str("</table>");
        }
    }

    let completeness = if report.repairs_complete {
        "minimal repairs complete"
    } else {
        "repairs w.r.t. found justifications (completeness not guaranteed)"
    };
    h.push_str(&format!(
        "<p class=\"foot\">Sound by construction: every justification and repair is verified against the reasoner. {completeness}. Self-contained HTML, no external resources. This report is read-only — it never modifies the ontology.</p>"
    ));
    h.push_str("</body></html>");
    h
}
```

- [ ] **Step 4: Run** — `cargo test -p owl-dl-cli --lib render_tests` → 3 passed. If the prefix-map / label-map types mismatch, fix the signatures to match `main.rs` (see NOTE). Paste the `test result:` line.

- [ ] **Step 5: clippy + fmt** —
```bash
cargo clippy -p owl-dl-cli --all-targets -- -D warnings
cargo fmt -p owl-dl-cli
```
`build_report` is still absent, so `render_html`/helpers are used only by tests → if clippy flags them dead in the non-test build, add `#[allow(dead_code)] // wired into the report command in Task 5; allow removed there` (Task 5 removes). Re-run render_tests after fmt.

- [ ] **Step 6: Commit**

```bash
git add crates/owl-dl-cli/src/report.rs
git commit -m "feat(report): self-contained HTML renderer (escaped, no external resources)"
```

---

### Task 4: `build_report` (orchestration)

**Files:** Modify `crates/owl-dl-cli/src/report.rs`

- [ ] **Step 1: Write the failing test** — append to `report.rs`:

```rust
#[cfg(test)]
mod build_tests {
    use super::*;
    use horned_owl::model::{Build, ClassExpression as CE, DeclareClass, MutableOntology, SubClassOf};

    // Bad ⊑ A ⊓ ¬A (root), SubBad ⊑ Bad (derived) → report names both.
    #[test]
    fn build_report_on_broken_ontology() {
        let b = Build::new_rc();
        let cls = |iri: &str| CE::Class(b.class(iri));
        let mut o = SetOntology::new();
        for c in ["urn:A", "urn:Bad", "urn:SubBad"] {
            o.insert(DeclareClass(b.class(c)));
        }
        o.insert(SubClassOf {
            sub: cls("urn:Bad"),
            sup: CE::ObjectIntersectionOf(vec![cls("urn:A"), CE::ObjectComplementOf(Box::new(cls("urn:A")))]),
        });
        o.insert(SubClassOf { sub: cls("urn:SubBad"), sup: cls("urn:Bad") });

        let report = build_report(&o, "broken.ofn".into(), 50).expect("build_report");
        assert!(report.consistent);
        assert_eq!(report.n_root, 1);
        assert!(report.roots.iter().any(|r| r.iri == "urn:Bad"));
        assert!(!report.roots[0].justification.is_empty(), "root has a justification");
        assert!(report.derived.iter().any(|(d, _)| d == "urn:SubBad"));
    }
}
```

- [ ] **Step 2: Run** — `cargo test -p owl-dl-cli --lib build_tests` → FAIL (`build_report` undefined).

- [ ] **Step 3: Implement `build_report`** — add to `report.rs`:

```rust
use owl_dl_reasoner::justify::{Entailment, find_one_justification};

/// Assemble a [`Report`] by running diagnose + per-root justify + per-root repair.
/// Read-only; no new reasoning of its own.
pub fn build_report(
    onto: &SetOntology<RcStr>,
    ontology_path: String,
    max_roots: usize,
) -> anyhow::Result<Report> {
    let classification = owl_dl_reasoner::classify(onto).context("classify")?;
    let class_count = classification.classes().len();
    let fragment = format!("{}", classification.stats().fragment);

    let diag = owl_dl_reasoner::diagnose(onto).context("diagnose")?;

    // Inconsistent: one section (justify + repair the inconsistency).
    if !diag.consistent {
        let q = Entailment::Inconsistent;
        let justification = find_one_justification(onto, &q)
            .context("justify inconsistency")?
            .map(|j| j.axioms)
            .unwrap_or_default();
        let rep = owl_dl_reasoner::find_repairs(onto, &q, 10).context("repair inconsistency")?;
        let repairs = rep.repairs.into_iter().map(|r| r.remove).collect();
        return Ok(Report {
            ontology_path,
            class_count,
            consistent: false,
            fragment,
            inconsistency: Some(Section { justification, repairs }),
            roots: Vec::new(),
            derived: Vec::new(),
            n_unsat: class_count, // inconsistent ⇒ every class unsatisfiable
            n_root: 0,
            n_derived: 0,
            repairs_complete: rep.complete,
            truncated_roots: 0,
        });
    }

    // Consistent: per-root justify + repair (capped at max_roots).
    let n_root = diag.roots.len();
    let n_derived = diag.derived.len();
    let n_unsat = diag.all_unsat.len();
    let truncated_roots = n_root.saturating_sub(max_roots);
    let mut repairs_complete = true;
    let mut roots = Vec::new();
    for iri in diag.roots.iter().take(max_roots) {
        let q = Entailment::Unsatisfiable { class: iri.clone() };
        let justification = find_one_justification(onto, &q)
            .context("justify root")?
            .map(|j| j.axioms)
            .unwrap_or_default();
        let rep = owl_dl_reasoner::find_repairs(onto, &q, 10).context("repair root")?;
        repairs_complete &= rep.complete;
        let derives = diag.root_derives.get(iri).cloned().unwrap_or_default();
        roots.push(RootEntry {
            iri: iri.clone(),
            justification,
            repairs: rep.repairs.into_iter().map(|r| r.remove).collect(),
            derives,
        });
    }
    let derived = diag.derived.iter().map(|d| (d.iri.clone(), d.roots.clone())).collect();

    Ok(Report {
        ontology_path,
        class_count,
        consistent: true,
        fragment,
        inconsistency: None,
        roots,
        derived,
        n_unsat,
        n_root,
        n_derived,
        repairs_complete,
        truncated_roots,
    })
}
```

- [ ] **Step 4: Run** — `cargo test -p owl-dl-cli --lib build_tests` → 1 passed. Paste the `test result:` line.

- [ ] **Step 5: full report tests + clippy + fmt** —
```bash
cargo test -p owl-dl-cli --lib report
cargo clippy -p owl-dl-cli --all-targets -- -D warnings
cargo fmt -p owl-dl-cli
```
All green. (`render_html`/helpers may still be `#[allow(dead_code)]` until Task 5 — leave any such allow.) Re-run after fmt.

- [ ] **Step 6: Commit**

```bash
git add crates/owl-dl-cli/src/report.rs
git commit -m "feat(report): build_report orchestration (diagnose + justify + repair)"
```

---

### Task 5: CLI `report` command

**Files:** Modify `crates/owl-dl-cli/src/main.rs`

- [ ] **Step 1: Add the `Report` variant** — in `enum Command`, after the `Diagnose { … }` variant:

```rust
    /// Generate a self-contained HTML debugging report (consistency, root/derived
    /// unsatisfiable classes, justifications, and repair suggestions).
    Report {
        /// Path to the ontology (.ofn / .owx / .owl / .rdf).
        file: PathBuf,
        /// Write the HTML to this file (default: stdout).
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Gloss each axiom with the rdfs:label of the entities it mentions.
        #[arg(long)]
        labels: bool,
        /// Maximum number of root classes given full justify+repair detail.
        #[arg(long, default_value_t = 50)]
        max_roots: usize,
    },
```

- [ ] **Step 2: Add the handler** — in `match command { … }`, after the `Command::Diagnose { … } => { … }` arm:

```rust
        Command::Report {
            file,
            output,
            labels,
            max_roots,
        } => {
            let (onto, pm) = parse_ofn_with_pm(&file)?;
            let label_map = labels.then(|| build_label_map(&onto));
            let report = report::build_report(&onto, file.display().to_string(), max_roots)?;
            let html = report::render_html(&report, &pm, label_map.as_ref());
            match output {
                Some(path) => {
                    std::fs::write(&path, html)
                        .with_context(|| format!("writing report to {}", path.display()))?;
                    eprintln!("report written to {}", path.display());
                }
                None => println!("{html}"),
            }
        }
```
Then REMOVE any `#[allow(dead_code)]` you added to `render_html`/`build_report`/helpers in Tasks 3–4 (now used). If `label_map.as_ref()` type doesn't match `render_html`'s `Option<&HashMap<String,String>>`, adjust `render_html`'s label param to the exact type `build_label_map` returns (find its signature in `main.rs`) — keep it consistent across `report.rs`.

- [ ] **Step 3: Build** — `cargo build -p owl-dl-cli`

- [ ] **Step 4: Smoke-test** —
```bash
cat > /tmp/report-smoke.ofn <<'EOF'
Prefix(:=<urn:>)
Ontology(
  Declaration(Class(:A)) Declaration(Class(:Bad)) Declaration(Class(:SubBad))
  SubClassOf(:Bad ObjectIntersectionOf(:A ObjectComplementOf(:A)))
  SubClassOf(:SubBad :Bad)
)
EOF
cargo build -p owl-dl-cli --release
./target/release/rustdl report /tmp/report-smoke.ofn -o /tmp/report-smoke.html
echo "--- head ---"; head -c 400 /tmp/report-smoke.html
echo ""; echo "--- key content present? ---"
grep -c 'Root unsatisfiable' /tmp/report-smoke.html
grep -c 'urn:Bad' /tmp/report-smoke.html
grep -c 'repair' /tmp/report-smoke.html
grep -c '<script\|<link \|https\?://' /tmp/report-smoke.html
```
Expected: file starts `<!doctype html`; "Root unsatisfiable" ≥1; "urn:Bad" ≥1; "repair" ≥1; external-resource count **0**. Paste the actual numbers. If "Root unsatisfiable" is 0 or external count >0, STOP and report.

- [ ] **Step 5: clippy + fmt** —
```bash
cargo clippy -p owl-dl-cli --all-targets -- -D warnings
cargo fmt -p owl-dl-cli
```

- [ ] **Step 6: Commit**

```bash
git add crates/owl-dl-cli/src/main.rs crates/owl-dl-cli/src/report.rs
git commit -m "feat(report): rustdl report CLI command"
```

---

### Task 6: Corpus check + docs + final gate

**Files:** Modify `crates/owl-dl-cli/src/report.rs` (optional ignored corpus test), `README.md`, `CLAUDE.md`

- [ ] **Step 1: Add an ignored corpus test** — append a test to `report.rs` (mirrors the build_tests style but reads pizza). It is `#[ignore]`d (SHOIN justify cost):

```rust
#[cfg(test)]
mod corpus_tests {
    use super::*;

    #[test]
    #[ignore = "reads the curated corpus (ontologies/real/pizza.ofn)"]
    fn report_on_pizza() {
        use horned_owl::io::ParserConfiguration;
        use horned_owl::io::ofn::reader::read as read_ofn;
        let p = std::path::Path::new("../../ontologies/real/pizza.ofn");
        if !p.exists() {
            eprintln!("skip pizza.ofn (not present)");
            return;
        }
        let mut r = std::io::BufReader::new(std::fs::File::open(p).unwrap());
        let (onto, pm): (SetOntology<RcStr>, _) =
            read_ofn(&mut r, ParserConfiguration::default()).expect("parse");
        let report = build_report(&onto, "pizza.ofn".into(), 50).expect("build");
        let html = render_html(&report, &pm, None);
        assert!(html.starts_with("<!doctype html"));
        assert!(html.contains("IceCream") || html.contains("CheeseyVegetableTopping"));
        assert!(!html.contains("<script"));
        eprintln!("pizza report: {} bytes, {} root(s)", html.len(), report.n_root);
    }
}
```

- [ ] **Step 2: Run it** —
```bash
cargo test -p owl-dl-cli --lib report_on_pizza -- --ignored --nocapture
```
Expected: PASS with `pizza report: N bytes, M root(s)`. May take a few minutes (SHOIN). If it exceeds ~5 min, kill and note — the in-memory tests validate correctness; this is supplementary and stays `#[ignore]`d.

- [ ] **Step 3: README** — in the CLI block, after the `rustdl diagnose …` line, add (column-aligned):
```
rustdl report    ontology.ofn -o report.html # self-contained HTML debugging report
```

- [ ] **Step 4: CLAUDE.md** — append to the `owl-dl-cli` bullet:
```
`report` generates a self-contained HTML debugging report (summary + diagnose
roots/derived + per-root justification + repairs); presentation-only over the
shipped reasoner output, read-only, no external resources. See
`docs/superpowers/specs/2026-06-21-html-report-design.md`.
```

- [ ] **Step 5: Full workspace gate** —
```bash
cd /data/dumontier/rustdl
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```
All three green. The corpus report test is `#[ignore]`d (do NOT pass `--ignored`). Report any NON-ignored failure verbatim; fix only report-related clippy, stop+report on unrelated pre-existing issues.

- [ ] **Step 6: Commit**

```bash
git add crates/owl-dl-cli/src/report.rs README.md CLAUDE.md
git commit -m "test+docs(report): corpus check + README/CLAUDE notes"
```

---

## Self-review notes (author)

- **Spec coverage:** html_escape → Task 2; render_html (summary/inconsistent/roots/derived/coherent, self-contained, escaped) → Task 3 (with self-contained + escaping assertions); build_report (diagnose + per-root justify + repair, max_roots cap, completeness flag) → Task 4; CLI `report` + `-o`/`--labels`/`--max-roots` → Task 5; corpus + read-only + docs → Task 6.
- **Soundness:** presentation-only; no new reasoning; the renderer escapes all dynamic text and emits no external resources (asserted in tests). FP=0 untouched (classification read-only).
- **No placeholders:** the only intentional "fix to match" notes are the prefix-map and label-map TYPES (Task 1/3/5) — the implementer confirms against `main.rs`'s `parse_ofn_with_pm` / `build_label_map` signatures. Every other step is complete.
- **Type consistency:** `Report`/`RootEntry`/`Section` field names consistent across Tasks 1/3/4; `render_html(&Report, &PrefixMapping, Option<&HashMap<String,String>>)` and `build_report(&SetOntology<RcStr>, String, usize)` signatures consistent across tasks. `Component::SubClassOf(SubClassOf{sub,sup})` matches the repo.
- **API risk flagged inline:** prefix-map type path (`curie::PrefixMapping`), label-map type, `RcStr` alias source, `mod report;` placement, and the `DeclareClass` requirement for in-memory test ontologies — each task points at `main.rs` / prior sub-features to copy.
