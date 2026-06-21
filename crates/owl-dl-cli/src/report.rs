//! `rustdl report`: a self-contained HTML debugging report (summary + diagnose
//! roots/derived + per-root justification + repairs). Presentation-only over the
//! shipped reasoner output; runs no new reasoning; read-only.

// Style-only; the `write!`-into-String alternative cascades into `unwrap_used`
// (warn→error here) at every site. PERMANENT — the verbatim renderer trips this
// regardless of wiring; do NOT remove in Task 5.
#![allow(clippy::format_push_string)]

use anyhow::Context;
use horned_owl::curie::PrefixMapping;
use horned_owl::io::omn::AsManchester;
use horned_owl::model::{Component, RcStr};
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::justify::{Entailment, component_entities, find_one_justification};
use std::collections::HashMap;

/// One root unsatisfiable class with its explanation and fixes.
pub(crate) struct RootEntry {
    pub(crate) iri: String,
    pub(crate) justification: Vec<Component<RcStr>>,
    pub(crate) repairs: Vec<Vec<Component<RcStr>>>,
    pub(crate) derives: Vec<String>,
}

/// The inconsistency explanation (when the whole ontology is inconsistent).
pub(crate) struct Section {
    pub(crate) justification: Vec<Component<RcStr>>,
    pub(crate) repairs: Vec<Vec<Component<RcStr>>>,
}

/// Everything the HTML renderer needs — assembled by `build_report`.
pub(crate) struct Report {
    pub(crate) ontology_path: String,
    pub(crate) class_count: usize,
    pub(crate) consistent: bool,
    pub(crate) fragment: String,
    pub(crate) inconsistency: Option<Section>,
    pub(crate) roots: Vec<RootEntry>,
    pub(crate) derived: Vec<(String, Vec<String>)>,
    pub(crate) n_unsat: usize,
    pub(crate) n_root: usize,
    pub(crate) n_derived: usize,
    pub(crate) repairs_complete: bool,
    pub(crate) truncated_roots: usize,
}

/// HTML-escape text for safe, faithful embedding as element content.
/// `&` first (so it doesn't double-escape the entities it introduces).
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

const REPORT_CSS: &str = r"
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
";

fn ax_html(
    ax: &Component<RcStr>,
    pm: &PrefixMapping,
    labels: Option<&HashMap<String, String>>,
) -> String {
    let man = html_escape(&ax.as_manchester_with_prefixes(pm).to_string());
    let mut s = format!("<div class=\"ax\">{man}");
    if let Some(lm) = labels {
        let glosses: Vec<String> = component_entities(ax)
            .into_iter()
            .filter_map(|iri| {
                lm.get(&iri).map(|l| {
                    format!(
                        "{} = \"{}\"",
                        html_escape(crate::local_name(&iri)),
                        html_escape(l)
                    )
                })
            })
            .collect();
        if !glosses.is_empty() {
            s.push_str(&format!(
                "<span class=\"lbl\"> · {}</span>",
                glosses.join("; ")
            ));
        }
    }
    s.push_str("</div>");
    s
}

