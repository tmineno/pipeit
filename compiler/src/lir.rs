//! LIR – Low-level IR for syntax-directed C++ codegen.
//!
//! `LirProgram` is a self-contained, pre-resolved representation of the
//! compiled pipeline.  Codegen reads it and emits C++ without consulting
//! any upstream phase output.
//!
//! See ADR-025 for design rationale.

use std::collections::{HashMap, HashSet};

use crate::analyze::AnalyzedProgram;
use crate::ast::{Arg, Scalar, SetValue, ShapeConstraint, Value};
use crate::graph::{Edge, NodeId, NodeKind, ProgramGraph, Subgraph, TaskGraph};
use crate::hir::{HirSwitchSource, HirTaskBody};
use crate::registry::{ActorMeta, ParamKind, ParamType, PipitType, TokenCount};
use crate::schedule::{FiringEntry, ScheduledProgram, SubgraphSchedule, TaskSchedule};
use crate::subgraph_index::{
    build_subgraph_indices, identify_back_edges, subgraphs_of, GraphQueryCtx, SubgraphIndex,
};
use crate::thir::ThirContext;

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
    /// True if actor can be hoisted above K-loop (no ParamRef args).
    pub tick_hoistable: bool,
}

/// Structured actor argument — resolved by LIR builder, formatted by codegen.
#[derive(Clone)]
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
    /// Source node ID (for retry variable naming: `_rb_retry_{src}_{tgt}`).
    pub src_node_id: NodeId,
    /// Target/peer node ID (for retry variable naming).
    pub peer_node_id: NodeId,
}

// ── Probes ─────────────────────────────────────────────────────────────────

pub struct LirProbe {
    pub name: String,
}

// ── Verification ─────────────────────────────────────────────────────────────

/// Machine-checkable evidence for LIR postconditions (R1-R2).
#[derive(Debug, Clone)]
pub struct LirCert {
    /// R1: Every scheduled task has a corresponding LIR task.
    pub r1_all_tasks_present: bool,
    /// R2: Every actor firing has a resolved (non-empty) C++ name.
    pub r2_all_actors_resolved: bool,
}

impl crate::pass::StageCert for LirCert {
    fn all_pass(&self) -> bool {
        self.r1_all_tasks_present && self.r2_all_actors_resolved
    }

    fn obligations(&self) -> Vec<(&'static str, bool)> {
        vec![
            ("R1_all_tasks_present", self.r1_all_tasks_present),
            ("R2_all_actors_resolved", self.r2_all_actors_resolved),
        ]
    }
}

/// Verify LIR postconditions.
pub fn verify_lir(lir: &LirProgram, schedule: &ScheduledProgram) -> LirCert {
    let r1 = verify_r1_all_tasks_present(lir, schedule);
    let r2 = verify_r2_all_actors_resolved(lir);
    LirCert {
        r1_all_tasks_present: r1,
        r2_all_actors_resolved: r2,
    }
}

/// R1: Every key in `schedule.tasks` has a corresponding `LirTask`.
fn verify_r1_all_tasks_present(lir: &LirProgram, schedule: &ScheduledProgram) -> bool {
    schedule
        .tasks
        .keys()
        .all(|name| lir.tasks.iter().any(|t| t.name == *name))
}

/// R2: Every `LirActorFiring` across the program has a non-empty `cpp_name`.
fn verify_r2_all_actors_resolved(lir: &LirProgram) -> bool {
    for task in &lir.tasks {
        if !check_subgraph_actors(&task_subgraphs(task)) {
            return false;
        }
    }
    true
}

/// Collect all subgraphs from a task body.
fn task_subgraphs(task: &LirTask) -> Vec<&LirSubgraph> {
    match &task.body {
        LirTaskBody::Pipeline(sub) => vec![sub],
        LirTaskBody::Modal(modal) => {
            let mut subs = vec![&modal.control];
            for (_, sub) in &modal.modes {
                subs.push(sub);
            }
            subs
        }
    }
}

/// Check that all actor firings in the given subgraphs have non-empty cpp_name.
fn check_subgraph_actors(subgraphs: &[&LirSubgraph]) -> bool {
    for sub in subgraphs {
        for group in &sub.firings {
            match group {
                LirFiringGroup::Single(firing) => {
                    if !check_firing_actor(firing) {
                        return false;
                    }
                }
                LirFiringGroup::Fused(chain) => {
                    for firing in &chain.body {
                        if !check_firing_actor(firing) {
                            return false;
                        }
                    }
                }
            }
        }
    }
    true
}

/// Check a single firing: if it's an Actor, verify cpp_name is non-empty.
fn check_firing_actor(firing: &LirFiring) -> bool {
    if let LirFiringKind::Actor(actor) = &firing.kind {
        if actor.cpp_name.is_empty() {
            return false;
        }
    }
    true
}

// ── Display ─────────────────────────────────────────────────────────────────

impl std::fmt::Display for LirProgram {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "LirProgram ({} consts, {} params, {} inter-task bufs, {} tasks, {} probes)",
            self.consts.len(),
            self.params.len(),
            self.inter_task_buffers.len(),
            self.tasks.len(),
            self.probes.len()
        )?;

        // Consts
        for c in &self.consts {
            match &c.value {
                LirConstValue::Scalar { literal } => {
                    writeln!(f, "  const {} = {}", c.name, literal)?;
                }
                LirConstValue::Array {
                    elem_type,
                    elements,
                } => {
                    writeln!(
                        f,
                        "  const {}: {}[{}] = [{}]",
                        c.name,
                        elem_type,
                        elements.len(),
                        elements.join(", ")
                    )?;
                }
            }
        }

        // Params
        for p in &self.params {
            writeln!(
                f,
                "  param {}: {} = \"{}\" (cli: {})",
                p.name, p.cpp_type, p.default_literal, p.cli_converter
            )?;
        }

        // Directives
        let timer = match &self.directives.timer_spin {
            LirTimerSpin::Fixed(n) => format!("Fixed({})", n),
            LirTimerSpin::Adaptive => "Adaptive".to_string(),
        };
        writeln!(
            f,
            "  directives: mem={}, overrun={}, timer={}",
            self.directives.mem_bytes, self.directives.overrun_policy, timer
        )?;

        // Inter-task buffers
        for buf in &self.inter_task_buffers {
            writeln!(
                f,
                "  inter-task {}: {}[{}] readers={} [{}]{}",
                buf.name,
                buf.cpp_type,
                buf.capacity_tokens,
                buf.reader_count,
                buf.reader_tasks.join(", "),
                if buf.skip_writes { " skip_writes" } else { "" }
            )?;
        }

        // Tasks
        for task in &self.tasks {
            fmt_lir_task(f, task, "  ")?;
        }

        // Probes
        for probe in &self.probes {
            writeln!(f, "  probe {}", probe.name)?;
        }

        Ok(())
    }
}

fn fmt_lir_task(f: &mut std::fmt::Formatter<'_>, task: &LirTask, indent: &str) -> std::fmt::Result {
    writeln!(
        f,
        "{}task '{}' @ {}Hz K={}",
        indent, task.name, task.freq_hz, task.k_factor
    )?;

    // Feedback buffers
    for fb in &task.feedback_buffers {
        writeln!(
            f,
            "{}  feedback {}: {}[{}] init={}",
            indent, fb.var_name, fb.cpp_type, fb.tokens, fb.init_val
        )?;
    }

    match &task.body {
        LirTaskBody::Pipeline(sub) => {
            fmt_lir_subgraph(f, sub, &format!("{}  ", indent))?;
        }
        LirTaskBody::Modal(modal) => {
            writeln!(
                f,
                "{}  ctrl: {}",
                indent,
                fmt_ctrl_source(&modal.ctrl_source)
            )?;
            writeln!(f, "{}  control subgraph:", indent)?;
            fmt_lir_subgraph(f, &modal.control, &format!("{}    ", indent))?;
            for (i, (mode_name, sub)) in modal.modes.iter().enumerate() {
                write!(f, "{}  mode '{}'", indent, mode_name)?;
                if let Some(resets) = modal.mode_feedback_resets.get(i) {
                    if !resets.is_empty() {
                        let reset_names: Vec<&str> =
                            resets.iter().map(|r| r.var_name.as_str()).collect();
                        write!(f, " resets=[{}]", reset_names.join(", "))?;
                    }
                }
                writeln!(f)?;
                fmt_lir_subgraph(f, sub, &format!("{}    ", indent))?;
            }
        }
    }
    Ok(())
}

fn fmt_ctrl_source(src: &LirCtrlSource) -> String {
    match src {
        LirCtrlSource::Param { name } => format!("param({})", name),
        LirCtrlSource::EdgeBuffer { var_name } => format!("edge({})", var_name),
        LirCtrlSource::RingBuffer { name, reader_idx } => {
            format!("ring({}, reader={})", name, reader_idx)
        }
    }
}

