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
    Task(TaskStmt),
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

// ── task_stmt: 'clock' FREQ IDENT '{' task_body '}' ──

#[derive(Debug, Clone, PartialEq)]
pub struct TaskStmt {
    pub freq: f64,
    pub freq_span: Span,
    pub name: Ident,
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
    /// `@name` — shared buffer read
    BufferRead(Ident),
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

/// `-> name` — shared buffer write (sink)
#[derive(Debug, Clone, PartialEq)]
pub struct Sink {
    pub buffer: Ident,
    pub span: Span,
}

// ── actor_call: IDENT '(' args? ')' shape_constraint? ──

#[derive(Debug, Clone, PartialEq)]
pub struct ActorCall {
    pub name: Ident,
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
    Number(f64, Span),
    Freq(f64, Span),
    Size(u64, Span),
    StringLit(String, Span),
    /// Bare identifier in value position — const reference (resolved later).
    Ident(Ident),
}

// ── Identifier ──

/// An identifier with its source text and span.
#[derive(Debug, Clone, PartialEq)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}
