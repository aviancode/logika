use std::{collections::BTreeMap, error, fmt};

use sha2::{Digest, Sha256};

const MAX_TYPE_NAME_LEN: usize = 255;
const MAX_MEMBER_NAME_LEN: usize = 255;
const MAX_SCHEMA_DEPTH: usize = 64;
const MAX_PAYLOAD_DEPTH: usize = 128;

/// A Rust type that can describe its portable Logika schema.
///
/// SDK derives can implement this trait without coupling the canonical model
/// to Rust layout or serialization details.
pub trait Schema {
    /// Stable, namespaced type name, for example `acme.order`.
    const NAME: &'static str;
    /// Positive schema version within the stable type name.
    const VERSION: u32;

    /// Returns the portable structural definition of this type.
    fn definition() -> SchemaDefinition;

    /// Builds and validates the complete type reference.
    fn type_ref() -> Result<TypeRef, SchemaError> {
        TypeRef::new(Self::NAME, Self::VERSION, Self::definition())
    }
}

/// Primitive values supported by the canonical schema model.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum PrimitiveType {
    /// A boolean value.
    Bool,
    /// A signed 64-bit integer.
    I64,
    /// An unsigned 64-bit integer.
    U64,
    /// An IEEE-754 double-precision number.
    F64,
    /// UTF-8 text.
    String,
    /// Arbitrary bytes.
    Bytes,
}

impl PrimitiveType {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::I64 => "i64",
            Self::U64 => "u64",
            Self::F64 => "f64",
            Self::String => "string",
            Self::Bytes => "bytes",
        }
    }
}

/// A field in a canonical struct schema.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaField {
    name: String,
    schema: SchemaDefinition,
    required: bool,
}

impl SchemaField {
    /// Creates a field that must be present in a payload.
    pub fn required(name: impl Into<String>, schema: SchemaDefinition) -> Self {
        Self {
            name: name.into(),
            schema,
            required: true,
        }
    }

    /// Creates a field that may be absent from a payload.
    pub fn optional(name: impl Into<String>, schema: SchemaDefinition) -> Self {
        Self {
            name: name.into(),
            schema,
            required: false,
        }
    }

    /// Returns the field name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the field schema.
    #[must_use]
    pub const fn schema(&self) -> &SchemaDefinition {
        &self.schema
    }

    /// Returns whether the field must be present.
    #[must_use]
    pub const fn is_required(&self) -> bool {
        self.required
    }
}

/// A struct with named required and optional fields.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StructSchema {
    fields: Vec<SchemaField>,
}

impl StructSchema {
    /// Creates a struct schema. Field order does not affect its fingerprint.
    pub fn new(fields: impl IntoIterator<Item = SchemaField>) -> Self {
        Self {
            fields: fields.into_iter().collect(),
        }
    }

    /// Returns fields in their canonical name order after construction of a [`TypeRef`].
    #[must_use]
    pub fn fields(&self) -> &[SchemaField] {
        &self.fields
    }
}

/// A closed enumeration represented by a string payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnumSchema {
    variants: Vec<String>,
}

impl EnumSchema {
    /// Creates an enum schema. Variant order does not affect its fingerprint.
    pub fn new<T, I>(variants: I) -> Self
    where
        T: Into<String>,
        I: IntoIterator<Item = T>,
    {
        Self {
            variants: variants.into_iter().map(Into::into).collect(),
        }
    }

    /// Returns variants in canonical order after construction of a [`TypeRef`].
    #[must_use]
    pub fn variants(&self) -> &[String] {
        &self.variants
    }
}

/// A named branch of a tagged union.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaggedVariant {
    tag: String,
    schema: SchemaDefinition,
}

impl TaggedVariant {
    /// Creates a tagged branch with its associated payload schema.
    pub fn new(tag: impl Into<String>, schema: SchemaDefinition) -> Self {
        Self {
            tag: tag.into(),
            schema,
        }
    }

    /// Returns the discriminator value.
    #[must_use]
    pub fn tag(&self) -> &str {
        &self.tag
    }

