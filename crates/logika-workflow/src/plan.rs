use std::{
    collections::{BTreeMap, BTreeSet},
    error, fmt,
    num::NonZeroU32,
    time::Duration,
};

use logika_core::{NodeId, PluginId, PortId, TypeRef};
use semver::Version;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{
    ConnectionMultiplicity, EdgeDefinition, Endpoint, NodeDefinition, NodeInterface, NodeReference,
    ValidationErrors, ValidationResolver, WorkflowDocument, validate_workflow,
};

macro_rules! hash_type {
    ($(#[$attribute:meta])* $name:ident) => {
        $(#[$attribute])*
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name([u8; 32]);

        impl $name {
            /// Returns the raw SHA-256 bytes.
            #[must_use]
            pub const fn as_bytes(&self) -> &[u8; 32] {
                &self.0
            }

            /// Returns the lowercase hexadecimal representation.
            #[must_use]
            pub fn to_hex(self) -> String {
                hex_digest(&self.0)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&hex_digest(&self.0))
            }
        }
    };
}

hash_type!(
    /// SHA-256 identity of the immutable compiled plan.
    PlanHash
);
hash_type!(
    /// SHA-256 cache lookup key derived from workflow, lock, and execution policy.
    PlanCacheKey
);
hash_type!(
    /// SHA-256 identity of the lock data used for compilation.
    LockHash
);

impl LockHash {
    /// Hashes the canonical bytes of a lock file or equivalent resolution snapshot.
    #[must_use]
    pub fn from_bytes(bytes: impl AsRef<[u8]>) -> Self {
        Self(Sha256::digest(bytes.as_ref()).into())
    }

    /// Returns the fingerprint used when compilation has no lock snapshot.
    #[must_use]
    pub fn unlocked() -> Self {
        Self::from_bytes([])
    }
}

impl Default for LockHash {
    fn default() -> Self {
        Self::unlocked()
    }
}

/// Retry and timeout constraints embedded in a compiled plan.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct ExecutionPolicy {
    max_attempts: NonZeroU32,
    initial_backoff: Duration,
    max_backoff: Duration,
    jitter: Duration,
    attempt_timeout: Option<Duration>,
    retriable_codes: BTreeSet<String>,
}

impl ExecutionPolicy {
    /// Returns a fail-fast policy with no attempt timeout.
    #[must_use]
    pub fn single_attempt() -> Self {
        Self {
            max_attempts: NonZeroU32::MIN,
            initial_backoff: Duration::ZERO,
            max_backoff: Duration::ZERO,
            jitter: Duration::ZERO,
            attempt_timeout: None,
            retriable_codes: BTreeSet::new(),
        }
    }

    /// Creates a retry policy with exponential backoff defaults.
    ///
    /// Errors remain non-retriable until their exact diagnostic codes are
    /// added with [`Self::with_retriable_code`].
    #[must_use]
    pub fn retry(max_attempts: NonZeroU32) -> Self {
        Self {
            max_attempts,
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(30),
            jitter: Duration::from_millis(100),
            attempt_timeout: None,
            retriable_codes: BTreeSet::new(),
        }
    }

    /// Replaces the exponential backoff bounds and maximum additive jitter.
    #[must_use]
    pub fn with_backoff(mut self, initial: Duration, maximum: Duration, jitter: Duration) -> Self {
        self.initial_backoff = initial;
        self.max_backoff = maximum;
        self.jitter = jitter;
        self
    }

    /// Sets the maximum duration of each node attempt.
    #[must_use]
    pub fn with_attempt_timeout(mut self, timeout: Duration) -> Self {
        self.attempt_timeout = Some(timeout);
        self
    }

    /// Adds one exact public error code to the retry classification.
    #[must_use]
    pub fn with_retriable_code(mut self, code: impl Into<String>) -> Self {
        self.retriable_codes.insert(code.into());
        self
    }

