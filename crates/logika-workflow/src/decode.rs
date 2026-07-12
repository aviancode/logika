use std::{collections::BTreeMap, error, fmt};

use logika_core::{NodeId, PortId};
use serde::{Deserialize, Deserializer};
use serde_json::Value;

use crate::{
    ApiVersion, EdgeDefinition, Endpoint, NodeDefinition, NodeReference, TypeReference,
    WorkflowDocument, WorkflowKind, WorkflowMetadata, WorkflowSpec,
};

/// The source representation used to decode a workflow document.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DocumentFormat {
    /// JavaScript Object Notation.
    Json,
    /// YAML Ain't Markup Language.
    Yaml,
}

impl fmt::Display for DocumentFormat {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Json => "JSON",
            Self::Yaml => "YAML",
        })
    }
}

/// Broad classification of a workflow decoding failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DecodeErrorKind {
    /// The source is not syntactically valid JSON or YAML.
    Syntax,
    /// The source is valid data but does not match the selected schema.
    InvalidDocument,
    /// `apiVersion` names a version this crate cannot read.
    UnsupportedVersion,
}

impl DecodeErrorKind {
    /// Returns the stable machine-readable diagnostic code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Syntax => "workflow.syntax",
            Self::InvalidDocument => "workflow.invalid_document",
            Self::UnsupportedVersion => "workflow.unsupported_version",
        }
    }
}

/// A one-based source location with a zero-based UTF-8 byte offset.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourcePosition {
    offset: usize,
    line: usize,
    column: usize,
}

impl SourcePosition {
    /// Returns the zero-based UTF-8 byte offset.
    #[must_use]
    pub const fn offset(self) -> usize {
        self.offset
    }

    /// Returns the one-based line number.
    #[must_use]
    pub const fn line(self) -> usize {
        self.line
    }

    /// Returns the one-based column number.
    #[must_use]
    pub const fn column(self) -> usize {
        self.column
    }
}

/// A half-open source span suitable for editor diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceSpan {
    start: SourcePosition,
    end: SourcePosition,
}

impl SourceSpan {
    /// Returns the first position included in the span.
    #[must_use]
    pub const fn start(self) -> SourcePosition {
        self.start
    }

    /// Returns the first position after the span.
    #[must_use]
    pub const fn end(self) -> SourcePosition {
        self.end
    }
}

/// A source-aware workflow decoding failure.
#[derive(Debug)]
pub struct DecodeError {
    format: DocumentFormat,
    kind: DecodeErrorKind,
    message: String,
    path: Option<String>,
    span: Option<Box<SourceSpan>>,
    source: Option<Box<dyn error::Error + Send + Sync + 'static>>,
}

impl DecodeError {
    /// Returns the input format that was being decoded.
    #[must_use]
    pub const fn format(&self) -> DocumentFormat {
        self.format
    }

    /// Returns the broad failure kind.
    #[must_use]
    pub const fn kind(&self) -> DecodeErrorKind {
        self.kind
    }

    /// Returns the stable machine-readable diagnostic code.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.kind.code()
    }

    /// Returns a safe human-readable diagnostic.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns the Serde field path, when the error is associated with a value.
    #[must_use]
    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    /// Returns the source span, when the parser reported a location.
    #[must_use]
    pub fn span(&self) -> Option<SourceSpan> {
        self.span.as_deref().copied()
    }

    fn unsupported(format: DocumentFormat, source: &str, value: &str) -> Self {
        let span = find_api_version_span(source, value).map(Box::new);
        Self {
            format,
            kind: DecodeErrorKind::UnsupportedVersion,
            message: format!(
                "unsupported workflow API version {value:?}; expected {}",
                ApiVersion::current()
            ),
            path: Some("apiVersion".to_owned()),
            span,
            source: None,
        }
    }
}

impl fmt::Display for DecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} {}: {}",
            self.format,
            self.code(),
            self.message
        )?;
        if let Some(span) = &self.span {
            write!(formatter, " at {}:{}", span.start.line, span.start.column)?;
        }
        if let Some(path) = &self.path {
            write!(formatter, " ({path})")?;
        }
        Ok(())
    }
}

impl error::Error for DecodeError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn error::Error + 'static))
    }
}

/// A format migration applied while decoding a workflow.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Migration {
    from: ApiVersion,
    to: ApiVersion,
}

