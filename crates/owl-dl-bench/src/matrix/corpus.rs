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
    // nominals, inverse roles — on both the object and data sides. Note
    // `DataSomeValuesFrom`/`DataHasValue` are EL-legal and deliberately absent.
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
        "DataAllValuesFrom",
        "DataMinCardinality",
        "DataMaxCardinality",
        "DataExactCardinality",
        "DataComplementOf",
        "DataUnionOf",
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
        // ORE-2015 pilot: the input ontologies are `.ofn` under `.../ore-run/input`
        // (the repo root also holds a 1920-file raw `pool_sample/` and `owx/`+`oracle/`
        // sibling dirs — globbing `.owl` at the root wrongly grabs those). Point at the
        // clean `.ofn` input set; oracles are (re)generated per-ont by the matrix.
        "ore" => (
            PathBuf::from(
                std::env::var("RUSTDL_ORE_DIR").unwrap_or(format!("{home}/data/ore-run/input")),
            ),
            "ofn",
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
    // `enumerate` returns Err only for a whole-run setup failure (bad tier /
    // missing root dir); a per-ont read/convert/parse failure must NOT abort
    // the tier — it yields an error row and we move on.
    for src in tier_sources(tier, work_dir)? {
        // Path derivations use fallbacks so a pathological filename never
        // propagates out of the loop.
        let stem = src.file_stem().map_or_else(
            || src.to_string_lossy().into_owned(),
            |s| s.to_string_lossy().into_owned(),
        );
        let dir = src.parent().unwrap_or_else(|| Path::new("."));
        let owx = dir.join(format!("{stem}.owx"));
        let ofn = dir.join(format!("{stem}.ofn"));
        let owl = if src.extension().is_some_and(|x| x == "owl") {
            src.clone()
        } else {
            dir.join(format!("{stem}.owl"))
        };
        // Normalize to the formats the reasoners need: `.owx` (Konclude),
        // `.ofn` (whelk), and `.owl` (HermiT/ELK global constraint). When the
        // source is already `.owl`, `ensure_convert` for it is a cached no-op.
        let converted = (|| -> Result<()> {
            ensure_convert(&robot, &src, &owx)?;
            ensure_convert(&robot, &src, &ofn)?;
            ensure_convert(&robot, &src, &owl)?;
            Ok(())
        })();
        // Read/hash the source, then (if conversion succeeded) load the `.ofn`
        // for the class count + fragment. Any failure at any step records an
        // error row (fragment="convert-error", classes=0, best-effort sha/size)
        // and continues rather than `?`-ing out of the whole enumeration.
        let (sha256, size_bytes, classes, fragment) = match std::fs::read(&src) {
            Err(e) => {
                eprintln!("owl-dl-bench: skipping {}: read failed: {e}", src.display());
                (String::new(), 0, 0, "convert-error".to_string())
            }
            Ok(bytes) => {
                let sha256 = sha256_hex(&bytes);
                let size_bytes = bytes.len() as u64;
                if let Err(e) = &converted {
                    eprintln!("owl-dl-bench: convert error for {}: {e}", src.display());
                    (sha256, size_bytes, 0, "convert-error".to_string())
                } else {
                    match corpus_load_ofn(&ofn) {
                        Ok(onto) => {
                            // Syntactic class count only — a full `classify()`
                            // here is unbounded and can hang forever on a hard
                            // ontology (see `matrix::profile`).
                            let count = crate::matrix::profile::profile(&onto).classes;
                            (sha256, size_bytes, count, fragment_of(&ofn))
                        }
                        Err(e) => {
                            eprintln!("owl-dl-bench: parse error for {}: {e}", src.display());
                            (sha256, size_bytes, 0, "convert-error".to_string())
                        }
                    }
                }
            }
        };
        staged.push(StagedOnt {
            meta: OntMeta {
                name: stem,
                source: src.to_string_lossy().into_owned(),
                sha256,
                size_bytes,
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

pub fn corpus_load_ofn(
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

    #[test]
    fn data_cardinality_source_is_non_el() {
        // A source whose only non-EL construct is a data-property cardinality
        // must still be labeled "DL" — otherwise ELK/whelk would be run on an
        // out-of-fragment ont and reported as valid EL numbers.
        let dir = tempdir_here("frag_data_card");
        let path = dir.join("data_card.ofn");
        std::fs::write(
            &path,
            "SubClassOf(:C DataMaxCardinality(1 :hasAge xsd:integer))\n",
        )
        .unwrap();
        assert_eq!(fragment_of(&path), "DL");
    }

    #[test]
    fn pure_el_source_is_el() {
        // Only EL-legal constructs (incl. the EL-legal DataSomeValuesFrom) must
        // stay "EL" — a control for the data-side additions to NON_EL.
        let dir = tempdir_here("frag_pure_el");
        let path = dir.join("pure_el.ofn");
        std::fs::write(
            &path,
            "SubClassOf(:C ObjectSomeValuesFrom(:r :D))\n\
             SubClassOf(:C DataSomeValuesFrom(:hasAge xsd:integer))\n",
        )
        .unwrap();
        assert_eq!(fragment_of(&path), "EL");
    }

    // Minimal deterministic scratch dir (no external tempdir crate); mirrors the
    // helper used in `matrix::model`'s tests.
    fn tempdir_here(tag: &str) -> std::path::PathBuf {
        let base = std::env::temp_dir().join(format!("rustdl-corpus-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        base
    }
}