    /// Returns the maximum attempts permitted for one node invocation.
    #[must_use]
    pub const fn max_attempts(&self) -> NonZeroU32 {
        self.max_attempts
    }

    /// Returns the initial exponential backoff delay.
    #[must_use]
    pub const fn initial_backoff(&self) -> Duration {
        self.initial_backoff
    }

    /// Returns the upper bound for exponential backoff before jitter.
    #[must_use]
    pub const fn max_backoff(&self) -> Duration {
        self.max_backoff
    }

    /// Returns the maximum additive jitter applied to a backoff delay.
    #[must_use]
    pub const fn jitter(&self) -> Duration {
        self.jitter
    }

    /// Returns the timeout applied independently to each node attempt.
    #[must_use]
    pub const fn attempt_timeout(&self) -> Option<Duration> {
        self.attempt_timeout
    }

    /// Returns the exact diagnostic codes classified as retriable.
    #[must_use]
    pub const fn retriable_codes(&self) -> &BTreeSet<String> {
        &self.retriable_codes
    }

    /// Returns whether an executor error code is eligible for retry.
    #[must_use]
    pub fn is_retriable(&self, code: &str) -> bool {
        self.retriable_codes.contains(code)
    }
}

impl Default for ExecutionPolicy {
    fn default() -> Self {
        Self::single_attempt()
    }
}

/// Inputs that affect deterministic plan compilation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilationOptions {
    lock_hash: LockHash,
    policy: ExecutionPolicy,
}

impl CompilationOptions {
    /// Creates options for a particular lock snapshot.
    #[must_use]
    pub fn new(lock_hash: LockHash) -> Self {
        Self {
            lock_hash,
            policy: ExecutionPolicy::default(),
        }
    }

    /// Compiles the supplied resilience policy into every plan node.
    #[must_use]
    pub fn with_policy(mut self, policy: ExecutionPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Returns the lock snapshot fingerprint.
    #[must_use]
    pub const fn lock_hash(&self) -> LockHash {
        self.lock_hash
    }

    /// Returns the resilience policy to compile into the plan.
    #[must_use]
    pub const fn policy(&self) -> &ExecutionPolicy {
        &self.policy
    }
}

impl Default for CompilationOptions {
    fn default() -> Self {
        Self::new(LockHash::default())
    }
}

/// Supplies the concrete node version selected for a validated reference.
///
/// The interface and type portions are inherited from [`ValidationResolver`].
/// Implementations must return the version belonging to the same resolution
/// result exposed to validation.
pub trait CompilationResolver: ValidationResolver {
    /// Resolves a version requirement to one concrete implementation version.
    fn resolve_node_version(&self, reference: &NodeReference) -> Option<&Version>;
}

/// A concrete node implementation selected during compilation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedImplementation {
    name: PluginId,
    version: Version,
}

impl ResolvedImplementation {
    /// Returns the globally namespaced node implementation name.
    #[must_use]
    pub const fn name(&self) -> &PluginId {
        &self.name
    }

    /// Returns the exact resolved implementation version.
    #[must_use]
    pub const fn version(&self) -> &Version {
        &self.version
    }
}

/// One immutable resolved node in topological plan order.
#[derive(Clone, Debug, PartialEq)]
pub struct PlanNode {
    id: NodeId,
    implementation: ResolvedImplementation,
    interface: NodeInterface,
    configuration: BTreeMap<String, Value>,
    dependencies: Vec<NodeId>,
    policy: ExecutionPolicy,
}

impl PlanNode {
    /// Returns the workflow-local node identifier.
    #[must_use]
    pub const fn id(&self) -> &NodeId {
        &self.id
    }

    /// Returns the concrete implementation selected by the resolver.
    #[must_use]
    pub const fn implementation(&self) -> &ResolvedImplementation {
        &self.implementation
    }

    /// Returns the canonical, validated port interface.
    #[must_use]
    pub const fn interface(&self) -> &NodeInterface {
        &self.interface
    }

