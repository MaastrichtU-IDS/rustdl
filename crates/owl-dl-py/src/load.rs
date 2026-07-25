//! Parse OWL ontologies from a file path or bytes, mapping
//! horned-owl parse errors to `ParseError`.

use horned_owl::curie::PrefixMapping;
use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::io::omn::reader::read as read_omn;
use horned_owl::io::owx::reader::read as read_owx;
use horned_owl::io::rdf::reader::read as read_rdf;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use pyo3::PyResult;
use std::io::Cursor;
use std::path::Path;

use crate::errors::ParseError;

/// Detect the parse format from a file path's extension
/// (`.ofn` | `.owx` | `.rdf` | `.owl` | `.omn`).
fn format_from_path(path: &str) -> PyResult<&'static str> {
    let ext = Path::new(path)
        .extension()
        .and_then(|s| s.to_str())
        .map(str::to_ascii_lowercase);
    match ext.as_deref() {
        Some("ofn") => Ok("ofn"),
        Some("owx") => Ok("owx"),
        Some("rdf" | "owl") => Ok("rdf-xml"),
        Some("omn") => Ok("omn"),
        Some(other) => Err(ParseError::new_err(format!(
            "unknown extension `.{other}` — pass format= explicitly via classify_bytes() or rename file to .ofn / .owx / .rdf / .omn"
        ))),
        None => Err(ParseError::new_err(format!(
            "no extension on `{path}` — pass format= explicitly via classify_bytes()"
        ))),
    }
}

/// Parse an ontology from a file path. Format is auto-detected from
/// the file extension (`.ofn` | `.owx` | `.rdf` | `.owl` | `.omn`).
pub(crate) fn load_path(path: &str) -> PyResult<SetOntology<RcStr>> {
    let src = std::fs::read_to_string(path)
        .map_err(|e| ParseError::new_err(format!("read {path}: {e}")))?;
    let format = format_from_path(path)?;
    parse_with_format(&src, format)
}

/// Like [`load_path`], but also returns the [`PrefixMapping`] the OFN/OWX/OMN
/// reader collected — used to resolve Manchester class-expression strings
/// (e.g. `:A`) against the ontology's own prefixes. The RDF/XML reader does
/// not expose a prefix map, so that path returns `PrefixMapping::default()`.
pub(crate) fn load_path_with_pm(path: &str) -> PyResult<(SetOntology<RcStr>, PrefixMapping)> {
    let src = std::fs::read_to_string(path)
        .map_err(|e| ParseError::new_err(format!("read {path}: {e}")))?;
    let format = format_from_path(path)?;
    parse_with_format_pm(&src, format)
}

/// Parse from bytes with an explicit format string.
pub(crate) fn load_bytes(data: &[u8], format: &str) -> PyResult<SetOntology<RcStr>> {
    let src = std::str::from_utf8(data)
        .map_err(|e| ParseError::new_err(format!("ontology bytes are not valid UTF-8: {e}")))?;
    parse_with_format(src, format)
}

fn parse_with_format_pm(src: &str, format: &str) -> PyResult<(SetOntology<RcStr>, PrefixMapping)> {
    let mut reader = Cursor::new(src);
    let cfg = ParserConfiguration::default();
    let result = match format {
        "ofn" => read_ofn(&mut reader, cfg).map(|(o, pm): (SetOntology<RcStr>, _)| (o, pm)),
        "owx" => read_owx(&mut reader, cfg).map(|(o, pm): (SetOntology<RcStr>, _)| (o, pm)),
        // rdf::reader::read exposes no PrefixMapping; use the default (empty) one.
        "rdf-xml" | "rdf" => {
            read_rdf(&mut reader, cfg).map(|(o, _)| (o.into(), PrefixMapping::default()))
        }
        "omn" | "manchester" => {
            read_omn(&mut reader, cfg).map(|(o, pm): (SetOntology<RcStr>, _)| (o, pm))
        }
        other => {
            return Err(ParseError::new_err(format!(
                "unknown format `{other}` — expected one of: ofn, owx, rdf-xml, omn"
            )));
        }
    };
    result.map_err(|e| ParseError::new_err(format!("parse {format}: {e}")))
}

fn parse_with_format(src: &str, format: &str) -> PyResult<SetOntology<RcStr>> {
    let mut reader = Cursor::new(src);
    let cfg = ParserConfiguration::default();
    let result = match format {
        "ofn" => read_ofn(&mut reader, cfg).map(|(o, _): (SetOntology<RcStr>, _)| o),
        "owx" => read_owx(&mut reader, cfg).map(|(o, _): (SetOntology<RcStr>, _)| o),
        // rdf::reader::read returns ConcreteRDFOntology; convert via the
        // From<ConcreteRDFOntology> impl that horned-owl provides for SetOntology.
        "rdf-xml" | "rdf" => read_rdf(&mut reader, cfg).map(|(o, _)| o.into()),
        "omn" | "manchester" => read_omn(&mut reader, cfg).map(|(o, _): (SetOntology<RcStr>, _)| o),
        other => {
            return Err(ParseError::new_err(format!(
                "unknown format `{other}` — expected one of: ofn, owx, rdf-xml, omn"
            )));
        }
    };
    result.map_err(|e| ParseError::new_err(format!("parse {format}: {e}")))
}
