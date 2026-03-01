// analyze.rs — Static analysis for Pipit SDF graphs
//
// Validates type compatibility at pipe endpoints, solves SDF balance equations
// to compute repetition vectors, verifies feedback loop delay presence,
// checks cross-clock rate matching, computes buffer sizes, and validates
// runtime parameter types.
//
// Preconditions: `thir` is a ThirContext wrapping HIR + resolved/typed/lowered;
//                `graph` is a valid ProgramGraph; `registry` has actor metadata.
// Postconditions: returns `AnalysisResult` with computed repetition vectors,
//                 buffer sizes, and all accumulated diagnostics.
// Failure modes: type mismatches, unsolvable balance equations, missing delays,
//                rate mismatches, memory overflow, param type mismatches
//                produce `Diagnostic` entries.
// Side effects: none.

use std::collections::{HashMap, HashSet, VecDeque};

use chumsky::span::Span as _;

use sha2::{Digest, Sha256};

use crate::ast::*;
use crate::diag::codes;
use crate::diag::{DiagCode, DiagLevel, Diagnostic};
use crate::graph::*;
use crate::hir::{HirSwitchSource, HirTaskBody};
use crate::id::CallId;
use crate::registry::{ActorMeta, ParamKind, ParamType, PipitType, PortShape, TokenCount};
use crate::subgraph_index::{
    build_global_node_index, build_subgraph_indices, find_node, subgraph_key, subgraphs_of,
    GraphQueryCtx, SubgraphIndex,
};
use crate::thir::ThirContext;

const SHAPE_WORKLIST_MIN_EDGES: usize = 24;

// ── Public types ────────────────────────────────────────────────────────────

/// Result of static analysis.
#[derive(Debug)]
pub struct AnalysisResult {
    pub analysis: AnalyzedProgram,
    pub diagnostics: Vec<Diagnostic>,
}

/// Computed data from analysis, consumed by downstream schedule/codegen phases.
#[derive(Debug)]
pub struct AnalyzedProgram {
    /// Repetition vector per (task_name, subgraph_label).
    /// Label: "pipeline" for Pipeline tasks, "control" / mode name for Modal.
    pub repetition_vectors: HashMap<(String, String), HashMap<NodeId, u32>>,
    /// Inter-task buffer sizes: buffer_name → bytes.
    pub inter_task_buffers: HashMap<String, u64>,
    /// Total memory required (bytes).
    pub total_memory: u64,
    /// Inferred shape constraints from SDF edge propagation (§13.3.3).
    /// For actors with unresolved SHAPE dims, this stores the shapes inferred
    /// from connected edges. NodeId → inferred ShapeConstraint.
    pub inferred_shapes: HashMap<NodeId, ShapeConstraint>,
    /// Span-derived dimension parameters: NodeId → (param_name → value).
    /// For actors where a symbolic dimension param was inferred from a span
    /// argument length (e.g., `fir(coeff)` with `coeff=[...5 elems...]` → N=5).
    /// Authoritative: takes precedence over SDF edge-derived fallbacks in codegen.
    pub span_derived_dims: HashMap<NodeId, HashMap<String, u32>>,
    /// Concrete input/output port rates per node, resolved once in analysis.
    /// `None` means the corresponding side could not be resolved statically.
    pub node_port_rates: HashMap<NodeId, NodePortRates>,
    /// Bind contracts inferred from graph analysis (§5.5).
    pub bind_contracts: HashMap<String, BindContract>,
}

/// Concrete input/output token rates for a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodePortRates {
    pub in_rate: Option<u32>,
    pub out_rate: Option<u32>,
}

/// Inferred bind contract: direction, data type, shape, rate, and stable ID.
#[derive(Debug, Clone)]
pub struct BindContract {
    pub direction: BindDirection,
    pub dtype: Option<PipitType>,
    pub shape: Vec<u32>,
    pub rate_hz: Option<f64>,
    /// Deterministic ID from graph lineage (§5.5.3). 16-char hex string derived
    /// from SHA-256 of (direction, adjacent actor CallIds, transport).
    pub stable_id: String,
}

// ── Public entry point ──────────────────────────────────────────────────────

/// Run all static analysis checks on a built SDF program graph.
pub fn analyze(thir: &ThirContext, graph: &ProgramGraph) -> AnalysisResult {
    let mut ctx = AnalyzeCtx::new(thir, graph);
    ctx.check_types();
    ctx.record_span_derived_dims();
    ctx.infer_shapes_from_edges();
    ctx.check_shape_constraints();
    ctx.check_dimension_param_order();
    ctx.precompute_node_port_rates();
    ctx.solve_balance_equations();
    ctx.check_feedback_delays();
    ctx.check_cross_clock_rates();
    ctx.compute_buffer_sizes();
    ctx.infer_bind_contracts();
    ctx.validate_bind_endpoints();
    ctx.check_memory_pool();
    ctx.check_param_types();
    ctx.check_ctrl_types();
    ctx.build_result()
}

// ── Internal context ────────────────────────────────────────────────────────

struct AnalyzeCtx<'a> {
    thir: &'a ThirContext<'a>,
    graph: &'a ProgramGraph,
    diagnostics: Vec<Diagnostic>,
    repetition_vectors: HashMap<(String, String), HashMap<NodeId, u32>>,
    inter_buffers: HashMap<String, u64>,
    total_memory: u64,
    inferred_shapes: HashMap<NodeId, ShapeConstraint>,
    span_derived_dims: HashMap<NodeId, HashMap<String, u32>>,
    subgraph_indices: HashMap<usize, SubgraphIndex>,
    subgraph_refs: HashMap<usize, &'a Subgraph>,
    global_node_index: HashMap<NodeId, (usize, usize)>,
    rv_by_task: HashMap<String, HashMap<NodeId, u32>>,
    bind_contracts: HashMap<String, BindContract>,
    node_port_rates: HashMap<NodeId, NodePortRates>,
    all_subgraphs: Vec<(&'a str, &'a str, &'a Subgraph)>,
}

struct BalanceGraph {
    rates: HashMap<(NodeId, NodeId), (u32, u32)>,
    adjacency: HashMap<NodeId, Vec<NodeId>>,
}

#[derive(Default)]
struct ShapeAdjacency {
    incoming: HashMap<NodeId, Vec<usize>>,
    outgoing: HashMap<NodeId, Vec<usize>>,
}

impl ShapeAdjacency {
    fn from_subgraph(sub: &Subgraph) -> Self {
        let mut adjacency = ShapeAdjacency::default();
        for (edge_idx, edge) in sub.edges.iter().enumerate() {
            adjacency
                .outgoing
                .entry(edge.source)
                .or_default()
                .push(edge_idx);
            adjacency
                .incoming
                .entry(edge.target)
                .or_default()
                .push(edge_idx);
        }
        adjacency
    }
}

struct ShapeWorklist {
    queue: VecDeque<NodeId>,
    queued: HashSet<NodeId>,
}

impl ShapeWorklist {
    fn seeded(sub: &Subgraph) -> Self {
        let mut queue = VecDeque::with_capacity(sub.nodes.len());
        let mut queued = HashSet::with_capacity(sub.nodes.len());
        for node in &sub.nodes {
            queue.push_back(node.id);
            queued.insert(node.id);
        }
        ShapeWorklist { queue, queued }
    }

    fn pop(&mut self) -> Option<NodeId> {
        let node_id = self.queue.pop_front()?;
        self.queued.remove(&node_id);
        Some(node_id)
    }

    fn push(&mut self, node_id: NodeId) {
        if self.queued.insert(node_id) {
            self.queue.push_back(node_id);
        }
    }
}

impl<'a> AnalyzeCtx<'a> {
    fn new(thir: &'a ThirContext<'a>, graph: &'a ProgramGraph) -> Self {
        let subgraph_indices = build_subgraph_indices(graph);
        let subgraph_refs = build_subgraph_refs(graph);
        let global_node_index = build_global_node_index(graph, &subgraph_indices);

        let mut all_subgraphs = Vec::new();
        for (task_name, task_graph) in &graph.tasks {
            match task_graph {
                TaskGraph::Pipeline(sub) => {
                    all_subgraphs.push((task_name.as_str(), "pipeline", sub));
                }
                TaskGraph::Modal { control, modes } => {
                    all_subgraphs.push((task_name.as_str(), "control", control));
                    for (mode_name, sub) in modes {
                        all_subgraphs.push((task_name.as_str(), mode_name.as_str(), sub));
                    }
                }
            }
        }

        AnalyzeCtx {
            thir,
            graph,
            diagnostics: Vec::new(),
            repetition_vectors: HashMap::new(),
            inter_buffers: HashMap::new(),
            total_memory: 0,
            inferred_shapes: HashMap::new(),
            span_derived_dims: HashMap::new(),
            subgraph_indices,
            subgraph_refs,
            global_node_index,
            rv_by_task: HashMap::new(),
            bind_contracts: HashMap::new(),
            node_port_rates: HashMap::new(),
            all_subgraphs,
        }
    }

    fn error(&mut self, code: DiagCode, span: Span, message: String) {
        self.diagnostics
            .push(Diagnostic::new(DiagLevel::Error, span, message).with_code(code));
    }

    fn error_with_hint(&mut self, code: DiagCode, span: Span, message: String, hint: String) {
        self.diagnostics.push(
            Diagnostic::new(DiagLevel::Error, span, message)
                .with_code(code)
                .with_hint(hint),
        );
    }

    fn warning_with_hint(&mut self, code: DiagCode, span: Span, message: String, hint: String) {
        self.diagnostics.push(
            Diagnostic::new(DiagLevel::Warning, span, message)
                .with_code(code)
                .with_hint(hint),
        );
    }

    fn build_result(self) -> AnalysisResult {
        AnalysisResult {
            analysis: AnalyzedProgram {
                repetition_vectors: self.repetition_vectors,
                inter_task_buffers: self.inter_buffers,
                total_memory: self.total_memory,
                inferred_shapes: self.inferred_shapes,
                span_derived_dims: self.span_derived_dims,
                node_port_rates: self.node_port_rates,
                bind_contracts: self.bind_contracts,
            },
            diagnostics: self.diagnostics,
        }
    }

    /// Precompute port rates for all nodes once, after shape inference.
    /// Reused by balance equation solving and exported in the final result.
    fn precompute_node_port_rates(&mut self) {
        let subs = std::mem::take(&mut self.all_subgraphs);
        for &(_, _, sub) in &subs {
            for node in &sub.nodes {
                self.node_port_rates.insert(
                    node.id,
                    NodePortRates {
                        in_rate: self.consumption_rate(node),
                        out_rate: self.production_rate(node),
                    },
                );
            }
        }
        self.all_subgraphs = subs;
    }

    /// Look up cached production rate. Falls back to live computation.
    fn cached_production_rate(&self, node: &Node) -> Option<u32> {
        self.node_port_rates
            .get(&node.id)
            .and_then(|r| r.out_rate)
            .or_else(|| self.production_rate(node))
    }

    /// Look up cached consumption rate. Falls back to live computation.
    fn cached_consumption_rate(&self, node: &Node) -> Option<u32> {
        self.node_port_rates
            .get(&node.id)
            .and_then(|r| r.in_rate)
            .or_else(|| self.consumption_rate(node))
    }

    // ── Helpers ─────────────────────────────────────────────────────────

    /// Look up actor metadata by name.
    fn actor_meta(&self, name: &str) -> Option<&ActorMeta> {
        self.thir.registry.lookup(name)
    }

