//! Rust SDK for defining typed `logika` nodes and workflows.
//!
//! [`WorkflowBuilder`] represents ports with [`Input<T>`] and [`Output<T>`].
//! Its [`WorkflowBuilder::connect`] method accepts the same `T` on both sides,
//! so incompatible Rust node types are rejected by the compiler before a
//! portable workflow document is created.

#![forbid(unsafe_code)]

extern crate self as logika_sdk;

use std::{
    collections::{BTreeMap, BTreeSet},
    error, fmt,
    marker::PhantomData,
};

pub use logika_core::Schema;
use logika_core::{IdentifierError, NodeId, PortId, SchemaError};
pub use logika_sdk_macros::Schema;
use logika_workflow::{
    EdgeDefinition, Endpoint, NodeDefinition, NodeReference, ReferenceError, TypeReference,
    WorkflowDocument, WorkflowMetadata, WorkflowSpec,
};
pub use semver::Version;

/// A statically typed local Rust node contract.
///
/// Execution is intentionally not part of this initial trait: the runtime
/// supplies its asynchronous execution context separately. Implementations
/// declare only the stable metadata and the portable schemas needed to build
/// and validate a workflow.
pub trait Node: Send + Sync + 'static {
    /// Value accepted by the node's input port.
    type Input: Schema;
    /// Value produced by the node's output port.
    type Output: Schema;

    /// Stable, namespaced node implementation name.
    const NAME: &'static str;
    /// Concrete semantic version registered for this implementation.
    const VERSION: &'static str;
    /// Name of the node's input port.
    const INPUT_PORT: &'static str = "input";
    /// Name of the node's output port.
    const OUTPUT_PORT: &'static str = "output";
}

/// A typed destination port owned by a node invocation.
pub struct Input<T> {
    endpoint: Endpoint,
    marker: PhantomData<fn(T)>,
}

impl<T> Input<T> {
    /// Returns the portable endpoint represented by this handle.
    #[must_use]
    pub const fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }
}

impl<T> Clone for Input<T> {
    fn clone(&self) -> Self {
        Self {
            endpoint: self.endpoint.clone(),
            marker: PhantomData,
        }
    }
}

impl<T> fmt::Debug for Input<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("Input")
            .field(&self.endpoint)
            .finish()
    }
}

/// A typed source port from a workflow input or node invocation.
pub struct Output<T> {
    endpoint: Endpoint,
    marker: PhantomData<fn() -> T>,
}

impl<T> Output<T> {
    /// Returns the portable endpoint represented by this handle.
    #[must_use]
    pub const fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }
}

impl<T> Clone for Output<T> {
    fn clone(&self) -> Self {
        Self {
            endpoint: self.endpoint.clone(),
            marker: PhantomData,
        }
    }
}

impl<T> fmt::Debug for Output<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("Output")
            .field(&self.endpoint)
            .finish()
    }
}

/// Typed handles for one node invocation added to a workflow.
pub struct NodeHandle<N: Node> {
    id: NodeId,
    input: Input<N::Input>,
    output: Output<N::Output>,
}

impl<N: Node> fmt::Debug for NodeHandle<N> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NodeHandle")
            .field("id", &self.id)
            .field("input", &self.input)
            .field("output", &self.output)
            .finish()
    }
}

impl<N: Node> NodeHandle<N> {
    /// Returns the stable invocation identifier.
    #[must_use]
    pub const fn id(&self) -> &NodeId {
        &self.id
    }

    /// Returns a typed handle to the node's input port.
    #[must_use]
    pub fn input(&self) -> Input<N::Input> {
        self.input.clone()
    }

    /// Returns a typed handle to the node's output port.
    #[must_use]
    pub fn output(&self) -> Output<N::Output> {
        self.output.clone()
    }
}

/// Builds a portable workflow while preserving Rust port types.
#[derive(Debug)]
pub struct WorkflowBuilder {
    metadata: WorkflowMetadata,
    inputs: BTreeMap<PortId, TypeReference>,
    nodes: Vec<NodeDefinition>,
    node_ids: BTreeSet<NodeId>,
    edges: Vec<EdgeDefinition>,
    outputs: BTreeMap<PortId, Endpoint>,
}

impl WorkflowBuilder {
    /// Starts a workflow with the supplied stable name and semantic version.
    #[must_use]
    pub fn new(name: impl Into<String>, version: Version) -> Self {
        Self {
            metadata: WorkflowMetadata::new(name, version),
            inputs: BTreeMap::new(),
            nodes: Vec::new(),
            node_ids: BTreeSet::new(),
            edges: Vec::new(),
            outputs: BTreeMap::new(),
        }
    }

    /// Declares a workflow input and returns it as a typed source port.
    pub fn input<T: Schema>(&mut self, name: impl Into<String>) -> Result<Output<T>, BuildError> {
        let port = PortId::new(name.into())?;
        if self.inputs.contains_key(&port) {
            return Err(BuildError::DuplicateWorkflowInput(port));
        }

        let type_ref = T::type_ref()?;
        let reference = TypeReference::new(type_ref.name(), type_ref.version())?;
        self.inputs.insert(port.clone(), reference);
        Ok(Output {
            endpoint: Endpoint::workflow_input(port),
            marker: PhantomData,
        })
    }

