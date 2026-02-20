// graph.rs — SDF graph construction for Pipit programs
//
// Transforms the resolved AST into directed dataflow graphs — one per task.
// Handles define inlining, tap fork expansion, shared buffer edges, modal
// task subgraphs, and feedback loop detection.
//
// Preconditions: `program` is a parsed AST; `resolved` has passed name resolution;
//                `registry` contains actor metadata from C++ headers.
// Postconditions: returns a `ProgramGraph` with per-task graphs, inter-task edges,
//                 and detected feedback cycles.
// Failure modes: recursion depth exceeded (define inlining) → `Diagnostic` error.
// Side effects: none.

use std::collections::HashMap;
use std::fmt;

use crate::ast::*;
use crate::registry::Registry;
use crate::resolve::{CallResolution, DiagLevel, Diagnostic, ResolvedProgram};

// ── Public types ────────────────────────────────────────────────────────────

/// Unique identifier for a node within a subgraph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub u32);

/// Unique identifier for an edge within a subgraph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EdgeId(pub u32);

/// The kind of a graph node.
#[derive(Debug, Clone, PartialEq)]
pub enum NodeKind {
    /// An actor call (or inlined define actor).
    Actor {
        name: String,
        call_span: Span,
        args: Vec<Arg>,
        /// Optional shape constraint from `actor(...)[d0, d1, ...]` (v0.2.0).
        shape_constraint: Option<ShapeConstraint>,
    },
    /// A fork node created by a tap declaration (`:name`).
    Fork { tap_name: String },
    /// A probe observation point (`?name`).
    Probe { probe_name: String },
    /// A shared buffer read (`@name`).
    BufferRead { buffer_name: String },
    /// A shared buffer write (`-> name`).
    BufferWrite { buffer_name: String },
}

/// A node in the dataflow graph.
#[derive(Debug, Clone)]
pub struct Node {
    pub id: NodeId,
    pub kind: NodeKind,
    pub span: Span,
}

/// A directed edge between two nodes.
#[derive(Debug, Clone)]
pub struct Edge {
    pub id: EdgeId,
    pub source: NodeId,
    pub target: NodeId,
    pub span: Span,
}

/// A subgraph: a set of nodes and edges forming a single pipeline or mode.
#[derive(Debug, Clone)]
pub struct Subgraph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

/// A task's graph structure.
#[derive(Debug, Clone)]
pub enum TaskGraph {
    Pipeline(Subgraph),
    Modal {
        control: Subgraph,
        modes: Vec<(String, Subgraph)>,
    },
}

/// An edge linking a buffer write in one task to a buffer read in another.
#[derive(Debug, Clone)]
pub struct InterTaskEdge {
    pub buffer_name: String,
    pub writer_task: String,
    pub writer_node: NodeId,
    pub reader_task: String,
    pub reader_node: NodeId,
}

/// The complete program graph.
#[derive(Debug)]
pub struct ProgramGraph {
    pub tasks: HashMap<String, TaskGraph>,
    pub inter_task_edges: Vec<InterTaskEdge>,
    pub cycles: Vec<Vec<NodeId>>,
}

/// Result of graph construction.
#[derive(Debug)]
pub struct GraphResult {
    pub graph: ProgramGraph,
    pub diagnostics: Vec<Diagnostic>,
}

// ── Public entry point ──────────────────────────────────────────────────────

/// Build the SDF graph from a resolved Pipit program.
pub fn build_graph(
    program: &Program,
    resolved: &ResolvedProgram,
    registry: &Registry,
) -> GraphResult {
    let mut builder = GraphBuilder::new(program, resolved, registry);
    builder.build_all_tasks();
    builder.link_inter_task_edges();
    builder.detect_all_cycles();

    GraphResult {
        graph: ProgramGraph {
            tasks: builder.task_graphs,
            inter_task_edges: builder.inter_task_edges,
            cycles: builder.cycles,
        },
        diagnostics: builder.diagnostics,
    }
}

// ── Display ─────────────────────────────────────────────────────────────────

impl fmt::Display for ProgramGraph {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "ProgramGraph ({} tasks)", self.tasks.len())?;
        for (name, graph) in &self.tasks {
            match graph {
                TaskGraph::Pipeline(sub) => {
                    writeln!(
                        f,
                        "  task '{}': {} nodes, {} edges",
                        name,
                        sub.nodes.len(),
                        sub.edges.len()
                    )?;
                }
                TaskGraph::Modal { control, modes } => {
                    writeln!(
                        f,
                        "  task '{}' (modal): control({} nodes, {} edges), {} modes",
                        name,
                        control.nodes.len(),
                        control.edges.len(),
                        modes.len()
                    )?;
                    for (mode_name, sub) in modes {
                        writeln!(
                            f,
                            "    mode '{}': {} nodes, {} edges",
                            mode_name,
                            sub.nodes.len(),
                            sub.edges.len()
                        )?;
                    }
                }
            }
        }
        if !self.inter_task_edges.is_empty() {
            writeln!(f, "  inter-task edges: {}", self.inter_task_edges.len())?;
        }
        if !self.cycles.is_empty() {
            writeln!(f, "  feedback cycles: {}", self.cycles.len())?;
        }
        Ok(())
    }
}

// ── Internal builder ────────────────────────────────────────────────────────

const MAX_INLINE_DEPTH: u32 = 16;

struct GraphBuilder<'a> {
    program: &'a Program,
    resolved: &'a ResolvedProgram,
    _registry: &'a Registry,
    task_graphs: HashMap<String, TaskGraph>,
    inter_task_edges: Vec<InterTaskEdge>,
    cycles: Vec<Vec<NodeId>>,
    diagnostics: Vec<Diagnostic>,
    /// Global counters to ensure unique NodeId/EdgeId across all subgraphs.
    next_global_node_id: u32,
    next_global_edge_id: u32,
}

/// Context for building a single subgraph.
struct SubgraphCtx {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    next_node_id: u32,
    next_edge_id: u32,
    /// Map from tap name to the ForkNode id.
    taps: HashMap<String, NodeId>,
    /// Map from buffer write name to the BufferWrite node id.
    buffer_writes: HashMap<String, NodeId>,
    /// Map from buffer read name to the BufferRead node id.
    buffer_reads: HashMap<String, NodeId>,
    /// Deferred tap-input edges: (actor_node_id, tap_name, span).
    /// Resolved after all lines are processed (supports forward references).
    pending_tap_inputs: Vec<(NodeId, String, Span)>,
}

impl SubgraphCtx {
    fn new(start_node_id: u32, start_edge_id: u32) -> Self {
        SubgraphCtx {
            nodes: Vec::new(),
            edges: Vec::new(),
            next_node_id: start_node_id,
            next_edge_id: start_edge_id,
            taps: HashMap::new(),
            buffer_writes: HashMap::new(),
            buffer_reads: HashMap::new(),
            pending_tap_inputs: Vec::new(),
        }
    }

