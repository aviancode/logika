//! Core domain types and traits for `orbita`.

#![forbid(unsafe_code)]

mod error;
mod identifiers;

pub use error::{Error, ErrorCategory, ErrorDetail, Result};
pub use identifiers::{
    IdentifierError, IdentifierKind, IdentifierViolation, NodeId, PluginId, PortId, RunId,
};
