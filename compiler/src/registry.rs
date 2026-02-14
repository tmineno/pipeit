// registry.rs — Actor metadata registry
//
// Extracts actor metadata from C++ header files by scanning ACTOR() macro
// invocations at the text level. No C++ parsing — the macro has a fixed
// positional format that maps to simple string operations.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

// ── Data types ──────────────────────────────────────────────────────────────

/// Pipit wire types (spec §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipitType {
    Int8,
    Int16,
    Int32,
    Float,
    Double,
    Cfloat,
    Cdouble,
    Void,
}

/// Token count on an actor port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenCount {
    /// Fixed count, e.g. `IN(float, 1)`.
    Literal(u32),
    /// Symbolic count referencing a PARAM, e.g. `IN(float, N)`.
    Symbolic(String),
}

/// Whether a parameter is compile-time or runtime-swappable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamKind {
    Param,
    RuntimeParam,
}

/// C++ type of an actor parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParamType {
    Int,
    Float,
    Double,
    SpanFloat,
    SpanChar,
}

/// A single actor parameter declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorParam {
    pub kind: ParamKind,
    pub param_type: ParamType,
    pub name: String,
}

/// Metadata extracted from one ACTOR() macro invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorMeta {
    pub name: String,
    pub in_type: PipitType,
    pub in_count: TokenCount,
    pub out_type: PipitType,
    pub out_count: TokenCount,
    pub params: Vec<ActorParam>,
}

/// Errors that can occur during registry loading.
#[derive(Debug)]
pub enum RegistryError {
    IoError {
        path: PathBuf,
        source: std::io::Error,
    },
    ParseError {
        file: PathBuf,
        line: usize,
        message: String,
    },
    DuplicateActor {
        name: String,
        first: PathBuf,
        second: PathBuf,
    },
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RegistryError::IoError { path, source } => {
                write!(f, "{}: {}", path.display(), source)
            }
            RegistryError::ParseError {
                file,
                line,
                message,
            } => {
                write!(f, "{}:{}: {}", file.display(), line, message)
            }
            RegistryError::DuplicateActor {
                name,
                first,
                second,
            } => {
                write!(
                    f,
                    "duplicate actor '{}': first defined in {}, redefined in {}",
                    name,
                    first.display(),
                    second.display()
                )
            }
        }
    }
}

impl std::error::Error for RegistryError {}

// ── Registry ────────────────────────────────────────────────────────────────

/// Actor metadata registry. Populated by scanning C++ header files.
pub struct Registry {
    actors: HashMap<String, (ActorMeta, PathBuf)>,
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

impl Registry {
    pub fn new() -> Self {
        Registry {
            actors: HashMap::new(),
        }
    }

    /// Load actors from a C++ header file. Returns the number of actors found.
    pub fn load_header(&mut self, path: &Path) -> Result<usize, RegistryError> {
        let source = std::fs::read_to_string(path).map_err(|e| RegistryError::IoError {
            path: path.to_path_buf(),
            source: e,
        })?;

        let actors = scan_actors(&source, path)?;
        let count = actors.len();

        for actor in actors {
            if let Some((_, first_path)) = self.actors.get(&actor.name) {
                return Err(RegistryError::DuplicateActor {
                    name: actor.name.clone(),
                    first: first_path.clone(),
                    second: path.to_path_buf(),
                });
            }
            self.actors
                .insert(actor.name.clone(), (actor, path.to_path_buf()));
        }

        Ok(count)
    }

    pub fn lookup(&self, name: &str) -> Option<&ActorMeta> {
        self.actors.get(name).map(|(meta, _)| meta)
    }

    pub fn actors(&self) -> impl Iterator<Item = &ActorMeta> {
        self.actors.values().map(|(meta, _)| meta)
    }

