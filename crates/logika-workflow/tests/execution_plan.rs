//! Public contract tests for deterministic workflow plan compilation.

use std::{collections::BTreeMap, num::NonZeroU32, time::Duration};

use logika_core::{NodeId, PortId, PrimitiveType, SchemaDefinition, TypeRef};
use logika_workflow::{
    CompilationOptions, CompilationResolver, EdgeDefinition, Endpoint, ExecutionPolicy, InputPort,
    LockHash, NodeDefinition, NodeInterface, NodeReference, TypeReference, ValidationResolver,
    WorkflowDocument, WorkflowMetadata, WorkflowSpec, compile_workflow,
};
use semver::Version;

#[derive(Default)]
struct TestResolver {
    nodes: BTreeMap<String, (Version, NodeInterface)>,
    types: BTreeMap<(String, u32), TypeRef>,
}

impl TestResolver {
    fn with_type(mut self, type_ref: TypeRef) -> Self {
        self.types
            .insert((type_ref.name().to_owned(), type_ref.version()), type_ref);
        self
    }

    fn with_node(mut self, name: &str, version: Version, interface: NodeInterface) -> Self {
        self.nodes.insert(name.to_owned(), (version, interface));
        self
    }
}

impl ValidationResolver for TestResolver {
    fn resolve_node(&self, reference: &NodeReference) -> Option<&NodeInterface> {
        let (version, interface) = self.nodes.get(reference.plugin().as_str())?;
        reference
            .version_requirement()
            .matches(version)
            .then_some(interface)
    }

    fn resolve_type(&self, reference: &TypeReference) -> Option<&TypeRef> {
        self.types
            .get(&(reference.name().to_owned(), reference.version()))
    }
}

impl CompilationResolver for TestResolver {
    fn resolve_node_version(&self, reference: &NodeReference) -> Option<&Version> {
        let (version, _) = self.nodes.get(reference.plugin().as_str())?;
        reference
            .version_requirement()
            .matches(version)
            .then_some(version)
    }
}

#[test]
fn compiles_resolved_nodes_types_dependencies_and_policy() {
    let value = type_ref("acme.value");
    let resolver = resolver(value.clone());
    let document = workflow(false);

    let result = compile_workflow(
        &document,
        &resolver,
        CompilationOptions::new(LockHash::from_bytes(b"lock-v1")),
    );
    let Ok(plan) = result else {
        panic!("valid workflow did not compile: {result:?}");
    };

    assert_eq!(plan.workflow_name(), "deterministic-dag");
    assert_eq!(plan.workflow_version(), &Version::new(1, 0, 0));
    assert_eq!(plan.inputs().get(&port("value")), Some(&value));
    assert_eq!(
        plan.nodes()
            .iter()
            .map(|node| node.id().as_str())
            .collect::<Vec<_>>(),
        ["left", "right", "join"]
    );

    let join = &plan.nodes()[2];
    assert_eq!(
        join.dependencies()
            .iter()
            .map(NodeId::as_str)
            .collect::<Vec<_>>(),
        ["left", "right"]
    );
    assert_eq!(join.implementation().name().as_str(), "acme/join");
    assert_eq!(join.implementation().version(), &Version::new(1, 7, 0));
    assert_eq!(join.policy().max_attempts().get(), 1);
    assert_eq!(plan.edges().len(), 4);
    assert!(
        plan.edges()
            .iter()
            .all(|edge| edge.payload_type() == &value)
    );
    assert_eq!(
        plan.outputs()
            .get(&port("result"))
            .map(|output| output.payload_type()),
        Some(&value)
    );
    assert_eq!(plan.plan_hash().to_hex().len(), 64);
    assert_eq!(plan.cache_key().to_hex().len(), 64);
}

#[test]
fn hashes_are_deterministic_across_irrelevant_document_order() {
    let resolver = resolver(type_ref("acme.value"));
    let lock = LockHash::from_bytes(b"same-lock");
    let first = compile_workflow(&workflow(false), &resolver, CompilationOptions::new(lock));
    let second = compile_workflow(&workflow(true), &resolver, CompilationOptions::new(lock));
    let (Ok(first), Ok(second)) = (first, second) else {
        panic!("equivalent valid workflows did not compile");
    };

    assert_eq!(first.plan_hash(), second.plan_hash());
    assert_eq!(first.cache_key(), second.cache_key());
    assert_eq!(first.nodes(), second.nodes());
    assert_eq!(first.edges(), second.edges());
}

