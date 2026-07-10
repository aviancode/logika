use std::{error, fmt, str::FromStr};

const MAX_LOCAL_ID_LEN: usize = 128;
const MAX_GLOBAL_ID_LEN: usize = 255;

/// Identifies the domain type whose textual representation was invalid.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum IdentifierKind {
    /// A node identifier.
    Node,
    /// A plugin identifier.
    Plugin,
    /// A port identifier.
    Port,
    /// A workflow run identifier.
    Run,
}

impl fmt::Display for IdentifierKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Node => "node",
            Self::Plugin => "plugin",
            Self::Port => "port",
            Self::Run => "run",
        })
    }
}

/// Explains why a domain identifier could not be parsed.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum IdentifierViolation {
    /// The identifier was empty.
    Empty,
    /// The identifier exceeded the limit for its domain type.
    TooLong {
        /// Maximum accepted length in ASCII bytes.
        max: usize,
        /// Actual length in bytes.
        actual: usize,
    },
    /// The first character is not valid for this identifier type.
    InvalidStart {
        /// Character found at the start of the identifier.
        found: char,
    },
    /// A character is not part of the identifier's grammar.
    InvalidCharacter {
        /// Zero-based byte offset of the character.
        index: usize,
        /// Invalid character.
        found: char,
    },
    /// A plugin namespace contains an empty segment.
    EmptySegment {
        /// Zero-based byte offset of the separator that ends the empty segment.
        index: usize,
    },
}

impl fmt::Display for IdentifierViolation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("it is empty"),
            Self::TooLong { max, actual } => {
                write!(
                    formatter,
                    "its length is {actual} bytes, but the limit is {max}"
                )
            }
            Self::InvalidStart { found } => {
                write!(formatter, "it starts with invalid character {found:?}")
            }
            Self::InvalidCharacter { index, found } => {
                write!(
                    formatter,
                    "character {found:?} at byte {index} is not allowed"
                )
            }
            Self::EmptySegment { index } => {
                write!(formatter, "it contains an empty segment at byte {index}")
            }
        }
    }
}

/// Error returned when parsing a domain identifier.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct IdentifierError {
    kind: IdentifierKind,
    value: String,
    violation: IdentifierViolation,
}

impl IdentifierError {
    /// Returns the kind of identifier that failed to parse.
    #[must_use]
    pub const fn kind(&self) -> IdentifierKind {
        self.kind
    }

    /// Returns the rejected input.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }

    /// Returns the precise validation failure.
    #[must_use]
    pub const fn violation(&self) -> &IdentifierViolation {
        &self.violation
    }
}

impl fmt::Display for IdentifierError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "invalid {} identifier {:?}: {}",
            self.kind, self.value, self.violation
        )
    }
}

impl error::Error for IdentifierError {}

fn invalid(kind: IdentifierKind, value: &str, violation: IdentifierViolation) -> IdentifierError {
    IdentifierError {
        kind,
        value: value.to_owned(),
        violation,
    }
}

fn validate_local(kind: IdentifierKind, value: &str) -> Result<(), IdentifierError> {
    validate(kind, value, MAX_LOCAL_ID_LEN, |character| {
        character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
    })
}

fn validate_run(value: &str) -> Result<(), IdentifierError> {
    validate(IdentifierKind::Run, value, MAX_GLOBAL_ID_LEN, |character| {
        character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.' | ':')
    })
}

fn validate(
    kind: IdentifierKind,
    value: &str,
    max_len: usize,
    allowed: impl Fn(char) -> bool,
) -> Result<(), IdentifierError> {
    let mut characters = value.char_indices();
    let Some((_, first)) = characters.next() else {
        return Err(invalid(kind, value, IdentifierViolation::Empty));
    };

    if value.len() > max_len {
        return Err(invalid(
            kind,
            value,
            IdentifierViolation::TooLong {
                max: max_len,
                actual: value.len(),
            },
        ));
    }

    if !first.is_ascii_alphanumeric() {
        return Err(invalid(
            kind,
            value,
            IdentifierViolation::InvalidStart { found: first },
        ));
    }

    for (index, character) in characters {
        if !allowed(character) {
            return Err(invalid(
                kind,
                value,
                IdentifierViolation::InvalidCharacter {
                    index,
                    found: character,
                },
            ));
        }
    }

    Ok(())
}

fn validate_plugin(value: &str) -> Result<(), IdentifierError> {
    validate(
        IdentifierKind::Plugin,
        value,
        MAX_GLOBAL_ID_LEN,
        |character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.' | '/'),
    )?;

    let mut segment_is_empty = false;
    for (index, character) in value.char_indices() {
        if matches!(character, '.' | '/') {
            if index == 0 || segment_is_empty {
                return Err(invalid(
                    IdentifierKind::Plugin,
                    value,
                    IdentifierViolation::EmptySegment { index },
                ));
            }
            segment_is_empty = true;
        } else {
            segment_is_empty = false;
        }
    }

    if segment_is_empty {
        return Err(invalid(
            IdentifierKind::Plugin,
            value,
            IdentifierViolation::EmptySegment { index: value.len() },
        ));
    }

    Ok(())
}

macro_rules! string_identifier {
    ($(#[$attribute:meta])* $name:ident, $kind:expr, $validator:expr) => {
        $(#[$attribute])*
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(String);

        impl $name {
            /// Parses and validates an identifier.
            pub fn new(value: impl Into<String>) -> Result<Self, IdentifierError> {
                let value = value.into();
                $validator(&value)?;
                Ok(Self(value))
            }

            /// Borrows the identifier as text.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Consumes the identifier and returns its textual representation.
            #[must_use]
            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = IdentifierError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(value)
            }
        }

        impl TryFrom<String> for $name {
            type Error = IdentifierError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl TryFrom<&str> for $name {
            type Error = IdentifierError;

            fn try_from(value: &str) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.into_inner()
            }
        }

        const _: IdentifierKind = $kind;
    };
}

string_identifier!(
    /// Stable identifier of a node within a workflow.
    ///
    /// Node identifiers start with an ASCII letter or digit and may continue
    /// with ASCII letters, digits, `_`, or `-`.
    NodeId,
    IdentifierKind::Node,
    |value| validate_local(IdentifierKind::Node, value)
);

string_identifier!(
    /// Stable identifier of an input or output port on a node.
    ///
    /// Port identifiers use the same portable grammar as [`NodeId`].
    PortId,
    IdentifierKind::Port,
    |value| validate_local(IdentifierKind::Port, value)
);

string_identifier!(
    /// Stable identifier of a plugin.
    ///
    /// Dot and slash separators allow namespaced identifiers such as
    /// `acme.crm/enrichment`; empty namespace segments are rejected.
    PluginId,
    IdentifierKind::Plugin,
    validate_plugin
);

string_identifier!(
    /// Host-assigned identifier of a workflow run.
    ///
    /// The grammar accepts common UUID and ULID encodings as well as
    /// namespaced host identifiers. Orbita deliberately does not prescribe an
    /// identifier generation strategy to embedding applications.
    RunId,
    IdentifierKind::Run,
    validate_run
);