fn fmt_lir_subgraph(
    f: &mut std::fmt::Formatter<'_>,
    sub: &LirSubgraph,
    indent: &str,
) -> std::fmt::Result {
    // Edge buffers
    if !sub.edge_buffers.is_empty() {
        let bufs: Vec<String> = sub
            .edge_buffers
            .iter()
            .filter(|eb| eb.alias_of.is_none())
            .map(|eb| {
                let fb_tag = if eb.is_feedback { " (fb)" } else { "" };
                format!("{}: {}[{}]{}", eb.var_name, eb.cpp_type, eb.tokens, fb_tag)
            })
            .collect();
        writeln!(f, "{}edge_buffers: {}", indent, bufs.join(", "))?;
    }

    // Firings
    writeln!(f, "{}firings:", indent)?;
    for group in &sub.firings {
        match group {
            LirFiringGroup::Single(firing) => {
                fmt_lir_firing(f, firing, &format!("{}  ", indent))?;
            }
            LirFiringGroup::Fused(chain) => {
                writeln!(f, "{}  fused [x{}]:", indent, chain.repetition)?;
                for h in &chain.hoisted_actors {
                    writeln!(
                        f,
                        "{}    hoist {} = {}({})",
                        indent,
                        h.var_name,
                        h.cpp_name,
                        fmt_args(&h.params)
                    )?;
                }
                for firing in &chain.body {
                    fmt_lir_firing(f, firing, &format!("{}    ", indent))?;
                }
            }
        }
    }
    Ok(())
}

fn fmt_lir_firing(
    f: &mut std::fmt::Formatter<'_>,
    firing: &LirFiring,
    indent: &str,
) -> std::fmt::Result {
    let rep = if firing.repetition > 1 {
        format!("[x{}] ", firing.repetition)
    } else {
        String::new()
    };
    match &firing.kind {
        LirFiringKind::Actor(actor) => {
            let inputs: Vec<&str> = actor.inputs.iter().map(|e| e.buffer_var.as_str()).collect();
            let outputs: Vec<&str> = actor
                .outputs
                .iter()
                .map(|e| e.buffer_var.as_str())
                .collect();
            writeln!(
                f,
                "{}{}{}<{}>({}){} -> [{}]",
                indent,
                rep,
                actor.cpp_name,
                actor.out_type,
                fmt_args(&actor.params),
                if inputs.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", inputs.join(", "))
                },
                outputs.join(", ")
            )
        }
        LirFiringKind::Fork(fork) => {
            writeln!(f, "{}{}fork(~{})", indent, rep, fork.tap_name)
        }
        LirFiringKind::Probe(probe) => {
            writeln!(
                f,
                "{}{}probe(?{}) {} tokens={}",
                indent, rep, probe.probe_name, probe.src_var, probe.tokens
            )
        }
        LirFiringKind::BufferRead(io) => {
            writeln!(
                f,
                "{}{}buf_read({}) -> {} tokens={}{}",
                indent,
                rep,
                io.buffer_name,
                io.edge_var,
                io.total_tokens,
                if io.skip { " skip" } else { "" }
            )
        }
        LirFiringKind::BufferWrite(io) => {
            writeln!(
                f,
                "{}{}buf_write({}) <- {} tokens={}{}",
                indent,
                rep,
                io.buffer_name,
                io.edge_var,
                io.total_tokens,
                if io.skip { " skip" } else { "" }
            )
        }
    }
}