    /// Returns the payload schema for this branch.
    #[must_use]
    pub const fn schema(&self) -> &SchemaDefinition {
        &self.schema
    }
}

/// A closed union selected by a string discriminator.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaggedUnionSchema {
    variants: Vec<TaggedVariant>,
}

impl TaggedUnionSchema {
    /// Creates a tagged union. Branch order does not affect its fingerprint.
    pub fn new(variants: impl IntoIterator<Item = TaggedVariant>) -> Self {
        Self {
            variants: variants.into_iter().collect(),
        }
    }

    /// Returns branches in canonical tag order after construction of a [`TypeRef`].
    #[must_use]
    pub fn variants(&self) -> &[TaggedVariant] {
        &self.variants
    }
}

/// Portable structural schema used by workflow documents and plugin boundaries.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SchemaDefinition {
    /// A scalar primitive.
    Primitive(PrimitiveType),
    /// A struct with named fields.
    Struct(StructSchema),
    /// A closed string enumeration.
    Enum(EnumSchema),
    /// A homogeneous sequence.
    Array(Box<Self>),
    /// A string-keyed homogeneous map.
    Map(Box<Self>),
    /// A nullable value.
    Optional(Box<Self>),
    /// A closed discriminated union.
    TaggedUnion(TaggedUnionSchema),
}

impl SchemaDefinition {
    /// Creates a primitive schema.
    #[must_use]
    pub const fn primitive(primitive: PrimitiveType) -> Self {
        Self::Primitive(primitive)
    }

    /// Creates a homogeneous array schema.
    #[must_use]
    pub fn array(items: Self) -> Self {
        Self::Array(Box::new(items))
    }

    /// Creates a string-keyed map schema.
    #[must_use]
    pub fn map(values: Self) -> Self {
        Self::Map(Box::new(values))
    }

    /// Creates a schema accepting either null or the nested value.
    #[must_use]
    pub fn optional(value: Self) -> Self {
        Self::Optional(Box::new(value))
    }

    fn normalize_and_validate(&mut self, depth: usize) -> Result<(), SchemaError> {
        if depth > MAX_SCHEMA_DEPTH {
            return Err(SchemaError::new(SchemaViolation::DepthLimitExceeded {
                max: MAX_SCHEMA_DEPTH,
            }));
        }

        match self {
            Self::Primitive(_) => Ok(()),
            Self::Array(items) | Self::Map(items) | Self::Optional(items) => {
                items.normalize_and_validate(depth + 1)
            }
            Self::Struct(schema) => {
                for field in &mut schema.fields {
                    validate_member_name("field", &field.name)?;
                    field.schema.normalize_and_validate(depth + 1)?;
                }
                schema
                    .fields
                    .sort_by(|left, right| left.name.cmp(&right.name));
                reject_duplicate_names(
                    "field",
                    schema.fields.iter().map(|field| field.name.as_str()),
                )
            }
            Self::Enum(schema) => {
                if schema.variants.is_empty() {
                    return Err(SchemaError::new(SchemaViolation::EmptyEnum));
                }
                for variant in &schema.variants {
                    validate_member_name("enum variant", variant)?;
                }
                schema.variants.sort();
                reject_duplicate_names("enum variant", schema.variants.iter().map(String::as_str))
            }
            Self::TaggedUnion(schema) => {
                if schema.variants.is_empty() {
                    return Err(SchemaError::new(SchemaViolation::EmptyTaggedUnion));
                }
                for variant in &mut schema.variants {
                    validate_member_name("union tag", &variant.tag)?;
                    variant.schema.normalize_and_validate(depth + 1)?;
                }
                schema
                    .variants
                    .sort_by(|left, right| left.tag.cmp(&right.tag));
                reject_duplicate_names(
                    "union tag",
                    schema.variants.iter().map(|variant| variant.tag.as_str()),
                )
            }
        }
    }

