//! Stable facade for embedding the `orbita` workflow engine.
//!
//! Version 0.1 exposes workflow modeling and validation as opt-in features.
//! Asynchronous execution and plugin hosting are planned for later releases.

#![forbid(unsafe_code)]

#[cfg(feature = "core")]
pub use orbita_core as core;
#[cfg(feature = "registry")]
pub use orbita_registry as registry;
#[cfg(feature = "sdk")]
pub use orbita_sdk as sdk;
#[cfg(feature = "store")]
pub use orbita_store as store;
#[cfg(feature = "workflow")]
pub use orbita_workflow as workflow;
