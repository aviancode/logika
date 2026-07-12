use std::{collections::BTreeMap, error, fmt, str::FromStr};

use logika_core::{NodeId, PluginId, PortId};
use semver::{Version, VersionReq};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use serde_json::Value;

const MAX_TYPE_NAME_LEN: usize = 255;

/// API version emitted by the current workflow serializer.
pub const CURRENT_API_VERSION: &str = "logika.dev/v1";

/// A workflow document format version understood by this crate.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[non_exhaustive]
pub enum ApiVersion {
    /// The pre-release format accepted only as a migration source.
    V1Alpha1,
    /// The stable workflow document format for Logika 0.1.
    V1,
}

impl ApiVersion {
    /// Returns the wire representation used by `apiVersion`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::V1Alpha1 => "logika.dev/v1alpha1",
            Self::V1 => CURRENT_API_VERSION,
        }
    }

    /// Returns the current canonical document version.
    #[must_use]
    pub const fn current() -> Self {
        Self::V1
    }
}

impl fmt::Display for ApiVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for ApiVersion {
    type Err = ApiVersionParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "logika.dev/v1alpha1" => Ok(Self::V1Alpha1),
            CURRENT_API_VERSION => Ok(Self::V1),
            _ => Err(ApiVersionParseError {
                value: value.to_owned(),
            }),
        }
    }
}

impl Serialize for ApiVersion {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ApiVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(de::Error::custom)
    }
}

/// Error returned for an unsupported workflow API version.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApiVersionParseError {
    value: String,
}

impl ApiVersionParseError {
    /// Returns the unsupported wire value.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }
}

impl fmt::Display for ApiVersionParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "unsupported workflow API version {:?}",
            self.value
        )
    }
}

impl error::Error for ApiVersionParseError {}

/// The Kubernetes-style document discriminator.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum WorkflowKind {
    /// A workflow graph document.
    Workflow,
}

/// A canonical `logika.dev/v1` workflow document.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowDocument {
    api_version: ApiVersion,
    kind: WorkflowKind,
    metadata: WorkflowMetadata,
    spec: WorkflowSpec,
}

impl WorkflowDocument {
    /// Creates a document in the current canonical API version.
    #[must_use]
    pub fn new(metadata: WorkflowMetadata, spec: WorkflowSpec) -> Self {
        Self {
            api_version: ApiVersion::current(),
            kind: WorkflowKind::Workflow,
            metadata,
            spec,
        }
    }

    /// Returns the canonical format version.
    #[must_use]
    pub const fn api_version(&self) -> ApiVersion {
        self.api_version
    }

    /// Returns the document kind.
    #[must_use]
    pub const fn kind(&self) -> WorkflowKind {
        self.kind
    }

    /// Returns workflow identity metadata.
    #[must_use]
    pub const fn metadata(&self) -> &WorkflowMetadata {
        &self.metadata
    }

    /// Returns the graph specification.
    #[must_use]
    pub const fn spec(&self) -> &WorkflowSpec {
        &self.spec
    }
}

/// Stable identity of a workflow definition.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowMetadata {
    name: String,
    version: Version,
}

impl WorkflowMetadata {
    /// Creates workflow metadata.
    #[must_use]
    pub fn new(name: impl Into<String>, version: Version) -> Self {
        Self {
            name: name.into(),
            version,
        }
    }

    /// Returns the host-facing workflow name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the semantic workflow version.
    #[must_use]
    pub const fn version(&self) -> &Version {
        &self.version
    }
}

/// Portable graph data contained by a workflow document.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowSpec {
    #[serde(
        default,
        serialize_with = "serialize_type_map",
        deserialize_with = "deserialize_type_map"
    )]
    inputs: BTreeMap<PortId, TypeReference>,
    #[serde(default)]
    nodes: Vec<NodeDefinition>,
    #[serde(default)]
    edges: Vec<EdgeDefinition>,
    #[serde(
        default,
        serialize_with = "serialize_endpoint_map",
        deserialize_with = "deserialize_endpoint_map"
    )]
    outputs: BTreeMap<PortId, Endpoint>,
}