    fn write_canonical(&self, output: &mut String) {
        match self {
            Self::Primitive(primitive) => {
                output.push_str("{\"type\":");
                write_json_string(output, primitive.as_str());
                output.push('}');
            }
            Self::Struct(schema) => {
                output.push_str("{\"type\":\"struct\",\"fields\":[");
                for (index, field) in schema.fields.iter().enumerate() {
                    if index > 0 {
                        output.push(',');
                    }
                    output.push_str("{\"name\":");
                    write_json_string(output, &field.name);
                    output.push_str(",\"required\":");
                    output.push_str(if field.required { "true" } else { "false" });
                    output.push_str(",\"schema\":");
                    field.schema.write_canonical(output);
                    output.push('}');
                }
                output.push_str("]}");
            }
            Self::Enum(schema) => {
                output.push_str("{\"type\":\"enum\",\"variants\":[");
                for (index, variant) in schema.variants.iter().enumerate() {
                    if index > 0 {
                        output.push(',');
                    }
                    write_json_string(output, variant);
                }
                output.push_str("]}");
            }
            Self::Array(items) => write_nested_canonical(output, "array", "items", items),
            Self::Map(values) => write_nested_canonical(output, "map", "values", values),
            Self::Optional(value) => write_nested_canonical(output, "optional", "value", value),
            Self::TaggedUnion(schema) => {
                output.push_str("{\"type\":\"tagged_union\",\"variants\":[");
                for (index, variant) in schema.variants.iter().enumerate() {
                    if index > 0 {
                        output.push(',');
                    }
                    output.push_str("{\"tag\":");
                    write_json_string(output, &variant.tag);
                    output.push_str(",\"schema\":");
                    variant.schema.write_canonical(output);
                    output.push('}');
                }
                output.push_str("]}");
            }
        }
    }

    fn validate_payload_at(
        &self,
        payload: &Payload,
        path: &str,
        depth: usize,
    ) -> Result<(), PayloadValidationError> {
        if depth > MAX_PAYLOAD_DEPTH {
            return Err(PayloadValidationError::new(
                path,
                PayloadViolation::DepthLimitExceeded {
                    max: MAX_PAYLOAD_DEPTH,
                },
            ));
        }

        match (self, payload) {
            (Self::Optional(_), Payload::Null) => Ok(()),
            (Self::Optional(schema), payload) => {
                schema.validate_payload_at(payload, path, depth + 1)
            }
            (Self::Primitive(expected), payload) => validate_primitive(*expected, payload, path),
            (Self::Array(schema), Payload::Array(items)) => {
                for (index, item) in items.iter().enumerate() {
                    schema.validate_payload_at(item, &path_index(path, index), depth + 1)?;
                }
                Ok(())
            }
            (Self::Map(schema), Payload::Object(entries)) => {
                for (key, value) in entries {
                    schema.validate_payload_at(value, &path_segment(path, key), depth + 1)?;
                }
                Ok(())
            }
            (Self::Struct(schema), Payload::Object(entries)) => {
                for field in &schema.fields {
                    match entries.get(&field.name) {
                        Some(value) => field.schema.validate_payload_at(
                            value,
                            &path_segment(path, &field.name),
                            depth + 1,
                        )?,
                        None if field.required => {
                            return Err(PayloadValidationError::new(
                                path_segment(path, &field.name),
                                PayloadViolation::MissingRequiredField {
                                    field: field.name.clone(),
                                },
                            ));
                        }
                        None => {}
                    }
                }
                for name in entries.keys() {
                    if schema
                        .fields
                        .binary_search_by(|field| field.name.cmp(name))
                        .is_err()
                    {
                        return Err(PayloadValidationError::new(
                            path_segment(path, name),
                            PayloadViolation::UnknownField {
                                field: name.clone(),
                            },
                        ));
                    }
                }
                Ok(())
            }
            (Self::Enum(schema), Payload::String(variant)) => {
                if schema
                    .variants
                    .binary_search_by(|candidate| candidate.as_str().cmp(variant))
                    .is_ok()
                {
                    Ok(())
                } else {
                    Err(PayloadValidationError::new(
                        path,
                        PayloadViolation::UnknownEnumVariant {
                            variant: variant.clone(),
                        },
                    ))
                }
            }
            (Self::TaggedUnion(schema), Payload::Tagged { tag, value }) => {
                let variant = schema
                    .variants
                    .binary_search_by(|candidate| candidate.tag.as_str().cmp(tag))
                    .ok()
                    .map(|index| &schema.variants[index]);
                match variant {
                    Some(variant) => variant.schema.validate_payload_at(
                        value,
                        &path_segment(path, "value"),
                        depth + 1,
                    ),
                    None => Err(PayloadValidationError::new(
                        path_segment(path, "tag"),
                        PayloadViolation::UnknownUnionTag { tag: tag.clone() },
                    )),
                }
            }
            (schema, payload) => Err(PayloadValidationError::new(
                path,
                PayloadViolation::TypeMismatch {
                    expected: schema.expected_payload_kind(),
                    actual: payload.kind(),
                },
            )),
        }
    }

