// codegen.rs — C++ code generation for Pipit programs
//
// Transforms the scheduled SDF program into compilable C++ source code.
// Each task becomes a function with a timer loop, actor firing sequence,
// and intra-task edge buffers. Inter-task communication uses ring buffers.
//
// Preconditions: all upstream phases (parse, resolve, graph, analyze, schedule)
//                completed without errors.
// Postconditions: returns `CodegenResult` with generated C++ source string.
// Failure modes: missing actor metadata or unresolvable types produce diagnostics.
// Side effects: none.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::PathBuf;

use crate::analyze::AnalyzedProgram;
use crate::ast::*;
use crate::graph::*;
use crate::lower::LoweredProgram;
use crate::registry::{ActorMeta, ParamKind, ParamType, PipitType, Registry, TokenCount};
use crate::resolve::{Diagnostic, ResolvedProgram};
use crate::schedule::*;
use crate::subgraph_index::{build_subgraph_indices, subgraph_key, SubgraphIndex};

// ── Public types ────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct CodegenResult {
    pub generated: GeneratedCode,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug)]
pub struct GeneratedCode {
    pub cpp_source: String,
}

#[derive(Debug, Clone)]
pub struct CodegenOptions {
    pub release: bool,
    pub include_paths: Vec<PathBuf>,
}

// ── Public entry point ──────────────────────────────────────────────────────

