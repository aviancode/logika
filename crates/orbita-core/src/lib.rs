//! Core domain types and traits for `orbita`.

#![forbid(unsafe_code)]

mod error;
mod identifiers;
mod schema;

pub use error::{Error, ErrorCategory, ErrorDetail, Result};
pub use identifiers::{
    IdentifierError, IdentifierKind, IdentifierViolation, NodeId, PluginId, PortId, RunId,
};
pub use schema::{
    EnumSchema, Payload, PayloadKind, PayloadValidationError, PayloadViolation, PrimitiveType,
    Schema, SchemaDefinition, SchemaError, SchemaField, SchemaFingerprint, SchemaViolation,
    StructSchema, TaggedUnionSchema, TaggedVariant, TypeRef,
};