    fn gqctx(&self) -> GraphQueryCtx<'_> {
        GraphQueryCtx::new(&self.subgraph_indices)
    }

    fn node_in_subgraph<'s>(&self, sub: &'s Subgraph, id: NodeId) -> Option<&'s Node> {
        self.gqctx().node_in_subgraph(sub, id)
    }

    fn first_incoming_edge_in_subgraph<'s>(
        &self,
        sub: &'s Subgraph,
        id: NodeId,
    ) -> Option<&'s Edge> {
        self.gqctx().first_incoming_edge(sub, id)
    }

    fn first_outgoing_edge_in_subgraph<'s>(
        &self,
        sub: &'s Subgraph,
        id: NodeId,
    ) -> Option<&'s Edge> {
        self.gqctx().first_outgoing_edge(sub, id)
    }

    /// Get the output type of a node, tracing through passthrough nodes.
    fn infer_output_type(&self, node: &Node, sub: &Subgraph) -> Option<PipitType> {
        match &node.kind {
            NodeKind::Actor { name, .. } => {
                self.actor_meta(name).and_then(|m| m.out_type.as_concrete())
            }
            NodeKind::Fork { .. }
            | NodeKind::Probe { .. }
            | NodeKind::BufferWrite { .. }
            | NodeKind::ScatterWrite { .. } => {
                // Trace backwards to find upstream actor
                self.trace_type_backward(node.id, sub)
            }
            NodeKind::BufferRead { buffer_name } => {
                // Find the writer task's BufferWrite and trace back from there
                self.infer_buffer_type(buffer_name)
            }
            NodeKind::GatherRead { family_name, .. } => {
                // Infer type from first element buffer
                self.infer_buffer_type(&format!("{family_name}__0"))
            }
        }
    }

    /// Get the input type of a node, tracing through passthrough nodes.
    fn infer_input_type(&self, node: &Node, sub: &Subgraph) -> Option<PipitType> {
        match &node.kind {
            NodeKind::Actor { name, .. } => {
                self.actor_meta(name).and_then(|m| m.in_type.as_concrete())
            }
            NodeKind::Fork { .. }
            | NodeKind::Probe { .. }
            | NodeKind::BufferRead { .. }
            | NodeKind::GatherRead { .. } => {
                // Passthrough: input type == output type, trace backward
                self.trace_type_backward(node.id, sub)
            }
            NodeKind::BufferWrite { .. } | NodeKind::ScatterWrite { .. } => {
                // BufferWrite/ScatterWrite accepts whatever type the upstream produces
                self.trace_type_backward(node.id, sub)
            }
        }
    }

    /// Trace backwards from a passthrough node to find the type produced by
    /// the nearest upstream Actor.
    fn trace_type_backward(&self, node_id: NodeId, sub: &Subgraph) -> Option<PipitType> {
        let mut current = node_id;
        let mut visited = HashSet::new();
        loop {
            if !visited.insert(current) {
                return None; // cycle guard
            }
            let node = self.node_in_subgraph(sub, current)?;
            if let NodeKind::Actor { name, .. } = &node.kind {
                return self.actor_meta(name).and_then(|m| m.out_type.as_concrete());
            }
            // Find predecessor
            let pred = self.first_incoming_edge_in_subgraph(sub, current);
            match pred {
                Some(edge) => current = edge.source,
                None => return None,
            }
        }
    }

    /// Infer the wire type of a shared buffer by tracing from the writer side.
    fn infer_buffer_type(&self, buffer_name: &str) -> Option<PipitType> {
        let buf_info = self.thir.resolved.buffers.get(buffer_name)?;
        let task_graph = self.graph.tasks.get(&buf_info.writer_task)?;
        let write_node = find_buffer_write_in_task(task_graph, buffer_name)?;
        // Find the subgraph containing this node and trace backward
        for sub in subgraphs_of(task_graph) {
            if self.node_in_subgraph(sub, write_node).is_some() {
                return self.trace_type_backward(write_node, sub);
            }
        }
        None
    }

    /// Get production rate of a node (out_count). Passthrough nodes return 1.
    /// Uses explicit shape constraint first, then inferred shape as fallback.
    fn production_rate(&self, node: &Node) -> Option<u32> {
        match &node.kind {
            NodeKind::Actor {
                name,
                args,
                shape_constraint,
                ..
            } => {
                let meta = self.actor_meta(name)?;
                // Try explicit shape constraint first, then inferred
                let result =
                    self.resolve_port_rate(&meta.out_shape, meta, args, shape_constraint.as_ref());
                if result.is_some() {
                    return result;
                }
                self.resolve_port_rate(
                    &meta.out_shape,
                    meta,
                    args,
                    self.inferred_shapes.get(&node.id),
                )
            }
            // GatherRead produces element_count tokens on its single output edge
            NodeKind::GatherRead { element_count, .. } => Some(*element_count),
            // ScatterWrite produces 1 token per output edge (one per element buffer)
            NodeKind::ScatterWrite { .. } => Some(1),
            _ => Some(1),
        }
    }

    /// Get consumption rate of a node (in_count). Passthrough nodes return 1.
    /// Uses explicit shape constraint first, then inferred shape as fallback.
    fn consumption_rate(&self, node: &Node) -> Option<u32> {
        match &node.kind {
            NodeKind::Actor {
                name,
                args,
                shape_constraint,
                ..
            } => {
                let meta = self.actor_meta(name)?;
                let result =
                    self.resolve_port_rate(&meta.in_shape, meta, args, shape_constraint.as_ref());
                if result.is_some() {
                    return result;
                }
                self.resolve_port_rate(
                    &meta.in_shape,
                    meta,
                    args,
                    self.inferred_shapes.get(&node.id),
                )
            }
            // GatherRead consumes element_count total (1 per incoming inter-task edge)
            NodeKind::GatherRead { element_count, .. } => Some(*element_count),
            // ScatterWrite consumes element_count tokens from its single input edge
            NodeKind::ScatterWrite { element_count, .. } => Some(*element_count),
            _ => Some(1),
        }
    }

    /// Resolve a PortShape to a concrete rate (product of resolved dimensions).
    /// Uses shape constraint from call site to infer symbolic dimensions.
    fn resolve_port_rate(
        &self,
        shape: &PortShape,
        actor_meta: &ActorMeta,
        actor_args: &[Arg],
        shape_constraint: Option<&ShapeConstraint>,
    ) -> Option<u32> {
        self.thir.resolve_port_rate(
            shape,
            actor_meta,
            actor_args,
            shape_constraint.map(|sc| sc.dims.as_slice()),
        )
    }

    /// Resolve a single ShapeDim from a call-site shape constraint.
    fn resolve_shape_dim(&self, dim: &ShapeDim) -> Option<u32> {
        self.thir.resolve_shape_dim(dim)
    }

    /// Resolve an Arg to a u32 value (for token count resolution).
    fn resolve_arg_to_u32(&self, arg: &Arg) -> Option<u32> {
        self.thir.resolve_arg_to_u32(arg)
    }

    /// Get the clock frequency for a task from HIR.
    fn get_task_freq(&self, task_name: &str) -> Option<(f64, Span)> {
        self.thir
            .task_info(task_name)
            .map(|t| (t.freq_hz, t.freq_span))
    }

    /// Get memory pool limit in bytes.
    ///
    /// Returns:
    /// - (`set mem` value, Some(span)) when explicitly configured.
    /// - (64MB, None) when omitted (spec default).
    fn get_mem_limit(&self) -> (u64, Option<Span>) {
        (self.thir.mem_bytes, self.thir.mem_span)
    }

    // ── Phase 0: Shape inference from SDF edges (§13.3.3) ────────────────
    //
    // For actors with unresolved symbolic dimensions in SHAPE(...),
    // propagate known shapes from connected edges. When the connected port
    // has a fully resolved shape of the same rank, infer dimension values
    // positionally (dim-for-dim).

    fn infer_shapes_from_edges(&mut self) {
        let subs = std::mem::take(&mut self.all_subgraphs);
        for &(_, _, sub) in &subs {
            self.infer_shapes_in_subgraph(sub);
        }
        self.all_subgraphs = subs;
    }

    fn infer_shapes_in_subgraph(&mut self, sub: &Subgraph) {
        if sub.nodes.is_empty() || sub.edges.is_empty() {
            return;
        }
        if self.should_use_dense_shape_inference(sub) {
            self.infer_shapes_in_subgraph_dense(sub);
            return;
        }

        let adjacency = ShapeAdjacency::from_subgraph(sub);
        let mut worklist = ShapeWorklist::seeded(sub);
        while let Some(node_id) = worklist.pop() {
            self.propagate_shapes_forward(sub, node_id, &adjacency, &mut worklist);
            self.propagate_shapes_reverse(sub, node_id, &adjacency, &mut worklist);
        }
    }

    fn infer_shapes_in_subgraph_dense(&mut self, sub: &Subgraph) {
        // Dense fallback for small subgraphs: avoids worklist bookkeeping overhead.
        let mut changed = true;
        while changed {
            changed = false;
            for edge_idx in 0..sub.edges.len() {
                if self.propagate_shape_for_edge_dense(sub, edge_idx) {
                    changed = true;
                }
            }
        }
    }

    fn should_use_dense_shape_inference(&self, sub: &Subgraph) -> bool {
        sub.edges.len() < SHAPE_WORKLIST_MIN_EDGES
    }

    fn propagate_shapes_forward(
        &mut self,
        sub: &Subgraph,
        node_id: NodeId,
        adjacency: &ShapeAdjacency,
        worklist: &mut ShapeWorklist,
    ) {
        let Some(edge_ids) = adjacency.outgoing.get(&node_id) else {
            return;
        };
        for &edge_idx in edge_ids {
            self.propagate_shape_forward_on_edge(sub, edge_idx, worklist);
        }
    }

    fn propagate_shapes_reverse(
        &mut self,
        sub: &Subgraph,
        node_id: NodeId,
        adjacency: &ShapeAdjacency,
        worklist: &mut ShapeWorklist,
    ) {
        let Some(edge_ids) = adjacency.incoming.get(&node_id) else {
            return;
        };
        for &edge_idx in edge_ids {
            self.propagate_shape_reverse_on_edge(sub, edge_idx, worklist);
        }
    }

    fn propagate_shape_forward_on_edge(
        &mut self,
        sub: &Subgraph,
        edge_idx: usize,
        worklist: &mut ShapeWorklist,
    ) {
        let Some((src, tgt)) = self.edge_endpoints(sub, edge_idx) else {
            return;
        };
        if let Some(sc) = self.try_propagate_shape(src, tgt, sub) {
            self.upsert_inferred_shape_and_enqueue(tgt.id, sc, worklist);
        }
    }

    fn propagate_shape_reverse_on_edge(
        &mut self,
        sub: &Subgraph,
        edge_idx: usize,
        worklist: &mut ShapeWorklist,
    ) {
        let Some((src, tgt)) = self.edge_endpoints(sub, edge_idx) else {
            return;
        };
        if let Some(sc) = self.try_propagate_shape_reverse(tgt, src, sub) {
            self.upsert_inferred_shape_and_enqueue(src.id, sc, worklist);
        }
    }

    fn propagate_shape_for_edge_dense(&mut self, sub: &Subgraph, edge_idx: usize) -> bool {
        let Some((src, tgt)) = self.edge_endpoints(sub, edge_idx) else {
            return false;
        };

        let mut changed = false;
        if let Some(sc) = self.try_propagate_shape(src, tgt, sub) {
            changed |= self.upsert_inferred_shape(tgt.id, sc);
        }
        if let Some(sc) = self.try_propagate_shape_reverse(tgt, src, sub) {
            changed |= self.upsert_inferred_shape(src.id, sc);
        }
        changed
    }

    fn edge_endpoints<'s>(
        &self,
        sub: &'s Subgraph,
        edge_idx: usize,
    ) -> Option<(&'s Node, &'s Node)> {
        let edge = sub.edges.get(edge_idx)?;
        let src = self.node_in_subgraph(sub, edge.source)?;
        let tgt = self.node_in_subgraph(sub, edge.target)?;
        Some((src, tgt))
    }

    fn upsert_inferred_shape_and_enqueue(
        &mut self,
        node_id: NodeId,
        sc: ShapeConstraint,
        worklist: &mut ShapeWorklist,
    ) {
        if self.upsert_inferred_shape(node_id, sc) {
            worklist.push(node_id);
        }
    }

    /// Insert/update inferred shape and report whether anything changed.
    fn upsert_inferred_shape(&mut self, node_id: NodeId, sc: ShapeConstraint) -> bool {
        match self.inferred_shapes.get(&node_id) {
            Some(prev) if prev == &sc => false,
            _ => {
                self.inferred_shapes.insert(node_id, sc);
                true
            }
        }
    }

    /// Resolve a symbolic dimension from explicit actor args.
    fn resolve_symbolic_dim_from_args(
        &self,
        sym: &str,
        meta: &ActorMeta,
        args: &[Arg],
    ) -> Option<u32> {
        meta.params
            .iter()
            .position(|p| p.name == sym)
            .and_then(|idx| args.get(idx))
            .and_then(|arg| self.resolve_arg_to_u32(arg))
    }

    /// Resolve a single port dimension using precedence:
    /// explicit arg / explicit shape / span-derived / existing inferred.
    #[allow(clippy::too_many_arguments)]
    fn resolve_port_dim_preferred(
        &self,
        node_id: NodeId,
        dim_idx: usize,
        dim: &TokenCount,
        meta: &ActorMeta,
        args: &[Arg],
        explicit_sc: Option<&ShapeConstraint>,
        existing_inferred_sc: Option<&ShapeConstraint>,
    ) -> Option<u32> {
        match dim {
            TokenCount::Literal(n) => Some(*n),
            TokenCount::Symbolic(sym) => self
                .resolve_symbolic_dim_from_args(sym, meta, args)
                .or_else(|| {
                    explicit_sc
                        .and_then(|sc| sc.dims.get(dim_idx))
                        .and_then(|sd| self.resolve_shape_dim(sd))
                })
                .or_else(|| {
                    self.span_derived_dims
                        .get(&node_id)
                        .and_then(|m| m.get(sym.as_str()))
                        .copied()
                })
                .or_else(|| {
                    existing_inferred_sc
                        .and_then(|sc| sc.dims.get(dim_idx))
                        .and_then(|sd| self.resolve_shape_dim(sd))
                }),
        }
    }

    /// Merge edge-propagated dimensions into a target/source actor shape.
    /// Per-dimension behavior:
    /// - keep dimensions already fixed by higher-priority sources
    /// - fill only unresolved dimensions from propagated dims
    ///
    /// Returns merged shape and whether any unresolved dimension was newly filled.
    #[allow(clippy::too_many_arguments)]
    fn merge_propagated_shape(
        &self,
        node_id: NodeId,
        port_shape: &PortShape,
        meta: &ActorMeta,
        args: &[Arg],
        explicit_sc: Option<&ShapeConstraint>,
        existing_inferred_sc: Option<&ShapeConstraint>,
        propagated_dims: &[u32],
        span: Span,
    ) -> Option<(ShapeConstraint, bool)> {
        if propagated_dims.len() != port_shape.rank() {
            return None;
        }

        let mut changed = false;
        let mut dims = Vec::with_capacity(port_shape.rank());
        for (i, dim) in port_shape.dims.iter().enumerate() {
            let val = if let Some(preferred) = self.resolve_port_dim_preferred(
                node_id,
                i,
                dim,
                meta,
                args,
                explicit_sc,
                existing_inferred_sc,
            ) {
                preferred
            } else {
                changed = true;
                propagated_dims[i]
            };
            dims.push(ShapeDim::Literal(val, span));
        }

        Some((ShapeConstraint { dims, span }, changed))
    }

    fn unresolved_actor_shape_target<'n>(&self, node: &'n Node) -> Option<(&'n [Arg], &ActorMeta)> {
        let NodeKind::Actor {
            name,
            args,
            shape_constraint,
            ..
        } = &node.kind
        else {
            return None;
        };
        if shape_constraint.is_some() {
            return None;
        }
        let meta = self.actor_meta(name)?;
        Some((args.as_slice(), meta))
    }

    fn target_has_fixed_actor_input_shape(&self, node: &Node) -> bool {
        let NodeKind::Actor { name, .. } = &node.kind else {
            return false;
        };
        let Some(meta) = self.actor_meta(name) else {
            return false;
        };
        !meta
            .in_shape
            .dims
            .iter()
            .any(|d| matches!(d, TokenCount::Symbolic(_)))
    }

    /// Try to propagate shape from src's output to tgt's input.
    /// Returns an inferred ShapeConstraint for tgt if successful.
    fn try_propagate_shape(
        &self,
        src: &Node,
        tgt: &Node,
        sub: &Subgraph,
    ) -> Option<ShapeConstraint> {
        let (tgt_args, tgt_meta) = self.unresolved_actor_shape_target(tgt)?;

        // Get src's resolved output shape (as a list of concrete dim values)
        let src_dims = self.resolve_output_shape_dims(src, sub)?;
        let existing = self.inferred_shapes.get(&tgt.id);
        let (sc, changed) = self.merge_propagated_shape(
            tgt.id,
            &tgt_meta.in_shape,
            tgt_meta,
            tgt_args,
            None,
            existing,
            &src_dims,
            tgt.span,
        )?;
        changed.then_some(sc)
    }

    /// Try to propagate shape from tgt's input back to src's output.
    /// Returns an inferred ShapeConstraint for src if successful.
    fn try_propagate_shape_reverse(
        &self,
        tgt: &Node,
        src: &Node,
        sub: &Subgraph,
    ) -> Option<ShapeConstraint> {
        let (src_args, src_meta) = self.unresolved_actor_shape_target(src)?;

        // Only propagate backward when tgt's input shape has symbolic dimensions.
        // If all dims are Literal, the shape is fixed by the actor definition
        // (e.g. stdout IN(float,1)) and should not be treated as a frame dimension
        // to propagate backward.
        if self.target_has_fixed_actor_input_shape(tgt) {
            return None;
        }

        // Get tgt's resolved input shape dims
        let tgt_dims = self.resolve_input_shape_dims(tgt, sub)?;
        let src_out_rank = src_meta.out_shape.rank();

        if tgt_dims.len() != src_out_rank {
            return None;
        }

        let existing = self.inferred_shapes.get(&src.id);
        let (sc, changed) = self.merge_propagated_shape(
            src.id,
            &src_meta.out_shape,
            src_meta,
            src_args,
            None,
            existing,
            &tgt_dims,
            src.span,
        )?;
        changed.then_some(sc)
    }

    /// Resolve a node's output shape to concrete dim values.
    /// For passthrough nodes (Fork, Probe), traces upstream to find the shape.
    fn resolve_output_shape_dims(&self, node: &Node, sub: &Subgraph) -> Option<Vec<u32>> {
        match &node.kind {
            NodeKind::Actor {
                name,
                args,
                shape_constraint,
                ..
            } => {
                let meta = self.actor_meta(name)?;
                let sc = shape_constraint
                    .as_ref()
                    .or_else(|| self.inferred_shapes.get(&node.id));
                self.resolve_shape_to_dims(&meta.out_shape, meta, args, sc)
            }
            // Passthrough nodes: trace upstream to find an actor with known shape
            NodeKind::Fork { .. } | NodeKind::Probe { .. } => {
                self.trace_shape_backward(node.id, sub)
            }
            _ => None,
        }
    }

    /// Resolve a node's input shape to concrete dim values.
    /// For passthrough nodes, traces downstream to find the shape.
    fn resolve_input_shape_dims(&self, node: &Node, sub: &Subgraph) -> Option<Vec<u32>> {
        match &node.kind {
            NodeKind::Actor {
                name,
                args,
                shape_constraint,
                ..
            } => {
                let meta = self.actor_meta(name)?;
                let sc = shape_constraint
                    .as_ref()
                    .or_else(|| self.inferred_shapes.get(&node.id));
                self.resolve_shape_to_dims(&meta.in_shape, meta, args, sc)
            }
            // Passthrough nodes: trace downstream to find an actor with known shape
            NodeKind::Fork { .. } | NodeKind::Probe { .. } => {
                self.trace_shape_forward(node.id, sub)
            }
            _ => None,
        }
    }

    /// Trace backward from a passthrough node to find resolved output shape dims.
    fn trace_shape_backward(&self, node_id: NodeId, sub: &Subgraph) -> Option<Vec<u32>> {
        let mut current = node_id;
        let mut visited = HashSet::new();
        loop {
            if !visited.insert(current) {
                return None;
            }
            let node = self.node_in_subgraph(sub, current)?;
            if let NodeKind::Actor { .. } = &node.kind {
                return self.resolve_output_shape_dims(node, sub);
            }
            let pred = self.first_incoming_edge_in_subgraph(sub, current);
            match pred {
                Some(edge) => current = edge.source,
                None => return None,
            }
        }
    }

    /// Trace forward from a passthrough node to find resolved input shape dims.
    fn trace_shape_forward(&self, node_id: NodeId, sub: &Subgraph) -> Option<Vec<u32>> {
        let mut current = node_id;
        let mut visited = HashSet::new();
        loop {
            if !visited.insert(current) {
                return None;
            }
            let node = self.node_in_subgraph(sub, current)?;
            if let NodeKind::Actor { .. } = &node.kind {
                return self.resolve_input_shape_dims(node, sub);
            }
            // Find first successor
            let succ = self.first_outgoing_edge_in_subgraph(sub, current);
            match succ {
                Some(edge) => current = edge.target,
                None => return None,
            }
        }
    }

    /// Resolve each dimension of a PortShape to a concrete u32 value.
    fn resolve_shape_to_dims(
        &self,
        shape: &PortShape,
        meta: &ActorMeta,
        args: &[Arg],
        shape_constraint: Option<&ShapeConstraint>,
    ) -> Option<Vec<u32>> {
        let mut dims = Vec::with_capacity(shape.dims.len());
        for (i, dim) in shape.dims.iter().enumerate() {
            let val = match dim {
                TokenCount::Literal(n) => Some(*n),
                TokenCount::Symbolic(sym) => {
                    let from_arg = meta
                        .params
                        .iter()
                        .position(|p| p.name == *sym)
                        .and_then(|idx| args.get(idx))
                        .and_then(|arg| self.resolve_arg_to_u32(arg));
                    if from_arg.is_some() {
                        from_arg
                    } else {
                        let from_shape = shape_constraint
                            .and_then(|sc| sc.dims.get(i))
                            .and_then(|sd| self.resolve_shape_dim(sd));
                        if from_shape.is_some() {
                            from_shape
                        } else {
                            self.infer_dim_param_from_span_args(sym, meta, args)
                        }
                    }
                }
            };
            dims.push(val?);
        }
        Some(dims)
    }

    fn infer_dim_param_from_span_args(
        &self,
        dim_name: &str,
        actor_meta: &ActorMeta,
        actor_args: &[Arg],
    ) -> Option<u32> {
        self.thir
            .infer_dim_param_from_span_args(dim_name, actor_meta, actor_args)
    }

    // ── Phase 0a2: Record span-derived dimension params ─────────────────
    //
    // For actors whose symbolic dimension can be inferred from a span argument
    // length (e.g., fir(coeff) → N = len(coeff)), store the authoritative value
    // so downstream phases don't need to re-derive it and can detect conflicts.

    fn record_span_derived_dims(&mut self) {
        let mut entries = Vec::new();
        for &(_, _, sub) in &self.all_subgraphs {
            for node in &sub.nodes {
                if let NodeKind::Actor { name, args, .. } = &node.kind {
                    if let Some(meta) = self.actor_meta(name) {
                        // Deduplicate: collect unique symbolic dim names
                        let mut seen = HashSet::new();
                        for dim in meta.in_shape.dims.iter().chain(meta.out_shape.dims.iter()) {
                            if let TokenCount::Symbolic(sym) = dim {
                                if !seen.insert(sym.clone()) {
                                    continue;
                                }
                                // Skip if already resolvable from explicit args
                                let from_arg = meta
                                    .params
                                    .iter()
                                    .position(|p| p.name == *sym)
                                    .and_then(|idx| args.get(idx))
                                    .and_then(|arg| self.resolve_arg_to_u32(arg));
                                if from_arg.is_some() {
                                    continue;
                                }
                                if let Some(val) =
                                    self.infer_dim_param_from_span_args(sym, meta, args)
                                {
                                    entries.push((node.id, sym.clone(), val));
                                }
                            }
                        }
                    }
                }
            }
        }
        for (node_id, sym, val) in entries {
            self.span_derived_dims
                .entry(node_id)
                .or_default()
                .insert(sym, val);
        }
    }

    // ── Phase 0b: Shape constraint validation (§13.6) ───────────────────

    fn check_shape_constraints(&mut self) {
        let subs = std::mem::take(&mut self.all_subgraphs);
        for &(_, _, sub) in &subs {
            self.check_shape_constraints_in_subgraph(sub);
        }
        self.all_subgraphs = subs;
    }

    fn check_shape_constraints_in_subgraph(&mut self, sub: &Subgraph) {
        for node in &sub.nodes {
            self.check_node_dim_constraints(node);
        }
        self.check_edge_shape_conflicts(sub);
    }

    /// Merged per-node dimension constraint check:
    /// 1. Unresolved frame dims (E0300)
    /// 2. Dim source conflicts: arg vs span-derived, shape constraint vs span-derived (E0302)
    fn check_node_dim_constraints(&mut self, node: &Node) {
        let NodeKind::Actor {
            name,
            args,
            shape_constraint,
            ..
        } = &node.kind
        else {
            return;
        };
        let Some(meta) = self.actor_meta(name) else {
            return;
        };

        // ── 1. Collect unique symbolic dims with sc_index in one pass ──
        // sc_index: position of first occurrence in in_shape or out_shape
        // (out_shape enumeration restarts at 0, matching shape_constraint positional semantics)
        let mut seen = HashSet::new();
        let mut symbolic_dims: Vec<(&str, usize)> = Vec::new();
        for (i, dim) in meta.in_shape.dims.iter().enumerate() {
            if let TokenCount::Symbolic(sym) = dim {
                if seen.insert(sym.as_str()) {
                    symbolic_dims.push((sym.as_str(), i));
                }
            }
        }
        for (i, dim) in meta.out_shape.dims.iter().enumerate() {
            if let TokenCount::Symbolic(sym) = dim {
                if seen.insert(sym.as_str()) {
                    symbolic_dims.push((sym.as_str(), i));
                }
            }
        }

        // Collect all errors before emitting (avoids borrow conflict with meta)
        // Format: (code, span, message, hint)
        let mut pending: Vec<(DiagCode, Span, String, String)> = Vec::new();

        // ── 2. Unresolved frame dim check (E0300) ──
        if shape_constraint.is_none() && args.is_empty() {
            if let Some(&(first_sym, _)) = symbolic_dims.first() {
                if self.production_rate(node).is_none() && self.consumption_rate(node).is_none() {
                    pending.push((
                        codes::E0300,
                        node.span,
                        format!(
                            "unresolved frame dimension '{}' at actor '{}'",
                            first_sym, name
                        ),
                        format!("add explicit shape constraint, e.g. {}()[<size>]", name),
                    ));
                }
            }
        }

        // ── 3. Dim source conflict checks (E0302) ──
        for &(sym_name, sc_idx) in &symbolic_dims {
            // Compute span-inferred value directly (not from span_derived_dims map,
            // since that map skips entries when explicit arg is present).
            let span_val = match self.thir.span_arg_length_for_dim(sym_name, meta, args) {
                Some(v) => v,
                None => continue,
            };

            // Check: explicit arg vs span-derived
            let param_idx = meta.params.iter().position(|p| p.name == sym_name);
            let from_arg = param_idx
                .and_then(|idx| args.get(idx))
                .and_then(|arg| self.thir.resolve_arg_to_u32(arg));
            if let Some(arg_val) = from_arg {
                if arg_val != span_val {
                    pending.push((
                        codes::E0302,
                        node.span,
                        format!(
                            "conflicting dimension '{}' at actor '{}': \
                             explicit argument specifies {}, but span-derived value is {}",
                            sym_name, name, arg_val, span_val
                        ),
                        "remove explicit argument to auto-infer from span, \
                         or align span length with explicit argument"
                            .to_string(),
                    ));
                }
            }

            // Check: shape constraint vs span-derived
            if let Some(sc) = shape_constraint {
                if let Some(sc_val) = sc
                    .dims
                    .get(sc_idx)
                    .and_then(|sd| self.thir.resolve_shape_dim(sd))
                {
                    if sc_val != span_val {
                        pending.push((
                            codes::E0302,
                            sc.span,
                            format!(
                                "conflicting dimension '{}' at actor '{}': \
                                 shape constraint specifies {}, but span-derived value is {}",
                                sym_name, name, sc_val, span_val
                            ),
                            "align the shape constraint with the span argument length".to_string(),
                        ));
                    }
                }
            }
        }

        for (code, span, message, hint) in pending {
            self.error_with_hint(code, span, message, hint);
        }
    }

    fn check_edge_shape_conflicts(&mut self, sub: &Subgraph) {
        for edge in &sub.edges {
            if let Some((span, message)) = self.find_shape_conflict_on_edge(sub, edge) {
                self.error(codes::E0301, span, message);
            }
        }

        // Check 4: span-derived vs edge-inferred dimension conflicts (v0.3.1)
        // Check both input and output shape dims by symbolic name.
        let mut conflicts = Vec::new();
        for node in &sub.nodes {
            if let NodeKind::Actor { name, .. } = &node.kind {
                if let Some(inferred_sc) = self.inferred_shapes.get(&node.id) {
                    if let Some(meta) = self.actor_meta(name) {
                        // Check in_shape dims
                        for (i, dim) in meta.in_shape.dims.iter().enumerate() {
                            self.check_span_vs_inferred_dim(
                                node,
                                name,
                                dim,
                                i,
                                inferred_sc,
                                &mut conflicts,
                            );
                        }
                        // Check out_shape dims
                        for (i, dim) in meta.out_shape.dims.iter().enumerate() {
                            self.check_span_vs_inferred_dim(
                                node,
                                name,
                                dim,
                                i,
                                inferred_sc,
                                &mut conflicts,
                            );
                        }
                    }
                }
            }
        }
        for (span, msg) in conflicts {
            self.error(codes::E0302, span, msg);
        }
    }

    fn check_span_vs_inferred_dim(
        &self,
        node: &Node,
        actor_name: &str,
        dim: &TokenCount,
        dim_idx: usize,
        inferred_sc: &ShapeConstraint,
        conflicts: &mut Vec<(Span, String)>,
    ) {
        if let TokenCount::Symbolic(sym) = dim {
            if let Some(&span_val) = self
                .span_derived_dims
                .get(&node.id)
                .and_then(|m| m.get(sym.as_str()))
            {
                if let Some(inferred_val) = inferred_sc
                    .dims
                    .get(dim_idx)
                    .and_then(|sd| self.resolve_shape_dim(sd))
                {
                    if span_val != inferred_val {
                        conflicts.push((
                            node.span,
                            format!(
                                "conflicting dimension '{}' at actor '{}': \
                                 span-derived value {} vs edge-inferred value {}",
                                sym, actor_name, span_val, inferred_val
                            ),
                        ));
                    }
                }
            }
        }
    }

    fn find_shape_conflict_on_edge(&self, sub: &Subgraph, edge: &Edge) -> Option<(Span, String)> {
        let src = self.node_in_subgraph(sub, edge.source)?;
        let tgt = self.node_in_subgraph(sub, edge.target)?;
        let (tgt_name, tgt_sc) = match &tgt.kind {
            NodeKind::Actor {
                name,
                shape_constraint: Some(sc),
                ..
            } => (name.as_str(), sc),
            _ => return None,
        };
        let src_dims = self.resolve_output_shape_dims(src, sub)?;
        let tgt_dims: Vec<u32> = tgt_sc
            .dims
            .iter()
            .map(|d| self.resolve_shape_dim(d))
            .collect::<Option<Vec<_>>>()?;

        if src_dims.len() != tgt_dims.len() {
            return None;
        }

        for (i, (&sv, &tv)) in src_dims.iter().zip(tgt_dims.iter()).enumerate() {
            if sv != tv {
                return Some((
                    tgt_sc.span,
                    format!(
                        "conflicting frame constraint for actor '{}': \
                         inferred dim[{}]={} from upstream, but explicit shape specifies {}",
                        tgt_name, i, sv, tv
                    ),
                ));
            }
        }
        None
    }

    // ── Phase 0c: Dimension PARAM order advisory ───────────────────────

    fn check_dimension_param_order(&mut self) {
        let mut checked: HashSet<String> = HashSet::new();
        let subs = std::mem::take(&mut self.all_subgraphs);
        for &(_, _, sub) in &subs {
            for node in &sub.nodes {
                let actor_name = match &node.kind {
                    NodeKind::Actor { name, .. } => name,
                    _ => continue,
                };
                if !checked.insert(actor_name.clone()) {
                    continue;
                }
                let Some(meta) = self.actor_meta(actor_name) else {
                    continue;
                };
                let dim_param_indices = self.inferred_dimension_param_indices(meta);
                if dim_param_indices.is_empty() {
                    continue;
                }
                let suffix_start = meta.params.len() - dim_param_indices.len();
                let dims_at_suffix = dim_param_indices
                    .iter()
                    .enumerate()
                    .all(|(offset, idx)| *idx == suffix_start + offset);
                if dims_at_suffix {
                    continue;
                }

                let dim_names = dim_param_indices
                    .iter()
                    .map(|idx| meta.params[*idx].name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                self.warning_with_hint(
                    codes::W0300,
                    node.span,
                    format!(
                        "actor '{}' declares inferred dimension PARAM(s) [{}] before non-dimension parameters",
                        actor_name, dim_names
                    ),
                    "move inferred dimension PARAM(int, ...) to the end of ACTOR(...) parameters"
                        .to_string(),
                );
            }
        }
        self.all_subgraphs = subs;
    }

    fn inferred_dimension_param_indices(&self, meta: &ActorMeta) -> Vec<usize> {
        let mut dim_names: HashSet<&str> = HashSet::new();
        for dim in meta.in_shape.dims.iter().chain(meta.out_shape.dims.iter()) {
            if let TokenCount::Symbolic(sym) = dim {
                dim_names.insert(sym.as_str());
            }
        }
        if dim_names.is_empty() {
            return Vec::new();
        }
        meta.params
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                p.kind == ParamKind::Param
                    && p.param_type == ParamType::Int
                    && dim_names.contains(p.name.as_str())
            })
            .map(|(idx, _)| idx)
            .collect()
    }

    // ── Phase 1: Type checking ──────────────────────────────────────────

    fn check_types(&mut self) {
        let subs = std::mem::take(&mut self.all_subgraphs);
        for &(_, _, sub) in &subs {
            self.check_types_in_subgraph(sub);
        }
        self.all_subgraphs = subs;
    }

    fn check_types_in_subgraph(&mut self, sub: &Subgraph) {
        for edge in &sub.edges {
            let src_node = match self.node_in_subgraph(sub, edge.source) {
                Some(n) => n,
                None => continue,
            };
            let tgt_node = match self.node_in_subgraph(sub, edge.target) {
                Some(n) => n,
                None => continue,
            };

            let src_type = self.infer_output_type(src_node, sub);
            let tgt_type = self.infer_input_type(tgt_node, sub);

            if let (Some(st), Some(tt)) = (src_type, tgt_type) {
                if st == PipitType::Void || tt == PipitType::Void {
                    continue;
                }
                if st != tt {
                    let src_name = node_display_name(src_node);
                    let tgt_name = node_display_name(tgt_node);
                    let mut d = Diagnostic::new(
                        DiagLevel::Error,
                        edge.span,
                        format!(
                            "type mismatch at pipe '{} -> {}': {} outputs {}, but {} expects {}",
                            src_name, tgt_name, src_name, st, tgt_name, tt
                        ),
                    )
                    .with_code(codes::E0303)
                    .with_hint(format!(
                        "insert a conversion actor between {} and {} (e.g. c2r, mag)",
                        src_name, tgt_name
                    ))
                    .with_related(src_node.span, format!("{} produces {}", src_name, st))
                    .with_related(tgt_node.span, format!("{} expects {}", tgt_name, tt));
                    d = d.with_cause(
                        format!("{} output type is {}", src_name, st),
                        Some(src_node.span),
                    );
                    d = d.with_cause(
                        format!("{} input type is {}", tgt_name, tt),
                        Some(tgt_node.span),
                    );
                    self.diagnostics.push(d);
                }
            }
        }
    }

    // ── Phase 2: SDF balance equation solving ───────────────────────────

    fn solve_balance_equations(&mut self) {
        let subs = std::mem::take(&mut self.all_subgraphs);
        for &(task_name, label, sub) in &subs {
            self.solve_subgraph_balance(task_name, label, sub);
        }
        self.all_subgraphs = subs;
    }

    fn solve_subgraph_balance(&mut self, task_name: &str, label: &str, sub: &Subgraph) {
        if sub.nodes.is_empty() {
            return;
        }

        let balance = self.build_balance_graph(sub);
        let rv_rat = self.solve_balance_ratios(sub, &balance);
        let rv = normalize_repetition_vector(&rv_rat);
        let consistent = self.verify_balance_equations(sub, task_name, &balance.rates, &rv);
        if consistent && !rv.is_empty() {
            let task_rv = self.rv_by_task.entry(task_name.to_string()).or_default();
            for (&node_id, &count) in &rv {
                task_rv.insert(node_id, count);
            }
            self.repetition_vectors
                .insert((task_name.to_string(), label.to_string()), rv);
        }
    }

    fn build_balance_graph(&self, sub: &Subgraph) -> BalanceGraph {
        let mut incoming_count: HashMap<NodeId, u32> = HashMap::new();
        for edge in &sub.edges {
            *incoming_count.entry(edge.target).or_insert(0) += 1;
        }

        let mut rates: HashMap<(NodeId, NodeId), (u32, u32)> = HashMap::new();
        let mut adjacency: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        for node in &sub.nodes {
            adjacency.entry(node.id).or_default();
        }

        for edge in &sub.edges {
            let Some(src) = self.node_in_subgraph(sub, edge.source) else {
                continue;
            };
            let Some(tgt) = self.node_in_subgraph(sub, edge.target) else {
                continue;
            };

            let p = self.cached_production_rate(src).unwrap_or(1);
            let total_c = self.cached_consumption_rate(tgt).unwrap_or(1);
            let num_in = incoming_count.get(&edge.target).copied().unwrap_or(1);
            let c = if num_in > 1 {
                total_c / num_in
            } else {
                total_c
            };

            rates.insert((edge.source, edge.target), (p, c));
            adjacency.entry(edge.source).or_default().push(edge.target);
            adjacency.entry(edge.target).or_default().push(edge.source);
        }

        BalanceGraph { rates, adjacency }
    }

    fn solve_balance_ratios(
        &self,
        sub: &Subgraph,
        balance: &BalanceGraph,
    ) -> HashMap<NodeId, (u64, u64)> {
        let mut rv_rat: HashMap<NodeId, (u64, u64)> = HashMap::new();
        let mut queue = std::collections::VecDeque::new();

        for node in &sub.nodes {
            if rv_rat.contains_key(&node.id) {
                continue;
            }
            rv_rat.insert(node.id, (1, 1));
            queue.push_back(node.id);

            while let Some(current) = queue.pop_front() {
                let Some((cur_num, cur_den)) = rv_rat.get(&current).copied() else {
                    continue;
                };
                let Some(neighbors) = balance.adjacency.get(&current) else {
                    continue;
                };

                for &neighbor in neighbors {
                    if rv_rat.contains_key(&neighbor) {
                        continue;
                    }
                    if let Some(next_ratio) =
                        self.propagate_ratio(current, neighbor, cur_num, cur_den, &balance.rates)
                    {
                        rv_rat.insert(neighbor, next_ratio);
                        queue.push_back(neighbor);
                    }
                }
            }
        }

        rv_rat
    }

    fn propagate_ratio(
        &self,
        current: NodeId,
        neighbor: NodeId,
        cur_num: u64,
        cur_den: u64,
        rates: &HashMap<(NodeId, NodeId), (u32, u32)>,
    ) -> Option<(u64, u64)> {
        if let Some(&(p, c)) = rates.get(&(current, neighbor)) {
            return Some(reduce_ratio(cur_num * p as u64, cur_den * c as u64));
        }
        if let Some(&(p, c)) = rates.get(&(neighbor, current)) {
            return Some(reduce_ratio(cur_num * c as u64, cur_den * p as u64));
        }
        None
    }

    fn verify_balance_equations(
        &mut self,
        sub: &Subgraph,
        task_name: &str,
        rates: &HashMap<(NodeId, NodeId), (u32, u32)>,
        rv: &HashMap<NodeId, u32>,
    ) -> bool {
        let mut consistent = true;
        for edge in &sub.edges {
            let Some(&(p, c)) = rates.get(&(edge.source, edge.target)) else {
                continue;
            };
            let lhs = rv.get(&edge.source).copied().unwrap_or(1) as u64 * p as u64;
            let rhs = rv.get(&edge.target).copied().unwrap_or(1) as u64 * c as u64;
            if lhs == rhs {
                continue;
            }
            consistent = false;
            self.report_balance_mismatch(sub, edge, task_name, rv, p, c);
        }
        consistent
    }

    fn report_balance_mismatch(
        &mut self,
        sub: &Subgraph,
        edge: &Edge,
        task_name: &str,
        rv: &HashMap<NodeId, u32>,
        p: u32,
        c: u32,
    ) {
        let src = self.node_in_subgraph(sub, edge.source);
        let tgt = self.node_in_subgraph(sub, edge.target);
        let src_name = src.map(node_display_name).unwrap_or("?".into());
        let tgt_name = tgt.map(node_display_name).unwrap_or("?".into());
        let src_rep = *rv.get(&edge.source).unwrap_or(&0);
        let tgt_rep = *rv.get(&edge.target).unwrap_or(&0);
        let mut d = Diagnostic::new(
            DiagLevel::Error,
            edge.span,
            format!(
                "SDF balance equation unsolvable at edge '{} -> {}' in task '{}': \
                 {}×{} ≠ {}×{}",
                src_name, tgt_name, task_name, src_rep, p, tgt_rep, c
            ),
        )
        .with_code(codes::E0304);
        if let Some(s) = src {
            d = d.with_related(s.span, format!("producer {}: out_count={}", src_name, p));
        }
        if let Some(t) = tgt {
            d = d.with_related(t.span, format!("consumer {}: in_count={}", tgt_name, c));
        }
        d = d.with_cause(
            format!(
                "{} produces {}×{} = {} tokens per cycle",
                src_name,
                src_rep,
                p,
                src_rep as u64 * p as u64
            ),
            src.map(|n| n.span),
        );
        d = d.with_cause(
            format!(
                "{} consumes {}×{} = {} tokens per cycle",
                tgt_name,
                tgt_rep,
                c,
                tgt_rep as u64 * c as u64
            ),
            tgt.map(|n| n.span),
        );
        self.diagnostics.push(d);
    }

    // ── Phase 3: Feedback loop delay verification ───────────────────────

    fn check_feedback_delays(&mut self) {
        for cycle in &self.graph.cycles {
            if cycle.is_empty() {
                continue;
            }
            let has_delay = cycle.iter().any(|&nid| {
                self.find_node_in_any_subgraph(nid)
                    .map(|node| {
                        matches!(
                            &node.kind,
                            NodeKind::Actor { name, .. } if name == "delay"
                        )
                    })
                    .unwrap_or(false)
            });
            if !has_delay {
                let cycle_desc = self.format_cycle_path(cycle);
                let span = self
                    .find_node_in_any_subgraph(cycle[0])
                    .map(|n| n.span)
                    .unwrap_or(Span::new((), 0..0));
                self.error_with_hint(
                    codes::E0305,
                    span,
                    format!("feedback loop detected at '{}' with no delay", cycle_desc),
                    "insert delay(N, init) to break the cycle".to_string(),
                );
            }
        }
    }

    fn find_node_in_any_subgraph(&self, node_id: NodeId) -> Option<&Node> {
        if let Some((sub_key, node_pos)) = self.global_node_index.get(&node_id).copied() {
            return self
                .subgraph_refs
                .get(&sub_key)
                .and_then(|sub| sub.nodes.get(node_pos));
        }

        for &(_, _, sub) in &self.all_subgraphs {
            if let Some(node) = find_node(sub, node_id) {
                return Some(node);
            }
        }
        None
    }

    fn format_cycle_path(&self, cycle: &[NodeId]) -> String {
        let names: Vec<String> = cycle
            .iter()
            .filter_map(|&nid| self.find_node_in_any_subgraph(nid))
            .map(node_display_name)
            .collect();
        names.join(" -> ")
    }

    // ── Phase 4: Cross-clock rate matching ──────────────────────────────

    fn check_cross_clock_rates(&mut self) {
        for edge in &self.graph.inter_task_edges {
            let fw = self.get_task_freq(&edge.writer_task);
            let fr = self.get_task_freq(&edge.reader_task);

            let pw = self.get_rv_for_node(&edge.writer_task, edge.writer_node);
            let cr = self.get_rv_for_node(&edge.reader_task, edge.reader_node);

            if let (Some((fw_val, _)), Some((fr_val, _)), Some(pw_val), Some(cr_val)) =
                (fw, fr, pw, cr)
            {
                let writer_rate = pw_val as f64 * fw_val;
                let reader_rate = cr_val as f64 * fr_val;

                if writer_rate > 0.0 && reader_rate > 0.0 {
                    let ratio = writer_rate / reader_rate;
                    if (ratio - 1.0).abs() > 1e-6 {
                        let span = self
                            .thir
                            .resolved
                            .buffers
                            .get(&edge.buffer_name)
                            .map(|b| b.writer_span)
                            .unwrap_or(Span::new((), 0..0));
                        let msg = format!(
                            "rate mismatch at shared buffer '{}': \
                             writer '{}' produces {:.0} tokens/sec, \
                             reader '{}' consumes {:.0} tokens/sec",
                            edge.buffer_name,
                            edge.writer_task,
                            writer_rate,
                            edge.reader_task,
                            reader_rate,
                        );
                        self.error(codes::E0306, span, msg);
                    }
                }
            }
        }
    }

    /// Look up the repetition vector value for a specific node in a task.
    fn get_rv_for_node(&self, task_name: &str, node_id: NodeId) -> Option<u32> {
        self.rv_by_task
            .get(task_name)
            .and_then(|rv| rv.get(&node_id))
            .copied()
    }

    // ── Bind contract inference (§5.5) ──────────────────────────────────
    //
    // Infers direction (in/out) and data contract (dtype/shape/rate) for each
    // bind declaration by scanning the post-expansion graph. Runs after
    // solve_balance_equations() so repetition vectors are available.

    fn infer_bind_contracts(&mut self) {
        let bind_names: Vec<String> = self.thir.binds().iter().map(|b| b.name.clone()).collect();

        for bind_name in &bind_names {
            let has_writer = self.graph_has_buffer_node(bind_name, true);
            let has_reader = self.graph_has_buffer_node(bind_name, false);

            let direction = if has_writer {
                // Spec §5.5.1 first-match rule: -> name exists → Out
                // (regardless of whether @name also exists)
                BindDirection::Out
            } else if has_reader {
                BindDirection::In
            } else {
                let span = self
                    .thir
                    .bind_info(bind_name)
                    .map(|b| b.name_span)
                    .unwrap_or(Span::new((), 0..0));
                self.error(
                    codes::E0311,
                    span,
                    format!(
                        "bind '{}' is not referenced by any pipe \
                         (expected @{0} or -> {0})",
                        bind_name
                    ),
                );
                continue;
            };

            let mut contract = match direction {
                BindDirection::Out => self.infer_out_bind_contract(bind_name),
                BindDirection::In => self.infer_in_bind_contract(bind_name),
            };

            // Compute stable_id from graph lineage (§5.5.3).
            let transport = self
                .thir
                .bind_info(bind_name)
                .map(|b| b.endpoint.transport.name.as_str())
                .unwrap_or("");
            let call_ids = self.collect_bind_call_ids(bind_name, direction);
            contract.stable_id = compute_stable_id(direction, &call_ids, transport);

            self.bind_contracts.insert(bind_name.clone(), contract);
        }
    }

    /// Validate SHM bind endpoint arguments (slots, slot_bytes, name).
    ///
    /// Preconditions: called after `infer_bind_contracts()` so binds are available.
    /// Postconditions: emits E0720–E0726 for invalid SHM endpoints.
    fn validate_bind_endpoints(&mut self) {
        let binds: Vec<_> = self
            .thir
            .binds()
            .iter()
            .filter(|b| b.endpoint.transport.name == "shm")
            .map(|b| (b.name.clone(), b.endpoint.clone()))
            .collect();

        for (name, ep) in &binds {
            let span = ep.span;

            // Check positional name arg
            let has_positional = ep.args.iter().any(|a| matches!(a, BindArg::Positional(_)));
            if !has_positional {
                self.error(
                    codes::E0724,
                    span,
                    format!("shm bind '{}': missing required name argument", name),
                );
            }

            // Check named arg: slots
            self.validate_shm_int_arg(name, &ep.args, "slots", span, codes::E0720, codes::E0722);

            // Check named arg: slot_bytes
            self.validate_shm_int_arg(
                name,
                &ep.args,
                "slot_bytes",
                span,
                codes::E0721,
                codes::E0723,
            );

            // Check slot_bytes alignment (must be multiple of 8)
            if let Some(slot_bytes_val) = self.find_named_number(&ep.args, "slot_bytes") {
                let v = slot_bytes_val as u64;
                if v > 0 && !v.is_multiple_of(8) {
                    self.error_with_hint(
                        codes::E0726,
                        span,
                        format!(
                            "shm bind '{}': slot_bytes={} is not a multiple of 8",
                            name, v
                        ),
                        "slot_bytes must be 8-byte aligned for atomic field access".to_string(),
                    );
                }
            }
        }
    }

    /// Validate a required named integer argument for an SHM endpoint.
    fn validate_shm_int_arg(
        &mut self,
        bind_name: &str,
        args: &[BindArg],
        arg_name: &str,
        span: Span,
        missing_code: DiagCode,
        zero_code: DiagCode,
    ) {
        let named = args.iter().find_map(|a| match a {
            BindArg::Named(ident, scalar) if ident.name == arg_name => Some(scalar),
            _ => None,
        });
        match named {
            None => {
                self.error(
                    missing_code,
                    span,
                    format!(
                        "shm bind '{}': missing required '{}' argument",
                        bind_name, arg_name
                    ),
                );
            }
            Some(Scalar::Number(val, _, is_int)) => {
                if !is_int {
                    self.error_with_hint(
                        codes::E0725,
                        span,
                        format!(
                            "shm bind '{}': '{}' must be an integer literal",
                            bind_name, arg_name
                        ),
                        format!("use an integer value like {}=1024", arg_name),
                    );
                } else if *val <= 0.0 {
                    self.error(
                        zero_code,
                        span,
                        format!(
                            "shm bind '{}': '{}' must be > 0 (got {})",
                            bind_name, arg_name, *val as i64
                        ),
                    );
                }
            }
            Some(Scalar::Ident(_)) => {
                self.error_with_hint(
                    codes::E0725,
                    span,
                    format!(
                        "shm bind '{}': '{}' must be an integer literal, not a const reference",
                        bind_name, arg_name
                    ),
                    format!(
                        "replace with a literal value like {}=1024; const refs for slots/slot_bytes are not supported",
                        arg_name
                    ),
                );
            }
            _ => {
                self.error(
                    codes::E0725,
                    span,
                    format!(
                        "shm bind '{}': '{}' must be an integer literal",
                        bind_name, arg_name
                    ),
                );
            }
        }
    }

    /// Find a named number argument value (helper for alignment check).
    fn find_named_number(&self, args: &[BindArg], name: &str) -> Option<f64> {
        args.iter().find_map(|a| match a {
            BindArg::Named(ident, Scalar::Number(val, _, true)) if ident.name == name => Some(*val),
            _ => None,
        })
    }

    /// Check whether the post-expansion graph contains a BufferWrite or
    /// BufferRead node matching the given buffer name.
    fn graph_has_buffer_node(&self, buffer_name: &str, is_write: bool) -> bool {
        for task_graph in self.graph.tasks.values() {
            for sub in subgraphs_of(task_graph) {
                for node in &sub.nodes {
                    match (&node.kind, is_write) {
                        (NodeKind::BufferWrite { buffer_name: n }, true) if n == buffer_name => {
                            return true
                        }
                        (NodeKind::BufferRead { buffer_name: n }, false) if n == buffer_name => {
                            return true
                        }
                        _ => {}
                    }
                }
            }
        }
        false
    }

    /// Collect the adjacent actor CallIds for a bind from the graph.
    ///
    /// For OUT binds: searches all task subgraphs for BufferWrite nodes matching
    /// the bind name, then walks predecessors to the upstream actor.
    ///
    /// For IN binds: searches all task subgraphs for BufferRead nodes matching
    /// the bind name, then walks successors to the downstream actors.
    ///
    /// Note: We search subgraphs directly rather than using InterTaskEdge because
    /// bind-backed buffers may not have inter-task edges (e.g., OUT binds with
    /// no internal reader, IN binds with no internal writer).
    fn collect_bind_call_ids(&self, bind_name: &str, direction: BindDirection) -> Vec<CallId> {
        let mut call_ids = Vec::new();
        let is_write = direction == BindDirection::Out;

        // Sorted iteration for deterministic ordering.
        let mut task_names: Vec<&String> = self.graph.tasks.keys().collect();
        task_names.sort();

        for task_name in task_names {
            let task_graph = &self.graph.tasks[task_name];
            for sub in subgraphs_of(task_graph) {
                for node in &sub.nodes {
                    let matches = match (&node.kind, is_write) {
                        (NodeKind::BufferWrite { buffer_name: n }, true) => n == bind_name,
                        (NodeKind::BufferRead { buffer_name: n }, false) => n == bind_name,
                        _ => false,
                    };
                    if matches {
                        // walk_predecessors for BufferWrite (find upstream actor),
                        // walk_successors for BufferRead (find downstream actor).
                        if let Some(cid) = adjacent_actor_call_id(task_graph, node.id, is_write) {
                            call_ids.push(cid);
                        }
                    }
                }
            }
        }

        if !is_write {
            call_ids.sort(); // sorted for deterministic hashing (IN may have multiple readers)
        }
        call_ids
    }

    /// Infer contract for an OUT bind (pipeline writes, external reads).
    fn infer_out_bind_contract(&self, bind_name: &str) -> BindContract {
        let dtype = self.infer_bind_type_from_writer(bind_name);
        let shape = self.infer_bind_shape_from_writer(bind_name);
        let rate_hz = self.infer_bind_rate_from_writer(bind_name);

        BindContract {
            direction: BindDirection::Out,
            dtype,
            shape: shape.unwrap_or_default(),
            rate_hz,
            stable_id: String::new(), // filled by infer_bind_contracts after CallId extraction
        }
    }

    /// Trace backward from BufferWrite to the nearest Actor, using
    /// concrete_actor (lowered → registry) for polymorphic type resolution.
    fn trace_type_backward_concrete(&self, node_id: NodeId, sub: &Subgraph) -> Option<PipitType> {
        let mut current = node_id;
        let mut visited = Vec::new();
        loop {
            if visited.contains(&current) {
                return None;
            }
            visited.push(current);
            let node = self.node_in_subgraph(sub, current)?;
            if let NodeKind::Actor { name, call_id, .. } = &node.kind {
                return self
                    .thir
                    .concrete_actor(name, *call_id)
                    .and_then(|m| m.out_type.as_concrete());
            }
            let pred = self.first_incoming_edge_in_subgraph(sub, current)?;
            current = pred.source;
        }
    }

    /// Infer the wire type of a bind buffer from the writer side,
    /// using concrete actor metadata for polymorphic type resolution.
    fn infer_bind_type_from_writer(&self, bind_name: &str) -> Option<PipitType> {
        let buf_info = self.thir.resolved.buffers.get(bind_name)?;
        let task_graph = self.graph.tasks.get(&buf_info.writer_task)?;
        for sub in subgraphs_of(task_graph) {
            for node in &sub.nodes {
                if let NodeKind::BufferWrite { buffer_name } = &node.kind {
                    if buffer_name == bind_name {
                        return self.trace_type_backward_concrete(node.id, sub);
                    }
                }
            }
        }
        None
    }

    /// Infer contract for an IN bind (external writes, pipeline reads).
    fn infer_in_bind_contract(&mut self, bind_name: &str) -> BindContract {
        let dtype = self.infer_bind_type_from_readers(bind_name);
        let shape = self.infer_bind_shape_from_readers(bind_name);
        let rate_hz = self.infer_bind_rate_from_readers(bind_name);

        BindContract {
            direction: BindDirection::In,
            dtype,
            shape: shape.unwrap_or_default(),
            rate_hz,
            stable_id: String::new(), // filled by infer_bind_contracts after CallId extraction
        }
    }

    /// Trace forward from a node to the nearest downstream Actor and return
    /// that actor's input type. Mirror of `trace_type_backward()`.
    fn trace_type_forward(&self, node_id: NodeId, sub: &Subgraph) -> Option<PipitType> {
        let mut current = node_id;
        let mut visited = Vec::new();
        loop {
            if visited.contains(&current) {
                return None;
            }
            visited.push(current);
            let node = self.node_in_subgraph(sub, current)?;
            if let NodeKind::Actor { name, call_id, .. } = &node.kind {
                return self
                    .thir
                    .concrete_actor(name, *call_id)
                    .and_then(|m| m.in_type.as_concrete());
            }
            let succ = self.first_outgoing_edge_in_subgraph(sub, current)?;
            current = succ.target;
        }
    }

    /// Infer the type from all readers of an IN bind, validating consistency.
    fn infer_bind_type_from_readers(&mut self, bind_name: &str) -> Option<PipitType> {
        let mut types: Vec<PipitType> = Vec::new();

        // Sorted iteration for deterministic conflict diagnostics
        let mut task_names: Vec<&String> = self.graph.tasks.keys().collect();
        task_names.sort();

        for task_name in task_names {
            let task_graph = &self.graph.tasks[task_name];
            for sub in subgraphs_of(task_graph) {
                for node in &sub.nodes {
                    if let NodeKind::BufferRead { buffer_name } = &node.kind {
                        if buffer_name == bind_name {
                            if let Some(t) = self.trace_type_forward(node.id, sub) {
                                types.push(t);
                            }
                        }
                    }
                }
            }
        }

        if types.is_empty() {
            return None;
        }

        let first = types[0];
        for t in &types[1..] {
            if *t != first {
                let span = self
                    .thir
                    .bind_info(bind_name)
                    .map(|b| b.name_span)
                    .unwrap_or(Span::new((), 0..0));
                self.error(
                    codes::E0312,
                    span,
                    format!(
                        "bind '{}' readers disagree on type: {} vs {}",
                        bind_name, first, t
                    ),
                );
                return Some(first);
            }
        }
        Some(first)
    }

    /// Get resolved out_shape dims from the upstream actor of a BufferWrite.
    fn infer_bind_shape_from_writer(&self, bind_name: &str) -> Option<Vec<u32>> {
        let buf_info = self.thir.resolved.buffers.get(bind_name)?;
        let task_graph = self.graph.tasks.get(&buf_info.writer_task)?;
        for sub in subgraphs_of(task_graph) {
            for node in &sub.nodes {
                if let NodeKind::BufferWrite { buffer_name } = &node.kind {
                    if buffer_name == bind_name {
                        return self.trace_shape_backward(node.id, sub);
                    }
                }
            }
        }
        None
    }

    /// Get resolved in_shape dims from downstream actors of BufferRead nodes.
    /// Validates all readers agree on shape; emits E0312 on mismatch.
    fn infer_bind_shape_from_readers(&mut self, bind_name: &str) -> Option<Vec<u32>> {
        let mut shapes: Vec<Vec<u32>> = Vec::new();

        let mut task_names: Vec<&String> = self.graph.tasks.keys().collect();
        task_names.sort();

        for task_name in task_names {
            let task_graph = &self.graph.tasks[task_name];
            for sub in subgraphs_of(task_graph) {
                for node in &sub.nodes {
                    if let NodeKind::BufferRead { buffer_name } = &node.kind {
                        if buffer_name == bind_name {
                            if let Some(s) = self.trace_shape_forward(node.id, sub) {
                                shapes.push(s);
                            }
                        }
                    }
                }
            }
        }

        if shapes.is_empty() {
            return None;
        }

        let first = &shapes[0];
        for s in &shapes[1..] {
            if s != first {
                let span = self
                    .thir
                    .bind_info(bind_name)
                    .map(|b| b.name_span)
                    .unwrap_or(Span::new((), 0..0));
                self.error(
                    codes::E0312,
                    span,
                    format!(
                        "bind '{}' readers disagree on shape: {:?} vs {:?}",
                        bind_name, first, s
                    ),
                );
                return Some(first.clone());
            }
        }
        Some(first.clone())
    }

    /// Infer the data rate (Hz) for an OUT bind from the writer side.
    fn infer_bind_rate_from_writer(&self, bind_name: &str) -> Option<f64> {
        let buf_info = self.thir.resolved.buffers.get(bind_name)?;
        if buf_info.writer_task.is_empty() {
            return None;
        }

        let task_graph = self.graph.tasks.get(&buf_info.writer_task)?;
        let write_node_id = find_buffer_write_in_task(task_graph, bind_name)?;

        let rv = self.get_rv_for_node(&buf_info.writer_task, write_node_id)?;
        let task_info = self.thir.task_info(&buf_info.writer_task)?;
        Some(rv as f64 * task_info.freq_hz)
    }

    /// Infer the data rate (Hz) for an IN bind from reader sides.
    /// Validates all readers agree on rate; emits E0312 on mismatch.
    fn infer_bind_rate_from_readers(&mut self, bind_name: &str) -> Option<f64> {
        let mut rates: Vec<f64> = Vec::new();

        // Sorted iteration for deterministic conflict diagnostics
        let mut task_names: Vec<String> = self.graph.tasks.keys().cloned().collect();
        task_names.sort();

        for task_name in &task_names {
            let task_graph = &self.graph.tasks[task_name];
            for sub in subgraphs_of(task_graph) {
                for node in &sub.nodes {
                    if let NodeKind::BufferRead { buffer_name } = &node.kind {
                        if buffer_name == bind_name {
                            if let Some(rv) = self.get_rv_for_node(task_name, node.id) {
                                if let Some(task_info) = self.thir.task_info(task_name) {
                                    rates.push(rv as f64 * task_info.freq_hz);
                                }
                            }
                        }
                    }
                }
            }
        }

        if rates.is_empty() {
            return None;
        }

        let first = rates[0];
        for r in &rates[1..] {
            if (r - first).abs() > 0.001 {
                let span = self
                    .thir
                    .bind_info(bind_name)
                    .map(|b| b.name_span)
                    .unwrap_or(Span::new((), 0..0));
                self.error(
                    codes::E0312,
                    span,
                    format!(
                        "bind '{}' readers require different rates: {} Hz vs {} Hz",
                        bind_name, first, r
                    ),
                );
                break;
            }
        }
        Some(first)
    }

    // ── Phase 5: Buffer size computation ────────────────────────────────

    fn compute_buffer_sizes(&mut self) {
        let mut total: u64 = 0;

        // Inter-task buffers
        for edge in &self.graph.inter_task_edges {
            let wire_type = self.infer_buffer_type(&edge.buffer_name);
            let type_size = wire_type.map(type_size_bytes).unwrap_or(4);
            let pw = self
                .get_rv_for_node(&edge.writer_task, edge.writer_node)
                .unwrap_or(1);
            let buffer_bytes = 2 * pw as u64 * type_size;
            self.inter_buffers
                .insert(edge.buffer_name.clone(), buffer_bytes);
            total += buffer_bytes;
        }

        self.total_memory = total;
    }

    // ── Phase 6: Memory pool check ──────────────────────────────────────

    fn check_memory_pool(&mut self) {
        let (limit, span_opt) = self.get_mem_limit();
        if self.total_memory > limit {
            let span = span_opt.unwrap_or(self.thir.program_span);
            let limit_src = if span_opt.is_some() {
                "set mem"
            } else {
                "default mem (64MB)"
            };
            self.error(
                codes::E0307,
                span,
                format!(
                    "shared memory pool exceeded: required {} bytes, available {} bytes ({})",
                    self.total_memory, limit, limit_src
                ),
            );
        }
    }

    // ── Phase 7: Param type vs RUNTIME_PARAM match ──────────────────────

    fn check_param_types(&mut self) {
        let subs = std::mem::take(&mut self.all_subgraphs);
        for &(_, _, sub) in &subs {
            self.check_param_types_in_subgraph(sub);
        }
        self.all_subgraphs = subs;
    }

    fn check_param_types_in_subgraph(&mut self, sub: &Subgraph) {
        // Collect param check requests first to avoid borrow conflict.
        let mut checks: Vec<(Ident, ParamType, String, Span)> = Vec::new();
        for node in &sub.nodes {
            if let NodeKind::Actor { name, args, .. } = &node.kind {
                let meta = match self.actor_meta(name) {
                    Some(m) => m,
                    None => continue,
                };
                for (idx, arg) in args.iter().enumerate() {
                    if let Arg::ParamRef(param_ident) = arg {
                        if let Some(actor_param) = meta.params.get(idx) {
                            if actor_param.kind == ParamKind::RuntimeParam {
                                checks.push((
                                    param_ident.clone(),
                                    actor_param.param_type.clone(),
                                    name.clone(),
                                    node.span,
                                ));
                            }
                        }
                    }
                }
            }
        }
        for (param_ident, expected_type, actor_name, span) in checks {
            self.check_single_param_type(&param_ident, expected_type, &actor_name, span);
        }
    }

    fn check_single_param_type(
        &mut self,
        param_ident: &Ident,
        expected_type: ParamType,
        actor_name: &str,
        span: Span,
    ) {
        let param = match self.thir.param_info(&param_ident.name) {
            Some(p) => p,
            None => return,
        };
        let inferred = infer_param_type(&param.default_value);

        if let Some(inferred_type) = inferred {
            if !param_type_compatible(&inferred_type, &expected_type) {
                self.error(
                    codes::E0308,
                    span,
                    format!(
                        "param '{}' type mismatch: actor '{}' expects RUNTIME_PARAM({:?}), \
                         but param value suggests {:?}",
                        param_ident.name, actor_name, expected_type, inferred_type
                    ),
                );
            }
        }
    }

    // ── Phase 8: Ctrl type validation (§6) ──────────────────────────────

    /// For each modal task, verify the ctrl buffer type is int32.
    fn check_ctrl_types(&mut self) {
        for hir_task in &self.thir.hir.tasks {
            let modal = match &hir_task.body {
                HirTaskBody::Modal(m) => m,
                _ => continue,
            };
            let ctrl_buffer_name = match &modal.switch {
                HirSwitchSource::Buffer(name, _) => name,
                HirSwitchSource::Param(name, span) => {
                    let Some(param) = self.thir.param_info(name) else {
                        // Undefined param is already reported by resolve.
                        continue;
                    };
                    let inferred = infer_param_type(&param.default_value);
                    if inferred != Some(ParamType::Int) {
                        self.error_with_hint(
                            codes::E0309,
                            *span,
                            format!(
                                "switch param '${}' in task '{}' has non-int32 default; \
                                 switch ctrl must be int32",
                                name, hir_task.name
                            ),
                            "use an integer default, e.g. `param sel = 0`".to_string(),
                        );
                    }
                    continue;
                }
            };
            let control_sub = match self.graph.tasks.get(&hir_task.name) {
                Some(TaskGraph::Modal { control, .. }) => control,
                _ => continue,
            };
            // Find the BufferWrite node for the ctrl buffer in the control subgraph
            for node in &control_sub.nodes {
                if let NodeKind::BufferWrite { buffer_name } = &node.kind {
                    if buffer_name == ctrl_buffer_name {
                        if let Some(wire_type) = self.trace_type_backward(node.id, control_sub) {
                            if wire_type != PipitType::Int32 {
                                self.error_with_hint(
                                    codes::E0310,
                                    node.span,
                                    format!(
                                        "ctrl buffer '{}' in task '{}' has type {}, \
                                         but switch ctrl must be int32",
                                        buffer_name, hir_task.name, wire_type
                                    ),
                                    "use detect() or another actor that outputs int32 for the ctrl signal".to_string(),
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

// ── Free helper functions ───────────────────────────────────────────────────

/// Find a node in a subgraph by NodeId.
fn build_subgraph_refs(graph: &ProgramGraph) -> HashMap<usize, &Subgraph> {
    let mut refs = HashMap::new();
    for task_graph in graph.tasks.values() {
        for sub in subgraphs_of(task_graph) {
            refs.insert(subgraph_key(sub), sub);
        }
    }
    refs
}

/// Find the NodeId of a BufferWrite node in a task.
fn find_buffer_write_in_task(task_graph: &TaskGraph, buffer_name: &str) -> Option<NodeId> {
    for sub in subgraphs_of(task_graph) {
        for node in &sub.nodes {
            if let NodeKind::BufferWrite {
                buffer_name: name, ..
            } = &node.kind
            {
                if name == buffer_name {
                    return Some(node.id);
                }
            }
        }
    }
    None
}

/// Display name for a node (for error messages).
fn node_display_name(node: &Node) -> String {
    match &node.kind {
        NodeKind::Actor { name, .. } => name.clone(),
        NodeKind::Fork { tap_name } => format!(":{}", tap_name),
        NodeKind::Probe { probe_name } => format!("?{}", probe_name),
        NodeKind::BufferRead { buffer_name } => format!("@{}", buffer_name),
        NodeKind::BufferWrite { buffer_name } => format!("->{}", buffer_name),
        NodeKind::GatherRead { family_name, .. } => format!("@{}[*]", family_name),
        NodeKind::ScatterWrite { family_name, .. } => format!("->{}[*]", family_name),
    }
}

/// Size in bytes for a PipitType.
fn type_size_bytes(t: PipitType) -> u64 {
    match t {
        PipitType::Int8 => 1,
        PipitType::Int16 => 2,
        PipitType::Int32 => 4,
        PipitType::Float => 4,
        PipitType::Double => 8,
        PipitType::Cfloat => 8,
        PipitType::Cdouble => 16,
        PipitType::Void => 0,
    }
}

/// GCD for u64.
fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

fn reduce_ratio(num: u64, den: u64) -> (u64, u64) {
    let g = gcd(num, den);
    (num / g, den / g)
}

/// GCD for u32.
fn gcd32(a: u32, b: u32) -> u32 {
    if b == 0 {
        a
    } else {
        gcd32(b, a % b)
    }
}

/// LCM for u64.
fn lcm(a: u64, b: u64) -> u64 {
    if a == 0 || b == 0 {
        0
    } else {
        a / gcd(a, b) * b
    }
}

fn normalize_repetition_vector(rv_rat: &HashMap<NodeId, (u64, u64)>) -> HashMap<NodeId, u32> {
    let lcm_den = rv_rat.values().fold(1u64, |acc, &(_, d)| lcm(acc, d));
    let mut rv: HashMap<NodeId, u32> = HashMap::new();
    for (&node_id, &(num, den)) in rv_rat {
        let val = num * (lcm_den / den);
        rv.insert(node_id, val as u32);
    }
    if rv.is_empty() {
        return rv;
    }
    let g = rv.values().copied().fold(0u32, gcd32);
    if g > 1 {
        for val in rv.values_mut() {
            *val /= g;
        }
    }
    rv
}

/// Infer a ParamType from a scalar value.
fn infer_param_type(scalar: &Scalar) -> Option<ParamType> {
    match scalar {
        Scalar::Number(_, _, is_int_literal) => Some(if *is_int_literal {
            ParamType::Int
        } else {
            ParamType::Float
        }),
        _ => None,
    }
}

/// Check if inferred param type exactly matches expected actor param type.
fn param_type_compatible(inferred: &ParamType, expected: &ParamType) -> bool {
    // TypeParam / SpanTypeParam are polymorphic — type_infer validates the concrete match
    if matches!(
        expected,
        ParamType::TypeParam(_) | ParamType::SpanTypeParam(_)
    ) {
        return true;
    }
    inferred == expected
}

// ── Stable ID computation ────────────────────────────────────────────────────

/// Compute a deterministic stable_id from graph lineage (§5.5.3).
///
/// Hash key: `direction + "\0" + sorted_call_ids.join("\0") + "\0" + transport`
/// Output: 16-char hex string (first 8 bytes of SHA-256).
fn compute_stable_id(direction: BindDirection, call_ids: &[CallId], transport: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(direction.to_string().as_bytes());
    hasher.update(b"\0");
    for (i, cid) in call_ids.iter().enumerate() {
        if i > 0 {
            hasher.update(b"\0");
        }
        hasher.update(cid.0.to_string().as_bytes());
    }
    hasher.update(b"\0");
    hasher.update(transport.as_bytes());
    let hash = hasher.finalize();
    // Truncate to first 8 bytes → 16 hex chars
    hash.iter().take(8).map(|b| format!("{:02x}", b)).collect()
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

    fn test_registry_with_extra_header(header_src: &str) -> Registry {
        let mut reg = test_registry();
        let tmp = std::env::temp_dir().join(format!(
            "pipit_extra_actor_{}_{}.h",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock before UNIX_EPOCH")
                .as_nanos()
        ));
        std::fs::write(&tmp, header_src).expect("write temp actor header");
        reg.load_header(&tmp).expect("load temp actor header");
        let _ = std::fs::remove_file(&tmp);
        reg
    }

    /// Parse, resolve, build HIR, graph, ThirContext, and analyze.
    fn analyze_source(source: &str, registry: &Registry) -> AnalysisResult {
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
        let type_result =
            crate::type_infer::type_infer(&hir_program, &resolve_result.resolved, registry);
        let lower_result = crate::lower::lower_and_verify(
            &hir_program,
            &resolve_result.resolved,
            &type_result.typed,
            registry,
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
        let thir = crate::thir::build_thir_context(
            &hir_program,
            &resolve_result.resolved,
            &type_result.typed,
            &lower_result.lowered,
            registry,
            &graph_result.graph,
        );
        analyze(&thir, &graph_result.graph)
    }

    fn analyze_ok(source: &str, registry: &Registry) -> AnalysisResult {
        let result = analyze_source(source, registry);
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.level == DiagLevel::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "unexpected analysis errors: {:#?}",
            errors
        );
        result
    }

    fn has_error(result: &AnalysisResult, pattern: &str) -> bool {
        result
            .diagnostics
            .iter()
            .any(|d| d.level == DiagLevel::Error && d.message.contains(pattern))
    }

    fn has_error_code(result: &AnalysisResult, code: DiagCode) -> bool {
        result
            .diagnostics
            .iter()
            .any(|d| d.level == DiagLevel::Error && d.code == Some(code))
    }

    /// Parse, resolve, build graph, and analyze — also return the graph for
    /// looking up NodeIds by actor name.
    fn analyze_with_graph(
        source: &str,
        registry: &Registry,
    ) -> (AnalysisResult, crate::graph::ProgramGraph) {
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
        let type_result =
            crate::type_infer::type_infer(&hir_program, &resolve_result.resolved, registry);
        let lower_result = crate::lower::lower_and_verify(
            &hir_program,
            &resolve_result.resolved,
            &type_result.typed,
            registry,
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
        let thir = crate::thir::build_thir_context(
            &hir_program,
            &resolve_result.resolved,
            &type_result.typed,
            &lower_result.lowered,
            registry,
            &graph_result.graph,
        );
        let result = analyze(&thir, &graph_result.graph);
        (result, graph_result.graph)
    }

    /// Find the NodeId of the first actor with the given name inside a task.
    fn find_actor_id(graph: &crate::graph::ProgramGraph, task: &str, actor_name: &str) -> NodeId {
        use crate::graph::{NodeKind, TaskGraph};
        let task_graph = graph.tasks.get(task).expect("task not found");
        let subgraphs: Vec<&crate::graph::Subgraph> = match task_graph {
            TaskGraph::Pipeline(sub) => vec![sub],
            TaskGraph::Modal { control, modes } => {
                let mut subs = vec![control];
                for (_, m) in modes {
                    subs.push(m);
                }
                subs
            }
        };
        for sub in subgraphs {
            for node in &sub.nodes {
                if let NodeKind::Actor { name, .. } = &node.kind {
                    if name == actor_name {
                        return node.id;
                    }
                }
            }
        }
        panic!("actor '{}' not found in task '{}'", actor_name, task);
    }

    // ── Phase 1: Type checking tests ────────────────────────────────────

    #[test]
    fn type_check_ok_linear() {
        let reg = test_registry();
        // adc(void→float) | fft(float→cfloat) | mag(cfloat→float) | stdout(float→void)
        analyze_ok(
            "clock 1kHz t {\n    constant(0.0) | fft(256) | mag() | stdout()\n}",
            &reg,
        );
    }

    #[test]
    fn type_check_mismatch() {
        let reg = test_registry();
        // fft outputs cfloat, second fft expects float → type mismatch
        // (both fft actors are concrete, so analysis catches the mismatch)
        let result = analyze_source(
            "clock 1kHz t {\n    constant(0.0) | fft(256) | fft(256) | stdout()\n}",
            &reg,
        );
        assert!(
            has_error(&result, "type mismatch"),
            "expected type mismatch error, got: {:#?}",
            result.diagnostics
        );
    }

    #[test]
    fn polymorphic_stdout_accepts_cfloat_from_fft() {
        let reg = test_registry();
        // constant(0.0) → float, fft(256) → cfloat, stdout<T> infers T=cfloat.
        // Analysis phase sees stdout as polymorphic (out_type returns None),
        // so the cfloat→stdout edge is not flagged — type_infer resolves it.
        analyze_ok(
            "clock 1kHz t {\n    constant(0.0) | fft(256) | stdout()\n}",
            &reg,
        );
    }

    #[test]
    fn polymorphic_fir_rate_resolution() {
        let reg = test_registry();
        // Polymorphic fir with span arg: N resolved from coeff array length.
        // Verifies that SpanTypeParam("T") is handled by infer_dim_param_from_span_args.
        let result = analyze_ok(
            "const coeff = [0.1, 0.2, 0.4, 0.2, 0.1]\nclock 1kHz t {\n    constant(0.0) | fir(coeff) | stdout()\n}",
            &reg,
        );
        let rv = result
            .analysis
            .repetition_vectors
            .get(&("t".to_string(), "pipeline".to_string()))
            .expect("rv missing");
        // fir(coeff) with 5-element array → N=5 → rate should be resolved
        assert!(!rv.is_empty(), "repetition vector should not be empty");
    }

    #[test]
    fn polymorphic_mul_passthrough_rate() {
        let reg = test_registry();
        // Polymorphic mul with upstream float: T=float, rate 1:1 passthrough.
        // Verifies polymorphic actors work in analyze without type_infer.
        analyze_ok(
            "clock 1kHz t {\n    constant(0.0) | mul(2.5) | stdout()\n}",
            &reg,
        );
    }

    #[test]
    fn type_check_void_source_ok() {
        let reg = test_registry();
        // adc has void input, should not flag type error
        analyze_ok("clock 1kHz t {\n    constant(0.0) | stdout()\n}", &reg);
    }

    #[test]
    fn type_check_through_fork() {
        let reg = test_registry();
        // Fork passes float through
        analyze_ok(
            "clock 1kHz t {\n    constant(0.0) | :raw | stdout()\n    :raw | stdout()\n}",
            &reg,
        );
    }

    #[test]
    fn type_check_through_probe() {
        let reg = test_registry();
        // Probe passes float through
        analyze_ok(
            "clock 1kHz t {\n    constant(0.0) | ?mon | stdout()\n}",
            &reg,
        );
    }

    // ── Phase 2: SDF balance equation tests ─────────────────────────────

    #[test]
    fn balance_uniform_rate() {
        let reg = test_registry();
        let result = analyze_ok("clock 1kHz t {\n    constant(0.0) | stdout()\n}", &reg);
        let rv = result
            .analysis
            .repetition_vectors
            .get(&("t".to_string(), "pipeline".to_string()))
            .expect("rv missing");
        // All rates 1:1 → rv should be all 1s
        for &val in rv.values() {
            assert_eq!(val, 1, "expected uniform rv=1, got {:?}", rv);
        }
    }

    #[test]
    fn balance_decimation() {
        let reg = test_registry();
        // constant(0.0) | fft(256) | c2r() | stdout()
        // fft: IN(float,256), OUT(cfloat,256). c2r: IN(cfloat,1), OUT(float,1)
        let source = "clock 1kHz t {\n    constant(0.0) | fft(256) | c2r() | stdout()\n}";
        let (result, graph) = analyze_with_graph(source, &reg);
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.level == DiagLevel::Error)
            .collect();
        assert!(errors.is_empty(), "unexpected errors: {:#?}", errors);
        let rv = result
            .analysis
            .repetition_vectors
            .get(&("t".to_string(), "pipeline".to_string()))
            .expect("rv missing");
        // After shape inference: constant OUT=256, c2r IN/OUT=256 (both SHAPE(N)).
        // constant(256)→fft(256): rv[constant]=rv[fft]=1
        // fft(256)→c2r(256): rv[c2r]=rv[fft]=1
        // c2r(256)→stdout(1): rv[c2r]×256 = rv[stdout]×1 → rv[stdout]=256
        let constant_id = find_actor_id(&graph, "t", "constant");
        let fft_id = find_actor_id(&graph, "t", "fft");
        let c2r_id = find_actor_id(&graph, "t", "c2r");
        let stdout_id = find_actor_id(&graph, "t", "stdout");
        assert_eq!(rv[&constant_id], 1);
        assert_eq!(rv[&fft_id], 1);
        assert_eq!(rv[&c2r_id], 1);
        assert_eq!(rv[&stdout_id], 256);
    }

    #[test]
    fn balance_fir_symbolic() {
        let reg = test_registry();
        // fir with const coeff array → N = array length = 5
        let source = concat!(
            "const coeff = [0.1, 0.2, 0.4, 0.2, 0.1]\n",
            "clock 1kHz t {\n    constant(0.0) | fir(coeff) | stdout()\n}",
        );
        let (result, graph) = analyze_with_graph(source, &reg);
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.level == DiagLevel::Error)
            .collect();
        assert!(errors.is_empty(), "unexpected errors: {:#?}", errors);
        let rv = result
            .analysis
            .repetition_vectors
            .get(&("t".to_string(), "pipeline".to_string()))
            .expect("rv missing");
        // After shape inference, constant gets inferred OUT rate=5 from fir(coeff).
        // constant→fir: rv[constant]×5 = rv[fir]×5 → rv[constant]=rv[fir]=1
        // fir→stdout: rv[fir]×1 = rv[stdout]×1 → rv[stdout]=1
        let constant_id = find_actor_id(&graph, "t", "constant");
        let fir_id = find_actor_id(&graph, "t", "fir");
        let stdout_id = find_actor_id(&graph, "t", "stdout");
        assert_eq!(rv[&constant_id], 1);
        assert_eq!(rv[&fir_id], 1);
        assert_eq!(rv[&stdout_id], 1);
    }

    #[test]
    fn balance_with_fork() {
        let reg = test_registry();
        // Fork: constant(0.0) | :raw | stdout() + :raw | stdout()
        // All rates 1:1 → rv should be uniform
        let result = analyze_ok(
            "clock 1kHz t {\n    constant(0.0) | :raw | stdout()\n    :raw | stdout()\n}",
            &reg,
        );
        let rv = result
            .analysis
            .repetition_vectors
            .get(&("t".to_string(), "pipeline".to_string()))
            .expect("rv missing");
        assert!(!rv.is_empty());
        for &val in rv.values() {
            assert_eq!(val, 1, "expected uniform rv=1 for all nodes, got {:?}", rv);
        }
    }

    // ── Phase 3: Feedback delay tests ───────────────────────────────────

    #[test]
    fn feedback_with_delay_ok() {
        let reg = test_registry();
        // Feedback loop with delay — should pass
        analyze_ok(
            concat!(
                "clock 1kHz t {\n",
                "    constant(0.0) | add(:fb) | :out | stdout()\n",
                "    :out | delay(1, 0.0) | :fb\n",
                "}",
            ),
            &reg,
        );
    }

    #[test]
    fn feedback_without_delay_error() {
        let reg = test_registry();
        // Feedback loop without delay — mul instead of delay
        // mul: IN(float,1), OUT(float,1) — not a delay
        let result = analyze_source(
            concat!(
                "param gain = 1.0\n",
                "clock 1kHz t {\n",
                "    constant(0.0) | add(:fb) | :out | stdout()\n",
                "    :out | mul($gain) | :fb\n",
                "}",
            ),
            &reg,
        );
        assert!(
            has_error(&result, "feedback loop"),
            "expected feedback delay error, got: {:#?}",
            result.diagnostics
        );
    }

    // ── Phase 4: Cross-clock rate matching tests ──────────────────────

    #[test]
    fn cross_clock_rate_match_ok() {
        let reg = test_registry();
        // fast 10kHz writes 1 token/iter → 10k tokens/sec
        // slow 1kHz reads via decimate(10): 10 tokens/iter → 10k tokens/sec ✓
        analyze_ok(
            concat!(
                "set mem = 64MB\n",
                "clock 10kHz fast { constant(0.0) -> sig }\n",
                "clock 1kHz slow { @sig | decimate(10) | stdout() }\n",
            ),
            &reg,
        );
    }

    #[test]
    fn cross_clock_rate_mismatch_error() {
        let reg = test_registry();
        // fast 10kHz writes 1 token/iter → 10k tokens/sec
        // slow 1kHz reads 1 token/iter → 1k tokens/sec ✗
        let result = analyze_source(
            concat!(
                "set mem = 64MB\n",
                "clock 10kHz fast { constant(0.0) -> sig }\n",
                "clock 1kHz slow { @sig | stdout() }\n",
            ),
            &reg,
        );
        assert!(
            has_error(&result, "rate mismatch"),
            "expected cross-clock rate mismatch error, got: {:#?}",
            result.diagnostics
        );
    }

    #[test]
    fn cross_clock_rate_mismatch_modal_writer_is_error() {
        let reg = test_registry();
        let result = analyze_source(
            concat!(
                "clock 10kHz producer {\n",
                "    control {\n        constant(0.0) | detect() -> ctrl\n    }\n",
                "    mode a {\n        constant(0.0) -> sig\n    }\n",
                "    mode b {\n        constant(0.0) -> sig\n    }\n",
                "    switch(ctrl, a, b)\n",
                "}\n",
                "clock 1kHz consumer {\n",
                "    @sig | stdout()\n",
                "}\n",
            ),
            &reg,
        );
        assert!(
            has_error(&result, "rate mismatch"),
            "expected modal-writer rate mismatch error, got: {:#?}",
            result.diagnostics
        );
    }

    // ── Phase 5/6: Buffer size and memory pool tests ────────────────────

    #[test]
    fn buffer_size_computation() {
        let reg = test_registry();
        // constant(float=4B) → BufferWrite, rv[writer]=1
        // buffer_bytes = 2 × 1 × 4 = 8 bytes
        let result = analyze_ok(
            concat!(
                "set mem = 64MB\n",
                "clock 1kHz a { constant(0.0) -> sig }\n",
                "clock 1kHz b { @sig | stdout() }\n",
            ),
            &reg,
        );
        assert_eq!(
            *result.analysis.inter_task_buffers.get("sig").unwrap(),
            8,
            "expected 2×1×4=8 bytes for float buffer"
        );
        assert_eq!(result.analysis.total_memory, 8);
    }

    #[test]
    fn memory_pool_exceeded_error() {
        let reg = test_registry();
        // fft(256): BufferWrite rv=256, type=cfloat(8B)
        // buffer = 2 × 256 × 8 = 4096B > 1KB(1024B) → error
        let result = analyze_source(
            concat!(
                "set mem = 1KB\n",
                "clock 1kHz a { constant(0.0) | fft(256) -> sig }\n",
                "clock 1kHz b { @sig | c2r() | stdout() }\n",
            ),
            &reg,
        );
        assert!(
            has_error(&result, "shared memory pool exceeded"),
            "expected memory pool exceeded error, got: {:#?}",
            result.diagnostics
        );
    }

    #[test]
    fn memory_pool_within_limit_ok() {
        let reg = test_registry();
        // buffer = 8 bytes << 64MB → ok
        analyze_ok(
            concat!(
                "set mem = 64MB\n",
                "clock 1kHz a { constant(0.0) -> sig }\n",
                "clock 1kHz b { @sig | stdout() }\n",
            ),
            &reg,
        );
    }

    // ── Phase 7: Param type tests ───────────────────────────────────────

    #[test]
    fn param_type_float_ok() {
        let reg = test_registry();
        // param gain = 1.0 (float), mul has RUNTIME_PARAM(float, gain) → match
        analyze_ok(
            "param gain = 1.0\nclock 1kHz t {\n    constant(0.0) | mul($gain) | stdout()\n}",
            &reg,
        );
    }

    #[test]
    fn param_type_int_to_polymorphic_ok() {
        let reg = test_registry();
        // param val = 1 (int), polymorphic constant has RUNTIME_PARAM(T, value)
        // → T inferred as int32, no mismatch
        analyze_ok(
            "param val = 1\nclock 1kHz t {\n    constant($val) | stdout()\n}",
            &reg,
        );
    }

    // ── Phase 8: Shape-aware dimension inference (v0.2.0) ─────────────

    #[test]
    fn dimension_inference_from_args() {
        // fft(256): N resolved from positional arg → rate = 256
        let reg = test_registry();
        let result = analyze_ok(
            "clock 1kHz t {\n    constant(0.0) | fft(256) | mag() | stdout()\n}",
            &reg,
        );
        let rv = result
            .analysis
            .repetition_vectors
            .get(&("t".to_string(), "pipeline".to_string()))
            .expect("rv missing");
        assert!(!rv.is_empty());
    }

    #[test]
    fn dimension_inference_from_shape_constraint() {
        // fft()[256]: N resolved from shape constraint → rate = 256
        let reg = test_registry();
        let result = analyze_ok(
            "clock 1kHz t {\n    constant(0.0) | fft()[256] | mag() | stdout()\n}",
            &reg,
        );
        let rv = result
            .analysis
            .repetition_vectors
            .get(&("t".to_string(), "pipeline".to_string()))
            .expect("rv missing");
        assert!(!rv.is_empty());
    }

    #[test]
    fn dimension_inference_from_const_ref_shape() {
        // fft()[N]: N resolved from const ref in shape constraint
        let reg = test_registry();
        let result = analyze_ok(
            "const N = 256\nclock 1kHz t {\n    constant(0.0) | fft()[N] | mag() | stdout()\n}",
            &reg,
        );
        let rv = result
            .analysis
            .repetition_vectors
            .get(&("t".to_string(), "pipeline".to_string()))
            .expect("rv missing");
        assert!(!rv.is_empty());
    }

    // ── Phase 9: SDF edge shape inference (§13.3.3) ───────────────────

    #[test]
    fn sdf_edge_inference_direct() {
        // fft()[256] | mag(): N inferred from upstream fft output shape
        let reg = test_registry();
        let source = "clock 1kHz t {\n    constant(0.0) | fft()[256] | mag() | stdout()\n}";
        let (result, graph) = analyze_with_graph(source, &reg);
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.level == DiagLevel::Error)
            .collect();
        assert!(errors.is_empty(), "unexpected errors: {:#?}", errors);
        // mag() should have an inferred shape [256]
        let mag_id = find_actor_id(&graph, "t", "mag");
        let inferred = result
            .analysis
            .inferred_shapes
            .get(&mag_id)
            .expect("expected inferred shape for mag()");
        assert_eq!(inferred.dims.len(), 1);
        assert!(
            matches!(&inferred.dims[0], ShapeDim::Literal(256, _)),
            "expected inferred dim 256, got {:?}",
            inferred.dims[0]
        );
    }

    #[test]
    fn sdf_edge_inference_through_fork() {
        // fft(256) | :raw | mag(): N inferred through fork node
        let reg = test_registry();
        let result = analyze_ok(
            concat!(
                "clock 1kHz t {\n",
                "    constant(0.0) | fft(256) | :raw | mag() | stdout()\n",
                "    :raw | c2r() | stdout()\n",
                "}",
            ),
            &reg,
        );
        assert!(
            !result.analysis.inferred_shapes.is_empty(),
            "expected inferred shapes for mag() through fork"
        );
    }

    #[test]
    fn sdf_edge_inference_chain() {
        // fft()[256] | mag() | stdout(): mag's N inferred from fft's output,
        // and the pipeline should have valid balance equations
        let reg = test_registry();
        let source = "clock 1kHz t {\n    constant(0.0) | fft()[256] | mag() | stdout()\n}";
        let (result, graph) = analyze_with_graph(source, &reg);
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.level == DiagLevel::Error)
            .collect();
        assert!(errors.is_empty(), "unexpected errors: {:#?}", errors);
        let rv = result
            .analysis
            .repetition_vectors
            .get(&("t".to_string(), "pipeline".to_string()))
            .expect("rv missing");
        // After shape inference: constant OUT=256, mag IN/OUT=256 (SHAPE(N)).
        // constant(256)→fft(256): rv[constant]=rv[fft]=1
        // fft(256)→mag(256): rv[mag]=rv[fft]=1
        // mag(256)→stdout(1): rv[mag]×256 = rv[stdout]×1 → rv[stdout]=256
        let constant_id = find_actor_id(&graph, "t", "constant");
        let fft_id = find_actor_id(&graph, "t", "fft");
        let mag_id = find_actor_id(&graph, "t", "mag");
        let stdout_id = find_actor_id(&graph, "t", "stdout");
        assert_eq!(rv[&constant_id], 1);
        assert_eq!(rv[&fft_id], 1);
        assert_eq!(rv[&mag_id], 1);
        assert_eq!(rv[&stdout_id], 256);
    }

    // ── Shape constraint error tests (§13.6) ──────────────────────────

    #[test]
    fn unresolved_dimension_error() {
        // fft() without arg or shape constraint → N unresolved
        let reg = test_registry();
        let result = analyze_source(
            "clock 1kHz t {\n    constant(0.0) | fft() | mag() | stdout()\n}",
            &reg,
        );
        assert!(
            has_error(&result, "unresolved frame dimension"),
            "should error about unresolved frame dimension: {:#?}",
            result.diagnostics
        );
    }

    #[test]
    fn conflicting_shape_constraint_error() {
        // fft(256) outputs [256], but mag()[128] has explicit [128] → conflict
        let reg = test_registry();
        let result = analyze_source(
            "clock 1kHz t {\n    constant(0.0) | fft(256) | mag()[128] | stdout()\n}",
            &reg,
        );
        assert!(
            has_error(&result, "conflicting frame constraint"),
            "should error about conflicting frame constraint: {:#?}",
            result.diagnostics
        );
    }

    #[test]
    fn dimension_param_order_warning() {
        let mut reg = test_registry();
        let tmp = std::env::temp_dir().join(format!(
            "pipit_bad_dim_order_{}_{}.h",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock before UNIX_EPOCH")
                .as_nanos()
        ));
        std::fs::write(
            &tmp,
            concat!(
                "ACTOR(bad_dim_order, IN(float, SHAPE(N)), OUT(float, SHAPE(N)),\n",
                "      PARAM(int, N) RUNTIME_PARAM(float, gain)) {\n",
                "    for (int i = 0; i < N; ++i) out[i] = in[i] * gain;\n",
                "    return ACTOR_OK;\n",
                "}\n",
            ),
        )
        .expect("write temp actor header");
        reg.load_header(&tmp).expect("load temp actor header");
        let _ = std::fs::remove_file(&tmp);

        let result = analyze_source(
            "param gain = 1.0\nclock 1kHz t {\n    constant(0.0) | bad_dim_order(4, $gain) | stdout()\n}",
            &reg,
        );
        let has_warning = result.diagnostics.iter().any(|d| {
            d.level == DiagLevel::Warning
                && d.message.contains("bad_dim_order")
                && d.message.contains("inferred dimension PARAM")
        });
        assert!(
            has_warning,
            "expected dimension param order warning, got: {:#?}",
            result.diagnostics
        );
    }

    #[test]
    fn dimension_param_order_no_warning_for_fir() {
        let reg = test_registry();
        let result = analyze_source(
            "const coeff = [0.1, 0.2, 0.3]\nclock 1kHz t {\n    constant(0.0) | fir(coeff) | stdout()\n}",
            &reg,
        );
        let has_fir_warning = result.diagnostics.iter().any(|d| {
            d.level == DiagLevel::Warning
                && d.message.contains("actor 'fir'")
                && d.message.contains("inferred dimension PARAM")
        });
        assert!(
            !has_fir_warning,
            "did not expect fir dimension param order warning, got: {:#?}",
            result.diagnostics
        );
    }

    // Note: runtime_param_as_shape_dim is already tested in resolve::tests
    // (resolve phase catches it before analysis runs).

    #[test]
    fn shape_constraint_matching_inference_ok() {
        // fft(256) outputs [256], mag()[256] has explicit [256] → matches → ok
        let reg = test_registry();
        analyze_ok(
            "clock 1kHz t {\n    constant(0.0) | fft(256) | mag()[256] | stdout()\n}",
            &reg,
        );
    }

    // ── Integration tests ───────────────────────────────────────────────

    #[test]
    fn example_pdl_analysis() {
        let reg = test_registry();
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("examples/example.pdl");
        let source = std::fs::read_to_string(&path).expect("failed to read example.pdl");
        let result = analyze_source(&source, &reg);
        // example.pdl should have no errors (warnings are OK for rate mismatch)
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.level == DiagLevel::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "example.pdl should pass analysis without errors: {:#?}",
            errors
        );
    }

    #[test]
    fn receiver_pdl_analysis() {
        let reg = test_registry();
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("examples/receiver.pdl");
        let source = std::fs::read_to_string(&path).expect("failed to read receiver.pdl");
        let result = analyze_source(&source, &reg);
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.level == DiagLevel::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "receiver.pdl should pass analysis without errors: {:#?}",
            errors
        );
    }

    // ── Ctrl type checks ──

    #[test]
    fn ctrl_type_int32_ok() {
        // detect() outputs int32 -> ctrl is valid
        let reg = test_registry();
        let result = analyze_ok(
            concat!(
                "clock 1kHz t {\n",
                "    control {\n",
                "        constant(0.0) | detect() -> ctrl\n",
                "    }\n",
                "    mode a {\n        constant(0.0) | stdout()\n    }\n",
                "    mode b {\n        constant(0.0) | stdout()\n    }\n",
                "    switch(ctrl, a, b) default a\n",
                "}",
            ),
            &reg,
        );
        assert!(
            result
                .diagnostics
                .iter()
                .all(|d| d.level != DiagLevel::Error),
            "ctrl int32 should pass: {:#?}",
            result.diagnostics
        );
    }

    #[test]
    fn ctrl_type_not_int32_error() {
        // float_src is a concrete float source → ctrl is NOT int32 → error
        let reg = test_registry_with_extra_header(
            r#"
#include <pipit.h>
ACTOR(float_src, IN(void, 0), OUT(float, 1), PARAM(float, value)) {
    (void)in; out[0] = value; return ACTOR_OK;
}};"#,
        );
        let result = analyze_source(
            concat!(
                "clock 1kHz t {\n",
                "    control {\n",
                "        float_src(0.0) -> ctrl\n",
                "    }\n",
                "    mode a {\n        constant(0.0) | stdout()\n    }\n",
                "    mode b {\n        constant(0.0) | stdout()\n    }\n",
                "    switch(ctrl, a, b) default a\n",
                "}",
            ),
            &reg,
        );
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.level == DiagLevel::Error)
            .collect();
        assert!(
            errors.iter().any(|d| d.message.contains("int32")),
            "should error about ctrl not being int32: {:#?}",
            errors
        );
    }

    #[test]
    fn switch_param_ctrl_type_int32_ok() {
        let reg = test_registry();
        let result = analyze_source(
            concat!(
                "param sel = 1\n",
                "clock 1kHz t {\n",
                "    mode a {\n        constant(0.0) | stdout()\n    }\n",
                "    mode b {\n        constant(0.0) | stdout()\n    }\n",
                "    switch($sel, a, b)\n",
                "}",
            ),
            &reg,
        );
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.level == DiagLevel::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "switch($param,...) with int param should pass: {:#?}",
            errors
        );
    }

    #[test]
    fn switch_param_ctrl_type_not_int32_error() {
        let reg = test_registry();
        let result = analyze_source(
            concat!(
                "param sel = 0.5\n",
                "clock 1kHz t {\n",
                "    mode a {\n        constant(0.0) | stdout()\n    }\n",
                "    mode b {\n        constant(0.0) | stdout()\n    }\n",
                "    switch($sel, a, b)\n",
                "}",
            ),
            &reg,
        );
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.level == DiagLevel::Error)
            .collect();
        assert!(
            errors
                .iter()
                .any(|d| d.message.contains("switch param '$sel'") && d.message.contains("int32")),
            "should error when switch($param,...) default is non-int: {:#?}",
            errors
        );
    }

    // ── v0.3.1 span-derived dimension tests ─────────────────────────────

    #[test]
    fn span_derived_dim_stored_for_fir() {
        let reg = test_registry();
        let source = concat!(
            "const coeff = [0.1, 0.2, 0.4, 0.2, 0.1]\n",
            "clock 1kHz t {\n    constant(0.0) | fir(coeff) | stdout()\n}",
        );
        let (result, graph) = analyze_with_graph(source, &reg);
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.level == DiagLevel::Error)
            .collect();
        assert!(errors.is_empty(), "unexpected errors: {:#?}", errors);

        let fir_id = find_actor_id(&graph, "t", "fir");
        let n_val = result
            .analysis
            .span_derived_dims
            .get(&fir_id)
            .and_then(|m| m.get("N"));
        assert_eq!(
            n_val,
            Some(&5),
            "fir(coeff) with 5-element array should store N=5 in span_derived_dims"
        );
    }

    #[test]
    fn span_derived_dim_not_stored_when_explicit_arg() {
        let reg = test_registry();
        // fir(taps, 3) provides N=3 explicitly — span_derived_dims should NOT store it
        let source = concat!(
            "const taps = [0.1, 0.2, 0.1]\n",
            "clock 1kHz t {\n    constant(0.0) | fir(taps, 3) | stdout()\n}",
        );
        let (result, graph) = analyze_with_graph(source, &reg);
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.level == DiagLevel::Error)
            .collect();
        assert!(errors.is_empty(), "unexpected errors: {:#?}", errors);

        let fir_id = find_actor_id(&graph, "t", "fir");
        assert!(
            !result
                .analysis
                .span_derived_dims
                .get(&fir_id)
                .map(|m| m.contains_key("N"))
                .unwrap_or(false),
            "N should not be in span_derived_dims when provided explicitly"
        );
    }

    #[test]
    fn span_derived_no_conflict_with_matching_pipeline() {
        // fir(coeff) with 5-tap filter in a pipeline that doesn't force a conflicting N
        let reg = test_registry();
        let source = concat!(
            "const coeff = [0.1, 0.2, 0.4, 0.2, 0.1]\n",
            "clock 1kHz t {\n    constant(0.0) | fir(coeff) | stdout()\n}",
        );
        let result = analyze_source(source, &reg);
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.level == DiagLevel::Error)
            .collect();
        assert!(errors.is_empty(), "no conflicts expected: {:#?}", errors);
    }

    #[test]
    fn span_derived_prevents_edge_inference_override() {
        // fir(coeff) with 5 taps after fft(256)|c2r() — edge inference should NOT
        // overwrite N=5 with 256
        let reg = test_registry();
        let source = concat!(
            "const coeff = [0.1, 0.2, 0.4, 0.2, 0.1]\n",
            "clock 1kHz t {\n    constant(0.0) | fft(256) | c2r() | fir(coeff) | stdout()\n}",
        );
        let (result, graph) = analyze_with_graph(source, &reg);
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.level == DiagLevel::Error)
            .collect();
        assert!(errors.is_empty(), "unexpected errors: {:#?}", errors);

        let fir_id = find_actor_id(&graph, "t", "fir");
        // span-derived N=5 must be authoritative
        assert_eq!(
            result
                .analysis
                .span_derived_dims
                .get(&fir_id)
                .and_then(|m| m.get("N")),
            Some(&5),
            "fir(coeff) N should be 5, not overridden by edge inference"
        );
        // inferred_shapes should NOT contain fir's node (edge inference skipped)
        assert!(
            !result.analysis.inferred_shapes.contains_key(&fir_id),
            "fir should not have edge-inferred shape when span-derived dims exist"
        );
    }

    #[test]
    fn mixed_dims_span_and_edge_inference_merge_per_dimension() {
        // Generalized case: one symbolic dim (H) resolved from span arg length,
        // the other dim (W) inferred from connected edge shape.
        let reg = test_registry_with_extra_header(concat!(
            "ACTOR(src2d, IN(void, 0), OUT(float, SHAPE(5, 4))) {\n",
            "    (void)in;\n",
            "    for (int i = 0; i < 20; ++i) out[i] = 0.0f;\n",
            "    return ACTOR_OK;\n",
            "}\n",
            "ACTOR(mixdim,\n",
            "      IN(float, SHAPE(H, W)), OUT(float, SHAPE(H, W)),\n",
            "      PARAM(std::span<const float>, coeff) PARAM(int, H) PARAM(int, W)) {\n",
            "    (void)coeff;\n",
            "    for (int i = 0; i < H * W; ++i) out[i] = in[i];\n",
            "    return ACTOR_OK;\n",
            "}\n",
            "ACTOR(sink2d, IN(float, SHAPE(H, W)), OUT(void, 0), PARAM(int, H) PARAM(int, W)) {\n",
            "    (void)in;\n",
            "    (void)out;\n",
            "    (void)H;\n",
            "    (void)W;\n",
            "    return ACTOR_OK;\n",
            "}\n",
        ));
        let source = concat!(
            "const coeff = [1, 2, 3, 4, 5]\n",
            "clock 1kHz t {\n",
            "    src2d() | mixdim(coeff) | sink2d()\n",
            "}",
        );
        let (result, graph) = analyze_with_graph(source, &reg);
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.level == DiagLevel::Error)
            .collect();
        assert!(errors.is_empty(), "unexpected errors: {:#?}", errors);

        let mix_id = find_actor_id(&graph, "t", "mixdim");
        assert_eq!(
            result
                .analysis
                .span_derived_dims
                .get(&mix_id)
                .and_then(|m| m.get("H")),
            Some(&5),
            "H should be span-derived from coeff length"
        );
        let inferred = result
            .analysis
            .inferred_shapes
            .get(&mix_id)
            .expect("mixdim should have inferred shape");
        assert_eq!(inferred.dims.len(), 2);
        assert!(
            matches!(inferred.dims[0], ShapeDim::Literal(5, _)),
            "H should remain 5 from span-derived source"
        );
        assert!(
            matches!(inferred.dims[1], ShapeDim::Literal(4, _)),
            "W should be inferred from upstream edge"
        );
        let rates = result
            .analysis
            .node_port_rates
            .get(&mix_id)
            .expect("mixdim should have precomputed node rates");
        assert_eq!(rates.in_rate, Some(20));
        assert_eq!(rates.out_rate, Some(20));
    }

    // ── Dimension mismatch diagnostic tests ──────────────────────────────

    #[test]
    fn dim_conflict_explicit_arg_vs_span() {
        // fir(coeff, 5) where coeff has 3 elements → explicit N=5 vs span N=3
        let reg = test_registry();
        let source = concat!(
            "const coeff = [0.1, 0.2, 0.1]\n",
            "clock 1kHz t {\n    constant(0.0) | fir(coeff, 5) | stdout()\n}",
        );
        let result = analyze_source(source, &reg);
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.level == DiagLevel::Error)
            .collect();
        assert!(
            errors
                .iter()
                .any(|d| d.message.contains("conflicting dimension")
                    && d.message.contains("explicit argument specifies 5")
                    && d.message.contains("span-derived value is 3")),
            "expected explicit-vs-span conflict error, got: {:#?}",
            errors
        );
    }

    #[test]
    fn dim_conflict_shape_constraint_vs_span() {
        // fir(coeff)[5] where coeff has 3 elements → shape constraint N=5 vs span N=3
        let reg = test_registry();
        let source = concat!(
            "const coeff = [0.1, 0.2, 0.1]\n",
            "clock 1kHz t {\n    constant(0.0) | fir(coeff)[5] | stdout()\n}",
        );
        let result = analyze_source(source, &reg);
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.level == DiagLevel::Error)
            .collect();
        assert!(
            errors
                .iter()
                .any(|d| d.message.contains("conflicting dimension")
                    && d.message.contains("shape constraint specifies 5")
                    && d.message.contains("span-derived value is 3")),
            "expected shape-constraint-vs-span conflict error, got: {:#?}",
            errors
        );
    }

    #[test]
    fn dim_no_conflict_when_sources_agree() {
        // fir(coeff, 3) where coeff has 3 elements → both agree on N=3
        let reg = test_registry();
        let source = concat!(
            "const coeff = [0.1, 0.2, 0.1]\n",
            "clock 1kHz t {\n    constant(0.0) | fir(coeff, 3) | stdout()\n}",
        );
        let result = analyze_source(source, &reg);
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.level == DiagLevel::Error && d.message.contains("conflicting dimension"))
            .collect();
        assert!(
            errors.is_empty(),
            "no conflict expected when sources agree, got: {:#?}",
            errors
        );
    }

    // ── Bind contract inference tests ────────────────────────────────────

    #[test]
    fn bind_direction_out() {
        let reg = test_registry();
        let source = r#"bind iq = udp("127.0.0.1:9100")
clock 48kHz audio {
    constant(0) -> iq
}
"#;
        let result = analyze_ok(source, &reg);
        let contract = result
            .analysis
            .bind_contracts
            .get("iq")
            .expect("contract for 'iq'");
        assert_eq!(contract.direction, BindDirection::Out);
    }

    #[test]
    fn bind_direction_in() {
        let reg = test_registry();
        let source = r#"bind iq = udp("127.0.0.1:9100")
clock 48kHz audio {
    @iq | stdout()
}
"#;
        let result = analyze_ok(source, &reg);
        let contract = result
            .analysis
            .bind_contracts
            .get("iq")
            .expect("contract for 'iq'");
        assert_eq!(contract.direction, BindDirection::In);
    }

    #[test]
    fn bind_unreferenced_e0311() {
        let reg = test_registry();
        let source = r#"bind iq = udp("127.0.0.1:9100")
clock 48kHz audio {
    constant(0) | stdout()
}
"#;
        let result = analyze_source(source, &reg);
        assert!(
            has_error(&result, "not referenced"),
            "expected E0311 for unreferenced bind"
        );
    }

    #[test]
    fn bind_out_contract_dtype() {
        let reg = test_registry();
        // constant(0) with integer literal 0 → concrete type Int32
        let source = r#"bind iq = udp("127.0.0.1:9100")
clock 48kHz audio {
    constant(0) -> iq
}
"#;
        let result = analyze_ok(source, &reg);
        let contract = result
            .analysis
            .bind_contracts
            .get("iq")
            .expect("contract for 'iq'");
        assert_eq!(contract.dtype, Some(PipitType::Int32));
    }

    #[test]
    fn bind_out_contract_rate() {
        let reg = test_registry();
        let source = r#"bind iq = udp("127.0.0.1:9100")
clock 48kHz audio {
    constant(0) -> iq
}
"#;
        let result = analyze_ok(source, &reg);
        let contract = result
            .analysis
            .bind_contracts
            .get("iq")
            .expect("contract for 'iq'");
        // constant() has rate 1, task freq 48000 → rate = 48000 Hz
        assert!(contract.rate_hz.is_some(), "expected rate_hz for OUT bind");
        assert!(
            (contract.rate_hz.unwrap() - 48000.0).abs() < 0.1,
            "expected ~48000 Hz, got {}",
            contract.rate_hz.unwrap()
        );
    }

    #[test]
    fn bind_in_contract_dtype() {
        let reg = test_registry();
        // binwrite() has concrete IN(float, 1) → dtype = Float
        let source = r#"bind iq = udp("127.0.0.1:9100")
clock 48kHz audio {
    @iq | binwrite("/dev/null")
}
"#;
        let result = analyze_ok(source, &reg);
        let contract = result
            .analysis
            .bind_contracts
            .get("iq")
            .expect("contract for 'iq'");
        assert_eq!(contract.dtype, Some(PipitType::Float));
    }

    #[test]
    fn bind_in_contract_rate() {
        let reg = test_registry();
        let source = r#"bind iq = udp("127.0.0.1:9100")
clock 48kHz audio {
    @iq | stdout()
}
"#;
        let result = analyze_ok(source, &reg);
        let contract = result
            .analysis
            .bind_contracts
            .get("iq")
            .expect("contract for 'iq'");
        // stdout() has rate 1, task freq 48000 → rate = 48000 Hz
        assert!(contract.rate_hz.is_some(), "expected rate_hz for IN bind");
        assert!(
            (contract.rate_hz.unwrap() - 48000.0).abs() < 0.1,
            "expected ~48000 Hz, got {}",
            contract.rate_hz.unwrap()
        );
    }

    #[test]
    fn stable_id_deterministic() {
        let reg = test_registry();
        let source = r#"bind iq = udp("127.0.0.1:9100")
clock 48kHz audio {
    constant(0) -> iq
}
"#;
        let r1 = analyze_ok(source, &reg);
        let r2 = analyze_ok(source, &reg);
        let id1 = &r1.analysis.bind_contracts["iq"].stable_id;
        let id2 = &r2.analysis.bind_contracts["iq"].stable_id;
        assert_eq!(
            id1, id2,
            "stable_id must be deterministic across compilations"
        );
        assert_eq!(id1.len(), 16, "stable_id must be 16 hex chars");
    }

    #[test]
    fn stable_id_reorder_stable() {
        let reg = test_registry();
        // Source A: iq first, rf second
        let source_a = r#"bind iq = udp("127.0.0.1:9100")
bind rf = udp("127.0.0.1:9200")
clock 48kHz audio {
    constant(0) -> iq
}
clock 48kHz ctrl {
    constant(0) -> rf
}
"#;
        // Source B: rf first, iq second (bind order swapped)
        let source_b = r#"bind rf = udp("127.0.0.1:9200")
bind iq = udp("127.0.0.1:9100")
clock 48kHz audio {
    constant(0) -> iq
}
clock 48kHz ctrl {
    constant(0) -> rf
}
"#;
        let ra = analyze_ok(source_a, &reg);
        let rb = analyze_ok(source_b, &reg);
        let iq_a = &ra.analysis.bind_contracts["iq"].stable_id;
        let iq_b = &rb.analysis.bind_contracts["iq"].stable_id;
        let rf_a = &ra.analysis.bind_contracts["rf"].stable_id;
        let rf_b = &rb.analysis.bind_contracts["rf"].stable_id;
        assert_eq!(
            iq_a, iq_b,
            "stable_id for 'iq' must be stable under bind reordering"
        );
        assert_eq!(
            rf_a, rf_b,
            "stable_id for 'rf' must be stable under bind reordering"
        );
        assert_ne!(iq_a, rf_a, "different binds must have different stable_ids");
    }

    #[test]
    fn stable_id_topology_change() {
        let reg = test_registry();
        // Source A: constant(0) writes to iq
        let source_a = r#"bind iq = udp("127.0.0.1:9100")
clock 48kHz audio {
    constant(0) -> iq
}
"#;
        // Source B: constant(0) | mul(2.0) writes to iq — different upstream actor
        let source_b = r#"bind iq = udp("127.0.0.1:9100")
clock 48kHz audio {
    constant(0) | mul(2.0) -> iq
}
"#;
        let ra = analyze_ok(source_a, &reg);
        let rb = analyze_ok(source_b, &reg);
        let id_a = &ra.analysis.bind_contracts["iq"].stable_id;
        let id_b = &rb.analysis.bind_contracts["iq"].stable_id;
        assert_ne!(
            id_a, id_b,
            "stable_id must change when graph topology changes"
        );
    }

    // ── SHM endpoint validation tests ──────────────────────────────────────

    #[test]
    fn shm_endpoint_valid() {
        let reg = test_registry();
        let source = r#"bind iq = shm("rx.iq", slots=1024, slot_bytes=4096)
clock 48kHz audio {
    constant(0) -> iq
}
"#;
        let result = analyze_source(source, &reg);
        assert!(!has_error_code(&result, codes::E0720), "unexpected E0720");
        assert!(!has_error_code(&result, codes::E0721), "unexpected E0721");
        assert!(!has_error_code(&result, codes::E0724), "unexpected E0724");
    }

    #[test]
    fn shm_endpoint_missing_slots() {
        let reg = test_registry();
        let source = r#"bind iq = shm("rx.iq", slot_bytes=4096)
clock 48kHz audio {
    constant(0) -> iq
}
"#;
        let result = analyze_source(source, &reg);
        assert!(
            has_error_code(&result, codes::E0720),
            "expected E0720 for missing slots"
        );
    }

    #[test]
    fn shm_endpoint_missing_slot_bytes() {
        let reg = test_registry();
        let source = r#"bind iq = shm("rx.iq", slots=1024)
clock 48kHz audio {
    constant(0) -> iq
}
"#;
        let result = analyze_source(source, &reg);
        assert!(
            has_error_code(&result, codes::E0721),
            "expected E0721 for missing slot_bytes"
        );
    }

    #[test]
    fn shm_endpoint_zero_slots() {
        let reg = test_registry();
        let source = r#"bind iq = shm("rx.iq", slots=0, slot_bytes=4096)
clock 48kHz audio {
    constant(0) -> iq
}
"#;
        let result = analyze_source(source, &reg);
        assert!(
            has_error_code(&result, codes::E0722),
            "expected E0722 for slots=0"
        );
    }

    #[test]
    fn shm_endpoint_zero_slot_bytes() {
        let reg = test_registry();
        let source = r#"bind iq = shm("rx.iq", slots=1024, slot_bytes=0)
clock 48kHz audio {
    constant(0) -> iq
}
"#;
        let result = analyze_source(source, &reg);
        assert!(
            has_error_code(&result, codes::E0723),
            "expected E0723 for slot_bytes=0"
        );
    }

    #[test]
    fn shm_endpoint_missing_name() {
        let reg = test_registry();
        let source = r#"bind iq = shm(slots=1024, slot_bytes=4096)
clock 48kHz audio {
    constant(0) -> iq
}
"#;
        let result = analyze_source(source, &reg);
        assert!(
            has_error_code(&result, codes::E0724),
            "expected E0724 for missing positional name"
        );
    }

    #[test]
    fn shm_endpoint_const_ref_slots() {
        let reg = test_registry();
        let source = r#"const SLOTS = 1024
bind iq = shm("rx.iq", slots=SLOTS, slot_bytes=4096)
clock 48kHz audio {
    constant(0) -> iq
}
"#;
        let result = analyze_source(source, &reg);
        assert!(
            has_error_code(&result, codes::E0725),
            "expected E0725 for const ref in slots"
        );
    }

    #[test]
    fn shm_endpoint_unaligned_slot_bytes() {
        let reg = test_registry();
        let source = r#"bind iq = shm("rx.iq", slots=1024, slot_bytes=100)
clock 48kHz audio {
    constant(0) -> iq
}
"#;
        let result = analyze_source(source, &reg);
        assert!(
            has_error_code(&result, codes::E0726),
            "expected E0726 for slot_bytes=100 (not multiple of 8)"
        );
    }

    #[test]
    fn shm_endpoint_udp_not_validated() {
        // Ensure SHM validation doesn't affect UDP binds
        let reg = test_registry();
        let source = r#"bind iq = udp("127.0.0.1:9100")
clock 48kHz audio {
    constant(0) -> iq
}
"#;
        let result = analyze_source(source, &reg);
        assert!(
            !has_error_code(&result, codes::E0720),
            "SHM validation should not apply to UDP binds"
        );
    }
}