    fn expected_payload_kind(&self) -> &'static str {
        match self {
            Self::Primitive(primitive) => primitive.as_str(),
            Self::Struct(_) | Self::Map(_) => "object",
            Self::Enum(_) => "string enum",
            Self::Array(_) => "array",
            Self::Optional(_) => "nullable value",
            Self::TaggedUnion(_) => "tagged union",
        }
    }
}

/// A stable SHA-256 digest of a canonical type reference.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct SchemaFingerprint([u8; 32]);

impl SchemaFingerprint {
    /// Returns the raw 32-byte SHA-256 digest.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Returns the lowercase hexadecimal digest.
    #[must_use]
    pub fn to_hex(self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut output = String::with_capacity(64);
        for byte in self.0 {
            output.push(char::from(HEX[usize::from(byte >> 4)]));
            output.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
        output
    }
}

impl fmt::Debug for SchemaFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("SchemaFingerprint")
            .field(&self.to_hex())
            .finish()
    }
}

impl fmt::Display for SchemaFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_hex())
    }
}

/// Complete portable identity of a value type.
///
/// The canonical representation includes the stable name, version, and
/// normalized structural schema. Its SHA-256 digest is therefore suitable for
/// strict port compatibility checks and lock files.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeRef {
    name: String,
    version: u32,
    definition: SchemaDefinition,
    canonical: String,
    fingerprint: SchemaFingerprint,
}

