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

use serde::Serialize;

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

// ── JSON serialization ──────────────────────────────────────────────

/// Unified JSON representation for both semantic and parse diagnostics.
///
/// Output format: one JSON object per line (JSONL) to stderr.
/// Both semantic and parse errors emit the same top-level schema.
#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticJson {
    pub kind: &'static str,
    pub level: &'static str,
    pub code: Option<&'static str>,
    pub message: String,
    pub span: SpanJson,
    pub hint: Option<String>,
    pub related_spans: Vec<RelatedSpanJson>,
    pub cause_chain: Vec<CauseRecordJson>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SpanJson {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct RelatedSpanJson {
    pub span: SpanJson,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CauseRecordJson {
    pub message: String,
    pub span: Option<SpanJson>,
}

impl Diagnostic {
    /// Convert to the unified JSON representation (kind = "semantic").
    pub fn to_json(&self) -> DiagnosticJson {
        DiagnosticJson {
            kind: "semantic",
            level: match self.level {
                DiagLevel::Error => "error",
                DiagLevel::Warning => "warning",
            },
            code: self.code.map(|c| c.0),
            message: self.message.clone(),
            span: SpanJson {
                start: self.span.start,
                end: self.span.end,
            },
            hint: self.hint.clone(),
            related_spans: self
                .related_spans
                .iter()
                .map(|r| RelatedSpanJson {
                    span: SpanJson {
                        start: r.span.start,
                        end: r.span.end,
                    },
                    label: r.label.clone(),
                })
                .collect(),
            cause_chain: self
                .cause_chain
                .iter()
                .map(|c| CauseRecordJson {
                    message: c.message.clone(),
                    span: c.span.map(|s| SpanJson {
                        start: s.start,
                        end: s.end,
                    }),
                })
                .collect(),
        }
    }
}

impl DiagnosticJson {
    /// Create a parse-error JSON diagnostic from chumsky error info.
    pub fn from_parse_error(message: String, span_start: usize, span_end: usize) -> Self {
        DiagnosticJson {
            kind: "parse",
            level: "error",
            code: None,
            message,
            span: SpanJson {
                start: span_start,
                end: span_end,
            },
            hint: None,
            related_spans: Vec::new(),
            cause_chain: Vec::new(),
        }
    }
}

// ── Stable diagnostic code registry ──────────────────────────────────

/// Stable diagnostic code constants.
///
/// Compatibility policy:
/// - Once assigned, a code must never be reassigned to a different meaning.
/// - Removed diagnostics retire their code permanently.
/// - Changing semantics requires a new code + deprecation note.
/// - See `doc/DIAGNOSTIC_CODES.md` for the full registry.
pub mod codes {
    use super::DiagCode;

    // ── Resolve (E0001-E0099, W0001-W0099) ───────────────────────────
    pub const E0001: DiagCode = DiagCode("E0001"); // duplicate const
    pub const E0002: DiagCode = DiagCode("E0002"); // duplicate param
    pub const E0003: DiagCode = DiagCode("E0003"); // duplicate define
    pub const E0004: DiagCode = DiagCode("E0004"); // duplicate task
    pub const E0005: DiagCode = DiagCode("E0005"); // cross-namespace collision
    pub const E0006: DiagCode = DiagCode("E0006"); // tap declared but never consumed
    pub const E0007: DiagCode = DiagCode("E0007"); // duplicate mode
    pub const E0008: DiagCode = DiagCode("E0008"); // undefined tap
    pub const E0009: DiagCode = DiagCode("E0009"); // duplicate tap
    pub const E0010: DiagCode = DiagCode("E0010"); // multiple writers to shared buffer
    pub const E0011: DiagCode = DiagCode("E0011"); // unknown actor or define
    pub const E0012: DiagCode = DiagCode("E0012"); // non-polymorphic actor with type args
    pub const E0013: DiagCode = DiagCode("E0013"); // wrong number of type arguments
    pub const E0014: DiagCode = DiagCode("E0014"); // undefined param
    pub const E0015: DiagCode = DiagCode("E0015"); // undefined const
    pub const E0016: DiagCode = DiagCode("E0016"); // runtime param in frame dimension
    pub const E0017: DiagCode = DiagCode("E0017"); // unknown name in shape constraint
    pub const E0018: DiagCode = DiagCode("E0018"); // undefined param in switch source
    pub const E0019: DiagCode = DiagCode("E0019"); // switch references undefined mode
    pub const E0020: DiagCode = DiagCode("E0020"); // mode not listed in switch
    pub const E0021: DiagCode = DiagCode("E0021"); // mode listed multiple times in switch
    pub const E0022: DiagCode = DiagCode("E0022"); // undefined tap as actor input
    pub const E0023: DiagCode = DiagCode("E0023"); // shared buffer has no writer
    pub const E0024: DiagCode = DiagCode("E0024"); // duplicate bind
    pub const E0025: DiagCode = DiagCode("E0025"); // bind target not referenced (reserved for Phase 2)
    pub const W0001: DiagCode = DiagCode("W0001"); // define shadows actor
    pub const W0002: DiagCode = DiagCode("W0002"); // deprecated switch default clause

    // ── Type infer (E0100-E0199) ─────────────────────────────────────
    pub const E0100: DiagCode = DiagCode("E0100"); // unknown type
    pub const E0101: DiagCode = DiagCode("E0101"); // ambiguous polymorphic call (upstream context)
    pub const E0102: DiagCode = DiagCode("E0102"); // ambiguous polymorphic call (no context)

    // ── Lower (E0200-E0299) ──────────────────────────────────────────
    pub const E0200: DiagCode = DiagCode("E0200"); // L1 type consistency
    pub const E0201: DiagCode = DiagCode("E0201"); // L2 widening safety
    pub const E0202: DiagCode = DiagCode("E0202"); // L3 rate/shape preservation
    pub const E0203: DiagCode = DiagCode("E0203"); // L4 not fully monomorphized
    pub const E0204: DiagCode = DiagCode("E0204"); // L4 no concrete instance
    pub const E0205: DiagCode = DiagCode("E0205"); // L5 unresolved input type
    pub const E0206: DiagCode = DiagCode("E0206"); // L5 unresolved output type

    // ── Analyze (E0300-E0399, W0300-W0399) ───────────────────────────
    pub const E0300: DiagCode = DiagCode("E0300"); // unresolved frame dimension
    pub const E0301: DiagCode = DiagCode("E0301"); // conflicting frame constraint (upstream)
    pub const E0302: DiagCode = DiagCode("E0302"); // conflicting dimension (span vs edge)
    pub const E0303: DiagCode = DiagCode("E0303"); // type mismatch at pipe
    pub const E0304: DiagCode = DiagCode("E0304"); // SDF balance unsolvable
    pub const E0305: DiagCode = DiagCode("E0305"); // feedback loop with no delay
    pub const E0306: DiagCode = DiagCode("E0306"); // shared buffer rate mismatch
    pub const E0307: DiagCode = DiagCode("E0307"); // shared memory pool exceeded
    pub const E0308: DiagCode = DiagCode("E0308"); // param type mismatch
    pub const E0309: DiagCode = DiagCode("E0309"); // switch param non-int32 default
    pub const E0310: DiagCode = DiagCode("E0310"); // ctrl buffer type mismatch
    pub const E0311: DiagCode = DiagCode("E0311"); // bind target not referenced in any task
    pub const E0312: DiagCode = DiagCode("E0312"); // bind contract conflict (readers disagree on type/shape/rate)
    pub const W0300: DiagCode = DiagCode("W0300"); // inferred dim param ordering

    // ── Schedule (E0400-E0499, W0400-W0499) ──────────────────────────
    pub const E0400: DiagCode = DiagCode("E0400"); // unresolvable cycle
    pub const W0400: DiagCode = DiagCode("W0400"); // unsustainable tick rate

    // ── Graph (E0500-E0599) ──────────────────────────────────────────
    pub const E0500: DiagCode = DiagCode("E0500"); // tap not found in graph

    // ── Pipeline certs (E0600-E0699) ─────────────────────────────────
    pub const E0600: DiagCode = DiagCode("E0600"); // HIR verification failed
    pub const E0601: DiagCode = DiagCode("E0601"); // lowering verification failed
    pub const E0602: DiagCode = DiagCode("E0602"); // schedule verification failed
    pub const E0603: DiagCode = DiagCode("E0603"); // LIR verification failed

    // ── Usage (E0700-E0709) ────────────────────────────────────────
    pub const E0700: DiagCode = DiagCode("E0700"); // --actor-meta required for emit stage

    // ── Codegen / Bind (E0710-E0799, W0710-W0799) ──────────────────
    pub const E0710: DiagCode = DiagCode("E0710"); // bind: unsupported transport
    pub const E0711: DiagCode = DiagCode("E0711"); // bind: unsupported dtype for PPKT
    pub const E0712: DiagCode = DiagCode("E0712"); // bind: unresolved endpoint argument
    pub const W0710: DiagCode = DiagCode("W0710"); // bind: no endpoint address (placeholder)
    pub const W0711: DiagCode = DiagCode("W0711"); // bind: dtype unresolved, no I/O adapter

    /// All assigned codes for uniqueness enforcement.
    pub const ALL_CODES: &[DiagCode] = &[
        E0001, E0002, E0003, E0004, E0005, E0006, E0007, E0008, E0009, E0010, E0011, E0012, E0013,
        E0014, E0015, E0016, E0017, E0018, E0019, E0020, E0021, E0022, E0023, E0024, E0025, W0001,
        W0002, E0100, E0101, E0102, E0200, E0201, E0202, E0203, E0204, E0205, E0206, E0300, E0301,
        E0302, E0303, E0304, E0305, E0306, E0307, E0308, E0309, E0310, E0311, E0312, W0300, E0400,
        W0400, E0500, E0600, E0601, E0602, E0603, E0700, E0710, E0711, E0712, W0710, W0711,
    ];
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

    #[test]
    fn code_uniqueness() {
        use std::collections::HashSet;
        let mut seen = HashSet::new();
        for code in codes::ALL_CODES {
            assert!(seen.insert(code.0), "duplicate diagnostic code: {}", code.0);
        }
    }

    #[test]
    fn code_format_valid() {
        for code in codes::ALL_CODES {
            let s = code.0;
            assert!(
                s.len() == 5,
                "code '{}' must be 5 chars (E/W + 4 digits)",
                s
            );
            let prefix = s.as_bytes()[0];
            assert!(
                prefix == b'E' || prefix == b'W',
                "code '{}' must start with E or W",
                s
            );
            assert!(
                s[1..].chars().all(|c| c.is_ascii_digit()),
                "code '{}' suffix must be 4 digits",
                s
            );
        }
    }

    #[test]
    fn json_roundtrip_semantic() {
        let d = Diagnostic::new(DiagLevel::Error, dummy_span(), "type mismatch")
            .with_code(codes::E0200)
            .with_hint("insert a conversion actor")
            .with_related(dummy_span(), "source actor here")
            .with_cause("inferred float from upstream", Some(dummy_span()));
        let json = d.to_json();
        assert_eq!(json.kind, "semantic");
        assert_eq!(json.level, "error");
        assert_eq!(json.code, Some("E0200"));
        assert_eq!(json.message, "type mismatch");
        assert_eq!(json.hint.as_deref(), Some("insert a conversion actor"));
        assert_eq!(json.related_spans.len(), 1);
        assert_eq!(json.cause_chain.len(), 1);
        // Verify it serializes without error
        let text = serde_json::to_string(&json).unwrap();
        assert!(text.contains("\"kind\":\"semantic\""));
    }

    #[test]
    fn json_parse_error() {
        let json = DiagnosticJson::from_parse_error("unexpected token".into(), 10, 15);
        assert_eq!(json.kind, "parse");
        assert_eq!(json.level, "error");
        assert!(json.code.is_none());
        assert_eq!(json.span.start, 10);
        assert_eq!(json.span.end, 15);
        let text = serde_json::to_string(&json).unwrap();
        assert!(text.contains("\"kind\":\"parse\""));
    }

    #[test]
    fn code_count() {
        // 25 resolve errors + 2 resolve warnings
        // + 3 type_infer + 7 lower + 13 analyze errors + 1 analyze warning
        // + 1 schedule error + 1 schedule warning + 1 graph + 4 pipeline
        // + 1 usage (E0700) + 3 codegen errors (E0710-E0712) + 2 codegen warnings (W0710-W0711)
        assert_eq!(codes::ALL_CODES.len(), 64);
    }
}
