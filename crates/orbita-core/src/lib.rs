//! Core domain types and traits for `orbita`.

#![forbid(unsafe_code)]

mod identifiers;

pub use identifiers::{
    IdentifierError, IdentifierKind, IdentifierViolation, NodeId, PluginId, PortId, RunId,
};