    fn add_node(&mut self, kind: NodeKind, span: Span) -> NodeId {
        let id = NodeId(self.next_node_id);
        self.next_node_id += 1;
        self.nodes.push(Node { id, kind, span });
        id
    }

    fn add_edge(&mut self, source: NodeId, target: NodeId, span: Span) -> EdgeId {
        let id = EdgeId(self.next_edge_id);
        self.next_edge_id += 1;
        self.edges.push(Edge {
            id,
            source,
            target,
            span,
        });
        id
    }

    fn into_subgraph(self) -> Subgraph {
        Subgraph {
            nodes: self.nodes,
            edges: self.edges,
        }
    }
}

/// Entry and exit nodes of an inlined define or a single actor.
/// For a single actor, entry == exit.
/// For an inlined define, entry is the first node, exit is the last.
struct InlineResult {
    entry: Option<NodeId>,
    exit: Option<NodeId>,
}

impl InlineResult {
    fn single(id: NodeId) -> Self {
        InlineResult {
            entry: Some(id),
            exit: Some(id),
        }
    }

    fn none() -> Self {
        InlineResult {
            entry: None,
            exit: None,
        }
    }
}

impl<'a> GraphBuilder<'a> {
    fn new(program: &'a Program, resolved: &'a ResolvedProgram, registry: &'a Registry) -> Self {
        GraphBuilder {
            program,
            resolved,
            _registry: registry,
            task_graphs: HashMap::new(),
            inter_task_edges: Vec::new(),
            cycles: Vec::new(),
            diagnostics: Vec::new(),
            next_global_node_id: 0,
            next_global_edge_id: 0,
        }
    }

    fn error(&mut self, span: Span, message: String) {
        self.diagnostics.push(Diagnostic {
            level: DiagLevel::Error,
            span,
            message,
            hint: None,
        });
    }

    // ── Build all tasks ─────────────────────────────────────────────────

    fn build_all_tasks(&mut self) {
        for stmt in &self.program.statements {
            if let StatementKind::Task(task) = &stmt.kind {
                let task_name = task.name.name.clone();
                let task_graph = self.build_task(task);
                self.task_graphs.insert(task_name, task_graph);
            }
        }
    }

    fn build_task(&mut self, task: &TaskStmt) -> TaskGraph {
        match &task.body {
            TaskBody::Pipeline(body) => {
                let sub = self.build_subgraph(body);
                TaskGraph::Pipeline(sub)
            }
            TaskBody::Modal(modal) => {
                let control = self.build_subgraph(&modal.control.body);
                let mut modes = Vec::new();
                for mode in &modal.modes {
                    let sub = self.build_subgraph(&mode.body);
                    modes.push((mode.name.name.clone(), sub));
                }
                TaskGraph::Modal { control, modes }
            }
        }
    }

    fn build_subgraph(&mut self, body: &PipelineBody) -> Subgraph {
        let mut ctx = SubgraphCtx::new(self.next_global_node_id, self.next_global_edge_id);

        for line in &body.lines {
            self.build_pipe_expr(line, &mut ctx, 0);
        }

        // Post-pass: resolve deferred tap-input edges (for feedback loops)
        self.resolve_pending_tap_inputs(&mut ctx);

        // Update global counters so next subgraph gets unique IDs
        self.next_global_node_id = ctx.next_node_id;
        self.next_global_edge_id = ctx.next_edge_id;

        ctx.into_subgraph()
    }

    // ── Build a single pipe expression ──────────────────────────────────

    fn build_pipe_expr(
        &mut self,
        expr: &PipeExpr,
        ctx: &mut SubgraphCtx,
        inline_depth: u32,
    ) -> Option<NodeId> {
        // Source — use the exit node as prev (for source, entry == exit for actors)
        let source_result = match &expr.source {
            PipeSource::ActorCall(call) => {
                self.build_actor_or_inline(call, ctx, expr.span, inline_depth)
            }
            PipeSource::BufferRead(ident) => {
                let id = ctx.add_node(
                    NodeKind::BufferRead {
                        buffer_name: ident.name.clone(),
                    },
                    ident.span,
                );
                ctx.buffer_reads.entry(ident.name.clone()).or_insert(id);
                InlineResult::single(id)
            }
            PipeSource::TapRef(ident) => {
                // Look up the fork node for this tap
                let id = ctx.taps.get(&ident.name).copied();
                InlineResult {
                    entry: id,
                    exit: id,
                }
            }
        };
        let mut prev_node = source_result.exit;

        // Elements
        for elem in &expr.elements {
            match elem {
                PipeElem::ActorCall(call) => {
                    let result = self.build_actor_or_inline(call, ctx, expr.span, inline_depth);
                    // Connect prev to entry of inlined define (or the single actor)
                    if let (Some(prev), Some(entry)) = (prev_node, result.entry) {
                        ctx.add_edge(prev, entry, expr.span);
                    }
                    // Continue chain from exit
                    prev_node = result.exit;
                }
                PipeElem::Tap(ident) => {
                    let fork_id = ctx.add_node(
                        NodeKind::Fork {
                            tap_name: ident.name.clone(),
                        },
                        ident.span,
                    );
                    if let Some(prev) = prev_node {
                        ctx.add_edge(prev, fork_id, ident.span);
                    }
                    ctx.taps.insert(ident.name.clone(), fork_id);
                    prev_node = Some(fork_id);
                }
                PipeElem::Probe(ident) => {
                    let probe_id = ctx.add_node(
                        NodeKind::Probe {
                            probe_name: ident.name.clone(),
                        },
                        ident.span,
                    );
                    if let Some(prev) = prev_node {
                        ctx.add_edge(prev, probe_id, ident.span);
                    }
                    prev_node = Some(probe_id);
                }
            }
        }

        // Sink
        if let Some(sink) = &expr.sink {
            let write_id = ctx.add_node(
                NodeKind::BufferWrite {
                    buffer_name: sink.buffer.name.clone(),
                },
                sink.span,
            );
            if let Some(prev) = prev_node {
                ctx.add_edge(prev, write_id, sink.span);
            }
            ctx.buffer_writes.insert(sink.buffer.name.clone(), write_id);
            prev_node = Some(write_id);
        }

        prev_node
    }

    // ── Actor or define inlining ────────────────────────────────────────

