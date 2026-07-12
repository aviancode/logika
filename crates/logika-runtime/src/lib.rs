//! Workflow runtime for `logika`.

#![forbid(unsafe_code)]

mod context;
mod run;
mod scheduler;

pub use context::NodeContext;
pub use run::{Run, RunMetadata, RunOptions, RunOutcome, RunStatus, RunTransitionError};
pub use scheduler::{
    ExecutionError, ExecutionResult, NodeExecution, NodeExecutor, NodeFuture, NodeInputs,
    NodeOutputs, Scheduler, SchedulerConfig, WorkflowInputs, WorkflowOutputs,
};