    /// Returns the portable node configuration.
    #[must_use]
    pub const fn configuration(&self) -> &BTreeMap<String, Value> {
        &self.configuration
    }

    /// Returns direct predecessor node identifiers in deterministic order.
    #[must_use]
    pub fn dependencies(&self) -> &[NodeId] {
        &self.dependencies
    }

    /// Returns the execution policy compiled for this node.
    #[must_use]
    pub const fn policy(&self) -> &ExecutionPolicy {
        &self.policy
    }
}

/// One validated data connection in an execution plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanEdge {
    from: Endpoint,
    to: Endpoint,
    payload_type: TypeRef,
}

impl PlanEdge {
    /// Returns the source endpoint.
    #[must_use]
    pub const fn from(&self) -> &Endpoint {
        &self.from
    }

    /// Returns the target node input endpoint.
    #[must_use]
    pub const fn to(&self) -> &Endpoint {
        &self.to
    }

    /// Returns the canonical source payload type checked for this connection.
    #[must_use]
    pub const fn payload_type(&self) -> &TypeRef {
        &self.payload_type
    }
}

/// One named workflow output and its resolved canonical type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanOutput {
    source: Endpoint,
    payload_type: TypeRef,
}

impl PlanOutput {
    /// Returns the endpoint that supplies this workflow output.
    #[must_use]
    pub const fn source(&self) -> &Endpoint {
        &self.source
    }

    /// Returns the canonical output payload type.
    #[must_use]
    pub const fn payload_type(&self) -> &TypeRef {
        &self.payload_type
    }
}

/// An immutable, validated, and fully resolved workflow execution plan.
#[derive(Clone, Debug, PartialEq)]
pub struct ExecutionPlan {
    workflow_name: String,
    workflow_version: Version,
    inputs: BTreeMap<PortId, TypeRef>,
    nodes: Vec<PlanNode>,
    edges: Vec<PlanEdge>,
    outputs: BTreeMap<PortId, PlanOutput>,
    policy: ExecutionPolicy,
    plan_hash: PlanHash,
    cache_key: PlanCacheKey,
}

impl ExecutionPlan {
    /// Returns the host-facing workflow name.
    #[must_use]
    pub fn workflow_name(&self) -> &str {
        &self.workflow_name
    }

    /// Returns the workflow document version.
    #[must_use]
    pub const fn workflow_version(&self) -> &Version {
        &self.workflow_version
    }

    /// Returns canonical workflow input types in name order.
    #[must_use]
    pub const fn inputs(&self) -> &BTreeMap<PortId, TypeRef> {
        &self.inputs
    }

    /// Returns resolved nodes in deterministic topological order.
    #[must_use]
    pub fn nodes(&self) -> &[PlanNode] {
        &self.nodes
    }

    /// Returns validated edges in deterministic endpoint order.
    #[must_use]
    pub fn edges(&self) -> &[PlanEdge] {
        &self.edges
    }

    /// Returns resolved workflow outputs in name order.
    #[must_use]
    pub const fn outputs(&self) -> &BTreeMap<PortId, PlanOutput> {
        &self.outputs
    }

    /// Returns the default execution policy compiled into the plan.
    #[must_use]
    pub const fn policy(&self) -> &ExecutionPolicy {
        &self.policy
    }

    /// Returns the identity of the compiled plan contents.
    #[must_use]
    pub const fn plan_hash(&self) -> PlanHash {
        self.plan_hash
    }

    /// Returns the workflow, lock, and execution-policy cache lookup key.
    #[must_use]
    pub const fn cache_key(&self) -> PlanCacheKey {
        self.cache_key
    }
}