    fn build_actor_or_inline(
        &mut self,
        call: &ActorCall,
        ctx: &mut SubgraphCtx,
        pipe_span: Span,
        inline_depth: u32,
    ) -> InlineResult {
        // Check if this call resolves to a define
        if let Some(CallResolution::Define) = self.resolved.call_resolutions.get(&call.span) {
            return self.inline_define(call, ctx, pipe_span, inline_depth);
        }

        // Regular actor node
        let id = ctx.add_node(
            NodeKind::Actor {
                name: call.name.name.clone(),
                call_span: call.span,
                args: call.args.clone(),
                shape_constraint: call.shape_constraint.clone(),
            },
            call.span,
        );

        // Process tap-ref args: create additional incoming edges from fork nodes
        for arg in &call.args {
            if let Arg::TapRef(ident) = arg {
                if let Some(&fork_id) = ctx.taps.get(&ident.name) {
                    ctx.add_edge(fork_id, id, ident.span);
                } else {
                    // Forward reference — defer until all lines processed
                    ctx.pending_tap_inputs
                        .push((id, ident.name.clone(), ident.span));
                }
            }
        }

        InlineResult::single(id)
    }

    fn inline_define(
        &mut self,
        call: &ActorCall,
        ctx: &mut SubgraphCtx,
        _pipe_span: Span,
        inline_depth: u32,
    ) -> InlineResult {
        if inline_depth >= MAX_INLINE_DEPTH {
            self.error(
                call.span,
                format!(
                    "define '{}' inlining exceeds maximum depth ({})",
                    call.name.name, MAX_INLINE_DEPTH
                ),
            );
            return InlineResult::none();
        }

        let define_entry = match self.resolved.defines.get(&call.name.name) {
            Some(e) => e.clone(),
            None => return InlineResult::none(),
        };

        // Get the DefineStmt from the program
        let define_stmt = match &self.program.statements[define_entry.stmt_index].kind {
            StatementKind::Define(d) => d,
            _ => return InlineResult::none(),
        };

        // Build argument substitution map: formal param name -> actual arg
        let arg_map: HashMap<String, Arg> = define_entry
            .param_names
            .iter()
            .zip(call.args.iter())
            .map(|(name, arg)| (name.clone(), arg.clone()))
            .collect();

        // Save tap scope — taps inside defines are scoped to the expansion
        let saved_taps = ctx.taps.clone();

        // Track which node is added first (entry point of the inlined body)
        let node_count_before = ctx.nodes.len();

        // Inline each line of the define body
        let mut last_node: Option<NodeId> = None;

        for line in &define_stmt.body.lines {
            let substituted = substitute_pipe_expr(line, &arg_map);
            let line_result = self.build_pipe_expr(&substituted, ctx, inline_depth + 1);

            if line_result.is_some() {
                last_node = line_result;
            }
        }

        // The entry node is the first node added during inlining
        let entry_node = if ctx.nodes.len() > node_count_before {
            Some(ctx.nodes[node_count_before].id)
        } else {
            None
        };

        // Resolve pending tap inputs referencing define-local taps
        // (before restoring outer tap scope)
        let mut remaining = Vec::new();
        for entry in std::mem::take(&mut ctx.pending_tap_inputs) {
            if let Some(&fork_id) = ctx.taps.get(&entry.1) {
                ctx.add_edge(fork_id, entry.0, entry.2);
            } else {
                remaining.push(entry);
            }
        }
        ctx.pending_tap_inputs = remaining;

        // Restore tap scope
        ctx.taps = saved_taps;

        InlineResult {
            entry: entry_node,
            exit: last_node,
        }
    }

    // ── Pending tap-input resolution ────────────────────────────────────

    fn resolve_pending_tap_inputs(&mut self, ctx: &mut SubgraphCtx) {
        for (actor_id, tap_name, span) in std::mem::take(&mut ctx.pending_tap_inputs) {
            if let Some(&fork_id) = ctx.taps.get(&tap_name) {
                ctx.add_edge(fork_id, actor_id, span);
            } else {
                self.error(
                    span,
                    format!("internal error: tap ':{tap_name}' not found in graph"),
                );
            }
        }
    }

    // ── Inter-task edge linking ─────────────────────────────────────────

    fn link_inter_task_edges(&mut self) {
        for (buf_name, buf_info) in &self.resolved.buffers {
            let writer_node = self.find_buffer_write_node(&buf_info.writer_task, buf_name);
            for (reader_task, _) in &buf_info.readers {
                let reader_node = self.find_buffer_read_node(reader_task, buf_name);
                if let (Some(w_node), Some(r_node)) = (writer_node, reader_node) {
                    self.inter_task_edges.push(InterTaskEdge {
                        buffer_name: buf_name.clone(),
                        writer_task: buf_info.writer_task.clone(),
                        writer_node: w_node,
                        reader_task: reader_task.clone(),
                        reader_node: r_node,
                    });
                }
            }
        }
    }

    fn find_buffer_write_node(&self, task_name: &str, buffer_name: &str) -> Option<NodeId> {
        let task_graph = self.task_graphs.get(task_name)?;
        find_buffer_node_in_task(task_graph, buffer_name, true)
    }

    fn find_buffer_read_node(&self, task_name: &str, buffer_name: &str) -> Option<NodeId> {
        let task_graph = self.task_graphs.get(task_name)?;
        find_buffer_node_in_task(task_graph, buffer_name, false)
    }

    // ── Cycle detection ─────────────────────────────────────────────────

