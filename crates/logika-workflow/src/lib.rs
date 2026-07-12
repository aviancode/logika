//! Versioned workflow documents and source-aware YAML/JSON decoding for `logika`.

#![forbid(unsafe_code)]

mod decode;
mod model;
mod validation;

pub use decode::{
    DecodeError, DecodeErrorKind, DecodedWorkflow, DocumentFormat, Migration, SourcePosition,
    SourceSpan, decode_json, decode_yaml,
};
pub use model::{
    ApiVersion, ApiVersionParseError, CURRENT_API_VERSION, EdgeDefinition, Endpoint,
    NodeDefinition, NodeReference, ReferenceError, ReferenceKind, TypeReference, WorkflowDocument,
    WorkflowKind, WorkflowMetadata, WorkflowSpec,
};
pub use validation::{
    ConnectionMultiplicity, InputPort, NodeInterface, ValidationError, ValidationErrorKind,
    ValidationErrors, ValidationResolver, validate_workflow,
};
