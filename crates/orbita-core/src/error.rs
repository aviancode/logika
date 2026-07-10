use std::{error, fmt};

use crate::IdentifierError;

type BoxError = Box<dyn error::Error + Send + Sync + 'static>;

/// Broad, stable classification of failures exposed by Orbita.
///
/// Categories are suitable for metrics, retry decisions, and mapping errors
/// onto host application or CLI protocols. Callers should use the diagnostic
/// code for a more precise machine-readable reason.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum ErrorCategory {
    /// A workflow or configuration failed validation.
    Validation,
    /// A node, plugin, version, or dependency could not be resolved.
    Resolution,
    /// A schema was invalid or incompatible with another schema.
    Schema,
    /// A plugin failed to load or honor its protocol.
    Plugin,
    /// A requested capability was not granted by the host.
    CapabilityDenied,
    /// An operation exceeded its deadline.
    Timeout,
    /// An operation was cancelled.
    Cancelled,
    /// A storage operation failed.
    Storage,
    /// User-provided node code returned an error.
    UserNode,
}

impl ErrorCategory {
    /// Returns the stable machine-readable name of this category.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Validation => "validation",
            Self::Resolution => "resolution",
            Self::Schema => "schema",
            Self::Plugin => "plugin",
            Self::CapabilityDenied => "capability_denied",
            Self::Timeout => "timeout",
            Self::Cancelled => "cancelled",
            Self::Storage => "storage",
            Self::UserNode => "user_node",
        }
    }
}

impl fmt::Display for ErrorCategory {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Safe diagnostic attached to a classified public error.
///
/// `code` is intended for programs and must not contain secrets. `message` is
/// intended for people and should be safe to show in logs or CLI output.
#[derive(Debug)]
#[non_exhaustive]
pub struct ErrorDetail {
    code: String,
    message: String,
    source: Option<BoxError>,
}

impl ErrorDetail {
    /// Creates a diagnostic without an underlying error.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            source: None,
        }
    }

    /// Attaches an underlying implementation error for diagnostics.
    ///
    /// The source is available through [`error::Error::source`] but is not
    /// included in the public display text, preventing accidental disclosure
    /// of implementation details.
    #[must_use]
    pub fn with_source(mut self, source: impl error::Error + Send + Sync + 'static) -> Self {
        self.source = Some(Box::new(source));
        self
    }

    /// Returns the stable machine-readable diagnostic code.
    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }

    /// Returns the safe human-readable diagnostic.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ErrorDetail {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl error::Error for ErrorDetail {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn error::Error + 'static))
    }
}

/// Unified public error hierarchy for Orbita APIs.
///
/// The variants provide a stable broad classification. Each variant carries
/// an [`ErrorDetail`] with a finer-grained code and a safe diagnostic.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// A workflow or configuration failed validation.
    Validation(ErrorDetail),
    /// A node, plugin, version, or dependency could not be resolved.
    Resolution(ErrorDetail),
    /// A schema was invalid or incompatible with another schema.
    Schema(ErrorDetail),
    /// A plugin failed to load or honor its protocol.
    Plugin(ErrorDetail),
    /// A requested capability was not granted by the host.
    CapabilityDenied(ErrorDetail),
    /// An operation exceeded its deadline.
    Timeout(ErrorDetail),
    /// An operation was cancelled.
    Cancelled(ErrorDetail),
    /// A storage operation failed.
    Storage(ErrorDetail),
    /// User-provided node code returned an error.
    UserNode(ErrorDetail),
}

impl Error {
    /// Creates an error in the requested category.
    #[must_use]
    pub fn new(category: ErrorCategory, detail: ErrorDetail) -> Self {
        match category {
            ErrorCategory::Validation => Self::Validation(detail),
            ErrorCategory::Resolution => Self::Resolution(detail),
            ErrorCategory::Schema => Self::Schema(detail),
            ErrorCategory::Plugin => Self::Plugin(detail),
            ErrorCategory::CapabilityDenied => Self::CapabilityDenied(detail),
            ErrorCategory::Timeout => Self::Timeout(detail),
            ErrorCategory::Cancelled => Self::Cancelled(detail),
            ErrorCategory::Storage => Self::Storage(detail),
            ErrorCategory::UserNode => Self::UserNode(detail),
        }
    }

    /// Returns the broad classification of this error.
    #[must_use]
    pub const fn category(&self) -> ErrorCategory {
        match self {
            Self::Validation(_) => ErrorCategory::Validation,
            Self::Resolution(_) => ErrorCategory::Resolution,
            Self::Schema(_) => ErrorCategory::Schema,
            Self::Plugin(_) => ErrorCategory::Plugin,
            Self::CapabilityDenied(_) => ErrorCategory::CapabilityDenied,
            Self::Timeout(_) => ErrorCategory::Timeout,
            Self::Cancelled(_) => ErrorCategory::Cancelled,
            Self::Storage(_) => ErrorCategory::Storage,
            Self::UserNode(_) => ErrorCategory::UserNode,
        }
    }

    /// Returns the diagnostic carried by this error.
    #[must_use]
    pub const fn detail(&self) -> &ErrorDetail {
        match self {
            Self::Validation(detail)
            | Self::Resolution(detail)
            | Self::Schema(detail)
            | Self::Plugin(detail)
            | Self::CapabilityDenied(detail)
            | Self::Timeout(detail)
            | Self::Cancelled(detail)
            | Self::Storage(detail)
            | Self::UserNode(detail) => detail,
        }
    }

    /// Returns the stable machine-readable diagnostic code.
    #[must_use]
    pub fn code(&self) -> &str {
        self.detail().code()
    }

    /// Returns the safe human-readable diagnostic.
    #[must_use]
    pub fn message(&self) -> &str {
        self.detail().message()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} ({}): {}",
            self.category(),
            self.code(),
            self.message()
        )
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        Some(self.detail())
    }
}

impl From<IdentifierError> for Error {
    fn from(source: IdentifierError) -> Self {
        let message = source.to_string();
        Self::Validation(ErrorDetail::new("core.invalid_identifier", message).with_source(source))
    }
}

/// Result type used by public Orbita APIs.
pub type Result<T> = std::result::Result<T, Error>;