    fn detect_all_cycles(&mut self) {
        for task_graph in self.task_graphs.values() {
            match task_graph {
                TaskGraph::Pipeline(sub) => {
                    let mut cycles = detect_cycles_in_subgraph(sub);
                    self.cycles.append(&mut cycles);
                }
                TaskGraph::Modal { control, modes } => {
                    let mut cycles = detect_cycles_in_subgraph(control);
                    self.cycles.append(&mut cycles);
                    for (_, sub) in modes {
                        let mut mode_cycles = detect_cycles_in_subgraph(sub);
                        self.cycles.append(&mut mode_cycles);
                    }
                }
            }
        }
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn find_buffer_node_in_task(
    task_graph: &TaskGraph,
    buffer_name: &str,
    is_write: bool,
) -> Option<NodeId> {
    let subgraphs: Vec<&Subgraph> = match task_graph {
        TaskGraph::Pipeline(sub) => vec![sub],
        TaskGraph::Modal { control, modes } => {
            let mut subs = vec![control];
            for (_, sub) in modes {
                subs.push(sub);
            }
            subs
        }
    };

    for sub in subgraphs {
        for node in &sub.nodes {
            match &node.kind {
                NodeKind::BufferWrite {
                    buffer_name: name, ..
                } if is_write && name == buffer_name => {
                    return Some(node.id);
                }
                NodeKind::BufferRead {
                    buffer_name: name, ..
                } if !is_write && name == buffer_name => {
                    return Some(node.id);
                }
                _ => {}
            }
        }
    }
    None
}

/// Substitute formal parameters with actual arguments in a PipeExpr.
fn substitute_pipe_expr(expr: &PipeExpr, arg_map: &HashMap<String, Arg>) -> PipeExpr {
    PipeExpr {
        source: substitute_source(&expr.source, arg_map),
        elements: expr
            .elements
            .iter()
            .map(|e| substitute_elem(e, arg_map))
            .collect(),
        sink: expr.sink.clone(),
        span: expr.span,
    }
}

fn substitute_source(source: &PipeSource, arg_map: &HashMap<String, Arg>) -> PipeSource {
    match source {
        PipeSource::ActorCall(call) => PipeSource::ActorCall(substitute_actor_call(call, arg_map)),
        other => other.clone(),
    }
}

fn substitute_elem(elem: &PipeElem, arg_map: &HashMap<String, Arg>) -> PipeElem {
    match elem {
        PipeElem::ActorCall(call) => PipeElem::ActorCall(substitute_actor_call(call, arg_map)),
        other => other.clone(),
    }
}

fn substitute_actor_call(call: &ActorCall, arg_map: &HashMap<String, Arg>) -> ActorCall {
    ActorCall {
        name: call.name.clone(),
        type_args: call.type_args.clone(),
        args: call
            .args
            .iter()
            .map(|arg| substitute_arg(arg, arg_map))
            .collect(),
        shape_constraint: call.shape_constraint.clone(),
        span: call.span,
    }
}

fn substitute_arg(arg: &Arg, arg_map: &HashMap<String, Arg>) -> Arg {
    match arg {
        Arg::ConstRef(ident) => {
            if let Some(replacement) = arg_map.get(&ident.name) {
                replacement.clone()
            } else {
                arg.clone()
            }
        }
        other => other.clone(),
    }
}

/// Detect cycles in a subgraph using DFS. Returns all cycles found.
fn detect_cycles_in_subgraph(sub: &Subgraph) -> Vec<Vec<NodeId>> {
    if sub.nodes.is_empty() {
        return Vec::new();
    }

    // Build adjacency list
    let mut adj: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
    for node in &sub.nodes {
        adj.entry(node.id).or_default();
    }
    for edge in &sub.edges {
        adj.entry(edge.source).or_default().push(edge.target);
    }

    let mut cycles = Vec::new();
    let mut visited = HashMap::new(); // 0 = unvisited, 1 = in progress, 2 = done
    let mut path = Vec::new();

    for node in &sub.nodes {
        if *visited.get(&node.id).unwrap_or(&0) == 0 {
            dfs_cycle(node.id, &adj, &mut visited, &mut path, &mut cycles);
        }
    }

    cycles
}

fn dfs_cycle(
    node: NodeId,
    adj: &HashMap<NodeId, Vec<NodeId>>,
    visited: &mut HashMap<NodeId, u8>,
    path: &mut Vec<NodeId>,
    cycles: &mut Vec<Vec<NodeId>>,
) {
    visited.insert(node, 1); // in progress
    path.push(node);

    if let Some(neighbors) = adj.get(&node) {
        for &next in neighbors {
            match visited.get(&next).unwrap_or(&0) {
                0 => {
                    dfs_cycle(next, adj, visited, path, cycles);
                }
                1 => {
                    // Found a cycle — extract the cycle from path
                    if let Some(pos) = path.iter().position(|&n| n == next) {
                        let cycle: Vec<NodeId> = path[pos..].to_vec();
                        cycles.push(cycle);
                    }
                }
                _ => {} // already done
            }
        }
    }

    path.pop();
    visited.insert(node, 2); // done
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::Registry;
    use crate::resolve;
    use std::path::PathBuf;

    /// Parse, resolve, and build graph from source with given registry.
    fn build_source(source: &str, registry: &Registry) -> GraphResult {
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
        build_graph(&program, &resolve_result.resolved, registry)
    }

    /// Build graph expecting no errors.
    fn build_ok(source: &str, registry: &Registry) -> ProgramGraph {
        let result = build_source(source, registry);
        assert!(
            result
                .diagnostics
                .iter()
                .all(|d| d.level != DiagLevel::Error),
            "unexpected graph errors: {:#?}",
            result.diagnostics
        );
        result.graph
    }

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

    fn get_pipeline_subgraph<'a>(graph: &'a ProgramGraph, task: &str) -> &'a Subgraph {
        match graph.tasks.get(task).expect("task not found") {
            TaskGraph::Pipeline(sub) => sub,
            TaskGraph::Modal { .. } => panic!("expected Pipeline, got Modal"),
        }
    }

    fn count_nodes_of_kind(sub: &Subgraph, pred: impl Fn(&NodeKind) -> bool) -> usize {
        sub.nodes.iter().filter(|n| pred(&n.kind)).count()
    }

    // ── Basic graph building ────────────────────────────────────────────

    #[test]
    fn single_actor_pipeline() {
        let reg = test_registry();
        let graph = build_ok("clock 1kHz t {\n    constant(0.0) | fft(256)\n}", &reg);
        let sub = get_pipeline_subgraph(&graph, "t");
        assert_eq!(sub.nodes.len(), 2);
        assert_eq!(sub.edges.len(), 1);
    }

    #[test]
    fn three_actor_chain() {
        let reg = test_registry();
        let graph = build_ok(
            "clock 1kHz t {\n    constant(0.0) | fft(256) | mag()\n}",
            &reg,
        );
        let sub = get_pipeline_subgraph(&graph, "t");
        assert_eq!(sub.nodes.len(), 3);
        assert_eq!(sub.edges.len(), 2);
    }

    #[test]
    fn source_only_pipeline() {
        let reg = test_registry();
        let graph = build_ok("clock 1kHz t {\n    constant(0.0)\n}", &reg);
        let sub = get_pipeline_subgraph(&graph, "t");
        assert_eq!(sub.nodes.len(), 1);
        assert_eq!(sub.edges.len(), 0);
    }

    // ── Tap fork expansion ──────────────────────────────────────────────

    #[test]
    fn tap_creates_fork_node() {
        let reg = test_registry();
        let graph = build_ok(
            "clock 1kHz t {\n    constant(0.0) | :raw | stdout()\n    :raw | stdout()\n}",
            &reg,
        );
        let sub = get_pipeline_subgraph(&graph, "t");
        let forks = count_nodes_of_kind(sub, |k| matches!(k, NodeKind::Fork { .. }));
        assert_eq!(forks, 1);
    }

    #[test]
    fn tap_consumed_creates_output_edge() {
        let reg = test_registry();
        let graph = build_ok(
            "clock 1kHz t {\n    constant(0.0) | :raw | stdout()\n    :raw | stdout()\n}",
            &reg,
        );
        let sub = get_pipeline_subgraph(&graph, "t");
        // Find fork node
        let fork = sub
            .nodes
            .iter()
            .find(|n| matches!(&n.kind, NodeKind::Fork { tap_name } if tap_name == "raw"))
            .expect("fork node not found");
        // Count edges from fork
        let edges_from_fork = sub.edges.iter().filter(|e| e.source == fork.id).count();
        // Fork should have 2 outgoing edges: one to stdout on line 1, one to stdout on line 2
        assert_eq!(edges_from_fork, 2);
    }

