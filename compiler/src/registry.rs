// registry.rs — Actor metadata registry
//
// Extracts actor metadata from C++ header files by scanning ACTOR() macro
// invocations at the text level. No C++ parsing — the macro has a fixed
// positional format that maps to simple string operations.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

// ── Data types ──────────────────────────────────────────────────────────────

/// Pipit wire types (spec §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

/// A port type expression: either a concrete Pipit type or a type parameter reference.
///
/// For concrete (non-polymorphic) actors, this is always `Concrete(...)`.
/// For polymorphic actors (`template <typename T>`), port types referencing the
/// type parameter use `TypeParam("T")`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TypeExpr {
    Concrete(PipitType),
    TypeParam(String),
}

impl TypeExpr {
    /// Extract the concrete PipitType, if this is not a type parameter.
    pub fn as_concrete(&self) -> Option<PipitType> {
        match self {
            TypeExpr::Concrete(t) => Some(*t),
            TypeExpr::TypeParam(_) => None,
        }
    }

    /// Extract the concrete PipitType, panicking if this is a type parameter.
    /// Use only when polymorphic actors are known to be resolved.
    pub fn unwrap_concrete(&self) -> PipitType {
        match self {
            TypeExpr::Concrete(t) => *t,
            TypeExpr::TypeParam(name) => {
                panic!("expected concrete type, found type parameter '{}'", name)
            }
        }
    }

    /// True if this is a concrete type (not a type parameter).
    pub fn is_concrete(&self) -> bool {
        matches!(self, TypeExpr::Concrete(_))
    }
}

impl From<PipitType> for TypeExpr {
    fn from(t: PipitType) -> Self {
        TypeExpr::Concrete(t)
    }
}

impl PartialEq<PipitType> for TypeExpr {
    fn eq(&self, other: &PipitType) -> bool {
        matches!(self, TypeExpr::Concrete(t) if t == other)
    }
}

impl fmt::Display for TypeExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeExpr::Concrete(t) => write!(f, "{}", t),
            TypeExpr::TypeParam(name) => write!(f, "{}", name),
        }
    }
}

impl fmt::Display for PipitType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PipitType::Int8 => write!(f, "int8"),
            PipitType::Int16 => write!(f, "int16"),
            PipitType::Int32 => write!(f, "int32"),
            PipitType::Float => write!(f, "float"),
            PipitType::Double => write!(f, "double"),
            PipitType::Cfloat => write!(f, "cfloat"),
            PipitType::Cdouble => write!(f, "cdouble"),
            PipitType::Void => write!(f, "void"),
        }
    }
}

/// Token count on an actor port.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TokenCount {
    /// Fixed count, e.g. `IN(float, 1)`.
    Literal(u32),
    /// Symbolic count referencing a PARAM, e.g. `IN(float, N)`.
    Symbolic(String),
}

/// Multi-dimensional shape of an actor port (v0.2.0).
///
/// Each dimension is a `TokenCount` (literal or symbolic).
/// The SDF token rate is the product of all dimensions: `|S| = Π di`.
/// Rank-1 shapes are equivalent to the legacy scalar `TokenCount`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortShape {
    /// One dimension per rank. Rank-1 for scalar/traditional ports.
    pub dims: Vec<TokenCount>,
}

impl PortShape {
    /// Create a rank-1 shape from a single `TokenCount` (backward compat).
    pub fn rank1(count: TokenCount) -> Self {
        PortShape { dims: vec![count] }
    }

    /// Number of dimensions (rank).
    pub fn rank(&self) -> usize {
        self.dims.len()
    }

    /// True if all dimensions are resolved literals.
    pub fn is_fully_literal(&self) -> bool {
        self.dims
            .iter()
            .all(|d| matches!(d, TokenCount::Literal(_)))
    }

    /// Collapse shape to a scalar `TokenCount` for backward compatibility.
    ///
    /// - Rank-1: returns the single dimension directly.
    /// - All-literal: returns `Literal(product)`.
    /// - Mixed/symbolic: returns the first symbolic dimension (analysis phase
    ///   handles full multi-dim resolution).
    pub fn to_scalar_count(&self) -> TokenCount {
        if self.dims.len() == 1 {
            return self.dims[0].clone();
        }
        if self.is_fully_literal() {
            let product: u32 = self
                .dims
                .iter()
                .map(|d| match d {
                    TokenCount::Literal(n) => *n,
                    _ => unreachable!(),
                })
                .product();
            TokenCount::Literal(product)
        } else {
            // Return first symbolic dimension; analysis will resolve the full shape.
            self.dims
                .iter()
                .find(|d| matches!(d, TokenCount::Symbolic(_)))
                .cloned()
                .unwrap_or(TokenCount::Literal(0))
        }
    }
}

/// Whether a parameter is compile-time or runtime-swappable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParamKind {
    Param,
    RuntimeParam,
}

/// C++ type of an actor parameter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParamType {
    Int,
    Float,
    Double,
    SpanFloat,
    SpanChar,
    /// Type parameter reference, e.g., `PARAM(T, gain)` in a polymorphic actor.
    TypeParam(String),
    /// Span of type parameter, e.g., `PARAM(std::span<const T>, coeff)`.
    SpanTypeParam(String),
}

/// A single actor parameter declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorParam {
    pub kind: ParamKind,
    pub param_type: ParamType,
    pub name: String,
}

/// Metadata extracted from one ACTOR() macro invocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorMeta {
    pub name: String,
    /// Type parameters for polymorphic actors (e.g., `["T"]`). Empty for concrete actors.
    pub type_params: Vec<String>,
    pub in_type: TypeExpr,
    pub in_count: TokenCount,
    pub in_shape: PortShape,
    pub out_type: TypeExpr,
    pub out_count: TokenCount,
    pub out_shape: PortShape,
    pub params: Vec<ActorParam>,
}

impl ActorMeta {
    /// True if this actor is polymorphic (has type parameters).
    pub fn is_polymorphic(&self) -> bool {
        !self.type_params.is_empty()
    }
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
    PreprocessorError {
        message: String,
        stderr: String,
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
            RegistryError::PreprocessorError { message, stderr } => {
                if stderr.is_empty() {
                    write!(f, "preprocessor: {}", message)
                } else {
                    write!(f, "preprocessor: {}\n{}", message, stderr)
                }
            }
        }
    }
}

impl std::error::Error for RegistryError {}

// ── Registry ────────────────────────────────────────────────────────────────