/// Failure returned while validating or compiling an execution plan.
#[derive(Debug)]
#[non_exhaustive]
pub enum CompilationError {
    /// The source workflow did not pass graph validation.
    Validation(ValidationErrors),
    /// The resolver did not provide a complete, stable result after validation.
    InconsistentResolution {
        /// Workflow-local node affected by the resolver inconsistency.
        node: NodeId,
        /// Requested implementation reference.
        reference: NodeReference,
    },
    /// A workflow input or endpoint type disappeared after validation.
    InconsistentTypeResolution {
        /// Portable path to the input or endpoint.
        path: String,
    },
    /// Portable node configuration could not be canonically encoded.
    ConfigurationEncoding(serde_json::Error),
}

impl fmt::Display for CompilationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation(source) => source.fmt(formatter),
            Self::InconsistentResolution { node, reference } => write!(
                formatter,
                "resolver returned incomplete data for node {node} using {reference}"
            ),
            Self::InconsistentTypeResolution { path } => {
                write!(
                    formatter,
                    "resolver returned incomplete type data for {path}"
                )
            }
            Self::ConfigurationEncoding(_) => {
                formatter.write_str("node configuration could not be canonically encoded")
            }
        }
    }
}

impl error::Error for CompilationError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Validation(source) => Some(source),
            Self::ConfigurationEncoding(source) => Some(source),
            Self::InconsistentResolution { .. } | Self::InconsistentTypeResolution { .. } => None,
        }
    }
}

impl From<ValidationErrors> for CompilationError {
    fn from(source: ValidationErrors) -> Self {
        Self::Validation(source)
    }
}

/// Validates and compiles a workflow into an immutable execution plan.
///
/// Compilation performs no I/O and never executes node code. The cache key is
/// based only on canonical workflow contents, lock fingerprint, and policy;
/// the plan hash additionally covers exact resolved versions, schemas, graph
/// dependencies, configuration, and policy.
pub fn compile_workflow<R>(
    document: &WorkflowDocument,
    resolver: &R,
    options: CompilationOptions,
) -> Result<ExecutionPlan, CompilationError>
where
    R: CompilationResolver + ?Sized,
{
    validate_workflow(document, resolver)?;

    let inputs = resolve_inputs(document, resolver)?;
    let drafts = resolve_nodes(document, resolver)?;
    let (dependencies, order) = dependency_order(document);
    let edges = compile_edges(document, &inputs, &drafts)?;
    let outputs = compile_outputs(document, &inputs, &drafts)?;
    let policy = options.policy().clone();

    let mut nodes = Vec::with_capacity(order.len());
    for id in order {
        let Some(draft) = drafts.get(&id) else {
            return Err(CompilationError::InconsistentTypeResolution {
                path: format!("spec.nodes.{id}"),
            });
        };
        nodes.push(PlanNode {
            id: id.clone(),
            implementation: ResolvedImplementation {
                name: draft.definition.uses().plugin().clone(),
                version: draft.version.clone(),
            },
            interface: draft.interface.clone(),
            configuration: draft.definition.configuration().clone(),
            dependencies: dependencies
                .get(&id)
                .map(|items| items.iter().cloned().collect())
                .unwrap_or_default(),
            policy: policy.clone(),
        });
    }

    let cache_key = workflow_cache_key(document, options.lock_hash(), &policy)?;
    let plan_hash = hash_plan(document, &inputs, &nodes, &edges, &outputs, &policy)?;

    Ok(ExecutionPlan {
        workflow_name: document.metadata().name().to_owned(),
        workflow_version: document.metadata().version().clone(),
        inputs,
        nodes,
        edges,
        outputs,
        policy,
        plan_hash,
        cache_key,
    })
}

struct NodeDraft<'a> {
    definition: &'a NodeDefinition,
    version: Version,
    interface: NodeInterface,
}

