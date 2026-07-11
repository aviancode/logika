//! Public behavior tests for the in-memory workflow store.

use std::{collections::BTreeMap, sync::Arc, thread};

use orbita_core::{ErrorCategory, PortId};
use orbita_store::{InMemoryWorkflowStore, WorkflowStore};
use orbita_workflow::{Endpoint, WorkflowDocument, WorkflowMetadata, WorkflowSpec};
use semver::Version;

#[test]
fn stores_exact_versions_and_lists_them_deterministically() {
    let store: Box<dyn WorkflowStore> = Box::new(InMemoryWorkflowStore::new());
    let v2 = workflow("orders", Version::new(2, 0, 0));
    let v1 = workflow("orders", Version::new(1, 4, 0));

    assert!(store.insert(v2.clone()).is_ok());
    assert!(store.insert(v1.clone()).is_ok());
    assert!(store.insert(v1.clone()).is_ok());

    let loaded = store.get("orders", &Version::new(1, 4, 0));
    let Ok(Some(loaded)) = loaded else {
        panic!("stored workflow version was not returned");
    };
    assert_eq!(loaded, v1);

    let versions = store.versions("orders");
    let Ok(versions) = versions else {
        panic!("stored workflow versions could not be listed");
    };
    assert_eq!(versions, [Version::new(1, 4, 0), Version::new(2, 0, 0)]);

    let missing = store.get("orders", &Version::new(3, 0, 0));
    assert!(matches!(missing, Ok(None)));
}

#[test]
fn rejects_replacing_an_existing_version_with_different_contents() {
    let store = InMemoryWorkflowStore::new();
    let original = workflow("orders", Version::new(1, 0, 0));
    assert!(store.insert(original.clone()).is_ok());

    let conflicting = WorkflowDocument::new(
        original.metadata().clone(),
        WorkflowSpec::new(
            BTreeMap::new(),
            Vec::new(),
            Vec::new(),
            BTreeMap::from([(
                port_id("result"),
                Endpoint::workflow_input(port_id("source")),
            )]),
        ),
    );
    let result = store.insert(conflicting);
    let Err(error) = result else {
        panic!("conflicting workflow definition was accepted");
    };

    assert_eq!(error.category(), ErrorCategory::Storage);
    assert_eq!(error.code(), "store.workflow_conflict");
    assert!(error.message().contains("orders@1.0.0"));
    assert!(matches!(store.len(), Ok(1)));

    let loaded = store.get("orders", &Version::new(1, 0, 0));
    assert!(matches!(loaded, Ok(Some(document)) if document == original));
}

#[test]
fn accepts_concurrent_writers_without_losing_versions() {
    let store = Arc::new(InMemoryWorkflowStore::new());
    let handles = (0..16)
        .map(|patch| {
            let store = Arc::clone(&store);
            thread::spawn(move || store.insert(workflow("orders", Version::new(1, 0, patch))))
        })
        .collect::<Vec<_>>();

    for handle in handles {
        let joined = handle.join();
        let Ok(inserted) = joined else {
            panic!("store writer thread panicked");
        };
        assert!(inserted.is_ok());
    }

    assert!(matches!(store.len(), Ok(16)));
    assert!(matches!(store.is_empty(), Ok(false)));
    let versions = store.versions("orders");
    let Ok(versions) = versions else {
        panic!("concurrently stored versions could not be listed");
    };
    assert_eq!(versions.first(), Some(&Version::new(1, 0, 0)));
    assert_eq!(versions.last(), Some(&Version::new(1, 0, 15)));
}

fn workflow(name: &str, version: Version) -> WorkflowDocument {
    WorkflowDocument::new(
        WorkflowMetadata::new(name, version),
        WorkflowSpec::default(),
    )
}

fn port_id(value: &str) -> PortId {
    let parsed = PortId::new(value);
    let Ok(parsed) = parsed else {
        panic!("invalid test port name: {value}");
    };
    parsed
}
