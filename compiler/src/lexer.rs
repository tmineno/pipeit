// Lexer for Pipit .pdl source files.
//
// Tokenizes source according to the Pipit Language Specification §2 (lexical structure).
// Uses the `logos` crate for DFA-based lexing.
//
// Preconditions: input is valid UTF-8.
// Postconditions: returns all tokens with byte-offset spans, plus any lex errors.
// Failure modes: unrecognized characters produce `LexError`; lexing continues.
// Side effects: none.

use logos::Logos;
use std::fmt;

/// Byte-offset span in source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

/// A lexer error with location.
#[derive(Debug, Clone, PartialEq)]
pub struct LexError {
    pub span: Span,
    pub message: String,
}

/// Result of lexing: tokens plus any errors (non-fatal).
#[derive(Debug)]
pub struct LexResult {
    pub tokens: Vec<(Token, Span)>,
    pub errors: Vec<LexError>,
}

/// Pipit token types.
///
/// Keywords and symbols are matched as fixed strings.
/// Literals carry parsed values. Identifiers carry no value — use the span
/// to retrieve the text from the source.
#[derive(Logos, Debug, Clone, PartialEq)]
#[logos(skip r"[ \t]+|#[^\n]*")]
pub enum Token {
    // ── Keywords ──
    #[token("set")]
    Set,
    #[token("const")]
    Const,
    #[token("param")]
    Param,
    #[token("define")]
    Define,
    #[token("clock")]
    Clock,
    #[token("mode")]
    Mode,
    #[token("control")]
    Control,
    #[token("switch")]
    Switch,
    #[token("default")]
    Default,
    #[token("delay")]
    Delay,
    #[token("bind")]
    Bind,

    // ── Symbols ──
    #[token("|")]
    Pipe,
    #[token("->")]
    Arrow,
    #[token("@")]
    At,
    #[token(":")]
    Colon,
    #[token("?")]
    Question,
    #[token("$")]
    Dollar,
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    #[token(",")]
    Comma,
    #[token("=")]
    Equals,
    #[token("<")]
    Lt,
    #[token(">")]
    Gt,

    // ── Literals ──
    //
    // Frequency and size regexes must appear before Number so the longer
    // match (number + unit suffix) wins over a bare number.
    /// Frequency literal (e.g. `48kHz`). Value stored in Hz.
    #[regex(r"-?[0-9]+(\.[0-9]+)?([eE][+-]?[0-9]+)?(Hz|kHz|MHz|GHz)", parse_freq)]
    Freq(f64),

    /// Size literal (e.g. `64KB`). Value stored in bytes (binary: 1 KB = 1024).
    #[regex(r"[0-9]+(KB|MB|GB)", parse_size)]
    Size(u64),

    /// Numeric literal (int, float, exponent, negative).
    #[regex(r"-?[0-9]+(\.[0-9]+)?([eE][+-]?[0-9]+)?", parse_number)]
    Number(f64),

    /// String literal with `\"` and `\\` escapes.
    #[regex(r#""([^"\\]|\\.)*""#, parse_string)]
    StringLit(String),

    // ── Identifier ──
    //
    // Placed after keywords — logos prioritises fixed `#[token]` matches
    // over regex for the same length, so `set` matches Set, not Ident.
    /// Identifier: `[a-zA-Z_][a-zA-Z0-9_]*`
    #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*")]
    Ident,