/// Actor metadata registry. Populated by scanning C++ header files.
#[derive(Clone)]
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

    /// Create an empty registry (test convenience alias for `new()`).
    pub fn empty() -> Self {
        Self::new()
    }

    /// Insert an actor directly (for tests). Uses a synthetic path.
    pub fn insert(&mut self, meta: ActorMeta) {
        let name = meta.name.clone();
        self.actors.insert(name, (meta, PathBuf::from("<test>")));
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

    /// Overlay entries from another registry.
    /// Existing names are replaced by entries from `other`.
    pub fn overlay_from(&mut self, other: &Registry) {
        for (name, (meta, path)) in &other.actors {
            self.actors
                .insert(name.clone(), (meta.clone(), path.clone()));
        }
    }
    /// Load actors from a JSON manifest file (`actors.meta.json` schema v1).
    pub fn load_manifest(&mut self, path: &Path) -> Result<usize, RegistryError> {
        let source = std::fs::read_to_string(path).map_err(|e| RegistryError::IoError {
            path: path.to_path_buf(),
            source: e,
        })?;

        let manifest: Manifest =
            serde_json::from_str(&source).map_err(|e| RegistryError::ParseError {
                file: path.to_path_buf(),
                line: 0,
                message: format!("invalid manifest JSON: {}", e),
            })?;

        if manifest.schema != 1 {
            return Err(RegistryError::ParseError {
                file: path.to_path_buf(),
                line: 0,
                message: format!(
                    "invalid actor metadata schema (expected: 1, found: {})",
                    manifest.schema
                ),
            });
        }

        let count = manifest.actors.len();
        for actor in manifest.actors {
            if self.actors.contains_key(&actor.name) {
                return Err(RegistryError::DuplicateActor {
                    name: actor.name.clone(),
                    first: path.to_path_buf(),
                    second: path.to_path_buf(),
                });
            }
            self.actors
                .insert(actor.name.clone(), (actor, path.to_path_buf()));
        }

        Ok(count)
    }

    /// Generate a pretty-printed JSON manifest string for display and `--emit manifest`.
    pub fn generate_manifest(&self) -> String {
        let manifest = self.build_manifest();
        serde_json::to_string_pretty(&manifest).expect("manifest serialization should not fail")
    }

    /// Generate compact canonical JSON for fingerprint computation.
    ///
    /// Uses `serde_json::to_string()` (no whitespace) to ensure the hash
    /// is independent of display formatting. Same sorting/collection logic
    /// as `generate_manifest()`.
    pub fn canonical_json(&self) -> String {
        let manifest = self.build_manifest();
        serde_json::to_string(&manifest).expect("manifest serialization should not fail")
    }

    fn build_manifest(&self) -> Manifest {
        let actors: Vec<&ActorMeta> = {
            let mut v: Vec<_> = self.actors.values().map(|(m, _)| m).collect();
            v.sort_by(|a, b| a.name.cmp(&b.name));
            v
        };
        Manifest {
            schema: 1,
            actors: actors.into_iter().cloned().collect(),
        }
    }
}

// ── Manifest (actors.meta.json) ──────────────────────────────────────────────

/// Top-level structure of `actors.meta.json` (schema v1).
#[derive(Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub schema: u32,
    pub actors: Vec<ActorMeta>,
}

// ── Scanner ─────────────────────────────────────────────────────────────────