fn fmt_args(args: &[LirActorArg]) -> String {
    args.iter()
        .map(|a| match a {
            LirActorArg::Literal(lit) => lit.clone(),
            LirActorArg::ParamRef(name) => format!("${}", name),
            LirActorArg::ConstScalar(lit) => lit.clone(),
            LirActorArg::ConstSpan { name, len } => format!(":{}[{}]", name, len),
            LirActorArg::ConstArrayLen(n) => format!("len({})", n),
            LirActorArg::DimValue(n) => format!("dim({})", n),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

// ── Builder ────────────────────────────────────────────────────────────────

/// Build a complete LIR program from upstream phase outputs.
///
/// Preconditions: all upstream phases (parse, resolve, HIR, type_infer, lower,
/// graph, ThirContext, analyze, schedule) have completed successfully.
///
/// Postconditions: the returned `LirProgram` is self-contained — codegen needs
/// only `&LirProgram` and `&CodegenOptions` to emit C++.
pub fn build_lir(
    thir: &ThirContext,
    graph: &ProgramGraph,
    analysis: &AnalyzedProgram,
    schedule: &ScheduledProgram,
) -> LirProgram {
    let subgraph_indices = build_subgraph_indices(graph);
    let builder = LirBuilder {
        thir,
        graph,
        analysis,
        schedule,
        subgraph_indices,
    };
    builder.build()
}

struct LirBuilder<'a> {
    thir: &'a ThirContext<'a>,
    graph: &'a ProgramGraph,
    analysis: &'a AnalyzedProgram,
    schedule: &'a ScheduledProgram,
    subgraph_indices: HashMap<usize, SubgraphIndex>,
}

impl<'a> LirBuilder<'a> {
    fn query_ctx(&self) -> GraphQueryCtx<'_> {
        GraphQueryCtx::new(&self.subgraph_indices)
    }

    fn build(&self) -> LirProgram {
        LirProgram {
            consts: self.build_consts(),
            params: self.build_params(),
            directives: self.build_directives(),
            inter_task_buffers: self.build_inter_task_buffers(),
            tasks: self.build_tasks(),
            probes: self.build_probes(),
            total_memory: self.analysis.total_memory,
        }
    }

    fn gqctx(&self) -> GraphQueryCtx<'_> {
        GraphQueryCtx::new(&self.subgraph_indices)
    }

    // ── Constants ──────────────────────────────────────────────────────

    fn build_consts(&self) -> Vec<LirConst> {
        self.thir
            .hir
            .consts
            .iter()
            .map(|c| LirConst {
                name: c.name.clone(),
                value: match &c.value {
                    Value::Scalar(s) => LirConstValue::Scalar {
                        literal: scalar_literal(s),
                    },
                    Value::Array(elems, _) => LirConstValue::Array {
                        elem_type: "float",
                        elements: elems.iter().map(scalar_literal).collect(),
                    },
                },
            })
            .collect()
    }

    // ── Parameters ─────────────────────────────────────────────────────

    fn build_params(&self) -> Vec<LirParam> {
        self.thir
            .hir
            .params
            .iter()
            .map(|p| {
                let cpp_type = self.thir.param_cpp_type(&p.name);
                LirParam {
                    name: p.name.clone(),
                    cpp_type,
                    default_literal: scalar_literal(&p.default_value),
                    cli_converter: cli_converter_for_type(cpp_type),
                }
            })
            .collect()
    }

    // ── Directives ─────────────────────────────────────────────────────

    fn build_directives(&self) -> LirDirectives {
        let timer_spin = match self.thir.set_directive("timer_spin") {
            None => LirTimerSpin::Fixed(10000),
            Some(d) => match &d.value {
                SetValue::Number(n, _) => LirTimerSpin::Fixed(*n as i64),
                SetValue::Ident(ident) if ident.name == "auto" => LirTimerSpin::Adaptive,
                _ => LirTimerSpin::Fixed(10000),
            },
        };
        LirDirectives {
            mem_bytes: self.thir.mem_bytes,
            overrun_policy: self.thir.overrun_policy.clone(),
            timer_spin,
        }
    }

    // ── Inter-task buffers ─────────────────────────────────────────────

    fn build_inter_task_buffers(&self) -> Vec<LirInterTaskBuffer> {
        let mut buf_names: Vec<&String> = self.thir.resolved.buffers.keys().collect();
        buf_names.sort();
        buf_names
            .into_iter()
            .map(|name| {
                let wire_type = self.infer_buffer_wire_type(name);
                let cpp_type = pipit_type_to_cpp(wire_type);
                let capacity_tokens = self.inter_task_buffer_capacity(name, wire_type);
                let reader_tasks = self.buffer_reader_tasks(name);
                let reader_count = reader_tasks.len().max(1);
                let skip_writes = self
                    .thir
                    .resolved
                    .buffers
                    .get(name)
                    .map(|info| info.readers.is_empty())
                    .unwrap_or(false);
                LirInterTaskBuffer {
                    name: name.clone(),
                    cpp_type,
                    capacity_tokens,
                    reader_count,
                    reader_tasks,
                    skip_writes,
                }
            })
            .collect()
    }

    // ── Probes ─────────────────────────────────────────────────────────

    fn build_probes(&self) -> Vec<LirProbe> {
        self.thir
            .resolved
            .probes
            .iter()
            .map(|p| LirProbe {
                name: p.name.clone(),
            })
            .collect()
    }

    // ── Tasks ──────────────────────────────────────────────────────────

    fn build_tasks(&self) -> Vec<LirTask> {
        let mut task_names: Vec<&String> = self.schedule.tasks.keys().collect();
        task_names.sort();
        task_names
            .into_iter()
            .filter_map(|name| self.build_task(name))
            .collect()
    }

    fn build_task(&self, task_name: &str) -> Option<LirTask> {
        let meta = self.schedule.tasks.get(task_name)?;
        let task_graph = self.graph.tasks.get(task_name)?;

        let used_params = self.collect_used_params(task_name, task_graph);
        let feedback_buffers = self.build_feedback_buffers(task_name, task_graph, &meta.schedule);

        let body = match (&meta.schedule, task_graph) {
            (TaskSchedule::Pipeline(sched), TaskGraph::Pipeline(sub)) => {
                LirTaskBody::Pipeline(self.build_subgraph(task_name, sub, sched))
            }
            (
                TaskSchedule::Modal {
                    control: ctrl_sched,
                    modes: mode_scheds,
                },
                TaskGraph::Modal { control, modes },
            ) => {
                let ctrl_sub = self.build_subgraph(task_name, control, ctrl_sched);
                let ctrl_source = self.resolve_ctrl_source(task_name, control, ctrl_sched);

                let mut lir_modes = Vec::new();
                let mut mode_feedback_resets = Vec::new();
                for (mode_name, mode_sched) in mode_scheds {
                    if let Some((_, sub)) = modes.iter().find(|(n, _)| n == mode_name) {
                        lir_modes.push((
                            mode_name.clone(),
                            self.build_subgraph(task_name, sub, mode_sched),
                        ));
                        mode_feedback_resets
                            .push(self.build_mode_feedback_resets(task_name, modes));
                    }
                }

                LirTaskBody::Modal(LirModalBody {
                    control: ctrl_sub,
                    ctrl_source,
                    modes: lir_modes,
                    mode_feedback_resets,
                })
            }
            _ => return None,
        };

        Some(LirTask {
            name: task_name.to_string(),
            freq_hz: meta.freq_hz,
            k_factor: meta.k_factor,
            body,
            used_params,
            feedback_buffers,
        })
    }

    // ── Used params ────────────────────────────────────────────────────

    fn collect_used_params(&self, task_name: &str, task_graph: &TaskGraph) -> Vec<LirUsedParam> {
        let mut params = HashSet::new();
        for sub in subgraphs_of(task_graph) {
            for node in &sub.nodes {
                if let NodeKind::Actor { args, .. } = &node.kind {
                    for arg in args {
                        if let Arg::ParamRef(ident) = arg {
                            params.insert(ident.name.clone());
                        }
                    }
                }
            }
        }
        // Modal switch($param) also uses a param.
        if let Some(task) = self.thir.task_info(task_name) {
            if let HirTaskBody::Modal(modal) = &task.body {
                if let HirSwitchSource::Param(name, _) = &modal.switch {
                    params.insert(name.clone());
                }
            }
        }
        let mut sorted: Vec<LirUsedParam> = params
            .into_iter()
            .map(|name| {
                let cpp_type = self.thir.param_cpp_type(&name);
                LirUsedParam { name, cpp_type }
            })
            .collect();
        sorted.sort_by(|a, b| a.name.cmp(&b.name));
        sorted
    }

    // ── Feedback buffers ───────────────────────────────────────────────

    fn build_feedback_buffers(
        &self,
        task_name: &str,
        task_graph: &TaskGraph,
        task_schedule: &TaskSchedule,
    ) -> Vec<LirFeedbackBuffer> {
        let subs_and_scheds: Vec<(&Subgraph, &SubgraphSchedule)> = match (task_graph, task_schedule)
        {
            (TaskGraph::Pipeline(sub), TaskSchedule::Pipeline(sched)) => vec![(sub, sched)],
            (
                TaskGraph::Modal { control, modes },
                TaskSchedule::Modal {
                    control: ctrl_sched,
                    modes: mode_scheds,
                },
            ) => {
                let mut v: Vec<(&Subgraph, &SubgraphSchedule)> = vec![(control, ctrl_sched)];
                for (name, sched) in mode_scheds {
                    if let Some((_, sub)) = modes.iter().find(|(n, _)| n == name) {
                        v.push((sub, sched));
                    }
                }
                v
            }
            _ => vec![],
        };

        let mut buffers = Vec::new();
        let mut seen = HashSet::new();
        for (sub, sched) in subs_and_scheds {
            let back_edges = identify_back_edges(sub, &self.graph.cycles);
            for (src, tgt) in &back_edges {
                let key = (src.0, tgt.0);
                if !seen.insert(key) {
                    continue;
                }
                let tokens = sched.edge_buffers.get(&(*src, *tgt)).copied().unwrap_or(1);
                let wire_type = self.infer_edge_wire_type(sub, *src);
                let cpp_type = pipit_type_to_cpp(wire_type);
                let init_val = self.delay_init_value(sub, *src);
                let var_name = format!("_fb_{}_{}", src.0, tgt.0);
                buffers.push(LirFeedbackBuffer {
                    var_name,
                    cpp_type,
                    tokens,
                    init_val,
                });
            }
        }
        // Sort by variable name to match the back_edges iteration order from codegen.
        // Back edges come from HashSet, which has non-deterministic order. But codegen
        // iterates them unsorted within emit_feedback_buffers. To match byte-identical
        // output, we must preserve the same declaration order. Since back edges are
        // identified per-subgraph (control first, then modes) and the HashSet iteration
        // within each subgraph is non-deterministic, we don't sort here — the `seen`
        // set deduplicates across subgraphs while preserving first-encounter order.
        let _ = task_name; // used in identify_back_edges call pattern
        buffers
    }

    fn delay_init_value(&self, sub: &Subgraph, node_id: NodeId) -> String {
        let gq = self.gqctx();
        if let Some(node) = gq.node_in_subgraph(sub, node_id) {
            if let NodeKind::Actor { args, name, .. } = &node.kind {
                if name == "delay" {
                    if let Some(arg) = args.get(1) {
                        return self.arg_to_literal(arg);
                    }
                }
            }
        }
        "0".to_string()
    }

    // ── Modal feedback resets ──────────────────────────────────────────

    fn build_mode_feedback_resets(
        &self,
        _task_name: &str,
        mode_subs: &[(String, Subgraph)],
    ) -> Vec<LirFeedbackReset> {
        let mut resets = Vec::new();
        for (_mode_name, sub) in mode_subs {
            let back_edges = identify_back_edges(sub, &self.graph.cycles);
            for (src, tgt) in &back_edges {
                let tokens = sub
                    .edges
                    .iter()
                    .find(|e| e.source == *src && e.target == *tgt)
                    .map(|_| 1u32) // back edge tokens come from schedule, not graph edges
                    .unwrap_or(1);
                // Use schedule edge buffer tokens if available — but we don't have the
                // mode schedule here. Use a conservative default.
                let _ = tokens;
                let init_val = self.delay_init_value(sub, *src);
                let var_name = format!("_fb_{}_{}", src.0, tgt.0);
                resets.push(LirFeedbackReset {
                    var_name,
                    tokens: 1, // Will be refined when mode schedule is available
                    init_val,
                });
            }
        }
        resets
    }

    // ── Ctrl source resolution ─────────────────────────────────────────

    fn resolve_ctrl_source(
        &self,
        task_name: &str,
        ctrl_sub: &Subgraph,
        ctrl_sched: &SubgraphSchedule,
    ) -> LirCtrlSource {
        let switch_source = self.thir.task_info(task_name).and_then(|task| {
            if let HirTaskBody::Modal(modal) = &task.body {
                Some(modal.switch.clone())
            } else {
                None
            }
        });

        match switch_source {
            Some(HirSwitchSource::Param(name, _)) => LirCtrlSource::Param { name },
            Some(HirSwitchSource::Buffer(name, _)) => {
                // Check if the buffer is read via a BufferRead in the ctrl subgraph
                if let Some(ctrl_var) = self.find_ctrl_output_for_buffer(ctrl_sub, &name) {
                    LirCtrlSource::EdgeBuffer { var_name: ctrl_var }
                } else {
                    let reader_idx = self
                        .buffer_reader_tasks(&name)
                        .iter()
                        .position(|t| t == task_name)
                        .unwrap_or(0);
                    LirCtrlSource::RingBuffer { name, reader_idx }
                }
            }
            None => {
                let ctrl_var = self.find_ctrl_output(ctrl_sub, ctrl_sched);
                LirCtrlSource::EdgeBuffer { var_name: ctrl_var }
            }
        }
    }

    fn find_ctrl_output(&self, ctrl_sub: &Subgraph, _ctrl_sched: &SubgraphSchedule) -> String {
        for node in ctrl_sub.nodes.iter().rev() {
            if let NodeKind::BufferWrite { .. } = &node.kind {
                let incoming: Vec<&Edge> = ctrl_sub
                    .edges
                    .iter()
                    .filter(|e| e.target == node.id)
                    .collect();
                if let Some(edge) = incoming.first() {
                    return format!("_e{}_{}[0]", edge.source.0, edge.target.0);
                }
            }
        }
        "0".to_string()
    }

    fn find_ctrl_output_for_buffer(
        &self,
        ctrl_sub: &Subgraph,
        ctrl_buffer_name: &str,
    ) -> Option<String> {
        for node in ctrl_sub.nodes.iter().rev() {
            let NodeKind::BufferWrite { buffer_name } = &node.kind else {
                continue;
            };
            if buffer_name != ctrl_buffer_name {
                continue;
            }
            let incoming: Vec<&Edge> = ctrl_sub
                .edges
                .iter()
                .filter(|e| e.target == node.id)
                .collect();
            if let Some(edge) = incoming.first() {
                return Some(format!("_e{}_{}[0]", edge.source.0, edge.target.0));
            }
        }
        None
    }

    // ── Subgraph building ──────────────────────────────────────────────

    fn build_subgraph(
        &self,
        task_name: &str,
        sub: &Subgraph,
        sched: &SubgraphSchedule,
    ) -> LirSubgraph {
        let back_edges = identify_back_edges(sub, &self.graph.cycles);
        let qctx = self.query_ctx();
        let aliases = build_passthrough_aliases(sub, &qctx);
        let edge_buffers = self.build_edge_buffers(sub, sched, &back_edges, &aliases);

        // Build name map for edge buffer variable names
        let edge_buf_names = self.build_edge_buf_name_map(sub, sched, &back_edges, &aliases);

        let firings = self.build_firing_groups(task_name, sub, sched, &edge_buf_names, &back_edges);

        LirSubgraph {
            edge_buffers,
            firings,
        }
    }

    fn build_edge_buffers(
        &self,
        sub: &Subgraph,
        sched: &SubgraphSchedule,
        back_edges: &HashSet<(NodeId, NodeId)>,
        aliases: &HashMap<(NodeId, NodeId), (NodeId, NodeId)>,
    ) -> Vec<LirEdgeBuffer> {
        let mut sorted_edges: Vec<_> = sched.edge_buffers.iter().collect();
        sorted_edges.sort_by_key(|&(&(src, tgt), _)| (src.0, tgt.0));

        // Pass 1: build non-aliased buffer entries
        let mut results = Vec::new();
        let mut names: HashMap<(NodeId, NodeId), String> = HashMap::new();
        for (&(src, tgt), &tokens) in &sorted_edges {
            if back_edges.contains(&(src, tgt)) {
                let var_name = format!("_fb_{}_{}", src.0, tgt.0);
                names.insert((src, tgt), var_name.clone());
                results.push(LirEdgeBuffer {
                    var_name,
                    cpp_type: "", // feedback buffers are declared in task prologue
                    tokens,
                    is_feedback: true,
                    alias_of: None,
                });
                continue;
            }
            if aliases.contains_key(&(src, tgt)) {
                continue;
            }
            let wire_type = self.infer_edge_wire_type(sub, src);
            let cpp_type = pipit_type_to_cpp(wire_type);
            let var_name = format!("_e{}_{}", src.0, tgt.0);
            names.insert((src, tgt), var_name.clone());
            results.push(LirEdgeBuffer {
                var_name,
                cpp_type,
                tokens,
                is_feedback: false,
                alias_of: None,
            });
        }

        // Pass 2: aliased edges
        for (&(src, tgt), &(alias_src, alias_tgt)) in aliases {
            let alias_name = names.get(&(alias_src, alias_tgt)).cloned();
            if let Some(alias_name) = alias_name {
                let var_name = format!("_e{}_{}", src.0, tgt.0);
                let tokens = sched.edge_buffers.get(&(src, tgt)).copied().unwrap_or(1);
                names.insert((src, tgt), var_name.clone());
                results.push(LirEdgeBuffer {
                    var_name,
                    cpp_type: "",
                    tokens,
                    is_feedback: false,
                    alias_of: Some(alias_name),
                });
            }
        }

        results
    }

    /// Build a map from edge (src,tgt) → variable name for use by firing builders.
    fn build_edge_buf_name_map(
        &self,
        sub: &Subgraph,
        sched: &SubgraphSchedule,
        back_edges: &HashSet<(NodeId, NodeId)>,
        aliases: &HashMap<(NodeId, NodeId), (NodeId, NodeId)>,
    ) -> HashMap<(NodeId, NodeId), String> {
        let mut names: HashMap<(NodeId, NodeId), String> = HashMap::new();

        // Back edges
        for &(src, tgt) in back_edges {
            names.insert((src, tgt), format!("_fb_{}_{}", src.0, tgt.0));
        }

        // Normal edges (sorted for determinism)
        let mut sorted_edges: Vec<_> = sched.edge_buffers.iter().collect();
        sorted_edges.sort_by_key(|&(&(src, tgt), _)| (src.0, tgt.0));
        for (&(src, tgt), _) in &sorted_edges {
            if names.contains_key(&(src, tgt)) || aliases.contains_key(&(src, tgt)) {
                continue;
            }
            names.insert((src, tgt), format!("_e{}_{}", src.0, tgt.0));
        }

        // Aliases
        for (&(src, tgt), &(alias_src, alias_tgt)) in aliases {
            if let Some(alias_name) = names.get(&(alias_src, alias_tgt)) {
                names.insert((src, tgt), alias_name.clone());
            }
        }

        let _ = sub; // used for context
        names
    }

    // ── Firing groups ──────────────────────────────────────────────────

    fn build_firing_groups(
        &self,
        task_name: &str,
        sub: &Subgraph,
        sched: &SubgraphSchedule,
        edge_bufs: &HashMap<(NodeId, NodeId), String>,
        back_edges: &HashSet<(NodeId, NodeId)>,
    ) -> Vec<LirFiringGroup> {
        let fused = self.plan_fusion_candidates(sub, sched, back_edges);
        let mut groups = Vec::new();
        let mut idx = 0;

        while idx < sched.firings.len() {
            if let Some(candidate) = fused.get(&idx) {
                if let Some(chain) =
                    self.build_fused_chain(task_name, sub, sched, candidate, edge_bufs)
                {
                    groups.push(LirFiringGroup::Fused(chain));
                    idx = candidate.end_idx + 1;
                    continue;
                }
            }

            let entry = &sched.firings[idx];
            let gq = self.gqctx();
            if let Some(node) = gq.node_in_subgraph(sub, entry.node_id) {
                let firing =
                    self.build_single_firing(task_name, sub, sched, node, entry, edge_bufs);
                groups.push(LirFiringGroup::Single(firing));
            }
            idx += 1;
        }

        groups
    }

    fn build_single_firing(
        &self,
        task_name: &str,
        sub: &Subgraph,
        sched: &SubgraphSchedule,
        node: &crate::graph::Node,
        entry: &FiringEntry,
        edge_bufs: &HashMap<(NodeId, NodeId), String>,
    ) -> LirFiring {
        let rep = entry.repetition_count;
        // Fork/Probe are zero-copy passthrough nodes; buffer I/O already performs
        // block transfers. These nodes do not get per-firing loops.
        let is_passthrough = matches!(node.kind, NodeKind::Fork { .. } | NodeKind::Probe { .. });
        let is_buffer_io = matches!(
            node.kind,
            NodeKind::BufferRead { .. } | NodeKind::BufferWrite { .. }
        );
        let needs_loop = rep > 1 && !is_passthrough && !is_buffer_io;
        let kind = self.build_firing_kind(task_name, sub, sched, node, edge_bufs, rep, true);
        LirFiring {
            kind,
            repetition: rep,
            needs_loop,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn build_firing_kind(
        &self,
        task_name: &str,
        sub: &Subgraph,
        sched: &SubgraphSchedule,
        node: &crate::graph::Node,
        edge_bufs: &HashMap<(NodeId, NodeId), String>,
        rep: u32,
        allow_hoist: bool,
    ) -> LirFiringKind {
        match &node.kind {
            NodeKind::Actor {
                name,
                args,
                call_id,
                shape_constraint,
                ..
            } => {
                let actor = self.build_actor_firing(
                    task_name,
                    sub,
                    sched,
                    node.id,
                    name,
                    *call_id,
                    args,
                    shape_constraint.as_ref(),
                    edge_bufs,
                    rep,
                    allow_hoist,
                );
                LirFiringKind::Actor(actor)
            }
            NodeKind::Fork { tap_name } => LirFiringKind::Fork(LirForkFiring {
                tap_name: tap_name.clone(),
            }),
            NodeKind::Probe { probe_name } => {
                let probe = self.build_probe_firing(sub, sched, node, probe_name, edge_bufs);
                LirFiringKind::Probe(probe)
            }
            NodeKind::BufferRead { buffer_name } => {
                let io =
                    self.build_buffer_read(task_name, sub, sched, node, buffer_name, edge_bufs);
                LirFiringKind::BufferRead(io)
            }
            NodeKind::BufferWrite { buffer_name } => {
                let io =
                    self.build_buffer_write(task_name, sub, sched, node, buffer_name, edge_bufs);
                LirFiringKind::BufferWrite(io)
            }
        }
    }

    // ── Actor firing ───────────────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    fn build_actor_firing(
        &self,
        task_name: &str,
        sub: &Subgraph,
        sched: &SubgraphSchedule,
        node_id: NodeId,
        actor_name: &str,
        call_id: crate::id::CallId,
        args: &[Arg],
        shape_constraint: Option<&ShapeConstraint>,
        edge_bufs: &HashMap<(NodeId, NodeId), String>,
        rep: u32,
        allow_hoist: bool,
    ) -> LirActorFiring {
        let meta = self.thir.concrete_actor(actor_name, call_id);
        let cpp_name = self.actor_cpp_name(actor_name, call_id);

        let schedule_dim_overrides = if let Some(meta) = meta {
            self.build_schedule_dim_overrides(meta, args, shape_constraint, sched, node_id, sub)
        } else {
            HashMap::new()
        };

        let params = if let Some(meta) = meta {
            self.resolve_actor_args(
                meta,
                args,
                shape_constraint,
                &schedule_dim_overrides,
                node_id,
            )
        } else {
            Vec::new()
        };

        let (in_type, out_type) = if let Some(meta) = meta {
            (
                pipit_type_to_cpp(meta.in_type.as_concrete().unwrap_or(PipitType::Float)),
                pipit_type_to_cpp(meta.out_type.as_concrete().unwrap_or(PipitType::Void)),
            )
        } else {
            ("float", "float")
        };

        let in_rate = self
            .analysis
            .node_port_rates
            .get(&node_id)
            .and_then(|r| r.in_rate);
        let out_rate = self
            .analysis
            .node_port_rates
            .get(&node_id)
            .and_then(|r| r.out_rate);

        let void_output = if let Some(meta) = meta {
            meta.out_type.as_concrete() == Some(PipitType::Void)
        } else {
            false
        };

        let inputs = self.build_edge_refs(sub, sched, node_id, edge_bufs, true);
        let outputs = self.build_edge_refs(sub, sched, node_id, edge_bufs, false);

        // Tick-level hoistable: no ParamRef or TapRef args (can live above K-loop)
        let tick_hoistable = is_actor_hoistable(args, false);

        let hoisted = if allow_hoist && rep > 1 && is_actor_hoistable(args, true) {
            Some(LirHoistedActor {
                var_name: format!("_actor_{}", node_id.0),
                cpp_name: cpp_name.clone(),
                params: params.clone(),
            })
        } else {
            None
        };

        let _ = task_name;
        LirActorFiring {
            actor_name: actor_name.to_string(),
            cpp_name,
            params,
            in_type,
            out_type,
            in_rate,
            out_rate,
            hoisted,
            inputs,
            outputs,
            node_id,
            void_output,
            tick_hoistable,
        }
    }

    fn build_edge_refs(
        &self,
        sub: &Subgraph,
        sched: &SubgraphSchedule,
        node_id: NodeId,
        edge_bufs: &HashMap<(NodeId, NodeId), String>,
        incoming: bool,
    ) -> Vec<LirEdgeRef> {
        let qctx = self.query_ctx();
        let edges: Vec<&Edge> = if incoming {
            qctx.incoming_edges(sub, node_id)
        } else {
            qctx.outgoing_edges(sub, node_id)
        };

        edges
            .into_iter()
            .filter_map(|e| {
                let key = (e.source, e.target);
                let buffer_var = edge_bufs.get(&key)?.clone();
                let tokens = sched.edge_buffers.get(&key).copied().unwrap_or(1);
                let peer_node_id = if incoming { e.source } else { e.target };
                Some(LirEdgeRef {
                    buffer_var,
                    tokens,
                    peer_node_id,
                })
            })
            .collect()
    }

    fn actor_cpp_name(&self, actor_name: &str, call_id: crate::id::CallId) -> String {
        if let Some(types) = self.thir.lowered.type_instantiations.get(&call_id) {
            if !types.is_empty() {
                let type_args: Vec<&str> = types.iter().map(|t| pipit_type_to_cpp(*t)).collect();
                return format!("Actor_{}<{}>", actor_name, type_args.join(", "));
            }
        }
        format!("Actor_{}", actor_name)
    }

    // ── Actor argument resolution ──────────────────────────────────────

    fn resolve_actor_args(
        &self,
        meta: &ActorMeta,
        args: &[Arg],
        shape_constraint: Option<&ShapeConstraint>,
        schedule_dim_overrides: &HashMap<String, u32>,
        node_id: NodeId,
    ) -> Vec<LirActorArg> {
        let mut parts = Vec::new();
        let mut last_array_const: Option<&Arg> = None;

        for (i, param) in meta.params.iter().enumerate() {
            if let Some(arg) = args.get(i) {
                match param.kind {
                    ParamKind::RuntimeParam => {
                        if let Arg::ParamRef(ident) = arg {
                            parts.push(LirActorArg::ParamRef(ident.name.clone()));
                        } else {
                            parts.push(self.arg_to_lir_literal(arg));
                        }
                        last_array_const = None;
                    }
                    ParamKind::Param => {
                        if self.is_const_array_ref(arg) && param.param_type == ParamType::Int {
                            last_array_const = Some(arg);
                        } else {
                            last_array_const = None;
                        }
                        parts.push(self.arg_to_lir_value(arg, &param.param_type));
                    }
                }
            } else if param.kind == ParamKind::Param {
                if let Some(val) = self.resolve_missing_param_value(
                    &param.name,
                    meta,
                    args,
                    shape_constraint,
                    schedule_dim_overrides,
                    node_id,
                ) {
                    parts.push(LirActorArg::DimValue(val));
                    continue;
                }
                self.try_autofill_span_param(&mut parts, &mut last_array_const, &param.param_type);
            } else {
                self.try_autofill_span_param(&mut parts, &mut last_array_const, &param.param_type);
            }
        }
        parts
    }

    fn resolve_missing_param_value(
        &self,
        param_name: &str,
        meta: &ActorMeta,
        args: &[Arg],
        shape_constraint: Option<&ShapeConstraint>,
        schedule_dim_overrides: &HashMap<String, u32>,
        node_id: NodeId,
    ) -> Option<u32> {
        self.resolve_dim_param_from_shape(param_name, meta, shape_constraint)
            .or_else(|| {
                self.thir
                    .infer_dim_param_from_span_args(param_name, meta, args)
            })
            .or_else(|| {
                self.analysis
                    .span_derived_dims
                    .get(&(node_id, param_name.to_string()))
                    .copied()
            })
            .or_else(|| schedule_dim_overrides.get(param_name).copied())
    }

    fn resolve_dim_param_from_shape(
        &self,
        param_name: &str,
        meta: &ActorMeta,
        shape_constraint: Option<&ShapeConstraint>,
    ) -> Option<u32> {
        let sc = shape_constraint?;
        for (i, dim) in meta.in_shape.dims.iter().enumerate() {
            if let TokenCount::Symbolic(sym) = dim {
                if sym == param_name {
                    return sc
                        .dims
                        .get(i)
                        .and_then(|sd| self.thir.resolve_shape_dim(sd));
                }
            }
        }
        for (i, dim) in meta.out_shape.dims.iter().enumerate() {
            if let TokenCount::Symbolic(sym) = dim {
                if sym == param_name {
                    return sc
                        .dims
                        .get(i)
                        .and_then(|sd| self.thir.resolve_shape_dim(sd));
                }
            }
        }
        None
    }

    fn is_const_array_ref(&self, arg: &Arg) -> bool {
        if let Arg::ConstRef(ident) = arg {
            if let Some(c) = self.thir.const_info(&ident.name) {
                return matches!(&c.value, Value::Array(_, _));
            }
        }
        false
    }

    fn arg_to_lir_literal(&self, arg: &Arg) -> LirActorArg {
        match arg {
            Arg::Value(Value::Scalar(s)) => LirActorArg::Literal(scalar_literal(s)),
            Arg::Value(Value::Array(_, _)) => LirActorArg::Literal("{}".to_string()),
            Arg::ParamRef(ident) => LirActorArg::ParamRef(ident.name.clone()),
            Arg::ConstRef(ident) => {
                if let Some(c) = self.thir.const_info(&ident.name) {
                    match &c.value {
                        Value::Scalar(s) => LirActorArg::ConstScalar(scalar_literal(s)),
                        Value::Array(_, _) => {
                            LirActorArg::Literal(format!("_const_{}", ident.name))
                        }
                    }
                } else {
                    LirActorArg::Literal(format!("_const_{}", ident.name))
                }
            }
            Arg::TapRef(_) => LirActorArg::Literal("/* tap */".to_string()),
        }
    }

    fn arg_to_lir_value(&self, arg: &Arg, param_type: &ParamType) -> LirActorArg {
        match arg {
            Arg::Value(Value::Scalar(s)) => LirActorArg::Literal(scalar_literal(s)),
            Arg::Value(Value::Array(_, _)) => LirActorArg::Literal("{}".to_string()),
            Arg::ConstRef(ident) => {
                if let Some(c) = self.thir.const_info(&ident.name) {
                    match &c.value {
                        Value::Scalar(s) => LirActorArg::ConstScalar(scalar_literal(s)),
                        Value::Array(elems, _) => {
                            if matches!(
                                param_type,
                                ParamType::SpanFloat
                                    | ParamType::SpanChar
                                    | ParamType::SpanTypeParam(_)
                            ) {
                                LirActorArg::ConstSpan {
                                    name: ident.name.clone(),
                                    len: elems.len() as u32,
                                }
                            } else {
                                LirActorArg::ConstArrayLen(elems.len() as u32)
                            }
                        }
                    }
                } else {
                    LirActorArg::Literal(format!("_const_{}", ident.name))
                }
            }
            Arg::ParamRef(ident) => LirActorArg::ParamRef(ident.name.clone()),
            Arg::TapRef(_) => LirActorArg::Literal("/* tap */".to_string()),
        }
    }

    fn try_autofill_span_param(
        &self,
        parts: &mut Vec<LirActorArg>,
        last_array_const: &mut Option<&Arg>,
        param_type: &ParamType,
    ) {
        let Some(array_arg) = *last_array_const else {
            return;
        };
        if !matches!(
            param_type,
            ParamType::SpanFloat | ParamType::SpanChar | ParamType::SpanTypeParam(_)
        ) {
            return;
        }
        parts.push(self.arg_to_lir_value(array_arg, param_type));
        *last_array_const = None;
    }

    // ── Schedule dim overrides ─────────────────────────────────────────

    fn build_schedule_dim_overrides(
        &self,
        meta: &ActorMeta,
        args: &[Arg],
        shape_constraint: Option<&ShapeConstraint>,
        sched: &SubgraphSchedule,
        node_id: NodeId,
        sub: &Subgraph,
    ) -> HashMap<String, u32> {
        let mut overrides = HashMap::new();
        let rep = firing_repetition(sched, node_id);
        let qctx = self.query_ctx();
        let in_edges = qctx.incoming_edges(sub, node_id);
        let incoming_override =
            consistent_dim_override_from_edges(sched, rep, in_edges.iter().copied());
        let out_edges = qctx.outgoing_edges(sub, node_id);
        let outgoing_override =
            consistent_dim_override_from_edges(sched, rep, out_edges.iter().copied());
        let symbolic_sides = symbolic_shape_sides(meta);
        let provided_params = provided_param_names(meta, args);

        for (sym, (in_side, out_side)) in symbolic_sides {
            if self.is_schedule_dim_override_blocked(
                sym,
                meta,
                args,
                shape_constraint,
                node_id,
                &provided_params,
            ) {
                continue;
            }
            if in_side {
                if let Some(val) = incoming_override {
                    overrides.insert(sym.to_string(), val);
                    continue;
                }
            }
            if out_side {
                if let Some(val) = outgoing_override {
                    overrides.insert(sym.to_string(), val);
                }
            }
        }
        overrides
    }

    fn is_schedule_dim_override_blocked(
        &self,
        sym: &str,
        meta: &ActorMeta,
        args: &[Arg],
        shape_constraint: Option<&ShapeConstraint>,
        node_id: NodeId,
        provided_params: &HashSet<&str>,
    ) -> bool {
        self.resolve_dim_param_from_shape(sym, meta, shape_constraint)
            .or_else(|| self.thir.infer_dim_param_from_span_args(sym, meta, args))
            .is_some()
            || self
                .analysis
                .span_derived_dims
                .contains_key(&(node_id, sym.to_string()))
            || provided_params.contains(sym)
    }

    // ── Probe firing ───────────────────────────────────────────────────

    fn build_probe_firing(
        &self,
        sub: &Subgraph,
        sched: &SubgraphSchedule,
        node: &crate::graph::Node,
        probe_name: &str,
        edge_bufs: &HashMap<(NodeId, NodeId), String>,
    ) -> LirProbeFiring {
        let incoming: Vec<&Edge> = self.query_ctx().incoming_edges(sub, node.id);
        if let Some(in_edge) = incoming.first() {
            if let Some(src_buf) = edge_bufs.get(&(in_edge.source, in_edge.target)) {
                let wire_type = self.infer_edge_wire_type(sub, in_edge.source);
                let cpp_type = pipit_type_to_cpp(wire_type);
                let tokens = sched
                    .edge_buffers
                    .get(&(in_edge.source, in_edge.target))
                    .copied()
                    .unwrap_or(1);
                let fmt_spec = fmt_spec_for_cpp_type(cpp_type);
                return LirProbeFiring {
                    probe_name: probe_name.to_string(),
                    src_var: src_buf.clone(),
                    tokens,
                    cpp_type,
                    fmt_spec,
                };
            }
        }
        LirProbeFiring {
            probe_name: probe_name.to_string(),
            src_var: String::new(),
            tokens: 0,
            cpp_type: "float",
            fmt_spec: "%f",
        }
    }

    // ── Buffer I/O ─────────────────────────────────────────────────────

    fn build_buffer_read(
        &self,
        task_name: &str,
        sub: &Subgraph,
        sched: &SubgraphSchedule,
        node: &crate::graph::Node,
        buffer_name: &str,
        edge_bufs: &HashMap<(NodeId, NodeId), String>,
    ) -> LirBufferIo {
        let outgoing: Vec<&Edge> = self.query_ctx().outgoing_edges(sub, node.id);
        let (edge_var, total_tokens) = if let Some(out_edge) = outgoing.first() {
            let var = edge_bufs
                .get(&(out_edge.source, out_edge.target))
                .cloned()
                .unwrap_or_default();
            let tokens = sched
                .edge_buffers
                .get(&(out_edge.source, out_edge.target))
                .copied()
                .unwrap_or(1);
            (var, tokens)
        } else {
            (String::new(), 1)
        };
        let reader_idx = self
            .buffer_reader_tasks(buffer_name)
            .iter()
            .position(|t| t == task_name);
        let (src_node_id, peer_node_id) = if let Some(out_edge) = outgoing.first() {
            (node.id, out_edge.target)
        } else {
            (node.id, node.id)
        };
        LirBufferIo {
            buffer_name: buffer_name.to_string(),
            task_name: task_name.to_string(),
            edge_var,
            total_tokens,
            reader_idx,
            skip: false,
            src_node_id,
            peer_node_id,
        }
    }

    fn build_buffer_write(
        &self,
        task_name: &str,
        sub: &Subgraph,
        sched: &SubgraphSchedule,
        node: &crate::graph::Node,
        buffer_name: &str,
        edge_bufs: &HashMap<(NodeId, NodeId), String>,
    ) -> LirBufferIo {
        let incoming: Vec<&Edge> = self.query_ctx().incoming_edges(sub, node.id);
        let (edge_var, total_tokens) = if let Some(in_edge) = incoming.first() {
            let var = edge_bufs
                .get(&(in_edge.source, in_edge.target))
                .cloned()
                .unwrap_or_default();
            let tokens = sched
                .edge_buffers
                .get(&(in_edge.source, in_edge.target))
                .copied()
                .unwrap_or(1);
            (var, tokens)
        } else {
            (String::new(), 1)
        };
        let skip = self
            .thir
            .resolved
            .buffers
            .get(buffer_name)
            .map(|info| info.readers.is_empty())
            .unwrap_or(false);
        let (src_node_id, peer_node_id) = if let Some(in_edge) = incoming.first() {
            (in_edge.source, node.id)
        } else {
            (node.id, node.id)
        };
        LirBufferIo {
            buffer_name: buffer_name.to_string(),
            task_name: task_name.to_string(),
            edge_var,
            total_tokens,
            reader_idx: None,
            skip,
            src_node_id,
            peer_node_id,
        }
    }

    // ── Fusion candidates ──────────────────────────────────────────────

    fn plan_fusion_candidates(
        &self,
        sub: &Subgraph,
        sched: &SubgraphSchedule,
        back_edges: &HashSet<(NodeId, NodeId)>,
    ) -> HashMap<usize, FusionCandidate> {
        let mut fused = HashMap::new();
        if sched.firings.len() < 2 {
            return fused;
        }

        let gq = self.gqctx();
        let mut i = 0usize;
        while i + 1 < sched.firings.len() {
            let start = i;
            let start_entry = &sched.firings[start];
            let rep = start_entry.repetition_count;
            if rep <= 1 || !is_fusion_entry_eligible(&gq, sub, start_entry, back_edges) {
                i += 1;
                continue;
            }

            let mut end = i;
            let mut node_ids = vec![start_entry.node_id];
            let mut chain_node_ids: HashSet<NodeId> = HashSet::from([start_entry.node_id]);

            while end + 1 < sched.firings.len() {
                let next = &sched.firings[end + 1];
                if !can_append_to_fusion_chain(&gq, sub, rep, next, &chain_node_ids, back_edges) {
                    break;
                }
                end += 1;
                node_ids.push(next.node_id);
                chain_node_ids.insert(next.node_id);
            }

            if node_ids.len() > 1 {
                fused.insert(
                    start,
                    FusionCandidate {
                        start_idx: start,
                        end_idx: end,
                        rep,
                        node_ids,
                    },
                );
                i = end + 1;
            } else {
                i += 1;
            }
        }

        fused
    }

    fn build_fused_chain(
        &self,
        task_name: &str,
        sub: &Subgraph,
        sched: &SubgraphSchedule,
        candidate: &FusionCandidate,
        edge_bufs: &HashMap<(NodeId, NodeId), String>,
    ) -> Option<LirFusedChain> {
        if candidate.start_idx >= sched.firings.len()
            || candidate.end_idx >= sched.firings.len()
            || candidate.start_idx >= candidate.end_idx
        {
            return None;
        }

        let gq = self.gqctx();
        if !candidate.node_ids.iter().all(|&nid| {
            matches!(
                gq.node_in_subgraph(sub, nid).map(|n| &n.kind),
                Some(NodeKind::Actor { .. } | NodeKind::Fork { .. } | NodeKind::Probe { .. })
            )
        }) {
            return None;
        }

        // Build hoisted actor declarations
        let mut hoisted_actors = Vec::new();
        for &node_id in &candidate.node_ids {
            let node = gq.node_in_subgraph(sub, node_id)?;
            if let NodeKind::Actor {
                name,
                args,
                call_id,
                shape_constraint,
                ..
            } = &node.kind
            {
                if is_actor_hoistable(args, true) {
                    let cpp_name = self.actor_cpp_name(name, *call_id);
                    let meta = self.thir.concrete_actor(name, *call_id);
                    let params = if let Some(meta) = meta {
                        let dim_overrides = self.build_schedule_dim_overrides(
                            meta,
                            args,
                            shape_constraint.as_ref(),
                            sched,
                            node_id,
                            sub,
                        );
                        self.resolve_actor_args(
                            meta,
                            args,
                            shape_constraint.as_ref(),
                            &dim_overrides,
                            node_id,
                        )
                    } else {
                        Vec::new()
                    };
                    hoisted_actors.push(LirHoistedActor {
                        var_name: format!("_actor_{}", node_id.0),
                        cpp_name,
                        params,
                    });
                }
            }
        }

        // Build body firings
        let mut body = Vec::new();
        for &node_id in &candidate.node_ids {
            let node = gq.node_in_subgraph(sub, node_id)?;
            let entry_rep = sched
                .firings
                .iter()
                .find(|f| f.node_id == node_id)
                .map(|f| f.repetition_count)
                .unwrap_or(1);
            let kind =
                self.build_firing_kind(task_name, sub, sched, node, edge_bufs, entry_rep, false);
            body.push(LirFiring {
                kind,
                repetition: entry_rep,
                needs_loop: false, // fused chain handles the loop
            });
        }

        Some(LirFusedChain {
            repetition: candidate.rep,
            hoisted_actors,
            body,
        })
    }

    // ── Wire type inference ────────────────────────────────────────────

    fn infer_edge_wire_type(&self, sub: &Subgraph, src_id: NodeId) -> PipitType {
        let gq = self.gqctx();
        let mut current = src_id;
        let mut visited = HashSet::new();
        loop {
            if !visited.insert(current) {
                return PipitType::Float;
            }
            if let Some(node) = gq.node_in_subgraph(sub, current) {
                match &node.kind {
                    NodeKind::Actor { name, call_id, .. } => {
                        if let Some(meta) = self.thir.concrete_actor(name, *call_id) {
                            return meta.out_type.as_concrete().unwrap_or(PipitType::Float);
                        }
                        return PipitType::Float;
                    }
                    NodeKind::BufferRead { buffer_name } => {
                        return self.infer_buffer_wire_type(buffer_name);
                    }
                    _ => {
                        if let Some(edge) = gq.first_incoming_edge(sub, current) {
                            current = edge.source;
                        } else {
                            return PipitType::Float;
                        }
                    }
                }
            } else {
                return PipitType::Float;
            }
        }
    }

    fn infer_buffer_wire_type(&self, buffer_name: &str) -> PipitType {
        let buf_info = match self.thir.resolved.buffers.get(buffer_name) {
            Some(b) => b,
            None => return PipitType::Float,
        };
        let task_graph = match self.graph.tasks.get(&buf_info.writer_task) {
            Some(g) => g,
            None => return PipitType::Float,
        };
        for sub in subgraphs_of(task_graph) {
            for node in &sub.nodes {
                if let NodeKind::BufferWrite {
                    buffer_name: name, ..
                } = &node.kind
                {
                    if name == buffer_name {
                        return self.infer_edge_wire_type(sub, node.id);
                    }
                }
            }
        }
        PipitType::Float
    }

    fn inter_task_buffer_capacity(&self, buf_name: &str, wire_type: PipitType) -> u32 {
        let bytes = self
            .analysis
            .inter_task_buffers
            .get(buf_name)
            .copied()
            .unwrap_or(1024);
        let elem_size = pipit_type_size(wire_type);
        if elem_size == 0 {
            return 1024;
        }
        (bytes / elem_size as u64).max(1) as u32
    }

    fn buffer_reader_tasks(&self, buffer_name: &str) -> Vec<String> {
        let mut readers = HashSet::new();
        if let Some(info) = self.thir.resolved.buffers.get(buffer_name) {
            for (task_name, _) in &info.readers {
                readers.insert(task_name.clone());
            }
        }
        let mut sorted: Vec<String> = readers.into_iter().collect();
        sorted.sort();
        sorted
    }

    /// Convert an Arg to a C++ literal string (for RuntimeParam / delay init).
    fn arg_to_literal(&self, arg: &Arg) -> String {
        match arg {
            Arg::Value(Value::Scalar(s)) => scalar_literal(s),
            Arg::Value(Value::Array(_, _)) => "{}".to_string(),
            Arg::ParamRef(ident) => format!("_param_{}_val", ident.name),
            Arg::ConstRef(ident) => {
                if let Some(c) = self.thir.const_info(&ident.name) {
                    match &c.value {
                        Value::Scalar(s) => return scalar_literal(s),
                        Value::Array(_, _) => return format!("_const_{}", ident.name),
                    }
                }
                format!("_const_{}", ident.name)
            }
            Arg::TapRef(_) => "/* tap */".to_string(),
        }
    }
}

// ── Free helpers ────────────────────────────────────────────────────────────

fn scalar_literal(scalar: &Scalar) -> String {
    match scalar {
        Scalar::Number(n, _, _) => {
            if *n == (*n as i64) as f64 && !n.is_nan() && !n.is_infinite() {
                format!("{}", *n as i64)
            } else {
                format!("{}f", n)
            }
        }
        Scalar::Freq(f, _) => format!("{:.1}", f),
        Scalar::Size(s, _) => format!("{}", s),
        Scalar::StringLit(s, _) => format!("\"{}\"", s),
        Scalar::Ident(ident) => format!("_const_{}", ident.name),
    }
}

fn pipit_type_to_cpp(t: PipitType) -> &'static str {
    match t {
        PipitType::Float => "float",
        PipitType::Double => "double",
        PipitType::Int8 => "int8_t",
        PipitType::Int16 => "int16_t",
        PipitType::Int32 => "int32_t",
        PipitType::Cfloat => "cfloat",
        PipitType::Cdouble => "cdouble",
        PipitType::Void => "void",
    }
}

