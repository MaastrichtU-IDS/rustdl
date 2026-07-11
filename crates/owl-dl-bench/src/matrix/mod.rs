// Harness types/functions are consumed across the matrix submodules and by the
// `matrix` subcommand wired in the final task; allow the interim unused/visibility
// lints while the module tree is being built out task by task.
#![allow(dead_code, unreachable_pub)]

pub mod corpus;
pub use corpus::corpus_load_ofn;
pub mod correctness;
pub mod model;
pub mod provenance;
pub mod run;
