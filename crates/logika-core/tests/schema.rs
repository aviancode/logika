//! Public contract tests for canonical schemas and payload validation.

use std::collections::BTreeMap;

use logika_core::{
    EnumSchema, Error, ErrorCategory, Payload, PayloadKind, PayloadViolation, PrimitiveType,
    Schema, SchemaDefinition, SchemaField, SchemaViolation, StructSchema, TaggedUnionSchema,
    TaggedVariant, TypeRef,
};

fn primitive(primitive: PrimitiveType) -> SchemaDefinition {
    SchemaDefinition::primitive(primitive)
}

fn object(entries: impl IntoIterator<Item = (&'static str, Payload)>) -> Payload {
    Payload::Object(
        entries
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect::<BTreeMap<_, _>>(),
    )
}

fn order_type(fields: Vec<SchemaField>) -> Result<TypeRef, logika_core::SchemaError> {
    TypeRef::new(
        "acme.order",
        1,
        SchemaDefinition::Struct(StructSchema::new(fields)),
    )
}

#[test]
fn canonicalizes_field_order_and_produces_a_golden_sha256() {
    let first = order_type(vec![
        SchemaField::optional("note", primitive(PrimitiveType::String)),
        SchemaField::required("id", primitive(PrimitiveType::U64)),
    ]);
    let second = order_type(vec![
        SchemaField::required("id", primitive(PrimitiveType::U64)),
        SchemaField::optional("note", primitive(PrimitiveType::String)),
    ]);
    let (Ok(first), Ok(second)) = (first, second) else {
        panic!("valid order schema was rejected");
    };

    assert_eq!(first.canonical(), second.canonical());
    assert_eq!(first.fingerprint(), second.fingerprint());
    assert_eq!(
        first.canonical(),
        concat!(
            r#"{"name":"acme.order","version":1,"schema":{"type":"struct","fields":["#,
            r#"{"name":"id","required":true,"schema":{"type":"u64"}},"#,
            r#"{"name":"note","required":false,"schema":{"type":"string"}}]}}"#,
        )
    );
    assert_eq!(
        first.fingerprint().to_hex(),
        "940204b0e103264fc1148ad31ca331f855e6a8ba01e0cbaf183b2ad95e67acb6"
    );
    assert_eq!(first.fingerprint().as_bytes().len(), 32);
}

#[test]
fn canonical_model_covers_nested_collections_enums_optional_values_and_unions() {
    let schema = TypeRef::new(
        "acme.event",
        3,
        SchemaDefinition::TaggedUnion(TaggedUnionSchema::new([
            TaggedVariant::new(
                "updated",
                SchemaDefinition::Struct(StructSchema::new([
                    SchemaField::required(
                        "labels",
                        SchemaDefinition::map(SchemaDefinition::array(primitive(
                            PrimitiveType::String,
                        ))),
                    ),
                    SchemaField::optional(
                        "state",
                        SchemaDefinition::optional(SchemaDefinition::Enum(EnumSchema::new([
                            "pending", "complete",
                        ]))),
                    ),
                ])),
            ),
            TaggedVariant::new("deleted", SchemaDefinition::Struct(StructSchema::new([]))),
        ])),
    );
    let Ok(schema) = schema else {
        panic!("valid nested schema was rejected");
    };

    assert!(schema.canonical().contains(r#""type":"tagged_union""#));
    assert!(schema.canonical().contains(r#""type":"map""#));
    assert!(schema.canonical().contains(r#""type":"array""#));
    assert!(schema.canonical().contains(r#""type":"optional""#));
    assert!(schema.canonical().contains(r#""type":"enum""#));
    assert!(schema.canonical().find("deleted") < schema.canonical().find("updated"));
}

#[test]
fn strict_compatibility_includes_name_version_and_structure() {
    let base = order_type(vec![SchemaField::required(
        "id",
        primitive(PrimitiveType::U64),
    )]);
    let same = order_type(vec![SchemaField::required(
        "id",
        primitive(PrimitiveType::U64),
    )]);
    let changed_structure = order_type(vec![SchemaField::required(
        "id",
        primitive(PrimitiveType::String),
    )]);
    let changed_version = TypeRef::new(
        "acme.order",
        2,
        SchemaDefinition::Struct(StructSchema::new([SchemaField::required(
            "id",
            primitive(PrimitiveType::U64),
        )])),
    );
    let changed_name = TypeRef::new(
        "acme.invoice",
        1,
        SchemaDefinition::Struct(StructSchema::new([SchemaField::required(
            "id",
            primitive(PrimitiveType::U64),
        )])),
    );
    let (Ok(base), Ok(same), Ok(changed_structure), Ok(changed_version), Ok(changed_name)) =
        (base, same, changed_structure, changed_version, changed_name)
    else {
        panic!("valid compatibility fixture was rejected");
    };

    assert!(base.is_compatible_with(&same));
    assert!(same.is_compatible_with(&base));
    assert!(!base.is_compatible_with(&changed_structure));
    assert!(!base.is_compatible_with(&changed_version));
    assert!(!base.is_compatible_with(&changed_name));
}

#[test]
fn rejects_invalid_type_identity_and_duplicate_schema_members() {
    let zero_version = TypeRef::new("acme.order", 0, primitive(PrimitiveType::String));
    let invalid_name = TypeRef::new("acme..order", 1, primitive(PrimitiveType::String));
    let duplicate = order_type(vec![
        SchemaField::required("id", primitive(PrimitiveType::U64)),
        SchemaField::optional("id", primitive(PrimitiveType::String)),
    ]);

    assert!(matches!(
        zero_version.as_ref().map_err(|error| error.violation()),
        Err(SchemaViolation::VersionZero)
    ));
    assert!(matches!(
        invalid_name.as_ref().map_err(|error| error.violation()),
        Err(SchemaViolation::InvalidTypeNameCharacter { .. })
    ));
    assert!(matches!(
        duplicate.as_ref().map_err(|error| error.violation()),
        Err(SchemaViolation::DuplicateMember {
            member: "field",
            name,
        }) if name == "id"
    ));
}

#[test]
fn validates_structured_payloads_at_the_boundary() {
    let schema = order_type(vec![
        SchemaField::required("id", primitive(PrimitiveType::U64)),
        SchemaField::required(
            "state",
            SchemaDefinition::Enum(EnumSchema::new(["pending", "complete"])),
        ),
        SchemaField::required(
            "tags",
            SchemaDefinition::array(primitive(PrimitiveType::String)),
        ),
        SchemaField::required(
            "metadata",
            SchemaDefinition::map(primitive(PrimitiveType::String)),
        ),
        SchemaField::optional(
            "note",
            SchemaDefinition::optional(primitive(PrimitiveType::String)),
        ),
    ]);
    let Ok(schema) = schema else {
        panic!("valid payload schema was rejected");
    };

    let valid = object([
        ("id", Payload::U64(42)),
        ("state", Payload::String("pending".to_owned())),
        (
            "tags",
            Payload::Array(vec![Payload::String("priority".to_owned())]),
        ),
        (
            "metadata",
            object([("source", Payload::String("api".to_owned()))]),
        ),
        ("note", Payload::Null),
    ]);

    assert_eq!(schema.validate_payload(&valid), Ok(()));
}

#[test]
fn payload_errors_report_the_precise_nested_path() {
    let schema = order_type(vec![
        SchemaField::required("id", primitive(PrimitiveType::U64)),
        SchemaField::required(
            "tags",
            SchemaDefinition::array(primitive(PrimitiveType::String)),
        ),
    ]);
    let Ok(schema) = schema else {
        panic!("valid payload schema was rejected");
    };

    let wrong_nested_type = object([
        ("id", Payload::U64(42)),
        (
            "tags",
            Payload::Array(vec![
                Payload::String("valid".to_owned()),
                Payload::Bool(false),
            ]),
        ),
    ]);
    let error = schema.validate_payload(&wrong_nested_type);
    let Err(error) = error else {
        panic!("invalid nested payload was accepted");
    };

    assert_eq!(error.path(), "$/tags/1");
    assert_eq!(
        error.violation(),
        &PayloadViolation::TypeMismatch {
            expected: "string",
            actual: PayloadKind::Bool,
        }
    );

    let missing = schema.validate_payload(&object([("tags", Payload::Array(Vec::new()))]));
    let Err(missing) = missing else {
        panic!("payload with a missing required field was accepted");
    };
    assert_eq!(missing.path(), "$/id");
    assert!(matches!(
        missing.violation(),
        PayloadViolation::MissingRequiredField { field } if field == "id"
    ));

    let unknown = schema.validate_payload(&object([
        ("id", Payload::U64(42)),
        ("tags", Payload::Array(Vec::new())),
        ("extra", Payload::Bool(true)),
    ]));
    let Err(unknown) = unknown else {
        panic!("payload with an unknown field was accepted");
    };
    assert_eq!(unknown.path(), "$/extra");
}

#[test]
fn validates_enum_union_and_finite_float_boundaries() {
    let schema = TypeRef::new(
        "acme.result",
        1,
        SchemaDefinition::TaggedUnion(TaggedUnionSchema::new([
            TaggedVariant::new("score", primitive(PrimitiveType::F64)),
            TaggedVariant::new(
                "state",
                SchemaDefinition::Enum(EnumSchema::new(["ready", "done"])),
            ),
        ])),
    );
    let Ok(schema) = schema else {
        panic!("valid union schema was rejected");
    };

    let valid = Payload::Tagged {
        tag: "state".to_owned(),
        value: Box::new(Payload::String("done".to_owned())),
    };
    assert_eq!(schema.validate_payload(&valid), Ok(()));

    let unknown_tag = schema.validate_payload(&Payload::Tagged {
        tag: "missing".to_owned(),
        value: Box::new(Payload::Null),
    });
    assert!(matches!(
        unknown_tag.as_ref().map_err(|error| error.violation()),
        Err(PayloadViolation::UnknownUnionTag { tag }) if tag == "missing"
    ));

    let unknown_enum = schema.validate_payload(&Payload::Tagged {
        tag: "state".to_owned(),
        value: Box::new(Payload::String("blocked".to_owned())),
    });
    assert!(matches!(
        unknown_enum.as_ref().map_err(|error| error.violation()),
        Err(PayloadViolation::UnknownEnumVariant { variant }) if variant == "blocked"
    ));

    let non_finite = schema.validate_payload(&Payload::Tagged {
        tag: "score".to_owned(),
        value: Box::new(Payload::F64(f64::NAN)),
    });
    assert!(matches!(
        non_finite.as_ref().map_err(|error| error.violation()),
        Err(PayloadViolation::NonFiniteFloat)
    ));
}

struct Order;

impl Schema for Order {
    const NAME: &'static str = "acme.order";
    const VERSION: u32 = 1;

    fn definition() -> SchemaDefinition {
        SchemaDefinition::Struct(StructSchema::new([SchemaField::required(
            "id",
            primitive(PrimitiveType::U64),
        )]))
    }
}

#[test]
fn schema_trait_builds_a_type_ref_and_errors_remain_classified() {
    let schema = Order::type_ref();
    let Ok(schema) = schema else {
        panic!("Schema implementation produced an invalid TypeRef");
    };
    assert_eq!(schema.name(), "acme.order");
    assert_eq!(schema.version(), 1);

    let payload_error = schema.validate_payload(&Payload::String("wrong".to_owned()));
    let Err(payload_error) = payload_error else {
        panic!("invalid payload was accepted");
    };
    let error = Error::from(payload_error);

    assert_eq!(error.category(), ErrorCategory::Schema);
    assert_eq!(error.code(), "core.invalid_payload");
    assert!(error.message().contains("payload at $"));
}
