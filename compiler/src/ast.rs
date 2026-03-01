// AST node types for Pipit .pdl source files.
//
// Mirrors the BNF grammar in the Pipit Language Specification §10.
// Every node carries a `SimpleSpan` for error reporting in downstream phases.
//
// Preconditions: produced by the parser from a valid or partially-valid token stream.
// Postconditions: each node's span covers the source range of the construct.
// Failure modes: none (data-only module).
// Side effects: none.

use chumsky::span::SimpleSpan;

/// Byte-offset span (alias for chumsky's `SimpleSpan`).
pub type Span = SimpleSpan;

// ── Root ──

/// A complete Pipit program: a sequence of top-level statements.
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub statements: Vec<Statement>,
    pub span: Span,
}

// ── Statements ──

/// A top-level statement with its source span.
#[derive(Debug, Clone, PartialEq)]
pub struct Statement {
    pub kind: StatementKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StatementKind {
    Set(SetStmt),
    Const(ConstStmt),
    Param(ParamStmt),
    Define(DefineStmt),
    Task(Box<TaskStmt>),
    Bind(BindStmt),
    Shared(SharedDecl),
}

// ── set_stmt: 'set' IDENT '=' set_value ──

#[derive(Debug, Clone, PartialEq)]
pub struct SetStmt {
    pub name: Ident,
    pub value: SetValue,
}

/// RHS of a `set` statement. Restricted to simple values (no arrays).
#[derive(Debug, Clone, PartialEq)]
pub enum SetValue {
    Number(f64, Span),
    Size(u64, Span),
    Freq(f64, Span),
    StringLit(String, Span),
    Ident(Ident),
}

// ── const_stmt: 'const' IDENT '=' value ──

#[derive(Debug, Clone, PartialEq)]
pub struct ConstStmt {
    pub name: Ident,
    pub value: Value,
}

// ── param_stmt: 'param' IDENT '=' scalar ──

#[derive(Debug, Clone, PartialEq)]
pub struct ParamStmt {
    pub name: Ident,
    pub value: Scalar,
}

// ── define_stmt: 'define' IDENT '(' params? ')' '{' pipeline_body '}' ──

#[derive(Debug, Clone, PartialEq)]
pub struct DefineStmt {
    pub name: Ident,
    pub params: Vec<Ident>,
    pub body: PipelineBody,
}

// ── bind_stmt: 'bind' IDENT '=' bind_endpoint ──

#[derive(Debug, Clone, PartialEq)]
pub struct BindStmt {
    pub name: Ident,
    pub endpoint: BindEndpoint,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BindEndpoint {
    pub transport: Ident,
    pub args: Vec<BindArg>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BindArg {
    /// Positional argument: a scalar value.
    Positional(Scalar),
    /// Named argument: `IDENT '=' scalar`.
    Named(Ident, Scalar),
}

// ── Bind direction (inferred in analyze phase) ──

/// Direction of a bind declaration: whether the pipeline writes (Out) or reads (In).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindDirection {
    /// External writes, pipeline reads (`@name` source).
    In,
    /// Pipeline writes, external reads (`-> name` sink).
    Out,
}

impl std::fmt::Display for BindDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BindDirection::In => write!(f, "in"),
            BindDirection::Out => write!(f, "out"),
        }
    }
}

// ── task_stmt: 'clock' FREQ IDENT '{' task_body '}' ──

