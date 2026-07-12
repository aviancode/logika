//! Workflow runtime for `logika`.

#![forbid(unsafe_code)]

mod context;
mod run;

pub use context::NodeContext;
pub use run::{Run, RunMetadata, RunOptions, RunOutcome, RunStatus, RunTransitionError};