    /// Adds a typed node invocation and returns its input and output handles.
    pub fn node<N: Node>(&mut self, id: impl Into<String>) -> Result<NodeHandle<N>, BuildError> {
        let id = NodeId::new(id.into())?;
        if self.node_ids.contains(&id) {
            return Err(BuildError::DuplicateNode(id));
        }

        N::Input::type_ref()?;
        N::Output::type_ref()?;
        let input_port = PortId::new(N::INPUT_PORT)?;
        let output_port = PortId::new(N::OUTPUT_PORT)?;
        let uses = format!("{}@={}", N::NAME, N::VERSION).parse::<NodeReference>()?;

        self.nodes.push(NodeDefinition::new(id.clone(), uses));
        self.node_ids.insert(id.clone());
        Ok(NodeHandle {
            input: Input {
                endpoint: Endpoint::node_port(id.clone(), input_port),
                marker: PhantomData,
            },
            output: Output {
                endpoint: Endpoint::node_port(id.clone(), output_port),
                marker: PhantomData,
            },
            id,
        })
    }

    /// Connects a typed source to a destination of the exact same Rust type.
    ///
    /// ```compile_fail
    /// use logika_sdk::{Input, Output, WorkflowBuilder};
    ///
    /// struct Order;
    /// struct Customer;
    ///
    /// fn incompatible(
    ///     builder: &mut WorkflowBuilder,
    ///     order: Output<Order>,
    ///     customer: Input<Customer>,
    /// ) {
    ///     builder.connect(order, customer);
    /// }
    /// ```
    pub fn connect<T>(&mut self, from: Output<T>, to: Input<T>) -> &mut Self {
        self.edges
            .push(EdgeDefinition::new(from.endpoint, to.endpoint));
        self
    }

    /// Publishes a typed source port as a workflow output.
    pub fn output<T>(
        &mut self,
        name: impl Into<String>,
        source: Output<T>,
    ) -> Result<&mut Self, BuildError> {
        let port = PortId::new(name.into())?;
        if self.outputs.contains_key(&port) {
            return Err(BuildError::DuplicateWorkflowOutput(port));
        }
        self.outputs.insert(port, source.endpoint);
        Ok(self)
    }

    /// Finishes the typed builder and returns the canonical workflow document.
    #[must_use]
    pub fn build(self) -> WorkflowDocument {
        WorkflowDocument::new(
            self.metadata,
            WorkflowSpec::new(self.inputs, self.nodes, self.edges, self.outputs),
        )
    }
}

/// Error raised while portable metadata is collected by [`WorkflowBuilder`].
#[derive(Debug)]
#[non_exhaustive]
pub enum BuildError {
    /// A node or port identifier is malformed.
    InvalidIdentifier(IdentifierError),
    /// A node or type reference is malformed.
    InvalidReference(ReferenceError),
    /// A Rust type produced an invalid canonical schema.
    InvalidSchema(SchemaError),
    /// The node invocation identifier is already present.
    DuplicateNode(NodeId),
    /// The workflow input name is already present.
    DuplicateWorkflowInput(PortId),
    /// The workflow output name is already present.
    DuplicateWorkflowOutput(PortId),
}

impl fmt::Display for BuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidIdentifier(source) => source.fmt(formatter),
            Self::InvalidReference(source) => source.fmt(formatter),
            Self::InvalidSchema(source) => source.fmt(formatter),
            Self::DuplicateNode(id) => write!(formatter, "node {id} is already declared"),
            Self::DuplicateWorkflowInput(port) => {
                write!(formatter, "workflow input {port} is already declared")
            }
            Self::DuplicateWorkflowOutput(port) => {
                write!(formatter, "workflow output {port} is already declared")
            }
        }
    }
}

impl error::Error for BuildError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::InvalidIdentifier(source) => Some(source),
            Self::InvalidReference(source) => Some(source),
            Self::InvalidSchema(source) => Some(source),
            Self::DuplicateNode(_)
            | Self::DuplicateWorkflowInput(_)
            | Self::DuplicateWorkflowOutput(_) => None,
        }
    }
}

impl From<IdentifierError> for BuildError {
    fn from(source: IdentifierError) -> Self {
        Self::InvalidIdentifier(source)
    }
}

impl From<ReferenceError> for BuildError {
    fn from(source: ReferenceError) -> Self {
        Self::InvalidReference(source)
    }
}

impl From<SchemaError> for BuildError {
    fn from(source: SchemaError) -> Self {
        Self::InvalidSchema(source)
    }
}

/// Implementation details used by the derive macro.
#[doc(hidden)]
pub mod _private {
    pub use logika_core::{
        EnumSchema, PrimitiveType, SchemaDefinition, SchemaField, StructSchema, TaggedUnionSchema,
        TaggedVariant,
    };
}