#[derive(Debug, Clone, PartialEq)]
pub struct TaskStmt {
    pub freq: f64,
    pub freq_span: Span,
    pub name: Ident,
    /// Optional spawn clause: `clock freq name[idx=begin..end] { ... }` (v0.4.8).
    pub spawn: Option<SpawnClause>,
    pub body: TaskBody,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TaskBody {
    Pipeline(PipelineBody),
    Modal(ModalBody),
}

// ── modal_body: control_block mode_block+ switch_stmt ──

#[derive(Debug, Clone, PartialEq)]
pub struct ModalBody {
    pub control: ControlBlock,
    pub modes: Vec<ModeBlock>,
    pub switch: SwitchStmt,
    pub span: Span,
}

/// `control { pipeline_body }`
#[derive(Debug, Clone, PartialEq)]
pub struct ControlBlock {
    pub body: PipelineBody,
    pub span: Span,
}

/// `mode IDENT { pipeline_body }`
#[derive(Debug, Clone, PartialEq)]
pub struct ModeBlock {
    pub name: Ident,
    pub body: PipelineBody,
    pub span: Span,
}

/// `switch(source, mode1, mode2, ...) default?`
#[derive(Debug, Clone, PartialEq)]
pub struct SwitchStmt {
    pub source: SwitchSource,
    pub modes: Vec<Ident>,
    pub default: Option<Ident>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SwitchSource {
    Buffer(Ident),
    Param(Ident),
}

// ── pipeline_body: (pipe_expr NL)* ──

#[derive(Debug, Clone, PartialEq)]
pub struct PipelineBody {
    pub lines: Vec<PipeExpr>,
    pub span: Span,
}

// ── pipe_expr: pipe_source ('|' pipe_elem)* sink? ──

#[derive(Debug, Clone, PartialEq)]
pub struct PipeExpr {
    pub source: PipeSource,
    pub elements: Vec<PipeElem>,
    pub sink: Option<Sink>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PipeSource {
    /// `@name` or `@name[idx]` or `@name[*]` — shared buffer read
    BufferRead(BufferRef),
    /// `:name` — tap reference (consume side)
    TapRef(Ident),
    /// `name(args)` — actor call
    ActorCall(ActorCall),
}

#[derive(Debug, Clone, PartialEq)]
pub enum PipeElem {
    /// `name(args)` — actor call
    ActorCall(ActorCall),
    /// `:name` — tap declaration (fork)
    Tap(Ident),
    /// `?name` — probe
    Probe(Ident),
}

/// `-> name` or `-> name[idx]` or `-> name[*]` — shared buffer write (sink)
#[derive(Debug, Clone, PartialEq)]
pub struct Sink {
    pub buffer: BufferRef,
    pub span: Span,
}

// ── actor_call: IDENT ('<' type_arg (',' type_arg)* '>')? '(' args? ')' shape_constraint? ──

#[derive(Debug, Clone, PartialEq)]
pub struct ActorCall {
    pub name: Ident,
    /// Optional explicit type arguments: `actor<float, double>(...)` (v0.3.0).
    /// Empty for non-polymorphic calls or inferred calls.
    pub type_args: Vec<Ident>,
    pub args: Vec<Arg>,
    /// Optional shape constraint: `actor(...)[d0, d1, ...]` (v0.2.0).
    pub shape_constraint: Option<ShapeConstraint>,
    pub span: Span,
}

/// A compile-time shape constraint on an actor call: `actor(...)[d0, d1, ...]`.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapeConstraint {
    pub dims: Vec<ShapeDim>,
    pub span: Span,
}

/// A single dimension in a shape constraint.
#[derive(Debug, Clone, PartialEq)]
pub enum ShapeDim {
    /// Integer literal dimension.
    Literal(u32, Span),
    /// Const reference dimension (resolved later).
    ConstRef(Ident),
}

// ── arg ──

#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    /// A literal or array value.
    Value(Value),
    /// `$name` — runtime parameter reference.
    ParamRef(Ident),
    /// Bare identifier — const reference (resolved later).
    ConstRef(Ident),
    /// `:name` — tap reference as additional actor input (for feedback loops).
    TapRef(Ident),
}

// ── value, scalar, array ──

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Scalar(Scalar),
    Array(Vec<Scalar>, Span),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Scalar {
    /// Number literal with lexical kind preservation.
    /// `is_int_literal` is true only when source token had no decimal/exponent.
    Number(f64, Span, bool),
    Freq(f64, Span),
    Size(u64, Span),
    StringLit(String, Span),
    /// Bare identifier in value position — const reference (resolved later).
    Ident(Ident),
}

// ── Buffer reference (v0.4.8) ──

/// Index into a shared buffer array.
#[derive(Debug, Clone, PartialEq)]
pub enum BufferIndex {
    /// Plain buffer reference: `name` (no subscript).
    None,
    /// Integer literal: `name[0]`.
    Literal(u32, Span),
    /// Const or spawn index variable: `name[ch]`.
    Ident(Ident),
    /// All-element reference: `name[*]`.
    Star(Span),
}

/// A buffer reference with optional subscript.
#[derive(Debug, Clone, PartialEq)]
pub struct BufferRef {
    pub name: Ident,
    pub index: BufferIndex,
}

// ── Shared buffer array (v0.4.8) ──

/// `shared name[N]` — declares a family of N shared buffers.
#[derive(Debug, Clone, PartialEq)]
pub struct SharedDecl {
    pub name: Ident,
    pub size: ShapeDim,
    pub span: Span,
}

// ── Spawn clause (v0.4.8) ──

/// Spawn range bound — non-negative integer (unlike ShapeDim which requires > 0).
#[derive(Debug, Clone, PartialEq)]
pub enum SpawnBound {
    /// Integer literal bound: `0`, `24`.
    Literal(u32, Span),
    /// Const reference bound: `CH`.
    ConstRef(Ident),
}

/// Spawn clause on a task: `clock freq name[idx=begin..end] { ... }`.
#[derive(Debug, Clone, PartialEq)]
pub struct SpawnClause {
    pub index_var: Ident,
    pub begin: SpawnBound,
    pub end: SpawnBound,
    pub span: Span,
}

// ── Identifier ──

/// An identifier with its source text and span.
#[derive(Debug, Clone, PartialEq)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}
