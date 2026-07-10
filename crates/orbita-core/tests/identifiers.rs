//! Public contract tests for core identifiers and classified errors.

use std::{error::Error as _, str::FromStr};

use orbita_core::{
    Error, ErrorCategory, ErrorDetail, IdentifierKind, IdentifierViolation, NodeId, PluginId,
    PortId, RunId,
};

#[test]
fn parses_portable_local_identifiers() {
    let node = NodeId::from_str("validate-order");
    let port = PortId::try_from("valid_order");

    assert_eq!(node.as_ref().map(NodeId::as_str), Ok("validate-order"));
    assert_eq!(port.as_ref().map(PortId::as_str), Ok("valid_order"));
}

#[test]
fn accepts_namespaced_plugins_and_host_assigned_runs() {
    let plugin = PluginId::new("acme.crm/enrichment");
    let uuid_run = RunId::new("550e8400-e29b-41d4-a716-446655440000");
    let namespaced_run = RunId::new("orders:01J3E4T5Y6Z7");

    assert!(plugin.is_ok());
    assert!(uuid_run.is_ok());
    assert!(namespaced_run.is_ok());
}

#[test]
fn preserves_owned_identifier_text() {
    let node = NodeId::try_from(String::from("enrich"));
    let Ok(node) = node else {
        panic!("valid node identifier was rejected");
    };

    assert_eq!(String::from(node), "enrich");
}

#[test]
fn rejects_empty_identifiers_with_domain_context() {
    let error = NodeId::new("").err();
    let Some(error) = error else {
        panic!("empty node identifier was accepted");
    };

    assert_eq!(error.kind(), IdentifierKind::Node);
    assert_eq!(error.value(), "");
    assert_eq!(error.violation(), &IdentifierViolation::Empty);
}

#[test]
fn rejects_invalid_start_and_non_ascii_characters() {
    let start_error = PortId::new("_input").err();
    let character_error = NodeId::new("validА").err();

    assert!(matches!(
        start_error.as_ref().map(|error| error.violation()),
        Some(IdentifierViolation::InvalidStart { found: '_' })
    ));
    assert!(matches!(
        character_error.as_ref().map(|error| error.violation()),
        Some(IdentifierViolation::InvalidCharacter {
            index: 5,
            found: 'А'
        })
    ));
}

#[test]
fn enforces_local_identifier_length_boundary() {
    let maximum = format!("n{}", "x".repeat(127));
    let over_limit = format!("n{}", "x".repeat(128));

    assert!(NodeId::new(maximum).is_ok());

    let error = NodeId::new(over_limit).err();
    assert!(matches!(
        error.as_ref().map(|error| error.violation()),
        Some(IdentifierViolation::TooLong {
            max: 128,
            actual: 129
        })
    ));
}

#[test]
fn rejects_empty_plugin_namespace_segments() {
    for value in ["acme..crm", "acme//crm", "acme.", "acme/"] {
        let error = PluginId::new(value).err();
        assert!(matches!(
            error.as_ref().map(|error| error.violation()),
            Some(IdentifierViolation::EmptySegment { .. })
        ));
    }
}

#[test]
fn maps_identifier_failures_to_public_validation_errors() {
    let identifier_error = NodeId::new("bad node").err();
    let Some(identifier_error) = identifier_error else {
        panic!("invalid node identifier was accepted");
    };
    let error = Error::from(identifier_error);

    assert_eq!(error.category(), ErrorCategory::Validation);
    assert_eq!(error.code(), "core.invalid_identifier");
    assert!(error.message().contains("node"));
    assert!(error.source().is_some());
}

#[derive(Debug)]
struct SensitiveSource;

impl std::fmt::Display for SensitiveSource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("internal secret")
    }
}

impl std::error::Error for SensitiveSource {}

#[test]
fn public_error_display_does_not_expose_its_source() {
    let detail = ErrorDetail::new("plugin.execute_failed", "plugin execution failed")
        .with_source(SensitiveSource);
    let error = Error::new(ErrorCategory::Plugin, detail);
    let displayed = error.to_string();

    assert_eq!(
        displayed,
        "plugin (plugin.execute_failed): plugin execution failed"
    );
    assert!(!displayed.contains("internal secret"));

    let hidden_source = error.source().and_then(std::error::Error::source);
    assert_eq!(
        hidden_source.map(ToString::to_string).as_deref(),
        Some("internal secret")
    );
}

#[test]
fn constructs_every_required_error_category() {
    let categories = [
        ErrorCategory::Validation,
        ErrorCategory::Resolution,
        ErrorCategory::Schema,
        ErrorCategory::Plugin,
        ErrorCategory::CapabilityDenied,
        ErrorCategory::Timeout,
        ErrorCategory::Cancelled,
        ErrorCategory::Storage,
        ErrorCategory::UserNode,
    ];

    for category in categories {
        let error = Error::new(category, ErrorDetail::new("test.code", "test diagnostic"));
        assert_eq!(error.category(), category);
        assert_eq!(error.category().to_string(), category.as_str());
    }
}