impl TypeRef {
    /// Validates and canonicalizes a type reference.
    pub fn new(
        name: impl Into<String>,
        version: u32,
        mut definition: SchemaDefinition,
    ) -> Result<Self, SchemaError> {
        let name = name.into();
        validate_type_name(&name)?;
        if version == 0 {
            return Err(SchemaError::new(SchemaViolation::VersionZero));
        }
        definition.normalize_and_validate(0)?;

        let mut canonical = String::new();
        canonical.push_str("{\"name\":");
        write_json_string(&mut canonical, &name);
        canonical.push_str(",\"version\":");
        canonical.push_str(&version.to_string());
        canonical.push_str(",\"schema\":");
        definition.write_canonical(&mut canonical);
        canonical.push('}');

        let fingerprint = SchemaFingerprint(Sha256::digest(canonical.as_bytes()).into());
        Ok(Self {
            name,
            version,
            definition,
            canonical,
            fingerprint,
        })
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

    /// Returns the normalized schema definition.
    #[must_use]
    pub const fn definition(&self) -> &SchemaDefinition {
        &self.definition
    }

    /// Returns the deterministic UTF-8 canonical representation.
    #[must_use]
    pub fn canonical(&self) -> &str {
        &self.canonical
    }

    /// Returns the SHA-256 digest of [`Self::canonical`].
    #[must_use]
    pub const fn fingerprint(&self) -> SchemaFingerprint {
        self.fingerprint
    }

    /// Returns whether two port types are strictly interchangeable.
    ///
    /// Explicit migrations or compatibility declarations can be layered on
    /// top by a registry; the core default requires the complete identity to
    /// match.
    #[must_use]
    pub fn is_compatible_with(&self, other: &Self) -> bool {
        self.name == other.name
            && self.version == other.version
            && self.fingerprint == other.fingerprint
    }

    /// Validates an actual payload against this type's structural schema.
    pub fn validate_payload(&self, payload: &Payload) -> Result<(), PayloadValidationError> {
        self.definition.validate_payload_at(payload, "$", 0)
    }
}

/// Describes why a schema definition could not become a [`TypeRef`].
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SchemaViolation {
    /// The stable type name was empty.
    EmptyTypeName,
    /// The stable type name exceeded its byte limit.
    TypeNameTooLong {
        /// Maximum accepted byte length.
        max: usize,
        /// Actual byte length.
        actual: usize,
    },
    /// The stable type name began with an unsupported character.
    InvalidTypeNameStart {
        /// Rejected character.
        found: char,
    },
    /// The stable type name contained an unsupported character or empty namespace segment.
    InvalidTypeNameCharacter {
        /// Zero-based UTF-8 byte offset.
        index: usize,
        /// Rejected character.
        found: char,
    },
    /// Schema versions start at one.
    VersionZero,
    /// An enum must declare at least one variant.
    EmptyEnum,
    /// A tagged union must declare at least one branch.
    EmptyTaggedUnion,
    /// A field, enum variant, or union tag was empty, too long, or contained a control character.
    InvalidMemberName {
        /// Kind of schema member.
        member: &'static str,
        /// Rejected member name.
        name: String,
    },
    /// A member name was repeated in one schema scope.
    DuplicateMember {
        /// Kind of schema member.
        member: &'static str,
        /// Repeated name.
        name: String,
    },
    /// The recursive schema exceeded the defensive nesting limit.
    DepthLimitExceeded {
        /// Maximum supported nesting depth.
        max: usize,
    },
}

impl fmt::Display for SchemaViolation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyTypeName => formatter.write_str("the type name is empty"),
            Self::TypeNameTooLong { max, actual } => write!(
                formatter,
                "the type name is {actual} bytes, but the limit is {max}"
            ),
            Self::InvalidTypeNameStart { found } => {
                write!(
                    formatter,
                    "the type name starts with invalid character {found:?}"
                )
            }
            Self::InvalidTypeNameCharacter { index, found } => write!(
                formatter,
                "the type name contains invalid character {found:?} at byte {index}"
            ),
            Self::VersionZero => formatter.write_str("the schema version must be at least 1"),
            Self::EmptyEnum => formatter.write_str("an enum must contain at least one variant"),
            Self::EmptyTaggedUnion => {
                formatter.write_str("a tagged union must contain at least one variant")
            }
            Self::InvalidMemberName { member, name } => {
                write!(formatter, "the {member} name {name:?} is invalid")
            }
            Self::DuplicateMember { member, name } => {
                write!(formatter, "the {member} name {name:?} is duplicated")
            }
            Self::DepthLimitExceeded { max } => {
                write!(formatter, "the schema nesting depth exceeds {max}")
            }
        }
    }
}

/// Error returned when constructing an invalid canonical schema.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaError {
    violation: SchemaViolation,
}

impl SchemaError {
    fn new(violation: SchemaViolation) -> Self {
        Self { violation }
    }

    /// Returns the precise schema rule that was violated.
    #[must_use]
    pub const fn violation(&self) -> &SchemaViolation {
        &self.violation
    }
}

impl fmt::Display for SchemaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid schema: {}", self.violation)
    }
}

impl error::Error for SchemaError {}

/// Format-neutral payload value validated at an untrusted plugin boundary.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Payload {
    /// An explicit null value, accepted only by optional schemas.
    Null,
    /// A boolean.
    Bool(bool),
    /// A signed 64-bit integer.
    I64(i64),
    /// An unsigned 64-bit integer.
    U64(u64),
    /// A double-precision number.
    F64(f64),
    /// UTF-8 text or an enum variant.
    String(String),
    /// Arbitrary bytes.
    Bytes(Vec<u8>),
    /// A sequence.
    Array(Vec<Self>),
    /// A string-keyed object used for structs and maps.
    Object(BTreeMap<String, Self>),
    /// A discriminated branch with its associated payload.
    Tagged {
        /// Discriminator value.
        tag: String,
        /// Branch payload.
        value: Box<Self>,
    },
}

