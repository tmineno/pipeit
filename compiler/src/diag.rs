// diag.rs — Unified diagnostics model
//
// Provides the shared diagnostic types used across all compiler phases.
// See ADR-022 for design rationale.
//
// Preconditions: none (types only).
// Postconditions: none (types only).
// Failure modes: none.
// Side effects: none.

use std::fmt;

use crate::ast::Span;

// ── Diagnostic code ──────────────────────────────────────────────────────

/// A stable diagnostic code (e.g., `E0001`, `W0300`).
///
/// Codes are `&'static str` constants defined in the `codes` module.
/// Once assigned, a code must never be reassigned to a different semantic
/// meaning — see `DIAGNOSTIC_CODES.md` for the compatibility policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DiagCode(pub &'static str);

impl fmt::Display for DiagCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ── Severity level ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagLevel {
    Error,
    Warning,
}

// ── Related span ─────────────────────────────────────────────────────────

/// A secondary source location providing context for a diagnostic.
#[derive(Debug, Clone)]
pub struct RelatedSpan {
    pub span: Span,
    pub label: String,
}

// ── Cause record ─────────────────────────────────────────────────────────

/// One link in a cause chain explaining a propagated constraint failure.
#[derive(Debug, Clone)]
pub struct CauseRecord {
    pub message: String,
    pub span: Option<Span>,
}

// ── Diagnostic ───────────────────────────────────────────────────────────

/// A compiler diagnostic emitted by any phase.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub code: Option<DiagCode>,
    pub level: DiagLevel,
    pub span: Span,
    pub message: String,
    pub hint: Option<String>,
    pub related_spans: Vec<RelatedSpan>,
    pub cause_chain: Vec<CauseRecord>,
}

impl Diagnostic {
    /// Create a new diagnostic with no code, hint, related spans, or causes.
    pub fn new(level: DiagLevel, span: Span, message: impl Into<String>) -> Self {
        Self {
            code: None,
            level,
            span,
            message: message.into(),
            hint: None,
            related_spans: Vec::new(),
            cause_chain: Vec::new(),
        }
    }

    /// Attach a stable diagnostic code.
    pub fn with_code(mut self, code: DiagCode) -> Self {
        self.code = Some(code);
        self
    }

    /// Attach a remediation hint.
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Attach a related span.
    pub fn with_related(mut self, span: Span, label: impl Into<String>) -> Self {
        self.related_spans.push(RelatedSpan {
            span,
            label: label.into(),
        });
        self
    }

    /// Attach a cause record to the chain.
    pub fn with_cause(mut self, message: impl Into<String>, span: Option<Span>) -> Self {
        self.cause_chain.push(CauseRecord {
            message: message.into(),
            span,
        });
        self
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let level = match self.level {
            DiagLevel::Error => "error",
            DiagLevel::Warning => "warning",
        };
        if let Some(code) = &self.code {
            write!(f, "{}[{}]: {}", level, code, self.message)?;
        } else {
            write!(f, "{}: {}", level, self.message)?;
        }
        if let Some(hint) = &self.hint {
            write!(f, "\n  hint: {}", hint)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_span() -> Span {
        use chumsky::span::Span as _;
        Span::new((), 0..1)
    }

    #[test]
    fn display_without_code() {
        let d = Diagnostic::new(DiagLevel::Error, dummy_span(), "something failed");
        assert_eq!(format!("{d}"), "error: something failed");
    }

    #[test]
    fn display_with_code() {
        let d = Diagnostic::new(DiagLevel::Warning, dummy_span(), "unused define")
            .with_code(DiagCode("W0001"));
        assert_eq!(format!("{d}"), "warning[W0001]: unused define");
    }

    #[test]
    fn builder_chain() {
        let d = Diagnostic::new(DiagLevel::Error, dummy_span(), "type mismatch")
            .with_code(DiagCode("E0200"))
            .with_hint("insert a conversion actor")
            .with_related(dummy_span(), "source actor here")
            .with_cause("inferred float from upstream", Some(dummy_span()));

        assert_eq!(d.code, Some(DiagCode("E0200")));
        assert_eq!(d.hint.as_deref(), Some("insert a conversion actor"));
        assert_eq!(d.related_spans.len(), 1);
        assert_eq!(d.cause_chain.len(), 1);
    }
}