fn resolve_inputs<R>(
    document: &WorkflowDocument,
    resolver: &R,
) -> Result<BTreeMap<PortId, TypeRef>, CompilationError>
where
    R: CompilationResolver + ?Sized,
{
    let mut inputs = BTreeMap::new();
    for (name, reference) in document.spec().inputs() {
        let Some(type_ref) = resolver.resolve_type(reference) else {
            return Err(CompilationError::InconsistentTypeResolution {
                path: format!("spec.inputs.{name}"),
            });
        };
        inputs.insert(name.clone(), type_ref.clone());
    }
    Ok(inputs)
}

fn resolve_nodes<'a, R>(
    document: &'a WorkflowDocument,
    resolver: &R,
) -> Result<BTreeMap<NodeId, NodeDraft<'a>>, CompilationError>
where
    R: CompilationResolver + ?Sized,
{
    let mut nodes = BTreeMap::new();
    for definition in document.spec().nodes() {
        let interface = resolver.resolve_node(definition.uses());
        let version = resolver.resolve_node_version(definition.uses());
        let (Some(interface), Some(version)) = (interface, version) else {
            return Err(CompilationError::InconsistentResolution {
                node: definition.id().clone(),
                reference: definition.uses().clone(),
            });
        };
        nodes.insert(
            definition.id().clone(),
            NodeDraft {
                definition,
                version: version.clone(),
                interface: interface.clone(),
            },
        );
    }
    Ok(nodes)
}

fn dependency_order(
    document: &WorkflowDocument,
) -> (BTreeMap<NodeId, BTreeSet<NodeId>>, Vec<NodeId>) {
    let mut incoming = document
        .spec()
        .nodes()
        .iter()
        .map(|node| (node.id().clone(), BTreeSet::new()))
        .collect::<BTreeMap<_, _>>();
    let mut outgoing = incoming.clone();

    for edge in document.spec().edges() {
        if let (Endpoint::NodePort { node: source, .. }, Endpoint::NodePort { node: target, .. }) =
            (edge.from(), edge.to())
        {
            incoming
                .entry(target.clone())
                .or_default()
                .insert(source.clone());
            outgoing
                .entry(source.clone())
                .or_default()
                .insert(target.clone());
        }
    }

    let dependencies = incoming.clone();
    let mut ready = incoming
        .iter()
        .filter(|(_, predecessors)| predecessors.is_empty())
        .map(|(node, _)| node.clone())
        .collect::<BTreeSet<_>>();
    let mut order = Vec::with_capacity(incoming.len());

    while let Some(node) = ready.pop_first() {
        order.push(node.clone());
        if let Some(dependents) = outgoing.get(&node) {
            for dependent in dependents {
                if let Some(predecessors) = incoming.get_mut(dependent) {
                    predecessors.remove(&node);
                    if predecessors.is_empty() {
                        ready.insert(dependent.clone());
                    }
                }
            }
        }
    }

    (dependencies, order)
}

fn compile_edges(
    document: &WorkflowDocument,
    inputs: &BTreeMap<PortId, TypeRef>,
    nodes: &BTreeMap<NodeId, NodeDraft<'_>>,
) -> Result<Vec<PlanEdge>, CompilationError> {
    let mut definitions = document.spec().edges().iter().collect::<Vec<_>>();
    definitions.sort_by_key(|edge| endpoint_key(edge));
    definitions
        .into_iter()
        .map(|edge| {
            Ok(PlanEdge {
                from: edge.from().clone(),
                to: edge.to().clone(),
                payload_type: endpoint_type(edge.from(), inputs, nodes).ok_or_else(|| {
                    CompilationError::InconsistentTypeResolution {
                        path: edge.from().to_string(),
                    }
                })?,
            })
        })
        .collect()
}

fn compile_outputs(
    document: &WorkflowDocument,
    inputs: &BTreeMap<PortId, TypeRef>,
    nodes: &BTreeMap<NodeId, NodeDraft<'_>>,
) -> Result<BTreeMap<PortId, PlanOutput>, CompilationError> {
    document
        .spec()
        .outputs()
        .iter()
        .map(|(name, source)| {
            Ok((
                name.clone(),
                PlanOutput {
                    source: source.clone(),
                    payload_type: endpoint_type(source, inputs, nodes).ok_or_else(|| {
                        CompilationError::InconsistentTypeResolution {
                            path: format!("spec.outputs.{name}"),
                        }
                    })?,
                },
            ))
        })
        .collect()
}

