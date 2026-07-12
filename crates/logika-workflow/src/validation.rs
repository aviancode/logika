use std::{
    collections::{BTreeMap, BTreeSet},
    error, fmt,
};

use logika_core::{Error, ErrorDetail, NodeId, PortId, TypeRef};

use crate::{Endpoint, NodeReference, TypeReference, WorkflowDocument};

/// Number of edges accepted by a node input port.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub enum ConnectionMultiplicity {
    /// At most one edge may target the input.
    #[default]
    Single,
    /// Any number of edges may target the input.
    Many,
}

/// Validation metadata for one input port of a resolved node.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputPort {
    type_ref: TypeRef,
    required: bool,
    multiplicity: ConnectionMultiplicity,
}

impl InputPort {
    /// Creates a required, single-connection input.
    #[must_use]
    pub const fn required(type_ref: TypeRef) -> Self {
        Self {
            type_ref,
            required: true,
            multiplicity: ConnectionMultiplicity::Single,
        }
    }

    /// Creates an optional, single-connection input.
    #[must_use]
    pub const fn optional(type_ref: TypeRef) -> Self {
        Self {
            type_ref,
            required: false,
            multiplicity: ConnectionMultiplicity::Single,
        }
    }

    /// Changes the accepted connection multiplicity.
    #[must_use]
    pub const fn with_multiplicity(mut self, multiplicity: ConnectionMultiplicity) -> Self {
        self.multiplicity = multiplicity;
        self
    }

    /// Returns the canonical input type.
    #[must_use]
    pub const fn type_ref(&self) -> &TypeRef {
        &self.type_ref
    }

    /// Returns whether at least one connection is required.
    #[must_use]
    pub const fn is_required(&self) -> bool {
        self.required
    }

    /// Returns the accepted connection multiplicity.
    #[must_use]
    pub const fn multiplicity(&self) -> ConnectionMultiplicity {
        self.multiplicity
    }
}

/// Resolved input and output contract used to validate a node invocation.
///
/// Registry implementations can own richer descriptors and expose this small
/// interface to the workflow crate without coupling validation to a registry.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NodeInterface {
    inputs: BTreeMap<PortId, InputPort>,
    outputs: BTreeMap<PortId, TypeRef>,
}

impl NodeInterface {
    /// Creates a node interface from named input and output ports.
    #[must_use]
    pub fn new(inputs: BTreeMap<PortId, InputPort>, outputs: BTreeMap<PortId, TypeRef>) -> Self {
        Self { inputs, outputs }
    }

    /// Returns the input port contracts.
    #[must_use]
    pub const fn inputs(&self) -> &BTreeMap<PortId, InputPort> {
        &self.inputs
    }

    /// Returns the output port types.
    #[must_use]
    pub const fn outputs(&self) -> &BTreeMap<PortId, TypeRef> {
        &self.outputs
    }
}

/// Supplies resolved node interfaces and canonical schemas to validation.
///
/// Node version selection and storage belong to a registry. The validator only
/// requires the resolved contract and therefore remains usable with application
/// registries, test doubles, and future plugin registries.
pub trait ValidationResolver {
    /// Resolves a version-constrained node reference to its port interface.
    fn resolve_node(&self, reference: &NodeReference) -> Option<&NodeInterface>;

    /// Resolves a document type identity to its canonical schema.
    fn resolve_type(&self, reference: &TypeReference) -> Option<&TypeRef>;

    /// Returns whether a source type can be connected to a target type.
    ///
    /// Implementations may honor explicit compatibility declarations. Strict
    /// canonical identity is used by default.
    fn are_types_compatible(&self, source: &TypeRef, target: &TypeRef) -> bool {
        source.is_compatible_with(target)
    }
}