    pub fn len(&self) -> usize {
        self.actors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.actors.is_empty()
    }
}

// ── Scanner ─────────────────────────────────────────────────────────────────

/// Scan a source string for ACTOR() macro invocations.
fn scan_actors(source: &str, file: &Path) -> Result<Vec<ActorMeta>, RegistryError> {
    let stripped = strip_comments(source);
    let mut results = Vec::new();
    let mut pos = 0;
    let bytes = stripped.as_bytes();

    while pos < bytes.len() {
        // Find "ACTOR(" at word boundary
        if let Some(idx) = stripped[pos..].find("ACTOR(") {
            let abs_idx = pos + idx;

            // Check word boundary: character before must not be alphanumeric or '_'
            if abs_idx > 0 {
                let prev = bytes[abs_idx - 1];
                if prev.is_ascii_alphanumeric() || prev == b'_' {
                    pos = abs_idx + 6;
                    continue;
                }
            }

            // Extract balanced parentheses content
            let paren_start = abs_idx + 5; // index of '('
            match extract_balanced(bytes, paren_start, b'(', b')') {
                Some(content_end) => {
                    let inner = &stripped[paren_start + 1..content_end];
                    let line = stripped[..abs_idx].chars().filter(|&c| c == '\n').count() + 1;

                    let actor = parse_actor_macro(inner, file, line)?;
                    results.push(actor);

                    pos = content_end + 1;
                }
                None => {
                    let line = stripped[..abs_idx].chars().filter(|&c| c == '\n').count() + 1;
                    return Err(RegistryError::ParseError {
                        file: file.to_path_buf(),
                        line,
                        message: "unbalanced parentheses in ACTOR() macro".to_string(),
                    });
                }
            }
        } else {
            break;
        }
    }

    Ok(results)
}

/// Strip C-style comments from source: `//` line comments and `/* */` blocks.
fn strip_comments(source: &str) -> String {
    let mut result = String::with_capacity(source.len());
    let bytes = source.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            // Line comment — skip to end of line, preserve newline
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
        } else if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            // Block comment — skip to */, preserve newlines for line counting
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                if bytes[i] == b'\n' {
                    result.push('\n');
                }
                i += 1;
            }
            if i + 1 < bytes.len() {
                i += 2; // skip */
            }
        } else if bytes[i] == b'"' {
            // String literal — don't strip inside strings
            result.push('"');
            i += 1;
            while i < bytes.len() && bytes[i] != b'"' {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    result.push(bytes[i] as char);
                    i += 1;
                }
                result.push(bytes[i] as char);
                i += 1;
            }
            if i < bytes.len() {
                result.push('"');
                i += 1;
            }
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }

    result
}

/// Extract balanced delimiter content. Returns index of closing delimiter.
fn extract_balanced(bytes: &[u8], start: usize, open: u8, close: u8) -> Option<usize> {
    if start >= bytes.len() || bytes[start] != open {
        return None;
    }

    let mut depth = 0;
    let mut i = start;

    while i < bytes.len() {
        if bytes[i] == open {
            depth += 1;
        } else if bytes[i] == close {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
        i += 1;
    }

    None
}

/// Split a string by commas at the top level (respecting nested `()` and `<>`).
fn split_top_level_commas(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth_paren = 0i32;
    let mut depth_angle = 0i32;
    let mut start = 0;
    let bytes = s.as_bytes();

    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'(' => depth_paren += 1,
            b')' => depth_paren -= 1,
            b'<' => depth_angle += 1,
            b'>' => depth_angle -= 1,
            b',' if depth_paren == 0 && depth_angle == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }

    parts.push(&s[start..]);
    parts
}

/// Parse the inner content of an ACTOR(...) invocation.
fn parse_actor_macro(inner: &str, file: &Path, line: usize) -> Result<ActorMeta, RegistryError> {
    let fields = split_top_level_commas(inner);

    if fields.len() < 3 {
        return Err(RegistryError::ParseError {
            file: file.to_path_buf(),
            line,
            message: format!(
                "ACTOR() requires at least 3 fields (name, IN, OUT), found {}",
                fields.len()
            ),
        });
    }

    let name = fields[0].trim().to_string();
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(RegistryError::ParseError {
            file: file.to_path_buf(),
            line,
            message: format!("invalid actor name: '{}'", name),
        });
    }

    let (in_type, in_count) = parse_port_spec(fields[1].trim(), "IN", file, line)?;
    let (out_type, out_count) = parse_port_spec(fields[2].trim(), "OUT", file, line)?;

    let mut params = Vec::new();
    for field in &fields[3..] {
        let trimmed = field.trim();
        if trimmed.is_empty() {
            continue;
        }
        let param = parse_param_spec(trimmed, file, line)?;
        params.push(param);
    }

    Ok(ActorMeta {
        name,
        in_type,
        in_count,
        out_type,
        out_count,
        params,
    })
}

