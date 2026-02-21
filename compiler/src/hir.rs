// hir.rs — High-level IR after resolve + define expansion.
//
// Normalized representation where all `define` calls are expanded inline,
// task metadata is extracted, and structural normalization is complete.
// Downstream phases (graph, analyze, schedule) consume HIR instead of raw AST.
//
// Preconditions: produced from a resolved AST (resolve phase complete).
// Postconditions: no DefineStmt or define calls remain; all pipe elements
//   are concrete actors, taps, probes, or buffer ops.
// Failure modes: define expansion depth exceeds limit (recursive defines).
// Side effects: allocates fresh CallIds for define-expanded actor calls.
//
// See ADR-024 for design rationale.

use std::collections::HashMap;

use crate::ast::{Arg, Scalar, SetValue, ShapeConstraint, Span, Value};
use crate::id::{CallId, DefId, TaskId};

// ── Program ─────────────────────────────────────────────────────────────────

/// Normalized program after resolve + define expansion.
///
/// All `define` calls are expanded inline. Task bodies are self-contained —
/// no indirection through define statements remains.
#[derive(Debug, Clone)]
pub struct HirProgram {
    pub tasks: Vec<HirTask>,
    pub consts: Vec<HirConst>,
    pub params: Vec<HirParam>,
    pub set_directives: Vec<HirSetDirective>,
    /// CallId maps for define-expanded calls (supplements resolve-phase maps).
    pub expanded_call_ids: HashMap<Span, CallId>,
    pub expanded_call_spans: HashMap<CallId, Span>,
}

// ── Task ────────────────────────────────────────────────────────────────────

/// A task with extracted metadata and normalized body.
#[derive(Debug, Clone)]
pub struct HirTask {
    pub name: String,
    pub task_id: TaskId,
    pub freq_hz: f64,
    pub freq_span: Span,
    pub body: HirTaskBody,
}

#[derive(Debug, Clone)]
pub enum HirTaskBody {
    Pipeline(HirPipeline),
    Modal(HirModal),
}

#[derive(Debug, Clone)]
pub struct HirPipeline {
    pub pipes: Vec<HirPipeExpr>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct HirModal {
    pub control: HirPipeline,
    pub modes: Vec<(String, HirPipeline)>,
    pub switch: HirSwitchSource,
    pub span: Span,
}

/// Normalized switch source (names only, no AST Ident wrapper).
#[derive(Debug, Clone)]
pub enum HirSwitchSource {
    Buffer(String),
    Param(String),
}

// ── Pipe expression ─────────────────────────────────────────────────────────

/// A pipe expression with defines already expanded.
///
/// `source → elements → optional sink`. No define calls remain.
#[derive(Debug, Clone)]
pub struct HirPipeExpr {
    pub source: HirPipeSource,
    pub elements: Vec<HirPipeElem>,
    pub sink: Option<String>, // buffer name for `-> name`
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum HirPipeSource {
    ActorCall(HirActorCall),
    BufferRead(String),
    TapRef(String),
}

#[derive(Debug, Clone)]
pub enum HirPipeElem {
    ActorCall(HirActorCall),
    Tap(String),
    Probe(String),
}

// ── Actor call ──────────────────────────────────────────────────────────────

/// A concrete actor call (no defines). Args are already substituted if
/// this call originated from define expansion.
#[derive(Debug, Clone)]
pub struct HirActorCall {
    pub name: String,
    pub call_id: CallId,
    pub call_span: Span,
    /// Actor arguments — reuses AST `Arg` type. Already substituted if
    /// this call was expanded from a define body.
    pub args: Vec<Arg>,
    /// Explicit type arguments (e.g., `actor<float>(...)`).
    pub type_args: Vec<String>,
    /// Optional shape constraint: `actor(...)[d0, d1, ...]`.
    pub shape_constraint: Option<ShapeConstraint>,
}

// ── Top-level declarations ──────────────────────────────────────────────────

/// Const declaration with precomputed value.
#[derive(Debug, Clone)]
pub struct HirConst {
    pub def_id: DefId,
    pub name: String,
    /// Reuses AST `Value` — either `Scalar` or `Array`.
    pub value: Value,
}

/// Param declaration with default value.
#[derive(Debug, Clone)]
pub struct HirParam {
    pub def_id: DefId,
    pub name: String,
    /// Reuses AST `Scalar` — preserves `is_int_literal` for type inference.
    pub default_value: Scalar,
}

/// Set directive (e.g., `set mem = 64M`, `set tick_rate = 1kHz`).
#[derive(Debug, Clone)]
pub struct HirSetDirective {
    pub name: String,
    /// Reuses AST `SetValue`.
    pub value: SetValue,
}