/// Stable category of a workflow graph validation diagnostic.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ValidationErrorKind {
    /// Two node invocations use the same identifier.
    DuplicateNode,
    /// A node implementation could not be resolved.
    UnresolvedNode,
    /// An endpoint names a node that is not declared by the workflow.
    UnknownNode,
    /// An endpoint names a workflow input that is not declared.
    UnknownWorkflowInput,
    /// A workflow type identity could not be resolved to a canonical schema.
    UnresolvedType,
    /// A source endpoint names no output port on its node.
    UnknownOutputPort,
    /// A target endpoint names no input port on its node.
    UnknownInputPort,
    /// An edge attempts to target a workflow input.
    InvalidEdgeTarget,
    /// A required node input has no incoming edge.
    MissingRequiredInput,
    /// A single-connection input has more than one incoming edge.
    TooManyInputConnections,
    /// The source and target port schemas are incompatible.
    IncompatibleTypes,
    /// A workflow output has an invalid source endpoint.
    InvalidWorkflowOutput,
    /// The graph contains an unsupported directed cycle.
    UnsupportedCycle,
}

impl ValidationErrorKind {
    /// Returns the stable machine-readable diagnostic code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::DuplicateNode => "workflow.duplicate_node",
            Self::UnresolvedNode => "workflow.unresolved_node",
            Self::UnknownNode => "workflow.unknown_node",
            Self::UnknownWorkflowInput => "workflow.unknown_input",
            Self::UnresolvedType => "workflow.unresolved_type",
            Self::UnknownOutputPort => "workflow.unknown_output_port",
            Self::UnknownInputPort => "workflow.unknown_input_port",
            Self::InvalidEdgeTarget => "workflow.invalid_edge_target",
            Self::MissingRequiredInput => "workflow.missing_required_input",
            Self::TooManyInputConnections => "workflow.too_many_input_connections",
            Self::IncompatibleTypes => "workflow.incompatible_types",
            Self::InvalidWorkflowOutput => "workflow.invalid_output",
            Self::UnsupportedCycle => "workflow.unsupported_cycle",
        }
    }
}

/// One source-addressable workflow graph validation diagnostic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidationError {
    kind: ValidationErrorKind,
    path: String,
    message: String,
    node: Option<NodeId>,
    port: Option<PortId>,
    source_type: Option<TypeRef>,
    target_type: Option<TypeRef>,
}

impl ValidationError {
    fn new(kind: ValidationErrorKind, path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind,
            path: path.into(),
            message: message.into(),
            node: None,
            port: None,
            source_type: None,
            target_type: None,
        }
    }

    fn at_port(mut self, node: &NodeId, port: &PortId) -> Self {
        self.node = Some(node.clone());
        self.port = Some(port.clone());
        self
    }

    fn with_types(mut self, source: &TypeRef, target: &TypeRef) -> Self {
        self.source_type = Some(source.clone());
        self.target_type = Some(target.clone());
        self
    }

    /// Returns the diagnostic category.
    #[must_use]
    pub const fn kind(&self) -> ValidationErrorKind {
        self.kind
    }

    /// Returns the stable machine-readable diagnostic code.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.kind.code()
    }

    /// Returns the model path associated with the diagnostic.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Returns the safe human-readable diagnostic.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns the affected node, when applicable.
    #[must_use]
    pub const fn node(&self) -> Option<&NodeId> {
        self.node.as_ref()
    }

    /// Returns the affected port, when applicable.
    #[must_use]
    pub const fn port(&self) -> Option<&PortId> {
        self.port.as_ref()
    }

    /// Returns the source schema for a type mismatch.
    #[must_use]
    pub const fn source_type(&self) -> Option<&TypeRef> {
        self.source_type.as_ref()
    }

    /// Returns the target schema for a type mismatch.
    #[must_use]
    pub const fn target_type(&self) -> Option<&TypeRef> {
        self.target_type.as_ref()
    }
}

impl fmt::Display for ValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} at {}: {}",
            self.code(),
            self.path,
            self.message
        )
    }
}

impl error::Error for ValidationError {}

/// All diagnostics collected during one workflow validation pass.
#[derive(Debug)]
pub struct ValidationErrors {
    diagnostics: Vec<ValidationError>,
}

impl ValidationErrors {
    /// Returns diagnostics in deterministic validation order.
    #[must_use]
    pub fn diagnostics(&self) -> &[ValidationError] {
        &self.diagnostics
    }

