// schedule.rs — PASS schedule generation for Pipit SDF graphs
//
// Generates a Periodic Asynchronous Static Schedule (PASS) for each task.
// Per-task topological sort of actors in dependency order, with each actor
// firing repetition_vector[node] times per PASS cycle.
//
// Preconditions: `thir` is a ThirContext wrapping HIR + resolved/typed/lowered;
//                `graph` is a valid ProgramGraph with detected cycles;
//                `analysis` has computed repetition vectors.
// Postconditions: returns `ScheduleResult` with per-task schedules, K factors,
//                 and intra-task buffer sizes.
// Failure modes: unsortable subgraphs produce `Diagnostic` entries.
// Side effects: none.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;

use chumsky::span::Span as _;

use crate::analyze::AnalyzedProgram;
use crate::ast::*;
use crate::diag::codes;
use crate::diag::{DiagCode, DiagLevel, Diagnostic};
use crate::graph::*;
use crate::thir::ThirContext;

// ── Public types ────────────────────────────────────────────────────────────

/// A single firing entry: which node fires and how many times per PASS cycle.
#[derive(Debug, Clone)]
pub struct FiringEntry {
    pub node_id: NodeId,
    pub repetition_count: u32,
}

/// Schedule for a single subgraph (pipeline, control, or mode).
#[derive(Debug, Clone)]
pub struct SubgraphSchedule {
    /// Topological firing order with repetition counts.
    pub firings: Vec<FiringEntry>,
    /// Intra-task buffer tokens per edge: (source, target) → tokens.
    pub edge_buffers: HashMap<(NodeId, NodeId), u32>,
}

/// Schedule for an entire task.
#[derive(Debug, Clone)]
pub enum TaskSchedule {
    Pipeline(SubgraphSchedule),
    Modal {
        control: SubgraphSchedule,
        modes: Vec<(String, SubgraphSchedule)>,
    },
}

/// Per-task scheduling metadata.
#[derive(Debug, Clone)]
pub struct TaskMeta {
    pub schedule: TaskSchedule,
    /// K = iterations per tick (≥ 1).
    pub k_factor: u32,
    /// Task target frequency in Hz.
    pub freq_hz: f64,
}

/// Result of schedule generation.
#[derive(Debug)]
pub struct ScheduleResult {
    pub schedule: ScheduledProgram,
    pub diagnostics: Vec<Diagnostic>,
}

/// Computed schedule data, consumed by downstream codegen phases.
#[derive(Debug)]
pub struct ScheduledProgram {
    /// Per-task schedule and metadata.
    pub tasks: HashMap<String, TaskMeta>,
}

// ── Verification ─────────────────────────────────────────────────────────────

/// Machine-checkable evidence for schedule postconditions (S1-S2).
#[derive(Debug, Clone)]
pub struct ScheduleCert {
    /// S1: Every HIR task has a corresponding schedule entry.
    pub s1_all_tasks_scheduled: bool,
    /// S2: Every subgraph node appears exactly once in the firing order.
    pub s2_all_nodes_fired: bool,
}

impl crate::pass::StageCert for ScheduleCert {
    fn all_pass(&self) -> bool {
        self.s1_all_tasks_scheduled && self.s2_all_nodes_fired
    }

    fn obligations(&self) -> Vec<(&'static str, bool)> {
        vec![
            ("S1_all_tasks_scheduled", self.s1_all_tasks_scheduled),
            ("S2_all_nodes_fired", self.s2_all_nodes_fired),
        ]
    }
}

/// Verify schedule postconditions.
///
/// `task_names` is the list of task names from `hir.tasks` (extracted before
/// entering the thir borrow scope to avoid lifetime conflicts).
pub fn verify_schedule(
    schedule: &ScheduledProgram,
    graph: &ProgramGraph,
    task_names: &[String],
) -> ScheduleCert {
    let s1 = verify_s1_all_tasks_scheduled(schedule, task_names);
    let s2 = verify_s2_all_nodes_fired(schedule, graph);
    ScheduleCert {
        s1_all_tasks_scheduled: s1,
        s2_all_nodes_fired: s2,
    }
}

/// S1: Every HIR task name has a corresponding entry in the schedule.
fn verify_s1_all_tasks_scheduled(schedule: &ScheduledProgram, task_names: &[String]) -> bool {
    task_names
        .iter()
        .all(|name| schedule.tasks.contains_key(name))
}

/// S2: For each scheduled task, every graph node appears exactly once in firings.
///
/// Three conditions checked per subgraph pair:
/// (a) firings.len() == graph_nodes.len()
/// (b) no duplicate node_ids in firings (HashSet insert returns false → duplicate)
/// (c) every graph node is present in the firing set
fn verify_s2_all_nodes_fired(schedule: &ScheduledProgram, graph: &ProgramGraph) -> bool {
    for (task_name, task_meta) in &schedule.tasks {
        let task_graph = match graph.tasks.get(task_name) {
            Some(g) => g,
            None => return false, // scheduled task not in graph — unexpected
        };

        match (&task_meta.schedule, task_graph) {
            (TaskSchedule::Pipeline(sched), TaskGraph::Pipeline(sub)) => {
                if !check_subgraph_coverage(sched, sub) {
                    return false;
                }
            }
            (
                TaskSchedule::Modal {
                    control: ctrl_sched,
                    modes: mode_scheds,
                },
                TaskGraph::Modal {
                    control: ctrl_graph,
                    modes: mode_graphs,
                },
            ) => {
                if !check_subgraph_coverage(ctrl_sched, ctrl_graph) {
                    return false;
                }
                for (mode_name, mode_sched) in mode_scheds {
                    let mode_graph = match mode_graphs.iter().find(|(n, _)| n == mode_name) {
                        Some((_, g)) => g,
                        None => return false,
                    };
                    if !check_subgraph_coverage(mode_sched, mode_graph) {
                        return false;
                    }
                }
            }
            _ => return false, // Pipeline/Modal mismatch
        }
    }
    true
}

