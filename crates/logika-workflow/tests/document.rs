//! Public contract tests for versioned workflow documents.

use logika_workflow::{
    ApiVersion, CURRENT_API_VERSION, DecodeErrorKind, DocumentFormat, Endpoint, ReferenceKind,
    TypeReference, decode_json, decode_yaml,
};

const V1_YAML: &str = include_str!("fixtures/workflow.v1.yaml");
const V1_JSON: &str = include_str!("fixtures/workflow.v1.json");
const V1_ALPHA1_YAML: &str = include_str!("fixtures/workflow.v1alpha1.yaml");

#[test]
fn yaml_and_json_decode_to_the_same_canonical_document() {
    let yaml = decode_yaml(V1_YAML);
    let json = decode_json(V1_JSON);
    let (Ok(yaml), Ok(json)) = (yaml, json) else {
        panic!("valid format fixtures were rejected");
    };

    assert_eq!(yaml.document(), json.document());
    assert_eq!(yaml.document().api_version(), ApiVersion::current());
    assert_eq!(CURRENT_API_VERSION, "logika.dev/v1");
    assert_eq!(yaml.document().metadata().name(), "enrich-order");
    assert_eq!(yaml.document().spec().nodes().len(), 2);
    assert_eq!(yaml.document().spec().edges().len(), 2);
    assert!(yaml.migrations().is_empty());
    assert!(json.migrations().is_empty());
}

#[test]
fn serde_serialization_round_trips_the_canonical_model() {
    let decoded = decode_yaml(V1_YAML);
    let Ok(decoded) = decoded else {
        panic!("valid YAML fixture was rejected");
    };
    let encoded_json = serde_json::to_string_pretty(decoded.document());
    let encoded_yaml = serde_yaml::to_string(decoded.document());
    let (Ok(encoded_json), Ok(encoded_yaml)) = (encoded_json, encoded_yaml) else {
        panic!("canonical document could not be serialized");
    };

    let reparsed_json = decode_json(&encoded_json);
    let reparsed_yaml = decode_yaml(&encoded_yaml);
    let (Ok(reparsed_json), Ok(reparsed_yaml)) = (reparsed_json, reparsed_yaml) else {
        panic!("serialized canonical document could not be decoded");
    };
    assert_eq!(reparsed_json.document(), decoded.document());
    assert_eq!(reparsed_yaml.document(), decoded.document());
}

#[test]
fn alpha_document_is_migrated_and_records_provenance() {
    let decoded = decode_yaml(V1_ALPHA1_YAML);
    let Ok(decoded) = decoded else {
        panic!("supported alpha fixture was rejected");
    };

    assert_eq!(decoded.source_version(), ApiVersion::V1Alpha1);
    assert_eq!(decoded.document().api_version(), ApiVersion::V1);
    assert_eq!(decoded.migrations().len(), 1);
    assert_eq!(decoded.migrations()[0].from(), ApiVersion::V1Alpha1);
    assert_eq!(decoded.migrations()[0].to(), ApiVersion::V1);
    assert_eq!(
        decoded.document().spec().nodes()[0]
            .configuration()
            .get("strict"),
        Some(&serde_json::Value::Bool(true))
    );

    let encoded = serde_yaml::to_string(decoded.document());
    let Ok(encoded) = encoded else {
        panic!("migrated document could not be encoded");
    };
    assert!(encoded.contains("apiVersion: logika.dev/v1"));
    assert!(encoded.contains("with:"));
    assert!(!encoded.contains("config:"));
}

#[test]
fn unsupported_versions_report_a_field_path_and_source_span() {
    let source = V1_YAML.replace(CURRENT_API_VERSION, "logika.dev/v2");
    let error = decode_yaml(&source);
    let Err(error) = error else {
        panic!("unsupported workflow version was accepted");
    };

    assert_eq!(error.kind(), DecodeErrorKind::UnsupportedVersion);
    assert_eq!(error.code(), "workflow.unsupported_version");
    assert_eq!(error.path(), Some("apiVersion"));
    assert_eq!(error.format(), DocumentFormat::Yaml);
    let Some(span) = error.span() else {
        panic!("unsupported version did not include a source span");
    };
    assert_eq!(span.start().line(), 1);
    assert_eq!(span.start().column(), 13);
    assert_eq!(
        &source[span.start().offset()..span.end().offset()],
        "logika.dev/v2"
    );
}

#[test]
fn invalid_references_report_the_nested_serde_path_and_position() {
    let source = V1_JSON.replace("validate.order", "not-an-endpoint");
    let error = decode_json(&source);
    let Err(error) = error else {
        panic!("invalid endpoint was accepted");
    };

    assert_eq!(error.kind(), DecodeErrorKind::InvalidDocument);
    assert_eq!(error.path(), Some("spec.edges[0].to"));
    assert!(error.message().contains("invalid endpoint reference"));
    let Some(span) = error.span() else {
        panic!("JSON data error did not include a source position");
    };
    assert!(span.start().line() > 1);
    assert!(span.start().column() > 1);
}

#[test]
fn malformed_yaml_is_rejected_with_a_source_position() {
    let source = concat!(
        "apiVersion: logika.dev/v1\n",
        "kind: Workflow\n",
        "metadata: [\n",
        "spec: {}\n",
    );
    let error = decode_yaml(source);
    let Err(error) = error else {
        panic!("malformed YAML was accepted");
    };

    assert!(matches!(
        error.kind(),
        DecodeErrorKind::Syntax | DecodeErrorKind::InvalidDocument
    ));
    assert!(error.span().is_some());
}

#[test]
fn inline_reference_types_are_typed_and_reject_invalid_values() {
    let type_reference = "acme.order@2".parse::<TypeReference>();
    let endpoint = "$inputs.order".parse::<Endpoint>();
    let invalid = "missing-version".parse::<TypeReference>();
    let (Ok(type_reference), Ok(endpoint), Err(invalid)) = (type_reference, endpoint, invalid)
    else {
        panic!("reference grammar produced unexpected results");
    };

    assert_eq!(type_reference.name(), "acme.order");
    assert_eq!(type_reference.version(), 2);
    assert_eq!(endpoint.to_string(), "$inputs.order");
    assert_eq!(invalid.kind(), ReferenceKind::Type);
    assert!("acme..order@1".parse::<TypeReference>().is_err());
}