impl WorkflowSpec {
    /// Creates a graph specification from its four document sections.
    #[must_use]
    pub fn new(
        inputs: BTreeMap<PortId, TypeReference>,
        nodes: Vec<NodeDefinition>,
        edges: Vec<EdgeDefinition>,
        outputs: BTreeMap<PortId, Endpoint>,
    ) -> Self {
        Self {
            inputs,
            nodes,
            edges,
            outputs,
        }
    }

    /// Returns declared workflow inputs.
    #[must_use]
    pub const fn inputs(&self) -> &BTreeMap<PortId, TypeReference> {
        &self.inputs
    }

    /// Returns node invocations in document order.
    #[must_use]
    pub fn nodes(&self) -> &[NodeDefinition] {
        &self.nodes
    }

    /// Returns graph edges in document order.
    #[must_use]
    pub fn edges(&self) -> &[EdgeDefinition] {
        &self.edges
    }

    /// Returns declared workflow outputs.
    #[must_use]
    pub const fn outputs(&self) -> &BTreeMap<PortId, Endpoint> {
        &self.outputs
    }
}

/// A single node invocation in a workflow.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NodeDefinition {
    #[serde(with = "node_id_serde")]
    id: NodeId,
    uses: NodeReference,
    #[serde(default, rename = "with", skip_serializing_if = "BTreeMap::is_empty")]
    configuration: BTreeMap<String, Value>,
}

impl NodeDefinition {
    /// Creates a node invocation without configuration.
    #[must_use]
    pub fn new(id: NodeId, uses: NodeReference) -> Self {
        Self {
            id,
            uses,
            configuration: BTreeMap::new(),
        }
    }

    /// Replaces the node configuration.
    #[must_use]
    pub fn with_configuration(mut self, configuration: BTreeMap<String, Value>) -> Self {
        self.configuration = configuration;
        self
    }

    /// Returns the stable node identifier.
    #[must_use]
    pub const fn id(&self) -> &NodeId {
        &self.id
    }

    /// Returns the requested node implementation.
    #[must_use]
    pub const fn uses(&self) -> &NodeReference {
        &self.uses
    }

    /// Returns portable, non-secret node configuration.
    #[must_use]
    pub const fn configuration(&self) -> &BTreeMap<String, Value> {
        &self.configuration
    }
}

/// A directed connection between two workflow endpoints.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EdgeDefinition {
    from: Endpoint,
    to: Endpoint,
}

impl EdgeDefinition {
    /// Creates a directed edge.
    #[must_use]
    pub const fn new(from: Endpoint, to: Endpoint) -> Self {
        Self { from, to }
    }

    /// Returns the source endpoint.
    #[must_use]
    pub const fn from(&self) -> &Endpoint {
        &self.from
    }

    /// Returns the destination endpoint.
    #[must_use]
    pub const fn to(&self) -> &Endpoint {
        &self.to
    }
}

/// A schema identity as written in a workflow, for example `acme.order@1`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeReference {
    name: String,
    version: u32,
}

impl TypeReference {
    /// Creates a portable type reference.
    pub fn new(name: impl Into<String>, version: u32) -> Result<Self, ReferenceError> {
        let name = name.into();
        if let Err(message) = validate_type_name(&name) {
            return Err(ReferenceError::new(ReferenceKind::Type, &name, message));
        }
        if version == 0 {
            return Err(ReferenceError::new(
                ReferenceKind::Type,
                format!("{name}@{version}"),
                "type version must be greater than zero",
            ));
        }
        Ok(Self { name, version })
    }

    /// Returns the stable type name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the positive schema version.
    #[must_use]
    pub const fn version(&self) -> u32 {
        self.version
    }
}