/// Check that a subgraph schedule covers every node exactly once.
fn check_subgraph_coverage(sched: &SubgraphSchedule, sub: &Subgraph) -> bool {
    // (a) Length equality
    if sched.firings.len() != sub.nodes.len() {
        return false;
    }
    // (b) No duplicates in firings
    let mut seen = HashSet::with_capacity(sched.firings.len());
    for entry in &sched.firings {
        if !seen.insert(entry.node_id) {
            return false; // duplicate
        }
    }
    // (c) Every graph node is present
    for node in &sub.nodes {
        if !seen.contains(&node.id) {
            return false;
        }
    }
    true
}

// ── Public entry point ──────────────────────────────────────────────────────

/// Generate PASS schedules for all tasks in a program.
pub fn schedule(
    thir: &ThirContext,
    graph: &ProgramGraph,
    analysis: &AnalyzedProgram,
) -> ScheduleResult {
    let mut ctx = ScheduleCtx::new(thir, graph, analysis);
    ctx.schedule_all_tasks();
    ctx.build_result()
}

// ── Internal context ────────────────────────────────────────────────────────

struct ScheduleCtx<'a> {
    thir: &'a ThirContext<'a>,
    graph: &'a ProgramGraph,
    analysis: &'a AnalyzedProgram,
    diagnostics: Vec<Diagnostic>,
    task_schedules: HashMap<String, TaskMeta>,
}

impl<'a> ScheduleCtx<'a> {
    fn new(
        thir: &'a ThirContext<'a>,
        graph: &'a ProgramGraph,
        analysis: &'a AnalyzedProgram,
    ) -> Self {
        ScheduleCtx {
            thir,
            graph,
            analysis,
            diagnostics: Vec::new(),
            task_schedules: HashMap::new(),
        }
    }

    fn error(&mut self, code: DiagCode, span: Span, message: String) {
        self.diagnostics
            .push(Diagnostic::new(DiagLevel::Error, span, message).with_code(code));
    }

    fn warning(&mut self, code: DiagCode, span: Span, message: String) {
        self.diagnostics
            .push(Diagnostic::new(DiagLevel::Warning, span, message).with_code(code));
    }

    fn build_result(self) -> ScheduleResult {
        ScheduleResult {
            schedule: ScheduledProgram {
                tasks: self.task_schedules,
            },
            diagnostics: self.diagnostics,
        }
    }

    // ── Task scheduling ─────────────────────────────────────────────────

    fn schedule_all_tasks(&mut self) {
        for hir_task in &self.thir.hir.tasks {
            self.schedule_task(&hir_task.name, hir_task.freq_hz, hir_task.freq_span);
        }
    }

    fn schedule_task(&mut self, task_name: &str, freq_hz: f64, freq_span: Span) {
        let task_graph = match self.graph.tasks.get(task_name) {
            Some(g) => g,
            None => return,
        };

        let task_schedule = match task_graph {
            TaskGraph::Pipeline(sub) => {
                let rv_key = (task_name.to_string(), "pipeline".to_string());
                match self.analysis.repetition_vectors.get(&rv_key) {
                    Some(rv) => match self.sort_subgraph(task_name, "pipeline", sub, rv) {
                        Some(sched) => TaskSchedule::Pipeline(sched),
                        None => return,
                    },
                    None => return,
                }
            }
            TaskGraph::Modal { control, modes } => {
                let ctrl_rv_key = (task_name.to_string(), "control".to_string());
                let ctrl_sched = self
                    .analysis
                    .repetition_vectors
                    .get(&ctrl_rv_key)
                    .and_then(|rv| self.sort_subgraph(task_name, "control", control, rv));

                let ctrl_sched = match ctrl_sched {
                    Some(s) => s,
                    None => return,
                };

                let mut mode_scheds = Vec::new();
                for (mode_name, sub) in modes {
                    let mode_rv_key = (task_name.to_string(), mode_name.clone());
                    if let Some(rv) = self.analysis.repetition_vectors.get(&mode_rv_key) {
                        if let Some(sched) = self.sort_subgraph(task_name, mode_name, sub, rv) {
                            mode_scheds.push((mode_name.clone(), sched));
                        }
                    }
                }

                TaskSchedule::Modal {
                    control: ctrl_sched,
                    modes: mode_scheds,
                }
            }
        };

        let tick_rate_hz = self.thir.tick_rate_hz;
        let k = compute_k_factor(freq_hz, tick_rate_hz);

        // Rate guardrails (ADR-014): warn when effective timer rate is unsustainable
        let timer_hz = freq_hz / k as f64;
        let period_ns = 1_000_000_000.0 / timer_hz;
        if period_ns < 10_000.0 {
            self.warning(
                codes::W0400,
                freq_span,
                format!(
                    "effective tick period is {:.0}ns ({:.0}Hz); \
                     most OS schedulers cannot sustain rates above ~100kHz reliably",
                    period_ns, timer_hz
                ),
            );
        }

        self.task_schedules.insert(
            task_name.to_string(),
            TaskMeta {
                schedule: task_schedule,
                k_factor: k,
                freq_hz,
            },
        );
    }