#[test]
fn cache_key_changes_with_lock_but_plan_identity_does_not() {
    let resolver = resolver(type_ref("acme.value"));
    let document = workflow(false);
    let first = compile_workflow(
        &document,
        &resolver,
        CompilationOptions::new(LockHash::from_bytes(b"lock-v1")),
    );
    let second = compile_workflow(
        &document,
        &resolver,
        CompilationOptions::new(LockHash::from_bytes(b"lock-v2")),
    );
    let (Ok(first), Ok(second)) = (first, second) else {
        panic!("valid workflow did not compile");
    };

    assert_eq!(first.plan_hash(), second.plan_hash());
    assert_ne!(first.cache_key(), second.cache_key());
}

#[test]
fn resilience_policy_is_part_of_plan_and_cache_identity() {
    let resolver = resolver(type_ref("acme.value"));
    let document = workflow(false);
    let default = compile_workflow(&document, &resolver, CompilationOptions::default());
    let Some(attempts) = NonZeroU32::new(3) else {
        panic!("three must be non-zero");
    };
    let policy = ExecutionPolicy::retry(attempts)
        .with_backoff(
            Duration::from_millis(25),
            Duration::from_secs(1),
            Duration::from_millis(5),
        )
        .with_attempt_timeout(Duration::from_secs(2))
        .with_retriable_code("acme.temporary");
    let resilient = compile_workflow(
        &document,
        &resolver,
        CompilationOptions::default().with_policy(policy.clone()),
    );
    let (Ok(default), Ok(resilient)) = (default, resilient) else {
        panic!("valid workflow did not compile");
    };

    assert_eq!(resilient.policy(), &policy);
    assert!(
        resilient
            .nodes()
            .iter()
            .all(|node| node.policy() == &policy)
    );
    assert_ne!(default.plan_hash(), resilient.plan_hash());
    assert_ne!(default.cache_key(), resilient.cache_key());
}

#[test]
fn compilation_preserves_validation_as_a_hard_boundary() {
    let resolver = resolver(type_ref("acme.value"));
    let mut document = workflow(false);
    document = WorkflowDocument::new(
        document.metadata().clone(),
        WorkflowSpec::new(
            document.spec().inputs().clone(),
            document.spec().nodes().to_vec(),
            vec![edge("left.out", "left.in")],
            document.spec().outputs().clone(),
        ),
    );

    let result = compile_workflow(&document, &resolver, CompilationOptions::default());
    assert!(result.is_err());
}

fn resolver(value: TypeRef) -> TestResolver {
    TestResolver::default()
        .with_type(value.clone())
        .with_node(
            "acme/pass",
            Version::new(1, 4, 0),
            NodeInterface::new(
                BTreeMap::from([(port("in"), InputPort::required(value.clone()))]),
                BTreeMap::from([(port("out"), value.clone())]),
            ),
        )
        .with_node(
            "acme/join",
            Version::new(1, 7, 0),
            NodeInterface::new(
                BTreeMap::from([
                    (port("left"), InputPort::required(value.clone())),
                    (port("right"), InputPort::required(value.clone())),
                ]),
                BTreeMap::from([(port("out"), value)]),
            ),
        )
}

fn workflow(reverse: bool) -> WorkflowDocument {
    let mut nodes = vec![
        node("left", "acme/pass@^1"),
        node("right", "acme/pass@^1"),
        node("join", "acme/join@^1"),
    ];
    let mut edges = vec![
        edge("$inputs.value", "left.in"),
        edge("$inputs.value", "right.in"),
        edge("left.out", "join.left"),
        edge("right.out", "join.right"),
    ];
    if reverse {
        nodes.reverse();
        edges.reverse();
    }

    WorkflowDocument::new(
        WorkflowMetadata::new("deterministic-dag", Version::new(1, 0, 0)),
        WorkflowSpec::new(
            BTreeMap::from([(port("value"), type_reference("acme.value", 1))]),
            nodes,
            edges,
            BTreeMap::from([(
                port("result"),
                Endpoint::node_port(node_id("join"), port("out")),
            )]),
        ),
    )
}

fn node(id: &str, uses: &str) -> NodeDefinition {
    let parsed = uses.parse();
    let Ok(parsed) = parsed else {
        panic!("invalid test node reference: {uses}");
    };
    NodeDefinition::new(node_id(id), parsed)
}

fn edge(from: &str, to: &str) -> EdgeDefinition {
    EdgeDefinition::new(endpoint(from), endpoint(to))
}

fn endpoint(value: &str) -> Endpoint {
    let parsed = value.parse();
    let Ok(parsed) = parsed else {
        panic!("invalid test endpoint: {value}");
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

fn type_ref(name: &str) -> TypeRef {
    let parsed = TypeRef::new(name, 1, SchemaDefinition::primitive(PrimitiveType::String));
    let Ok(parsed) = parsed else {
        panic!("invalid test canonical type: {name}");
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

fn port(value: &str) -> PortId {
    let parsed = PortId::new(value);
    let Ok(parsed) = parsed else {
        panic!("invalid test port id: {value}");
    };
    parsed
}