impl Migration {
    /// Returns the migration source version.
    #[must_use]
    pub const fn from(self) -> ApiVersion {
        self.from
    }

    /// Returns the migration target version.
    #[must_use]
    pub const fn to(self) -> ApiVersion {
        self.to
    }
}

/// A canonical document and provenance from its decoding process.
#[derive(Clone, Debug, PartialEq)]
pub struct DecodedWorkflow {
    document: WorkflowDocument,
    source_version: ApiVersion,
    migrations: Vec<Migration>,
}

impl DecodedWorkflow {
    /// Returns the canonical current-version document.
    #[must_use]
    pub const fn document(&self) -> &WorkflowDocument {
        &self.document
    }

    /// Consumes the result and returns its canonical document.
    #[must_use]
    pub fn into_document(self) -> WorkflowDocument {
        self.document
    }

    /// Returns the version found in the source document.
    #[must_use]
    pub const fn source_version(&self) -> ApiVersion {
        self.source_version
    }

    /// Returns migrations applied in order.
    #[must_use]
    pub fn migrations(&self) -> &[Migration] {
        &self.migrations
    }
}

/// Decodes a JSON workflow and migrates it to the current document version.
pub fn decode_json(source: &str) -> Result<DecodedWorkflow, DecodeError> {
    let version = decode_json_value::<VersionProbe>(source)?.api_version;
    decode_selected_json(source, version)
}

/// Decodes a YAML workflow and migrates it to the current document version.
pub fn decode_yaml(source: &str) -> Result<DecodedWorkflow, DecodeError> {
    let version = decode_yaml_value::<VersionProbe>(source)?.api_version;
    decode_selected_yaml(source, version)
}

fn decode_selected_json(source: &str, version: String) -> Result<DecodedWorkflow, DecodeError> {
    let version = version
        .parse::<ApiVersion>()
        .map_err(|_| DecodeError::unsupported(DocumentFormat::Json, source, &version))?;
    match version {
        ApiVersion::V1 => Ok(current(decode_json_value(source)?)),
        ApiVersion::V1Alpha1 => Ok(migrated(decode_json_value::<V1Alpha1Document>(source)?)),
    }
}

fn decode_selected_yaml(source: &str, version: String) -> Result<DecodedWorkflow, DecodeError> {
    let version = version
        .parse::<ApiVersion>()
        .map_err(|_| DecodeError::unsupported(DocumentFormat::Yaml, source, &version))?;
    match version {
        ApiVersion::V1 => Ok(current(decode_yaml_value(source)?)),
        ApiVersion::V1Alpha1 => Ok(migrated(decode_yaml_value::<V1Alpha1Document>(source)?)),
    }
}

fn current(document: WorkflowDocument) -> DecodedWorkflow {
    DecodedWorkflow {
        document,
        source_version: ApiVersion::V1,
        migrations: Vec::new(),
    }
}

fn migrated(document: V1Alpha1Document) -> DecodedWorkflow {
    DecodedWorkflow {
        document: document.into_current(),
        source_version: ApiVersion::V1Alpha1,
        migrations: vec![Migration {
            from: ApiVersion::V1Alpha1,
            to: ApiVersion::V1,
        }],
    }
}

fn decode_json_value<T>(source: &str) -> Result<T, DecodeError>
where
    T: for<'de> Deserialize<'de>,
{
    let mut deserializer = serde_json::Deserializer::from_str(source);
    serde_path_to_error::deserialize(&mut deserializer).map_err(|error| {
        let path = path_or_none(error.path().to_string());
        let inner = error.into_inner();
        let kind = match inner.classify() {
            serde_json::error::Category::Syntax | serde_json::error::Category::Eof => {
                DecodeErrorKind::Syntax
            }
            serde_json::error::Category::Data | serde_json::error::Category::Io => {
                DecodeErrorKind::InvalidDocument
            }
        };
        let span = span_from_line_column(source, inner.line(), inner.column()).map(Box::new);
        DecodeError {
            format: DocumentFormat::Json,
            kind,
            message: inner.to_string(),
            path,
            span,
            source: Some(Box::new(inner)),
        }
    })
}