fn endpoint_type(
    endpoint: &Endpoint,
    inputs: &BTreeMap<PortId, TypeRef>,
    nodes: &BTreeMap<NodeId, NodeDraft<'_>>,
) -> Option<TypeRef> {
    match endpoint {
        Endpoint::WorkflowInput(port) => inputs.get(port).cloned(),
        Endpoint::NodePort { node, port } => {
            nodes.get(node)?.interface.outputs().get(port).cloned()
        }
    }
}

fn endpoint_key(edge: &EdgeDefinition) -> (String, String) {
    (edge.from().to_string(), edge.to().to_string())
}

fn workflow_cache_key(
    document: &WorkflowDocument,
    lock_hash: LockHash,
    policy: &ExecutionPolicy,
) -> Result<PlanCacheKey, CompilationError> {
    let mut hasher = CanonicalHasher::new(b"logika.plan-cache.v1");
    hash_workflow_document(&mut hasher, document)?;
    hasher.bytes(lock_hash.as_bytes());
    hash_policy(&mut hasher, policy);
    Ok(PlanCacheKey(hasher.finish()))
}

fn hash_plan(
    document: &WorkflowDocument,
    inputs: &BTreeMap<PortId, TypeRef>,
    nodes: &[PlanNode],
    edges: &[PlanEdge],
    outputs: &BTreeMap<PortId, PlanOutput>,
    policy: &ExecutionPolicy,
) -> Result<PlanHash, CompilationError> {
    let mut hasher = CanonicalHasher::new(b"logika.execution-plan.v1");
    hasher.text(document.metadata().name());
    hasher.text(&document.metadata().version().to_string());
    hash_type_map(&mut hasher, inputs);
    hasher.usize(nodes.len());
    for node in nodes {
        hasher.text(node.id().as_str());
        hasher.text(node.implementation().name().as_str());
        hasher.text(&node.implementation().version().to_string());
        hash_interface(&mut hasher, node.interface());
        hash_configuration(&mut hasher, node.configuration())?;
        hasher.usize(node.dependencies().len());
        for dependency in node.dependencies() {
            hasher.text(dependency.as_str());
        }
        hash_policy(&mut hasher, node.policy());
    }
    hasher.usize(edges.len());
    for edge in edges {
        hasher.text(&edge.from().to_string());
        hasher.text(&edge.to().to_string());
        hash_type_ref(&mut hasher, edge.payload_type());
    }
    hasher.usize(outputs.len());
    for (name, output) in outputs {
        hasher.text(name.as_str());
        hasher.text(&output.source().to_string());
        hash_type_ref(&mut hasher, output.payload_type());
    }
    hash_policy(&mut hasher, policy);
    Ok(PlanHash(hasher.finish()))
}

fn hash_policy(hasher: &mut CanonicalHasher, policy: &ExecutionPolicy) {
    hasher.u32(policy.max_attempts().get());
    hash_duration(hasher, policy.initial_backoff());
    hash_duration(hasher, policy.max_backoff());
    hash_duration(hasher, policy.jitter());
    match policy.attempt_timeout() {
        Some(timeout) => {
            hasher.u8(1);
            hash_duration(hasher, timeout);
        }
        None => hasher.u8(0),
    }
    hasher.usize(policy.retriable_codes().len());
    for code in policy.retriable_codes() {
        hasher.text(code);
    }
}

fn hash_duration(hasher: &mut CanonicalHasher, duration: Duration) {
    hasher.u64(duration.as_secs());
    hasher.u32(duration.subsec_nanos());
}