    // ── Structure ──
    /// One or more newlines (significant — statement terminator per BNF §10).
    #[regex(r"\n+")]
    Newline,
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Set => write!(f, "set"),
            Token::Const => write!(f, "const"),
            Token::Param => write!(f, "param"),
            Token::Define => write!(f, "define"),
            Token::Clock => write!(f, "clock"),
            Token::Mode => write!(f, "mode"),
            Token::Control => write!(f, "control"),
            Token::Switch => write!(f, "switch"),
            Token::Default => write!(f, "default"),
            Token::Delay => write!(f, "delay"),
            Token::Bind => write!(f, "bind"),
            Token::Pipe => write!(f, "|"),
            Token::Arrow => write!(f, "->"),
            Token::At => write!(f, "@"),
            Token::Colon => write!(f, ":"),
            Token::Question => write!(f, "?"),
            Token::Dollar => write!(f, "$"),
            Token::LParen => write!(f, "("),
            Token::RParen => write!(f, ")"),
            Token::LBrace => write!(f, "{{"),
            Token::RBrace => write!(f, "}}"),
            Token::LBracket => write!(f, "["),
            Token::RBracket => write!(f, "]"),
            Token::Comma => write!(f, ","),
            Token::Equals => write!(f, "="),
            Token::Lt => write!(f, "<"),
            Token::Gt => write!(f, ">"),
            Token::Freq(v) => write!(f, "{v}Hz"),
            Token::Size(v) => write!(f, "{v}B"),
            Token::Number(v) => write!(f, "{v}"),
            Token::StringLit(s) => write!(f, "\"{s}\""),
            Token::Ident => write!(f, "<ident>"),
            Token::Newline => write!(f, "<newline>"),
        }
    }
}

// ── Callbacks ──

fn parse_number(lex: &mut logos::Lexer<'_, Token>) -> Option<f64> {
    lex.slice().parse().ok()
}

fn parse_freq(lex: &mut logos::Lexer<'_, Token>) -> Option<f64> {
    let slice = lex.slice();
    let unit_start = slice.find(|c: char| c.is_alphabetic())?;
    let (num_str, unit) = slice.split_at(unit_start);
    let num: f64 = num_str.parse().ok()?;
    let multiplier = match unit {
        "Hz" => 1.0,
        "kHz" => 1_000.0,
        "MHz" => 1_000_000.0,
        "GHz" => 1_000_000_000.0,
        _ => return None,
    };
    Some(num * multiplier)
}

fn parse_size(lex: &mut logos::Lexer<'_, Token>) -> Option<u64> {
    let slice = lex.slice();
    let unit_start = slice.find(|c: char| c.is_alphabetic())?;
    let (num_str, unit) = slice.split_at(unit_start);
    let num: u64 = num_str.parse().ok()?;
    let multiplier: u64 = match unit {
        "KB" => 1_024,
        "MB" => 1_024 * 1_024,
        "GB" => 1_024 * 1_024 * 1_024,
        _ => return None,
    };
    num.checked_mul(multiplier)
}

fn parse_string(lex: &mut logos::Lexer<'_, Token>) -> Option<String> {
    let slice = lex.slice();
    let inner = &slice[1..slice.len() - 1]; // strip quotes
    let mut result = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next()? {
                '"' => result.push('"'),
                '\\' => result.push('\\'),
                _ => {
                    // Spec only supports \" and \\. Reject unknown escapes.
                    return None;
                }
            }
        } else {
            result.push(c);
        }
    }
    Some(result)
}

// ── Public API ──

