//! Consequence-based saturation engine for the EL fragment.
//!
//! Algorithm follows Kazakov, Krötzsch, Simančík (JAR 2014) "The Incredible
//! ELK". The Rust crate `whelk-rs` is the working reference implementation;
//! we re-implement against our own IR (see `owl-dl-core`) to avoid IR-boundary
//! copies in the hot loop.
//!
//! Phase 6 work — currently empty.
