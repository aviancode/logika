//! Persistence contracts and in-memory adapters for `logika`.
//!
//! Release 0.1 deliberately exposes only storage for versioned workflow
//! definitions. Runtime state, checkpoints, and plugin artifacts get their
//! own versioned contracts when those domain models are introduced.

#![forbid(unsafe_code)]

use std::{
    collections::{BTreeMap, btree_map::Entry},
    sync::RwLock,
};

use logika_core::{Error, ErrorCategory, ErrorDetail, Result};
use logika_workflow::WorkflowDocument;
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

/// Thread-safe in-memory storage for workflow definitions.
///
/// This adapter is intended for embedded applications, local tools, and
/// tests that do not require persistence across process restarts. It performs
/// no I/O and has no database dependency.
#[derive(Debug, Default)]
pub struct InMemoryWorkflowStore {
    workflows: RwLock<BTreeMap<(String, Version), WorkflowDocument>>,
}

impl InMemoryWorkflowStore {
    /// Creates an empty in-memory store.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            workflows: RwLock::new(BTreeMap::new()),
        }
    }

    /// Returns the number of stored workflow versions.
    pub fn len(&self) -> Result<usize> {
        self.workflows
            .read()
            .map(|workflows| workflows.len())
            .map_err(|_| lock_error())
    }

    /// Returns whether the store contains no workflow versions.
    pub fn is_empty(&self) -> Result<bool> {
        self.len().map(|len| len == 0)
    }
}

impl WorkflowStore for InMemoryWorkflowStore {
    fn insert(&self, workflow: WorkflowDocument) -> Result<()> {
        let name = workflow.metadata().name().to_owned();
        let version = workflow.metadata().version().clone();
        let mut workflows = self.workflows.write().map_err(|_| lock_error())?;

        match workflows.entry((name.clone(), version.clone())) {
            Entry::Vacant(entry) => {
                entry.insert(workflow);
                Ok(())
            }
            Entry::Occupied(entry) if entry.get() == &workflow => Ok(()),
            Entry::Occupied(_) => Err(storage_error(
                "store.workflow_conflict",
                format!("workflow {name}@{version} already exists with different contents"),
            )),
        }
    }

    fn get(&self, name: &str, version: &Version) -> Result<Option<WorkflowDocument>> {
        self.workflows
            .read()
            .map(|workflows| workflows.get(&(name.to_owned(), version.clone())).cloned())
            .map_err(|_| lock_error())
    }

    fn versions(&self, name: &str) -> Result<Vec<Version>> {
        self.workflows
            .read()
            .map(|workflows| {
                workflows
                    .keys()
                    .filter(|(stored_name, _)| stored_name == name)
                    .map(|(_, version)| version.clone())
                    .collect()
            })
            .map_err(|_| lock_error())
    }
}

fn lock_error() -> Error {
    storage_error(
        "store.lock_poisoned",
        "in-memory workflow store lock is poisoned".to_owned(),
    )
}

fn storage_error(code: &'static str, message: String) -> Error {
    Error::new(ErrorCategory::Storage, ErrorDetail::new(code, message))
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
