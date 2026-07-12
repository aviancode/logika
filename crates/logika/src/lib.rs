//! Stable facade for embedding the `logika` workflow engine.
//!
//! Workflow modeling, validation, and bounded DAG scheduling are available as
//! opt-in features. Plugin hosting is added by later release stages.

#![forbid(unsafe_code)]

#[cfg(feature = "core")]
pub use logika_core as core;
#[cfg(feature = "registry")]
pub use logika_registry as registry;
#[cfg(feature = "runtime")]
pub use logika_runtime as runtime;
#[cfg(feature = "sdk")]
pub use logika_sdk as sdk;
#[cfg(feature = "store")]
pub use logika_store as store;
#[cfg(feature = "workflow")]
pub use logika_workflow as workflow;