    /// Consumes the error and returns its diagnostics.
    #[must_use]
    pub fn into_diagnostics(self) -> Vec<ValidationError> {
        self.diagnostics
    }
}

impl fmt::Display for ValidationErrors {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let count = self.diagnostics.len();
        write!(
            formatter,
            "workflow validation failed with {count} diagnostic(s)"
        )?;
        if let Some(first) = self.diagnostics.first() {
            write!(formatter, ": {first}")?;
        }
        Ok(())
    }
}

impl error::Error for ValidationErrors {}

impl From<ValidationErrors> for Error {
    fn from(source: ValidationErrors) -> Self {
        let message = source.to_string();
        Self::Validation(ErrorDetail::new("workflow.invalid_graph", message).with_source(source))
    }
}

/// Validates graph references, port contracts, types, multiplicity, and acyclicity.
///
/// All discoverable failures are returned together. Node implementations and
/// canonical schemas are obtained through `resolver`; validation performs no
/// I/O and never executes node code.
pub fn validate_workflow<R>(
    document: &WorkflowDocument,
    resolver: &R,
) -> Result<(), ValidationErrors>
where
    R: ValidationResolver + ?Sized,
{
    Validator::new(document, resolver).validate()
}

struct Validator<'a, R: ?Sized> {
    document: &'a WorkflowDocument,
    resolver: &'a R,
    diagnostics: Vec<ValidationError>,
    workflow_inputs: BTreeMap<PortId, Option<&'a TypeRef>>,
    nodes: BTreeMap<NodeId, ResolvedNode<'a>>,
    incoming: BTreeMap<(NodeId, PortId), usize>,
    dependencies: BTreeMap<NodeId, BTreeSet<NodeId>>,
}

#[derive(Clone, Copy)]
struct ResolvedNode<'a> {
    index: usize,
    interface: Option<&'a NodeInterface>,
}

impl<'a, R: ValidationResolver + ?Sized> Validator<'a, R> {
    fn new(document: &'a WorkflowDocument, resolver: &'a R) -> Self {
        Self {
            document,
            resolver,
            diagnostics: Vec::new(),
            workflow_inputs: BTreeMap::new(),
            nodes: BTreeMap::new(),
            incoming: BTreeMap::new(),
            dependencies: BTreeMap::new(),
        }
    }

    fn validate(mut self) -> Result<(), ValidationErrors> {
        self.resolve_workflow_inputs();
        self.resolve_nodes();
        self.validate_edges();
        self.validate_input_cardinality();
        self.validate_outputs();
        self.validate_cycles();

        if self.diagnostics.is_empty() {
            Ok(())
        } else {
            Err(ValidationErrors {
                diagnostics: self.diagnostics,
            })
        }
    }

    fn resolve_workflow_inputs(&mut self) {
        for (port, reference) in self.document.spec().inputs() {
            let resolved = self.resolver.resolve_type(reference);
            if resolved.is_none() {
                self.diagnostics.push(ValidationError::new(
                    ValidationErrorKind::UnresolvedType,
                    format!("spec.inputs.{port}"),
                    format!("workflow input {port} uses unresolved type {reference}"),
                ));
            }
            self.workflow_inputs.insert(port.clone(), resolved);
        }
    }

    fn resolve_nodes(&mut self) {
        for (index, node) in self.document.spec().nodes().iter().enumerate() {
            let path = format!("spec.nodes[{index}]");
            if let Some(first) = self.nodes.get(node.id()) {
                self.diagnostics.push(
                    ValidationError::new(
                        ValidationErrorKind::DuplicateNode,
                        format!("{path}.id"),
                        format!(
                            "node {} duplicates the declaration at spec.nodes[{}]",
                            node.id(),
                            first.index
                        ),
                    )
                    .with_node(node.id()),
                );
                continue;
            }

            let interface = self.resolver.resolve_node(node.uses());
            if interface.is_none() {
                self.diagnostics.push(
                    ValidationError::new(
                        ValidationErrorKind::UnresolvedNode,
                        format!("{path}.uses"),
                        format!(
                            "node {} uses unresolved implementation {}",
                            node.id(),
                            node.uses()
                        ),
                    )
                    .with_node(node.id()),
                );
            }
            self.dependencies.entry(node.id().clone()).or_default();
            self.nodes
                .insert(node.id().clone(), ResolvedNode { index, interface });
        }
    }