fn pipit_type_size(t: PipitType) -> usize {
    match t {
        PipitType::Int8 => 1,
        PipitType::Int16 => 2,
        PipitType::Int32 | PipitType::Float => 4,
        PipitType::Double => 8,
        PipitType::Cfloat => 8,
        PipitType::Cdouble => 16,
        PipitType::Void => 0,
    }
}

fn cli_converter_for_type(cpp_type: &str) -> &'static str {
    match cpp_type {
        "int" => "std::stoi",
        "float" => "std::stof",
        "double" => "std::stod",
        _ => "std::stod",
    }
}

fn fmt_spec_for_cpp_type(cpp_type: &str) -> &'static str {
    match cpp_type {
        "float" | "double" => "%f",
        "int32_t" | "int16_t" | "int8_t" => "%d",
        _ => "%f",
    }
}

fn build_passthrough_aliases(
    sub: &Subgraph,
    qctx: &GraphQueryCtx,
) -> HashMap<(NodeId, NodeId), (NodeId, NodeId)> {
    let mut aliases = HashMap::new();
    for node in &sub.nodes {
        let is_passthrough = matches!(node.kind, NodeKind::Fork { .. } | NodeKind::Probe { .. });
        if !is_passthrough {
            continue;
        }
        let incoming = qctx.incoming_edges(sub, node.id);
        if let Some(in_edge) = incoming.first() {
            let src_key = (in_edge.source, in_edge.target);
            for out_edge in qctx.outgoing_edges(sub, node.id) {
                aliases.insert((out_edge.source, out_edge.target), src_key);
            }
        }
    }
    // Resolve transitive aliases
    let mut changed = true;
    while changed {
        changed = false;
        let snapshot: Vec<_> = aliases.iter().map(|(&k, &v)| (k, v)).collect();
        for (key, target) in snapshot {
            if let Some(&deeper) = aliases.get(&target) {
                if deeper != target {
                    aliases.insert(key, deeper);
                    changed = true;
                }
            }
        }
    }
    aliases
}