impl fmt::Display for TypeReference {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}@{}", self.name, self.version)
    }
}

impl FromStr for TypeReference {
    type Err = ReferenceError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let Some((name, version)) = value.rsplit_once('@') else {
            return Err(ReferenceError::new(
                ReferenceKind::Type,
                value,
                "expected `<type-name>@<positive-version>`",
            ));
        };
        let version = version.parse::<u32>().map_err(|_| {
            ReferenceError::new(
                ReferenceKind::Type,
                value,
                "schema version must be a positive integer",
            )
        })?;
        Self::new(name, version).map_err(|error| error.with_value(value))
    }
}

/// A version-constrained node implementation, for example `acme.crm/enrich@^2`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NodeReference {
    plugin: PluginId,
    version_requirement: VersionReq,
}

impl NodeReference {
    /// Creates a node implementation reference.
    #[must_use]
    pub const fn new(plugin: PluginId, version_requirement: VersionReq) -> Self {
        Self {
            plugin,
            version_requirement,
        }
    }

    /// Returns the namespaced implementation identifier.
    #[must_use]
    pub const fn plugin(&self) -> &PluginId {
        &self.plugin
    }

    /// Returns the requested compatible version set.
    #[must_use]
    pub const fn version_requirement(&self) -> &VersionReq {
        &self.version_requirement
    }
}

impl fmt::Display for NodeReference {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}@{}", self.plugin, self.version_requirement)
    }
}

impl FromStr for NodeReference {
    type Err = ReferenceError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let Some((plugin, requirement)) = value.rsplit_once('@') else {
            return Err(ReferenceError::new(
                ReferenceKind::Node,
                value,
                "expected `<node-name>@<semver-requirement>`",
            ));
        };
        let plugin = PluginId::new(plugin).map_err(|source| {
            ReferenceError::new(ReferenceKind::Node, value, source.to_string())
        })?;
        let version_requirement = VersionReq::parse(requirement).map_err(|source| {
            ReferenceError::new(ReferenceKind::Node, value, source.to_string())
        })?;
        Ok(Self::new(plugin, version_requirement))
    }
}

/// A source or destination address in a workflow graph.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Endpoint {
    /// A declared workflow input, encoded as `$inputs.<port>`.
    WorkflowInput(PortId),
    /// A node port, encoded as `<node>.<port>`.
    NodePort {
        /// Node that owns the port.
        node: NodeId,
        /// Input or output port on the node.
        port: PortId,
    },
}

impl Endpoint {
    /// Creates a workflow input endpoint.
    #[must_use]
    pub const fn workflow_input(port: PortId) -> Self {
        Self::WorkflowInput(port)
    }

    /// Creates a node port endpoint.
    #[must_use]
    pub const fn node_port(node: NodeId, port: PortId) -> Self {
        Self::NodePort { node, port }
    }
}

impl fmt::Display for Endpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WorkflowInput(port) => write!(formatter, "$inputs.{port}"),
            Self::NodePort { node, port } => write!(formatter, "{node}.{port}"),
        }
    }
}

impl FromStr for Endpoint {
    type Err = ReferenceError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if let Some(port) = value.strip_prefix("$inputs.") {
            return PortId::new(port)
                .map(Self::WorkflowInput)
                .map_err(|source| {
                    ReferenceError::new(ReferenceKind::Endpoint, value, source.to_string())
                });
        }
        let Some((node, port)) = value.split_once('.') else {
            return Err(ReferenceError::new(
                ReferenceKind::Endpoint,
                value,
                "expected `$inputs.<port>` or `<node>.<port>`",
            ));
        };
        let node = NodeId::new(node).map_err(|source| {
            ReferenceError::new(ReferenceKind::Endpoint, value, source.to_string())
        })?;
        let port = PortId::new(port).map_err(|source| {
            ReferenceError::new(ReferenceKind::Endpoint, value, source.to_string())
        })?;
        Ok(Self::NodePort { node, port })
    }
}