    #[test]
    fn tap_multiple_consumers() {
        let reg = test_registry();
        let graph = build_ok(
            concat!(
                "clock 1kHz t {\n",
                "    constant(0.0) | :raw | stdout()\n",
                "    :raw | mag() | stdout()\n",
                "    :raw | stdout()\n",
                "}"
            ),
            &reg,
        );
        let sub = get_pipeline_subgraph(&graph, "t");
        let fork = sub
            .nodes
            .iter()
            .find(|n| matches!(&n.kind, NodeKind::Fork { tap_name } if tap_name == "raw"))
            .expect("fork node not found");
        let edges_from_fork = sub.edges.iter().filter(|e| e.source == fork.id).count();
        // 3 consumers: stdout on line 1, mag on line 2, stdout on line 3
        assert_eq!(edges_from_fork, 3);
    }

    // ── Shared buffers ──────────────────────────────────────────────────

    #[test]
    fn buffer_write_creates_node() {
        let reg = test_registry();
        let graph = build_ok("clock 1kHz t {\n    constant(0.0) -> sig\n}", &reg);
        let sub = get_pipeline_subgraph(&graph, "t");
        let writes = count_nodes_of_kind(
            sub,
            |k| matches!(k, NodeKind::BufferWrite { buffer_name } if buffer_name == "sig"),
        );
        assert_eq!(writes, 1);
    }

    #[test]
    fn buffer_read_creates_node() {
        let reg = test_registry();
        let graph = build_ok(
            "clock 1kHz a {\n    constant(0.0) -> sig\n}\nclock 1kHz b {\n    @sig | stdout()\n}",
            &reg,
        );
        let sub = get_pipeline_subgraph(&graph, "b");
        let reads = count_nodes_of_kind(
            sub,
            |k| matches!(k, NodeKind::BufferRead { buffer_name } if buffer_name == "sig"),
        );
        assert_eq!(reads, 1);
    }

    #[test]
    fn inter_task_edge_linked() {
        let reg = test_registry();
        let graph = build_ok(
            "clock 1kHz a {\n    constant(0.0) -> sig\n}\nclock 1kHz b {\n    @sig | stdout()\n}",
            &reg,
        );
        assert_eq!(graph.inter_task_edges.len(), 1);
        let edge = &graph.inter_task_edges[0];
        assert_eq!(edge.buffer_name, "sig");
        assert_eq!(edge.writer_task, "a");
        assert_eq!(edge.reader_task, "b");
    }

    #[test]
    fn inter_task_edges_multiple_readers() {
        let reg = test_registry();
        let graph = build_ok(
            concat!(
                "set mem = 64MB\n",
                "clock 1kHz a { constant(0.0) -> sig }\n",
                "clock 1kHz b { @sig | stdout() }\n",
                "clock 1kHz c { @sig | stdout() }\n",
            ),
            &reg,
        );
        assert_eq!(
            graph.inter_task_edges.len(),
            2,
            "1 writer + 2 readers should produce 2 inter-task edges"
        );
        // Both edges reference the same buffer and writer
        assert!(graph
            .inter_task_edges
            .iter()
            .all(|e| e.buffer_name == "sig"));
        assert!(graph.inter_task_edges.iter().all(|e| e.writer_task == "a"));
        // Reader tasks should be b and c
        let reader_tasks: Vec<&str> = graph
            .inter_task_edges
            .iter()
            .map(|e| e.reader_task.as_str())
            .collect();
        assert!(reader_tasks.contains(&"b"));
        assert!(reader_tasks.contains(&"c"));
    }

    #[test]
    fn inter_task_edge_in_modal_task() {
        let reg = test_registry();
        let graph = build_ok(
            concat!(
                "set mem = 64MB\n",
                "clock 1kHz producer { constant(0.0) -> sig }\n",
                "clock 1kHz consumer {\n",
                "    control { constant(0.0) | detect() -> ctrl }\n",
                "    mode sync { @sig | stdout() }\n",
                "    mode data { constant(0.0) | stdout() }\n",
                "    switch(ctrl, sync, data) default sync\n",
                "}\n",
            ),
            &reg,
        );
        // find_buffer_node_in_task traverses modal subgraphs to find BufferRead
        let sig_edges: Vec<_> = graph
            .inter_task_edges
            .iter()
            .filter(|e| e.buffer_name == "sig")
            .collect();
        assert_eq!(
            sig_edges.len(),
            1,
            "modal task with @sig should produce 1 inter-task edge: {:?}",
            graph.inter_task_edges
        );
        assert_eq!(sig_edges[0].reader_task, "consumer");
    }

    // ── Probes ──────────────────────────────────────────────────────────

    #[test]
    fn probe_creates_passthrough_node() {
        let reg = test_registry();
        let graph = build_ok(
            "clock 1kHz t {\n    constant(0.0) | ?mon | stdout()\n}",
            &reg,
        );
        let sub = get_pipeline_subgraph(&graph, "t");
        let probes = count_nodes_of_kind(
            sub,
            |k| matches!(k, NodeKind::Probe { probe_name } if probe_name == "mon"),
        );
        assert_eq!(probes, 1);
        // Should be in chain: adc -> probe -> stdout (3 nodes, 2 edges)
        assert_eq!(sub.nodes.len(), 3);
        assert_eq!(sub.edges.len(), 2);
    }

    // ── Define inlining ─────────────────────────────────────────────────

    #[test]
    fn define_inlined_as_actors() {
        let reg = test_registry();
        let graph = build_ok(
            "define foo() {\n    constant(0.0) | mag()\n}\nclock 1kHz t {\n    foo() | stdout()\n}",
            &reg,
        );
        let sub = get_pipeline_subgraph(&graph, "t");
        // foo() expands to adc + mag, then piped to stdout => 3 actor nodes
        let actors: Vec<_> = sub
            .nodes
            .iter()
            .filter(|n| matches!(&n.kind, NodeKind::Actor { .. }))
            .collect();
        assert_eq!(actors.len(), 3);
        // Check actor names
        let names: Vec<&str> = actors
            .iter()
            .map(|n| match &n.kind {
                NodeKind::Actor { name, .. } => name.as_str(),
                _ => unreachable!(),
            })
            .collect();
        assert!(names.contains(&"constant"));
        assert!(names.contains(&"mag"));
        assert!(names.contains(&"stdout"));
    }

