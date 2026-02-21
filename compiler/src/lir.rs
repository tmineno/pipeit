//! LIR – Low-level IR for syntax-directed C++ codegen.
//!
//! `LirProgram` is a self-contained, pre-resolved representation of the
//! compiled pipeline.  Codegen reads it and emits C++ without consulting
//! any upstream phase output.
//!
//! See ADR-025 for design rationale.

use crate::graph::NodeId;

// ── Top-level ──────────────────────────────────────────────────────────────

pub struct LirProgram {
    pub consts: Vec<LirConst>,
    pub params: Vec<LirParam>,
    pub directives: LirDirectives,
    pub inter_task_buffers: Vec<LirInterTaskBuffer>,
    pub tasks: Vec<LirTask>,
    pub probes: Vec<LirProbe>,
    pub total_memory: u64,
}

// ── Constants ──────────────────────────────────────────────────────────────

pub struct LirConst {
    pub name: String,
    pub value: LirConstValue,
}

pub enum LirConstValue {
    /// Scalar constant: e.g. "256", "3.14f".
    Scalar { literal: String },
    /// Array constant: e.g. float[] = {1.0f, 2.0f}.
    Array {
        elem_type: &'static str,
        elements: Vec<String>,
    },
}

// ── Parameters ─────────────────────────────────────────────────────────────

pub struct LirParam {
    pub name: String,
    pub cpp_type: &'static str,
    pub default_literal: String,
    pub cli_converter: &'static str,
}

// ── Directives ─────────────────────────────────────────────────────────────

pub struct LirDirectives {
    pub mem_bytes: u64,
    pub overrun_policy: String,
    pub timer_spin: LirTimerSpin,
}

/// Timer spin mode — resolved from `set timer_spin` directive.
///
/// Three-way distinction: not set (default 10μs), explicit ns, or adaptive.
///
/// Resolution: LIR builder inspects raw HIR set-directive value:
///   - None → `Fixed(10000)` (default 10 μs)
///   - `SetValue::Ident("auto")` → `Adaptive` (sentinel -1)
///   - `SetValue::Number(n)` → `Fixed(n as i64)`
///
/// Cannot use `thir.timer_spin: Option<f64>` because it returns
/// `None` for both "not set" and "auto" (Ident values don't match Number).
pub enum LirTimerSpin {
    Fixed(i64),
    Adaptive,
}

// ── Inter-task buffers ─────────────────────────────────────────────────────

pub struct LirInterTaskBuffer {
    pub name: String,
    pub cpp_type: &'static str,
    pub capacity_tokens: u32,
    pub reader_count: usize,
    pub reader_tasks: Vec<String>,
    pub skip_writes: bool,
}

// ── Tasks ──────────────────────────────────────────────────────────────────

pub struct LirTask {
    pub name: String,
    pub freq_hz: f64,
    pub k_factor: u32,
    pub body: LirTaskBody,
    pub used_params: Vec<LirUsedParam>,
    pub feedback_buffers: Vec<LirFeedbackBuffer>,
}

pub struct LirUsedParam {
    pub name: String,
    pub cpp_type: &'static str,
}

pub enum LirTaskBody {
    Pipeline(LirSubgraph),
    Modal(LirModalBody),
}

pub struct LirModalBody {
    pub control: LirSubgraph,
    pub ctrl_source: LirCtrlSource,
    pub modes: Vec<(String, LirSubgraph)>,
    /// Per-mode feedback reset lists. Indexed parallel to `modes`.
    pub mode_feedback_resets: Vec<Vec<LirFeedbackReset>>,
}

pub enum LirCtrlSource {
    Param { name: String },
    EdgeBuffer { var_name: String },
    RingBuffer { name: String, reader_idx: usize },
}

pub struct LirFeedbackReset {
    pub var_name: String,
    pub tokens: u32,
    pub init_val: String,
}

pub struct LirFeedbackBuffer {
    pub var_name: String,
    pub cpp_type: &'static str,
    pub tokens: u32,
    pub init_val: String,
}

// ── Subgraph (scheduled firing sequence) ───────────────────────────────────

pub struct LirSubgraph {
    /// Invariant: sorted by (src_node_id, tgt_node_id) for deterministic output.
    pub edge_buffers: Vec<LirEdgeBuffer>,
    pub firings: Vec<LirFiringGroup>,
}

pub struct LirEdgeBuffer {
    pub var_name: String,
    pub cpp_type: &'static str,
    pub tokens: u32,
    pub is_feedback: bool,
    /// Passthrough alias — no declaration needed, use this var instead.
    pub alias_of: Option<String>,
}

pub enum LirFiringGroup {
    Single(LirFiring),
    Fused(LirFusedChain),
}

pub struct LirFusedChain {
    pub repetition: u32,
    pub hoisted_actors: Vec<LirHoistedActor>,
    pub body: Vec<LirFiring>,
}

// ── Per-firing data ────────────────────────────────────────────────────────

pub struct LirFiring {
    pub kind: LirFiringKind,
    pub repetition: u32,
    pub needs_loop: bool,
}

pub enum LirFiringKind {
    Actor(LirActorFiring),
    Fork(LirForkFiring),
    Probe(LirProbeFiring),
    BufferRead(LirBufferIo),
    BufferWrite(LirBufferIo),
}

pub struct LirActorFiring {
    pub actor_name: String,
    pub cpp_name: String,
    pub params: Vec<LirActorArg>,
    pub in_type: &'static str,
    pub out_type: &'static str,
    pub in_rate: Option<u32>,
    pub out_rate: Option<u32>,
    pub hoisted: Option<LirHoistedActor>,
    pub inputs: Vec<LirEdgeRef>,
    pub outputs: Vec<LirEdgeRef>,
    pub node_id: NodeId,
    pub void_output: bool,
}

/// Structured actor argument — resolved by LIR builder, formatted by codegen.
pub enum LirActorArg {
    /// Scalar literal: "256", "3.14f", "0".
    Literal(String),
    /// Runtime param reference: emits `_param_{name}_val`.
    ParamRef(String),
    /// Const scalar reference: emits resolved literal value.
    ConstScalar(String),
    /// Const array as span: emits `std::span<const {elem_type}>(_const_{name}, {len})`.
    ConstSpan { name: String, len: u32 },
    /// Const array length: emits the numeric count.
    ConstArrayLen(u32),
    /// Resolved dimension value from shape/span/schedule inference.
    DimValue(u32),
}

pub struct LirEdgeRef {
    pub buffer_var: String,
    pub tokens: u32,
    pub peer_node_id: NodeId,
}

pub struct LirHoistedActor {
    pub var_name: String,
    pub cpp_name: String,
    pub params: Vec<LirActorArg>,
}

pub struct LirForkFiring {
    pub tap_name: String,
}

pub struct LirProbeFiring {
    pub probe_name: String,
    pub src_var: String,
    pub tokens: u32,
    pub cpp_type: &'static str,
    pub fmt_spec: &'static str,
}

pub struct LirBufferIo {
    pub buffer_name: String,
    pub task_name: String,
    pub edge_var: String,
    pub total_tokens: u32,
    pub reader_idx: Option<usize>,
    pub skip: bool,
}

// ── Probes ─────────────────────────────────────────────────────────────────

pub struct LirProbe {
    pub name: String,
}