/// Identifies the workflow reference grammar that rejected a value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ReferenceKind {
    /// A type and schema version reference.
    Type,
    /// A node implementation and SemVer requirement.
    Node,
    /// A workflow input or node port endpoint.
    Endpoint,
}

impl fmt::Display for ReferenceKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Type => "type",
            Self::Node => "node",
            Self::Endpoint => "endpoint",
        })
    }
}

/// Error returned for a malformed inline workflow reference.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferenceError {
    kind: ReferenceKind,
    value: String,
    message: String,
}

impl ReferenceError {
    fn new(kind: ReferenceKind, value: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind,
            value: value.into(),
            message: message.into(),
        }
    }

    fn with_value(mut self, value: impl Into<String>) -> Self {
        self.value = value.into();
        self
    }

    /// Returns the rejected grammar category.
    #[must_use]
    pub const fn kind(&self) -> ReferenceKind {
        self.kind
    }

    /// Returns the rejected wire value.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }
}

impl fmt::Display for ReferenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "invalid {} reference {:?}: {}",
            self.kind, self.value, self.message
        )
    }
}

impl error::Error for ReferenceError {}

fn validate_type_name(name: &str) -> Result<(), String> {
    let mut characters = name.char_indices();
    let Some((_, first)) = characters.next() else {
        return Err("type name cannot be empty".to_owned());
    };
    if name.len() > MAX_TYPE_NAME_LEN {
        return Err(format!(
            "type name is {} bytes, but the limit is {MAX_TYPE_NAME_LEN}",
            name.len()
        ));
    }
    if !first.is_ascii_alphanumeric() {
        return Err(format!("type name starts with invalid character {first:?}"));
    }

    let mut after_separator = false;
    for (index, character) in characters {
        let separator = matches!(character, '.' | '/');
        let allowed =
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.' | '/');
        if !allowed || (separator && after_separator) {
            return Err(format!(
                "character {character:?} at byte {index} is not allowed in a type name"
            ));
        }
        after_separator = separator;
    }
    if after_separator {
        return Err("type name cannot end with a namespace separator".to_owned());
    }
    Ok(())
}

mod node_id_serde {
    use super::*;

    pub fn serialize<S>(value: &NodeId, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(value.as_str())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<NodeId, D::Error>
    where
        D: Deserializer<'de>,
    {
        NodeId::new(String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

fn serialize_type_map<S>(
    values: &BTreeMap<PortId, TypeReference>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let wire = values
        .iter()
        .map(|(key, value)| (key.as_str(), value))
        .collect::<BTreeMap<_, _>>();
    wire.serialize(serializer)
}

fn deserialize_type_map<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<PortId, TypeReference>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_port_map(deserializer)
}

fn serialize_endpoint_map<S>(
    values: &BTreeMap<PortId, Endpoint>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let wire = values
        .iter()
        .map(|(key, value)| (key.as_str(), value))
        .collect::<BTreeMap<_, _>>();
    wire.serialize(serializer)
}

fn deserialize_endpoint_map<'de, D>(deserializer: D) -> Result<BTreeMap<PortId, Endpoint>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_port_map(deserializer)
}

fn deserialize_port_map<'de, D, T>(deserializer: D) -> Result<BTreeMap<PortId, T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    BTreeMap::<String, T>::deserialize(deserializer)?
        .into_iter()
        .map(|(key, value)| {
            PortId::new(key)
                .map(|key| (key, value))
                .map_err(de::Error::custom)
        })
        .collect()
}

macro_rules! impl_string_serde {
    ($type:ty) => {
        impl Serialize for $type {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(&self.to_string())
            }
        }

        impl<'de> Deserialize<'de> for $type {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                String::deserialize(deserializer)?
                    .parse()
                    .map_err(de::Error::custom)
            }
        }
    };
}

impl_string_serde!(TypeReference);
impl_string_serde!(NodeReference);
impl_string_serde!(Endpoint);