fn is_actor_hoistable(args: &[Arg], allow_param_ref: bool) -> bool {
    args.iter().all(|arg| match arg {
        Arg::Value(_) | Arg::ConstRef(_) => true,
        Arg::ParamRef(_) => allow_param_ref,
        Arg::TapRef(_) => false,
    })
}

fn firing_repetition(sched: &SubgraphSchedule, node_id: NodeId) -> u32 {
    sched
        .firings
        .iter()
        .find(|f| f.node_id == node_id)
        .map(|f| f.repetition_count)
        .unwrap_or(1)
}

fn consistent_dim_override_from_edges<'e, I>(
    sched: &SubgraphSchedule,
    rep: u32,
    mut edges: I,
) -> Option<u32>
where
    I: Iterator<Item = &'e Edge>,
{
    if rep == 0 {
        return None;
    }
    let first = edges.next()?;
    let candidate = sched
        .edge_buffers
        .get(&(first.source, first.target))
        .copied()
        .unwrap_or(1)
        / rep;
    for edge in edges {
        let tokens = sched
            .edge_buffers
            .get(&(edge.source, edge.target))
            .copied()
            .unwrap_or(1);
        let val = tokens / rep;
        if candidate != val {
            return None;
        }
    }
    Some(candidate)
}

fn symbolic_shape_sides(meta: &ActorMeta) -> HashMap<&str, (bool, bool)> {
    let mut sides = HashMap::new();
    for dim in &meta.in_shape.dims {
        let TokenCount::Symbolic(sym) = dim else {
            continue;
        };
        sides
            .entry(sym.as_str())
            .and_modify(|entry: &mut (bool, bool)| entry.0 = true)
            .or_insert((true, false));
    }
    for dim in &meta.out_shape.dims {
        let TokenCount::Symbolic(sym) = dim else {
            continue;
        };
        sides
            .entry(sym.as_str())
            .and_modify(|entry: &mut (bool, bool)| entry.1 = true)
            .or_insert((false, true));
    }
    sides
}

