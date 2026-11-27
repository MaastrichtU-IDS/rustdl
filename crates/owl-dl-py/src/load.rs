//! Parse OWL ontologies from a file path or bytes, mapping
//! horned-owl parse errors to `ParseError`.

use horned_owl::curie::PrefixMapping;
use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::io::omn::reader::read as read_omn;
use horned_owl::io::owx::reader::read as read_owx;
use horned_owl::io::rdf::reader::{ConcreteRcRDFOntology, read as read_rdf};
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

/// #597: what a query's conversion dropped, plus #543's two universal-role flags
/// (class level, role level), as the Python wrappers warn them.
pub(crate) type ConversionNotes = (std::collections::BTreeMap<String, u64>, bool, bool);

thread_local! {
    /// #597: the notes of the last noted load on this thread. A Python wrapper
    /// clears it, calls the native query, and takes it, so it never parses the file
    /// a second time. A thread-local, not a return value, so the 26 natives keep
    /// their signatures; the Python call and its native run on one thread.
    static LAST_NOTES: std::cell::RefCell<Option<ConversionNotes>> =
        const { std::cell::RefCell::new(None) };
}

/// #597: record `notes` for the Python wrapper to take.
pub(crate) fn set_notes(notes: ConversionNotes) {
    LAST_NOTES.with(|c| *c.borrow_mut() = Some(notes));
}

/// #597: the notes of `ontology`'s conversion, from the already-parsed ontology
/// (the parse, which dominates, is not repeated). A conversion that fails records
/// nothing; the query itself then fails on the same conversion.
pub(crate) fn note_conversion(ontology: &SetOntology<RcStr>) {
    if let Ok((dropped, uro, uro_for_roles)) = owl_dl_reasoner::conversion_notes(ontology) {
        set_notes((dropped.by_kind().clone(), uro, uro_for_roles));
    }
}

/// [`load_path`], recording the conversion's notes for the Python wrapper (#597).
pub(crate) fn load_path_noted(path: &str) -> PyResult<SetOntology<RcStr>> {
    let ontology = load_path(path)?;
    note_conversion(&ontology);
    Ok(ontology)
}

/// `rustdl._take_conversion_notes()`: the notes the last noted native query on
/// this thread recorded, cleared on read; `None` if there are none (#597).
#[pyo3::pyfunction]
#[pyo3(name = "_take_conversion_notes")]
pub(crate) fn take_conversion_notes() -> Option<ConversionNotes> {
    LAST_NOTES.with(|c| c.borrow_mut().take())
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
        "rdf-xml" | "rdf" => read_rdf(&mut reader, cfg.into())
            .map(|(o, _): (ConcreteRcRDFOntology, _)| (o.into(), PrefixMapping::default())),
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
        "rdf-xml" | "rdf" => {
            read_rdf(&mut reader, cfg.into()).map(|(o, _): (ConcreteRcRDFOntology, _)| o.into())
        }
        "omn" | "manchester" => read_omn(&mut reader, cfg).map(|(o, _): (SetOntology<RcStr>, _)| o),
        other => {
            return Err(ParseError::new_err(format!(
                "unknown format `{other}` — expected one of: ofn, owx, rdf-xml, omn"
            )));
        }
    };
    result.map_err(|e| ParseError::new_err(format!("parse {format}: {e}")))
}
