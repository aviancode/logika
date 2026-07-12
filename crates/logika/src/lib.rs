//! Stable facade for embedding the `logika` workflow engine.
//!
//! Version 0.1 exposes workflow modeling and validation as opt-in features.
//! Asynchronous execution and plugin hosting are planned for later releases.

#![forbid(unsafe_code)]

#[cfg(feature = "core")]
pub use logika_core as core;
#[cfg(feature = "registry")]
pub use logika_registry as registry;
#[cfg(feature = "sdk")]
pub use logika_sdk as sdk;
#[cfg(feature = "store")]
pub use logika_store as store;
#[cfg(feature = "workflow")]
pub use logika_workflow as workflow;