/// Scan a source string for ACTOR() macro invocations.
/// Recognizes both concrete actors and `template <typename T> ACTOR(...)`.
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

            // Skip ACTOR inside preprocessor directives (#define ACTOR ...)
            let line_start = stripped[..abs_idx].rfind('\n').map_or(0, |i| i + 1);
            let line_prefix = stripped[line_start..abs_idx].trim_start();
            if line_prefix.starts_with('#') {
                pos = abs_idx + 6;
                continue;
            }

            // Check for preceding `template <typename T>` pattern
            let type_params = extract_template_params(&stripped[..abs_idx]);

            // Extract balanced parentheses content
            let paren_start = abs_idx + 5; // index of '('
            match extract_balanced(bytes, paren_start, b'(', b')') {
                Some(content_end) => {
                    let inner = &stripped[paren_start + 1..content_end];
                    let line = stripped[..abs_idx].chars().filter(|&c| c == '\n').count() + 1;

                    let actor = parse_actor_macro(inner, &type_params, file, line)?;
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

/// Extract template type parameters from text preceding an ACTOR() call.
/// Matches patterns like `template <typename T>` or `template <typename T, typename U>`.
/// Returns an empty vec if no template declaration is found.
fn extract_template_params(text_before_actor: &str) -> Vec<String> {
    let trimmed = text_before_actor.trim_end();

    // Look for '>' at the end of the preceding text
    if !trimmed.ends_with('>') {
        return Vec::new();
    }

    // Find matching '<'
    let mut depth = 0i32;
    let mut angle_start = None;
    for (i, ch) in trimmed.char_indices().rev() {
        match ch {
            '>' => depth += 1,
            '<' => {
                depth -= 1;
                if depth == 0 {
                    angle_start = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }

    let angle_start = match angle_start {
        Some(i) => i,
        None => return Vec::new(),
    };

    // Check that "template" keyword precedes the '<'
    let before_angle = trimmed[..angle_start].trim_end();
    if !before_angle.ends_with("template") {
        return Vec::new();
    }

    // Extract type parameter names from between < and >
    let inner = &trimmed[angle_start + 1..trimmed.len() - 1];
    inner
        .split(',')
        .filter_map(|part| {
            let part = part.trim();
            // Accept "typename T" or "class T"
            let name = part
                .strip_prefix("typename ")
                .or_else(|| part.strip_prefix("class "))
                .map(|s| s.trim().to_string());
            name.filter(|n| !n.is_empty())
        })
        .collect()
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

/// Split a string that may contain multiple space-separated PARAM()/RUNTIME_PARAM() specs.
/// Returns references into the original string for each individual spec.
/// Handles both single specs ("PARAM(int, N)") and multiple ("PARAM(int, N) PARAM(float, init)").
fn split_param_specs(s: &str) -> Vec<&str> {
    let mut specs = Vec::new();
    let bytes = s.as_bytes();
    let mut pos = 0;

    while pos < bytes.len() {
        // Skip whitespace
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= bytes.len() {
            break;
        }

        let start = pos;
        // Find the opening paren
        if let Some(rel_paren) = s[pos..].find('(') {
            let paren_pos = pos + rel_paren;
            if let Some(end) = extract_balanced(bytes, paren_pos, b'(', b')') {
                specs.push(s[start..=end].trim());
                pos = end + 1;
            } else {
                // Unbalanced — push remainder and let caller handle error
                specs.push(s[start..].trim());
                break;
            }
        } else {
            let remaining = s[start..].trim();
            if !remaining.is_empty() {
                specs.push(remaining);
            }
            break;
        }
    }

    specs
}

/// Parse the inner content of an ACTOR(...) invocation.
fn parse_actor_macro(
    inner: &str,
    type_params: &[String],
    file: &Path,
    line: usize,
) -> Result<ActorMeta, RegistryError> {
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

    let (in_type, in_count, in_shape) =
        parse_port_spec(fields[1].trim(), "IN", type_params, file, line)?;
    let (out_type, out_count, out_shape) =
        parse_port_spec(fields[2].trim(), "OUT", type_params, file, line)?;

    // Collect remaining fields (params). Fields may be comma-separated (old style)
    // or space-separated within a single field (new style). Handle both.
    let mut params = Vec::new();
    for field in &fields[3..] {
        let trimmed = field.trim();
        if trimmed.is_empty() {
            continue;
        }
        // A field may contain multiple space-separated PARAM/RUNTIME_PARAM specs
        let specs = split_param_specs(trimmed);
        for spec in specs {
            let param = parse_param_spec(spec, type_params, file, line)?;
            params.push(param);
        }
    }

    Ok(ActorMeta {
        name,
        type_params: type_params.to_vec(),
        in_type,
        in_count,
        in_shape,
        out_type,
        out_count,
        out_shape,
        params,
    })
}

/// Parse `IN(type, count_or_shape)` or `OUT(type, count_or_shape)`.
///
/// Supports both legacy scalar counts (`IN(float, N)`) and v0.2.0
/// `SHAPE(...)` notation (`IN(float, SHAPE(H, W, C))`).
/// For polymorphic actors, the type field may reference a type parameter (e.g., `IN(T, N)`).
fn parse_port_spec(
    s: &str,
    expected_prefix: &str,
    type_params: &[String],
    file: &Path,
    line: usize,
) -> Result<(TypeExpr, TokenCount, PortShape), RegistryError> {
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

    let type_str = parts[0].trim();
    let type_expr = if type_params.iter().any(|tp| tp == type_str) {
        TypeExpr::TypeParam(type_str.to_string())
    } else {
        TypeExpr::Concrete(parse_pipit_type(type_str, file, line)?)
    };
    let shape = parse_port_shape(parts[1].trim());
    let count = shape.to_scalar_count();

    Ok((type_expr, count, shape))
}

/// Parse a port count field into a `PortShape`.
///
/// Recognizes `SHAPE(d0, d1, ...)` for multi-dimensional shapes,
/// or falls back to a rank-1 shape from a scalar `TokenCount`.
fn parse_port_shape(count_str: &str) -> PortShape {
    let trimmed = count_str.trim();
    if let Some(inner) = trimmed
        .strip_prefix("SHAPE(")
        .and_then(|s| s.strip_suffix(')'))
    {
        let dims: Vec<TokenCount> = inner
            .split(',')
            .map(|d| parse_token_count(d.trim()))
            .collect();
        PortShape { dims }
    } else {
        PortShape::rank1(parse_token_count(trimmed))
    }
}

/// Parse `PARAM(type, name)` or `RUNTIME_PARAM(type, name)`.
/// For polymorphic actors, param types may reference type parameters.
fn parse_param_spec(
    s: &str,
    type_params: &[String],
    file: &Path,
    line: usize,
) -> Result<ActorParam, RegistryError> {
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

    let param_type = parse_param_type(parts[0].trim(), type_params, file, line)?;
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
/// For polymorphic actors, checks if the type string matches a type parameter name.
fn parse_param_type(
    s: &str,
    type_params: &[String],
    file: &Path,
    line: usize,
) -> Result<ParamType, RegistryError> {
    // Check for type parameter references first
    if type_params.iter().any(|tp| tp == s) {
        return Ok(ParamType::TypeParam(s.to_string()));
    }
    // Check for std::span<const T> where T is a type parameter
    if let Some(inner) = s
        .strip_prefix("std::span<const ")
        .and_then(|r| r.strip_suffix('>'))
    {
        let inner = inner.trim();
        if type_params.iter().any(|tp| tp == inner) {
            return Ok(ParamType::SpanTypeParam(inner.to_string()));
        }
    }

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

// ── PP Record Extraction ─────────────────────────────────────────────────────

/// Scan actor headers via C++ preprocessor record extraction.
///
/// Builds a probe translation unit that redefines ACTOR() to emit
/// PIPIT_REC_V1(...) records, pipes it through the preprocessor,
/// and parses the output.
///
/// `cc` is the compiler command (e.g. "clang++").
/// `include_headers` are the `-I`-sourced headers.
/// `actor_path_headers` are the `--actor-path`-sourced headers.
/// `extra_include_dirs` are additional `-I` directories for the preprocessor.
pub fn scan_actors_pp(
    cc: &str,
    include_headers: &[PathBuf],
    actor_path_headers: &[PathBuf],
    extra_include_dirs: &[PathBuf],
) -> Result<(Registry, Registry), RegistryError> {
    let all_headers: Vec<&Path> = actor_path_headers
        .iter()
        .chain(include_headers.iter())
        .map(|p| p.as_path())
        .collect();

    let probe_tu = build_probe_tu(&all_headers);
    let pp_output = invoke_preprocessor(cc, &probe_tu, extra_include_dirs)?;
    let records = parse_pp_records(&pp_output)?;

    // Split records into two registries by source path
    let mut include_registry = Registry::new();
    let mut actor_path_registry = Registry::new();

    for (meta, source_path) in records {
        let source = PathBuf::from(&source_path);

        // Determine which group this record belongs to
        let is_actor_path = actor_path_headers.iter().any(|ap| {
            // Match if the __FILE__ path ends with the same filename,
            // or if the source path is under an actor-path directory
            if let (Some(ap_name), Some(src_name)) = (ap.file_name(), source.file_name()) {
                if ap_name == src_name {
                    return true;
                }
            }
            if let Some(ap_parent) = ap.parent() {
                if let Ok(src_canonical) = std::fs::canonicalize(&source) {
                    if let Ok(ap_canonical) = std::fs::canonicalize(ap_parent) {
                        return src_canonical.starts_with(&ap_canonical);
                    }
                }
            }
            false
        });

        let registry = if is_actor_path {
            &mut actor_path_registry
        } else {
            &mut include_registry
        };

        // Enforce same-group duplicate check
        if let Some((_, first_path)) = registry.actors.get(&meta.name) {
            return Err(RegistryError::DuplicateActor {
                name: meta.name.clone(),
                first: first_path.clone(),
                second: source.clone(),
            });
        }
        registry.actors.insert(meta.name.clone(), (meta, source));
    }

    Ok((include_registry, actor_path_registry))
}

/// Build the probe translation unit source text.
fn build_probe_tu(headers: &[&Path]) -> String {
    let mut tu = String::with_capacity(2048);

    // 1. Include pipit.h to get type aliases and activate include guard
    tu.push_str("#include \"pipit.h\"\n\n");

    // 2. Undefine and redefine macros for record emission
    tu.push_str("#undef ACTOR\n");
    tu.push_str("#undef IN\n");
    tu.push_str("#undef OUT\n");
    tu.push_str("#undef PARAM\n");
    tu.push_str("#undef RUNTIME_PARAM\n");
    tu.push_str("#undef _PIPIT_FIRST\n\n");

    // Self-referential macros: prevent expansion, preserve text
    tu.push_str("#define IN(type, count) IN(type, count)\n");
    tu.push_str("#define OUT(type, count) OUT(type, count)\n");
    tu.push_str("#define PARAM(type, name) PARAM(type, name)\n");
    tu.push_str("#define RUNTIME_PARAM(type, name) RUNTIME_PARAM(type, name)\n");
    tu.push_str("#define _PIPIT_FIRST(a, ...) void\n\n");

    // Record emission macro
    tu.push_str("#define ACTOR(name, in_spec, out_spec, ...) \\\n");
    tu.push_str(
        "    PIPIT_REC_V1(__FILE__, __LINE__, #name, #in_spec, #out_spec, #__VA_ARGS__)\n\n",
    );

    // 3. Include all discovered actor headers
    for header in headers {
        // Use absolute paths to avoid include path ambiguity
        let abs = if header.is_absolute() {
            header.to_path_buf()
        } else if let Ok(canonical) = std::fs::canonicalize(header) {
            canonical
        } else {
            header.to_path_buf()
        };
        tu.push_str(&format!("#include \"{}\"\n", abs.display()));
    }

    tu
}

/// Invoke the C++ preprocessor and capture stdout.
fn invoke_preprocessor(
    cc: &str,
    source: &str,
    include_dirs: &[PathBuf],
) -> Result<String, RegistryError> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut cmd = Command::new(cc);
    cmd.arg("-E")
        .arg("-P")
        .arg("-x")
        .arg("c++")
        .arg("-std=c++20")
        .arg("-");

    // Auto-add runtime include paths
    let runtime_include = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("runtime")
        .join("libpipit")
        .join("include");
    if runtime_include.exists() {
        cmd.arg("-I").arg(&runtime_include);
        let third_party = runtime_include.join("third_party");
        if third_party.exists() {
            cmd.arg("-I").arg(&third_party);
        }
    }

    for dir in include_dirs {
        cmd.arg("-I").arg(dir);
    }

    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| RegistryError::PreprocessorError {
        message: format!("failed to launch '{}': {}", cc, e),
        stderr: String::new(),
    })?;

    // Write probe TU to stdin
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(source.as_bytes())
            .map_err(|e| RegistryError::PreprocessorError {
                message: format!("failed to write to preprocessor stdin: {}", e),
                stderr: String::new(),
            })?;
    }

    let output = child
        .wait_with_output()
        .map_err(|e| RegistryError::PreprocessorError {
            message: format!("failed to wait for preprocessor: {}", e),
            stderr: String::new(),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(RegistryError::PreprocessorError {
            message: format!(
                "'{}' preprocessing failed (exit code: {})",
                cc,
                output.status.code().unwrap_or(-1)
            ),
            stderr,
        });
    }

    String::from_utf8(output.stdout).map_err(|e| RegistryError::PreprocessorError {
        message: format!("preprocessor output is not valid UTF-8: {}", e),
        stderr: String::new(),
    })
}

/// Parse all PIPIT_REC_V1(...) records from preprocessor output.
/// Returns (ActorMeta, source_file_path) pairs.
fn parse_pp_records(output: &str) -> Result<Vec<(ActorMeta, String)>, RegistryError> {
    let mut results = Vec::new();
    let marker = "PIPIT_REC_V1(";

    let mut search_start = 0;
    while let Some(rel_idx) = output[search_start..].find(marker) {
        let abs_idx = search_start + rel_idx;

        // Check for template<...> before this record
        let text_before = &output[..abs_idx];
        let type_params = extract_template_params(text_before);

        // Find the matching closing paren (string-literal aware)
        let paren_start = abs_idx + marker.len() - 1; // index of '('
        let content_end = match find_balanced_paren_string_aware(output, paren_start) {
            Some(end) => end,
            None => {
                return Err(RegistryError::ParseError {
                    file: PathBuf::from("<pp-output>"),
                    line: output[..abs_idx].lines().count(),
                    message: "unbalanced parentheses in PIPIT_REC_V1()".to_string(),
                });
            }
        };

        let inner = &output[paren_start + 1..content_end];
        let (file, line, name, in_spec, out_spec, params) =
            parse_record_fields(inner, &output[..abs_idx])?;

        let file_path = PathBuf::from(&file);
        let (in_type, in_count, in_shape) =
            parse_port_spec(&in_spec, "IN", &type_params, &file_path, line)?;
        let (out_type, out_count, out_shape) =
            parse_port_spec(&out_spec, "OUT", &type_params, &file_path, line)?;

        let mut actor_params = Vec::new();
        if !params.is_empty() {
            let specs = split_param_specs(&params);
            for spec in specs {
                let param = parse_param_spec(spec, &type_params, &file_path, line)?;
                actor_params.push(param);
            }
        }

        results.push((
            ActorMeta {
                name,
                type_params,
                in_type,
                in_count,
                in_shape,
                out_type,
                out_count,
                out_shape,
                params: actor_params,
            },
            file,
        ));

        search_start = content_end + 1;
    }

    Ok(results)
}

/// Extract 6 fields from PIPIT_REC_V1 inner content.
/// Fields: (file: String, line: u32, name: String, in_spec: String, out_spec: String, params: String)
fn parse_record_fields(
    inner: &str,
    context_before: &str,
) -> Result<(String, usize, String, String, String, String), RegistryError> {
    let fields = split_record_fields(inner);
    if fields.len() != 6 {
        let line = context_before.lines().count();
        return Err(RegistryError::ParseError {
            file: PathBuf::from("<pp-output>"),
            line,
            message: format!("PIPIT_REC_V1 requires 6 fields, found {}", fields.len()),
        });
    }

    let file = unescape_string_literal(fields[0].trim());
    let line: usize = fields[1].trim().parse().unwrap_or(0);
    let name = unescape_string_literal(fields[2].trim());
    let in_spec = unescape_string_literal(fields[3].trim());
    let out_spec = unescape_string_literal(fields[4].trim());
    let params = unescape_string_literal(fields[5].trim());

    Ok((file, line, name, in_spec, out_spec, params))
}

/// Split record fields by commas, respecting string literals and nested parens.
fn split_record_fields(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let bytes = s.as_bytes();
    let mut start = 0;
    let mut depth_paren = 0i32;
    let mut in_string = false;
    let mut i = 0;

    while i < bytes.len() {
        if in_string {
            if bytes[i] == b'\\' && i + 1 < bytes.len() {
                i += 2; // skip escaped char
                continue;
            }
            if bytes[i] == b'"' {
                in_string = false;
            }
        } else {
            match bytes[i] {
                b'"' => in_string = true,
                b'(' => depth_paren += 1,
                b')' => depth_paren -= 1,
                b',' if depth_paren == 0 => {
                    parts.push(&s[start..i]);
                    start = i + 1;
                }
                _ => {}
            }
        }
        i += 1;
    }

    parts.push(&s[start..]);
    parts
}

/// Find matching closing paren, aware of string literals.
fn find_balanced_paren_string_aware(s: &str, start: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    if start >= bytes.len() || bytes[start] != b'(' {
        return None;
    }

    let mut depth = 0i32;
    let mut in_string = false;
    let mut i = start;

    while i < bytes.len() {
        if in_string {
            if bytes[i] == b'\\' && i + 1 < bytes.len() {
                i += 2;
                continue;
            }
            if bytes[i] == b'"' {
                in_string = false;
            }
        } else {
            match bytes[i] {
                b'"' => in_string = true,
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }

    None
}

/// Unescape a C string literal: remove surrounding quotes and handle escape sequences.
fn unescape_string_literal(s: &str) -> String {
    let s = s.trim();
    let s = s.strip_prefix('"').unwrap_or(s);
    let s = s.strip_suffix('"').unwrap_or(s);

    let mut result = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            match bytes[i + 1] {
                b'n' => result.push('\n'),
                b't' => result.push('\t'),
                b'\\' => result.push('\\'),
                b'"' => result.push('"'),
                b'\'' => result.push('\''),
                other => {
                    result.push('\\');
                    result.push(other as char);
                }
            }
            i += 2;
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }

    result
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

    // parse_span_param removed: merged into parse_multi_param (identical input)

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
            "ACTOR(fir, IN(float, N), OUT(float, 1), PARAM(std::span<const float>, coeff), PARAM(int, N)) { return ACTOR_OK; }",
        );
        assert_eq!(a.params.len(), 2);
        assert_eq!(a.params[0].kind, ParamKind::Param);
        assert_eq!(a.params[0].param_type, ParamType::SpanFloat);
        assert_eq!(a.params[0].name, "coeff");
        assert_eq!(a.params[1].kind, ParamKind::Param);
        assert_eq!(a.params[1].param_type, ParamType::Int);
        assert_eq!(a.params[1].name, "N");
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
    fn malformed_actor_missing_fields() {
        let path = PathBuf::from("test.h");
        let result = scan_actors("ACTOR(foo, IN(float, 1)) { return ACTOR_OK; }", &path);
        assert!(result.is_err());
        match result.unwrap_err() {
            RegistryError::ParseError { message, .. } => {
                assert!(
                    message.contains("requires at least 3 fields"),
                    "got: {}",
                    message
                );
            }
            other => panic!("expected ParseError, got: {}", other),
        }
    }

    #[test]
    fn malformed_actor_invalid_name() {
        let path = PathBuf::from("test.h");
        let result = scan_actors(
            "ACTOR(bad-name!, IN(float, 1), OUT(float, 1)) { return ACTOR_OK; }",
            &path,
        );
        assert!(result.is_err());
        match result.unwrap_err() {
            RegistryError::ParseError { message, .. } => {
                assert!(message.contains("invalid actor name"), "got: {}", message);
            }
            other => panic!("expected ParseError, got: {}", other),
        }
    }

    #[test]
    fn malformed_actor_unbalanced_parens() {
        let path = PathBuf::from("test.h");
        let result = scan_actors("ACTOR(foo, IN(float, 1), OUT(float, 1)", &path);
        assert!(result.is_err());
        match result.unwrap_err() {
            RegistryError::ParseError { message, .. } => {
                assert!(
                    message.contains("unbalanced parentheses"),
                    "got: {}",
                    message
                );
            }
            other => panic!("expected ParseError, got: {}", other),
        }
    }

    #[test]
    fn unknown_param_type_error() {
        let path = PathBuf::from("test.h");
        let result = scan_actors(
            "ACTOR(x, IN(float, 1), OUT(float, 1), PARAM(bool, flag)) { return ACTOR_OK; }",
            &path,
        );
        assert!(result.is_err());
        match result.unwrap_err() {
            RegistryError::ParseError { message, .. } => {
                assert!(
                    message.contains("unknown parameter type"),
                    "got: {}",
                    message
                );
            }
            other => panic!("expected ParseError, got: {}", other),
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
    fn skip_preprocessor_define() {
        // #define ACTOR(...) should not be parsed as an actor definition
        let src = r#"
#define ACTOR(name, in_spec, out_spec, ...) \
    struct Actor_##name { int operator()(); }
ACTOR(real, IN(float, 1), OUT(float, 1)) { return ACTOR_OK; }
"#;
        let path = PathBuf::from("test.h");
        let actors = scan_actors(src, &path).unwrap();
        assert_eq!(actors.len(), 1);
        assert_eq!(actors[0].name, "real");
    }

    #[test]
    fn comment_markers_inside_string_preserved() {
        // strip_comments should NOT strip // or /* */ inside string literals
        let src = r#"
const char* s1 = "// not a comment";
const char* s2 = "/* not either */";
ACTOR(real, IN(float, 1), OUT(float, 1)) { return ACTOR_OK; }
"#;
        let path = PathBuf::from("test.h");
        let actors = scan_actors(src, &path).unwrap();
        assert_eq!(
            actors.len(),
            1,
            "actor should survive string-embedded comment markers"
        );
        assert_eq!(actors[0].name, "real");
    }

    #[test]
    fn load_examples_actors_h() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("examples/example_actors.h");
        if !path.exists() {
            // Skip if examples not available (CI without full checkout)
            return;
        }

        let mut reg = Registry::new();
        let count = reg.load_header(&path).unwrap();
        assert_eq!(count, 4, "expected 4 actors in examples/example_actors.h");
        assert_eq!(reg.len(), 4);

        // Spot-check a few example actors
        let correlate = reg.lookup("correlate").expect("correlate not found");
        assert_eq!(correlate.in_type, PipitType::Float);
        assert_eq!(correlate.in_count, TokenCount::Literal(64));
        assert_eq!(correlate.out_type, PipitType::Float);

        let detect = reg.lookup("detect").expect("detect not found");
        assert_eq!(detect.out_type, PipitType::Int32);

        let csvwrite = reg.lookup("csvwrite").expect("csvwrite not found");
        assert_eq!(csvwrite.params[0].param_type, ParamType::SpanChar);

        let sync_process = reg.lookup("sync_process").expect("sync_process not found");
        assert_eq!(sync_process.in_type, PipitType::Float);
        assert_eq!(sync_process.in_count, TokenCount::Literal(256));
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

    // ── PortShape tests ─────────────────────────────────────────────────

    #[test]
    fn port_shape_rank1_from_literal() {
        let shape = PortShape::rank1(TokenCount::Literal(5));
        assert_eq!(shape.rank(), 1);
        assert!(shape.is_fully_literal());
        assert_eq!(shape.to_scalar_count(), TokenCount::Literal(5));
    }

    #[test]
    fn port_shape_rank1_from_symbolic() {
        let shape = PortShape::rank1(TokenCount::Symbolic("N".into()));
        assert_eq!(shape.rank(), 1);
        assert!(!shape.is_fully_literal());
        assert_eq!(shape.to_scalar_count(), TokenCount::Symbolic("N".into()));
    }

    #[test]
    fn port_shape_multi_dim_literal_product() {
        let shape = PortShape {
            dims: vec![
                TokenCount::Literal(1080),
                TokenCount::Literal(1920),
                TokenCount::Literal(3),
            ],
        };
        assert_eq!(shape.rank(), 3);
        assert!(shape.is_fully_literal());
        assert_eq!(
            shape.to_scalar_count(),
            TokenCount::Literal(1080 * 1920 * 3)
        );
    }

    #[test]
    fn port_shape_multi_dim_symbolic_fallback() {
        let shape = PortShape {
            dims: vec![
                TokenCount::Symbolic("H".into()),
                TokenCount::Symbolic("W".into()),
                TokenCount::Literal(3),
            ],
        };
        assert_eq!(shape.rank(), 3);
        assert!(!shape.is_fully_literal());
        assert_eq!(shape.to_scalar_count(), TokenCount::Symbolic("H".into()));
    }

    // ── SHAPE(...) parsing tests ────────────────────────────────────────

    #[test]
    fn parse_shape_rank1() {
        let a = scan_one(
            "ACTOR(gain, IN(float, SHAPE(N)), OUT(float, SHAPE(N)), PARAM(int, N)) { return ACTOR_OK; }",
        );
        assert_eq!(a.in_shape.rank(), 1);
        assert_eq!(a.in_shape.dims[0], TokenCount::Symbolic("N".into()));
        assert_eq!(a.out_shape.rank(), 1);
        assert_eq!(a.out_shape.dims[0], TokenCount::Symbolic("N".into()));
        // Backward compat: in_count should match
        assert_eq!(a.in_count, TokenCount::Symbolic("N".into()));
    }

    #[test]
    fn parse_shape_rank3() {
        let a = scan_one(
            "ACTOR(img, IN(float, SHAPE(H, W, C)), OUT(float, SHAPE(H, W, C)), PARAM(int, H) PARAM(int, W) PARAM(int, C)) { return ACTOR_OK; }",
        );
        assert_eq!(a.in_shape.rank(), 3);
        assert_eq!(a.in_shape.dims[0], TokenCount::Symbolic("H".into()));
        assert_eq!(a.in_shape.dims[1], TokenCount::Symbolic("W".into()));
        assert_eq!(a.in_shape.dims[2], TokenCount::Symbolic("C".into()));
        assert_eq!(a.out_shape.rank(), 3);
    }

    #[test]
    fn parse_shape_literal_multi() {
        let a = scan_one(
            "ACTOR(block, IN(float, SHAPE(1080, 1920, 3)), OUT(float, 1)) { return ACTOR_OK; }",
        );
        assert_eq!(a.in_shape.rank(), 3);
        assert_eq!(a.in_shape.dims[0], TokenCount::Literal(1080));
        assert_eq!(a.in_shape.dims[1], TokenCount::Literal(1920));
        assert_eq!(a.in_shape.dims[2], TokenCount::Literal(3));
        assert_eq!(a.in_count, TokenCount::Literal(1080 * 1920 * 3));
        // OUT is plain scalar — should be rank-1
        assert_eq!(a.out_shape.rank(), 1);
        assert_eq!(a.out_count, TokenCount::Literal(1));
    }

    #[test]
    fn parse_shape_backward_compat() {
        // Existing IN(float, N) should produce rank-1 shape
        let a = scan_one(
            "ACTOR(fft, IN(float, N), OUT(cfloat, N), PARAM(int, N)) { return ACTOR_OK; }",
        );
        assert_eq!(a.in_shape.rank(), 1);
        assert_eq!(a.in_shape.dims[0], TokenCount::Symbolic("N".into()));
        assert_eq!(a.in_count, TokenCount::Symbolic("N".into()));
    }

    #[test]
    fn existing_actors_have_rank1_shapes() {
        let a = scan_one("ACTOR(mag, IN(cfloat, 1), OUT(float, 1)) { return ACTOR_OK; }");
        assert_eq!(a.in_shape.rank(), 1);
        assert_eq!(a.in_shape.dims[0], TokenCount::Literal(1));
        assert_eq!(a.out_shape.rank(), 1);
        assert_eq!(a.out_shape.dims[0], TokenCount::Literal(1));
    }

    // ── Shape edge cases (documenting current permissive behavior) ──────

    #[test]
    fn shape_empty_becomes_symbolic() {
        // SHAPE() → inner "" parsed as Symbolic("") — no validation in registry
        let a = scan_one("ACTOR(x, IN(float, SHAPE()), OUT(float, 1)) { return ACTOR_OK; }");
        assert_eq!(a.in_shape.rank(), 1);
        assert_eq!(
            a.in_shape.dims[0],
            TokenCount::Symbolic("".into()),
            "empty SHAPE() should produce Symbolic(\"\")"
        );
    }

    #[test]
    fn shape_zero_literal() {
        // SHAPE(0) is valid at registry level — semantics checked in analysis
        let a = scan_one("ACTOR(x, IN(float, SHAPE(0)), OUT(float, 1)) { return ACTOR_OK; }");
        assert_eq!(a.in_shape.rank(), 1);
        assert_eq!(a.in_shape.dims[0], TokenCount::Literal(0));
        assert_eq!(a.in_count, TokenCount::Literal(0));
    }

    #[test]
    fn shape_negative_becomes_symbolic() {
        // -1 fails u32::parse → treated as Symbolic("-1")
        let a = scan_one("ACTOR(x, IN(float, SHAPE(-1)), OUT(float, 1)) { return ACTOR_OK; }");
        assert_eq!(a.in_shape.rank(), 1);
        assert_eq!(
            a.in_shape.dims[0],
            TokenCount::Symbolic("-1".into()),
            "negative value should become Symbolic"
        );
    }

    // ── Registry API contract tests ─────────────────────────────────────

    #[test]
    fn registry_is_empty_and_len() {
        let reg = Registry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn registry_actors_iterator() {
        let src = r#"
ACTOR(a, IN(float, 1), OUT(float, 1)) { return ACTOR_OK; }
ACTOR(b, IN(float, 1), OUT(float, 1)) { return ACTOR_OK; }
"#;
        let dir = std::env::temp_dir().join("pipit_test_iter");
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("actors.h");
        std::fs::write(&f, src).unwrap();

        let mut reg = Registry::new();
        reg.load_header(&f).unwrap();

        assert!(!reg.is_empty());
        assert_eq!(reg.len(), 2);

        let names: Vec<&str> = reg.actors().map(|a| a.name.as_str()).collect();
        assert!(names.contains(&"a"));
        assert!(names.contains(&"b"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn registry_overlay_from() {
        let dir = std::env::temp_dir().join("pipit_test_overlay");
        std::fs::create_dir_all(&dir).unwrap();

        // Base registry: actor "a" with float input
        let f1 = dir.join("base.h");
        std::fs::write(
            &f1,
            "ACTOR(a, IN(float, 1), OUT(float, 1)) { return ACTOR_OK; }",
        )
        .unwrap();
        let mut base = Registry::new();
        base.load_header(&f1).unwrap();
        assert_eq!(base.lookup("a").unwrap().in_type, PipitType::Float);

        // Overlay registry: actor "a" with int32 input + new actor "b"
        let f2 = dir.join("overlay.h");
        std::fs::write(
            &f2,
            concat!(
                "ACTOR(a, IN(int32, 1), OUT(int32, 1)) { return ACTOR_OK; }\n",
                "ACTOR(b, IN(float, 1), OUT(float, 1)) { return ACTOR_OK; }\n",
            ),
        )
        .unwrap();
        let mut overlay = Registry::new();
        overlay.load_header(&f2).unwrap();

        base.overlay_from(&overlay);

        // "a" should be overwritten by overlay's version
        assert_eq!(base.lookup("a").unwrap().in_type, PipitType::Int32);
        // "b" should be added
        assert!(base.lookup("b").is_some());
        assert_eq!(base.len(), 2);

        std::fs::remove_dir_all(&dir).ok();
    }

    // ── Template (polymorphic) actor scanning ────────────────────────────

    #[test]
    fn scan_template_actor() {
        let src = r#"
template <typename T>
ACTOR(scale, IN(T, N), OUT(T, N), PARAM(T, gain) PARAM(int, N)) {
    for (int i = 0; i < N; ++i) out[i] = in[i] * gain;
    return ACTOR_OK;
}
"#;
        let a = scan_one(src);
        assert_eq!(a.name, "scale");
        assert_eq!(a.type_params, vec!["T"]);
        assert!(a.is_polymorphic());
        assert_eq!(a.in_type, TypeExpr::TypeParam("T".into()));
        assert_eq!(a.out_type, TypeExpr::TypeParam("T".into()));
        assert_eq!(a.in_count, TokenCount::Symbolic("N".into()));
        assert_eq!(a.out_count, TokenCount::Symbolic("N".into()));
        assert_eq!(a.params.len(), 2);
        assert_eq!(a.params[0].param_type, ParamType::TypeParam("T".into()));
        assert_eq!(a.params[0].name, "gain");
        assert_eq!(a.params[1].param_type, ParamType::Int);
        assert_eq!(a.params[1].name, "N");
    }

    #[test]
    fn scan_template_actor_with_span_type_param() {
        let src = r#"
template <typename T>
ACTOR(fir_generic, IN(T, N), OUT(T, 1), PARAM(std::span<const T>, coeff) PARAM(int, N)) {
    return ACTOR_OK;
}
"#;
        let a = scan_one(src);
        assert_eq!(a.name, "fir_generic");
        assert_eq!(a.type_params, vec!["T"]);
        assert_eq!(a.in_type, TypeExpr::TypeParam("T".into()));
        assert_eq!(a.out_type, TypeExpr::TypeParam("T".into()));
        assert_eq!(a.params[0].param_type, ParamType::SpanTypeParam("T".into()));
    }

    #[test]
    fn scan_concrete_actor_has_empty_type_params() {
        let a = scan_one("ACTOR(mag, IN(cfloat, 1), OUT(float, 1)) { return ACTOR_OK; }");
        assert!(a.type_params.is_empty());
        assert!(!a.is_polymorphic());
        assert_eq!(a.in_type, TypeExpr::Concrete(PipitType::Cfloat));
        assert_eq!(a.out_type, TypeExpr::Concrete(PipitType::Float));
    }

    #[test]
    fn scan_mixed_concrete_and_template() {
        let src = r#"
ACTOR(concrete_a, IN(float, 1), OUT(float, 1)) { return ACTOR_OK; }

template <typename T>
ACTOR(poly_b, IN(T, 1), OUT(T, 1)) { return ACTOR_OK; }

ACTOR(concrete_c, IN(int32, 1), OUT(int32, 1)) { return ACTOR_OK; }
"#;
        let path = PathBuf::from("test.h");
        let actors = scan_actors(src, &path).unwrap();
        assert_eq!(actors.len(), 3);

        assert_eq!(actors[0].name, "concrete_a");
        assert!(!actors[0].is_polymorphic());

        assert_eq!(actors[1].name, "poly_b");
        assert!(actors[1].is_polymorphic());
        assert_eq!(actors[1].type_params, vec!["T"]);

        assert_eq!(actors[2].name, "concrete_c");
        assert!(!actors[2].is_polymorphic());
    }

    #[test]
    fn extract_template_params_single() {
        let params = extract_template_params("template <typename T>\n");
        assert_eq!(params, vec!["T"]);
    }

    #[test]
    fn extract_template_params_multiple() {
        let params = extract_template_params("template <typename T, typename U>\n");
        assert_eq!(params, vec!["T", "U"]);
    }

    #[test]
    fn extract_template_params_class_keyword() {
        let params = extract_template_params("template <class T>\n");
        assert_eq!(params, vec!["T"]);
    }

    #[test]
    fn extract_template_params_none() {
        let params = extract_template_params("some_other_code\n");
        assert!(params.is_empty());
    }

    // ── Manifest (JSON) tests ────────────────────────────────────────────

    #[test]
    fn manifest_roundtrip() {
        let src = r#"
ACTOR(mul, IN(float, 1), OUT(float, 1), RUNTIME_PARAM(float, gain)) { return ACTOR_OK; }
ACTOR(fft, IN(float, N), OUT(cfloat, N), PARAM(int, N)) { return ACTOR_OK; }
"#;
        let dir = std::env::temp_dir().join("pipit_test_manifest_rt");
        std::fs::create_dir_all(&dir).unwrap();
        let h = dir.join("actors.h");
        std::fs::write(&h, src).unwrap();

        // Load from header
        let mut reg1 = Registry::new();
        reg1.load_header(&h).unwrap();

        // Generate manifest
        let json = reg1.generate_manifest();

        // Write and reload
        let manifest_path = dir.join("actors.meta.json");
        std::fs::write(&manifest_path, &json).unwrap();

        let mut reg2 = Registry::new();
        reg2.load_manifest(&manifest_path).unwrap();

        // Compare
        assert_eq!(reg2.len(), reg1.len());
        for a1 in reg1.actors() {
            let a2 = reg2
                .lookup(&a1.name)
                .expect("actor missing after roundtrip");
            assert_eq!(a1, a2, "mismatch for actor '{}'", a1.name);
        }

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn manifest_invalid_schema_version() {
        let dir = std::env::temp_dir().join("pipit_test_manifest_schema");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad_schema.json");
        std::fs::write(&path, r#"{"schema": 99, "actors": []}"#).unwrap();

        let mut reg = Registry::new();
        let err = reg.load_manifest(&path).unwrap_err();
        match err {
            RegistryError::ParseError { message, .. } => {
                assert!(
                    message.contains("expected: 1, found: 99"),
                    "got: {}",
                    message
                );
            }
            other => panic!("expected ParseError, got: {}", other),
        }

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn manifest_invalid_json() {
        let dir = std::env::temp_dir().join("pipit_test_manifest_badjson");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.json");
        std::fs::write(&path, "not json at all").unwrap();

        let mut reg = Registry::new();
        let err = reg.load_manifest(&path).unwrap_err();
        match err {
            RegistryError::ParseError { message, .. } => {
                assert!(
                    message.contains("invalid manifest JSON"),
                    "got: {}",
                    message
                );
            }
            other => panic!("expected ParseError, got: {}", other),
        }

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn manifest_template_roundtrip() {
        let src = r#"
template <typename T>
ACTOR(scale, IN(T, N), OUT(T, N), PARAM(T, gain) PARAM(int, N)) {
    return ACTOR_OK;
}
"#;
        let dir = std::env::temp_dir().join("pipit_test_manifest_poly");
        std::fs::create_dir_all(&dir).unwrap();
        let h = dir.join("poly.h");
        std::fs::write(&h, src).unwrap();

        let mut reg1 = Registry::new();
        reg1.load_header(&h).unwrap();

        let json = reg1.generate_manifest();
        let manifest_path = dir.join("actors.meta.json");
        std::fs::write(&manifest_path, &json).unwrap();

        let mut reg2 = Registry::new();
        reg2.load_manifest(&manifest_path).unwrap();

        let a2 = reg2.lookup("scale").unwrap();
        assert!(a2.is_polymorphic());
        assert_eq!(a2.type_params, vec!["T"]);
        assert_eq!(a2.in_type, TypeExpr::TypeParam("T".into()));
        assert_eq!(a2.params[0].param_type, ParamType::TypeParam("T".into()));

        std::fs::remove_dir_all(&dir).ok();
    }

    // ── Overlay tests ────────────────────────────────────────────────────

    fn simple_meta(name: &str, in_type: PipitType, out_type: PipitType) -> ActorMeta {
        ActorMeta {
            name: name.into(),
            type_params: vec![],
            in_type: TypeExpr::Concrete(in_type),
            in_count: TokenCount::Literal(1),
            in_shape: PortShape::rank1(TokenCount::Literal(1)),
            out_type: TypeExpr::Concrete(out_type),
            out_count: TokenCount::Literal(1),
            out_shape: PortShape::rank1(TokenCount::Literal(1)),
            params: vec![],
        }
    }

    #[test]
    fn overlay_from_replaces_existing() {
        let mut base = Registry::new();
        base.actors.insert(
            "Gain".into(),
            (
                simple_meta("Gain", PipitType::Float, PipitType::Float),
                PathBuf::from("old.h"),
            ),
        );

        let mut overlay = Registry::new();
        overlay.actors.insert(
            "Gain".into(),
            (
                simple_meta("Gain", PipitType::Cfloat, PipitType::Cfloat),
                PathBuf::from("new.h"),
            ),
        );

        base.overlay_from(&overlay);
        let meta = base.lookup("Gain").unwrap();
        assert_eq!(meta.in_type, PipitType::Cfloat, "overlay should replace");
    }

    #[test]
    fn overlay_from_preserves_non_conflicting() {
        let mut base = Registry::new();
        base.actors.insert(
            "A".into(),
            (
                simple_meta("A", PipitType::Float, PipitType::Float),
                PathBuf::from("a.h"),
            ),
        );

        let mut overlay = Registry::new();
        overlay.actors.insert(
            "B".into(),
            (
                simple_meta("B", PipitType::Float, PipitType::Float),
                PathBuf::from("b.h"),
            ),
        );

        base.overlay_from(&overlay);
        assert!(base.lookup("A").is_some(), "base actor A preserved");
        assert!(base.lookup("B").is_some(), "overlay actor B added");
        assert_eq!(base.len(), 2);
    }

    // ── Canonical JSON tests ─────────────────────────────────────────────

    #[test]
    fn canonical_json_is_compact() {
        let mut reg = Registry::new();
        reg.actors.insert(
            "Gain".into(),
            (
                simple_meta("Gain", PipitType::Float, PipitType::Float),
                PathBuf::from("gain.h"),
            ),
        );

        let json = reg.canonical_json();
        assert!(
            !json.contains('\n'),
            "canonical JSON should be compact (no newlines)"
        );
        assert!(
            !json.contains("  "),
            "canonical JSON should have no indentation"
        );
    }

    #[test]
    fn canonical_json_stable() {
        let mut reg = Registry::new();
        reg.actors.insert(
            "Gain".into(),
            (
                simple_meta("Gain", PipitType::Float, PipitType::Float),
                PathBuf::from("gain.h"),
            ),
        );

        let first = reg.canonical_json();
        let second = reg.canonical_json();
        assert_eq!(
            first, second,
            "canonical JSON should be stable across calls"
        );
    }

    // ── PP extraction unit tests ────────────────────────────────────────

    #[test]
    fn parse_pp_records_simple() {
        let output = r#"PIPIT_REC_V1("std_actors.h", 42, "gain", "IN(float, 1)", "OUT(float, 1)", "RUNTIME_PARAM(float, gain)")"#;
        let records = parse_pp_records(output).expect("parse should succeed");
        assert_eq!(records.len(), 1);
        let (meta, source) = &records[0];
        assert_eq!(meta.name, "gain");
        assert_eq!(meta.in_type, PipitType::Float);
        assert_eq!(meta.in_count, TokenCount::Literal(1));
        assert_eq!(meta.out_type, PipitType::Float);
        assert_eq!(meta.out_count, TokenCount::Literal(1));
        assert_eq!(meta.params.len(), 1);
        assert_eq!(meta.params[0].kind, ParamKind::RuntimeParam);
        assert_eq!(source, "std_actors.h");
    }

    #[test]
    fn parse_pp_records_no_params() {
        let output =
            r#"PIPIT_REC_V1("test.h", 10, "pass_through", "IN(float, 1)", "OUT(float, 1)", "")"#;
        let records = parse_pp_records(output).expect("parse should succeed");
        assert_eq!(records.len(), 1);
        assert!(records[0].0.params.is_empty());
    }

    #[test]
    fn parse_pp_records_multiple() {
        let output = concat!(
            "PIPIT_REC_V1(\"a.h\", 1, \"alpha\", \"IN(float, 1)\", \"OUT(float, 1)\", \"\")\n",
            "PIPIT_REC_V1(\"b.h\", 2, \"beta\", \"IN(int32, 2)\", \"OUT(int32, 2)\", \"PARAM(int, ch)\")\n",
        );
        let records = parse_pp_records(output).expect("parse should succeed");
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].0.name, "alpha");
        assert_eq!(records[1].0.name, "beta");
    }

    #[test]
    fn parse_pp_records_template() {
        let output = "template <typename T>\nPIPIT_REC_V1(\"poly.h\", 5, \"poly_scale\", \"IN(T, 1)\", \"OUT(T, 1)\", \"RUNTIME_PARAM(T, scale)\")";
        let records = parse_pp_records(output).expect("parse should succeed");
        assert_eq!(records.len(), 1);
        let meta = &records[0].0;
        assert_eq!(meta.name, "poly_scale");
        assert!(
            !meta.type_params.is_empty(),
            "template actor should have type params"
        );
        assert!(
            meta.type_params.iter().any(|p| p == "T"),
            "type_params should contain T, got: {:?}",
            meta.type_params
        );
    }

    #[test]
    fn parse_pp_records_empty_output() {
        let records = parse_pp_records("").expect("empty should succeed");
        assert!(records.is_empty());
    }

    #[test]
    fn parse_pp_records_no_records_in_noise() {
        let records =
            parse_pp_records("some other text\nno records here\n").expect("should succeed");
        assert!(records.is_empty());
    }

    #[test]
    fn split_record_fields_simple() {
        let fields =
            split_record_fields(r#""a.h", 42, "gain", "IN(float, 1)", "OUT(float, 1)", """#);
        assert_eq!(fields.len(), 6);
        assert_eq!(fields[0].trim(), r#""a.h""#);
        assert_eq!(fields[1].trim(), "42");
        assert_eq!(fields[2].trim(), r#""gain""#);
    }

    #[test]
    fn split_record_fields_commas_in_strings() {
        let fields = split_record_fields(
            r#""file.h", 1, "name", "IN(float, 1)", "OUT(float, 1)", "PARAM(int, a) PARAM(float, b)""#,
        );
        assert_eq!(fields.len(), 6);
    }

    #[test]
    fn unescape_simple() {
        assert_eq!(unescape_string_literal(r#""hello""#), "hello");
    }

    #[test]
    fn unescape_with_escapes() {
        assert_eq!(unescape_string_literal(r#""a\"b""#), "a\"b");
        assert_eq!(unescape_string_literal(r#""a\\b""#), "a\\b");
    }

    #[test]
    fn unescape_no_quotes() {
        assert_eq!(unescape_string_literal("plain"), "plain");
    }

    #[test]
    fn invoke_preprocessor_missing_compiler() {
        let result =
            invoke_preprocessor("nonexistent_compiler_xyz_12345", "#include <stdio.h>", &[]);
        assert!(result.is_err());
        match result.unwrap_err() {
            RegistryError::PreprocessorError { message, .. } => {
                assert!(
                    message.contains("nonexistent_compiler_xyz_12345"),
                    "error should mention the compiler: {message}"
                );
            }
            other => panic!("expected PreprocessorError, got: {other}"),
        }
    }

    #[test]
    fn invoke_preprocessor_bad_include() {
        let result = invoke_preprocessor(
            "clang++",
            "#include \"nonexistent_header_xyz_12345.h\"",
            &[],
        );
        assert!(result.is_err());
        match result.unwrap_err() {
            RegistryError::PreprocessorError { stderr, .. } => {
                assert!(
                    !stderr.is_empty(),
                    "preprocessor stderr should not be empty"
                );
            }
            other => panic!("expected PreprocessorError, got: {other}"),
        }
    }

    #[test]
    fn find_balanced_paren_basic() {
        assert_eq!(find_balanced_paren_string_aware("(abc)", 0), Some(4));
        assert_eq!(find_balanced_paren_string_aware("(a(b)c)", 0), Some(6));
    }

    #[test]
    fn find_balanced_paren_with_string() {
        assert_eq!(find_balanced_paren_string_aware(r#"("a)")"#, 0), Some(5));
    }

    #[test]
    fn build_probe_tu_includes_headers() {
        let h1 = PathBuf::from("/test/a.h");
        let h2 = PathBuf::from("/test/b.h");
        let tu = build_probe_tu(&[&h1, &h2]);
        assert!(tu.contains("#include \"/test/a.h\""));
        assert!(tu.contains("#include \"/test/b.h\""));
        assert!(tu.contains("#undef ACTOR"));
        assert!(tu.contains("PIPIT_REC_V1"));
    }
}