    fn validate_edges(&mut self) {
        for (index, edge) in self.document.spec().edges().iter().enumerate() {
            let path = format!("spec.edges[{index}]");
            let source = self.resolve_source(edge.from(), &format!("{path}.from"), false);
            let target = self.resolve_target(edge.to(), &format!("{path}.to"));

            if let Some((node, port, _)) = target.as_ref() {
                *self
                    .incoming
                    .entry((node.clone(), port.clone()))
                    .or_default() += 1;
            }

            if let (
                Some((source_type, source_node)),
                Some((target_node, target_port, target_type)),
            ) = (source, target)
            {
                if !self.resolver.are_types_compatible(source_type, target_type) {
                    self.diagnostics.push(
                        ValidationError::new(
                            ValidationErrorKind::IncompatibleTypes,
                            path,
                            format!(
                                "node {target_node} input port {target_port} expects {}, but the edge provides {}",
                                type_label(target_type),
                                type_label(source_type)
                            ),
                        )
                        .at_port(&target_node, &target_port)
                        .with_types(source_type, target_type),
                    );
                }

                if let Some(source_node) = source_node {
                    self.dependencies
                        .entry(source_node)
                        .or_default()
                        .insert(target_node);
                }
            }
        }
    }

    fn resolve_source(
        &mut self,
        endpoint: &'a Endpoint,
        path: &str,
        workflow_output: bool,
    ) -> Option<(&'a TypeRef, Option<NodeId>)> {
        match endpoint {
            Endpoint::WorkflowInput(port) => {
                let Some(resolved) = self.workflow_inputs.get(port) else {
                    self.diagnostics.push(ValidationError::new(
                        if workflow_output {
                            ValidationErrorKind::InvalidWorkflowOutput
                        } else {
                            ValidationErrorKind::UnknownWorkflowInput
                        },
                        path,
                        format!("workflow input {port} is not declared"),
                    ));
                    return None;
                };
                let type_ref = resolved.as_ref().copied()?;
                Some((type_ref, None))
            }
            Endpoint::NodePort { node, port } => {
                let Some(resolved) = self.nodes.get(node) else {
                    self.diagnostics.push(
                        ValidationError::new(
                            if workflow_output {
                                ValidationErrorKind::InvalidWorkflowOutput
                            } else {
                                ValidationErrorKind::UnknownNode
                            },
                            path,
                            format!("source node {node} is not declared"),
                        )
                        .with_node(node),
                    );
                    return None;
                };
                let interface = resolved.interface?;
                let Some(type_ref) = interface.outputs().get(port) else {
                    self.diagnostics.push(
                        ValidationError::new(
                            if workflow_output {
                                ValidationErrorKind::InvalidWorkflowOutput
                            } else {
                                ValidationErrorKind::UnknownOutputPort
                            },
                            path,
                            format!("node {node} has no output port {port}"),
                        )
                        .at_port(node, port),
                    );
                    return None;
                };
                Some((type_ref, Some(node.clone())))
            }
        }
    }

    fn resolve_target(
        &mut self,
        endpoint: &'a Endpoint,
        path: &str,
    ) -> Option<(NodeId, PortId, &'a TypeRef)> {
        let Endpoint::NodePort { node, port } = endpoint else {
            self.diagnostics.push(ValidationError::new(
                ValidationErrorKind::InvalidEdgeTarget,
                path,
                format!("edge target {endpoint} is a workflow input, not a node input port"),
            ));
            return None;
        };
        let Some(resolved) = self.nodes.get(node) else {
            self.diagnostics.push(
                ValidationError::new(
                    ValidationErrorKind::UnknownNode,
                    path,
                    format!("target node {node} is not declared"),
                )
                .with_node(node),
            );
            return None;
        };
        let interface = resolved.interface?;
        let Some(input) = interface.inputs().get(port) else {
            self.diagnostics.push(
                ValidationError::new(
                    ValidationErrorKind::UnknownInputPort,
                    path,
                    format!("node {node} has no input port {port}"),
                )
                .at_port(node, port),
            );
            return None;
        };
        Some((node.clone(), port.clone(), input.type_ref()))
    }

