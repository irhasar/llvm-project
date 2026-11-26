//! Recoverable error type used across the Rust LLVM core.

use core::fmt;

use crate::diag::{Diagnostic, Severity, Span};

/// Broad category for an error.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    Parse,
    Bitcode,
    Verify,
    Io,
    Internal,
}

impl ErrorKind {
    /// Returns a short stable label.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Parse => "parse error",
            Self::Bitcode => "bitcode error",
            Self::Verify => "verification error",
            Self::Io => "I/O error",
            Self::Internal => "internal error",
        }
    }
}

/// A recoverable error with an optional source location.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    pub message: String,
    pub span: Option<Span>,
}

impl Error {
    /// Creates an error.
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            span: None,
        }
    }

    /// Creates an error at a source span.
    pub fn at(kind: ErrorKind, message: impl Into<String>, span: Span) -> Self {
        Self {
            kind,
            message: message.into(),
            span: Some(span),
        }
    }

    /// Creates a parse error.
    pub fn parse(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Parse, message)
    }

    /// Creates a parse error at a source span.
    pub fn parse_at(message: impl Into<String>, span: Span) -> Self {
        Self::at(ErrorKind::Parse, message, span)
    }

    /// Creates a bitcode error.
    pub fn bitcode(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Bitcode, message)
    }

    /// Creates a verification error.
    pub fn verify(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Verify, message)
    }

    /// Creates an I/O error.
    pub fn io(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Io, message)
    }

    /// Creates an internal error.
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Internal, message)
    }

    /// Renders the error against a source buffer.
    pub fn render(&self, path: &str, source: &str) -> String {
        let severity = match self.kind {
            ErrorKind::Internal => Severity::Note,
            _ => Severity::Error,
        };
        Diagnostic::new(
            severity,
            format!("{}: {}", self.kind.as_str(), self.message),
            self.span,
        )
        .render(path, source)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind.as_str(), self.message)
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self::io(value.to_string())
    }
}

/// Result alias for the Rust LLVM core.
pub type Result<T> = core::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carries_kind_and_span() {
        let error = Error::parse_at("expected type", Span::new(3, 8));
        assert_eq!(error.kind, ErrorKind::Parse);
        assert_eq!(error.span, Some(Span::new(3, 8)));
        assert_eq!(error.to_string(), "parse error: expected type");
    }
}