pub fn codegen(
    program: &Program,
    resolved: &ResolvedProgram,
    graph: &ProgramGraph,
    analysis: &AnalyzedProgram,
    schedule: &ScheduledProgram,
    registry: &Registry,
    options: &CodegenOptions,
) -> CodegenResult {
    codegen_with_lowered(
        program, resolved, graph, analysis, schedule, registry, options, None,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn codegen_with_lowered(
    program: &Program,
    resolved: &ResolvedProgram,
    graph: &ProgramGraph,
    analysis: &AnalyzedProgram,
    schedule: &ScheduledProgram,
    registry: &Registry,
    options: &CodegenOptions,
    lowered: Option<&LoweredProgram>,
) -> CodegenResult {
    let mut ctx = CodegenCtx::new(
        program, resolved, graph, analysis, schedule, registry, options, lowered,
    );
    ctx.emit_all();
    ctx.build_result()
}

// ── Internal context ────────────────────────────────────────────────────────

struct CodegenCtx<'a> {
    program: &'a Program,
    resolved: &'a ResolvedProgram,
    graph: &'a ProgramGraph,
    analysis: &'a AnalyzedProgram,
    schedule: &'a ScheduledProgram,
    registry: &'a Registry,
    options: &'a CodegenOptions,
    lowered: Option<&'a LoweredProgram>,
    subgraph_indices: HashMap<usize, SubgraphIndex>,
    out: String,
    diagnostics: Vec<Diagnostic>,
}

#[derive(Debug)]
struct FiringPlan {
    use_loop: bool,
    body_indent: String,
    hoisted_actor_var: Option<String>,
}

#[derive(Debug)]
struct ActorCallPlan {
    in_ptr: String,
    out_ptr: String,
    call_expr: String,
}

#[derive(Debug, Clone)]
struct FusionCandidate {
    start_idx: usize,
    end_idx: usize,
    rep: u32,
    node_ids: Vec<NodeId>,
}

impl<'a> CodegenCtx<'a> {
    #[allow(clippy::too_many_arguments)]
    fn new(
        program: &'a Program,
        resolved: &'a ResolvedProgram,
        graph: &'a ProgramGraph,
        analysis: &'a AnalyzedProgram,
        schedule: &'a ScheduledProgram,
        registry: &'a Registry,
        options: &'a CodegenOptions,
        lowered: Option<&'a LoweredProgram>,
    ) -> Self {
        let subgraph_indices = build_subgraph_indices(graph);
        CodegenCtx {
            program,
            resolved,
            graph,
            analysis,
            schedule,
            registry,
            options,
            lowered,
            subgraph_indices,
            out: String::with_capacity(8192),
            diagnostics: Vec::new(),
        }
    }

    /// Look up the concrete actor metadata for a call, preferring the lowered
    /// program (which has monomorphized metadata for polymorphic actors) over
    /// the raw registry.
    fn lookup_actor(&self, actor_name: &str, call_span: Span) -> Option<&ActorMeta> {
        lookup_actor_in(self.lowered, self.registry, actor_name, call_span)
    }

    fn subgraph_index(&self, sub: &Subgraph) -> Option<&SubgraphIndex> {
        self.subgraph_indices.get(&subgraph_key(sub))
    }

    fn node_in_subgraph<'s>(&self, sub: &'s Subgraph, id: NodeId) -> Option<&'s Node> {
        self.subgraph_index(sub)
            .and_then(|idx| idx.node(sub, id))
            .or_else(|| find_node(sub, id))
    }

    fn first_incoming_edge_in_subgraph<'s>(
        &self,
        sub: &'s Subgraph,
        id: NodeId,
    ) -> Option<&'s Edge> {
        self.subgraph_index(sub)
            .and_then(|idx| idx.first_incoming_edge(sub, id))
            .or_else(|| sub.edges.iter().find(|e| e.target == id))
    }

    fn incoming_edge_count_in_subgraph(&self, sub: &Subgraph, id: NodeId) -> usize {
        self.subgraph_index(sub)
            .map(|idx| idx.incoming_count(id))
            .unwrap_or_else(|| sub.edges.iter().filter(|e| e.target == id).count())
    }

    fn outgoing_edge_count_in_subgraph(&self, sub: &Subgraph, id: NodeId) -> usize {
        self.subgraph_index(sub)
            .map(|idx| idx.outgoing_count(id))
            .unwrap_or_else(|| sub.edges.iter().filter(|e| e.source == id).count())
    }

    /// Format the C++ actor struct name, including template parameters for
    /// polymorphic actors (e.g., `Actor_scale<float>`).
    fn actor_cpp_name(&self, actor_name: &str, call_span: Span) -> String {
        if let Some(lowered) = self.lowered {
            if let Some(types) = lowered.type_instantiations.get(&call_span) {
                if !types.is_empty() {
                    let type_args: Vec<&str> =
                        types.iter().map(|t| pipit_type_to_cpp(*t)).collect();
                    return format!("Actor_{}<{}>", actor_name, type_args.join(", "));
                }
            }
        }
        format!("Actor_{}", actor_name)
    }

    fn build_result(self) -> CodegenResult {
        CodegenResult {
            generated: GeneratedCode {
                cpp_source: self.out,
            },
            diagnostics: self.diagnostics,
        }
    }

    // ── Top-level emit ──────────────────────────────────────────────────

    fn emit_all(&mut self) {
        self.emit_preamble();
        self.emit_const_storage();
        self.emit_param_storage();
        self.emit_shared_buffers();
        self.emit_stop_flag();
        self.emit_stats_storage();
        self.emit_task_functions();
        self.emit_main();
    }

    // ── Phase 1: Preamble ───────────────────────────────────────────────

    fn emit_preamble(&mut self) {
        self.out
            .push_str("// Generated by pcc (Pipit Compiler Collection)\n");
        self.out.push_str("#include <pipit.h>\n");
        self.out.push_str("#include <atomic>\n");
        self.out.push_str("#include <cerrno>\n");
        self.out.push_str("#include <cmath>\n");
        self.out.push_str("#include <chrono>\n");
        self.out.push_str("#include <csignal>\n");
        self.out.push_str("#include <cstdio>\n");
        self.out.push_str("#include <cstring>\n");
        self.out.push_str("#include <limits>\n");
        self.out.push_str("#include <string>\n");
        self.out.push_str("#include <thread>\n");
        self.out.push_str("#include <unordered_set>\n");
        self.out.push_str("#include <vector>\n");
        self.out.push('\n');

        // Actor headers are included via -include flags from the compiler driver,
        // but add forward declarations for external functions referenced in actors
        for path in &self.options.include_paths {
            let escaped = path
                .to_string_lossy()
                .replace('\\', "\\\\")
                .replace('"', "\\\"");
            let _ = writeln!(self.out, "#include \"{}\"", escaped);
        }
        if !self.options.include_paths.is_empty() {
            self.out.push('\n');
        }
    }

    // ── Phase 2: Const storage ──────────────────────────────────────────

    fn emit_const_storage(&mut self) {
        let mut has_any = false;
        for stmt in &self.program.statements {
            if let StatementKind::Const(c) = &stmt.kind {
                has_any = true;
                match &c.value {
                    Value::Scalar(scalar) => {
                        let _ = writeln!(
                            self.out,
                            "static constexpr auto _const_{} = {};",
                            c.name.name,
                            self.scalar_literal(scalar)
                        );
                    }
                    Value::Array(elems, _) => {
                        let elem_type = self.infer_array_elem_type(elems);
                        let values: Vec<String> =
                            elems.iter().map(|s| self.scalar_literal(s)).collect();
                        let _ = writeln!(
                            self.out,
                            "static constexpr {} _const_{}[] = {{{}}};",
                            elem_type,
                            c.name.name,
                            values.join(", ")
                        );
                    }
                }
            }
        }
        if has_any {
            self.out.push('\n');
        }
    }

    // ── Phase 3: Param storage ──────────────────────────────────────────

    /// Look up the C++ type for a runtime param by scanning the graph for actor
    /// nodes that reference `$param_name`, then finding the corresponding
    /// RUNTIME_PARAM type in the actor registry at the matching argument position.
    fn param_cpp_type(&self, param_name: &str, fallback: &Scalar) -> &'static str {
        // Search all task graphs for actor nodes that use ParamRef(param_name)
        for task_graph in self.graph.tasks.values() {
            for sub in subgraphs_of(task_graph) {
                for node in &sub.nodes {
                    if let NodeKind::Actor {
                        name,
                        call_span,
                        args,
                        ..
                    } = &node.kind
                    {
                        for (i, arg) in args.iter().enumerate() {
                            if let Arg::ParamRef(ident) = arg {
                                if ident.name == param_name {
                                    // Found the actor+position; look up type
                                    if let Some(meta) = self.lookup_actor(name, *call_span) {
                                        if let Some(p) = meta.params.get(i) {
                                            return match p.param_type {
                                                ParamType::Int => "int",
                                                ParamType::Float => "float",
                                                ParamType::Double => "double",
                                                _ => self.scalar_cpp_type(fallback),
                                            };
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        self.scalar_cpp_type(fallback)
    }

    fn emit_param_storage(&mut self) {
        let mut has_any = false;
        for stmt in &self.program.statements {
            if let StatementKind::Param(p) = &stmt.kind {
                has_any = true;
                let cpp_type = self.param_cpp_type(&p.name.name, &p.value);
                let init = self.scalar_literal(&p.value);
                let _ = writeln!(
                    self.out,
                    "static std::atomic<{}> _param_{}({});",
                    cpp_type, p.name.name, init
                );
            }
        }
        if has_any {
            self.out.push('\n');
        }
    }

    // ── Phase 4: Shared (inter-task) buffers ────────────────────────────

    fn emit_shared_buffers(&mut self) {
        if self.resolved.buffers.is_empty() {
            return;
        }

        for buf_name in self.resolved.buffers.keys() {
            let wire_type = self.infer_buffer_wire_type(buf_name);
            let cpp_type = pipit_type_to_cpp(wire_type);
            let capacity = self.inter_task_buffer_capacity(buf_name, wire_type);
            let reader_count = self.buffer_reader_tasks(buf_name).len().max(1);
            let _ = writeln!(
                self.out,
                "static pipit::RingBuffer<{}, {}, {}> _ringbuf_{};",
                cpp_type, capacity, reader_count, buf_name
            );
        }
        self.out.push('\n');
    }

    // ── Phase 5: Stop flag ──────────────────────────────────────────────

    fn emit_stop_flag(&mut self) {
        self.out
            .push_str("static std::atomic<bool> _stop{false};\n");
        self.out
            .push_str("static std::atomic<int> _exit_code{0};\n");
        self.out
            .push_str("static std::atomic<bool> _start{false};\n\n");
    }

    // ── Phase 5b: Statistics and probe storage ────────────────────────────

    fn emit_stats_storage(&mut self) {
        self.out.push_str("static bool _stats = false;\n");

        let mut task_names: Vec<&String> = self.schedule.tasks.keys().collect();
        task_names.sort();
        for name in &task_names {
            let _ = writeln!(self.out, "static pipit::TaskStats _stats_{};", name);
        }

        // Probe flags and output
        if !self.resolved.probes.is_empty() && !self.options.release {
            self.out
                .push_str("static FILE* _probe_output_file = nullptr;\n");
            for probe in &self.resolved.probes {
                let _ = writeln!(
                    self.out,
                    "static bool _probe_{}_enabled = false;",
                    probe.name
                );
            }
        }
        self.out.push('\n');
    }

    // ── Phase 6: Task functions ─────────────────────────────────────────

    fn emit_task_functions(&mut self) {
        for task_name in self.sorted_task_names() {
            let Some(task_graph) = self.graph.tasks.get(task_name.as_str()) else {
                continue;
            };
            self.emit_task_function(&task_name, task_graph);
        }
    }

    fn sorted_task_names(&self) -> Vec<String> {
        let mut task_names: Vec<String> = self.schedule.tasks.keys().cloned().collect();
        task_names.sort();
        task_names
    }

    fn emit_task_function(&mut self, task_name: &str, task_graph: &TaskGraph) {
        let Some(meta) = self.schedule.tasks.get(task_name) else {
            return;
        };
        let _ = writeln!(self.out, "void task_{}() {{", task_name);
        self.emit_task_prologue(task_name, meta, task_graph);
        self.out
            .push_str("    while (!_stop.load(std::memory_order_acquire)) {\n");
        self.out.push_str("        _timer.wait();\n");

        let policy = self.emit_task_overrun_policy(task_name);
        let tick_hoisted_actors = self.emit_tick_hoisted_actor_declarations(
            task_name,
            task_graph,
            &meta.schedule,
            "        ",
        );
        let indent =
            self.emit_task_iteration_setup(task_name, task_graph, meta.k_factor, &meta.schedule);
        self.emit_task_schedule_dispatch(
            task_name,
            task_graph,
            &meta.schedule,
            indent,
            &tick_hoisted_actors,
        );

        if meta.k_factor > 1 {
            self.out.push_str("        }\n");
        }
        if policy == "backlog" {
            self.out.push_str("        }\n");
        }
        self.out.push_str("    }\n");
        self.out.push_str("}\n\n");
    }

    fn emit_tick_hoisted_actor_declarations(
        &mut self,
        task_name: &str,
        task_graph: &TaskGraph,
        schedule: &TaskSchedule,
        indent: &str,
    ) -> HashMap<NodeId, String> {
        let mut hoisted = HashMap::new();
        match (task_graph, schedule) {
            (TaskGraph::Pipeline(sub), TaskSchedule::Pipeline(sched)) => {
                self.emit_tick_hoisted_for_subgraph(task_name, sub, sched, indent, &mut hoisted);
            }
            (
                TaskGraph::Modal { control, modes },
                TaskSchedule::Modal {
                    control: ctrl_sched,
                    modes: mode_scheds,
                },
            ) => {
                self.emit_tick_hoisted_for_subgraph(
                    task_name,
                    control,
                    ctrl_sched,
                    indent,
                    &mut hoisted,
                );
                for (mode_name, mode_sched) in mode_scheds {
                    let mode_sub = modes.iter().find(|(name, _)| name == mode_name);
                    if let Some((_, sub)) = mode_sub {
                        self.emit_tick_hoisted_for_subgraph(
                            task_name,
                            sub,
                            mode_sched,
                            indent,
                            &mut hoisted,
                        );
                    }
                }
            }
            _ => {}
        }
        hoisted
    }

    fn emit_tick_hoisted_for_subgraph(
        &mut self,
        task_name: &str,
        sub: &Subgraph,
        sched: &SubgraphSchedule,
        indent: &str,
        hoisted: &mut HashMap<NodeId, String>,
    ) {
        for entry in &sched.firings {
            if hoisted.contains_key(&entry.node_id) {
                continue;
            }
            let Some(node) = self.node_in_subgraph(sub, entry.node_id) else {
                continue;
            };
            let Some(var_name) = self
                .maybe_emit_hoisted_actor_declaration(task_name, sub, sched, node, indent, false)
            else {
                continue;
            };
            hoisted.insert(node.id, var_name);
        }
    }

    fn emit_task_prologue(&mut self, task_name: &str, meta: &TaskMeta, task_graph: &TaskGraph) {
        // Wait for all task threads to be created before starting timer.
        self.out.push_str(
            "    while (!_start.load(std::memory_order_acquire)) { std::this_thread::yield(); }\n",
        );

        // Timer (measure_latency enabled only when stats are active;
        // spin_ns from `set timer_spin`, default 10us; `auto` = adaptive).
        let spin_ns = if self.get_set_ident("timer_spin") == Some("auto") {
            -1_i64
        } else {
            self.get_set_number("timer_spin").unwrap_or(10_000.0) as i64
        };
        let _ = writeln!(
            self.out,
            "    pipit::Timer _timer({:.1}, _stats, {});",
            meta.freq_hz / meta.k_factor as f64,
            spin_ns
        );
        let _ = writeln!(
            self.out,
            "    pipit::detail::set_actor_task_rate_hz({:.1});",
            meta.freq_hz
        );
        self.out.push_str("    uint64_t _iter_idx = 0;\n");

        // Feedback back-edge buffers (persist across K-loop iterations).
        self.emit_feedback_buffers(task_name, task_graph, &meta.schedule);
        if matches!(&meta.schedule, TaskSchedule::Modal { .. }) {
            self.out.push_str("    int32_t _active_mode = -1;\n");
        }
    }

    fn emit_task_overrun_policy(&mut self, task_name: &str) -> String {
        let policy = self.get_overrun_policy().to_string();
        match policy.as_str() {
            "drop" => {
                let _ = writeln!(
                    self.out,
                    "        if (_timer.overrun()) {{ if (_stats) _stats_{}.record_miss(); continue; }}",
                    task_name
                );
            }
            "slip" => {
                self.out
                    .push_str("        if (_timer.overrun()) _timer.reset_phase();\n");
            }
            "backlog" => {
                self.out.push_str(
                    "        int _backlog = _timer.overrun() ? static_cast<int>(_timer.missed_count()) : 0;\n",
                );
                self.out
                    .push_str("        for (int _bl = 0; _bl <= _backlog; ++_bl) {\n");
            }
            _ => {}
        }
        let _ = writeln!(
            self.out,
            "        if (_stats) _stats_{}.record_tick(_timer.last_latency());",
            task_name
        );
        policy
    }

    /// Compute the iteration stride for a task: total samples produced per PASS cycle.
    ///
    /// For pipeline tasks, this is the first actor's `out_rate × repetition_count`.
    /// Falls back to 1 for modal tasks or when rates are unavailable.
    fn iteration_stride(&self, task_graph: &TaskGraph, schedule: &TaskSchedule) -> u32 {
        let (sub, sched) = match (task_graph, schedule) {
            (TaskGraph::Pipeline(sub), TaskSchedule::Pipeline(sched)) => (sub, sched),
            _ => return 1,
        };
        for firing in &sched.firings {
            let node = match find_node(sub, firing.node_id) {
                Some(n) => n,
                None => continue,
            };
            if let NodeKind::Actor { .. } = &node.kind {
                if let Some(rates) = self.analysis.node_port_rates.get(&node.id) {
                    if let Some(r) = rates.out_rate {
                        if r > 0 {
                            return r * firing.repetition_count;
                        }
                    }
                }
            }
        }
        1
    }

    fn emit_task_iteration_setup(
        &mut self,
        task_name: &str,
        task_graph: &TaskGraph,
        k_factor: u32,
        schedule: &TaskSchedule,
    ) -> &'static str {
        let stride = self.iteration_stride(task_graph, schedule);
        let iter_advance = if stride <= 1 {
            "_iter_idx++".to_string()
        } else {
            format!("_iter_idx += {}", stride)
        };
        if k_factor > 1 {
            let _ = writeln!(
                self.out,
                "        for (int _k = 0; _k < {}; ++_k) {{",
                k_factor
            );
            let _ = writeln!(
                self.out,
                "            pipit::detail::set_actor_iteration_index({});",
                iter_advance
            );
            self.emit_param_reads(task_name, task_graph, "            ");
            "            "
        } else {
            let _ = writeln!(
                self.out,
                "        pipit::detail::set_actor_iteration_index({});",
                iter_advance
            );
            self.emit_param_reads(task_name, task_graph, "        ");
            "        "
        }
    }

    fn emit_task_schedule_dispatch(
        &mut self,
        task_name: &str,
        task_graph: &TaskGraph,
        schedule: &TaskSchedule,
        indent: &str,
        tick_hoisted_actors: &HashMap<NodeId, String>,
    ) {
        match schedule {
            TaskSchedule::Pipeline(sched) => {
                let TaskGraph::Pipeline(sub) = task_graph else {
                    return;
                };
                self.emit_subgraph_firings(
                    task_name,
                    "pipeline",
                    sub,
                    sched,
                    indent,
                    tick_hoisted_actors,
                );
            }
            TaskSchedule::Modal { control, modes } => {
                let TaskGraph::Modal {
                    control: ctrl_sub,
                    modes: mode_subs,
                } = task_graph
                else {
                    return;
                };

                let _ = writeln!(self.out, "{}// Control subgraph", indent);
                self.emit_subgraph_firings(
                    task_name,
                    "control",
                    ctrl_sub,
                    control,
                    indent,
                    tick_hoisted_actors,
                );

                self.emit_ctrl_source_read(task_name, ctrl_sub, indent);
                let _ = writeln!(
                    self.out,
                    "{}if (_active_mode != -1 && _ctrl != _active_mode) {{",
                    indent
                );
                self.emit_mode_feedback_resets(
                    task_name,
                    mode_subs,
                    modes,
                    &format!("{}    ", indent),
                );
                let _ = writeln!(self.out, "{}}}", indent);
                let _ = writeln!(self.out, "{}_active_mode = _ctrl;", indent);

                let _ = writeln!(self.out, "{}switch (_ctrl) {{", indent);
                for (i, (mode_name, mode_sched)) in modes.iter().enumerate() {
                    let _ = writeln!(self.out, "{}case {}: {{", indent, i);
                    let mode_sub = mode_subs.iter().find(|(n, _)| n == mode_name);
                    if let Some((_, sub)) = mode_sub {
                        self.emit_subgraph_firings(
                            task_name,
                            mode_name,
                            sub,
                            mode_sched,
                            &format!("{}    ", indent),
                            tick_hoisted_actors,
                        );
                    }
                    let _ = writeln!(self.out, "{}    break;", indent);
                    let _ = writeln!(self.out, "{}}}", indent);
                }
                let _ = writeln!(self.out, "{}default: break;", indent);
                let _ = writeln!(self.out, "{}}}", indent);
            }
        }
    }

    // ── Subgraph firing sequence ────────────────────────────────────────

    fn emit_subgraph_firings(
        &mut self,
        task_name: &str,
        label: &str,
        sub: &Subgraph,
        sched: &SubgraphSchedule,
        indent: &str,
        tick_hoisted_actors: &HashMap<NodeId, String>,
    ) {
        // Declare intra-task edge buffers
        let edge_bufs = self.declare_edge_buffers(task_name, label, sub, sched, indent);

        let fusion_by_start = self.plan_fusion_candidates(task_name, sub, sched);
        let mut idx = 0usize;
        while idx < sched.firings.len() {
            if let Some(candidate) = fusion_by_start.get(&idx) {
                if self.emit_fused_actor_chain(
                    task_name,
                    sub,
                    sched,
                    candidate,
                    indent,
                    &edge_bufs,
                    tick_hoisted_actors,
                ) {
                    idx = candidate.end_idx + 1;
                    continue;
                }
            }

            let entry = &sched.firings[idx];
            let node = match self.node_in_subgraph(sub, entry.node_id) {
                Some(n) => n,
                None => {
                    idx += 1;
                    continue;
                }
            };
            self.emit_firing(
                task_name,
                label,
                sub,
                sched,
                node,
                entry,
                indent,
                &edge_bufs,
                tick_hoisted_actors,
            );
            idx += 1;
        }
    }

    fn plan_fusion_candidates(
        &self,
        task_name: &str,
        sub: &Subgraph,
        sched: &SubgraphSchedule,
    ) -> HashMap<usize, FusionCandidate> {
        let mut fused = HashMap::new();
        if sched.firings.len() < 2 {
            return fused;
        }
        let back_edges = self.identify_back_edges(task_name, sub);

        let mut i = 0usize;
        while i + 1 < sched.firings.len() {
            let start = i;
            let start_entry = &sched.firings[start];
            let rep = start_entry.repetition_count;
            if rep <= 1 || !self.is_fusion_entry_eligible(sub, start_entry, &back_edges) {
                i += 1;
                continue;
            }

            let mut end = i;
            let mut node_ids = vec![start_entry.node_id];
            let mut chain_node_ids: HashSet<NodeId> = HashSet::from([start_entry.node_id]);

            while end + 1 < sched.firings.len() {
                let next = &sched.firings[end + 1];
                if !self.can_append_to_fusion_chain(sub, rep, next, &chain_node_ids, &back_edges) {
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

    fn is_fusion_entry_eligible(
        &self,
        sub: &Subgraph,
        entry: &FiringEntry,
        back_edges: &HashSet<(NodeId, NodeId)>,
    ) -> bool {
        let Some(node) = self.node_in_subgraph(sub, entry.node_id) else {
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
                let in_deg = self.incoming_edge_count_in_subgraph(sub, entry.node_id);
                let out_deg = self.outgoing_edge_count_in_subgraph(sub, entry.node_id);
                in_deg <= 1 && out_deg == 1
            }
            NodeKind::Fork { .. } | NodeKind::Probe { .. } => true,
            _ => false,
        }
    }

    fn can_append_to_fusion_chain(
        &self,
        sub: &Subgraph,
        rep: u32,
        next: &FiringEntry,
        chain_node_ids: &HashSet<NodeId>,
        back_edges: &HashSet<(NodeId, NodeId)>,
    ) -> bool {
        if chain_node_ids.contains(&next.node_id) {
            return false;
        }
        let Some(next_node) = self.node_in_subgraph(sub, next.node_id) else {
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
        if !self.is_fusion_entry_eligible(sub, next, back_edges) {
            return false;
        }

        sub.edges
            .iter()
            .any(|e| e.target == next.node_id && chain_node_ids.contains(&e.source))
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_fused_actor_chain(
        &mut self,
        task_name: &str,
        sub: &Subgraph,
        sched: &SubgraphSchedule,
        candidate: &FusionCandidate,
        indent: &str,
        edge_bufs: &HashMap<(NodeId, NodeId), String>,
        tick_hoisted_actors: &HashMap<NodeId, String>,
    ) -> bool {
        if candidate.start_idx >= sched.firings.len()
            || candidate.end_idx >= sched.firings.len()
            || candidate.start_idx >= candidate.end_idx
        {
            return false;
        }

        if !candidate.node_ids.iter().all(|&nid| {
            matches!(
                self.node_in_subgraph(sub, nid).map(|n| &n.kind),
                Some(NodeKind::Actor { .. } | NodeKind::Fork { .. } | NodeKind::Probe { .. })
            )
        }) {
            return false;
        }

        let mut hoisted_vars: HashMap<NodeId, String> = HashMap::new();
        for &node_id in &candidate.node_ids {
            let Some(node) = self.node_in_subgraph(sub, node_id) else {
                return false;
            };
            if !matches!(node.kind, NodeKind::Actor { .. }) {
                continue;
            }
            if let Some(var_name) = tick_hoisted_actors.get(&node_id) {
                hoisted_vars.insert(node_id, var_name.clone());
                continue;
            }
            if let Some(var_name) =
                self.maybe_emit_hoisted_actor_declaration(task_name, sub, sched, node, indent, true)
            {
                hoisted_vars.insert(node_id, var_name);
            }
        }

        for &node_id in &candidate.node_ids {
            let Some(node) = self.node_in_subgraph(sub, node_id) else {
                return false;
            };
            if matches!(node.kind, NodeKind::Fork { .. }) {
                self.emit_fork(sub, sched, node, indent, edge_bufs);
            }
        }

        let _ = writeln!(
            self.out,
            "{}for (int _r = 0; _r < {}; ++_r) {{",
            indent, candidate.rep
        );
        let body_indent = format!("{}    ", indent);
        for &node_id in &candidate.node_ids {
            let Some(node) = self.node_in_subgraph(sub, node_id) else {
                return false;
            };
            match &node.kind {
                NodeKind::Actor { .. } => self.emit_actor_body_in_existing_r_loop(
                    task_name,
                    sub,
                    sched,
                    node,
                    body_indent.as_str(),
                    edge_bufs,
                    hoisted_vars.get(&node_id).map(String::as_str),
                ),
                NodeKind::Fork { .. } => {}
                NodeKind::Probe { probe_name } => self.emit_probe_in_existing_r_loop(
                    sub,
                    sched,
                    node,
                    candidate.rep,
                    probe_name,
                    body_indent.as_str(),
                    edge_bufs,
                ),
                _ => return false,
            }
        }
        let _ = writeln!(self.out, "{}}}", indent);
        true
    }

    fn declare_edge_buffers(
        &mut self,
        task_name: &str,
        _label: &str,
        sub: &Subgraph,
        sched: &SubgraphSchedule,
        indent: &str,
    ) -> HashMap<(NodeId, NodeId), String> {
        let mut names = HashMap::new();

        // Identify back-edges (feedback) — these are declared outside the K-loop
        let back_edges = self.identify_back_edges(task_name, sub);

        // Build alias map: Fork/Probe outgoing edges share the incoming edge's buffer.
        // Since scheduling is sequential within a task, no concurrent access occurs,
        // so downstream actors can safely read the upstream buffer directly.
        let aliases = self.build_passthrough_aliases(sub);

        // Pass 1: Declare real (non-aliased) buffers
        for (&(src, tgt), &tokens) in &sched.edge_buffers {
            if back_edges.contains(&(src, tgt)) {
                let var_name = format!("_fb_{}_{}", src.0, tgt.0);
                names.insert((src, tgt), var_name);
                continue;
            }

            // Skip aliased edges — they'll be resolved in pass 2
            if aliases.contains_key(&(src, tgt)) {
                continue;
            }

            let wire_type = self.infer_edge_wire_type(sub, src);
            let cpp_type = pipit_type_to_cpp(wire_type);
            let var_name = format!("_e{}_{}", src.0, tgt.0);
            let _ = writeln!(
                self.out,
                "{}static {} {}[{}];",
                indent, cpp_type, var_name, tokens
            );
            names.insert((src, tgt), var_name);
        }

        // Pass 2: Resolve aliases (now all source buffers are named)
        for (&(src, tgt), &(alias_src, alias_tgt)) in &aliases {
            if let Some(alias_name) = names.get(&(alias_src, alias_tgt)) {
                names.insert((src, tgt), alias_name.clone());
            }
        }

        names
    }

    /// For Fork and Probe nodes, map each outgoing edge to the node's incoming edge.
    /// This allows downstream actors to read from the upstream buffer directly,
    /// eliminating memcpy for passthrough nodes.
    fn build_passthrough_aliases(
        &self,
        sub: &Subgraph,
    ) -> HashMap<(NodeId, NodeId), (NodeId, NodeId)> {
        let mut aliases = HashMap::new();

        for node in &sub.nodes {
            let is_passthrough =
                matches!(node.kind, NodeKind::Fork { .. } | NodeKind::Probe { .. });
            if !is_passthrough {
                continue;
            }

            // Find the single incoming edge
            let incoming: Vec<&Edge> = sub.edges.iter().filter(|e| e.target == node.id).collect();
            if let Some(in_edge) = incoming.first() {
                let src_key = (in_edge.source, in_edge.target);
                // All outgoing edges alias to this incoming edge
                for out_edge in sub.edges.iter().filter(|e| e.source == node.id) {
                    aliases.insert((out_edge.source, out_edge.target), src_key);
                }
            }
        }

        // Resolve transitive aliases (e.g., Probe feeding into Fork)
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

    #[allow(clippy::too_many_arguments)]
    /// Check if an actor's constructor params are all loop-invariant.
    /// If true, the actor can be constructed once before the repetition loop.
    fn is_actor_hoistable(&self, args: &[Arg], allow_param_ref: bool) -> bool {
        args.iter().all(|arg| match arg {
            Arg::Value(_) | Arg::ConstRef(_) => true,
            Arg::ParamRef(_) => allow_param_ref,
            Arg::TapRef(_) => false,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_firing(
        &mut self,
        task_name: &str,
        label: &str,
        sub: &Subgraph,
        sched: &SubgraphSchedule,
        node: &Node,
        entry: &FiringEntry,
        indent: &str,
        edge_bufs: &HashMap<(NodeId, NodeId), String>,
        tick_hoisted_actors: &HashMap<NodeId, String>,
    ) {
        let plan = self.plan_firing(
            task_name,
            sub,
            sched,
            node,
            entry,
            indent,
            tick_hoisted_actors,
        );
        let ind = plan.body_indent.as_str();

        match &node.kind {
            NodeKind::Actor { .. } => self.emit_actor_body_in_existing_r_loop(
                task_name,
                sub,
                sched,
                node,
                ind,
                edge_bufs,
                plan.hoisted_actor_var.as_deref(),
            ),
            NodeKind::Fork { .. } => {
                self.emit_fork(sub, sched, node, ind, edge_bufs);
            }
            NodeKind::Probe { probe_name } => {
                self.emit_probe(sub, sched, node, probe_name, ind, edge_bufs);
            }
            NodeKind::BufferRead { buffer_name } => {
                self.emit_buffer_read(task_name, sub, sched, node, buffer_name, ind, edge_bufs);
            }
            NodeKind::BufferWrite { buffer_name } => {
                if !self.should_skip_shared_buffer_write(task_name, label, sub, buffer_name) {
                    self.emit_buffer_write(
                        task_name,
                        sub,
                        sched,
                        node,
                        buffer_name,
                        ind,
                        edge_bufs,
                    );
                }
            }
        }

        if plan.use_loop {
            let _ = writeln!(self.out, "{}}}", indent);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_actor_body_in_existing_r_loop(
        &mut self,
        task_name: &str,
        sub: &Subgraph,
        sched: &SubgraphSchedule,
        node: &Node,
        indent: &str,
        edge_bufs: &HashMap<(NodeId, NodeId), String>,
        hoisted_var: Option<&str>,
    ) {
        let NodeKind::Actor {
            name,
            call_span,
            args,
            shape_constraint,
        } = &node.kind
        else {
            return;
        };

        let effective_sc = shape_constraint
            .as_ref()
            .or_else(|| self.analysis.inferred_shapes.get(&node.id));
        self.emit_actor_firing(
            task_name,
            sub,
            sched,
            node,
            name,
            *call_span,
            args,
            effective_sc,
            indent,
            edge_bufs,
            hoisted_var,
        );
    }

    fn should_use_firing_loop(&self, node: &Node, rep: u32) -> bool {
        // Fork/Probe are zero-copy passthrough nodes; buffer I/O already performs
        // block transfers, so these nodes do not benefit from per-firing loops.
        let is_passthrough = matches!(node.kind, NodeKind::Fork { .. } | NodeKind::Probe { .. });
        let is_buffer_io = matches!(
            node.kind,
            NodeKind::BufferRead { .. } | NodeKind::BufferWrite { .. }
        );
        rep > 1 && !is_passthrough && !is_buffer_io
    }

    fn plan_firing(
        &mut self,
        task_name: &str,
        sub: &Subgraph,
        sched: &SubgraphSchedule,
        node: &Node,
        entry: &FiringEntry,
        indent: &str,
        tick_hoisted_actors: &HashMap<NodeId, String>,
    ) -> FiringPlan {
        let use_loop = self.should_use_firing_loop(node, entry.repetition_count);
        let hoisted_actor_var = if let Some(var_name) = tick_hoisted_actors.get(&node.id) {
            Some(var_name.clone())
        } else if use_loop {
            self.maybe_emit_hoisted_actor_declaration(task_name, sub, sched, node, indent, true)
        } else {
            None
        };

        let body_indent = if use_loop {
            let _ = writeln!(
                self.out,
                "{}for (int _r = 0; _r < {}; ++_r) {{",
                indent, entry.repetition_count
            );
            format!("{}    ", indent)
        } else {
            indent.to_string()
        };

        FiringPlan {
            use_loop,
            body_indent,
            hoisted_actor_var,
        }
    }

    fn maybe_emit_hoisted_actor_declaration(
        &mut self,
        task_name: &str,
        sub: &Subgraph,
        sched: &SubgraphSchedule,
        node: &Node,
        indent: &str,
        allow_param_ref: bool,
    ) -> Option<String> {
        let NodeKind::Actor {
            name,
            call_span,
            args,
            shape_constraint,
        } = &node.kind
        else {
            return None;
        };
        if !self.is_actor_hoistable(args, allow_param_ref) {
            return None;
        }

        let meta = lookup_actor_in(self.lowered, self.registry, name, *call_span)?;
        let effective_sc = shape_constraint
            .as_ref()
            .or_else(|| self.analysis.inferred_shapes.get(&node.id));
        let schedule_dim_overrides =
            self.build_schedule_dim_overrides(meta, args, effective_sc, sched, node.id, sub);
        let params = self.format_actor_params(
            task_name,
            meta,
            args,
            effective_sc,
            &schedule_dim_overrides,
            node.id,
        );
        let cpp_name = self.actor_cpp_name(name, *call_span);
        let var_name = format!("_actor_{}", node.id.0);
        if params.is_empty() {
            let _ = writeln!(self.out, "{}auto {} = {}{{}};", indent, var_name, cpp_name);
        } else {
            let _ = writeln!(
                self.out,
                "{}auto {} = {}{{{}}};",
                indent, var_name, cpp_name, params
            );
        }
        Some(var_name)
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_actor_firing(
        &mut self,
        task_name: &str,
        sub: &Subgraph,
        sched: &SubgraphSchedule,
        node: &Node,
        actor_name: &str,
        call_span: Span,
        args: &[Arg],
        shape_constraint: Option<&ShapeConstraint>,
        indent: &str,
        edge_bufs: &HashMap<(NodeId, NodeId), String>,
        hoisted_var: Option<&str>,
    ) {
        let meta = match lookup_actor_in(self.lowered, self.registry, actor_name, call_span) {
            Some(m) => m,
            None => return,
        };
        let plan = self.build_actor_call_plan(
            task_name,
            sub,
            sched,
            node,
            actor_name,
            call_span,
            args,
            shape_constraint,
            indent,
            edge_bufs,
            hoisted_var,
            meta,
        );
        let _io_plan = (&plan.in_ptr, &plan.out_ptr);

        let _ = writeln!(self.out, "{}if ({} != ACTOR_OK) {{", indent, plan.call_expr);
        let _ = writeln!(
            self.out,
            "{}    fprintf(stderr, \"runtime error: actor '{}' in task '{}' returned ACTOR_ERROR\\n\");",
            indent, actor_name, task_name
        );
        let _ = writeln!(
            self.out,
            "{}    _exit_code.store(1, std::memory_order_release);",
            indent
        );
        let _ = writeln!(
            self.out,
            "{}    _stop.store(true, std::memory_order_release);",
            indent
        );
        let _ = writeln!(self.out, "{}    return;", indent);
        let _ = writeln!(self.out, "{}}}", indent);
    }

    #[allow(clippy::too_many_arguments)]
    fn build_actor_call_plan(
        &mut self,
        task_name: &str,
        sub: &Subgraph,
        sched: &SubgraphSchedule,
        node: &Node,
        actor_name: &str,
        call_span: Span,
        args: &[Arg],
        shape_constraint: Option<&ShapeConstraint>,
        indent: &str,
        edge_bufs: &HashMap<(NodeId, NodeId), String>,
        hoisted_var: Option<&str>,
        meta: &ActorMeta,
    ) -> ActorCallPlan {
        let in_count = self.node_in_rate(node.id);
        let out_count = self.node_out_rate(node.id);
        let in_cpp = pipit_type_to_cpp(meta.in_type.as_concrete().unwrap_or(PipitType::Float));

        let incoming_edges: Vec<&Edge> = sub.edges.iter().filter(|e| e.target == node.id).collect();
        let in_ptr = self.build_actor_input_ptr(
            sched,
            node,
            meta,
            in_count,
            in_cpp,
            &incoming_edges,
            indent,
            edge_bufs,
        );

        let outgoing_edges: Vec<&Edge> = sub.edges.iter().filter(|e| e.source == node.id).collect();
        let out_ptr =
            self.build_actor_output_ptr(sched, node, meta, out_count, &outgoing_edges, edge_bufs);

        let schedule_dim_overrides =
            self.build_schedule_dim_overrides(meta, args, shape_constraint, sched, node.id, sub);
        let call_expr = if let Some(var_name) = hoisted_var {
            format!("{}.operator()({}, {})", var_name, in_ptr, out_ptr)
        } else {
            let params = self.format_actor_params(
                task_name,
                meta,
                args,
                shape_constraint,
                &schedule_dim_overrides,
                node.id,
            );
            let cpp_name = self.actor_cpp_name(actor_name, call_span);
            if params.is_empty() {
                format!("{}{{}}({}, {})", cpp_name, in_ptr, out_ptr)
            } else {
                format!(
                    "{}{{{}}}.operator()({}, {})",
                    cpp_name, params, in_ptr, out_ptr
                )
            }
        };

        ActorCallPlan {
            in_ptr,
            out_ptr,
            call_expr,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn build_actor_input_ptr(
        &mut self,
        sched: &SubgraphSchedule,
        node: &Node,
        meta: &ActorMeta,
        in_count: Option<u32>,
        in_cpp: &str,
        incoming_edges: &[&Edge],
        indent: &str,
        edge_bufs: &HashMap<(NodeId, NodeId), String>,
    ) -> String {
        if meta.in_type == PipitType::Void || incoming_edges.is_empty() {
            return "nullptr".to_string();
        }
        if incoming_edges.len() == 1 {
            let edge = incoming_edges[0];
            if let Some(buf_name) = edge_bufs.get(&(edge.source, edge.target)) {
                let rep = self.firing_repetition(sched, node.id);
                if rep > 1 {
                    let stride = in_count.unwrap_or_else(|| {
                        self.edge_buffer_tokens(sched, edge.source, edge.target) / rep
                    });
                    return format!("&{}[_r * {}]", buf_name, stride);
                }
                return buf_name.clone();
            }
            return "nullptr".to_string();
        }

        // Multi-input actor: concatenate each edge slice into a local buffer.
        let rep = self.firing_repetition(sched, node.id);
        let mut segments: Vec<(String, u32)> = Vec::with_capacity(incoming_edges.len());
        for edge in incoming_edges {
            let Some(buf_name) = edge_bufs.get(&(edge.source, edge.target)) else {
                continue;
            };
            let total_tokens = self.edge_buffer_tokens(sched, edge.source, edge.target);
            let tokens_per_firing = if rep > 1 {
                total_tokens / rep
            } else {
                total_tokens
            };
            let src_expr = if rep > 1 {
                format!("&{}[_r * {}]", buf_name, tokens_per_firing)
            } else {
                buf_name.clone()
            };
            segments.push((src_expr, tokens_per_firing));
        }
        if segments.is_empty() {
            return "nullptr".to_string();
        }

        let total_tokens: u32 = segments.iter().map(|(_, n)| *n).sum();
        let effective_in = in_count.unwrap_or(total_tokens);
        let local_in = format!("_in_{}", node.id.0);
        if total_tokens == effective_in {
            let _ = writeln!(
                self.out,
                "{}{} {}[{}];",
                indent, in_cpp, local_in, effective_in
            );
        } else {
            // Keep deterministic behavior when inferred IN size diverges from edge slices.
            let _ = writeln!(
                self.out,
                "{}{} {}[{}] = {{}};",
                indent, in_cpp, local_in, effective_in
            );
        }

        let mut offset = 0u32;
        for (src_expr, tokens) in segments {
            if offset >= effective_in {
                break;
            }
            let copy_tokens = tokens.min(effective_in - offset);
            self.emit_compact_input_copy(
                indent,
                &local_in,
                offset,
                src_expr.as_str(),
                copy_tokens,
                in_cpp,
            );
            offset += copy_tokens;
        }
        local_in
    }

    fn emit_compact_input_copy(
        &mut self,
        indent: &str,
        dst_buf: &str,
        dst_offset: u32,
        src_expr: &str,
        tokens: u32,
        cpp_type: &str,
    ) {
        match tokens {
            0 => {}
            1 => {
                let _ = writeln!(
                    self.out,
                    "{}{}[{}] = ({})[0];",
                    indent, dst_buf, dst_offset, src_expr
                );
            }
            2..=4 => {
                for i in 0..tokens {
                    let _ = writeln!(
                        self.out,
                        "{}{}[{}] = ({})[{}];",
                        indent,
                        dst_buf,
                        dst_offset + i,
                        src_expr,
                        i
                    );
                }
            }
            _ => {
                let _ = writeln!(
                    self.out,
                    "{}std::memcpy(&{}[{}], {}, {} * sizeof({}));",
                    indent, dst_buf, dst_offset, src_expr, tokens, cpp_type
                );
            }
        }
    }

    fn build_actor_output_ptr(
        &self,
        sched: &SubgraphSchedule,
        node: &Node,
        meta: &ActorMeta,
        out_count: Option<u32>,
        outgoing_edges: &[&Edge],
        edge_bufs: &HashMap<(NodeId, NodeId), String>,
    ) -> String {
        if meta.out_type == PipitType::Void || outgoing_edges.is_empty() {
            return "nullptr".to_string();
        }
        if outgoing_edges.len() == 1 {
            let edge = outgoing_edges[0];
            if let Some(buf_name) = edge_bufs.get(&(edge.source, edge.target)) {
                let rep = self.firing_repetition(sched, node.id);
                if rep > 1 {
                    let stride = out_count.unwrap_or_else(|| {
                        self.edge_buffer_tokens(sched, edge.source, edge.target) / rep
                    });
                    return format!("&{}[_r * {}]", buf_name, stride);
                }
                return buf_name.clone();
            }
            return "nullptr".to_string();
        }

        // Multiple outgoing edges: write to first, then copy to rest (handled by fork).
        if let Some(buf_name) = edge_bufs.get(&(outgoing_edges[0].source, outgoing_edges[0].target))
        {
            let rep = self.firing_repetition(sched, node.id);
            if rep > 1 {
                let stride = out_count.unwrap_or_else(|| {
                    self.edge_buffer_tokens(
                        sched,
                        outgoing_edges[0].source,
                        outgoing_edges[0].target,
                    ) / rep
                });
                return format!("&{}[_r * {}]", buf_name, stride);
            }
            return buf_name.clone();
        }
        "nullptr".to_string()
    }

    fn emit_fork(
        &mut self,
        _sub: &Subgraph,
        _sched: &SubgraphSchedule,
        node: &Node,
        indent: &str,
        _edge_bufs: &HashMap<(NodeId, NodeId), String>,
    ) {
        // Fork is a no-op: downstream actors share the upstream buffer directly
        // (aliased in declare_edge_buffers via build_passthrough_aliases)
        if let NodeKind::Fork { tap_name } = &node.kind {
            let _ = writeln!(self.out, "{}// fork: {} (zero-copy)", indent, tap_name);
        }
    }

    fn emit_probe(
        &mut self,
        sub: &Subgraph,
        sched: &SubgraphSchedule,
        node: &Node,
        probe_name: &str,
        indent: &str,
        edge_bufs: &HashMap<(NodeId, NodeId), String>,
    ) {
        // Probe is a no-op for data flow: downstream shares the upstream buffer.
        // Observation hook emits actual data when probe is enabled (stripped in release).
        if self.options.release {
            return;
        }
        let incoming: Vec<&Edge> = sub.edges.iter().filter(|e| e.target == node.id).collect();
        if let Some(in_edge) = incoming.first() {
            if let Some(src_buf) = edge_bufs.get(&(in_edge.source, in_edge.target)) {
                let wire_type = self.infer_edge_wire_type(sub, in_edge.source);
                let cpp_type = pipit_type_to_cpp(wire_type);
                let count = self.edge_buffer_tokens(sched, in_edge.source, in_edge.target);
                self.emit_probe_observation(probe_name, indent, src_buf.as_str(), count, cpp_type);
            }
        }
    }

    fn emit_probe_in_existing_r_loop(
        &mut self,
        sub: &Subgraph,
        sched: &SubgraphSchedule,
        node: &Node,
        fused_rep: u32,
        probe_name: &str,
        indent: &str,
        edge_bufs: &HashMap<(NodeId, NodeId), String>,
    ) {
        if self.options.release {
            return;
        }
        let incoming: Vec<&Edge> = sub.edges.iter().filter(|e| e.target == node.id).collect();
        if let Some(in_edge) = incoming.first() {
            if let Some(src_buf) = edge_bufs.get(&(in_edge.source, in_edge.target)) {
                let wire_type = self.infer_edge_wire_type(sub, in_edge.source);
                let cpp_type = pipit_type_to_cpp(wire_type);
                let total_tokens = self.edge_buffer_tokens(sched, in_edge.source, in_edge.target);
                let rep = fused_rep.max(1);
                let count = if rep > 1 {
                    total_tokens / rep
                } else {
                    total_tokens
                };
                if count == 0 {
                    return;
                }
                let src_expr = if rep > 1 {
                    format!("&{}[_r * {}]", src_buf, count)
                } else {
                    src_buf.clone()
                };
                self.emit_probe_observation(probe_name, indent, &src_expr, count, cpp_type);
            }
        }
    }

    fn emit_probe_observation(
        &mut self,
        probe_name: &str,
        indent: &str,
        src_expr: &str,
        count: u32,
        cpp_type: &str,
    ) {
        let fmt_spec = match cpp_type {
            "float" | "double" => "%f",
            "int32_t" | "int16_t" | "int8_t" => "%d",
            _ => "%f", // cfloat/cdouble: print real part
        };
        let _ = writeln!(self.out, "{}#ifndef NDEBUG", indent);
        let _ = writeln!(self.out, "{}if (_probe_{}_enabled) {{", indent, probe_name);
        let _ = writeln!(
            self.out,
            "{}    for (int _pi = 0; _pi < {}; ++_pi)",
            indent, count
        );
        let _ = writeln!(
            self.out,
            "{}        fprintf(_probe_output_file, \"[probe:{}] {}\\n\", ({})[_pi]);",
            indent, probe_name, fmt_spec, src_expr
        );
        let _ = writeln!(self.out, "{}    fflush(_probe_output_file);", indent);
        let _ = writeln!(self.out, "{}}}", indent);
        let _ = writeln!(self.out, "{}#endif", indent);
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_buffer_read(
        &mut self,
        task_name: &str,
        sub: &Subgraph,
        sched: &SubgraphSchedule,
        node: &Node,
        buffer_name: &str,
        indent: &str,
        edge_bufs: &HashMap<(NodeId, NodeId), String>,
    ) {
        // Read from inter-task ring buffer into outgoing edge buffer.
        // Uses block transfer (total_tokens at once) for throughput.
        let outgoing: Vec<&Edge> = sub.edges.iter().filter(|e| e.source == node.id).collect();

        if let Some(out_edge) = outgoing.first() {
            if let Some(dst_buf) = edge_bufs.get(&(out_edge.source, out_edge.target)) {
                let total_tokens = self.edge_buffer_tokens(sched, out_edge.source, out_edge.target);
                let reader_idx = self
                    .reader_index_for_task(buffer_name, task_name)
                    .unwrap_or(0);
                let _ = writeln!(
                    self.out,
                    "{}int _rb_retry_{}_{} = 0;",
                    indent, node.id.0, out_edge.target.0
                );
                let _ = writeln!(self.out, "{}while (true) {{", indent);
                let _ = writeln!(
                    self.out,
                    "{}    if (!_ringbuf_{}.read({}, {}, {})) {{",
                    indent, buffer_name, reader_idx, dst_buf, total_tokens
                );
                let _ = writeln!(
                    self.out,
                    "{}        if (_stop.load(std::memory_order_acquire)) {{",
                    indent
                );
                let _ = writeln!(self.out, "{}            return;", indent);
                let _ = writeln!(self.out, "{}        }}", indent);
                let _ = writeln!(
                    self.out,
                    "{}        if (++_rb_retry_{}_{} < 1000000) {{",
                    indent, node.id.0, out_edge.target.0
                );
                let _ = writeln!(self.out, "{}            std::this_thread::yield();", indent);
                let _ = writeln!(self.out, "{}            continue;", indent);
                let _ = writeln!(self.out, "{}        }}", indent);
                let _ = writeln!(
                    self.out,
                    "{}        std::fprintf(stderr, \"runtime error: task '{}' failed to read {} token(s) from shared buffer '{}'\\n\");",
                    indent, task_name, total_tokens, buffer_name
                );
                let _ = writeln!(
                    self.out,
                    "{}        _stop.store(true, std::memory_order_release);",
                    indent
                );
                let _ = writeln!(self.out, "{}        return;", indent);
                let _ = writeln!(self.out, "{}    }}", indent);
                let _ = writeln!(self.out, "{}    break;", indent);
                let _ = writeln!(self.out, "{}}}", indent);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_buffer_write(
        &mut self,
        task_name: &str,
        sub: &Subgraph,
        sched: &SubgraphSchedule,
        node: &Node,
        buffer_name: &str,
        indent: &str,
        edge_bufs: &HashMap<(NodeId, NodeId), String>,
    ) {
        // Write from incoming edge buffer to inter-task ring buffer.
        // Uses block transfer (total_tokens at once) for throughput.
        let incoming: Vec<&Edge> = sub.edges.iter().filter(|e| e.target == node.id).collect();

        if let Some(in_edge) = incoming.first() {
            if let Some(src_buf) = edge_bufs.get(&(in_edge.source, in_edge.target)) {
                let total_tokens = self.edge_buffer_tokens(sched, in_edge.source, in_edge.target);
                let _ = writeln!(
                    self.out,
                    "{}int _rb_retry_{}_{} = 0;",
                    indent, in_edge.source.0, node.id.0
                );
                let _ = writeln!(self.out, "{}while (true) {{", indent);
                let _ = writeln!(
                    self.out,
                    "{}    if (!_ringbuf_{}.write({}, {})) {{",
                    indent, buffer_name, src_buf, total_tokens
                );
                let _ = writeln!(
                    self.out,
                    "{}        if (_stop.load(std::memory_order_acquire)) {{",
                    indent
                );
                let _ = writeln!(self.out, "{}            return;", indent);
                let _ = writeln!(self.out, "{}        }}", indent);
                let _ = writeln!(
                    self.out,
                    "{}        if (++_rb_retry_{}_{} < 1000000) {{",
                    indent, in_edge.source.0, node.id.0
                );
                let _ = writeln!(self.out, "{}            std::this_thread::yield();", indent);
                let _ = writeln!(self.out, "{}            continue;", indent);
                let _ = writeln!(self.out, "{}        }}", indent);
                let _ = writeln!(
                    self.out,
                    "{}        std::fprintf(stderr, \"runtime error: task '{}' failed to write {} token(s) to shared buffer '{}'\\n\");",
                    indent, task_name, total_tokens, buffer_name
                );
                let _ = writeln!(
                    self.out,
                    "{}        _stop.store(true, std::memory_order_release);",
                    indent
                );
                let _ = writeln!(self.out, "{}        return;", indent);
                let _ = writeln!(self.out, "{}    }}", indent);
                let _ = writeln!(self.out, "{}    break;", indent);
                let _ = writeln!(self.out, "{}}}", indent);
            }
        }
    }

    // ── Feedback buffer handling ────────────────────────────────────────

    fn emit_feedback_buffers(
        &mut self,
        task_name: &str,
        task_graph: &TaskGraph,
        task_schedule: &TaskSchedule,
    ) {
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

        for (sub, sched) in subs_and_scheds {
            let back_edges = self.identify_back_edges(task_name, sub);
            for (src, tgt) in &back_edges {
                let tokens = sched.edge_buffers.get(&(*src, *tgt)).copied().unwrap_or(1);
                let wire_type = self.infer_edge_wire_type(sub, *src);
                let cpp_type = pipit_type_to_cpp(wire_type);

                // Get initial value from delay actor
                let init_val = self.delay_init_value(sub, *src);
                let var_name = format!("_fb_{}_{}", src.0, tgt.0);

                // Always declare as array for consistent pointer semantics
                let _ = writeln!(
                    self.out,
                    "    {} {}[{}] = {{{}}};",
                    cpp_type, var_name, tokens, init_val
                );
            }
        }
    }

    fn identify_back_edges(&self, _task_name: &str, sub: &Subgraph) -> HashSet<(NodeId, NodeId)> {
        let mut back_edges = HashSet::new();
        let node_ids: HashSet<u32> = sub.nodes.iter().map(|n| n.id.0).collect();

        for cycle in &self.graph.cycles {
            if !cycle.iter().all(|id| node_ids.contains(&id.0)) {
                continue;
            }
            for (i, &nid) in cycle.iter().enumerate() {
                if let Some(node) = self.node_in_subgraph(sub, nid) {
                    if matches!(&node.kind, NodeKind::Actor { name, .. } if name == "delay") {
                        let next_nid = cycle[(i + 1) % cycle.len()];
                        if sub
                            .edges
                            .iter()
                            .any(|e| e.source == nid && e.target == next_nid)
                        {
                            back_edges.insert((nid, next_nid));
                        }
                        break;
                    }
                }
            }
        }
        back_edges
    }

    fn delay_init_value(&self, sub: &Subgraph, node_id: NodeId) -> String {
        if let Some(node) = self.node_in_subgraph(sub, node_id) {
            if let NodeKind::Actor { args, name, .. } = &node.kind {
                if name == "delay" {
                    // Second arg is init value
                    if let Some(arg) = args.get(1) {
                        return self.arg_to_cpp_literal(arg);
                    }
                }
            }
        }
        "0".to_string()
    }

    // ── Runtime param reads ─────────────────────────────────────────────

    fn emit_param_reads(&mut self, task_name: &str, task_graph: &TaskGraph, indent: &str) {
        let mut used_params: HashSet<String> = HashSet::new();
        self.collect_used_params(task_name, task_graph, &mut used_params);

        for param_name in &used_params {
            if let Some(entry) = self.resolved.params.get(param_name) {
                let stmt = &self.program.statements[entry.stmt_index];
                if let StatementKind::Param(p) = &stmt.kind {
                    let cpp_type = self.param_cpp_type(param_name, &p.value);
                    let _ = writeln!(
                        self.out,
                        "{}{} _param_{}_val = _param_{}.load(std::memory_order_acquire);",
                        indent, cpp_type, param_name, param_name
                    );
                }
            }
        }
    }

    fn collect_used_params(
        &self,
        task_name: &str,
        task_graph: &TaskGraph,
        params: &mut HashSet<String>,
    ) {
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
        // switch($param, ...) is also a runtime-param use even if no actor consumes it.
        if let Some(task) = self.find_task(task_name) {
            if let TaskBody::Modal(modal) = &task.body {
                if let SwitchSource::Param(ident) = &modal.switch.source {
                    params.insert(ident.name.clone());
                }
            }
        }
    }

    // ── Phase 7: main() ─────────────────────────────────────────────────

    fn emit_main(&mut self) {
        self.out.push_str("int main(int argc, char* argv[]) {\n");
        self.out
            .push_str("    double _duration_seconds = std::numeric_limits<double>::infinity();\n");
        self.out.push_str("    int _threads = 0;\n");
        self.out
            .push_str("    std::string _probe_output = \"/dev/stderr\";\n");
        self.out
            .push_str("    std::vector<std::string> _enabled_probes;\n");
        self.out.push('\n');

        // CLI argument parsing for runtime options.
        self.emit_runtime_cli_parsing();
        self.out.push('\n');

        // Probe initialization (validate and wire CLI args)
        self.emit_probe_initialization();

        // Signal handler
        self.out.push_str(
            "    std::signal(SIGINT, [](int) { _stop.store(true, std::memory_order_release); });\n",
        );
        self.out.push('\n');

        // Launch task threads
        let mut task_names: Vec<&String> = self.schedule.tasks.keys().collect();
        task_names.sort();
        for (i, name) in task_names.iter().enumerate() {
            let _ = writeln!(self.out, "    std::thread _t{}(task_{});", i, name);
        }
        // Release all task threads (synchronized timer start)
        self.out
            .push_str("    _start.store(true, std::memory_order_release);\n");
        self.out.push('\n');

        // Wait for stop signal (duration or SIGINT)
        self.emit_duration_wait();

        // Join all threads
        for i in 0..task_names.len() {
            let _ = writeln!(self.out, "    _t{}.join();", i);
        }
        self.out.push('\n');

        if task_names.len() > 1 {
            let _ = writeln!(
                self.out,
                "    if (_threads > 0 && _threads < {}) {{",
                task_names.len()
            );
            let _ = writeln!(
                self.out,
                "        std::fprintf(stderr, \"startup warning: --threads is advisory (requested=%d, tasks={})\\\\n\", _threads);",
                task_names.len()
            );
            self.out.push_str("    }\n");
            self.out.push('\n');
        }

        // Stats output
        self.emit_stats_output();

        self.out
            .push_str("    return _exit_code.load(std::memory_order_acquire);\n");
        self.out.push_str("}\n");
    }

    fn emit_stats_output(&mut self) {
        self.out.push_str("    if (_stats) {\n");

        let mut task_names: Vec<&String> = self.schedule.tasks.keys().collect();
        task_names.sort();
        let policy = self.get_overrun_policy().to_string();

        for name in &task_names {
            let _ = writeln!(
                self.out,
                "        fprintf(stderr, \"[stats] task '{}': ticks=%lu, missed=%lu ({}), max_latency=%ldns, avg_latency=%ldns\\n\",\n\
                             (unsigned long)_stats_{}.ticks, (unsigned long)_stats_{}.missed,\n\
                             _stats_{}.max_latency_ns, _stats_{}.avg_latency_ns());",
                name, policy, name, name, name, name
            );
        }

        // Shared buffer stats
        for buf_name in self.resolved.buffers.keys() {
            let wire_type = self.infer_buffer_wire_type(buf_name);
            let cpp_type = pipit_type_to_cpp(wire_type);
            let _ = writeln!(
                self.out,
                "        fprintf(stderr, \"[stats] shared buffer '{}': %zu tokens (%zuB)\\n\",\n\
                             (size_t)_ringbuf_{}.available(), _ringbuf_{}.available() * sizeof({}));",
                buf_name, buf_name, buf_name, cpp_type
            );
        }

        let mem_limit = self.get_set_size("mem").unwrap_or(64 * 1024 * 1024);
        let _ = writeln!(
            self.out,
            "        fprintf(stderr, \"[stats] memory pool: %zuB allocated, %zuB used\\n\", (size_t){}, (size_t){});",
            mem_limit, self.analysis.total_memory
        );

        self.out.push_str("    }\n");
    }

    fn emit_runtime_cli_parsing(&mut self) {
        self.out.push_str(
            "    auto _parse_duration = [](const std::string& s, double* out) -> bool {\n",
        );
        self.out
            .push_str("        if (s == \"inf\") { *out = std::numeric_limits<double>::infinity(); return true; }\n");
        self.out.push_str("        std::size_t pos = 0;\n");
        self.out.push_str("        double base = 0.0;\n");
        self.out.push_str("        try {\n");
        self.out
            .push_str("            base = std::stod(s, &pos);\n");
        self.out.push_str("        } catch (...) {\n");
        self.out.push_str("            return false;\n");
        self.out.push_str("        }\n");
        self.out
            .push_str("        std::string unit = s.substr(pos);\n");
        self.out
            .push_str("        if (unit.empty() || unit == \"s\") { *out = base; return true; }\n");
        self.out
            .push_str("        if (unit == \"m\") { *out = base * 60.0; return true; }\n");
        self.out.push_str("        return false;\n");
        self.out.push_str("    };\n\n");

        self.out.push_str("    for (int i = 1; i < argc; ++i) {\n");
        self.out.push_str("        std::string opt(argv[i]);\n");
        self.out.push_str("        if (opt == \"--param\") {\n");
        self.out.push_str("            if (i + 1 >= argc) {\n");
        self.out
            .push_str("                std::fprintf(stderr, \"startup error: --param requires name=value\\\\n\");\n");
        self.out.push_str("                return 2;\n");
        self.out.push_str("            }\n");
        self.out
            .push_str("            std::string arg(argv[++i]);\n");
        self.out.push_str("            auto eq = arg.find('=');\n");
        self.out
            .push_str("            if (eq == std::string::npos) {\n");
        self.out
            .push_str("                std::fprintf(stderr, \"startup error: --param requires name=value\\\\n\");\n");
        self.out.push_str("                return 2;\n");
        self.out.push_str("            }\n");
        self.out
            .push_str("            auto name = arg.substr(0, eq);\n");
        self.out
            .push_str("            auto val = arg.substr(eq + 1);\n");

        let mut first = true;
        for (param_name, entry) in &self.resolved.params {
            let keyword = if first { "if" } else { "else if" };
            first = false;
            let stmt = &self.program.statements[entry.stmt_index];
            let converter = if let StatementKind::Param(p) = &stmt.kind {
                match self.param_cpp_type(param_name, &p.value) {
                    "int" => "std::stoi",
                    "double" => "std::stod",
                    _ => "std::stof",
                }
            } else {
                "std::stof"
            };
            let _ = writeln!(
                self.out,
                "            {} (name == \"{}\") _param_{}.store({}(val), std::memory_order_release);",
                keyword, param_name, param_name, converter
            );
        }

        if self.resolved.params.is_empty() {
            self.out.push_str(
                "            std::fprintf(stderr, \"startup error: --param is unsupported (no runtime params)\\\\n\");\n",
            );
            self.out.push_str("            return 2;\n");
        } else {
            self.out.push_str("            else {\n");
            self.out
                .push_str("                std::fprintf(stderr, \"startup error: unknown param '%s'\\\\n\", name.c_str());\n");
            self.out.push_str("                return 2;\n");
            self.out.push_str("            }\n");
        }

        self.out.push_str("            continue;\n");
        self.out.push_str("        }\n");

        self.out.push_str("        if (opt == \"--duration\") {\n");
        self.out.push_str("            if (i + 1 >= argc) {\n");
        self.out
            .push_str("                std::fprintf(stderr, \"startup error: --duration requires a value\\\\n\");\n");
        self.out.push_str("                return 2;\n");
        self.out.push_str("            }\n");
        self.out.push_str("            std::string d(argv[++i]);\n");
        self.out
            .push_str("            if (!_parse_duration(d, &_duration_seconds)) {\n");
        self.out
            .push_str("                std::fprintf(stderr, \"startup error: invalid --duration '%s' (use <sec>, <sec>s, <min>m, or inf)\\\\n\", d.c_str());\n");
        self.out.push_str("                return 2;\n");
        self.out.push_str("            }\n");
        self.out.push_str("            continue;\n");
        self.out.push_str("        }\n");

        self.out.push_str("        if (opt == \"--threads\") {\n");
        self.out.push_str("            if (i + 1 >= argc) {\n");
        self.out
            .push_str("                std::fprintf(stderr, \"startup error: --threads requires a positive integer\\\\n\");\n");
        self.out.push_str("                return 2;\n");
        self.out.push_str("            }\n");
        self.out.push_str("            try {\n");
        self.out
            .push_str("                _threads = std::stoi(std::string(argv[++i]));\n");
        self.out.push_str("            } catch (...) {\n");
        self.out
            .push_str("                std::fprintf(stderr, \"startup error: --threads requires a positive integer\\\\n\");\n");
        self.out.push_str("                return 2;\n");
        self.out.push_str("            }\n");
        self.out.push_str("            if (_threads <= 0) {\n");
        self.out
            .push_str("                std::fprintf(stderr, \"startup error: --threads requires a positive integer\\\\n\");\n");
        self.out.push_str("                return 2;\n");
        self.out.push_str("            }\n");
        self.out.push_str("            continue;\n");
        self.out.push_str("        }\n");

        self.out.push_str("        if (opt == \"--probe\") {\n");
        self.out.push_str("            if (i + 1 >= argc) {\n");
        self.out
            .push_str("                std::fprintf(stderr, \"startup error: --probe requires a name\\\\n\");\n");
        self.out.push_str("                return 2;\n");
        self.out.push_str("            }\n");
        self.out
            .push_str("            _enabled_probes.emplace_back(argv[++i]);\n");
        self.out.push_str("            continue;\n");
        self.out.push_str("        }\n");

        self.out
            .push_str("        if (opt == \"--probe-output\") {\n");
        self.out.push_str("            if (i + 1 >= argc) {\n");
        self.out
            .push_str("                std::fprintf(stderr, \"startup error: --probe-output requires a path\\\\n\");\n");
        self.out.push_str("                return 2;\n");
        self.out.push_str("            }\n");
        self.out
            .push_str("            _probe_output = std::string(argv[++i]);\n");
        self.out.push_str("            continue;\n");
        self.out.push_str("        }\n");

        self.out.push_str("        if (opt == \"--stats\") {\n");
        self.out.push_str("            _stats = true;\n");
        self.out.push_str("            continue;\n");
        self.out.push_str("        }\n");

        self.out.push_str(
            "        std::fprintf(stderr, \"startup error: unknown option '%s'\\\\n\", argv[i]);\n",
        );
        self.out.push_str("        return 2;\n");
        self.out.push_str("    }\n");
    }

    fn emit_probe_initialization(&mut self) {
        // Only emit probe initialization in debug builds
        if self.options.release || self.resolved.probes.is_empty() {
            return;
        }

        self.out.push_str("    // Probe initialization\n");
        self.out.push_str("    #ifndef NDEBUG\n");
        self.out
            .push_str("    if (!_enabled_probes.empty() || _probe_output != \"/dev/stderr\") {\n");

        // Build valid probe name set
        self.out
            .push_str("        std::unordered_set<std::string> _valid_probes = {");
        let mut first = true;
        for probe in &self.resolved.probes {
            if !first {
                self.out.push_str(", ");
            }
            first = false;
            let _ = write!(self.out, "\"{}\"", probe.name);
        }
        self.out.push_str("};\n");

        // Validate and enable probes
        self.out
            .push_str("        for (const auto& name : _enabled_probes) {\n");
        self.out
            .push_str("            if (_valid_probes.find(name) == _valid_probes.end()) {\n");
        self.out.push_str("                std::fprintf(stderr, \"startup error: unknown probe '%s'\\\\n\", name.c_str());\n");
        self.out.push_str("                return 2;\n");
        self.out.push_str("            }\n");

        // Enable specific probe flags
        for probe in &self.resolved.probes {
            let _ = writeln!(
                self.out,
                "            if (name == \"{}\") _probe_{}_enabled = true;",
                probe.name, probe.name
            );
        }

        self.out.push_str("        }\n");

        // Open probe output path
        self.out
            .push_str("        _probe_output_file = std::fopen(_probe_output.c_str(), \"w\");\n");
        self.out.push_str("        if (!_probe_output_file) {\n");
        self.out.push_str("                std::fprintf(stderr, \"startup error: failed to open probe output file '%s': %s\\\\n\",\n");
        self.out.push_str(
            "                            _probe_output.c_str(), std::strerror(errno));\n",
        );
        self.out.push_str("                return 2;\n");
        self.out.push_str("        }\n");

        self.out.push_str("    }\n");
        self.out.push_str("    #endif\n\n");
    }

    fn emit_duration_wait(&mut self) {
        self.out
            .push_str("    if (std::isfinite(_duration_seconds)) {\n");
        self.out.push_str(
            "        std::this_thread::sleep_for(std::chrono::duration<double>(_duration_seconds));\n",
        );
        self.out
            .push_str("        _stop.store(true, std::memory_order_release);\n");
        self.out.push_str("    } else {\n");
        self.out.push_str("        // Run until SIGINT\n");
        self.out
            .push_str("        while (!_stop.load(std::memory_order_acquire))\n");
        self.out
            .push_str("            std::this_thread::sleep_for(std::chrono::milliseconds(100));\n");
        self.out.push_str("    }\n\n");
    }

    // ── Helpers ─────────────────────────────────────────────────────────

    fn find_task(&self, task_name: &str) -> Option<&TaskStmt> {
        for stmt in &self.program.statements {
            if let StatementKind::Task(task) = &stmt.kind {
                if task.name.name == task_name {
                    return Some(task);
                }
            }
        }
        None
    }

    /// Skip shared-buffer writes when the buffer has no readers.
    fn should_skip_shared_buffer_write(
        &self,
        _task_name: &str,
        _label: &str,
        _sub: &Subgraph,
        buffer_name: &str,
    ) -> bool {
        self.resolved
            .buffers
            .get(buffer_name)
            .map(|info| info.readers.is_empty())
            .unwrap_or(false)
    }

    fn emit_ctrl_source_read(&mut self, task_name: &str, ctrl_sub: &Subgraph, indent: &str) {
        let switch_source = self.find_task(task_name).and_then(|task| {
            let TaskBody::Modal(modal) = &task.body else {
                return None;
            };
            Some(modal.switch.source.clone())
        });
        let Some(switch_source) = switch_source else {
            let ctrl_var = self.find_ctrl_output(ctrl_sub);
            let _ = writeln!(self.out, "{}int32_t _ctrl = {};", indent, ctrl_var);
            return;
        };

        match switch_source {
            SwitchSource::Param(ident) => {
                let _ = writeln!(
                    self.out,
                    "{}int32_t _ctrl = static_cast<int32_t>(_param_{}_val);",
                    indent, ident.name
                );
            }
            SwitchSource::Buffer(ident) => {
                if let Some(ctrl_var) = self.find_ctrl_output_for_buffer(ctrl_sub, &ident.name) {
                    let _ = writeln!(self.out, "{}int32_t _ctrl = {};", indent, ctrl_var);
                } else {
                    let reader_idx = self
                        .reader_index_for_task(&ident.name, task_name)
                        .unwrap_or(0);
                    let _ = writeln!(self.out, "{}int32_t _ctrl_buf[1];", indent);
                    let _ = writeln!(
                        self.out,
                        "{}if (!_ringbuf_{}.read({}, _ctrl_buf, 1)) {{",
                        indent, ident.name, reader_idx
                    );
                    let _ = writeln!(
                        self.out,
                        "{}    std::fprintf(stderr, \"runtime error: task '{}' failed to read 1 token(s) from shared buffer '{}' for switch ctrl\\n\");",
                        indent, task_name, ident.name
                    );
                    let _ = writeln!(
                        self.out,
                        "{}    _stop.store(true, std::memory_order_release);",
                        indent
                    );
                    let _ = writeln!(self.out, "{}    return;", indent);
                    let _ = writeln!(self.out, "{}}}", indent);
                    let _ = writeln!(self.out, "{}int32_t _ctrl = _ctrl_buf[0];", indent);
                }
            }
        }
    }

    fn emit_mode_feedback_resets(
        &mut self,
        task_name: &str,
        mode_subs: &[(String, Subgraph)],
        mode_scheds: &[(String, SubgraphSchedule)],
        indent: &str,
    ) {
        let mut reset_points: Vec<(String, u32, String)> = Vec::new();

        for (mode_name, sched) in mode_scheds {
            let Some((_, sub)) = mode_subs.iter().find(|(n, _)| n == mode_name) else {
                continue;
            };
            let mut back_edges: Vec<(NodeId, NodeId)> = self
                .identify_back_edges(task_name, sub)
                .into_iter()
                .collect();
            back_edges.sort_by_key(|(src, tgt)| (src.0, tgt.0));

            for (src, tgt) in back_edges {
                let tokens = sched.edge_buffers.get(&(src, tgt)).copied().unwrap_or(1);
                let init_val = self.delay_init_value(sub, src);
                let var_name = format!("_fb_{}_{}", src.0, tgt.0);
                reset_points.push((var_name, tokens, init_val));
            }
        }

        for (var_name, tokens, init_val) in reset_points {
            if tokens <= 1 {
                let _ = writeln!(self.out, "{}{}[0] = {};", indent, var_name, init_val);
            } else {
                let _ = writeln!(
                    self.out,
                    "{}for (int _fb_i = 0; _fb_i < {}; ++_fb_i) {}[_fb_i] = {};",
                    indent, tokens, var_name, init_val
                );
            }
        }
    }

    fn get_set_ident<'b>(&'b self, name: &str) -> Option<&'b str> {
        for stmt in &self.program.statements {
            if let StatementKind::Set(set) = &stmt.kind {
                if set.name.name == name {
                    if let SetValue::Ident(ident) = &set.value {
                        return Some(&ident.name);
                    }
                }
            }
        }
        None
    }

    fn get_set_number(&self, name: &str) -> Option<f64> {
        for stmt in &self.program.statements {
            if let StatementKind::Set(set) = &stmt.kind {
                if set.name.name == name {
                    if let SetValue::Number(n, _) = &set.value {
                        return Some(*n);
                    }
                }
            }
        }
        None
    }

    fn get_set_size(&self, name: &str) -> Option<u64> {
        for stmt in &self.program.statements {
            if let StatementKind::Set(set) = &stmt.kind {
                if set.name.name == name {
                    if let SetValue::Size(v, _) = &set.value {
                        return Some(*v);
                    }
                }
            }
        }
        None
    }

    fn get_overrun_policy(&self) -> &str {
        match self.get_set_ident("overrun") {
            Some("drop") | Some("slip") | Some("backlog") => self.get_set_ident("overrun").unwrap(),
            _ => "drop",
        }
    }

    fn find_ctrl_output(&self, ctrl_sub: &Subgraph) -> String {
        // Find the last BufferWrite in the control subgraph; the data feeding it
        // is the ctrl value. We'll use the incoming edge buffer.
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

    fn scalar_literal(&self, scalar: &Scalar) -> String {
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

    fn scalar_cpp_type(&self, scalar: &Scalar) -> &'static str {
        match scalar {
            Scalar::Number(n, _, _) => {
                if *n == (*n as i64) as f64 && !n.is_nan() && !n.is_infinite() {
                    if *n >= i32::MIN as f64 && *n <= i32::MAX as f64 {
                        "int"
                    } else {
                        "double"
                    }
                } else {
                    "float"
                }
            }
            Scalar::Freq(_, _) => "double",
            Scalar::Size(_, _) => "size_t",
            Scalar::StringLit(_, _) => "const char*",
            Scalar::Ident(_) => "auto",
        }
    }

    fn infer_array_elem_type(&self, _elems: &[Scalar]) -> &'static str {
        // Const arrays in Pipit are coefficient arrays — always emit as float.
        // Integer-valued elements like [1.0, -1.0] are still float coefficients.
        "float"
    }

    fn format_actor_params(
        &self,
        _task_name: &str,
        meta: &ActorMeta,
        args: &[Arg],
        shape_constraint: Option<&ShapeConstraint>,
        schedule_dim_overrides: &HashMap<String, u32>,
        node_id: NodeId,
    ) -> String {
        let mut parts = Vec::new();
        // Track const array args used for count params so they can auto-fill span params.
        let mut last_array_const: Option<&Arg> = None;
        for (i, param) in meta.params.iter().enumerate() {
            if let Some(arg) = args.get(i) {
                match param.kind {
                    ParamKind::RuntimeParam => {
                        // Use the local cached value
                        if let Arg::ParamRef(ident) = arg {
                            parts.push(format!("_param_{}_val", ident.name));
                        } else {
                            parts.push(self.arg_to_cpp_literal(arg));
                        }
                        last_array_const = None;
                    }
                    ParamKind::Param => {
                        // If arg is a const array ref and param is int, remember it
                        if self.is_const_array_ref(arg) && param.param_type == ParamType::Int {
                            last_array_const = Some(arg);
                        } else {
                            last_array_const = None;
                        }
                        parts.push(self.arg_to_cpp_value(arg, &param.param_type));
                    }
                }
            } else if param.kind == ParamKind::Param {
                // No arg at this index — try shape/span/analysis/schedule-based inference.
                if let Some(val) = self.resolve_missing_param_value(
                    &param.name,
                    meta,
                    args,
                    shape_constraint,
                    schedule_dim_overrides,
                    node_id,
                ) {
                    parts.push(val.to_string());
                    continue;
                }
                self.try_autofill_span_param(&mut parts, &mut last_array_const, &param.param_type);
            } else {
                self.try_autofill_span_param(&mut parts, &mut last_array_const, &param.param_type);
            }
        }
        parts.join(", ")
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
            .or_else(|| self.infer_dim_param_from_span_args(param_name, meta, args))
            .or_else(|| {
                self.analysis
                    .span_derived_dims
                    .get(&(node_id, param_name.to_string()))
                    .copied()
            })
            .or_else(|| schedule_dim_overrides.get(param_name).copied())
    }

    fn try_autofill_span_param(
        &self,
        parts: &mut Vec<String>,
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
        parts.push(self.arg_to_cpp_value(array_arg, param_type));
        *last_array_const = None;
    }

    /// Resolve a dimension parameter from shape constraints.
    /// Returns the inferred value if the param name appears in the actor's
    /// PortShape dims and can be resolved from the call-site shape constraint.
    fn resolve_dim_param_from_shape(
        &self,
        param_name: &str,
        meta: &ActorMeta,
        shape_constraint: Option<&ShapeConstraint>,
    ) -> Option<u32> {
        let sc = shape_constraint?;
        // Check in_shape dims for this param name
        for (i, dim) in meta.in_shape.dims.iter().enumerate() {
            if let TokenCount::Symbolic(sym) = dim {
                if sym == param_name {
                    return sc.dims.get(i).and_then(|sd| self.resolve_shape_dim(sd));
                }
            }
        }
        // Check out_shape dims
        for (i, dim) in meta.out_shape.dims.iter().enumerate() {
            if let TokenCount::Symbolic(sym) = dim {
                if sym == param_name {
                    return sc.dims.get(i).and_then(|sd| self.resolve_shape_dim(sd));
                }
            }
        }
        None
    }

    fn is_const_array_ref(&self, arg: &Arg) -> bool {
        if let Arg::ConstRef(ident) = arg {
            if let Some(entry) = self.resolved.consts.get(&ident.name) {
                let stmt = &self.program.statements[entry.stmt_index];
                if let StatementKind::Const(c) = &stmt.kind {
                    return matches!(&c.value, Value::Array(_, _));
                }
            }
        }
        false
    }

    fn arg_to_cpp_literal(&self, arg: &Arg) -> String {
        match arg {
            Arg::Value(Value::Scalar(s)) => self.scalar_literal(s),
            Arg::Value(Value::Array(_, _)) => "{}".to_string(),
            Arg::ParamRef(ident) => format!("_param_{}_val", ident.name),
            Arg::ConstRef(ident) => {
                // Resolve const value
                if let Some(entry) = self.resolved.consts.get(&ident.name) {
                    let stmt = &self.program.statements[entry.stmt_index];
                    if let StatementKind::Const(c) = &stmt.kind {
                        match &c.value {
                            Value::Scalar(s) => return self.scalar_literal(s),
                            Value::Array(_, _) => {
                                return format!("_const_{}", ident.name);
                            }
                        }
                    }
                }
                format!("_const_{}", ident.name)
            }
            Arg::TapRef(_) => "/* tap */".to_string(),
        }
    }

    fn arg_to_cpp_value(&self, arg: &Arg, param_type: &ParamType) -> String {
        match arg {
            Arg::Value(Value::Scalar(s)) => self.scalar_literal(s),
            Arg::Value(Value::Array(_, _)) => "{}".to_string(),
            Arg::ConstRef(ident) => {
                if let Some(entry) = self.resolved.consts.get(&ident.name) {
                    let stmt = &self.program.statements[entry.stmt_index];
                    if let StatementKind::Const(c) = &stmt.kind {
                        match &c.value {
                            Value::Scalar(s) => return self.scalar_literal(s),
                            Value::Array(elems, _) => {
                                // For span params, use std::span
                                if matches!(
                                    param_type,
                                    ParamType::SpanFloat
                                        | ParamType::SpanChar
                                        | ParamType::SpanTypeParam(_)
                                ) {
                                    return format!(
                                        "std::span<const float>(_const_{}, {})",
                                        ident.name,
                                        elems.len()
                                    );
                                }
                                return format!("{}", elems.len());
                            }
                        }
                    }
                }
                format!("_const_{}", ident.name)
            }
            Arg::ParamRef(ident) => format!("_param_{}_val", ident.name),
            Arg::TapRef(_) => "/* tap */".to_string(),
        }
    }

    fn infer_edge_wire_type(&self, sub: &Subgraph, src_id: NodeId) -> PipitType {
        // Trace from source node to find the wire type
        let mut current = src_id;
        let mut visited = HashSet::new();
        loop {
            if !visited.insert(current) {
                return PipitType::Float; // fallback
            }
            if let Some(node) = self.node_in_subgraph(sub, current) {
                match &node.kind {
                    NodeKind::Actor {
                        name, call_span, ..
                    } => {
                        if let Some(meta) = self.lookup_actor(name, *call_span) {
                            return meta.out_type.as_concrete().unwrap_or(PipitType::Float);
                        }
                        return PipitType::Float;
                    }
                    NodeKind::BufferRead { buffer_name } => {
                        return self.infer_buffer_wire_type(buffer_name);
                    }
                    _ => {
                        // Passthrough: trace backward
                        if let Some(edge) = self.first_incoming_edge_in_subgraph(sub, current) {
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
        let buf_info = match self.resolved.buffers.get(buffer_name) {
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

    fn buffer_reader_tasks(&self, buffer_name: &str) -> Vec<String> {
        let mut readers = HashSet::new();
        if let Some(info) = self.resolved.buffers.get(buffer_name) {
            for (task_name, _) in &info.readers {
                readers.insert(task_name.clone());
            }
        }
        let mut sorted: Vec<String> = readers.into_iter().collect();
        sorted.sort();
        sorted
    }

    fn reader_index_for_task(&self, buffer_name: &str, task_name: &str) -> Option<usize> {
        self.buffer_reader_tasks(buffer_name)
            .iter()
            .position(|t| t == task_name)
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

    fn node_in_rate(&self, node_id: NodeId) -> Option<u32> {
        self.analysis
            .node_port_rates
            .get(&node_id)
            .and_then(|r| r.in_rate)
    }

    fn node_out_rate(&self, node_id: NodeId) -> Option<u32> {
        self.analysis
            .node_port_rates
            .get(&node_id)
            .and_then(|r| r.out_rate)
    }

    fn infer_dim_param_from_span_args(
        &self,
        dim_name: &str,
        actor_meta: &ActorMeta,
        actor_args: &[Arg],
    ) -> Option<u32> {
        crate::dim_resolve::infer_dim_param_from_span_args(
            dim_name,
            actor_meta,
            actor_args,
            self.resolved,
            self.program,
        )
    }

    fn resolve_shape_dim(&self, dim: &ShapeDim) -> Option<u32> {
        crate::dim_resolve::resolve_shape_dim(dim, self.resolved, self.program)
    }

    fn firing_repetition(&self, sched: &SubgraphSchedule, node_id: NodeId) -> u32 {
        sched
            .firings
            .iter()
            .find(|f| f.node_id == node_id)
            .map(|f| f.repetition_count)
            .unwrap_or(1)
    }

    fn edge_buffer_tokens(&self, sched: &SubgraphSchedule, src: NodeId, tgt: NodeId) -> u32 {
        sched.edge_buffers.get(&(src, tgt)).copied().unwrap_or(1)
    }

    fn consistent_dim_override_from_edges<'e, I>(
        &self,
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
        let candidate = self.edge_buffer_tokens(sched, first.source, first.target) / rep;
        for edge in edges {
            let tokens = self.edge_buffer_tokens(sched, edge.source, edge.target);
            let val = tokens / rep;
            if candidate != val {
                return None;
            }
        }
        Some(candidate)
    }

    fn symbolic_shape_sides<'m>(&self, meta: &'m ActorMeta) -> HashMap<&'m str, (bool, bool)> {
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

    fn provided_param_names<'m>(&self, meta: &'m ActorMeta, args: &[Arg]) -> HashSet<&'m str> {
        meta.params
            .iter()
            .zip(args.iter())
            .map(|(p, _)| p.name.as_str())
            .collect()
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
            .or_else(|| self.infer_dim_param_from_span_args(sym, meta, args))
            .is_some()
            || self
                .analysis
                .span_derived_dims
                .contains_key(&(node_id, sym.to_string()))
            || provided_params.contains(sym)
    }

    /// Build dimension overrides from the SDF schedule for symbolic params that
    /// couldn't be resolved from args or shape constraints.
    ///
    /// When both sides of an edge have symbolic port rates (e.g. sine OUT(float,N)
    /// → socket_write IN(float,N)), shape inference can't propagate.  But the SDF
    /// balance solver has already computed the per-edge token count, so we derive
    /// the dimension value from: edge_tokens / firing_repetition.
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
        let rep = self.firing_repetition(sched, node_id);
        let incoming_override = self.consistent_dim_override_from_edges(
            sched,
            rep,
            sub.edges.iter().filter(|e| e.target == node_id),
        );
        let outgoing_override = self.consistent_dim_override_from_edges(
            sched,
            rep,
            sub.edges.iter().filter(|e| e.source == node_id),
        );
        let symbolic_sides = self.symbolic_shape_sides(meta);
        let provided_params = self.provided_param_names(meta, args);

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
}

// ── Free helpers ────────────────────────────────────────────────────────────

fn find_node(sub: &Subgraph, id: NodeId) -> Option<&Node> {
    sub.nodes.iter().find(|n| n.id == id)
}

fn subgraphs_of(task_graph: &TaskGraph) -> Vec<&Subgraph> {
    match task_graph {
        TaskGraph::Pipeline(sub) => vec![sub],
        TaskGraph::Modal { control, modes } => {
            let mut subs = vec![control];
            for (_, sub) in modes {
                subs.push(sub);
            }
            subs
        }
    }
}

/// Free-function actor lookup that borrows only the lowered program and registry,
/// not the entire CodegenCtx — avoids whole-self borrow conflicts with `self.out`.
fn lookup_actor_in<'a>(
    lowered: Option<&'a LoweredProgram>,
    registry: &'a Registry,
    actor_name: &str,
    call_span: Span,
) -> Option<&'a ActorMeta> {
    if let Some(l) = lowered {
        if let Some(meta) = l.concrete_actors.get(&call_span) {
            return Some(meta);
        }
    }
    registry.lookup(actor_name)
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

// ── Tests ───────────────────────────────────────────────────────────────────
// Unit tests: verify structural and semantic properties of generated C++ strings
// (firing order, buffer layout, probe stripping) without a C++ compiler.
// Complements compiler/tests/codegen_compile.rs (compilation + runtime checks).

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::Registry;
    use crate::resolve::{self, DiagLevel};
    use std::path::PathBuf;

    fn test_registry() -> Registry {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_path_buf();
        let std_actors = root.join("runtime/libpipit/include/std_actors.h");
        let std_math = root.join("runtime/libpipit/include/std_math.h");
        let example_actors = root.join("examples/example_actors.h");
        let std_sink = root.join("runtime/libpipit/include/std_sink.h");
        let std_source = root.join("runtime/libpipit/include/std_source.h");
        let mut reg = Registry::new();
        reg.load_header(&std_actors)
            .expect("failed to load std_actors.h");
        reg.load_header(&std_math)
            .expect("failed to load std_math.h");
        reg.load_header(&example_actors)
            .expect("failed to load example_actors.h");
        reg.load_header(&std_sink)
            .expect("failed to load std_sink.h");
        reg.load_header(&std_source)
            .expect("failed to load std_source.h");
        reg
    }

    fn codegen_source_with_options(
        source: &str,
        registry: &Registry,
        options: CodegenOptions,
    ) -> CodegenResult {
        let parse_result = crate::parser::parse(source);
        assert!(
            parse_result.errors.is_empty(),
            "parse errors: {:?}",
            parse_result.errors
        );
        let program = parse_result.program.expect("parse failed");
        let resolve_result = resolve::resolve(&program, registry);
        assert!(
            resolve_result
                .diagnostics
                .iter()
                .all(|d| d.level != DiagLevel::Error),
            "resolve errors: {:#?}",
            resolve_result.diagnostics
        );
        let graph_result = crate::graph::build_graph(&program, &resolve_result.resolved, registry);
        assert!(
            graph_result
                .diagnostics
                .iter()
                .all(|d| d.level != DiagLevel::Error),
            "graph errors: {:#?}",
            graph_result.diagnostics
        );
        let analysis_result = crate::analyze::analyze(
            &program,
            &resolve_result.resolved,
            &graph_result.graph,
            registry,
        );
        assert!(
            analysis_result
                .diagnostics
                .iter()
                .all(|d| d.level != DiagLevel::Error),
            "analysis errors: {:#?}",
            analysis_result.diagnostics
        );
        let schedule_result = crate::schedule::schedule(
            &program,
            &resolve_result.resolved,
            &graph_result.graph,
            &analysis_result.analysis,
            registry,
        );
        assert!(
            schedule_result
                .diagnostics
                .iter()
                .all(|d| d.level != DiagLevel::Error),
            "schedule errors: {:#?}",
            schedule_result.diagnostics
        );
        codegen(
            &program,
            &resolve_result.resolved,
            &graph_result.graph,
            &analysis_result.analysis,
            &schedule_result.schedule,
            registry,
            &options,
        )
    }

    fn codegen_source(source: &str, registry: &Registry) -> CodegenResult {
        codegen_source_with_options(
            source,
            registry,
            CodegenOptions {
                release: false,
                include_paths: vec![],
            },
        )
    }

    fn codegen_ok(source: &str, registry: &Registry) -> String {
        let result = codegen_source(source, registry);
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.level == DiagLevel::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "unexpected codegen errors: {:#?}",
            errors
        );
        result.generated.cpp_source
    }

    fn count_occurrences(haystack: &str, needle: &str) -> usize {
        haystack.match_indices(needle).count()
    }

    // ── Const emission tests ────────────────────────────────────────────

    #[test]
    fn const_array_emission() {
        let reg = test_registry();
        let cpp = codegen_ok(
            "const coeff = [0.1, 0.2, 0.4]\nclock 1kHz t { constant(0.0) | fir(coeff) | stdout() }",
            &reg,
        );
        assert!(
            cpp.contains("static constexpr float _const_coeff[]"),
            "should emit const array"
        );
        assert!(cpp.contains("0.1f"), "should have float literals");
    }

    #[test]
    fn const_scalar_emission() {
        let reg = test_registry();
        let cpp = codegen_ok(
            "const fft_size = 256\nclock 1kHz t { constant(0.0) | fft(fft_size) | c2r() | stdout() }",
            &reg,
        );
        assert!(
            cpp.contains("static constexpr auto _const_fft_size = 256;"),
            "should emit const scalar: {}",
            cpp
        );
    }

    // ── Param storage tests ─────────────────────────────────────────────

    #[test]
    fn param_storage_emission() {
        let reg = test_registry();
        let cpp = codegen_ok(
            "param gain = 2.5\nclock 1kHz t { constant(0.0) | mul($gain) | stdout() }",
            &reg,
        );
        assert!(
            cpp.contains("std::atomic<float> _param_gain(2.5f)"),
            "should emit param atomic: {}",
            cpp
        );
    }

    // ── Actor firing tests ──────────────────────────────────────────────

    #[test]
    fn source_actor_firing() {
        let reg = test_registry();
        let cpp = codegen_ok("clock 1kHz t { constant(0.0) | stdout() }", &reg);
        assert!(
            cpp.contains("Actor_constant"),
            "should fire constant actor: {}",
            cpp
        );
        assert!(
            cpp.contains("Actor_stdout"),
            "should fire stdout actor: {}",
            cpp
        );
        // Verify topological firing order
        let pos_const = cpp.find("Actor_constant").unwrap();
        let pos_stdout = cpp.find("Actor_stdout").unwrap();
        assert!(
            pos_const < pos_stdout,
            "constant must fire before stdout in schedule order"
        );
    }

    #[test]
    fn transform_actor_firing() {
        let reg = test_registry();
        let cpp = codegen_ok(
            "clock 1kHz t { constant(0.0) | fft(256) | c2r() | stdout() }",
            &reg,
        );
        assert!(cpp.contains("Actor_fft"), "should fire fft actor: {}", cpp);
        assert!(cpp.contains("Actor_c2r"), "should fire c2r actor: {}", cpp);
        // Verify full topological firing order
        let pos_const = cpp.find("Actor_constant").unwrap();
        let pos_fft = cpp.find("Actor_fft").unwrap();
        let pos_c2r = cpp.find("Actor_c2r").unwrap();
        let pos_stdout = cpp.find("Actor_stdout").unwrap();
        assert!(pos_const < pos_fft, "constant must fire before fft");
        assert!(pos_fft < pos_c2r, "fft must fire before c2r");
        assert!(pos_c2r < pos_stdout, "c2r must fire before stdout");
    }

    #[test]
    fn repetition_offset_emission() {
        let reg = test_registry();
        let cpp = codegen_ok(
            "clock 1kHz t { constant(0.0) | fft(256) | c2r() | stdout() }",
            &reg,
        );
        // fft produces 256 tokens from 1 input; downstream actors repeat with _r offsets
        assert!(
            cpp.contains("for (int _r = 0; _r <"),
            "rate mismatch should produce _r repetition loop: {}",
            cpp
        );
        assert!(
            cpp.contains("_r *"),
            "repetition loop should use _r offset expressions: {}",
            cpp
        );
    }

    #[test]
    fn same_rep_chain_fused_into_single_r_loop() {
        let reg = test_registry();
        let cpp = codegen_ok(
            concat!(
                "const coeff = [0.1, 0.2, 0.4, 0.2, 0.1]\n",
                "clock 1kHz t { constant(0.0) | fft(256) | c2r() | fir(coeff) | stdout() }",
            ),
            &reg,
        );
        assert_eq!(
            count_occurrences(&cpp, "for (int _r = 0; _r < 5; ++_r)"),
            1,
            "rep=5 chain should be fused into a single loop, got:\n{}",
            cpp
        );
    }

    #[test]
    fn rep_mismatch_not_fused() {
        let reg = test_registry();
        let cpp = codegen_ok(
            concat!(
                "const coeff = [0.1, 0.2, 0.4, 0.2, 0.1]\n",
                "clock 1kHz t { constant(0.0) | fft(256) | c2r() | fir(coeff) | stdout() }",
            ),
            &reg,
        );
        assert_eq!(
            count_occurrences(&cpp, "for (int _r = 0; _r < 5; ++_r)"),
            1,
            "rep=5 actors should be fused once, got:\n{}",
            cpp
        );
        let pos_c2r_call = cpp
            .find("_actor_2.operator()(&_e1_2[_r * 256], &_e2_3[_r * 256])")
            .expect("expected c2r call in rep=5 loop");
        let pos_fir_loop = cpp
            .find("for (int _r = 0; _r < 256; ++_r)")
            .expect("expected separate rep=256 loop for fir");
        assert!(
            pos_c2r_call < pos_fir_loop,
            "rep mismatch boundary (c2r->fir) should remain separated, got:\n{}",
            cpp
        );
    }

    #[test]
    fn feedback_edge_not_fused() {
        let reg = test_registry();
        let cpp = codegen_ok(
            concat!(
                "clock 1kHz iir {\n",
                "    constant(0.0)[4] | add(:fb) | mul(2.0) | :out | stdout()\n",
                "    :out | delay(1, 0.0) | :fb\n",
                "}\n"
            ),
            &reg,
        );
        assert!(
            cpp.contains("_fb_"),
            "feedback buffer should be present for cycle graph, got:\n{}",
            cpp
        );
        assert!(
            count_occurrences(&cpp, "for (int _r = 0; _r < ") >= 3,
            "feedback-related nodes should not be coalesced into a single fused loop, got:\n{}",
            cpp
        );
    }

    #[test]
    fn fork_passthrough_chain_fused_into_single_r_loop() {
        let reg = test_registry();
        let cpp = codegen_ok(
            concat!(
                "const coeff = [0.1, 0.2, 0.4, 0.2, 0.1]\n",
                "clock 1kHz t {\n",
                "    constant(0.0)[256] | fft(256) | :raw | c2r() | fir(coeff) | stdout()\n",
                "    :raw | mag() | stdout()\n",
                "}\n"
            ),
            &reg,
        );
        assert_eq!(
            count_occurrences(&cpp, "for (int _r = 0; _r < 5; ++_r)"),
            1,
            "fork-branch same-rep region should be fused into one loop, got:\n{}",
            cpp
        );
    }

    #[test]
    fn probe_passthrough_fusion_uses_per_firing_slice() {
        let reg = test_registry();
        let cpp = codegen_ok(
            concat!(
                "const coeff = [0.1, 0.2, 0.4, 0.2, 0.1]\n",
                "clock 1kHz t {\n",
                "    constant(0.0)[256] | fft(256) | ?spec | c2r() | fir(coeff) | stdout()\n",
                "}\n"
            ),
            &reg,
        );
        assert_eq!(
            count_occurrences(&cpp, "for (int _r = 0; _r < 5; ++_r)"),
            1,
            "probe passthrough chain should be fused into one loop, got:\n{}",
            cpp
        );
        assert!(
            cpp.contains("[probe:spec]"),
            "probe output formatting should be emitted in debug mode, got:\n{}",
            cpp
        );
        assert!(
            cpp.contains("for (int _pi = 0; _pi < 256; ++_pi)"),
            "fused probe should observe per-firing slice (256), got:\n{}",
            cpp
        );
        assert!(
            !cpp.contains("for (int _pi = 0; _pi < 1280; ++_pi)"),
            "fused probe should not replay full buffer per _r, got:\n{}",
            cpp
        );
        assert!(
            cpp.contains("_r * 256"),
            "fused probe should index by _r slice offset, got:\n{}",
            cpp
        );
    }

    // ── Task structure tests ────────────────────────────────────────────

    #[test]
    fn task_function_structure() {
        let reg = test_registry();
        let cpp = codegen_ok("clock 1kHz t { constant(0.0) | stdout() }", &reg);
        assert!(
            cpp.contains("void task_t()"),
            "should emit task function: {}",
            cpp
        );
        assert!(cpp.contains("pipit::Timer"), "should have timer: {}", cpp);
        assert!(
            cpp.contains("_stop.load"),
            "should check stop flag: {}",
            cpp
        );
    }

    #[test]
    fn runtime_context_hooks_emitted() {
        let reg = test_registry();
        let cpp = codegen_ok("clock 1kHz t { constant(0.0) | stdout() }", &reg);
        assert!(
            cpp.contains("pipit::detail::set_actor_task_rate_hz(1000.0);"),
            "should set task rate in runtime context: {}",
            cpp
        );
        assert!(
            cpp.contains("pipit::detail::set_actor_iteration_index(_iter_idx++);"),
            "should set iteration index per logical iteration: {}",
            cpp
        );
    }

    #[test]
    fn k_factor_loop() {
        let reg = test_registry();
        // default tick_rate = 10kHz → K = ceil(10MHz / 10kHz) = 1000
        let cpp = codegen_ok("clock 10MHz t { constant(0.0) | stdout() }", &reg);
        assert!(
            cpp.contains("for (int _k = 0; _k < 1000; ++_k)"),
            "should have K-loop: {}",
            cpp
        );
        // Verify firing order within K-loop
        let pos_const = cpp.find("Actor_constant").unwrap();
        let pos_stdout = cpp.find("Actor_stdout").unwrap();
        assert!(
            pos_const < pos_stdout,
            "constant must fire before stdout within K-loop"
        );
    }

    #[test]
    fn iteration_stride_block_size() {
        let reg = test_registry();
        // sine[256] produces 256 samples per firing → stride = 256
        let cpp = codegen_ok(
            "clock 1kHz t { sine<float>(100.0, 1.0)[256] | stdout() }",
            &reg,
        );
        assert!(
            cpp.contains("_iter_idx += 256"),
            "should advance iteration index by block size 256: {}",
            cpp
        );
    }

    #[test]
    fn iteration_stride_unit_block() {
        // When block size is 1 (default), stride should be 1 → _iter_idx++
        let reg = test_registry();
        let cpp = codegen_ok("clock 1kHz t { constant(0.0) | stdout() }", &reg);
        assert!(
            cpp.contains("_iter_idx++"),
            "unit block should use _iter_idx++: {}",
            cpp
        );
    }

    // ── timer_spin tests ─────────────────────────────────────────────────

    #[test]
    fn timer_spin_default() {
        let reg = test_registry();
        let cpp = codegen_ok("clock 1kHz t { constant(0.0) | stdout() }", &reg);
        // default timer_spin = 10000 (10us)
        assert!(
            cpp.contains("pipit::Timer _timer(1000.0, _stats, 10000);"),
            "default spin should be 10000ns: {}",
            cpp
        );
    }

    #[test]
    fn timer_spin_explicit() {
        let reg = test_registry();
        let cpp = codegen_ok(
            "set timer_spin = 5000\nclock 1kHz t { constant(0.0) | stdout() }",
            &reg,
        );
        assert!(
            cpp.contains("pipit::Timer _timer(1000.0, _stats, 5000);"),
            "explicit spin should be 5000ns: {}",
            cpp
        );
    }

    #[test]
    fn timer_spin_auto() {
        let reg = test_registry();
        let cpp = codegen_ok(
            "set timer_spin = auto\nclock 1kHz t { constant(0.0) | stdout() }",
            &reg,
        );
        // auto → sentinel -1 for adaptive EWMA mode
        assert!(
            cpp.contains("pipit::Timer _timer(1000.0, _stats, -1);"),
            "auto spin should emit sentinel -1: {}",
            cpp
        );
    }

    // ── Fork test ───────────────────────────────────────────────────────

    #[test]
    fn fork_zero_copy() {
        let reg = test_registry();
        let cpp = codegen_ok(
            "clock 1kHz t { constant(0.0) | :raw | stdout()\n:raw | stdout() }",
            &reg,
        );
        // Fork should NOT use memcpy — downstream actors share the upstream buffer
        assert!(
            !cpp.contains("memcpy"),
            "fork should be zero-copy (no memcpy): {}",
            cpp
        );
        assert!(cpp.contains("// fork:"), "fork comment expected: {}", cpp);
    }

    // ── Shared buffer test ──────────────────────────────────────────────

    #[test]
    fn shared_buffer_declaration() {
        let reg = test_registry();
        let cpp = codegen_ok(
            concat!(
                "set mem = 64MB\n",
                "clock 1kHz a { constant(0.0) -> sig }\n",
                "clock 1kHz b { @sig | stdout() }\n",
            ),
            &reg,
        );
        assert!(
            cpp.contains("pipit::RingBuffer<float"),
            "should declare ring buffer: {}",
            cpp
        );
        assert!(cpp.contains("_ringbuf_sig"), "should name buffer: {}", cpp);
    }

    // ── Main function test ──────────────────────────────────────────────

    #[test]
    fn main_with_threads() {
        let reg = test_registry();
        let cpp = codegen_ok(
            concat!(
                "set mem = 64MB\n",
                "clock 1kHz a { constant(0.0) -> sig }\n",
                "clock 1kHz b { @sig | stdout() }\n",
            ),
            &reg,
        );
        assert!(cpp.contains("int main("), "should have main: {}", cpp);
        assert!(cpp.contains("std::thread"), "should spawn threads: {}", cpp);
    }

    // ── Release mode tests ─────────────────────────────────────────────

    #[test]
    fn release_mode_strips_probes() {
        let reg = test_registry();
        let source = "clock 1kHz t { constant(0.0) | mul(1.0) | ?debug | stdout() }";

        // Debug mode: probe infrastructure present
        let debug_cpp = codegen_ok(source, &reg);
        assert!(
            debug_cpp.contains("#ifndef NDEBUG"),
            "debug build should contain #ifndef NDEBUG: {}",
            debug_cpp
        );
        assert!(
            debug_cpp.contains("_probe_debug_enabled"),
            "debug build should contain probe enable flag: {}",
            debug_cpp
        );

        // Release mode: probe infrastructure stripped
        let release_result = codegen_source_with_options(
            source,
            &reg,
            CodegenOptions {
                release: true,
                include_paths: vec![],
            },
        );
        let errors: Vec<_> = release_result
            .diagnostics
            .iter()
            .filter(|d| d.level == DiagLevel::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "unexpected codegen errors: {:#?}",
            errors
        );
        let release_cpp = release_result.generated.cpp_source;

        assert!(
            !release_cpp.contains("#ifndef NDEBUG"),
            "release build should NOT contain #ifndef NDEBUG: {}",
            release_cpp
        );
        assert!(
            !release_cpp.contains("_probe_debug_enabled"),
            "release build should NOT contain probe enable flag: {}",
            release_cpp
        );
        assert!(
            !release_cpp.contains("[probe:"),
            "release build should NOT contain probe output formatting: {}",
            release_cpp
        );
    }

    #[test]
    fn probe_output_is_path_only() {
        let reg = test_registry();
        let source = "clock 1kHz t { constant(0.0) | ?debug | stdout() }";
        let cpp = codegen_ok(source, &reg);
        assert!(
            cpp.contains("std::string _probe_output = \"/dev/stderr\";"),
            "default probe output should be an implementation-defined path: {}",
            cpp
        );
        assert!(
            !cpp.contains("_probe_output != \"stderr\""),
            "should not special-case literal 'stderr' token: {}",
            cpp
        );
        assert!(
            cpp.contains("_probe_output_file = std::fopen(_probe_output.c_str(), \"w\");"),
            "probe output should always be opened as a path: {}",
            cpp
        );
    }

    // ── Integration tests ───────────────────────────────────────────────

    #[test]
    fn gain_pdl_codegen() {
        let reg = test_registry();
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("examples/gain.pdl");
        let source = std::fs::read_to_string(&path).expect("failed to read gain.pdl");
        let cpp = codegen_ok(&source, &reg);
        assert!(cpp.contains("Actor_mul"), "gain.pdl should have mul");
        assert!(cpp.contains("_param_gain"), "gain.pdl should ref param");
    }

    #[test]
    fn example_pdl_codegen() {
        let reg = test_registry();
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("examples/example.pdl");
        let source = std::fs::read_to_string(&path).expect("failed to read example.pdl");
        let cpp = codegen_ok(&source, &reg);
        assert!(
            cpp.contains("pipit::RingBuffer"),
            "example.pdl should have inter-task buffer"
        );
        assert!(
            cpp.contains("std::thread"),
            "example.pdl should have threads"
        );
    }

    #[test]
    fn receiver_pdl_codegen() {
        let reg = test_registry();
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("examples/receiver.pdl");
        let source = std::fs::read_to_string(&path).expect("failed to read receiver.pdl");
        let cpp = codegen_ok(&source, &reg);
        assert!(
            cpp.contains("switch (_ctrl)"),
            "receiver.pdl should have mode switch: {}",
            cpp
        );
        assert!(
            cpp.contains("int32_t _active_mode = -1;"),
            "receiver.pdl should track active mode for transition handling: {}",
            cpp
        );
    }

    #[test]
    fn switch_param_source_reads_runtime_param_for_ctrl() {
        let reg = test_registry();
        let cpp = codegen_ok(
            concat!(
                "param sel = 0\n",
                "clock 1kHz t {\n",
                "    control {\n        constant(0.0) | stdout()\n    }\n",
                "    mode a {\n        constant(0.0) | stdout()\n    }\n",
                "    mode b {\n        constant(0.0) | stdout()\n    }\n",
                "    switch($sel, a, b)\n",
                "}\n"
            ),
            &reg,
        );
        assert!(
            cpp.contains("int32_t _ctrl = static_cast<int32_t>(_param_sel_val);"),
            "switch($param, ...) should read ctrl from runtime param: {}",
            cpp
        );
    }

    #[test]
    fn switch_external_buffer_source_reads_ring_buffer_for_ctrl() {
        let reg = test_registry();
        let cpp = codegen_ok(
            concat!(
                "clock 1kHz producer {\n",
                "    constant(0.0) | detect() -> ctrl\n",
                "}\n",
                "clock 1kHz t {\n",
                "    control {\n        constant(0.0) | stdout()\n    }\n",
                "    mode a {\n        constant(0.0) | stdout()\n    }\n",
                "    mode b {\n        constant(0.0) | stdout()\n    }\n",
                "    switch(ctrl, a, b)\n",
                "}\n"
            ),
            &reg,
        );
        assert!(
            cpp.contains("_ringbuf_ctrl.read(") && cpp.contains("int32_t _ctrl = _ctrl_buf[0];"),
            "switch(buffer, ...) should read ctrl from shared ring buffer: {}",
            cpp
        );
    }

    #[test]
    fn switch_default_clause_has_no_codegen_fallback() {
        let reg = test_registry();
        let cpp = codegen_ok(
            concat!(
                "clock 1kHz t {\n",
                "    control {\n        constant(0.0) | detect() -> ctrl\n    }\n",
                "    mode a {\n        constant(0.0) | stdout()\n    }\n",
                "    mode b {\n        constant(0.0) | stdout()\n    }\n",
                "    switch(ctrl, a, b) default a\n",
                "}\n"
            ),
            &reg,
        );
        assert!(
            cpp.contains("switch (_ctrl)"),
            "modal task should dispatch by ctrl value: {}",
            cpp
        );
        assert!(
            cpp.contains("default: break;"),
            "out-of-range ctrl should not force fallback mode: {}",
            cpp
        );
        assert!(
            !cpp.contains("_ringbuf_ctrl.write("),
            "internally consumed switch ctrl should not be written to shared ring buffer: {}",
            cpp
        );
    }

    #[test]
    fn feedback_pdl_codegen() {
        let reg = test_registry();
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("examples/feedback.pdl");
        let source = std::fs::read_to_string(&path).expect("failed to read feedback.pdl");
        let cpp = codegen_ok(&source, &reg);
        assert!(
            cpp.contains("_fb_"),
            "feedback.pdl should have feedback buffer: {}",
            cpp
        );
        assert!(
            cpp.contains("Actor_delay"),
            "feedback.pdl should have delay actor"
        );
    }

    // ── Polymorphic actor tests (v0.3.0) ─────────────────────────────────

    fn poly_registry() -> Registry {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_path_buf();
        let mut reg = test_registry();
        let poly_actors = root.join("examples/poly_actors.h");
        reg.load_header(&poly_actors)
            .expect("failed to load poly_actors.h");
        reg
    }

    /// Run the full pipeline (with type_infer + lower) and return generated C++.
    fn codegen_poly_ok(source: &str, registry: &Registry) -> String {
        let parse_result = crate::parser::parse(source);
        assert!(
            parse_result.errors.is_empty(),
            "parse errors: {:?}",
            parse_result.errors
        );
        let program = parse_result.program.expect("parse failed");
        let resolve_result = resolve::resolve(&program, registry);
        assert!(
            resolve_result
                .diagnostics
                .iter()
                .all(|d| d.level != DiagLevel::Error),
            "resolve errors: {:#?}",
            resolve_result.diagnostics
        );

        // Type inference
        let type_infer_result =
            crate::type_infer::type_infer(&program, &resolve_result.resolved, registry);
        assert!(
            type_infer_result
                .diagnostics
                .iter()
                .all(|d| d.level != DiagLevel::Error),
            "type_infer errors: {:#?}",
            type_infer_result.diagnostics
        );

        // Lowering
        let lower_result = crate::lower::lower_and_verify(
            &program,
            &resolve_result.resolved,
            &type_infer_result.typed,
            registry,
        );
        assert!(
            !lower_result.has_errors(),
            "lower errors: {:#?}",
            lower_result.diagnostics
        );
        assert!(
            lower_result.cert.all_pass(),
            "L1-L5 cert failed: {:?}",
            lower_result.cert
        );

        let graph_result = crate::graph::build_graph(&program, &resolve_result.resolved, registry);
        assert!(
            graph_result
                .diagnostics
                .iter()
                .all(|d| d.level != DiagLevel::Error),
            "graph errors: {:#?}",
            graph_result.diagnostics
        );
        let analysis_result = crate::analyze::analyze(
            &program,
            &resolve_result.resolved,
            &graph_result.graph,
            registry,
        );
        assert!(
            analysis_result
                .diagnostics
                .iter()
                .all(|d| d.level != DiagLevel::Error),
            "analysis errors: {:#?}",
            analysis_result.diagnostics
        );
        let schedule_result = crate::schedule::schedule(
            &program,
            &resolve_result.resolved,
            &graph_result.graph,
            &analysis_result.analysis,
            registry,
        );
        assert!(
            schedule_result
                .diagnostics
                .iter()
                .all(|d| d.level != DiagLevel::Error),
            "schedule errors: {:#?}",
            schedule_result.diagnostics
        );

        let result = codegen_with_lowered(
            &program,
            &resolve_result.resolved,
            &graph_result.graph,
            &analysis_result.analysis,
            &schedule_result.schedule,
            registry,
            &CodegenOptions {
                release: false,
                include_paths: vec![],
            },
            Some(&lower_result.lowered),
        );
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.level == DiagLevel::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "unexpected codegen errors: {:#?}",
            errors
        );
        result.generated.cpp_source
    }

    #[test]
    fn poly_scale_explicit_float_template_syntax() {
        let reg = poly_registry();
        let cpp = codegen_poly_ok(
            "clock 1kHz t { constant(0.0) | poly_scale<float>(2.0) | stdout() }",
            &reg,
        );
        assert!(
            cpp.contains("Actor_poly_scale<float>"),
            "should emit template instantiation Actor_poly_scale<float>, got:\n{}",
            cpp
        );
    }

    #[test]
    fn poly_pass_explicit_float_template_syntax() {
        let reg = poly_registry();
        let cpp = codegen_poly_ok(
            "clock 1kHz t { constant(0.0) | poly_pass<float>() | stdout() }",
            &reg,
        );
        assert!(
            cpp.contains("Actor_poly_pass<float>"),
            "should emit Actor_poly_pass<float>, got:\n{}",
            cpp
        );
    }

    #[test]
    fn poly_now_polymorphic_constant() {
        // constant and stdout are now polymorphic — should emit template syntax
        let reg = poly_registry();
        let cpp = codegen_poly_ok("clock 1kHz t { constant(0.0) | stdout() }", &reg);
        assert!(
            cpp.contains("Actor_constant<float>"),
            "polymorphic constant should use Actor_constant<float> syntax, got:\n{}",
            cpp
        );
        assert!(
            cpp.contains("Actor_stdout<float>"),
            "polymorphic stdout should use Actor_stdout<float> syntax, got:\n{}",
            cpp
        );
    }

    // ── v0.3.1 regression tests ─────────────────────────────────────────

    #[test]
    fn fir_span_derived_param_in_codegen() {
        let reg = test_registry();
        let cpp = codegen_ok(
            "const coeff = [0.1, 0.2, 0.4, 0.2, 0.1]\nclock 1kHz t { constant(0.0) | fir(coeff) | stdout() }",
            &reg,
        );
        // fir actor constructor should have N=5 from span-derived dim
        assert!(
            cpp.contains(", 5}"),
            "fir actor should have N=5 from span-derived dim, got:\n{}",
            cpp
        );
    }

    #[test]
    fn fir_span_derived_not_overridden_by_edge_inference() {
        let reg = test_registry();
        let cpp = codegen_ok(
            concat!(
                "const coeff = [0.1, 0.2, 0.4, 0.2, 0.1]\n",
                "clock 1kHz t { constant(0.0) | fft(256) | c2r() | fir(coeff) | stdout() }",
            ),
            &reg,
        );
        // Even with fft(256) upstream producing 256 tokens, fir N should be 5
        assert!(
            cpp.contains("Actor_fir") && cpp.contains(", 5}"),
            "fir should have N=5 even after fft(256)|c2r(), not 256, got:\n{}",
            cpp
        );
        // fir's constructor should NOT contain 256 as its N param
        assert!(
            !cpp.contains("Actor_fir")
                || !cpp.contains("Actor_fir<float>{std::span<const float>(_const_coeff, 5), 256}"),
            "fir should NOT have N=256 from edge inference, got:\n{}",
            cpp
        );
    }

    #[test]
    fn shared_buffer_block_transfer() {
        let reg = test_registry();
        let cpp = codegen_ok(
            concat!(
                "set mem = 64MB\n",
                "clock 1kHz a { constant(0.0) -> sig }\n",
                "clock 1kHz b { @sig | stdout() }\n",
            ),
            &reg,
        );
        // Ring buffer read/write should use base buffer pointer (no &buf[_r*N] offset)
        assert!(
            !cpp.contains("_ringbuf_sig.write(&_"),
            "ring buffer write should NOT use per-firing pointer offset (block transfer), got:\n{}",
            cpp
        );
        assert!(
            !cpp.contains("_ringbuf_sig.read(0, &_"),
            "ring buffer read should NOT use per-firing pointer offset (block transfer), got:\n{}",
            cpp
        );
    }

    #[test]
    fn actor_construction_hoisted_for_repetition() {
        let reg = test_registry();
        let cpp = codegen_ok(
            "clock 1kHz t { constant(0.0) | fft(256) | c2r() | stdout() }",
            &reg,
        );
        // c2r has rep=256. Its actor should be hoisted before the _r loop.
        assert!(
            cpp.contains("auto _actor_"),
            "hoistable actor with rep>1 should have pre-loop declaration, got:\n{}",
            cpp
        );
        // The hoisted actor should be called via .operator() inside the loop
        assert!(
            cpp.contains("_actor_") && cpp.contains(".operator()("),
            "hoisted actor should be called via .operator() inside the loop, got:\n{}",
            cpp
        );
    }

    #[test]
    fn actor_construction_hoisted_before_k_loop() {
        let reg = test_registry();
        let cpp = codegen_ok("clock 10MHz t { constant(0.0) | stdout() }", &reg);
        let task_start = cpp.find("void task_t()").expect("missing task_t");
        let task_tail = &cpp[task_start..];
        let task_end = task_tail.find("int main(").unwrap_or(task_tail.len());
        let task = &task_tail[..task_end];

        let decl_pos = task
            .find("auto _actor_")
            .expect("expected hoisted actor declaration in task_t");
        let k_pos = task
            .find("for (int _k = 0; _k < ")
            .expect("expected k-loop in task_t");
        assert!(
            decl_pos < k_pos,
            "actor should be hoisted before _k loop in task_t, got:\n{}",
            task
        );
    }

    #[test]
    fn multi_input_single_token_edges_avoid_memcpy() {
        let reg = test_registry();
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let root = manifest_dir.parent().expect("missing workspace root");
        let path = root.join("examples/feedback.pdl");
        let source = std::fs::read_to_string(path).expect("failed to read feedback.pdl");
        let cpp = codegen_ok(&source, &reg);
        assert!(
            !cpp.contains("std::memcpy(&_in_"),
            "single-token multi-input copies should use direct assignments, got:\n{}",
            cpp
        );
        assert!(
            cpp.contains("_in_") && cpp.contains("[0] = (") && cpp.contains("[1] = ("),
            "feedback add input packing should still populate local input slots, got:\n{}",
            cpp
        );
    }
}
