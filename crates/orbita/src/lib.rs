//! Stable facade for embedding the `orbita` workflow engine.
//!
//! The facade intentionally contains no implementation.  Capabilities are
//! exposed as opt-in features while the underlying crates are developed.

#![forbid(unsafe_code)]

#[cfg(feature = "core")]
pub use orbita_core as core;
#[cfg(feature = "plugin-api")]
pub use orbita_plugin_api as plugin_api;
#[cfg(feature = "plugin-host")]
pub use orbita_plugin_host as plugin_host;
#[cfg(feature = "registry")]
pub use orbita_registry as registry;
#[cfg(feature = "runtime")]
pub use orbita_runtime as runtime;
#[cfg(feature = "sdk")]
pub use orbita_sdk as sdk;
#[cfg(feature = "store")]
pub use orbita_store as store;
#[cfg(feature = "testkit")]
pub use orbita_testkit as testkit;
#[cfg(feature = "workflow")]
pub use orbita_workflow as workflow;