fn hash_workflow_document(
    hasher: &mut CanonicalHasher,
    document: &WorkflowDocument,
) -> Result<(), CompilationError> {
    hasher.text(document.api_version().as_str());
    hasher.text("Workflow");
    hasher.text(document.metadata().name());
    hasher.text(&document.metadata().version().to_string());

    hasher.usize(document.spec().inputs().len());
    for (name, type_ref) in document.spec().inputs() {
        hasher.text(name.as_str());
        hasher.text(type_ref.name());
        hasher.u32(type_ref.version());
    }

    let mut nodes = document.spec().nodes().iter().collect::<Vec<_>>();
    nodes.sort_by(|left, right| left.id().cmp(right.id()));
    hasher.usize(nodes.len());
    for node in nodes {
        hasher.text(node.id().as_str());
        hasher.text(node.uses().plugin().as_str());
        hasher.text(&node.uses().version_requirement().to_string());
        hash_configuration(hasher, node.configuration())?;
    }

    let mut edges = document.spec().edges().iter().collect::<Vec<_>>();
    edges.sort_by_key(|edge| endpoint_key(edge));
    hasher.usize(edges.len());
    for edge in edges {
        hasher.text(&edge.from().to_string());
        hasher.text(&edge.to().to_string());
    }

    hasher.usize(document.spec().outputs().len());
    for (name, endpoint) in document.spec().outputs() {
        hasher.text(name.as_str());
        hasher.text(&endpoint.to_string());
    }
    Ok(())
}

fn hash_interface(hasher: &mut CanonicalHasher, interface: &NodeInterface) {
    hasher.usize(interface.inputs().len());
    for (name, input) in interface.inputs() {
        hasher.text(name.as_str());
        hash_type_ref(hasher, input.type_ref());
        hasher.u8(u8::from(input.is_required()));
        hasher.u8(match input.multiplicity() {
            ConnectionMultiplicity::Single => 0,
            ConnectionMultiplicity::Many => 1,
        });
    }
    hash_type_map(hasher, interface.outputs());
}

fn hash_type_map(hasher: &mut CanonicalHasher, types: &BTreeMap<PortId, TypeRef>) {
    hasher.usize(types.len());
    for (name, type_ref) in types {
        hasher.text(name.as_str());
        hash_type_ref(hasher, type_ref);
    }
}

fn hash_type_ref(hasher: &mut CanonicalHasher, type_ref: &TypeRef) {
    hasher.text(type_ref.name());
    hasher.u32(type_ref.version());
    hasher.text(&type_ref.fingerprint().to_string());
}

fn hash_configuration(
    hasher: &mut CanonicalHasher,
    configuration: &BTreeMap<String, Value>,
) -> Result<(), CompilationError> {
    let bytes =
        serde_json::to_vec(configuration).map_err(CompilationError::ConfigurationEncoding)?;
    hasher.bytes(&bytes);
    Ok(())
}

struct CanonicalHasher(Sha256);

impl CanonicalHasher {
    fn new(domain: &[u8]) -> Self {
        let mut hasher = Self(Sha256::new());
        hasher.bytes(domain);
        hasher
    }

    fn bytes(&mut self, value: &[u8]) {
        self.0.update((value.len() as u64).to_be_bytes());
        self.0.update(value);
    }

    fn text(&mut self, value: &str) {
        self.bytes(value.as_bytes());
    }

    fn usize(&mut self, value: usize) {
        self.0.update((value as u64).to_be_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.0.update(value.to_be_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.0.update(value.to_be_bytes());
    }

    fn u8(&mut self, value: u8) {
        self.0.update([value]);
    }

    fn finish(self) -> [u8; 32] {
        self.0.finalize().into()
    }
}

fn hex_digest(bytes: &[u8; 32]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(64);
    for byte in bytes {
        value.push(char::from(DIGITS[usize::from(byte >> 4)]));
        value.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    value
}