/// Parse `IN(type, count)` or `OUT(type, count)`.
fn parse_port_spec(
    s: &str,
    expected_prefix: &str,
    file: &Path,
    line: usize,
) -> Result<(PipitType, TokenCount), RegistryError> {
    let rest = s
        .strip_prefix(expected_prefix)
        .and_then(|r| r.strip_prefix('('))
        .and_then(|r| r.strip_suffix(')'))
        .ok_or_else(|| RegistryError::ParseError {
            file: file.to_path_buf(),
            line,
            message: format!("expected {}(type, count), found '{}'", expected_prefix, s),
        })?;

    let parts = split_top_level_commas(rest);
    if parts.len() != 2 {
        return Err(RegistryError::ParseError {
            file: file.to_path_buf(),
            line,
            message: format!(
                "{}() requires exactly 2 fields (type, count), found {}",
                expected_prefix,
                parts.len()
            ),
        });
    }

    let pipit_type = parse_pipit_type(parts[0].trim(), file, line)?;
    let count = parse_token_count(parts[1].trim());

    Ok((pipit_type, count))
}

/// Parse `PARAM(type, name)` or `RUNTIME_PARAM(type, name)`.
fn parse_param_spec(s: &str, file: &Path, line: usize) -> Result<ActorParam, RegistryError> {
    let (kind, rest) = if let Some(r) = s.strip_prefix("RUNTIME_PARAM(") {
        (ParamKind::RuntimeParam, r)
    } else if let Some(r) = s.strip_prefix("PARAM(") {
        (ParamKind::Param, r)
    } else {
        return Err(RegistryError::ParseError {
            file: file.to_path_buf(),
            line,
            message: format!("expected PARAM() or RUNTIME_PARAM(), found '{}'", s),
        });
    };

    let rest = rest
        .strip_suffix(')')
        .ok_or_else(|| RegistryError::ParseError {
            file: file.to_path_buf(),
            line,
            message: format!("missing closing ')' in parameter spec: '{}'", s),
        })?;

    let parts = split_top_level_commas(rest);
    if parts.len() != 2 {
        return Err(RegistryError::ParseError {
            file: file.to_path_buf(),
            line,
            message: format!(
                "parameter spec requires exactly 2 fields (type, name), found {}",
                parts.len()
            ),
        });
    }

    let param_type = parse_param_type(parts[0].trim(), file, line)?;
    let name = parts[1].trim().to_string();

    Ok(ActorParam {
        kind,
        param_type,
        name,
    })
}

/// Map a type string to PipitType.
fn parse_pipit_type(s: &str, file: &Path, line: usize) -> Result<PipitType, RegistryError> {
    match s {
        "int8" | "int8_t" => Ok(PipitType::Int8),
        "int16" | "int16_t" => Ok(PipitType::Int16),
        "int32" | "int32_t" | "std::int32_t" => Ok(PipitType::Int32),
        "float" => Ok(PipitType::Float),
        "double" => Ok(PipitType::Double),
        "cfloat" | "std::complex<float>" => Ok(PipitType::Cfloat),
        "cdouble" | "std::complex<double>" => Ok(PipitType::Cdouble),
        "void" => Ok(PipitType::Void),
        _ => Err(RegistryError::ParseError {
            file: file.to_path_buf(),
            line,
            message: format!("unknown pipit type: '{}'", s),
        }),
    }
}

/// Map a type string to ParamType.
fn parse_param_type(s: &str, file: &Path, line: usize) -> Result<ParamType, RegistryError> {
    match s {
        "int" | "int32" | "int32_t" | "std::int32_t" => Ok(ParamType::Int),
        "float" => Ok(ParamType::Float),
        "double" => Ok(ParamType::Double),
        "std::span<const float>" => Ok(ParamType::SpanFloat),
        "std::span<const char>" => Ok(ParamType::SpanChar),
        _ => Err(RegistryError::ParseError {
            file: file.to_path_buf(),
            line,
            message: format!("unknown parameter type: '{}'", s),
        }),
    }
}

