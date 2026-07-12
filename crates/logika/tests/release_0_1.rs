//! Acceptance scenarios for the public `logika` 0.1 API.

#![allow(clippy::expect_used)]

use std::collections::BTreeMap;

use logika::{
    core::{NodeId, PluginId, PortId, PrimitiveType, SchemaDefinition, TypeRef},
    registry::{NodeDescriptor, NodeRegistry},
    workflow::{
        InputPort, NodeInterface, ValidationErrorKind, decode_json, decode_yaml, validate_workflow,
    },
};
use semver::Version;

#[test]
fn validates_a_yaml_workflow_through_the_public_facade() {
    let workflow = decode_yaml(include_str!("fixtures/0.1/valid-workflow.yaml"))
        .expect("release YAML fixture should decode");
    let registry = release_registry();

    validate_workflow(workflow.document(), &registry)
        .expect("release YAML fixture should validate before execution");
}

#[test]
fn rejects_an_incompatible_json_edge_with_actionable_types() {
    let workflow = decode_json(include_str!("fixtures/0.1/incompatible-workflow.json"))
        .expect("release JSON fixture should decode");
    let registry = release_registry();

    let errors = validate_workflow(workflow.document(), &registry)
        .expect_err("an incompatible edge must be rejected before execution");
    let mismatch = errors
        .diagnostics()
        .iter()
        .find(|error| error.kind() == ValidationErrorKind::IncompatibleTypes)
        .expect("the validator should emit an incompatible-types diagnostic");

    assert_eq!(mismatch.code(), "workflow.incompatible_types");
    assert_eq!(mismatch.node().map(NodeId::as_str), Some("load-customer"));
    assert_eq!(mismatch.port().map(PortId::as_str), Some("customer"));
    assert_eq!(
        mismatch.source_type().map(|type_ref| type_ref.name()),
        Some("acme.order")
    );
    assert_eq!(
        mismatch.target_type().map(|type_ref| type_ref.name()),
        Some("acme.customer")
    );
    assert!(mismatch.message().contains("acme.order@1"));
    assert!(mismatch.message().contains("acme.customer@1"));
}

fn release_registry() -> NodeRegistry {
    let order = type_ref("acme.order");
    let customer = type_ref("acme.customer");
    let mut registry = NodeRegistry::new();

    registry
        .register_local(descriptor(
            "acme.validation/validate-order",
            "1.2.0",
            [("order", InputPort::required(order.clone()))],
            [("validated", order)],
        ))
        .expect("validation node should register");
    registry
        .register_local(descriptor(
            "acme.customer/load",
            "1.0.0",
            [("customer", InputPort::required(customer.clone()))],
            [("loaded", customer)],
        ))
        .expect("customer node should register");

    registry
}

fn descriptor<const I: usize, const O: usize>(
    name: &str,
    version: &str,
    inputs: [(&str, InputPort); I],
    outputs: [(&str, TypeRef); O],
) -> NodeDescriptor {
    NodeDescriptor::new(
        PluginId::new(name).expect("fixture node name should be valid"),
        Version::parse(version).expect("fixture node version should be valid"),
        NodeInterface::new(
            inputs
                .into_iter()
                .map(|(name, input)| (port(name), input))
                .collect::<BTreeMap<_, _>>(),
            outputs
                .into_iter()
                .map(|(name, output)| (port(name), output))
                .collect::<BTreeMap<_, _>>(),
        ),
    )
}

fn type_ref(name: &str) -> TypeRef {
    TypeRef::new(name, 1, SchemaDefinition::primitive(PrimitiveType::String))
        .expect("fixture type should be valid")
}

fn port(name: &str) -> PortId {
    PortId::new(name).expect("fixture port should be valid")
}
