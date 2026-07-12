//! Tests for the portable schema derive.

#![allow(clippy::expect_used)]

use std::collections::BTreeMap;

use logika_sdk::{_private::SchemaDefinition, Schema};

#[allow(dead_code)]
#[derive(Schema)]
#[schema(name = "acme.address", version = 1)]
struct Address {
    city: String,
}

#[allow(dead_code)]
#[derive(Schema)]
#[schema(name = "acme.order", version = 2)]
struct Order {
    id: u64,
    #[schema(rename = "delivery_address")]
    address: Address,
    note: Option<String>,
    labels: Vec<String>,
    attributes: BTreeMap<String, String>,
}

#[allow(dead_code)]
#[derive(Schema)]
#[schema(name = "acme.status", version = 1)]
enum Status {
    Pending,
    #[schema(rename = "complete")]
    Complete,
}

#[allow(dead_code)]
#[derive(Schema)]
#[schema(name = "acme.event", version = 1)]
enum Event {
    Created(Order),
    Rejected { reason: String, retryable: bool },
    Deleted,
}

#[test]
fn derives_struct_schema_with_containers_and_optional_fields() {
    let type_ref = Order::type_ref().expect("derived schema should be valid");

    assert_eq!(type_ref.name(), "acme.order");
    assert_eq!(type_ref.version(), 2);
    let SchemaDefinition::Struct(schema) = type_ref.definition() else {
        panic!("expected a struct schema");
    };
    let fields = schema.fields();
    assert_eq!(
        fields.iter().map(|field| field.name()).collect::<Vec<_>>(),
        ["attributes", "delivery_address", "id", "labels", "note"]
    );
    assert!(!fields[4].is_required());
    assert!(matches!(fields[0].schema(), SchemaDefinition::Map(_)));
    assert!(matches!(fields[3].schema(), SchemaDefinition::Array(_)));
    assert!(matches!(fields[1].schema(), SchemaDefinition::Struct(_)));
}

#[test]
fn derives_unit_and_tagged_enums() {
    let status = Status::type_ref().expect("unit enum schema should be valid");
    let SchemaDefinition::Enum(status) = status.definition() else {
        panic!("expected an enum schema");
    };
    assert_eq!(status.variants(), ["Pending", "complete"]);

    let event = Event::type_ref().expect("tagged enum schema should be valid");
    let SchemaDefinition::TaggedUnion(event) = event.definition() else {
        panic!("expected a tagged union schema");
    };
    assert_eq!(
        event
            .variants()
            .iter()
            .map(|variant| variant.tag())
            .collect::<Vec<_>>(),
        ["Created", "Deleted", "Rejected"]
    );
}