impl Payload {
    /// Returns the broad payload kind for diagnostics.
    #[must_use]
    pub const fn kind(&self) -> PayloadKind {
        match self {
            Self::Null => PayloadKind::Null,
            Self::Bool(_) => PayloadKind::Bool,
            Self::I64(_) => PayloadKind::I64,
            Self::U64(_) => PayloadKind::U64,
            Self::F64(_) => PayloadKind::F64,
            Self::String(_) => PayloadKind::String,
            Self::Bytes(_) => PayloadKind::Bytes,
            Self::Array(_) => PayloadKind::Array,
            Self::Object(_) => PayloadKind::Object,
            Self::Tagged { .. } => PayloadKind::Tagged,
        }
    }
}

/// Broad runtime payload kind used in validation diagnostics.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum PayloadKind {
    /// Null.
    Null,
    /// Boolean.
    Bool,
    /// Signed integer.
    I64,
    /// Unsigned integer.
    U64,
    /// Floating point number.
    F64,
    /// String.
    String,
    /// Byte string.
    Bytes,
    /// Array.
    Array,
    /// Object.
    Object,
    /// Tagged value.
    Tagged,
}

impl PayloadKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool => "bool",
            Self::I64 => "i64",
            Self::U64 => "u64",
            Self::F64 => "f64",
            Self::String => "string",
            Self::Bytes => "bytes",
            Self::Array => "array",
            Self::Object => "object",
            Self::Tagged => "tagged union",
        }
    }
}

impl fmt::Display for PayloadKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Describes why a payload did not satisfy its canonical schema.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum PayloadViolation {
    /// The runtime payload kind did not match the schema.
    TypeMismatch {
        /// Expected schema kind.
        expected: &'static str,
        /// Actual runtime kind.
        actual: PayloadKind,
    },
    /// A required struct field was absent.
    MissingRequiredField {
        /// Missing field name.
        field: String,
    },
    /// A closed struct contained an undeclared field.
    UnknownField {
        /// Undeclared field name.
        field: String,
    },
    /// A string was not part of a closed enum.
    UnknownEnumVariant {
        /// Rejected variant.
        variant: String,
    },
    /// A discriminator was not part of a tagged union.
    UnknownUnionTag {
        /// Rejected discriminator.
        tag: String,
    },
    /// A float payload was NaN or infinite.
    NonFiniteFloat,
    /// The payload exceeded the defensive nesting limit.
    DepthLimitExceeded {
        /// Maximum supported nesting depth.
        max: usize,
    },
}

impl fmt::Display for PayloadViolation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TypeMismatch { expected, actual } => {
                write!(formatter, "expected {expected}, found {actual}")
            }
            Self::MissingRequiredField { field } => {
                write!(formatter, "required field {field:?} is missing")
            }
            Self::UnknownField { field } => write!(formatter, "field {field:?} is not declared"),
            Self::UnknownEnumVariant { variant } => {
                write!(formatter, "enum variant {variant:?} is not declared")
            }
            Self::UnknownUnionTag { tag } => {
                write!(formatter, "union tag {tag:?} is not declared")
            }
            Self::NonFiniteFloat => formatter.write_str("floating point value is not finite"),
            Self::DepthLimitExceeded { max } => {
                write!(formatter, "payload nesting depth exceeds {max}")
            }
        }
    }
}

/// Path-aware payload validation failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PayloadValidationError {
    path: String,
    violation: PayloadViolation,
}

impl PayloadValidationError {
    fn new(path: impl Into<String>, violation: PayloadViolation) -> Self {
        Self {
            path: path.into(),
            violation,
        }
    }

    /// Returns the JSONPath-like location of the rejected value.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Returns the precise payload rule that was violated.
    #[must_use]
    pub const fn violation(&self) -> &PayloadViolation {
        &self.violation
    }
}

impl fmt::Display for PayloadValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "payload at {} does not match schema: {}",
            self.path, self.violation
        )
    }
}

impl error::Error for PayloadValidationError {}