fn provided_param_names<'m>(meta: &'m ActorMeta, args: &[Arg]) -> HashSet<&'m str> {
    meta.params
        .iter()
        .zip(args.iter())
        .map(|(p, _)| p.name.as_str())
        .collect()
}

#[derive(Debug, Clone)]
struct FusionCandidate {
    start_idx: usize,
    end_idx: usize,
    rep: u32,
    node_ids: Vec<NodeId>,
}

fn is_fusion_entry_eligible(
    gq: &GraphQueryCtx<'_>,
    sub: &Subgraph,
    entry: &FiringEntry,
    back_edges: &HashSet<(NodeId, NodeId)>,
) -> bool {
    let Some(node) = gq.node_in_subgraph(sub, entry.node_id) else {
        return false;
    };
    if back_edges
        .iter()
        .any(|(src, tgt)| *src == entry.node_id || *tgt == entry.node_id)
    {
        return false;
    }
    match &node.kind {
        NodeKind::Actor { args, .. } => {
            if args.iter().any(|arg| matches!(arg, Arg::TapRef(_))) {
                return false;
            }
            let in_deg = gq.incoming_edge_count(sub, entry.node_id);
            let out_deg = gq.outgoing_edge_count(sub, entry.node_id);
            in_deg <= 1 && out_deg == 1
        }
        NodeKind::Fork { .. } | NodeKind::Probe { .. } => true,
        _ => false,
    }
}

