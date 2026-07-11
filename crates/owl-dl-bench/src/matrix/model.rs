use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Ok,
    Dnf,
    Error,
    Na,
    Inconsistent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CellResult {
    pub ont: String,
    pub source: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub classes: usize,
    pub fragment: String,
    pub reasoner: String,
    pub status: Status,
    pub wall_ms: Option<u64>,
    pub peak_rss_mb: Option<u64>,
    pub closure_size: Option<usize>,
    pub fp: Option<usize>,
    pub missed: Option<usize>,
    pub oracle: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntMeta {
    pub name: String,
    pub source: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub classes: usize,
    pub fragment: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Budgets {
    pub pair_timeout_ms: u64,
    pub global_timeout_s: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostInfo {
    pub model: String,
    pub cpu: String,
    pub cores: u32,
    pub ram_gb: u64,
    pub os: String,
    pub arch: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunMetadata {
    pub date: String,
    pub tier: String,
    pub oracle: String,
    pub host: HostInfo,
    pub budgets: Budgets,
    pub reasoners: BTreeMap<String, serde_json::Value>,
}

pub fn append_cell(path: &Path, cell: &CellResult) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("open {}", path.display()))?;
    let line = serde_json::to_string(cell)?;
    writeln!(f, "{line}")?;
    Ok(())
}

pub fn read_cells(path: &Path) -> Result<Vec<CellResult>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let f = std::fs::File::open(path)?;
    let mut out = Vec::new();
    for line in BufReader::new(f).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        out.push(serde_json::from_str(&line).with_context(|| format!("parse cell: {line}"))?);
    }
    Ok(out)
}

pub fn write_metadata(path: &Path, meta: &RunMetadata) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(path, serde_json::to_string_pretty(meta)?)
        .with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn cell_roundtrips_through_jsonl() {
        let dir = tempdir_here("cell_rt");
        let path = dir.join("results.jsonl");
        let cell = CellResult {
            ont: "wine".into(),
            source: "corpus/wine.ofn".into(),
            sha256: "abc".into(),
            size_bytes: 10,
            classes: 653,
            fragment: "SHOIN(D)".into(),
            reasoner: "rustdl".into(),
            status: Status::Ok,
            wall_ms: Some(1770),
            peak_rss_mb: Some(210),
            closure_size: Some(653),
            fp: Some(0),
            missed: Some(0),
            oracle: "konclude-0.7.0-1138".into(),
        };
        append_cell(&path, &cell).unwrap();
        append_cell(&path, &cell).unwrap();
        let back = read_cells(&path).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].ont, "wine");
        assert_eq!(back[0].status, Status::Ok);
        assert_eq!(back[0].fp, Some(0));
    }

    #[test]
    fn null_correctness_serializes_as_json_null() {
        let cell = CellResult {
            ont: "big".into(),
            source: "x".into(),
            sha256: "z".into(),
            size_bytes: 1,
            classes: 1,
            fragment: "EL".into(),
            reasoner: "hermit".into(),
            status: Status::Dnf,
            wall_ms: None,
            peak_rss_mb: None,
            closure_size: None,
            fp: None,
            missed: None,
            oracle: "konclude-0.7.0-1138".into(),
        };
        let line = serde_json::to_string(&cell).unwrap();
        assert!(line.contains("\"fp\":null"));
        assert!(line.contains("\"status\":\"dnf\""));
    }

    // Minimal deterministic scratch dir under the crate target dir (no external
    // tempdir crate). Uses the process id + test name for uniqueness.
    fn tempdir_here(tag: &str) -> std::path::PathBuf {
        let base = std::env::temp_dir().join(format!("rustdl-matrix-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        base
    }
}