/// Parse a token count: integer literal or symbolic name.
fn parse_token_count(s: &str) -> TokenCount {
    if let Ok(n) = s.parse::<u32>() {
        TokenCount::Literal(n)
    } else {
        TokenCount::Symbolic(s.to_string())
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: scan a single ACTOR() from a string and return the ActorMeta.
    fn scan_one(src: &str) -> ActorMeta {
        let path = PathBuf::from("test.h");
        let actors = scan_actors(src, &path).expect("scan failed");
        assert_eq!(actors.len(), 1, "expected exactly 1 actor");
        actors.into_iter().next().unwrap()
    }

    #[test]
    fn parse_simple_actor() {
        let a = scan_one("ACTOR(mag, IN(cfloat, 1), OUT(float, 1)) { return ACTOR_OK; }");
        assert_eq!(a.name, "mag");
        assert_eq!(a.in_type, PipitType::Cfloat);
        assert_eq!(a.in_count, TokenCount::Literal(1));
        assert_eq!(a.out_type, PipitType::Float);
        assert_eq!(a.out_count, TokenCount::Literal(1));
        assert!(a.params.is_empty());
    }

    #[test]
    fn parse_actor_with_param() {
        let a = scan_one(
            "ACTOR(adc, IN(void, 0), OUT(float, 1), PARAM(int, channel)) { return ACTOR_OK; }",
        );
        assert_eq!(a.name, "adc");
        assert_eq!(a.in_type, PipitType::Void);
        assert_eq!(a.in_count, TokenCount::Literal(0));
        assert_eq!(a.out_type, PipitType::Float);
        assert_eq!(a.out_count, TokenCount::Literal(1));
        assert_eq!(a.params.len(), 1);
        assert_eq!(a.params[0].kind, ParamKind::Param);
        assert_eq!(a.params[0].param_type, ParamType::Int);
        assert_eq!(a.params[0].name, "channel");
    }

    #[test]
    fn parse_actor_with_runtime_param() {
        let a = scan_one(
            "ACTOR(mul, IN(float, 1), OUT(float, 1), RUNTIME_PARAM(float, gain)) { out[0] = in[0] * gain; return ACTOR_OK; }",
        );
        assert_eq!(a.name, "mul");
        assert_eq!(a.params.len(), 1);
        assert_eq!(a.params[0].kind, ParamKind::RuntimeParam);
        assert_eq!(a.params[0].param_type, ParamType::Float);
        assert_eq!(a.params[0].name, "gain");
    }

    #[test]
    fn parse_symbolic_count() {
        let a = scan_one(
            "ACTOR(fft, IN(float, N), OUT(cfloat, N), PARAM(int, N)) { return ACTOR_OK; }",
        );
        assert_eq!(a.name, "fft");
        assert_eq!(a.in_type, PipitType::Float);
        assert_eq!(a.in_count, TokenCount::Symbolic("N".to_string()));
        assert_eq!(a.out_type, PipitType::Cfloat);
        assert_eq!(a.out_count, TokenCount::Symbolic("N".to_string()));
        assert_eq!(a.params.len(), 1);
        assert_eq!(a.params[0].name, "N");
    }

    #[test]
    fn parse_span_param() {
        let a = scan_one(
            "ACTOR(fir, IN(float, N), OUT(float, 1), PARAM(int, N), PARAM(std::span<const float>, coeff)) { return ACTOR_OK; }",
        );
        assert_eq!(a.name, "fir");
        assert_eq!(a.params.len(), 2);
        assert_eq!(a.params[1].param_type, ParamType::SpanFloat);
        assert_eq!(a.params[1].name, "coeff");
    }

    #[test]
    fn parse_string_span_param() {
        let a = scan_one(
            "ACTOR(csvwrite, IN(float, 1), OUT(void, 0), PARAM(std::span<const char>, path)) { return ACTOR_OK; }",
        );
        assert_eq!(a.name, "csvwrite");
        assert_eq!(a.out_type, PipitType::Void);
        assert_eq!(a.out_count, TokenCount::Literal(0));
        assert_eq!(a.params.len(), 1);
        assert_eq!(a.params[0].param_type, ParamType::SpanChar);
        assert_eq!(a.params[0].name, "path");
    }

    #[test]
    fn parse_multi_param() {
        let a = scan_one(
            "ACTOR(fir, IN(float, N), OUT(float, 1), PARAM(int, N), PARAM(std::span<const float>, coeff)) { return ACTOR_OK; }",
        );
        assert_eq!(a.params.len(), 2);
        assert_eq!(a.params[0].kind, ParamKind::Param);
        assert_eq!(a.params[0].param_type, ParamType::Int);
        assert_eq!(a.params[0].name, "N");
        assert_eq!(a.params[1].kind, ParamKind::Param);
        assert_eq!(a.params[1].param_type, ParamType::SpanFloat);
        assert_eq!(a.params[1].name, "coeff");
    }

    #[test]
    fn parse_void_source() {
        let a = scan_one(
            "ACTOR(adc, IN(void, 0), OUT(float, 1), PARAM(int, channel)) { return ACTOR_OK; }",
        );
        assert_eq!(a.in_type, PipitType::Void);
        assert_eq!(a.in_count, TokenCount::Literal(0));
    }

    #[test]
    fn parse_void_sink() {
        let a =
            scan_one("ACTOR(stdout, IN(float, 1), OUT(void, 0)) { printf(\"%f\\n\", in[0]); return ACTOR_OK; }");
        assert_eq!(a.out_type, PipitType::Void);
        assert_eq!(a.out_count, TokenCount::Literal(0));
    }

    #[test]
    fn duplicate_actor_error() {
        let mut reg = Registry::new();
        let src = "ACTOR(foo, IN(float, 1), OUT(float, 1)) { return ACTOR_OK; }";
        // Write temp files
        let dir = std::env::temp_dir().join("pipit_test_dup");
        std::fs::create_dir_all(&dir).unwrap();
        let f1 = dir.join("a.h");
        let f2 = dir.join("b.h");
        std::fs::write(&f1, src).unwrap();
        std::fs::write(&f2, src).unwrap();

        reg.load_header(&f1).unwrap();
        let err = reg.load_header(&f2).unwrap_err();

        match err {
            RegistryError::DuplicateActor { name, .. } => assert_eq!(name, "foo"),
            _ => panic!("expected DuplicateActor error, got: {}", err),
        }

        // Cleanup
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unknown_type_error() {
        let path = PathBuf::from("test.h");
        let result = scan_actors(
            "ACTOR(bad, IN(uint64, 1), OUT(float, 1)) { return ACTOR_OK; }",
            &path,
        );
        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            RegistryError::ParseError { message, .. } => {
                assert!(message.contains("unknown pipit type"), "got: {}", message);
            }
            _ => panic!("expected ParseError, got: {}", err),
        }
    }

    #[test]
    fn skip_line_comments() {
        let src = r#"
// ACTOR(commented_out, IN(float, 1), OUT(float, 1)) { }
ACTOR(real, IN(float, 1), OUT(float, 1)) { return ACTOR_OK; }
"#;
        let path = PathBuf::from("test.h");
        let actors = scan_actors(src, &path).unwrap();
        assert_eq!(actors.len(), 1);
        assert_eq!(actors[0].name, "real");
    }

    #[test]
    fn skip_block_comments() {
        let src = r#"
/* ACTOR(commented_out, IN(float, 1), OUT(float, 1)) { } */
ACTOR(real, IN(float, 1), OUT(float, 1)) { return ACTOR_OK; }
"#;
        let path = PathBuf::from("test.h");
        let actors = scan_actors(src, &path).unwrap();
        assert_eq!(actors.len(), 1);
        assert_eq!(actors[0].name, "real");
    }

    #[test]
    fn load_examples_actors_h() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("examples/actors.h");
        if !path.exists() {
            // Skip if examples not available (CI without full checkout)
            return;
        }

        let mut reg = Registry::new();
        let count = reg.load_header(&path).unwrap();
        assert_eq!(count, 12, "expected 12 actors in examples/actors.h");
        assert_eq!(reg.len(), 12);

        // Spot-check a few actors
        let adc = reg.lookup("adc").expect("adc not found");
        assert_eq!(adc.in_type, PipitType::Void);
        assert_eq!(adc.in_count, TokenCount::Literal(0));
        assert_eq!(adc.out_type, PipitType::Float);
        assert_eq!(adc.out_count, TokenCount::Literal(1));
        assert_eq!(adc.params.len(), 1);
        assert_eq!(adc.params[0].name, "channel");

        let fft = reg.lookup("fft").expect("fft not found");
        assert_eq!(fft.in_type, PipitType::Float);
        assert_eq!(fft.in_count, TokenCount::Symbolic("N".to_string()));
        assert_eq!(fft.out_type, PipitType::Cfloat);

        let mul = reg.lookup("mul").expect("mul not found");
        assert_eq!(mul.params.len(), 1);
        assert_eq!(mul.params[0].kind, ParamKind::RuntimeParam);

        let csvwrite = reg.lookup("csvwrite").expect("csvwrite not found");
        assert_eq!(csvwrite.params[0].param_type, ParamType::SpanChar);

        let stdout = reg.lookup("stdout").expect("stdout not found");
        assert!(stdout.params.is_empty());
    }

    #[test]
    fn multiple_actors_in_one_file() {
        let src = r#"
ACTOR(a, IN(float, 1), OUT(float, 1)) { return ACTOR_OK; }
ACTOR(b, IN(int32, 2), OUT(double, 1)) { return ACTOR_OK; }
"#;
        let path = PathBuf::from("test.h");
        let actors = scan_actors(src, &path).unwrap();
        assert_eq!(actors.len(), 2);
        assert_eq!(actors[0].name, "a");
        assert_eq!(actors[1].name, "b");
    }
}