/// Lex a Pipit source string into tokens.
///
/// Returns all successfully parsed tokens together with any errors for
/// unrecognised characters. Lexing is non-fatal: errors are collected and
/// the lexer continues past bad characters.
pub fn lex(source: &str) -> LexResult {
    let lexer = Token::lexer(source);
    let mut tokens = Vec::new();
    let mut errors = Vec::new();

    for (result, range) in lexer.spanned() {
        let span = Span {
            start: range.start,
            end: range.end,
        };
        match result {
            Ok(token) => tokens.push((token, span)),
            Err(()) => errors.push(LexError {
                span,
                message: format!("unexpected character: {:?}", &source[span.start..span.end]),
            }),
        }
    }

    LexResult { tokens, errors }
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: lex and assert no errors, return token list.
    fn lex_ok(source: &str) -> Vec<Token> {
        let result = lex(source);
        assert!(
            result.errors.is_empty(),
            "unexpected lex errors: {:?}",
            result.errors
        );
        result.tokens.into_iter().map(|(t, _)| t).collect()
    }

    /// Helper: lex and return (tokens, errors).
    fn lex_all(source: &str) -> (Vec<Token>, Vec<LexError>) {
        let result = lex(source);
        let tokens = result.tokens.into_iter().map(|(t, _)| t).collect();
        (tokens, result.errors)
    }

    // ── Keywords ──

    #[test]
    fn keywords() {
        let tokens = lex_ok("set const param define clock mode control switch default delay bind");
        assert_eq!(
            tokens,
            vec![
                Token::Set,
                Token::Const,
                Token::Param,
                Token::Define,
                Token::Clock,
                Token::Mode,
                Token::Control,
                Token::Switch,
                Token::Default,
                Token::Delay,
                Token::Bind,
            ]
        );
    }

    #[test]
    fn keyword_vs_ident() {
        // `setting` is an identifier, not keyword `set` + `ting`
        let tokens = lex_ok("set setting");
        assert_eq!(tokens, vec![Token::Set, Token::Ident]);
    }

    #[test]
    fn bind_keyword_vs_ident() {
        // `binding` is an identifier, not keyword `bind` + `ing`
        let tokens = lex_ok("bind binding");
        assert_eq!(tokens, vec![Token::Bind, Token::Ident]);
    }

    // ── Symbols ──

    #[test]
    fn symbols() {
        let tokens = lex_ok("| -> @ : ? $ ( ) { } [ ] , =");
        assert_eq!(
            tokens,
            vec![
                Token::Pipe,
                Token::Arrow,
                Token::At,
                Token::Colon,
                Token::Question,
                Token::Dollar,
                Token::LParen,
                Token::RParen,
                Token::LBrace,
                Token::RBrace,
                Token::LBracket,
                Token::RBracket,
                Token::Comma,
                Token::Equals,
            ]
        );
    }

    // ── Number literals ──

    #[test]
    fn number_integer() {
        let tokens = lex_ok("42");
        assert_eq!(tokens, vec![Token::Number(42.0)]);
    }

    #[test]
    fn number_float() {
        let tokens = lex_ok("3.25");
        assert_eq!(tokens, vec![Token::Number(3.25)]);
    }

    #[test]
    fn number_negative() {
        let tokens = lex_ok("-1.5");
        assert_eq!(tokens, vec![Token::Number(-1.5)]);
    }

    #[test]
    fn number_exponent() {
        let tokens = lex_ok("1e-3");
        assert_eq!(tokens, vec![Token::Number(0.001)]);
    }

    #[test]
    fn number_negative_exponent() {
        let tokens = lex_ok("-2.5e10");
        assert_eq!(tokens, vec![Token::Number(-2.5e10)]);
    }

    // ── Frequency literals ──

    #[test]
    fn freq_hz() {
        let tokens = lex_ok("100Hz");
        assert_eq!(tokens, vec![Token::Freq(100.0)]);
    }

    #[test]
    fn freq_khz() {
        let tokens = lex_ok("48kHz");
        assert_eq!(tokens, vec![Token::Freq(48_000.0)]);
    }

    #[test]
    fn freq_mhz() {
        let tokens = lex_ok("10MHz");
        assert_eq!(tokens, vec![Token::Freq(10_000_000.0)]);
    }

    #[test]
    fn freq_ghz() {
        let tokens = lex_ok("2.4GHz");
        assert_eq!(tokens, vec![Token::Freq(2_400_000_000.0)]);
    }

    // ── Size literals ──

    #[test]
    fn size_kb() {
        let tokens = lex_ok("64KB");
        assert_eq!(tokens, vec![Token::Size(64 * 1024)]);
    }

    #[test]
    fn size_mb() {
        let tokens = lex_ok("256MB");
        assert_eq!(tokens, vec![Token::Size(256 * 1024 * 1024)]);
    }

    #[test]
    fn size_gb() {
        let tokens = lex_ok("1GB");
        assert_eq!(tokens, vec![Token::Size(1024 * 1024 * 1024)]);
    }

    // ── String literals ──

    #[test]
    fn string_simple() {
        let tokens = lex_ok(r#""hello""#);
        assert_eq!(tokens, vec![Token::StringLit("hello".into())]);
    }

    #[test]
    fn string_escape_quote() {
        let tokens = lex_ok(r#""say \"hi\"""#);
        assert_eq!(tokens, vec![Token::StringLit(r#"say "hi""#.into())]);
    }

    #[test]
    fn string_escape_backslash() {
        let tokens = lex_ok(r#""a\\b""#);
        assert_eq!(tokens, vec![Token::StringLit(r"a\b".into())]);
    }

    // ── Identifiers ──

    #[test]
    fn identifiers() {
        let tokens = lex_ok("foo _bar baz_123");
        assert_eq!(tokens, vec![Token::Ident, Token::Ident, Token::Ident]);
    }

    // ── Newlines ──

    #[test]
    fn newlines_significant() {
        let tokens = lex_ok("a\nb");
        assert_eq!(tokens, vec![Token::Ident, Token::Newline, Token::Ident]);
    }

    #[test]
    fn multiple_newlines_collapsed() {
        let tokens = lex_ok("a\n\n\nb");
        assert_eq!(tokens, vec![Token::Ident, Token::Newline, Token::Ident]);
    }

    // ── Comments ──

    #[test]
    fn comment_skipped() {
        let tokens = lex_ok("foo # this is a comment\nbar");
        assert_eq!(tokens, vec![Token::Ident, Token::Newline, Token::Ident]);
    }

    #[test]
    fn comment_only_line() {
        let tokens = lex_ok("# full line comment");
        assert!(tokens.is_empty());
    }

    // ── Spans ──

    #[test]
    fn spans_correct() {
        let result = lex("set foo");
        assert!(result.errors.is_empty());
        assert_eq!(result.tokens.len(), 2);
        assert_eq!(result.tokens[0].1, Span { start: 0, end: 3 });
        assert_eq!(result.tokens[1].1, Span { start: 4, end: 7 });
    }

    // ── Pipeline expression ──

    #[test]
    fn pipeline_expression() {
        let tokens = lex_ok("adc(0) | fft(256) | fir(coeff) -> signal");
        assert_eq!(
            tokens,
            vec![
                Token::Ident, // adc
                Token::LParen,
                Token::Number(0.0),
                Token::RParen,
                Token::Pipe,
                Token::Ident, // fft
                Token::LParen,
                Token::Number(256.0),
                Token::RParen,
                Token::Pipe,
                Token::Ident, // fir
                Token::LParen,
                Token::Ident, // coeff
                Token::RParen,
                Token::Arrow,
                Token::Ident, // signal
            ]
        );
    }

    // ── Task statement ──

    #[test]
    fn task_statement() {
        let source = "clock 48kHz audio {\n  adc(0) | fir(coeff)\n}";
        let tokens = lex_ok(source);
        assert_eq!(
            tokens,
            vec![
                Token::Clock,
                Token::Freq(48_000.0),
                Token::Ident, // audio
                Token::LBrace,
                Token::Newline,
                Token::Ident, // adc
                Token::LParen,
                Token::Number(0.0),
                Token::RParen,
                Token::Pipe,
                Token::Ident, // fir
                Token::LParen,
                Token::Ident, // coeff
                Token::RParen,
                Token::Newline,
                Token::RBrace,
            ]
        );
    }

    // ── Error recovery ──

    #[test]
    fn error_recovery() {
        let (tokens, errors) = lex_all("foo ~ bar");
        // `~` is not a valid token
        assert_eq!(tokens, vec![Token::Ident, Token::Ident]);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].span, Span { start: 4, end: 5 });
    }

    // ── Full example.pdl snippet ──

    #[test]
    fn example_pdl_snippet() {
        let source = r#"set mem = 256MB
set scheduler = "static"
const coeff = [0.1, 0.4, 0.1]
"#;
        let tokens = lex_ok(source);
        assert_eq!(
            tokens,
            vec![
                // set mem = 256MB
                Token::Set,
                Token::Ident, // mem
                Token::Equals,
                Token::Size(256 * 1024 * 1024),
                Token::Newline,
                // set scheduler = "static"
                Token::Set,
                Token::Ident, // scheduler
                Token::Equals,
                Token::StringLit("static".into()),
                Token::Newline,
                // const coeff = [0.1, 0.4, 0.1]
                Token::Const,
                Token::Ident, // coeff
                Token::Equals,
                Token::LBracket,
                Token::Number(0.1),
                Token::Comma,
                Token::Number(0.4),
                Token::Comma,
                Token::Number(0.1),
                Token::RBracket,
                Token::Newline,
            ]
        );
    }
}