    #[test]
    fn define_with_args_substituted() {
        let reg = test_registry();
        let graph = build_ok(
            "define foo(n) {\n    fft(n)\n}\nclock 1kHz t {\n    constant(0.0) | foo(256)\n}",
            &reg,
        );
        let sub = get_pipeline_subgraph(&graph, "t");
        // adc + fft (inlined) => 2 actor nodes
        assert_eq!(sub.nodes.len(), 2);
        // Check that fft got the substituted arg (256, not 'n')
        let fft_node = sub
            .nodes
            .iter()
            .find(|n| matches!(&n.kind, NodeKind::Actor { name, .. } if name == "fft"))
            .expect("fft node not found");
        if let NodeKind::Actor { args, .. } = &fft_node.kind {
            assert_eq!(args.len(), 1);
            // The arg should be Value (number 256), not ConstRef("n")
            assert!(
                matches!(&args[0], Arg::Value(_)),
                "expected substituted arg, got: {:?}",
                args[0]
            );
        }
    }

    #[test]
    fn nested_define_inlined() {
        let reg = test_registry();
        let graph = build_ok(
            concat!(
                "define inner() {\n    mag()\n}\n",
                "define outer() {\n    inner() | stdout()\n}\n",
                "clock 1kHz t {\n    constant(0.0) | outer()\n}",
            ),
            &reg,
        );
        let sub = get_pipeline_subgraph(&graph, "t");
        // adc | (mag | stdout) => 3 actor nodes
        let actors: Vec<_> = sub
            .nodes
            .iter()
            .filter(|n| matches!(&n.kind, NodeKind::Actor { .. }))
            .collect();
        assert_eq!(actors.len(), 3);
    }

