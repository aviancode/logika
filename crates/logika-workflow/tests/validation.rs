//! Public contract tests for workflow graph validation.

use std::collections::BTreeMap;

use logika_core::{NodeId, PortId, PrimitiveType, SchemaDefinition, TypeRef};
use logika_workflow::{
    ConnectionMultiplicity, EdgeDefinition, Endpoint, InputPort, NodeDefinition, NodeInterface,
    NodeReference, TypeReference, ValidationErrorKind, ValidationResolver, WorkflowDocument,
    WorkflowMetadata, WorkflowSpec, validate_workflow,
};
use semver::Version;

#[derive(Default)]
struct TestResolver {
    nodes: BTreeMap<String, NodeInterface>,
    types: BTreeMap<(String, u32), TypeRef>,
}

impl TestResolver {
    fn with_type(mut self, type_ref: TypeRef) -> Self {
        self.types
            .insert((type_ref.name().to_owned(), type_ref.version()), type_ref);
        self
    }

    fn with_node(mut self, name: &str, interface: NodeInterface) -> Self {
        self.nodes.insert(name.to_owned(), interface);
        self
    }
}

impl ValidationResolver for TestResolver {
    fn resolve_node(&self, reference: &NodeReference) -> Option<&NodeInterface> {
        self.nodes.get(reference.plugin().as_str())
    }

    fn resolve_type(&self, reference: &TypeReference) -> Option<&TypeRef> {
        self.types
            .get(&(reference.name().to_owned(), reference.version()))
    }
}

#[test]
fn accepts_a_valid_typed_acyclic_graph() {
    let order = type_ref("acme.order", PrimitiveType::String);
    let resolver = TestResolver::default()
        .with_type(order.clone())
        .with_node(
            "acme/validate",
            interface(
                [("order", InputPort::required(order.clone()))],
                [("valid", order.clone())],
            ),
        )
        .with_node(
            "acme/archive",
            interface(
                [(
                    "orders",
                    InputPort::required(order.clone())
                        .with_multiplicity(ConnectionMultiplicity::Many),
                )],
                [("stored", order)],
            ),
        );
    let document = workflow(
        [("order", type_reference("acme.order", 1))],
        [
            node("validate", "acme/validate@^1"),
            node("archive", "acme/archive@^1"),
        ],
        [
            edge("$inputs.order", "validate.order"),
            edge("validate.valid", "archive.orders"),
        ],
        [("stored", "archive.stored")],
    );

    let resolver: &dyn ValidationResolver = &resolver;
    assert!(validate_workflow(&document, resolver).is_ok());
}

#[test]
fn reports_reference_required_input_and_multiplicity_failures_together() {
    let value = type_ref("acme.value", PrimitiveType::String);
    let resolver = TestResolver::default()
        .with_type(value.clone())
        .with_node(
            "acme/sink",
            interface([("in", InputPort::required(value.clone()))], []),
        )
        .with_node(
            "acme/missing",
            interface([("in", InputPort::required(value))], []),
        );
    let document = workflow(
        [
            ("left", type_reference("acme.value", 1)),
            ("right", type_reference("acme.value", 1)),
            ("unused", type_reference("acme.unknown", 1)),
        ],
        [
            node("sink", "acme/sink@1"),
            node("missing", "acme/missing@1"),
            node("unresolved", "acme/unknown@1"),
            node("sink", "acme/sink@1"),
        ],
        [
            edge("$inputs.left", "sink.in"),
            edge("$inputs.right", "sink.in"),
            edge("$inputs.unknown", "ghost.in"),
        ],
        [("bad", "sink.unknown")],
    );

    let result = validate_workflow(&document, &resolver);
    let Err(errors) = result else {
        panic!("invalid graph was accepted");
    };
    let kinds = errors
        .diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.kind())
        .collect::<Vec<_>>();

    assert!(kinds.contains(&ValidationErrorKind::DuplicateNode));
    assert!(kinds.contains(&ValidationErrorKind::UnresolvedNode));
    assert!(kinds.contains(&ValidationErrorKind::UnknownWorkflowInput));
    assert!(kinds.contains(&ValidationErrorKind::UnresolvedType));
    assert!(kinds.contains(&ValidationErrorKind::UnknownNode));
    assert!(kinds.contains(&ValidationErrorKind::MissingRequiredInput));
    assert!(kinds.contains(&ValidationErrorKind::TooManyInputConnections));
    assert!(kinds.contains(&ValidationErrorKind::InvalidWorkflowOutput));
}

