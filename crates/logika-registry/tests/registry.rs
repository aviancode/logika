//! Public contract tests for the in-memory local node registry.

use std::collections::BTreeMap;

use logika_core::{
    ErrorCategory, NodeId, PluginId, PortId, PrimitiveType, SchemaDefinition, TypeRef,
};
use logika_registry::{NodeDescriptor, NodeRegistry};
use logika_workflow::{
    EdgeDefinition, Endpoint, InputPort, NodeDefinition, NodeInterface, NodeReference,
    TypeReference, WorkflowDocument, WorkflowMetadata, WorkflowSpec, validate_workflow,
};
use semver::Version;

#[test]
fn resolves_the_highest_compatible_local_node_version() {
    let value = type_ref("acme.value", PrimitiveType::String);
    let mut registry = NodeRegistry::new();

    for version in [
        Version::new(2, 0, 0),
        Version::new(1, 2, 0),
        Version::new(1, 9, 0),
    ] {
        let result = registry.register_local(descriptor(version, value.clone()));
        assert!(result.is_ok());
    }

    let reference = node_reference("acme/echo@^1.0");
    let resolved = registry.resolve(&reference);
    let Ok(resolved) = resolved else {
        panic!("compatible local node was not resolved");
    };

    assert_eq!(resolved.name().as_str(), "acme/echo");
    assert_eq!(resolved.version(), &Version::new(1, 9, 0));
    assert_eq!(registry.len(), 3);
    assert_eq!(registry.descriptors().count(), 3);
}

#[test]
fn resolves_local_descriptors_for_workflow_validation() {
    let value = type_ref("acme.value", PrimitiveType::String);
    let mut registry = NodeRegistry::new();
    let registered = registry.register_local(descriptor(Version::new(1, 4, 0), value));
    assert!(registered.is_ok());

    let document = WorkflowDocument::new(
        WorkflowMetadata::new("local-echo", Version::new(1, 0, 0)),
        WorkflowSpec::new(
            BTreeMap::from([(port_id("value"), type_reference("acme.value", 1))]),
            vec![NodeDefinition::new(
                node_id("echo"),
                node_reference("acme/echo@^1"),
            )],
            vec![EdgeDefinition::new(
                Endpoint::workflow_input(port_id("value")),
                Endpoint::node_port(node_id("echo"), port_id("value")),
            )],
            BTreeMap::from([(
                port_id("value"),
                Endpoint::node_port(node_id("echo"), port_id("value")),
            )]),
        ),
    );

    assert!(validate_workflow(&document, &registry).is_ok());
}

#[test]
fn reports_unknown_nodes_and_unsatisfied_versions_separately() {
    let mut registry = NodeRegistry::new();
    let registered = registry.register_local(descriptor(
        Version::new(1, 3, 0),
        type_ref("acme.value", PrimitiveType::String),
    ));
    assert!(registered.is_ok());

    let missing = registry.resolve(&node_reference("acme/missing@^1"));
    let Err(missing) = missing else {
        panic!("unknown node unexpectedly resolved");
    };
    assert_eq!(missing.category(), ErrorCategory::Resolution);
    assert_eq!(missing.code(), "registry.node_not_found");
    assert!(missing.message().contains("acme/missing"));

    let incompatible = registry.resolve(&node_reference("acme/echo@^2"));
    let Err(incompatible) = incompatible else {
        panic!("incompatible node version unexpectedly resolved");
    };
    assert_eq!(incompatible.category(), ErrorCategory::Resolution);
    assert_eq!(incompatible.code(), "registry.no_matching_version");
    assert!(incompatible.message().contains("^2"));
    assert!(incompatible.message().contains("1.3.0"));
}

#[test]
fn rejects_duplicate_nodes_and_conflicting_schemas_without_mutation() {
    let value = type_ref("acme.value", PrimitiveType::String);
    let mut registry = NodeRegistry::new();
    let original = descriptor(Version::new(1, 0, 0), value.clone());
    assert!(registry.register_local(original.clone()).is_ok());

    let duplicate = registry.register_local(original);
    let Err(duplicate) = duplicate else {
        panic!("duplicate node version was accepted");
    };
    assert_eq!(duplicate.code(), "registry.duplicate_node");
    assert_eq!(registry.len(), 1);

    let conflicting = descriptor(
        Version::new(2, 0, 0),
        type_ref("acme.value", PrimitiveType::I64),
    );
    let conflict = registry.register_local(conflicting);
    let Err(conflict) = conflict else {
        panic!("conflicting canonical schema was accepted");
    };
    assert_eq!(conflict.category(), ErrorCategory::Schema);
    assert_eq!(conflict.code(), "registry.conflicting_schema");
    assert_eq!(registry.len(), 1);
    assert!(
        registry
            .get(&plugin_id("acme/echo"), &Version::new(2, 0, 0))
            .is_none()
    );
}

fn descriptor(version: Version, value: TypeRef) -> NodeDescriptor {
    NodeDescriptor::new(
        plugin_id("acme/echo"),
        version,
        NodeInterface::new(
            BTreeMap::from([(port_id("value"), InputPort::required(value.clone()))]),
            BTreeMap::from([(port_id("value"), value)]),
        ),
    )
}

fn node_reference(value: &str) -> NodeReference {
    let parsed = value.parse();
    let Ok(parsed) = parsed else {
        panic!("invalid test node reference: {value}");
    };
    parsed
}

fn type_reference(name: &str, version: u32) -> TypeReference {
    let parsed = TypeReference::new(name, version);
    let Ok(parsed) = parsed else {
        panic!("invalid test type reference: {name}@{version}");
    };
    parsed
}

fn type_ref(name: &str, primitive: PrimitiveType) -> TypeRef {
    let parsed = TypeRef::new(name, 1, SchemaDefinition::primitive(primitive));
    let Ok(parsed) = parsed else {
        panic!("invalid canonical test type: {name}");
    };
    parsed
}

fn plugin_id(value: &str) -> PluginId {
    let parsed = PluginId::new(value);
    let Ok(parsed) = parsed else {
        panic!("invalid test node name: {value}");
    };
    parsed
}

fn node_id(value: &str) -> NodeId {
    let parsed = NodeId::new(value);
    let Ok(parsed) = parsed else {
        panic!("invalid test node id: {value}");
    };
    parsed
}

fn port_id(value: &str) -> PortId {
    let parsed = PortId::new(value);
    let Ok(parsed) = parsed else {
        panic!("invalid test port name: {value}");
    };
    parsed
}
