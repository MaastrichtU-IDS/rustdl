//! Print every structural property surface's answer and its `structural_gaps`
//! for one ontology (#389, #390), as one JSON object on stdout.
//!
//! Used for the ORE before/after of the structural fixes: every fix is behind a
//! default-ON flag, so running this once with the flags at their defaults and once
//! with all of them `=0` compares the two answers on one binary. Answers are the
//! transitive closures and pair sets, not Hasse rows: a fix that merges two
//! properties into one equivalence group removes direct rows while adding
//! entailments.
//!
//! ```sh
//! cargo run --release --example property_surfaces_probe -- <file.ofn>
//! ```

#![allow(clippy::unwrap_used, clippy::print_stdout, clippy::print_stderr)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::ReasonError;
use std::collections::hash_map::DefaultHasher;
use std::fmt::Write as _;
use std::hash::{Hash, Hasher};

fn q(s: &str) -> String {
    let mut o = String::from('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                let _ = write!(o, "\\u{:04x}", c as u32);
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// `{"n":…,"hash":…[,"rows":[[a,b],…]]}` for a sorted copy of `rows`.
fn summary(rows: &[(String, String)], keep: bool) -> String {
    let mut v = rows.to_vec();
    v.sort();
    let mut h = DefaultHasher::new();
    v.hash(&mut h);
    let mut s = format!("{{\"n\":{},\"hash\":\"{:016x}\"", v.len(), h.finish());
    if keep {
        let r: Vec<String> = v
            .iter()
            .map(|(a, b)| format!("[{},{}]", q(a), q(b)))
            .collect();
        let _ = write!(s, ",\"rows\":[{}]", r.join(","));
    }
    s.push('}');
    s
}

fn res(r: Result<String, ReasonError>) -> String {
    r.unwrap_or_else(|e| format!("{{\"error\":{}}}", q(&e.to_string())))
}

fn list(v: &[String]) -> String {
    format!("[{}]", v.iter().map(|k| q(k)).collect::<Vec<_>>().join(","))
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: property_surfaces_probe <file.ofn>");
    let f = std::fs::File::open(&path).unwrap();
    let (o, _): (SetOntology<RcStr>, _) = read_ofn(
        &mut std::io::BufReader::new(f),
        ParserConfiguration::default(),
    )
    .unwrap();
    // `PROBE_ROWS=1` keeps the row lists so a DIFFER can be read without re-running.
    let keep = std::env::var_os("PROBE_ROWS").is_some();
    let pairs = |r: Result<Vec<(String, String)>, ReasonError>| res(r.map(|v| summary(&v, keep)));
    let values = res(
        owl_dl_reasoner::materialize_data_property_assertions(&o).map(|v| {
            let flat: Vec<(String, String)> = v
                .into_iter()
                .map(|(s, p, l, d, g)| (s, format!("{p}\t{l}\t{d}\t{g}")))
                .collect();
            summary(&flat, false)
        }),
    );
    let gaps = res(owl_dl_reasoner::property_structural_gaps(&o).map(|g| {
        format!(
            "{{\"object_hierarchy\":{},\"data_hierarchy\":{},\"object_disjoint\":{},\"data_disjoint\":{},\"data_values\":{}}}",
            list(g.object_hierarchy()),
            list(g.data_hierarchy()),
            list(g.object_disjoint()),
            list(g.data_disjoint()),
            list(g.data_values()),
        )
    }));
    println!(
        "{{\"sub_object\":{},\"sub_data\":{},\"disjoint_object\":{},\"disjoint_data\":{},\"data_values\":{},\"gaps\":{}}}",
        pairs(owl_dl_reasoner::materialize_subobjectproperty_axioms(&o)),
        pairs(owl_dl_reasoner::materialize_subdataproperty_axioms(&o)),
        pairs(owl_dl_reasoner::disjoint_object_properties(&o)),
        pairs(owl_dl_reasoner::disjoint_data_properties(&o)),
        values,
        gaps,
    );
}