#[test]
fn incompatible_edge_names_the_node_port_and_both_canonical_types() {
    let text = type_ref("acme.text", PrimitiveType::String);
    let number = type_ref("acme.number", PrimitiveType::I64);
    let resolver = TestResolver::default().with_type(text.clone()).with_node(
        "acme/number-sink",
        interface([("value", InputPort::required(number.clone()))], []),
    );
    let document = workflow(
        [("value", type_reference("acme.text", 1))],
        [node("sink", "acme/number-sink@1")],
        [edge("$inputs.value", "sink.value")],
        [],
    );

    let result = validate_workflow(&document, &resolver);
    let Err(errors) = result else {
        panic!("incompatible edge was accepted");
    };
    let mismatch = errors
        .diagnostics()
        .iter()
        .find(|diagnostic| diagnostic.kind() == ValidationErrorKind::IncompatibleTypes);
    let Some(mismatch) = mismatch else {
        panic!("type mismatch diagnostic was not emitted");
    };

    assert_eq!(mismatch.node().map(NodeId::as_str), Some("sink"));
    assert_eq!(mismatch.port().map(PortId::as_str), Some("value"));
    assert_eq!(mismatch.source_type(), Some(&text));
    assert_eq!(mismatch.target_type(), Some(&number));
    assert!(mismatch.message().contains("acme.text@1"));
    assert!(mismatch.message().contains("acme.number@1"));
}

#[test]
fn rejects_an_arbitrary_cycle_with_a_deterministic_path() {
    let value = type_ref("acme.value", PrimitiveType::String);
    let resolver = TestResolver::default()
        .with_node(
            "acme/a",
            interface(
                [("in", InputPort::optional(value.clone()))],
                [("out", value.clone())],
            ),
        )
        .with_node(
            "acme/b",
            interface(
                [("in", InputPort::optional(value.clone()))],
                [("out", value)],
            ),
        );
    let document = workflow(
        [],
        [node("a", "acme/a@1"), node("b", "acme/b@1")],
        [edge("a.out", "b.in"), edge("b.out", "a.in")],
        [],
    );

    let result = validate_workflow(&document, &resolver);
    let Err(errors) = result else {
        panic!("cyclic graph was accepted");
    };
    let cycle = errors
        .diagnostics()
        .iter()
        .find(|diagnostic| diagnostic.kind() == ValidationErrorKind::UnsupportedCycle);
    let Some(cycle) = cycle else {
        panic!("cycle diagnostic was not emitted");
    };

    assert_eq!(cycle.path(), "spec.edges");
    assert!(cycle.message().contains("a -> b -> a"));
}

fn workflow<const I: usize, const N: usize, const E: usize, const O: usize>(
    inputs: [(&str, TypeReference); I],
    nodes: [NodeDefinition; N],
    edges: [EdgeDefinition; E],
    outputs: [(&str, &str); O],
) -> WorkflowDocument {
    WorkflowDocument::new(
        WorkflowMetadata::new("test", Version::new(1, 0, 0)),
        WorkflowSpec::new(
            inputs
                .into_iter()
                .map(|(name, type_ref)| (port(name), type_ref))
                .collect(),
            nodes.into_iter().collect(),
            edges.into_iter().collect(),
            outputs
                .into_iter()
                .map(|(name, endpoint)| (port(name), endpoint_ref(endpoint)))
                .collect(),
        ),
    )
}

fn interface<const I: usize, const O: usize>(
    inputs: [(&str, InputPort); I],
    outputs: [(&str, TypeRef); O],
) -> NodeInterface {
    NodeInterface::new(
        inputs
            .into_iter()
            .map(|(name, input)| (port(name), input))
            .collect(),
        outputs
            .into_iter()
            .map(|(name, output)| (port(name), output))
            .collect(),
    )
}

fn node(id: &str, uses: &str) -> NodeDefinition {
    let parsed = uses.parse::<NodeReference>();
    let Ok(parsed) = parsed else {
        panic!("test node reference is invalid: {uses}");
    };
    NodeDefinition::new(node_id(id), parsed)
}

fn edge(from: &str, to: &str) -> EdgeDefinition {
    EdgeDefinition::new(endpoint_ref(from), endpoint_ref(to))
}

fn endpoint_ref(value: &str) -> Endpoint {
    let parsed = value.parse::<Endpoint>();
    let Ok(parsed) = parsed else {
        panic!("test endpoint is invalid: {value}");
    };
    parsed
}

fn type_reference(name: &str, version: u32) -> TypeReference {
    let parsed = TypeReference::new(name, version);
    let Ok(parsed) = parsed else {
        panic!("test type reference is invalid: {name}@{version}");
    };
    parsed
}

fn type_ref(name: &str, primitive: PrimitiveType) -> TypeRef {
    let parsed = TypeRef::new(name, 1, SchemaDefinition::primitive(primitive));
    let Ok(parsed) = parsed else {
        panic!("test canonical type is invalid: {name}");
    };
    parsed
}

fn node_id(value: &str) -> NodeId {
    let parsed = NodeId::new(value);
    let Ok(parsed) = parsed else {
        panic!("test node identifier is invalid: {value}");
    };
    parsed
}

fn port(value: &str) -> PortId {
    let parsed = PortId::new(value);
    let Ok(parsed) = parsed else {
        panic!("test port identifier is invalid: {value}");
    };
    parsed
}