fn decode_yaml_value<T>(source: &str) -> Result<T, DecodeError>
where
    T: for<'de> Deserialize<'de>,
{
    let deserializer = serde_yaml::Deserializer::from_str(source);
    serde_path_to_error::deserialize(deserializer).map_err(|error| {
        let path = path_or_none(error.path().to_string());
        let inner = error.into_inner();
        let span = inner
            .location()
            .and_then(|location| span_from_line_column(source, location.line(), location.column()))
            .map(Box::new);
        let message = inner.to_string();
        let kind = classify_yaml_error(&message, &path);
        DecodeError {
            format: DocumentFormat::Yaml,
            kind,
            message,
            path,
            span,
            source: Some(Box::new(inner)),
        }
    })
}

fn classify_yaml_error(message: &str, path: &Option<String>) -> DecodeErrorKind {
    if path.is_none()
        && (message.contains("did not find expected")
            || message.contains("while parsing")
            || message.contains("could not find expected"))
    {
        DecodeErrorKind::Syntax
    } else {
        DecodeErrorKind::InvalidDocument
    }
}

fn path_or_none(path: String) -> Option<String> {
    if path == "." || path.is_empty() {
        None
    } else {
        Some(path)
    }
}

fn span_from_line_column(source: &str, line: usize, column: usize) -> Option<SourceSpan> {
    if line == 0 || column == 0 {
        return None;
    }
    let line_start = source
        .split_inclusive('\n')
        .take(line.saturating_sub(1))
        .map(str::len)
        .sum::<usize>();
    let line_text = source.get(line_start..)?.split('\n').next()?;
    let relative = line_text
        .char_indices()
        .nth(column.saturating_sub(1))
        .map_or(line_text.len(), |(index, _)| index);
    let start_offset = line_start + relative;
    let end_offset = source
        .get(start_offset..)
        .and_then(|tail| tail.chars().next())
        .map_or(start_offset, |character| {
            start_offset + character.len_utf8()
        });
    Some(SourceSpan {
        start: position_at(source, start_offset),
        end: position_at(source, end_offset),
    })
}

fn find_api_version_span(source: &str, value: &str) -> Option<SourceSpan> {
    let key = source.find("apiVersion")?;
    let relative = source.get(key..)?.find(value)?;
    let start = key + relative;
    let end = start + value.len();
    Some(SourceSpan {
        start: position_at(source, start),
        end: position_at(source, end),
    })
}

fn position_at(source: &str, offset: usize) -> SourcePosition {
    let prefix = source.get(..offset).unwrap_or(source);
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let line_start = prefix.rfind('\n').map_or(0, |index| index + 1);
    let column = prefix[line_start..].chars().count() + 1;
    SourcePosition {
        offset,
        line,
        column,
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VersionProbe {
    api_version: String,
}

/// The alpha format used `config`; v1 renamed it to the less ambiguous `with`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct V1Alpha1Document {
    api_version: ApiVersion,
    kind: WorkflowKind,
    metadata: WorkflowMetadata,
    spec: V1Alpha1Spec,
}

impl V1Alpha1Document {
    fn into_current(self) -> WorkflowDocument {
        let _ = (self.api_version, self.kind);
        WorkflowDocument::new(self.metadata, self.spec.into_current())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct V1Alpha1Spec {
    #[serde(default, deserialize_with = "deserialize_type_map")]
    inputs: BTreeMap<PortId, TypeReference>,
    #[serde(default)]
    nodes: Vec<V1Alpha1Node>,
    #[serde(default)]
    edges: Vec<EdgeDefinition>,
    #[serde(default, deserialize_with = "deserialize_endpoint_map")]
    outputs: BTreeMap<PortId, Endpoint>,
}

impl V1Alpha1Spec {
    fn into_current(self) -> WorkflowSpec {
        WorkflowSpec::new(
            self.inputs,
            self.nodes
                .into_iter()
                .map(V1Alpha1Node::into_current)
                .collect(),
            self.edges,
            self.outputs,
        )
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct V1Alpha1Node {
    #[serde(deserialize_with = "deserialize_node_id")]
    id: NodeId,
    uses: NodeReference,
    #[serde(default)]
    config: BTreeMap<String, Value>,
}

impl V1Alpha1Node {
    fn into_current(self) -> NodeDefinition {
        NodeDefinition::new(self.id, self.uses).with_configuration(self.config)
    }
}

fn deserialize_node_id<'de, D>(deserializer: D) -> Result<NodeId, D::Error>
where
    D: Deserializer<'de>,
{
    NodeId::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
}

fn deserialize_type_map<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<PortId, TypeReference>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_port_map(deserializer)
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
                .map_err(serde::de::Error::custom)
        })
        .collect()
}
