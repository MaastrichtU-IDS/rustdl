//! `rustdl` — a sound OWL 2 DL (SROIQ) reasoner in Rust.
//!
//! This is the **umbrella entry point**. It re-exports [`owl_dl_reasoner`]'s full
//! public API *and* the exact [`horned_owl`] version that API is built against, so
//! one dependency is enough:
//!
//! ```sh
//! cargo add rustdl
//! ```
//!
//! ```no_run
//! use rustdl::horned_owl::io::ParserConfiguration;
//! use rustdl::horned_owl::io::ofn::reader::read as read_ofn;
//! use rustdl::horned_owl::model::RcStr;
//! use rustdl::horned_owl::ontology::set::SetOntology;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let src = std::fs::read_to_string("ontology.ofn")?;
//! let (onto, _): (SetOntology<RcStr>, _) =
//!     read_ofn(&mut std::io::Cursor::new(src), ParserConfiguration::default())?;
//!
//! let h = rustdl::classify(&onto)?;
//! println!("{} classes", h.classes().len());
//! # Ok(())
//! # }
//! ```
//!
//! # Why this crate exists rather than depending on `owl-dl-reasoner` directly
//!
//! `owl-dl-reasoner` takes `horned-owl` types in its signatures but does not
//! re-export the crate, so a caller had to add `horned-owl` themselves — and
//! `cargo add horned-owl` resolves the newest MAJOR (3.x at time of writing) while
//! `owl-dl-reasoner` requires `^1.4`. Cargo installs both happily, and the
//! `SetOntology<RcStr>` built with 3.x is then a *different type* from the one
//! [`classify`] accepts, so the natural first program fails with a type mismatch
//! whose cause is invisible from the error. Going through [`horned_owl`] here makes
//! that mismatch unconstructible.
//!
//! # This crate has no binary
//!
//! `cargo install rustdl` will not give you the `rustdl` command-line tool — this
//! is a library. The CLI lives in `owl-dl-cli`, which is deliberately NOT published
//! (it needs a Manchester-syntax reader that is not yet in upstream `horned-owl`,
//! so a crates.io build of it would not compile). Get the `rustdl` binary from the
//! [GitHub releases](https://github.com/MaastrichtU-IDS/rustdl/releases/latest),
//! which carry prebuilt Linux (x86-64/aarch64, musl-static), macOS (aarch64) and
//! Windows (x86-64) binaries. When that upstream reader lands, a `[[bin]]` can be
//! added here and `cargo install rustdl` will start working.
//!
//! # Soundness
//!
//! Every reported subsumption holds (FP=0 against Konclude across the curated
//! corpus). Completeness is the subtle part and is documented per-fragment on
//! [`owl_dl_reasoner`] and in the repository's `CLAUDE.md`; the default classifier
//! is a sound, near-complete approximation rather than a guaranteed-complete one.

// The whole point of this crate: callers get the reasoner API and the exact
// horned-owl it was compiled against, from one dependency.
pub use horned_owl;
pub use owl_dl_reasoner;

#[doc(inline)]
pub use owl_dl_reasoner::*;