fn can_append_to_fusion_chain(
    gq: &GraphQueryCtx<'_>,
    sub: &Subgraph,
    rep: u32,
    next: &FiringEntry,
    chain_node_ids: &HashSet<NodeId>,
    back_edges: &HashSet<(NodeId, NodeId)>,
) -> bool {
    if chain_node_ids.contains(&next.node_id) {
        return false;
    }
    let Some(next_node) = gq.node_in_subgraph(sub, next.node_id) else {
        return false;
    };
    let rep_compatible = match &next_node.kind {
        NodeKind::Actor { .. } => next.repetition_count == rep,
        NodeKind::Fork { .. } | NodeKind::Probe { .. } => true,
        _ => false,
    };
    if !rep_compatible {
        return false;
    }
    if !is_fusion_entry_eligible(gq, sub, next, back_edges) {
        return false;
    }
    sub.edges
        .iter()
        .any(|e| e.target == next.node_id && chain_node_ids.contains(&e.source))
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chumsky::span::Span as _;

    fn span() -> crate::ast::Span {
        crate::ast::Span::new((), 0..0)
    }

    #[test]
    fn scalar_literal_integer() {
        let s = Scalar::Number(42.0, span(), false);
        assert_eq!(scalar_literal(&s), "42");
    }

    #[test]
    fn scalar_literal_float() {
        let s = Scalar::Number(2.75, span(), false);
        assert_eq!(scalar_literal(&s), "2.75f");
    }

    #[test]
    fn scalar_literal_size() {
        let s = Scalar::Size(1024, span());
        assert_eq!(scalar_literal(&s), "1024");
    }

    #[test]
    fn cli_converter_types() {
        assert_eq!(cli_converter_for_type("int"), "std::stoi");
        assert_eq!(cli_converter_for_type("float"), "std::stof");
        assert_eq!(cli_converter_for_type("double"), "std::stod");
    }

    #[test]
    fn fmt_spec_types() {
        assert_eq!(fmt_spec_for_cpp_type("float"), "%f");
        assert_eq!(fmt_spec_for_cpp_type("int32_t"), "%d");
        assert_eq!(fmt_spec_for_cpp_type("cfloat"), "%f");
    }

    #[test]
    fn pipit_type_sizes() {
        assert_eq!(pipit_type_size(PipitType::Float), 4);
        assert_eq!(pipit_type_size(PipitType::Double), 8);
        assert_eq!(pipit_type_size(PipitType::Cfloat), 8);
        assert_eq!(pipit_type_size(PipitType::Void), 0);
    }

    #[test]
    fn hoistable_value_args() {
        let sp = span();
        let args = vec![
            Arg::Value(Value::Scalar(Scalar::Number(1.0, sp, false))),
            Arg::ConstRef(crate::ast::Ident {
                name: "x".to_string(),
                span: sp,
            }),
        ];
        assert!(is_actor_hoistable(&args, false));
        assert!(is_actor_hoistable(&args, true));
    }

    #[test]
    fn not_hoistable_with_tap_ref() {
        let sp = span();
        let args = vec![Arg::TapRef(crate::ast::Ident {
            name: "t".to_string(),
            span: sp,
        })];
        assert!(!is_actor_hoistable(&args, true));
    }

    #[test]
    fn param_ref_hoistable_when_allowed() {
        let sp = span();
        let args = vec![Arg::ParamRef(crate::ast::Ident {
            name: "p".to_string(),
            span: sp,
        })];
        assert!(!is_actor_hoistable(&args, false));
        assert!(is_actor_hoistable(&args, true));
    }

    // ── verify_lir tests ────────────────────────────────────────────────

    /// Build a full LIR + ScheduledProgram from source for verification.
    fn build_lir_and_schedule(source: &str) -> (LirProgram, crate::schedule::ScheduledProgram) {
        use std::path::PathBuf;
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_path_buf();
        let mut reg = crate::registry::Registry::new();
        for header in &[
            "runtime/libpipit/include/std_actors.h",
            "runtime/libpipit/include/std_math.h",
            "runtime/libpipit/include/std_sink.h",
            "runtime/libpipit/include/std_source.h",
            "examples/example_actors.h",
        ] {
            reg.load_header(&root.join(header))
                .unwrap_or_else(|e| panic!("failed to load {header}: {e}"));
        }
        let parse_result = crate::parser::parse(source);
        assert!(parse_result.errors.is_empty());
        let program = parse_result.program.unwrap();
        let mut resolve_result = crate::resolve::resolve(&program, &reg);
        let hir = crate::hir::build_hir(
            &program,
            &resolve_result.resolved,
            &mut resolve_result.id_alloc,
        );
        let graph_result = crate::graph::build_graph(&hir, &resolve_result.resolved, &reg);
        let type_result = crate::type_infer::type_infer(&hir, &resolve_result.resolved, &reg);
        let lower_result = crate::lower::lower_and_verify(
            &hir,
            &resolve_result.resolved,
            &type_result.typed,
            &reg,
        );
        let thir = crate::thir::build_thir_context(
            &hir,
            &resolve_result.resolved,
            &type_result.typed,
            &lower_result.lowered,
            &reg,
            &graph_result.graph,
        );
        let analysis = crate::analyze::analyze(&thir, &graph_result.graph);
        let sched_result =
            crate::schedule::schedule(&thir, &graph_result.graph, &analysis.analysis);
        let lir = build_lir(
            &thir,
            &graph_result.graph,
            &analysis.analysis,
            &sched_result.schedule,
        );
        (lir, sched_result.schedule)
    }

    #[test]
    fn verify_lir_passing() {
        use crate::pass::StageCert;
        let (lir, schedule) =
            build_lir_and_schedule("clock 1kHz t {\n    constant(0.0) | stdout()\n}");
        let cert = verify_lir(&lir, &schedule);
        assert!(
            cert.all_pass(),
            "cert should pass: {:?}",
            cert.obligations()
        );
    }

    #[test]
    fn verify_lir_r1_missing_task() {
        let (lir, mut schedule) =
            build_lir_and_schedule("clock 1kHz t {\n    constant(0.0) | stdout()\n}");
        // Inject a phantom task into the schedule that has no LIR counterpart
        schedule.tasks.insert(
            "phantom".to_string(),
            crate::schedule::TaskMeta {
                schedule: crate::schedule::TaskSchedule::Pipeline(
                    crate::schedule::SubgraphSchedule {
                        firings: vec![],
                        edge_buffers: std::collections::HashMap::new(),
                    },
                ),
                k_factor: 1,
                freq_hz: 1000.0,
            },
        );
        let cert = verify_lir(&lir, &schedule);
        assert!(!cert.r1_all_tasks_present, "R1 should fail");
        assert!(cert.r2_all_actors_resolved, "R2 should still pass");
    }

    #[test]
    fn verify_lir_r2_empty_cpp_name() {
        let (mut lir, schedule) =
            build_lir_and_schedule("clock 1kHz t {\n    constant(0.0) | stdout()\n}");
        // Corrupt the first actor's cpp_name to empty
        for task in &mut lir.tasks {
            match &mut task.body {
                LirTaskBody::Pipeline(sub) => {
                    for group in &mut sub.firings {
                        if let LirFiringGroup::Single(firing) = group {
                            if let LirFiringKind::Actor(actor) = &mut firing.kind {
                                actor.cpp_name = String::new();
                                break;
                            }
                        }
                        if let LirFiringGroup::Fused(chain) = group {
                            for firing in &mut chain.body {
                                if let LirFiringKind::Actor(actor) = &mut firing.kind {
                                    actor.cpp_name = String::new();
                                    break;
                                }
                            }
                        }
                    }
                }
                LirTaskBody::Modal(_) => {}
            }
        }
        let cert = verify_lir(&lir, &schedule);
        assert!(cert.r1_all_tasks_present, "R1 should still pass");
        assert!(!cert.r2_all_actors_resolved, "R2 should fail");
    }
}
