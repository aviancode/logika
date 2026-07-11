//! Tests for typed workflow construction.

#![allow(clippy::expect_used)]

use orbita_sdk::{Node, Schema, Version, WorkflowBuilder};
use orbita_workflow::Endpoint;

#[allow(dead_code)]
#[derive(Schema)]
#[schema(name = "acme.order", version = 1)]
struct Order {
    id: u64,
}

#[allow(dead_code)]
#[derive(Schema)]
#[schema(name = "acme.validated-order", version = 1)]
struct ValidatedOrder {
    id: u64,
}

struct ValidateOrder;

impl Node for ValidateOrder {
    type Input = Order;
    type Output = ValidatedOrder;

    const NAME: &'static str = "acme.validation/validate-order";
    const VERSION: &'static str = "1.2.3";
}

struct SaveOrder;

impl Node for SaveOrder {
    type Input = ValidatedOrder;
    type Output = ValidatedOrder;

    const NAME: &'static str = "acme.storage/save-order";
    const VERSION: &'static str = "2.0.0";
}

#[test]
fn builds_portable_document_from_typed_connections() {
    let mut builder = WorkflowBuilder::new("typed-order", Version::new(1, 0, 0));
    let order = builder
        .input::<Order>("order")
        .expect("workflow input should be valid");
    let validate = builder
        .node::<ValidateOrder>("validate")
        .expect("node metadata should be valid");
    let save = builder
        .node::<SaveOrder>("save")
        .expect("node metadata should be valid");

    builder.connect(order, validate.input());
    builder.connect(validate.output(), save.input());
    builder
        .output("saved", save.output())
        .expect("workflow output should be valid");

    let document = builder.build();
    assert_eq!(document.metadata().name(), "typed-order");
    assert_eq!(
        document
            .spec()
            .inputs()
            .iter()
            .find(|(port, _)| port.as_str() == "order")
            .map(|(_, reference)| reference.to_string())
            .as_deref(),
        Some("acme.order@1")
    );
    assert_eq!(document.spec().nodes().len(), 2);
    assert_eq!(
        document.spec().nodes()[0].uses().to_string(),
        "acme.validation/validate-order@=1.2.3"
    );
    assert_eq!(document.spec().edges().len(), 2);
    assert!(matches!(
        document.spec().edges()[0].from(),
        Endpoint::WorkflowInput(port) if port.as_str() == "order"
    ));
    let saved = document
        .spec()
        .outputs()
        .iter()
        .find(|(port, _)| port.as_str() == "saved")
        .map(|(_, endpoint)| endpoint)
        .expect("saved output should exist");
    assert!(matches!(
        saved,
        Endpoint::NodePort { node, port }
            if node.as_str() == "save" && port.as_str() == "output"
    ));
}

#[test]
fn rejects_duplicate_typed_graph_members() {
    let mut builder = WorkflowBuilder::new("duplicates", Version::new(1, 0, 0));
    builder
        .input::<Order>("order")
        .expect("first workflow input should be accepted");
    assert_eq!(
        builder
            .input::<Order>("order")
            .expect_err("duplicate workflow input should fail")
            .to_string(),
        "workflow input order is already declared"
    );

    builder
        .node::<ValidateOrder>("validate")
        .expect("first node should be accepted");
    assert_eq!(
        builder
            .node::<ValidateOrder>("validate")
            .expect_err("duplicate node should fail")
            .to_string(),
        "node validate is already declared"
    );
}