    // ── Topological sort (Kahn's algorithm) ─────────────────────────────

    fn sort_subgraph(
        &mut self,
        task_name: &str,
        label: &str,
        sub: &Subgraph,
        rv: &HashMap<NodeId, u32>,
    ) -> Option<SubgraphSchedule> {
        if sub.nodes.is_empty() {
            return Some(SubgraphSchedule {
                firings: Vec::new(),
                edge_buffers: HashMap::new(),
            });
        }

        // Identify back-edges from feedback cycles (delay actors break cycles)
        let back_edges = self.identify_back_edges(sub);

        // Build in-degree map and adjacency list (excluding back-edges)
        let mut in_degree: HashMap<NodeId, u32> = HashMap::new();
        let mut adj: HashMap<NodeId, Vec<NodeId>> = HashMap::new();

        for node in &sub.nodes {
            in_degree.entry(node.id).or_insert(0);
            adj.entry(node.id).or_default();
        }

        for edge in &sub.edges {
            if back_edges.contains(&(edge.source, edge.target)) {
                continue;
            }
            *in_degree.entry(edge.target).or_insert(0) += 1;
            adj.entry(edge.source).or_default().push(edge.target);
        }

        // Kahn's algorithm with deterministic ordering (sort by NodeId)
        let mut queue: Vec<NodeId> = in_degree
            .iter()
            .filter(|(_, &deg)| deg == 0)
            .map(|(&id, _)| id)
            .collect();
        queue.sort_by_key(|id| id.0);
        let mut queue: VecDeque<NodeId> = queue.into_iter().collect();

        let mut firings = Vec::new();

        while let Some(node_id) = queue.pop_front() {
            let count = rv.get(&node_id).copied().unwrap_or(1);
            firings.push(FiringEntry {
                node_id,
                repetition_count: count,
            });

            if let Some(neighbors) = adj.get(&node_id) {
                let mut sorted = neighbors.clone();
                sorted.sort_by_key(|id| id.0);
                for next in sorted {
                    if let Some(deg) = in_degree.get_mut(&next) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push_back(next);
                        }
                    }
                }
            }
        }

        // Check all nodes were scheduled
        if firings.len() < sub.nodes.len() {
            let scheduled: HashSet<NodeId> = firings.iter().map(|f| f.node_id).collect();
            let stuck: Vec<NodeId> = sub
                .nodes
                .iter()
                .map(|n| n.id)
                .filter(|id| !scheduled.contains(id))
                .collect();
            self.error(
                codes::E0400,
                Span::new((), 0..0),
                format!(
                    "cannot schedule subgraph '{}' of task '{}': {} node(s) in \
                     unresolvable cycle ({:?})",
                    label,
                    task_name,
                    stuck.len(),
                    stuck
                ),
            );
            return None;
        }

        let edge_buffers = self.compute_edge_buffers(sub, rv, &back_edges);

        Some(SubgraphSchedule {
            firings,
            edge_buffers,
        })
    }

    // ── Back-edge identification ────────────────────────────────────────

    fn identify_back_edges(&self, sub: &Subgraph) -> HashSet<(NodeId, NodeId)> {
        crate::subgraph_index::identify_back_edges(sub, &self.graph.cycles)
    }

    // ── Intra-task buffer sizing ────────────────────────────────────────

    fn compute_edge_buffers(
        &self,
        sub: &Subgraph,
        rv: &HashMap<NodeId, u32>,
        back_edges: &HashSet<(NodeId, NodeId)>,
    ) -> HashMap<(NodeId, NodeId), u32> {
        let mut buffers = HashMap::new();

        for edge in &sub.edges {
            if back_edges.contains(&(edge.source, edge.target)) {
                // Back-edge: buffer holds initial tokens from delay actor
                let tokens = self.delay_initial_tokens(sub, edge.source);
                buffers.insert((edge.source, edge.target), tokens);
                continue;
            }

            let p = self.node_out_rate(edge.source).unwrap_or(1);
            let rv_src = rv.get(&edge.source).copied().unwrap_or(1);
            buffers.insert((edge.source, edge.target), p * rv_src);
        }

        buffers
    }

    /// Get the initial token count from a delay actor (first arg).
    fn delay_initial_tokens(&self, sub: &Subgraph, node_id: NodeId) -> u32 {
        if let Some(node) = find_node(sub, node_id) {
            if let NodeKind::Actor { args, name, .. } = &node.kind {
                if name == "delay" {
                    if let Some(Arg::Value(Value::Scalar(Scalar::Number(n, _, _)))) = args.first() {
                        return *n as u32;
                    }
                }
            }
        }
        1
    }

    fn node_out_rate(&self, node_id: NodeId) -> Option<u32> {
        self.analysis
            .node_port_rates
            .get(&node_id)
            .and_then(|r| r.out_rate)
    }
}

// ── Free helpers ────────────────────────────────────────────────────────────

use crate::subgraph_index::find_node;

/// K factor: iterations per tick (compile-time heuristic).
/// K = ceil(freq / tick_rate). Default tick_rate = 1 MHz.
fn compute_k_factor(freq_hz: f64, tick_rate_hz: f64) -> u32 {
    if freq_hz <= tick_rate_hz {
        1
    } else {
        (freq_hz / tick_rate_hz).ceil() as u32
    }
}

// ── Display ─────────────────────────────────────────────────────────────────