fn repairs_html(
    repairs: &[Vec<Component<RcStr>>],
    pm: &PrefixMapping,
    labels: Option<&HashMap<String, String>>,
) -> String {
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
pub(crate) fn render_html(
    report: &Report,
    pm: &PrefixMapping,
    labels: Option<&HashMap<String, String>>,
) -> String {
    let mut h = String::new();
    h.push_str("<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">");
    h.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">");
    h.push_str(&format!(
        "<title>rustdl report — {}</title>",
        html_escape(&report.ontology_path)
    ));
    h.push_str(&format!("<style>{REPORT_CSS}</style></head><body>"));

    h.push_str("<h1>rustdl debugging report</h1>");
    h.push_str(&format!(
        "<p class=\"path\">{} · generated by rustdl</p>",
        html_escape(&report.ontology_path)
    ));

    let consistency = if report.consistent {
        "<span class=\"n ok\">consistent</span>"
    } else {
        "<span class=\"n bad\">INCONSISTENT</span>"
    };
    h.push_str("<div class=\"summary\">");
    h.push_str(&format!(
        "<div class=\"stat\"><div class=\"n\">{}</div><div class=\"l\">classes</div></div>",
        report.class_count
    ));
    h.push_str(&format!(
        "<div class=\"stat\">{consistency}<div class=\"l\">consistency</div></div>"
    ));
    h.push_str(&format!(
        "<div class=\"stat\"><div class=\"n bad\">{}</div><div class=\"l\">unsatisfiable</div></div>",
        report.n_unsat
    ));
    h.push_str(&format!(
        "<div class=\"stat\"><div class=\"n\">{} / {}</div><div class=\"l\">root / derived</div></div>",
        report.n_root, report.n_derived
    ));
    h.push_str(&format!(
        "<div class=\"stat\"><div class=\"n\">{}</div><div class=\"l\">fragment</div></div>",
        html_escape(&report.fragment)
    ));
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
                let list: Vec<String> = root
                    .derives
                    .iter()
                    .map(|d| format!("<code>{}</code>", html_escape(d)))
                    .collect();
                h.push_str(&format!(
                    "<div class=\"derives\">Causes {} derived class(es): {}</div>",
                    root.derives.len(),
                    list.join(", ")
                ));
            }
            h.push_str("</details>");
        }
        if report.truncated_roots > 0 {
            h.push_str(&format!(
                "<p class=\"derives\">… and {} more root(s) not detailed (raise --max-roots).</p>",
                report.truncated_roots
            ));
        }
        if !report.derived.is_empty() {
            h.push_str(
                "<h2>Derived unsatisfiable classes — likely resolve once roots are fixed</h2>",
            );
            h.push_str("<table><tr><th>derived class</th><th>depends on root</th></tr>");
            for (d, roots) in &report.derived {
                let rs: Vec<String> = roots
                    .iter()
                    .map(|r| format!("<code>{}</code>", html_escape(r)))
                    .collect();
                h.push_str(&format!(
                    "<tr><td class=\"derived-cls cls\">{}</td><td>{}</td></tr>",
                    html_escape(d),
                    rs.join(", ")
                ));
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

/// Assemble a [`Report`] by running diagnose + per-root justify + per-root repair.
/// Read-only; no new reasoning of its own.
pub(crate) fn build_report(
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
            inconsistency: Some(Section {
                justification,
                repairs,
            }),
            roots: Vec::new(),
            derived: Vec::new(),
            n_unsat: class_count,
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
    let derived = diag
        .derived
        .iter()
        .map(|d| (d.iri.clone(), d.roots.clone()))
        .collect();

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

#[cfg(test)]
mod render_tests {
    use super::*;
    use horned_owl::model::{Build, ClassExpression as CE, SubClassOf};

    fn prefixes() -> horned_owl::curie::PrefixMapping {
        horned_owl::curie::PrefixMapping::default()
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
        assert!(
            html.starts_with("<!doctype html"),
            "must be a full document"
        );
        assert!(html.contains("<head>") && html.contains("<body>"));
        assert!(html.to_lowercase().contains("no problems"));
        assert!(!html.contains("<script"), "no scripts");
        assert!(!html.contains("<link "), "no external stylesheets");
        assert!(!html.contains("src="), "no external src");
        assert!(
            !html.contains("http://") && !html.contains("https://"),
            "no external URLs"
        );
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

#[cfg(test)]
mod build_tests {
    use super::*;
    use horned_owl::model::{
        Build, ClassExpression as CE, DeclareClass, MutableOntology, SubClassOf,
    };
    use horned_owl::ontology::set::SetOntology;

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
            sup: CE::ObjectIntersectionOf(vec![
                cls("urn:A"),
                CE::ObjectComplementOf(Box::new(cls("urn:A"))),
            ]),
        });
        o.insert(SubClassOf {
            sub: cls("urn:SubBad"),
            sup: cls("urn:Bad"),
        });

        let report = build_report(&o, "broken.ofn".into(), 50).expect("build_report");
        assert!(report.consistent);
        assert_eq!(report.n_root, 1);
        assert!(report.roots.iter().any(|r| r.iri == "urn:Bad"));
        assert!(
            !report.roots[0].justification.is_empty(),
            "root has a justification"
        );
        assert!(report.derived.iter().any(|(d, _)| d == "urn:SubBad"));
    }
}

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
        let mut r = std::io::BufReader::new(std::fs::File::open(p).expect("open pizza.ofn"));
        let (onto, pm): (horned_owl::ontology::set::SetOntology<RcStr>, _) =
            read_ofn(&mut r, ParserConfiguration::default()).expect("parse");
        let report = build_report(&onto, "pizza.ofn".into(), 50).expect("build");
        let html = render_html(&report, &pm, None);
        assert!(html.starts_with("<!doctype html"));
        assert!(html.contains("IceCream") || html.contains("CheeseyVegetableTopping"));
        assert!(!html.contains("<script"));
        eprintln!(
            "pizza report: {} bytes, {} root(s)",
            html.len(),
            report.n_root
        );
    }
}
