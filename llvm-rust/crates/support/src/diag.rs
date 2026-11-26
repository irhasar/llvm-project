//! Source spans and diagnostics.

use core::fmt;

/// A half-open byte range in a source buffer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    /// Creates a span covering `start..end`.
    pub const fn new(start: usize, end: usize) -> Self {
        Self {
            start: start as u32,
            end: end as u32,
        }
    }

    /// Creates an empty span at `offset`.
    pub const fn point(offset: usize) -> Self {
        Self::new(offset, offset)
    }

    /// Returns the byte length of the span.
    pub const fn len(self) -> usize {
        (self.end - self.start) as usize
    }

    /// Returns true when the span covers no bytes.
    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }

    /// Returns the smallest span covering both inputs.
    pub const fn merge(self, other: Self) -> Self {
        let start = if self.start < other.start {
            self.start
        } else {
            other.start
        };
        let end = if self.end > other.end {
            self.end
        } else {
            other.end
        };
        Self { start, end }
    }
}

/// Severity of a diagnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Severity {
    Error,
    Warning,
    Note,
}

impl Severity {
    /// Returns the lowercase label used when rendering.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Note => "note",
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One message attached to an optional source location.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub span: Option<Span>,
}

impl Diagnostic {
    /// Creates a diagnostic.
    pub fn new(severity: Severity, message: impl Into<String>, span: Option<Span>) -> Self {
        Self {
            severity,
            message: message.into(),
            span,
        }
    }

    /// Creates an error diagnostic.
    pub fn error(message: impl Into<String>, span: Option<Span>) -> Self {
        Self::new(Severity::Error, message, span)
    }

    /// Creates a warning diagnostic.
    pub fn warning(message: impl Into<String>, span: Option<Span>) -> Self {
        Self::new(Severity::Warning, message, span)
    }

    /// Creates a note diagnostic.
    pub fn note(message: impl Into<String>, span: Option<Span>) -> Self {
        Self::new(Severity::Note, message, span)
    }

    /// Renders the diagnostic with a `file:line:col` prefix and a caret line.
    ///
    /// `source` must be the buffer the span refers to.
    pub fn render(&self, path: &str, source: &str) -> String {
        let mut out = String::new();
        if let Some(span) = self.span {
            let (line, column) = line_column(source, span.start as usize);
            out.push_str(&format!(
                "{path}:{line}:{column}: {}: {}\n",
                self.severity, self.message
            ));
            if let Some(text) = source.lines().nth(line - 1) {
                out.push_str(text);
                out.push('\n');
                let indent = column.saturating_sub(1);
                out.push_str(&" ".repeat(indent));
                let width = span
                    .len()
                    .max(1)
                    .min(text.len().saturating_sub(indent).max(1));
                out.push('^');
                for _ in 1..width {
                    out.push('~');
                }
                out.push('\n');
            }
        } else {
            out.push_str(&format!("{path}: {}: {}\n", self.severity, self.message));
        }
        out
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.severity, self.message)
    }
}

/// Returns the one-based line and column for a byte offset.
fn line_column(source: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(source.len());
    let mut line = 1;
    let mut column = 1;
    for (idx, byte) in source.bytes().enumerate() {
        if idx >= offset {
            break;
        }
        if byte == b'\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    (line, column)
}

/// A collection of diagnostics produced by a pass.
#[derive(Clone, Debug, Default)]
pub struct Diagnostics {
    items: Vec<Diagnostic>,
    errors: usize,
}

impl Diagnostics {
    /// Creates an empty collection.
    pub const fn new() -> Self {
        Self {
            items: Vec::new(),
            errors: 0,
        }
    }

    /// Adds a diagnostic.
    pub fn push(&mut self, diagnostic: Diagnostic) {
        if diagnostic.severity == Severity::Error {
            self.errors += 1;
        }
        self.items.push(diagnostic);
    }

    /// Adds an error diagnostic.
    pub fn error(&mut self, message: impl Into<String>, span: Option<Span>) {
        self.push(Diagnostic::error(message, span));
    }

    /// Adds a warning diagnostic.
    pub fn warning(&mut self, message: impl Into<String>, span: Option<Span>) {
        self.push(Diagnostic::warning(message, span));
    }

    /// Adds a note diagnostic.
    pub fn note(&mut self, message: impl Into<String>, span: Option<Span>) {
        self.push(Diagnostic::note(message, span));
    }

    /// Returns true when at least one error was recorded.
    pub fn has_errors(&self) -> bool {
        self.errors > 0
    }

    /// Returns the number of errors.
    pub fn error_count(&self) -> usize {
        self.errors
    }

    /// Returns true when no diagnostics were recorded.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Returns the number of diagnostics.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Iterates over diagnostics.
    pub fn iter(&self) -> impl Iterator<Item = &Diagnostic> {
        self.items.iter()
    }

    /// Appends all diagnostics from `other`.
    pub fn extend(&mut self, other: Self) {
        for diagnostic in other.items {
            self.push(diagnostic);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn span_merge_and_len() {
        let a = Span::new(2, 4);
        let b = Span::new(6, 7);
        assert_eq!(a.len(), 2);
        assert_eq!(a.merge(b), Span::new(2, 7));
        assert!(Span::point(3).is_empty());
    }

    #[test]
    fn counts_only_errors() {
        let mut diagnostics = Diagnostics::new();
        diagnostics.error("bad", None);
        diagnostics.warning("meh", None);
        diagnostics.note("fyi", None);
        assert!(diagnostics.has_errors());
        assert_eq!(diagnostics.error_count(), 1);
        assert_eq!(diagnostics.len(), 3);
    }

    #[test]
    fn renders_line_and_column() {
        let source = "define i32 @f() {\n  ret i32 0\n}\n";
        let offset = source.find("ret").unwrap();
        let diagnostic =
            Diagnostic::error("unexpected return", Some(Span::new(offset, offset + 3)));
        let rendered = diagnostic.render("t.ll", source);
        assert!(rendered.starts_with("t.ll:2:3: error: unexpected return\n"));
        assert!(rendered.contains("  ret i32 0"));
        assert!(rendered.contains("  ^~~"));
    }
}