    fn validate_input_cardinality(&mut self) {
        for (node, resolved) in &self.nodes {
            let Some(interface) = resolved.interface else {
                continue;
            };
            for (port, input) in interface.inputs() {
                let count = self
                    .incoming
                    .get(&(node.clone(), port.clone()))
                    .copied()
                    .unwrap_or_default();
                let path = format!("spec.nodes[{}]", resolved.index);
                if input.is_required() && count == 0 {
                    self.diagnostics.push(
                        ValidationError::new(
                            ValidationErrorKind::MissingRequiredInput,
                            path.clone(),
                            format!("node {node} required input port {port} has no connection"),
                        )
                        .at_port(node, port),
                    );
                }
                if input.multiplicity() == ConnectionMultiplicity::Single && count > 1 {
                    self.diagnostics.push(
                        ValidationError::new(
                            ValidationErrorKind::TooManyInputConnections,
                            path,
                            format!(
                                "node {node} input port {port} accepts one connection, but received {count}"
                            ),
                        )
                        .at_port(node, port),
                    );
                }
            }
        }
    }

    fn validate_outputs(&mut self) {
        for (name, endpoint) in self.document.spec().outputs() {
            let path = format!("spec.outputs.{name}");
            let _ = self.resolve_source(endpoint, &path, true);
        }
    }

    fn validate_cycles(&mut self) {
        if let Some(cycle) = find_cycle(&self.dependencies) {
            let labels = cycle
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" -> ");
            self.diagnostics.push(ValidationError::new(
                ValidationErrorKind::UnsupportedCycle,
                "spec.edges",
                format!("workflow graph contains unsupported cycle: {labels}"),
            ));
        }
    }
}

impl ValidationError {
    fn with_node(mut self, node: &NodeId) -> Self {
        self.node = Some(node.clone());
        self
    }
}

fn type_label(type_ref: &TypeRef) -> String {
    format!(
        "{}@{}#{}",
        type_ref.name(),
        type_ref.version(),
        type_ref.fingerprint()
    )
}

fn find_cycle(graph: &BTreeMap<NodeId, BTreeSet<NodeId>>) -> Option<Vec<NodeId>> {
    let mut states = BTreeMap::new();
    let mut stack = Vec::new();
    for node in graph.keys() {
        if !states.contains_key(node)
            && let Some(cycle) = visit(node, graph, &mut states, &mut stack)
        {
            return Some(cycle);
        }
    }
    None
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum VisitState {
    Visiting,
    Complete,
}

fn visit(
    node: &NodeId,
    graph: &BTreeMap<NodeId, BTreeSet<NodeId>>,
    states: &mut BTreeMap<NodeId, VisitState>,
    stack: &mut Vec<NodeId>,
) -> Option<Vec<NodeId>> {
    states.insert(node.clone(), VisitState::Visiting);
    stack.push(node.clone());

    if let Some(neighbors) = graph.get(node) {
        for neighbor in neighbors {
            match states.get(neighbor) {
                Some(VisitState::Visiting) => {
                    let start = stack.iter().position(|item| item == neighbor)?;
                    let mut cycle = stack[start..].to_vec();
                    cycle.push(neighbor.clone());
                    return Some(cycle);
                }
                Some(VisitState::Complete) => {}
                None => {
                    if let Some(cycle) = visit(neighbor, graph, states, stack) {
                        return Some(cycle);
                    }
                }
            }
        }
    }

    let _ = stack.pop();
    states.insert(node.clone(), VisitState::Complete);
    None
}