fn validate_primitive(
    expected: PrimitiveType,
    payload: &Payload,
    path: &str,
) -> Result<(), PayloadValidationError> {
    let matches = matches!(
        (expected, payload),
        (PrimitiveType::Bool, Payload::Bool(_))
            | (PrimitiveType::I64, Payload::I64(_))
            | (PrimitiveType::U64, Payload::U64(_))
            | (PrimitiveType::F64, Payload::F64(_))
            | (PrimitiveType::String, Payload::String(_))
            | (PrimitiveType::Bytes, Payload::Bytes(_))
    );
    if !matches {
        return Err(PayloadValidationError::new(
            path,
            PayloadViolation::TypeMismatch {
                expected: expected.as_str(),
                actual: payload.kind(),
            },
        ));
    }
    if let Payload::F64(value) = payload
        && !value.is_finite()
    {
        return Err(PayloadValidationError::new(
            path,
            PayloadViolation::NonFiniteFloat,
        ));
    }
    Ok(())
}

fn validate_type_name(name: &str) -> Result<(), SchemaError> {
    let mut chars = name.char_indices();
    let Some((_, first)) = chars.next() else {
        return Err(SchemaError::new(SchemaViolation::EmptyTypeName));
    };
    if name.len() > MAX_TYPE_NAME_LEN {
        return Err(SchemaError::new(SchemaViolation::TypeNameTooLong {
            max: MAX_TYPE_NAME_LEN,
            actual: name.len(),
        }));
    }
    if !first.is_ascii_alphanumeric() {
        return Err(SchemaError::new(SchemaViolation::InvalidTypeNameStart {
            found: first,
        }));
    }

    let mut after_separator = false;
    for (index, character) in chars {
        let separator = matches!(character, '.' | '/');
        let allowed =
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.' | '/');
        if !allowed || (separator && after_separator) {
            return Err(SchemaError::new(
                SchemaViolation::InvalidTypeNameCharacter {
                    index,
                    found: character,
                },
            ));
        }
        after_separator = separator;
    }
    if after_separator {
        let (index, found) = name.char_indices().next_back().unwrap_or((0, '.'));
        return Err(SchemaError::new(
            SchemaViolation::InvalidTypeNameCharacter { index, found },
        ));
    }
    Ok(())
}

fn validate_member_name(member: &'static str, name: &str) -> Result<(), SchemaError> {
    if name.is_empty() || name.len() > MAX_MEMBER_NAME_LEN || name.chars().any(char::is_control) {
        return Err(SchemaError::new(SchemaViolation::InvalidMemberName {
            member,
            name: name.to_owned(),
        }));
    }
    Ok(())
}

fn reject_duplicate_names<'a>(
    member: &'static str,
    names: impl IntoIterator<Item = &'a str>,
) -> Result<(), SchemaError> {
    let mut previous: Option<&str> = None;
    for name in names {
        if previous == Some(name) {
            return Err(SchemaError::new(SchemaViolation::DuplicateMember {
                member,
                name: name.to_owned(),
            }));
        }
        previous = Some(name);
    }
    Ok(())
}

fn write_nested_canonical(
    output: &mut String,
    kind: &str,
    nested_name: &str,
    nested: &SchemaDefinition,
) {
    output.push_str("{\"type\":");
    write_json_string(output, kind);
    output.push(',');
    write_json_string(output, nested_name);
    output.push(':');
    nested.write_canonical(output);
    output.push('}');
}

fn write_json_string(output: &mut String, value: &str) {
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\u{08}' => output.push_str("\\b"),
            '\u{0c}' => output.push_str("\\f"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            control if control.is_control() => {
                use fmt::Write as _;
                let _ = write!(output, "\\u{:04x}", u32::from(control));
            }
            character => output.push(character),
        }
    }
    output.push('"');
}

fn path_segment(path: &str, segment: &str) -> String {
    let escaped = segment.replace('~', "~0").replace('/', "~1");
    format!("{path}/{escaped}")
}

fn path_index(path: &str, index: usize) -> String {
    format!("{path}/{index}")
}
