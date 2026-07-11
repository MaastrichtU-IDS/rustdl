use crate::matrix::model::OntMeta;
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex(&h.finalize())
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

pub fn is_el_fragment(frag: &str) -> bool {
    frag.starts_with("EL")
}

/// Coarse fragment label. A refined label is not required by the matrix — only
/// the EL/non-EL distinction gates the `na` status for ELK/whelk. We reuse the
/// reasoner's own profile detection where available; otherwise a lexical scan
/// of the functional-syntax source for non-EL constructs.
pub fn fragment_of(ofn_path: &Path) -> String {
    let src = std::fs::read_to_string(ofn_path).unwrap_or_default();
    // Non-EL constructs: unions, complements, universals, cardinalities,
    // nominals, inverse roles.
    const NON_EL: &[&str] = &[
        "ObjectUnionOf",
        "ObjectComplementOf",
        "ObjectAllValuesFrom",
        "ObjectMaxCardinality",
        "ObjectMinCardinality",
        "ObjectExactCardinality",
        "ObjectOneOf",
        "ObjectInverseOf",
        "DisjointUnion",
    ];
    if NON_EL.iter().any(|c| src.contains(c)) {
        "DL".into()
    } else {
        "EL".into()
    }
}

pub struct StagedOnt {
    pub meta: OntMeta,
    pub ofn: PathBuf,
    pub owl: PathBuf,
    pub owx: PathBuf,
}

/// Ensure `dst` exists by converting `src` with `robot convert`. No-op if `dst`
/// is already present (conversions are cached per ont).
fn ensure_convert(robot: &Path, src: &Path, dst: &Path) -> Result<()> {
    if dst.exists() {
        return Ok(());
    }
    let status = Command::new(robot)
        .arg("convert")
        .arg("-i")
        .arg(src)
        .arg("-o")
        .arg(dst)
        .status()
        .with_context(|| format!("robot convert {} -> {}", src.display(), dst.display()))?;
    if !status.success() {
        anyhow::bail!("robot convert failed for {}", src.display());
    }
    Ok(())
}

/// Directory + glob per tier. `curated` reads the pre-staged `work_dir`;
/// `ore`/`bioportal` read the data dirs (env-overridable).
fn tier_sources(tier: &str, work_dir: &Path) -> Result<Vec<PathBuf>> {
    let home = std::env::var("HOME").unwrap_or_default();
    let (root, ext): (PathBuf, &str) = match tier {
        "curated" => (work_dir.to_path_buf(), "ofn"),
        "ore" => (
            PathBuf::from(
                std::env::var("RUSTDL_ORE_DIR").unwrap_or(format!("{home}/data/ore-run")),
            ),
            "owl",
        ),
        "bioportal" => (
            PathBuf::from(
                std::env::var("RUSTDL_BIOPORTAL_DIR")
                    .unwrap_or(format!("{home}/data/bioportal/owl")),
            ),
            "owl",
        ),
        other => anyhow::bail!("unknown tier {other}"),
    };
    let mut out = Vec::new();
    for e in walkdir::WalkDir::new(&root)
        .into_iter()
        .filter_map(std::result::Result::ok)
    {
        if e.path().extension().is_some_and(|x| x == ext) {
            out.push(e.path().to_path_buf());
        }
    }
    out.sort();
    Ok(out)
}

pub fn enumerate(tier: &str, work_dir: &Path, tools: &Path) -> Result<Vec<StagedOnt>> {
    let robot = tools.join("bin/robot");
    let mut staged = Vec::new();
    for src in tier_sources(tier, work_dir)? {
        let stem = src
            .file_stem()
            .with_context(|| format!("no file stem for {}", src.display()))?
            .to_string_lossy()
            .to_string();
        let dir = src
            .parent()
            .with_context(|| format!("no parent dir for {}", src.display()))?;
        let owx = dir.join(format!("{stem}.owx"));
        let ofn = dir.join(format!("{stem}.ofn"));
        let owl = if src.extension().is_some_and(|x| x == "owl") {
            src.clone()
        } else {
            dir.join(format!("{stem}.owl"))
        };
        // Normalize to the formats the reasoners need. Record convert errors as
        // an empty meta with fragment="convert-error" so the caller marks cells.
        let converted = (|| -> Result<()> {
            ensure_convert(&robot, &src, &owx)?;
            ensure_convert(&robot, &src, &ofn)?;
            Ok(())
        })();
        let bytes = std::fs::read(&src).with_context(|| format!("read {}", src.display()))?;
        let sha256 = sha256_hex(&bytes);
        let (classes, fragment) = if converted.is_err() {
            (0, "convert-error".to_string())
        } else {
            let count =
                owl_dl_reasoner::classify(&load_any(&ofn)?).map_or(0, |c| c.classes().len());
            (count, fragment_of(&ofn))
        };
        staged.push(StagedOnt {
            meta: OntMeta {
                name: stem,
                source: src.to_string_lossy().into_owned(),
                sha256,
                size_bytes: bytes.len() as u64,
                classes,
                fragment,
            },
            ofn,
            owl,
            owx,
        });
    }
    Ok(staged)
}

fn load_any(
    ofn: &Path,
) -> Result<horned_owl::ontology::set::SetOntology<horned_owl::model::RcStr>> {
    use horned_owl::io::ParserConfiguration;
    use horned_owl::io::ofn::reader::read as read_ofn;
    let src = std::fs::read_to_string(ofn)?;
    let mut cur = std::io::Cursor::new(src);
    let (onto, _) = read_ofn(&mut cur, ParserConfiguration::default())
        .map_err(|e| anyhow::anyhow!("parse {}: {e}", ofn.display()))?;
    Ok(onto)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn sha256_is_stable_and_hex() {
        let h = sha256_hex(b"hello");
        assert_eq!(
            h,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn el_flag_matches_fragment() {
        assert!(is_el_fragment("EL"));
        assert!(is_el_fragment("EL+"));
        assert!(!is_el_fragment("SHOIN(D)"));
        assert!(!is_el_fragment("DL"));
    }
}
