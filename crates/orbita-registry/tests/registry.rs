//! Public contract tests for the in-memory local node registry.

use std::collections::BTreeMap;

use orbita_core::{PluginId, PortId, PrimitiveType, SchemaDefinition, TypeRef};
use orbita_registry::{NodeDescriptor, NodeRegistry};
use orbita_workflow::{InputPort, NodeInterface, NodeReference};
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

fn port_id(value: &str) -> PortId {
    let parsed = PortId::new(value);
    let Ok(parsed) = parsed else {
        panic!("invalid test port name: {value}");
    };
    parsed
}