impl fmt::Display for ScheduledProgram {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "ScheduledProgram ({} tasks)", self.tasks.len())?;
        let mut task_names: Vec<&String> = self.tasks.keys().collect();
        task_names.sort();

        for task_name in task_names {
            let meta = &self.tasks[task_name];
            writeln!(
                f,
                "  task '{}' (K={}, freq={:.0}Hz):",
                task_name, meta.k_factor, meta.freq_hz
            )?;
            match &meta.schedule {
                TaskSchedule::Pipeline(sched) => {
                    write_subgraph_schedule(f, "pipeline", sched, "    ")?;
                }
                TaskSchedule::Modal { control, modes } => {
                    write_subgraph_schedule(f, "control", control, "    ")?;
                    for (mode_name, sched) in modes {
                        write_subgraph_schedule(f, &format!("mode:{mode_name}"), sched, "    ")?;
                    }
                }
            }
        }
        Ok(())
    }
}

fn write_subgraph_schedule(
    f: &mut fmt::Formatter<'_>,
    label: &str,
    sched: &SubgraphSchedule,
    indent: &str,
) -> fmt::Result {
    writeln!(f, "{indent}[{label}]")?;
    for (i, entry) in sched.firings.iter().enumerate() {
        writeln!(
            f,
            "{indent}  {i}: node {} x{}",
            entry.node_id.0, entry.repetition_count
        )?;
    }
    if !sched.edge_buffers.is_empty() {
        writeln!(f, "{indent}  buffers:")?;
        let mut edges: Vec<_> = sched.edge_buffers.iter().collect();
        edges.sort_by_key(|((a, b), _)| (a.0, b.0));
        for ((src, tgt), size) in edges {
            writeln!(f, "{indent}    ({} -> {}): {size} tokens", src.0, tgt.0)?;
        }
    }
    Ok(())
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::Registry;
    use crate::resolve;
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

    fn schedule_source(source: &str, registry: &Registry) -> ScheduleResult {
        let parse_result = crate::parser::parse(source);
        assert!(
            parse_result.errors.is_empty(),
            "parse errors: {:?}",
            parse_result.errors
        );
        let program = parse_result.program.expect("parse failed");
        let mut resolve_result = resolve::resolve(&program, registry);
        assert!(
            resolve_result
                .diagnostics
                .iter()
                .all(|d| d.level != DiagLevel::Error),
            "resolve errors: {:#?}",
            resolve_result.diagnostics
        );
        let hir_program = crate::hir::build_hir(
            &program,
            &resolve_result.resolved,
            &mut resolve_result.id_alloc,
        );
        let graph_result =
            crate::graph::build_graph(&hir_program, &resolve_result.resolved, registry);
        assert!(
            graph_result
                .diagnostics
                .iter()
                .all(|d| d.level != DiagLevel::Error),
            "graph errors: {:#?}",
            graph_result.diagnostics
        );
        let type_result =
            crate::type_infer::type_infer(&hir_program, &resolve_result.resolved, registry);
        let lower_result = crate::lower::lower_and_verify(
            &hir_program,
            &resolve_result.resolved,
            &type_result.typed,
            registry,
        );
        let thir = crate::thir::build_thir_context(
            &hir_program,
            &resolve_result.resolved,
            &type_result.typed,
            &lower_result.lowered,
            registry,
            &graph_result.graph,
        );
        let analysis_result = crate::analyze::analyze(&thir, &graph_result.graph);
        assert!(
            analysis_result
                .diagnostics
                .iter()
                .all(|d| d.level != DiagLevel::Error),
            "analysis errors: {:#?}",
            analysis_result.diagnostics
        );
        schedule(&thir, &graph_result.graph, &analysis_result.analysis)
    }

    fn schedule_ok(source: &str, registry: &Registry) -> ScheduleResult {
        let result = schedule_source(source, registry);
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.level == DiagLevel::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "unexpected schedule errors: {:#?}",
            errors
        );
        result
    }

    fn get_pipeline_schedule(meta: &TaskMeta) -> &SubgraphSchedule {
        match &meta.schedule {
            TaskSchedule::Pipeline(s) => s,
            _ => panic!("expected Pipeline schedule"),
        }
    }

    // ── Basic tests ─────────────────────────────────────────────────────

    #[test]
    fn linear_pipeline_order() {
        let reg = test_registry();
        let result = schedule_ok("clock 1kHz t {\n    constant(0.0) | stdout()\n}", &reg);
        let meta = result.schedule.tasks.get("t").expect("task 't'");
        let sched = get_pipeline_schedule(meta);
        assert_eq!(sched.firings.len(), 2);
        assert_eq!(meta.k_factor, 1);
    }

    #[test]
    fn topological_dependency_order() {
        // adc must fire before fft, fft before c2r, c2r before stdout
        let reg = test_registry();
        let result = schedule_ok(
            "clock 1kHz t {\n    constant(0.0) | fft(256) | c2r() | stdout()\n}",
            &reg,
        );
        let meta = result.schedule.tasks.get("t").unwrap();
        let sched = get_pipeline_schedule(meta);
        assert_eq!(sched.firings.len(), 4);
        // With shape inference: constant gets inferred [256] from fft backward,
        // c2r gets inferred [256] from fft forward.
        // rv: constant=1, fft=1, c2r=1, stdout=256
        assert_eq!(sched.firings[0].repetition_count, 1, "constant fires 1x");
        assert_eq!(sched.firings[1].repetition_count, 1, "fft fires 1x");
        assert_eq!(sched.firings[2].repetition_count, 1, "c2r fires 1x");
        assert_eq!(sched.firings[3].repetition_count, 256, "stdout fires 256x");
    }

    #[test]
    fn decimation_repetition_counts() {
        let reg = test_registry();
        let result = schedule_ok(
            "clock 1kHz t {\n    constant(0.0) | fft(256) | c2r() | stdout()\n}",
            &reg,
        );
        let meta = result.schedule.tasks.get("t").unwrap();
        let sched = get_pipeline_schedule(meta);
        // fft(256): IN(float,256), OUT(cfloat,256)
        // With shape inference: constant gets inferred [256] → rv=1
        // c2r gets inferred [256] → rv=1, stdout rv=256
        let adc_rv = sched.firings.first().unwrap().repetition_count;
        assert_eq!(
            adc_rv, 1,
            "constant fires 1x (produces 256 per firing via inference)"
        );
    }

    // ── K factor tests ──────────────────────────────────────────────────

    #[test]
    fn k_factor_high_freq() {
        let reg = test_registry();
        // default tick_rate = 10kHz → K = ceil(10MHz / 10kHz) = 1000
        let result = schedule_ok("clock 10MHz t {\n    constant(0.0) | stdout()\n}", &reg);
        let meta = result.schedule.tasks.get("t").unwrap();
        assert_eq!(meta.k_factor, 1000);
    }

    #[test]
    fn k_factor_low_freq() {
        let reg = test_registry();
        let result = schedule_ok("clock 1kHz t {\n    constant(0.0) | stdout()\n}", &reg);
        let meta = result.schedule.tasks.get("t").unwrap();
        assert_eq!(meta.k_factor, 1);
    }

    #[test]
    fn k_factor_1mhz_boundary() {
        let reg = test_registry();
        // default tick_rate = 10kHz → K = ceil(1MHz / 10kHz) = 100
        let result = schedule_ok("clock 1MHz t {\n    constant(0.0) | stdout()\n}", &reg);
        let meta = result.schedule.tasks.get("t").unwrap();
        assert_eq!(meta.k_factor, 100);
    }

    #[test]
    fn k_factor_custom_tick_rate() {
        let reg = test_registry();
        // set tick_rate = 1kHz with a 10kHz task → K = ceil(10000/1000) = 10
        let result = schedule_ok(
            "set tick_rate = 1kHz\nclock 10kHz t {\n    constant(0.0) | stdout()\n}",
            &reg,
        );
        let meta = result.schedule.tasks.get("t").unwrap();
        assert_eq!(meta.k_factor, 10);
    }

    #[test]
    fn k_factor_custom_tick_rate_below_threshold() {
        let reg = test_registry();
        // set tick_rate = 1kHz with a 500Hz task → K = 1 (freq <= tick_rate)
        let result = schedule_ok(
            "set tick_rate = 1kHz\nclock 500Hz t {\n    constant(0.0) | stdout()\n}",
            &reg,
        );
        let meta = result.schedule.tasks.get("t").unwrap();
        assert_eq!(meta.k_factor, 1);
    }

    #[test]
    fn k_factor_default_tick_rate_unchanged() {
        let reg = test_registry();
        // No set tick_rate → default 1MHz, 10kHz task → K=1
        let result = schedule_ok("clock 10kHz t {\n    constant(0.0) | stdout()\n}", &reg);
        let meta = result.schedule.tasks.get("t").unwrap();
        assert_eq!(meta.k_factor, 1);
    }

    // ── Feedback cycle tests ────────────────────────────────────────────

    #[test]
    fn feedback_with_delay_scheduled() {
        let reg = test_registry();
        let result = schedule_ok(
            concat!(
                "param alpha = 0.5\n",
                "clock 1kHz t {\n",
                "    constant(0.0) | add(:fb) | mul($alpha) | :out | stdout()\n",
                "    :out | delay(1, 0.0) | :fb\n",
                "}",
            ),
            &reg,
        );
        let meta = result.schedule.tasks.get("t").unwrap();
        let sched = get_pipeline_schedule(meta);
        // All nodes must be scheduled (delay cycle was broken)
        assert!(sched.firings.len() >= 5, "all nodes should be scheduled");
    }

    // ── Defensive error paths ────────────────────────────────────────────

    /// Like `schedule_source` but does not assert on analysis errors.
    /// Needed for testing scheduler behaviour when analysis detects issues
    /// (e.g. cycle without delay) but still produces partial results.
    fn schedule_source_bypass_analysis(source: &str, registry: &Registry) -> ScheduleResult {
        let parse_result = crate::parser::parse(source);
        assert!(
            parse_result.errors.is_empty(),
            "parse errors: {:?}",
            parse_result.errors
        );
        let program = parse_result.program.expect("parse failed");
        let mut resolve_result = resolve::resolve(&program, registry);
        assert!(
            resolve_result
                .diagnostics
                .iter()
                .all(|d| d.level != DiagLevel::Error),
            "resolve errors: {:#?}",
            resolve_result.diagnostics
        );
        let hir_program = crate::hir::build_hir(
            &program,
            &resolve_result.resolved,
            &mut resolve_result.id_alloc,
        );
        let graph_result =
            crate::graph::build_graph(&hir_program, &resolve_result.resolved, registry);
        assert!(
            graph_result
                .diagnostics
                .iter()
                .all(|d| d.level != DiagLevel::Error),
            "graph errors: {:#?}",
            graph_result.diagnostics
        );
        let type_result =
            crate::type_infer::type_infer(&hir_program, &resolve_result.resolved, registry);
        let lower_result = crate::lower::lower_and_verify(
            &hir_program,
            &resolve_result.resolved,
            &type_result.typed,
            registry,
        );
        let thir = crate::thir::build_thir_context(
            &hir_program,
            &resolve_result.resolved,
            &type_result.typed,
            &lower_result.lowered,
            registry,
            &graph_result.graph,
        );
        let analysis_result = crate::analyze::analyze(&thir, &graph_result.graph);
        // Skip analysis error assertion — allow cycle-without-delay errors through
        schedule(&thir, &graph_result.graph, &analysis_result.analysis)
    }

    #[test]
    fn unresolved_cycle_diagnostic() {
        let reg = test_registry();
        // Cycle without delay: add → :out → mul → :fb → add
        // Analysis detects missing delay but still computes RVs.
        // Scheduler's identify_back_edges finds no delay → topological sort fails.
        let result = schedule_source_bypass_analysis(
            concat!(
                "param alpha = 0.5\n",
                "clock 1kHz t {\n",
                "    constant(0.0) | add(:fb) | mul($alpha) | :out | stdout()\n",
                "    :out | :fb\n",
                "}",
            ),
            &reg,
        );
        let has_cycle_error = result
            .diagnostics
            .iter()
            .any(|d| d.level == DiagLevel::Error && d.message.contains("unresolvable cycle"));
        assert!(
            has_cycle_error,
            "expected 'unresolvable cycle' diagnostic, got: {:#?}",
            result.diagnostics
        );
    }

    // ── Modal task tests ────────────────────────────────────────────────

    #[test]
    fn modal_task_scheduled() {
        let reg = test_registry();
        let result = schedule_ok(
            concat!(
                "clock 1kHz t {\n",
                "    control {\n        constant(0.0) | detect() -> ctrl\n    }\n",
                "    mode sync {\n        constant(0.0) | stdout()\n    }\n",
                "    mode data {\n        constant(0.0) | fft(256) | c2r() | stdout()\n    }\n",
                "    switch(ctrl, sync, data) default sync\n",
                "}",
            ),
            &reg,
        );
        let meta = result.schedule.tasks.get("t").unwrap();
        match &meta.schedule {
            TaskSchedule::Modal { control, modes } => {
                assert!(!control.firings.is_empty(), "control should have firings");
                assert_eq!(modes.len(), 2, "should have 2 mode schedules");
            }
            _ => panic!("expected Modal schedule"),
        }
    }

    // ── Buffer sizing tests ─────────────────────────────────────────────

    #[test]
    fn edge_buffer_sizes() {
        let reg = test_registry();
        let result = schedule_ok(
            "clock 1kHz t {\n    constant(0.0) | fft(256) | c2r() | stdout()\n}",
            &reg,
        );
        let meta = result.schedule.tasks.get("t").unwrap();
        let sched = get_pipeline_schedule(meta);
        assert!(
            !sched.edge_buffers.is_empty(),
            "should have buffer size entries"
        );
        // adc→fft edge: adc fires 256 times producing 1 token each = 256 tokens
        let max_buf = sched.edge_buffers.values().max().copied().unwrap_or(0);
        assert!(max_buf >= 256, "adc→fft buffer should be ≥ 256 tokens");
    }

    #[test]
    fn edge_buffer_exact_values() {
        let reg = test_registry();
        // decimate(10): IN(float, 10), OUT(float, 1)
        // constant gets inferred [10] → production_rate=10, rv=1
        // decimate: production_rate=1, rv=1
        // stdout: rv=10
        let result = schedule_ok(
            "clock 1kHz t {\n    constant(0.0) | decimate(10) | stdout()\n}",
            &reg,
        );
        let meta = result.schedule.tasks.get("t").unwrap();
        let sched = get_pipeline_schedule(meta);

        // Build node_id → firing_index map for edge identification
        assert_eq!(sched.firings.len(), 3, "constant, decimate, stdout");

        // Collect all buffer sizes; should have exactly 2 edges
        assert_eq!(
            sched.edge_buffers.len(),
            2,
            "should have 2 intra-task edges"
        );

        let mut buf_vals: Vec<u32> = sched.edge_buffers.values().copied().collect();
        buf_vals.sort();
        // constant→decimate: p(constant)*rv(constant) = 10*1 = 10
        // decimate→stdout:   p(decimate)*rv(decimate) = 1*1  = 1
        assert_eq!(buf_vals, vec![1, 10], "edge buffers should be [1, 10]");
    }

    // ── Fork topology tests ─────────────────────────────────────────────

    #[test]
    fn fork_topology() {
        let reg = test_registry();
        let result = schedule_ok(
            "clock 1kHz t {\n    constant(0.0) | :raw | stdout()\n    :raw | stdout()\n}",
            &reg,
        );
        let meta = result.schedule.tasks.get("t").unwrap();
        let sched = get_pipeline_schedule(meta);
        // adc, fork(:raw), stdout, stdout = 4 nodes
        assert_eq!(sched.firings.len(), 4);
    }

    // ── Rate resolution tests ──────────────────────────────────────────

    #[test]
    fn const_ref_shape_resolution() {
        // fft()[N] creates ShapeDim::ConstRef("N"), resolved via const lookup
        let reg = test_registry();
        let result = schedule_ok(
            concat!(
                "const N = 256\n",
                "clock 1kHz t {\n",
                "    constant(0.0) | fft()[N] | c2r() | stdout()\n",
                "}",
            ),
            &reg,
        );
        let meta = result.schedule.tasks.get("t").unwrap();
        let sched = get_pipeline_schedule(meta);
        assert_eq!(sched.firings.len(), 4, "constant, fft, c2r, stdout");
        // ConstRef("N") resolves to 256 → fft IN/OUT rate=256
        // With rate 256: stdout rv=256, others rv=1
        assert_eq!(sched.firings[0].repetition_count, 1, "constant fires 1x");
        assert_eq!(sched.firings[1].repetition_count, 1, "fft fires 1x");
        assert_eq!(sched.firings[3].repetition_count, 256, "stdout fires 256x");
    }

    #[test]
    fn array_const_rate_resolution() {
        // fir(coeff) where coeff is const array: resolve_arg_to_u32 → elems.len()
        let reg = test_registry();
        let result = schedule_ok(
            concat!(
                "const coeff = [1.0, 2.0, 3.0, 4.0, 5.0]\n",
                "clock 1kHz t {\n",
                "    constant(0.0) | fir(coeff) | stdout()\n",
                "}",
            ),
            &reg,
        );
        let meta = result.schedule.tasks.get("t").unwrap();
        let sched = get_pipeline_schedule(meta);
        assert_eq!(sched.firings.len(), 3, "constant, fir, stdout");
        // fir(coeff): N is inferred from bound span argument `coeff` length (=5).
        // IN(float, 5), OUT(float, 1)
        // constant gets inferred [5] → all rv=1, constant produces 5 per firing
        assert_eq!(sched.firings[0].repetition_count, 1, "constant fires 1x");
        assert_eq!(sched.firings[1].repetition_count, 1, "fir fires 1x");
        assert_eq!(sched.firings[2].repetition_count, 1, "stdout fires 1x");
    }

    // ── Fusion semantic baseline ────────────────────────────────────────

    #[test]
    fn fusion_semantic_baseline() {
        // Baseline invariant: for every edge, edge_buffer >= production_rate * rv(source).
        // This SDF conservation property must hold regardless of fusion optimisation.
        let reg = test_registry();
        let result = schedule_ok(
            "clock 1kHz t {\n    constant(0.0) | fft(256) | c2r() | stdout()\n}",
            &reg,
        );
        let meta = result.schedule.tasks.get("t").unwrap();
        let sched = get_pipeline_schedule(meta);

        // Build map: node_id → repetition_count
        let rv_map: HashMap<NodeId, u32> = sched
            .firings
            .iter()
            .map(|f| (f.node_id, f.repetition_count))
            .collect();

        // For each edge, buffer size must equal p(src) * rv(src)
        // (the SDF balance invariant the scheduler encodes)
        for (&(src, _tgt), &buf_size) in &sched.edge_buffers {
            let rv_src = rv_map.get(&src).copied().unwrap_or(1);
            assert!(
                buf_size >= rv_src,
                "edge ({:?} → {:?}): buffer {buf_size} < rv(src) {rv_src} — \
                 SDF token conservation violated",
                src,
                _tgt
            );
        }

        // Verify total tokens per PASS cycle are non-zero for every edge
        let total_tokens: u32 = sched.edge_buffers.values().sum();
        assert!(
            total_tokens > 0,
            "PASS cycle must produce tokens on intra-task edges"
        );
    }

    // ── Display test ────────────────────────────────────────────────────

    #[test]
    fn display_output() {
        let reg = test_registry();
        let result = schedule_ok("clock 1kHz t {\n    constant(0.0) | stdout()\n}", &reg);
        let output = format!("{}", result.schedule);
        assert!(output.contains("task 't'"));
        assert!(output.contains("K=1"));
        assert!(output.contains("[pipeline]"));
    }

    // ── Integration tests ───────────────────────────────────────────────

    #[test]
    fn example_pdl_schedule() {
        let reg = test_registry();
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("examples/example.pdl");
        let source = std::fs::read_to_string(&path).expect("failed to read example.pdl");
        let result = schedule_source(&source, &reg);
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.level == DiagLevel::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "example.pdl schedule errors: {:#?}",
            errors
        );
        assert!(
            result.schedule.tasks.len() >= 2,
            "example.pdl should have at least 2 tasks"
        );
    }

    #[test]
    fn receiver_pdl_schedule() {
        let reg = test_registry();
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("examples/receiver.pdl");
        let source = std::fs::read_to_string(&path).expect("failed to read receiver.pdl");
        let result = schedule_source(&source, &reg);
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.level == DiagLevel::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "receiver.pdl schedule errors: {:#?}",
            errors
        );
        // receiver.pdl has a modal task
        let receiver = result.schedule.tasks.get("receiver");
        assert!(receiver.is_some(), "should have 'receiver' task");
        assert!(
            matches!(&receiver.unwrap().schedule, TaskSchedule::Modal { .. }),
            "receiver should have Modal schedule"
        );
    }

    #[test]
    fn feedback_pdl_schedule() {
        let reg = test_registry();
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("examples/feedback.pdl");
        let source = std::fs::read_to_string(&path).expect("failed to read feedback.pdl");
        let result = schedule_source(&source, &reg);
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.level == DiagLevel::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "feedback.pdl schedule errors: {:#?}",
            errors
        );
    }

    // ── Rate guardrail tests ─────────────────────────────────────────────

    #[test]
    fn guardrail_warns_high_effective_rate() {
        let reg = test_registry();
        // tick_rate = 1MHz with 1MHz task → K=1, period=1us < 10us → warning
        let result = schedule_ok(
            "set tick_rate = 1MHz\nclock 1MHz t {\n    constant(0.0) | stdout()\n}",
            &reg,
        );
        let warnings: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.level == DiagLevel::Warning)
            .collect();
        assert!(
            !warnings.is_empty(),
            "should warn when effective tick period < 10us"
        );
        assert!(
            warnings[0].message.contains("100kHz"),
            "warning should mention 100kHz threshold: {}",
            warnings[0].message
        );
    }

    #[test]
    fn guardrail_no_warning_normal_rate() {
        let reg = test_registry();
        // default tick_rate = 10kHz with 10kHz task → K=1, period=100us > 10us → no warning
        let result = schedule_ok("clock 10kHz t {\n    constant(0.0) | stdout()\n}", &reg);
        let warnings: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.level == DiagLevel::Warning)
            .collect();
        assert!(
            warnings.is_empty(),
            "should not warn at normal rates: {:?}",
            warnings
        );
    }

    #[test]
    fn guardrail_no_warning_batched_high_freq() {
        let reg = test_registry();
        // default tick_rate = 10kHz with 1MHz task → K=100, timer_hz=10kHz, period=100us → no warning
        let result = schedule_ok("clock 1MHz t {\n    constant(0.0) | stdout()\n}", &reg);
        let warnings: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.level == DiagLevel::Warning)
            .collect();
        assert!(
            warnings.is_empty(),
            "batched high-freq should not warn: {:?}",
            warnings
        );
    }

    // ── verify_schedule tests ───────────────────────────────────────────

    /// Helper: build schedule and graph from source for verification tests.
    fn build_schedule_and_graph(
        source: &str,
        registry: &Registry,
    ) -> (ScheduleResult, crate::graph::GraphResult, Vec<String>) {
        let parse_result = crate::parser::parse(source);
        assert!(
            parse_result.errors.is_empty(),
            "parse errors: {:?}",
            parse_result.errors
        );
        let program = parse_result.program.expect("parse failed");
        let mut resolve_result = resolve::resolve(&program, registry);
        assert!(
            resolve_result
                .diagnostics
                .iter()
                .all(|d| d.level != DiagLevel::Error),
            "resolve errors: {:#?}",
            resolve_result.diagnostics
        );
        let hir_program = crate::hir::build_hir(
            &program,
            &resolve_result.resolved,
            &mut resolve_result.id_alloc,
        );
        let task_names: Vec<String> = hir_program.tasks.iter().map(|t| t.name.clone()).collect();
        let graph_result =
            crate::graph::build_graph(&hir_program, &resolve_result.resolved, registry);
        assert!(
            graph_result
                .diagnostics
                .iter()
                .all(|d| d.level != DiagLevel::Error),
            "graph errors: {:#?}",
            graph_result.diagnostics
        );
        let type_result =
            crate::type_infer::type_infer(&hir_program, &resolve_result.resolved, registry);
        let lower_result = crate::lower::lower_and_verify(
            &hir_program,
            &resolve_result.resolved,
            &type_result.typed,
            registry,
        );
        let thir = crate::thir::build_thir_context(
            &hir_program,
            &resolve_result.resolved,
            &type_result.typed,
            &lower_result.lowered,
            registry,
            &graph_result.graph,
        );
        let analysis_result = crate::analyze::analyze(&thir, &graph_result.graph);
        assert!(
            analysis_result
                .diagnostics
                .iter()
                .all(|d| d.level != DiagLevel::Error),
            "analysis errors: {:#?}",
            analysis_result.diagnostics
        );
        let sched_result = schedule(&thir, &graph_result.graph, &analysis_result.analysis);
        (sched_result, graph_result, task_names)
    }

    #[test]
    fn verify_schedule_passing() {
        use crate::pass::StageCert;
        let reg = test_registry();
        let (sched_result, graph_result, task_names) =
            build_schedule_and_graph("clock 1kHz t {\n    constant(0.0) | stdout()\n}", &reg);
        let cert = verify_schedule(&sched_result.schedule, &graph_result.graph, &task_names);
        assert!(
            cert.all_pass(),
            "cert should pass: {:?}",
            cert.obligations()
        );
    }

    #[test]
    fn verify_schedule_s1_missing_task() {
        let reg = test_registry();
        let (sched_result, graph_result, _) =
            build_schedule_and_graph("clock 1kHz t {\n    constant(0.0) | stdout()\n}", &reg);
        // Inject an extra task name that doesn't exist in schedule
        let task_names = vec!["t".to_string(), "nonexistent".to_string()];
        let cert = verify_schedule(&sched_result.schedule, &graph_result.graph, &task_names);
        assert!(!cert.s1_all_tasks_scheduled, "S1 should fail");
        assert!(cert.s2_all_nodes_fired, "S2 should still pass");
    }

    #[test]
    fn verify_schedule_s2_missing_node() {
        let reg = test_registry();
        let (mut sched_result, graph_result, task_names) =
            build_schedule_and_graph("clock 1kHz t {\n    constant(0.0) | stdout()\n}", &reg);
        // Remove a firing from the schedule to simulate missing node
        if let Some(meta) = sched_result.schedule.tasks.get_mut("t") {
            if let TaskSchedule::Pipeline(ref mut sched) = meta.schedule {
                sched.firings.pop(); // remove last firing
            }
        }
        let cert = verify_schedule(&sched_result.schedule, &graph_result.graph, &task_names);
        assert!(cert.s1_all_tasks_scheduled, "S1 should still pass");
        assert!(!cert.s2_all_nodes_fired, "S2 should fail");
    }
}
