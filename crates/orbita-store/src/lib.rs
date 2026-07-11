//! Persistence contracts and in-memory adapters for `orbita`.
//!
//! Release 0.1 deliberately exposes only storage for versioned workflow
//! definitions. Runtime state, checkpoints, and plugin artifacts get their
//! own versioned contracts when those domain models are introduced.

#![forbid(unsafe_code)]

use orbita_core::Result;
use orbita_workflow::WorkflowDocument;
use semver::Version;

/// Storage for immutable, versioned workflow definitions.
///
/// Implementations must treat a repeated insertion of the same document as
/// idempotent and reject a different document with the same name and version.
/// Returned version lists must be sorted in ascending semantic-version order.
///
/// The contract is object-safe so embedding applications can provide an
/// adapter without exposing its concrete storage implementation.
pub trait WorkflowStore: Send + Sync {
    /// Stores a workflow definition.
    ///
    /// Inserting the same definition more than once succeeds. Reusing its
    /// name and version for different contents returns a storage error.
    fn insert(&self, workflow: WorkflowDocument) -> Result<()>;

    /// Loads one exact workflow version.
    fn get(&self, name: &str, version: &Version) -> Result<Option<WorkflowDocument>>;

    /// Lists all stored versions of a workflow in ascending order.
    fn versions(&self, name: &str) -> Result<Vec<Version>>;
}

#[cfg(test)]
mod tests {
    use super::WorkflowStore;

    #[test]
    fn workflow_store_contract_is_object_safe() {
        fn accept_store(_: &dyn WorkflowStore) {}

        let _: fn(&dyn WorkflowStore) = accept_store;
    }
}
