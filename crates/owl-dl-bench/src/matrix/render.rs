use crate::matrix::model::{CellResult, RunMetadata, Status};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

const REASONER_ORDER: &[&str] = &["rustdl", "konclude", "hermit", "elk", "whelk-rs"];

fn cell_wall(c: &CellResult) -> String {
    match c.status {
        Status::Na => "n/a".into(),
        Status::Dnf => "DNF".into(),
        Status::Error => "err".into(),
        Status::Inconsistent => "inconsistent".into(),
        Status::Ok => c.wall_ms.map_or("?".into(), |w| format!("{w} ms")),
    }
}

fn cell_correctness(c: &CellResult) -> String {
    match (c.fp, c.missed) {
        (Some(fp), Some(m)) => format!("FP {fp} / M {m}"),
        _ => "—".into(),
    }
}

pub fn render_markdown(meta: &RunMetadata, cells: &[CellResult]) -> String {
    let mut s = String::new();
    s.push_str("# rustdl performance matrix\n\n");
    writeln!(s, "**Date:** {}  ", meta.date).ok();
    writeln!(s, "**Tier:** {}  ", meta.tier).ok();
    writeln!(s, "**Oracle:** {} (FP = asserts what the oracle does not; MISSED = oracle subsumptions not asserted)  ", meta.oracle).ok();
    writeln!(
        s,
        "**Host:** {} · {} · {} cores · {} GB · {}  ",
        meta.host.model, meta.host.cpu, meta.host.cores, meta.host.ram_gb, meta.host.os
    )
    .ok();
    writeln!(
        s,
        "**Budgets:** per-pair {} ms, global {} s",
        meta.budgets.pair_timeout_ms, meta.budgets.global_timeout_s
    )
    .ok();
    s.push('\n');
    s.push_str(
        "> **Caveats.** HermiT/ELK walls & RSS are end-to-end **JVM** figures \
        (~0.4–1 s boot floor, ~240 MB baseline) — not pure reasoning time. \
        Konclude runs under **Rosetta 2** (x64), so its walls/RSS are upper bounds. \
        `n/a` = EL-only reasoner on a non-EL ontology.\n\n",
    );

    // Group cells by ont.
    let mut by_ont: BTreeMap<&str, BTreeMap<&str, &CellResult>> = BTreeMap::new();
    let mut onts_order: Vec<&str> = Vec::new();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for c in cells {
        if seen.insert(c.ont.as_str()) {
            onts_order.push(c.ont.as_str());
        }
        by_ont
            .entry(c.ont.as_str())
            .or_default()
            .insert(c.reasoner.as_str(), c);
    }

    // Header.
    s.push_str("| ontology | frag | classes |");
    for r in REASONER_ORDER {
        write!(s, " {r} wall | {r} RSS | {r} FP/M |").ok();
    }
    s.push('\n');
    s.push_str("|---|---|--:|");
    for _ in REASONER_ORDER {
        s.push_str("--:|--:|:--|");
    }
    s.push('\n');

    for ont in &onts_order {
        let row = &by_ont[ont];
        let any = row
            .values()
            .next()
            .expect("ontology should have at least one cell");
        write!(s, "| {} | {} | {} |", ont, any.fragment, any.classes).ok();
        for r in REASONER_ORDER {
            match row.get(r) {
                Some(c) => {
                    write!(
                        s,
                        " {} | {} MB | {} |",
                        cell_wall(c),
                        c.peak_rss_mb.map_or("—".into(), |x| x.to_string()),
                        cell_correctness(c)
                    )
                    .ok();
                }
                None => s.push_str(" — | — | — |"),
            }
        }
        s.push('\n');
    }

    // Summary per reasoner.
    s.push_str(
        "\n## Summary\n\n| reasoner | finished | DNF | error | n/a | total FP | total MISSED |\n",
    );
    s.push_str("|---|--:|--:|--:|--:|--:|--:|\n");
    for r in REASONER_ORDER {
        let rc: Vec<&CellResult> = cells.iter().filter(|c| c.reasoner == *r).collect();
        let count = |st: Status| rc.iter().filter(|c| c.status == st).count();
        let sum_fp: usize = rc.iter().filter_map(|c| c.fp).sum();
        let sum_missed: usize = rc.iter().filter_map(|c| c.missed).sum();
        writeln!(
            s,
            "| {} | {} | {} | {} | {} | {} | {} |",
            r,
            count(Status::Ok),
            count(Status::Dnf),
            count(Status::Error),
            count(Status::Na),
            sum_fp,
            sum_missed
        )
        .ok();
    }
    s
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::matrix::model::*;
    use std::collections::BTreeMap;

    fn meta() -> RunMetadata {
        RunMetadata {
            date: "2026-07-11T00:00:00Z".into(),
            tier: "curated".into(),
            oracle: "konclude-0.7.0-1138".into(),
            host: HostInfo {
                model: "Mac".into(),
                cpu: "M".into(),
                cores: 8,
                ram_gb: 16,
                os: "macOS".into(),
                arch: "arm64".into(),
            },
            budgets: Budgets {
                pair_timeout_ms: 25,
                global_timeout_s: 120,
            },
            reasoners: BTreeMap::new(),
        }
    }
    fn cell(
        ont: &str,
        r: &str,
        status: Status,
        wall: Option<u64>,
        fp: Option<usize>,
    ) -> CellResult {
        CellResult {
            ont: ont.into(),
            source: "s".into(),
            sha256: "h".into(),
            size_bytes: 1,
            classes: 10,
            fragment: "EL".into(),
            reasoner: r.into(),
            status,
            wall_ms: wall,
            peak_rss_mb: Some(20),
            closure_size: Some(10),
            fp,
            missed: Some(0),
            oracle: "konclude-0.7.0-1138".into(),
        }
    }

    #[test]
    fn renders_header_rows_and_caveats() {
        let cells = vec![
            cell("wine", "rustdl", Status::Ok, Some(1770), Some(0)),
            cell("wine", "hermit", Status::Ok, Some(6390), Some(0)),
            cell("wine", "elk", Status::Na, None, None),
        ];
        let md = render_markdown(&meta(), &cells);
        assert!(md.contains("# rustdl performance matrix"));
        assert!(md.contains("konclude-0.7.0-1138")); // oracle stated
        assert!(md.contains("wine"));
        assert!(md.contains("n/a") || md.contains("N/A")); // ELK na cell
        assert!(md.to_lowercase().contains("jvm")); // JVM caveat present
        assert!(md.to_lowercase().contains("rosetta")); // Rosetta caveat present
    }
}