    #[test]
    fn recursion_depth_error() {
        let reg = test_registry();
        // Create mutually recursive defines by having a define call itself
        // Since name resolution doesn't catch this (it resolves to Define), graph building should.
        // However, we need to trick resolve: define "a" calls "b" which calls "a"
        // Both resolve to CallResolution::Define.
        let source = concat!(
            "define a() {\n    b()\n}\n",
            "define b() {\n    a()\n}\n",
            "clock 1kHz t {\n    a()\n}",
        );
        let parse_result = crate::parser::parse(source);
        assert!(parse_result.errors.is_empty());
        let program = parse_result.program.unwrap();
        let resolve_result = resolve::resolve(&program, &reg);
        // resolve should report "unknown actor or define 'b'" since there are no actors named b
        // and we need b to be known as a define. Actually "a" calls "b" and "b" calls "a" —
        // both are defines, so resolve should work. But resolve also checks that actor calls
        // match registry or defines. "b" is a define, "a" is a define. Both resolve fine.

        // However, at graph building time, mutual recursion will exceed depth.
        let result = build_graph(&program, &resolve_result.resolved, &reg);
        let has_depth_error = result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("exceeds maximum depth"));
        assert!(
            has_depth_error,
            "expected recursion depth error, got: {:#?}",
            result.diagnostics
        );
    }

    // ── Defensive error paths ────────────────────────────────────────────

    #[test]
    fn pending_tap_unresolved_diagnostic() {
        let reg = test_registry();
        // :nonexistent is never declared — resolve catches it first,
        // but graph layer should also emit its own defensive diagnostic.
        let source = "clock 1kHz t {\n    constant(0.0) | add(:nonexistent) | stdout()\n}";
        let parse_result = crate::parser::parse(source);
        assert!(parse_result.errors.is_empty());
        let program = parse_result.program.unwrap();
        let resolve_result = resolve::resolve(&program, &reg);
        // Skip resolve error assertion — resolve catches the undefined tap before graph does
        let result = build_graph(&program, &resolve_result.resolved, &reg);
        let has_graph_error = result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("not found in graph"));
        assert!(
            has_graph_error,
            "expected graph-level 'not found in graph' diagnostic, got: {:#?}",
            result.diagnostics
        );
    }

    // ── Modal tasks ─────────────────────────────────────────────────────

    #[test]
    fn modal_control_subgraph() {
        let reg = test_registry();
        let graph = build_ok(
            concat!(
                "clock 1kHz t {\n",
                "    control {\n        constant(0.0) | detect() -> ctrl\n    }\n",
                "    mode sync {\n        constant(0.0) | stdout()\n    }\n",
                "    mode data {\n        constant(0.0) | stdout()\n    }\n",
                "    switch(ctrl, sync, data) default sync\n",
                "}",
            ),
            &reg,
        );
        match graph.tasks.get("t").unwrap() {
            TaskGraph::Modal { control, .. } => {
                // control: adc | detect | -> ctrl = 3 nodes
                assert_eq!(control.nodes.len(), 3);
                assert_eq!(control.edges.len(), 2);
            }
            _ => panic!("expected Modal"),
        }
    }

    #[test]
    fn modal_mode_subgraphs() {
        let reg = test_registry();
        let graph = build_ok(
            concat!(
                "clock 1kHz t {\n",
                "    control {\n        constant(0.0) | detect() -> ctrl\n    }\n",
                "    mode sync {\n        constant(0.0) | stdout()\n    }\n",
                "    mode data {\n        constant(0.0) | fft(256) | stdout()\n    }\n",
                "    switch(ctrl, sync, data) default sync\n",
                "}",
            ),
            &reg,
        );
        match graph.tasks.get("t").unwrap() {
            TaskGraph::Modal { modes, .. } => {
                assert_eq!(modes.len(), 2);
                let sync = modes.iter().find(|(n, _)| n == "sync").unwrap();
                assert_eq!(sync.1.nodes.len(), 2); // adc | stdout
                let data = modes.iter().find(|(n, _)| n == "data").unwrap();
                assert_eq!(data.1.nodes.len(), 3); // adc | fft | stdout
            }
            _ => panic!("expected Modal"),
        }
    }

    #[test]
    fn modal_graph_structure() {
        let reg = test_registry();
        let graph = build_ok(
            concat!(
                "clock 1kHz t {\n",
                "    control {\n        constant(0.0) | detect() -> ctrl\n    }\n",
                "    mode a {\n        constant(0.0) | stdout()\n    }\n",
                "    mode b {\n        constant(0.0) | stdout()\n    }\n",
                "    switch(ctrl, a, b) default a\n",
                "}",
            ),
            &reg,
        );
        assert!(matches!(
            graph.tasks.get("t").unwrap(),
            TaskGraph::Modal { .. }
        ));
    }

    // ── Cycle detection ─────────────────────────────────────────────────

    #[test]
    fn no_cycle_linear() {
        let reg = test_registry();
        let graph = build_ok(
            "clock 1kHz t {\n    constant(0.0) | fft(256) | stdout()\n}",
            &reg,
        );
        assert!(graph.cycles.is_empty());
    }

    #[test]
    fn feedback_loop_detected() {
        // Manually construct a subgraph with a cycle to verify detection.
        // In Pipit programs each actor call creates a distinct node, so
        // cycles appear when the graph topology is explicitly wired.
        fn sp(start: usize, end: usize) -> Span {
            use chumsky::span::Span;
            Span::new((), start..end)
        }
        let sub = Subgraph {
            nodes: vec![
                Node {
                    id: NodeId(0),
                    kind: NodeKind::Actor {
                        name: "a".into(),
                        call_span: sp(0, 1),
                        args: vec![],
                        shape_constraint: None,
                    },
                    span: sp(0, 1),
                },
                Node {
                    id: NodeId(1),
                    kind: NodeKind::Actor {
                        name: "b".into(),
                        call_span: sp(2, 3),
                        args: vec![],
                        shape_constraint: None,
                    },
                    span: sp(2, 3),
                },
                Node {
                    id: NodeId(2),
                    kind: NodeKind::Actor {
                        name: "c".into(),
                        call_span: sp(4, 5),
                        args: vec![],
                        shape_constraint: None,
                    },
                    span: sp(4, 5),
                },
            ],
            edges: vec![
                Edge {
                    id: EdgeId(0),
                    source: NodeId(0),
                    target: NodeId(1),
                    span: sp(0, 1),
                },
                Edge {
                    id: EdgeId(1),
                    source: NodeId(1),
                    target: NodeId(2),
                    span: sp(2, 3),
                },
                // Back edge creating a cycle: c -> a
                Edge {
                    id: EdgeId(2),
                    source: NodeId(2),
                    target: NodeId(0),
                    span: sp(4, 5),
                },
            ],
        };
        let cycles = detect_cycles_in_subgraph(&sub);
        assert!(!cycles.is_empty(), "expected feedback cycle, found none");
        assert_eq!(cycles[0].len(), 3);
    }

    // ── Integration ─────────────────────────────────────────────────────

    #[test]
    fn example_pdl_graph() {
        let reg = test_registry();
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("examples/example.pdl");
        let source = std::fs::read_to_string(&path).expect("failed to read example.pdl");
        let parse_result = crate::parser::parse(&source);
        assert!(parse_result.errors.is_empty());
        let program = parse_result.program.unwrap();
        let resolve_result = resolve::resolve(&program, &reg);
        assert!(
            resolve_result
                .diagnostics
                .iter()
                .all(|d| d.level != DiagLevel::Error),
            "resolve errors: {:#?}",
            resolve_result.diagnostics
        );
        let result = build_graph(&program, &resolve_result.resolved, &reg);
        assert!(
            result
                .diagnostics
                .iter()
                .all(|d| d.level != DiagLevel::Error),
            "graph errors: {:#?}",
            result.diagnostics
        );

        // Should have 2 tasks: capture, drain
        assert_eq!(result.graph.tasks.len(), 2);
        assert!(result.graph.tasks.contains_key("capture"));
        assert!(result.graph.tasks.contains_key("drain"));

        // Capture should be Pipeline
        assert!(matches!(
            result.graph.tasks.get("capture").unwrap(),
            TaskGraph::Pipeline(_)
        ));

        // Inter-task edge for 'signal' buffer
        assert!(!result.graph.inter_task_edges.is_empty());
    }

    #[test]
    fn receiver_pdl_graph() {
        let reg = test_registry();
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("examples/receiver.pdl");
        let source = std::fs::read_to_string(&path).expect("failed to read receiver.pdl");
        let parse_result = crate::parser::parse(&source);
        assert!(parse_result.errors.is_empty());
        let program = parse_result.program.unwrap();
        let resolve_result = resolve::resolve(&program, &reg);
        assert!(
            resolve_result
                .diagnostics
                .iter()
                .all(|d| d.level != DiagLevel::Error),
            "resolve errors: {:#?}",
            resolve_result.diagnostics
        );
        let result = build_graph(&program, &resolve_result.resolved, &reg);
        assert!(
            result
                .diagnostics
                .iter()
                .all(|d| d.level != DiagLevel::Error),
            "graph errors: {:#?}",
            result.diagnostics
        );

        // Should have 2 tasks: receiver (modal), logger (pipeline)
        assert_eq!(result.graph.tasks.len(), 2);

        // receiver should be Modal
        match result.graph.tasks.get("receiver").unwrap() {
            TaskGraph::Modal { control, modes } => {
                assert!(!control.nodes.is_empty());
                assert_eq!(modes.len(), 2);
            }
            _ => panic!("expected Modal for receiver task"),
        }

        // logger should be Pipeline
        assert!(matches!(
            result.graph.tasks.get("logger").unwrap(),
            TaskGraph::Pipeline(_)
        ));
    }

    // ── Regression: define inlining edge chaining ────────────────────────

    #[test]
    fn define_as_middle_elem_chains_correctly() {
        let reg = test_registry();
        // When a define is used as a PipeElem (not source), the incoming edge
        // should connect to the FIRST node of the inlined body, and the chain
        // should continue from the LAST node.
        let graph = build_ok(
            concat!(
                "define mid() {\n    fft(256) | mag()\n}\n",
                "clock 1kHz t {\n    constant(0.0) | mid() | stdout()\n}",
            ),
            &reg,
        );
        let sub = get_pipeline_subgraph(&graph, "t");
        // adc + fft + mag + stdout = 4 actor nodes
        assert_eq!(sub.nodes.len(), 4);
        // adc -> fft -> mag -> stdout = 3 edges
        assert_eq!(sub.edges.len(), 3);

        // Verify edge connectivity: adc should connect to fft (first node of define)
        let adc = sub
            .nodes
            .iter()
            .find(|n| matches!(&n.kind, NodeKind::Actor { name, .. } if name == "constant"))
            .unwrap();
        let fft = sub
            .nodes
            .iter()
            .find(|n| matches!(&n.kind, NodeKind::Actor { name, .. } if name == "fft"))
            .unwrap();
        let mag = sub
            .nodes
            .iter()
            .find(|n| matches!(&n.kind, NodeKind::Actor { name, .. } if name == "mag"))
            .unwrap();
        let stdout = sub
            .nodes
            .iter()
            .find(|n| matches!(&n.kind, NodeKind::Actor { name, .. } if name == "stdout"))
            .unwrap();

        // adc -> fft (NOT adc -> mag)
        assert!(
            sub.edges
                .iter()
                .any(|e| e.source == adc.id && e.target == fft.id),
            "expected edge adc -> fft"
        );
        // fft -> mag
        assert!(
            sub.edges
                .iter()
                .any(|e| e.source == fft.id && e.target == mag.id),
            "expected edge fft -> mag"
        );
        // mag -> stdout (NOT fft -> stdout)
        assert!(
            sub.edges
                .iter()
                .any(|e| e.source == mag.id && e.target == stdout.id),
            "expected edge mag -> stdout"
        );
    }

    // ── Tap-input feedback loops ────────────────────────────────────────

    #[test]
    fn tap_input_feedback_loop() {
        let reg = test_registry();
        // add(:fb) explicitly connects tap :fb as an additional input.
        // :fb is declared at end of line 2 (forward reference).
        let graph = build_ok(
            concat!(
                "clock 1kHz t {\n",
                "    constant(0.0) | add(:fb) | :out | stdout()\n",
                "    :out | delay(1, 0.0) | :fb\n",
                "}",
            ),
            &reg,
        );
        let sub = get_pipeline_subgraph(&graph, "t");

        // Exactly one add node (no merge needed)
        let add_count = count_nodes_of_kind(
            sub,
            |k| matches!(k, NodeKind::Actor { name, .. } if name == "add"),
        );
        assert_eq!(add_count, 1, "expected 1 add node, got {}", add_count);

        // add should have 2 incoming edges: from adc (pipe) + from fork(:fb) (tap-input)
        let add_node = sub
            .nodes
            .iter()
            .find(|n| matches!(&n.kind, NodeKind::Actor { name, .. } if name == "add"))
            .unwrap();
        let incoming = sub.edges.iter().filter(|e| e.target == add_node.id).count();
        assert_eq!(
            incoming, 2,
            "expected 2 incoming edges to add, got {}",
            incoming
        );

        // Cycle detected: add -> fork(:out) -> delay -> fork(:fb) -> add
        assert!(
            !graph.cycles.is_empty(),
            "expected feedback cycle with tap-input syntax"
        );
    }

    #[test]
    fn three_plus_add_actors_partial_feedback() {
        let reg = test_registry();
        // Two independent add() actors, each using different tap-inputs:
        //   Feedback pair: add(:fb) with :fb from delay loop
        //   Feedforward pair: add(:fwd) with :fwd from a separate source
        let graph = build_ok(
            concat!(
                "clock 1kHz t {\n",
                "    constant(0.0) | add(:fb) | :out | stdout()\n",
                "    :out | delay(1, 0.0) | :fb\n",
                "    constant(0.0) | :sig | add(:fwd) | stdout()\n",
                "    :sig | delay(1, 0.0) | :fwd\n",
                "}",
            ),
            &reg,
        );
        let sub = get_pipeline_subgraph(&graph, "t");

        // Two add nodes remain (one per pair)
        let add_count = count_nodes_of_kind(
            sub,
            |k| matches!(k, NodeKind::Actor { name, .. } if name == "add"),
        );
        assert_eq!(
            add_count, 2,
            "expected 2 add nodes (feedback + feedforward), got {}",
            add_count
        );

        // Both add nodes should have 2 incoming edges
        let add_nodes: Vec<_> = sub
            .nodes
            .iter()
            .filter(|n| matches!(&n.kind, NodeKind::Actor { name, .. } if name == "add"))
            .collect();
        for add_node in &add_nodes {
            let in_count = sub.edges.iter().filter(|e| e.target == add_node.id).count();
            assert_eq!(
                in_count, 2,
                "add node {:?} should have 2 inputs, got {}",
                add_node.id, in_count
            );
        }

        // Only the feedback pair produces a cycle
        assert!(
            !graph.cycles.is_empty(),
            "expected feedback cycle from the feedback pair"
        );
    }

    #[test]
    fn tap_input_multiple_refs() {
        let reg = test_registry();
        // add(:fb1, :fw2) — actor with multiple tap-ref inputs
        // Tests the user's example: 3 inputs to one actor
        let graph = build_ok(
            concat!(
                "clock 1kHz t {\n",
                "    constant(0.0) | add(:fb1, :fw2) | :out | stdout()\n",
                "    constant(0.0) | delay(1, 0.0) | :fw2\n",
                "    :out | delay(1, 0.0) | :fb1\n",
                "}",
            ),
            &reg,
        );
        let sub = get_pipeline_subgraph(&graph, "t");

        let add_node = sub
            .nodes
            .iter()
            .find(|n| matches!(&n.kind, NodeKind::Actor { name, .. } if name == "add"))
            .unwrap();
        // 3 incoming: adc (pipe) + fork(:fb1) + fork(:fw2)
        let incoming = sub.edges.iter().filter(|e| e.target == add_node.id).count();
        assert_eq!(
            incoming, 3,
            "expected 3 incoming edges to add, got {}",
            incoming
        );

        // Feedback cycle from :fb1 path
        assert!(!graph.cycles.is_empty(), "expected feedback cycle via :fb1");
    }

    // ── Regression: tap scope isolation in defines ──────────────────────

    #[test]
    fn define_with_tap_scope_isolated() {
        let reg = test_registry();
        // A define that declares a tap should not leak it to the outer scope
        let graph = build_ok(
            concat!(
                "define proc() {\n",
                "    constant(0.0) | :sig | mag() | stdout()\n",
                "    :sig | stdout()\n",
                "}\n",
                "clock 1kHz t {\n    proc()\n}",
            ),
            &reg,
        );
        let sub = get_pipeline_subgraph(&graph, "t");
        // Should have: adc, fork(:sig), mag, stdout, stdout = 5 nodes
        let forks = count_nodes_of_kind(sub, |k| matches!(k, NodeKind::Fork { .. }));
        assert_eq!(forks, 1, "expected 1 fork node");
    }

    // ── Shape constraint preservation ────────────────────────────────────

    #[test]
    fn shape_constraint_preserved_in_node() {
        let reg = test_registry();
        let graph = build_ok(
            "clock 1kHz t {\n    constant(0.0) | fft()[256] | c2r() | stdout()\n}",
            &reg,
        );
        let sub = get_pipeline_subgraph(&graph, "t");

        // fft()[256] should have shape_constraint = Some(ShapeConstraint { dims: [Literal(256)] })
        let fft_node = sub
            .nodes
            .iter()
            .find(|n| matches!(&n.kind, NodeKind::Actor { name, .. } if name == "fft"))
            .expect("fft node not found");
        if let NodeKind::Actor {
            shape_constraint, ..
        } = &fft_node.kind
        {
            let sc = shape_constraint
                .as_ref()
                .expect("fft()[256] should have shape_constraint");
            assert_eq!(sc.dims.len(), 1, "expected 1 dimension");
            assert!(
                matches!(sc.dims[0], crate::ast::ShapeDim::Literal(256, _)),
                "expected ShapeDim::Literal(256), got: {:?}",
                sc.dims[0]
            );
        } else {
            panic!("expected Actor node");
        }

        // constant(0.0) should NOT have shape_constraint
        let const_node = sub
            .nodes
            .iter()
            .find(|n| matches!(&n.kind, NodeKind::Actor { name, .. } if name == "constant"))
            .expect("constant node not found");
        if let NodeKind::Actor {
            shape_constraint, ..
        } = &const_node.kind
        {
            assert!(
                shape_constraint.is_none(),
                "constant(0.0) should not have shape_constraint"
            );
        }
    }
}
