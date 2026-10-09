//! Error and result types for `magik-core`.
//!
//! Every failure mode the library can produce is classified into a small,
//! stable set of [`ErrorKind`]s. The Python layer maps these one-to-one onto its
//! exception hierarchy, so this enum is deliberately coarse and exhaustive
//! rather than detailed.

use std::error::Error as StdError;
use std::fmt;
use std::io;

/// Convenience alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// The classification of a failure.
///
/// These variants are stable and mirror the Python exception hierarchy
/// one-to-one; adding a variant is a breaking change for consumers that
/// exhaustively match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    /// An image could not be opened (missing file, unreadable data, ...).
    Open,
    /// An image could not be written to disk or to a buffer.
    Save,
    /// A geometric or colour operation failed (bad dimensions, bad box, ...).
    Operation,
    /// A format/codec name is unknown or cannot be used here.
    Format,
    /// A named format was recognised but is not supported by this build of
    /// ImageMagick (missing delegate).
    UnsupportedFormat,
    /// An ImageMagick internal invariant was violated. Should not happen; if it
    /// does, please report it.
    Internal,
}

impl ErrorKind {
    /// A short, stable, machine-friendly name for this kind.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Save => "save",
            Self::Operation => "operation",
            Self::Format => "format",
            Self::UnsupportedFormat => "unsupported_format",
            Self::Internal => "internal",
        }
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An error raised by `magik-core`.
///
/// Carries two pieces of information:
///
/// * `context` — a magik-authored description of what was attempted
///   (`"could not open image"`);
/// * `detail` — the verbatim message from ImageMagick, when one was available
///   (`"unable to open image 'nope.png': No such file or directory"`).
///
/// `detail` is preserved rather than swallowed so that the Python layer can put
/// the real ImageMagick reason in front of the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    kind: ErrorKind,
    context: String,
    detail: Option<String>,
}

impl Error {
    /// Builds a new error of `kind` with magik-authored `context`.
    pub fn new(kind: ErrorKind, context: impl Into<String>) -> Self {
        Self {
            kind,
            context: context.into(),
            detail: None,
        }
    }

    /// Attaches (or replaces) the underlying ImageMagick message.
    #[must_use]
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        let detail = detail.into();
        if !detail.trim().is_empty() {
            self.detail = Some(detail);
        }
        self
    }

    /// Attaches a detail only if `detail` yields something non-blank.
    #[must_use]
    pub fn with_optional_detail(self, detail: Option<String>) -> Self {
        match detail {
            Some(d) => self.with_detail(d),
            None => self,
        }
    }

    /// Builds an [`ErrorKind::Open`] error.
    pub fn open(context: impl Into<String>) -> Self {
        Self::new(ErrorKind::Open, context)
    }

    /// Builds an [`ErrorKind::Save`] error.
    pub fn save(context: impl Into<String>) -> Self {
        Self::new(ErrorKind::Save, context)
    }

    /// Builds an [`ErrorKind::Operation`] error.
    pub fn operation(context: impl Into<String>) -> Self {
        Self::new(ErrorKind::Operation, context)
    }

    /// Builds an [`ErrorKind::Format`] error.
    pub fn format(context: impl Into<String>) -> Self {
        Self::new(ErrorKind::Format, context)
    }

    /// Builds an [`ErrorKind::UnsupportedFormat`] error.
    pub fn unsupported_format(context: impl Into<String>) -> Self {
        Self::new(ErrorKind::UnsupportedFormat, context)
    }

    /// Builds an [`ErrorKind::Internal`] error.
    pub fn internal(context: impl Into<String>) -> Self {
        Self::new(ErrorKind::Internal, context)
    }

    /// The failure classification.
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// The magik-authored description.
    pub fn context(&self) -> &str {
        &self.context
    }

    /// The ImageMagick message, if the C layer produced one.
    pub fn detail(&self) -> Option<&str> {
        self.detail.as_deref()
    }

    /// Renders `"<context>: <detail>"` when a detail is present.
    pub fn message(&self) -> String {
        match &self.detail {
            Some(detail) => format!("{}: {}", self.context, detail),
            None => self.context.clone(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message())
    }
}

impl StdError for Error {}

impl From<Error> for io::Error {
    fn from(err: Error) -> Self {
        io::Error::other(err.message())
    }
}
