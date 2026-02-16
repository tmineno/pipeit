// analyze.rs — Static analysis for Pipit SDF graphs
//
// Validates type compatibility at pipe endpoints, solves SDF balance equations
// to compute repetition vectors, verifies feedback loop delay presence,
// checks cross-clock rate matching, computes buffer sizes, and validates
// runtime parameter types.
//
// Preconditions: `program` is a parsed AST; `resolved` passed name resolution;
//                `graph` is a valid ProgramGraph; `registry` has actor metadata.
// Postconditions: returns `AnalysisResult` with computed repetition vectors,
//                 buffer sizes, and all accumulated diagnostics.
// Failure modes: type mismatches, unsolvable balance equations, missing delays,
//                rate mismatches, memory overflow, param type mismatches
//                produce `Diagnostic` entries.
// Side effects: none.

use std::collections::HashMap;

use chumsky::span::Span as _;

use crate::ast::*;
use crate::graph::*;
use crate::registry::{
    ActorMeta, ParamKind, ParamType, PipitType, PortShape, Registry, TokenCount,
};
use crate::resolve::{DiagLevel, Diagnostic, ResolvedProgram};

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
}

// ── Public entry point ──────────────────────────────────────────────────────

/// Run all static analysis checks on a built SDF program graph.
pub fn analyze(
    program: &Program,
    resolved: &ResolvedProgram,
    graph: &ProgramGraph,
    registry: &Registry,
) -> AnalysisResult {
    let mut ctx = AnalyzeCtx::new(program, resolved, graph, registry);
    ctx.check_types();
    ctx.infer_shapes_from_edges();
    ctx.check_shape_constraints();
    ctx.solve_balance_equations();
    ctx.check_feedback_delays();
    ctx.check_cross_clock_rates();
    ctx.compute_buffer_sizes();
    ctx.check_memory_pool();
    ctx.check_param_types();
    ctx.check_ctrl_types();
    ctx.build_result()
}

// ── Internal context ────────────────────────────────────────────────────────

struct AnalyzeCtx<'a> {
    program: &'a Program,
    resolved: &'a ResolvedProgram,
    graph: &'a ProgramGraph,
    registry: &'a Registry,
    diagnostics: Vec<Diagnostic>,
    repetition_vectors: HashMap<(String, String), HashMap<NodeId, u32>>,
    inter_buffers: HashMap<String, u64>,
    total_memory: u64,
    inferred_shapes: HashMap<NodeId, ShapeConstraint>,
}

impl<'a> AnalyzeCtx<'a> {
    fn new(
        program: &'a Program,
        resolved: &'a ResolvedProgram,
        graph: &'a ProgramGraph,
        registry: &'a Registry,
    ) -> Self {
        AnalyzeCtx {
            program,
            resolved,
            graph,
            registry,
            diagnostics: Vec::new(),
            repetition_vectors: HashMap::new(),
            inter_buffers: HashMap::new(),
            total_memory: 0,
            inferred_shapes: HashMap::new(),
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

    fn error_with_hint(&mut self, span: Span, message: String, hint: String) {
        self.diagnostics.push(Diagnostic {
            level: DiagLevel::Error,
            span,
            message,
            hint: Some(hint),
        });
    }

    fn warning(&mut self, span: Span, message: String) {
        self.diagnostics.push(Diagnostic {
            level: DiagLevel::Warning,
            span,
            message,
            hint: None,
        });
    }

    fn build_result(self) -> AnalysisResult {
        AnalysisResult {
            analysis: AnalyzedProgram {
                repetition_vectors: self.repetition_vectors,
                inter_task_buffers: self.inter_buffers,
                total_memory: self.total_memory,
                inferred_shapes: self.inferred_shapes,
            },
            diagnostics: self.diagnostics,
        }
    }

    // ── Helpers ─────────────────────────────────────────────────────────

    /// Look up actor metadata by name.
    fn actor_meta(&self, name: &str) -> Option<&ActorMeta> {
        self.registry.lookup(name)
    }

    /// Get the output type of a node, tracing through passthrough nodes.
    fn infer_output_type(&self, node: &Node, sub: &Subgraph) -> Option<PipitType> {
        match &node.kind {
            NodeKind::Actor { name, .. } => self.actor_meta(name).map(|m| m.out_type),
            NodeKind::Fork { .. } | NodeKind::Probe { .. } | NodeKind::BufferWrite { .. } => {
                // Trace backwards to find upstream actor
                self.trace_type_backward(node.id, sub)
            }
            NodeKind::BufferRead { buffer_name } => {
                // Find the writer task's BufferWrite and trace back from there
                self.infer_buffer_type(buffer_name)
            }
        }
    }

    /// Get the input type of a node, tracing through passthrough nodes.
    fn infer_input_type(&self, node: &Node, sub: &Subgraph) -> Option<PipitType> {
        match &node.kind {
            NodeKind::Actor { name, .. } => self.actor_meta(name).map(|m| m.in_type),
            NodeKind::Fork { .. } | NodeKind::Probe { .. } | NodeKind::BufferRead { .. } => {
                // Passthrough: input type == output type, trace backward
                self.trace_type_backward(node.id, sub)
            }
            NodeKind::BufferWrite { .. } => {
                // BufferWrite accepts whatever type the upstream produces
                self.trace_type_backward(node.id, sub)
            }
        }
    }

    /// Trace backwards from a passthrough node to find the type produced by
    /// the nearest upstream Actor.
    fn trace_type_backward(&self, node_id: NodeId, sub: &Subgraph) -> Option<PipitType> {
        let mut current = node_id;
        let mut visited = Vec::new();
        loop {
            if visited.contains(&current) {
                return None; // cycle guard
            }
            visited.push(current);
            let node = find_node(sub, current)?;
            if let NodeKind::Actor { name, .. } = &node.kind {
                return self.actor_meta(name).map(|m| m.out_type);
            }
            // Find predecessor
            let pred = sub.edges.iter().find(|e| e.target == current);
            match pred {
                Some(edge) => current = edge.source,
                None => return None,
            }
        }
    }

    /// Infer the wire type of a shared buffer by tracing from the writer side.
    fn infer_buffer_type(&self, buffer_name: &str) -> Option<PipitType> {
        let buf_info = self.resolved.buffers.get(buffer_name)?;
        let task_graph = self.graph.tasks.get(&buf_info.writer_task)?;
        let write_node = find_buffer_write_in_task(task_graph, buffer_name)?;
        // Find the subgraph containing this node and trace backward
        for sub in subgraphs_of(task_graph) {
            if find_node(sub, write_node).is_some() {
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
            _ => Some(1),
        }
    }

    /// Resolve a PortShape to a concrete rate (product of resolved dimensions).
    /// Uses shape constraint from call site to infer symbolic dimensions.
    /// Falls back to scalar `resolve_token_count` for rank-1 backward compat.
    fn resolve_port_rate(
        &self,
        shape: &PortShape,
        actor_meta: &ActorMeta,
        actor_args: &[Arg],
        shape_constraint: Option<&ShapeConstraint>,
    ) -> Option<u32> {
        let mut product: u32 = 1;
        for (i, dim) in shape.dims.iter().enumerate() {
            let val = match dim {
                TokenCount::Literal(n) => Some(*n),
                TokenCount::Symbolic(sym) => {
                    // 1. Try resolving from explicit actor arguments
                    let from_arg = actor_meta
                        .params
                        .iter()
                        .position(|p| p.name == *sym)
                        .and_then(|idx| actor_args.get(idx))
                        .and_then(|arg| self.resolve_arg_to_u32(arg));
                    if from_arg.is_some() {
                        from_arg
                    } else {
                        // 2. Try resolving from shape constraint at call site
                        shape_constraint
                            .and_then(|sc| sc.dims.get(i))
                            .and_then(|sd| self.resolve_shape_dim(sd))
                    }
                }
            };
            product = product.checked_mul(val?)?;
        }
        Some(product)
    }

    /// Resolve a single ShapeDim from a call-site shape constraint.
    fn resolve_shape_dim(&self, dim: &ShapeDim) -> Option<u32> {
        match dim {
            ShapeDim::Literal(n, _) => Some(*n),
            ShapeDim::ConstRef(ident) => {
                let entry = self.resolved.consts.get(&ident.name)?;
                let stmt = &self.program.statements[entry.stmt_index];
                if let StatementKind::Const(c) = &stmt.kind {
                    match &c.value {
                        Value::Scalar(Scalar::Number(n, _)) => Some(*n as u32),
                        _ => None,
                    }
                } else {
                    None
                }
            }
        }
    }

    /// Resolve an Arg to a u32 value (for token count resolution).
    fn resolve_arg_to_u32(&self, arg: &Arg) -> Option<u32> {
        match arg {
            Arg::Value(Value::Scalar(Scalar::Number(n, _))) => Some(*n as u32),
            Arg::Value(Value::Array(elems, _)) => Some(elems.len() as u32),
            Arg::ConstRef(ident) => {
                let entry = self.resolved.consts.get(&ident.name)?;
                let stmt = &self.program.statements[entry.stmt_index];
                if let StatementKind::Const(c) = &stmt.kind {
                    match &c.value {
                        Value::Scalar(Scalar::Number(n, _)) => Some(*n as u32),
                        Value::Array(elems, _) => Some(elems.len() as u32),
                        _ => None,
                    }
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Get the clock frequency for a task from the AST.
    fn get_task_freq(&self, task_name: &str) -> Option<(f64, Span)> {
        for stmt in &self.program.statements {
            if let StatementKind::Task(t) = &stmt.kind {
                if t.name.name == task_name {
                    return Some((t.freq, t.freq_span));
                }
            }
        }
        None
    }

    /// Get `set mem` limit in bytes from the AST.
    fn get_mem_limit(&self) -> Option<(u64, Span)> {
        for stmt in &self.program.statements {
            if let StatementKind::Set(set) = &stmt.kind {
                if set.name.name == "mem" {
                    if let SetValue::Size(bytes, span) = &set.value {
                        return Some((*bytes, *span));
                    }
                }
            }
        }
        None
    }

    // ── Phase 0: Shape inference from SDF edges (§13.3.3) ────────────────
    //
    // For actors with unresolved symbolic dimensions in SHAPE(...),
    // propagate known shapes from connected edges. When the connected port
    // has a fully resolved shape of the same rank, infer dimension values
    // positionally (dim-for-dim).

    fn infer_shapes_from_edges(&mut self) {
        for task_graph in self.graph.tasks.values() {
            for sub in subgraphs_of(task_graph) {
                self.infer_shapes_in_subgraph(sub);
            }
        }
    }

    fn infer_shapes_in_subgraph(&mut self, sub: &Subgraph) {
        // Iterate until fixpoint (handles chains like fft()[256] | mag() | some_other())
        let mut changed = true;
        while changed {
            changed = false;
            for edge in &sub.edges {
                let src = match find_node(sub, edge.source) {
                    Some(n) => n,
                    None => continue,
                };
                let tgt = match find_node(sub, edge.target) {
                    Some(n) => n,
                    None => continue,
                };

                // Try: propagate from src's output shape → tgt's input shape
                if let Some(sc) = self.try_propagate_shape(src, tgt, sub) {
                    if let std::collections::hash_map::Entry::Vacant(e) =
                        self.inferred_shapes.entry(tgt.id)
                    {
                        e.insert(sc);
                        changed = true;
                    }
                }

                // Try: propagate from tgt's input shape → src's output shape
                if let Some(sc) = self.try_propagate_shape_reverse(tgt, src, sub) {
                    if let std::collections::hash_map::Entry::Vacant(e) =
                        self.inferred_shapes.entry(src.id)
                    {
                        e.insert(sc);
                        changed = true;
                    }
                }
            }
        }
    }

    /// Try to propagate shape from src's output to tgt's input.
    /// Returns an inferred ShapeConstraint for tgt if successful.
    fn try_propagate_shape(
        &self,
        src: &Node,
        tgt: &Node,
        sub: &Subgraph,
    ) -> Option<ShapeConstraint> {
        // tgt must be an actor with unresolved input shape
        let (tgt_name, tgt_args, tgt_sc) = match &tgt.kind {
            NodeKind::Actor {
                name,
                args,
                shape_constraint,
                ..
            } => (name.as_str(), args.as_slice(), shape_constraint.as_ref()),
            _ => return None,
        };

        // Skip if tgt already has explicit shape constraint or inferred shape
        if tgt_sc.is_some() || self.inferred_shapes.contains_key(&tgt.id) {
            return None;
        }

        let tgt_meta = self.actor_meta(tgt_name)?;

        // Check if tgt has unresolved symbolic dims in its input shape
        if !self.has_unresolved_dims(&tgt_meta.in_shape, tgt_meta, tgt_args) {
            return None;
        }

        // Get src's resolved output shape (as a list of concrete dim values)
        let src_dims = self.resolve_output_shape_dims(src, sub)?;
        let tgt_in_rank = tgt_meta.in_shape.rank();

        // Only propagate if ranks match
        if src_dims.len() != tgt_in_rank {
            return None;
        }

        // Build a ShapeConstraint from src's resolved output dims
        let span = tgt.span;
        Some(ShapeConstraint {
            dims: src_dims
                .into_iter()
                .map(|v| ShapeDim::Literal(v, span))
                .collect(),
            span,
        })
    }

    /// Try to propagate shape from tgt's input back to src's output.
    /// Returns an inferred ShapeConstraint for src if successful.
    fn try_propagate_shape_reverse(
        &self,
        tgt: &Node,
        src: &Node,
        sub: &Subgraph,
    ) -> Option<ShapeConstraint> {
        // src must be an actor with unresolved output shape
        let (src_name, src_args, src_sc) = match &src.kind {
            NodeKind::Actor {
                name,
                args,
                shape_constraint,
                ..
            } => (name.as_str(), args.as_slice(), shape_constraint.as_ref()),
            _ => return None,
        };

        if src_sc.is_some() || self.inferred_shapes.contains_key(&src.id) {
            return None;
        }

        let src_meta = self.actor_meta(src_name)?;

        if !self.has_unresolved_dims(&src_meta.out_shape, src_meta, src_args) {
            return None;
        }

        // Only propagate backward when tgt's input shape has symbolic dimensions.
        // If all dims are Literal, the shape is fixed by the actor definition
        // (e.g. stdout IN(float,1)) and should not be treated as a frame dimension
        // to propagate backward.
        let tgt_meta_for_check = match &tgt.kind {
            NodeKind::Actor { name, .. } => self.actor_meta(name),
            _ => None,
        };
        if let Some(tm) = tgt_meta_for_check {
            if !tm
                .in_shape
                .dims
                .iter()
                .any(|d| matches!(d, TokenCount::Symbolic(_)))
            {
                return None;
            }
        }

        // Get tgt's resolved input shape dims
        let tgt_dims = self.resolve_input_shape_dims(tgt, sub)?;
        let src_out_rank = src_meta.out_shape.rank();

        if tgt_dims.len() != src_out_rank {
            return None;
        }

        let span = src.span;
        Some(ShapeConstraint {
            dims: tgt_dims
                .into_iter()
                .map(|v| ShapeDim::Literal(v, span))
                .collect(),
            span,
        })
    }

    /// Check if a PortShape has any unresolved symbolic dimensions.
    fn has_unresolved_dims(&self, shape: &PortShape, meta: &ActorMeta, args: &[Arg]) -> bool {
        for dim in &shape.dims {
            if let TokenCount::Symbolic(sym) = dim {
                // Check if resolved from args
                let from_arg = meta
                    .params
                    .iter()
                    .position(|p| p.name == *sym)
                    .and_then(|idx| args.get(idx))
                    .and_then(|arg| self.resolve_arg_to_u32(arg));
                if from_arg.is_none() {
                    return true;
                }
            }
        }
        false
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
        let mut visited = Vec::new();
        loop {
            if visited.contains(&current) {
                return None;
            }
            visited.push(current);
            let node = find_node(sub, current)?;
            if let NodeKind::Actor { .. } = &node.kind {
                return self.resolve_output_shape_dims(node, sub);
            }
            let pred = sub.edges.iter().find(|e| e.target == current);
            match pred {
                Some(edge) => current = edge.source,
                None => return None,
            }
        }
    }

    /// Trace forward from a passthrough node to find resolved input shape dims.
    fn trace_shape_forward(&self, node_id: NodeId, sub: &Subgraph) -> Option<Vec<u32>> {
        let mut current = node_id;
        let mut visited = Vec::new();
        loop {
            if visited.contains(&current) {
                return None;
            }
            visited.push(current);
            let node = find_node(sub, current)?;
            if let NodeKind::Actor { .. } = &node.kind {
                return self.resolve_input_shape_dims(node, sub);
            }
            // Find first successor
            let succ = sub.edges.iter().find(|e| e.source == current);
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
                        shape_constraint
                            .and_then(|sc| sc.dims.get(i))
                            .and_then(|sd| self.resolve_shape_dim(sd))
                    }
                }
            };
            dims.push(val?);
        }
        Some(dims)
    }

    // ── Phase 0b: Shape constraint validation (§13.6) ───────────────────

    fn check_shape_constraints(&mut self) {
        for task_graph in self.graph.tasks.values() {
            for sub in subgraphs_of(task_graph) {
                self.check_shape_constraints_in_subgraph(sub);
            }
        }
    }

    fn check_shape_constraints_in_subgraph(&mut self, sub: &Subgraph) {
        for node in &sub.nodes {
            if let NodeKind::Actor {
                name,
                args,
                shape_constraint,
                ..
            } = &node.kind
            {
                // Note: runtime param as shape dim (§13.6 check 1) is already
                // caught by the resolve phase (resolve.rs).

                // Check: unresolved frame dimensions after shape inference (§13.6)
                // Only flag when:
                // - both production and consumption rates are unresolvable
                // - the actor was called with zero args (if args were provided,
                //   the user intentionally left the frame dim to default)
                // - no explicit shape constraint was provided
                if shape_constraint.is_none() && args.is_empty() {
                    if let Some(meta) = self.actor_meta(name) {
                        let has_symbolic = meta
                            .in_shape
                            .dims
                            .iter()
                            .chain(meta.out_shape.dims.iter())
                            .any(|d| matches!(d, TokenCount::Symbolic(_)));

                        if has_symbolic {
                            let prod = self.production_rate(node);
                            let cons = self.consumption_rate(node);

                            if prod.is_none() && cons.is_none() {
                                let sym_name = meta
                                    .in_shape
                                    .dims
                                    .iter()
                                    .chain(meta.out_shape.dims.iter())
                                    .find_map(|d| match d {
                                        TokenCount::Symbolic(s) => Some(s.as_str()),
                                        _ => None,
                                    })
                                    .unwrap_or("?");

                                self.error_with_hint(
                                    node.span,
                                    format!(
                                        "unresolved frame dimension '{}' at actor '{}'",
                                        sym_name, name
                                    ),
                                    format!(
                                        "add explicit shape constraint, e.g. {}()[<size>]",
                                        name
                                    ),
                                );
                            }
                        }
                    }
                }
            }
        }

        // Check 3: conflicting explicit vs edge-inferred shape constraints (§13.6)
        for edge in &sub.edges {
            let src = match find_node(sub, edge.source) {
                Some(n) => n,
                None => continue,
            };
            let tgt = match find_node(sub, edge.target) {
                Some(n) => n,
                None => continue,
            };

            let (tgt_name, tgt_sc) = match &tgt.kind {
                NodeKind::Actor {
                    name,
                    shape_constraint: Some(sc),
                    ..
                } => (name.as_str(), sc),
                _ => continue,
            };

            let src_dims = match self.resolve_output_shape_dims(src, sub) {
                Some(d) => d,
                None => continue,
            };

            let tgt_dims: Option<Vec<u32>> = tgt_sc
                .dims
                .iter()
                .map(|d| self.resolve_shape_dim(d))
                .collect();
            let tgt_dims = match tgt_dims {
                Some(d) => d,
                None => continue,
            };

            if src_dims.len() == tgt_dims.len() {
                for (i, (&sv, &tv)) in src_dims.iter().zip(tgt_dims.iter()).enumerate() {
                    if sv != tv {
                        self.error(
                            tgt_sc.span,
                            format!(
                                "conflicting frame constraint for actor '{}': \
                                 inferred dim[{}]={} from upstream, but explicit shape specifies {}",
                                tgt_name, i, sv, tv
                            ),
                        );
                        break;
                    }
                }
            }
        }
    }

    // ── Phase 1: Type checking ──────────────────────────────────────────

    fn check_types(&mut self) {
        for (task_name, task_graph) in &self.graph.tasks {
            match task_graph {
                TaskGraph::Pipeline(sub) => self.check_types_in_subgraph(task_name, sub),
                TaskGraph::Modal { control, modes } => {
                    self.check_types_in_subgraph(task_name, control);
                    for (_, sub) in modes {
                        self.check_types_in_subgraph(task_name, sub);
                    }
                }
            }
        }
    }

    fn check_types_in_subgraph(&mut self, _task_name: &str, sub: &Subgraph) {
        for edge in &sub.edges {
            let src_node = match find_node(sub, edge.source) {
                Some(n) => n,
                None => continue,
            };
            let tgt_node = match find_node(sub, edge.target) {
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
                    self.error_with_hint(
                        edge.span,
                        format!(
                            "type mismatch at pipe '{} -> {}': {} outputs {}, but {} expects {}",
                            src_name, tgt_name, src_name, st, tgt_name, tt
                        ),
                        format!(
                            "insert a conversion actor between {} and {} (e.g. c2r, mag)",
                            src_name, tgt_name
                        ),
                    );
                }
            }
        }
    }

    // ── Phase 2: SDF balance equation solving ───────────────────────────

    fn solve_balance_equations(&mut self) {
        for (task_name, task_graph) in &self.graph.tasks {
            match task_graph {
                TaskGraph::Pipeline(sub) => {
                    self.solve_subgraph_balance(task_name, "pipeline", sub);
                }
                TaskGraph::Modal { control, modes } => {
                    self.solve_subgraph_balance(task_name, "control", control);
                    for (mode_name, sub) in modes {
                        self.solve_subgraph_balance(task_name, mode_name, sub);
                    }
                }
            }
        }
    }

    fn solve_subgraph_balance(&mut self, task_name: &str, label: &str, sub: &Subgraph) {
        if sub.nodes.is_empty() {
            return;
        }

        // Pre-compute incoming edge counts per node.
        // For multi-input actors (e.g. add(IN(float,2)) with 2 edges),
        // the per-edge consumption rate is in_count / num_incoming_edges.
        let mut incoming_count: HashMap<NodeId, u32> = HashMap::new();
        for edge in &sub.edges {
            *incoming_count.entry(edge.target).or_insert(0) += 1;
        }

        // Build adjacency list: node -> [(neighbor, production, consumption)]
        // For edge (u, v): production = prod(u), consumption = per-edge cons(v)
        let mut rates: HashMap<(NodeId, NodeId), (u32, u32)> = HashMap::new();
        let mut adj: HashMap<NodeId, Vec<NodeId>> = HashMap::new();

        for node in &sub.nodes {
            adj.entry(node.id).or_default();
        }

        for edge in &sub.edges {
            let src = match find_node(sub, edge.source) {
                Some(n) => n,
                None => continue,
            };
            let tgt = match find_node(sub, edge.target) {
                Some(n) => n,
                None => continue,
            };
            let p = self.production_rate(src).unwrap_or(1);
            let total_c = self.consumption_rate(tgt).unwrap_or(1);
            // Split consumption evenly across incoming edges
            let num_in = incoming_count.get(&edge.target).copied().unwrap_or(1);
            let c = if num_in > 1 {
                total_c / num_in
            } else {
                total_c
            };
            rates.insert((edge.source, edge.target), (p, c));
            adj.entry(edge.source).or_default().push(edge.target);
            adj.entry(edge.target).or_default().push(edge.source);
        }

        // BFS with rational arithmetic to compute repetition vector.
        // Each node gets a rational rv = (numerator, denominator).
        let mut rv_rat: HashMap<NodeId, (u64, u64)> = HashMap::new();
        let mut queue = std::collections::VecDeque::new();

        // Process each connected component
        for node in &sub.nodes {
            if rv_rat.contains_key(&node.id) {
                continue;
            }
            // Start of new component
            rv_rat.insert(node.id, (1, 1));
            queue.push_back(node.id);

            while let Some(current) = queue.pop_front() {
                let (cur_num, cur_den) = rv_rat[&current];
                let neighbors: Vec<NodeId> = adj.get(&current).cloned().unwrap_or_default();
                for &neighbor in &neighbors {
                    if rv_rat.contains_key(&neighbor) {
                        continue; // already visited
                    }
                    // Find the edge between current and neighbor to get rates
                    if let Some(&(p, c)) = rates.get(&(current, neighbor)) {
                        // Edge current -> neighbor: rv[current]*p == rv[neighbor]*c
                        // rv[neighbor] = rv[current] * p / c = (cur_num * p) / (cur_den * c)
                        let n_num = cur_num * p as u64;
                        let n_den = cur_den * c as u64;
                        let g = gcd(n_num, n_den);
                        rv_rat.insert(neighbor, (n_num / g, n_den / g));
                        queue.push_back(neighbor);
                    } else if let Some(&(p, c)) = rates.get(&(neighbor, current)) {
                        // Edge neighbor -> current: rv[neighbor]*p == rv[current]*c
                        // rv[neighbor] = rv[current] * c / p = (cur_num * c) / (cur_den * p)
                        let n_num = cur_num * c as u64;
                        let n_den = cur_den * p as u64;
                        let g = gcd(n_num, n_den);
                        rv_rat.insert(neighbor, (n_num / g, n_den / g));
                        queue.push_back(neighbor);
                    }
                }
            }
        }

        // Normalize: find LCM of all denominators, then multiply each numerator
        let lcm_den = rv_rat.values().fold(1u64, |acc, &(_, d)| lcm(acc, d));
        let mut rv: HashMap<NodeId, u32> = HashMap::new();
        for (&node_id, &(num, den)) in &rv_rat {
            let val = num * (lcm_den / den);
            rv.insert(node_id, val as u32);
        }

        // Reduce by GCD of all values
        if !rv.is_empty() {
            let g = rv.values().copied().fold(0u32, gcd32);
            if g > 1 {
                for val in rv.values_mut() {
                    *val /= g;
                }
            }
        }

        // Verify all edges satisfy balance equation
        let mut consistent = true;
        for edge in &sub.edges {
            if let Some(&(p, c)) = rates.get(&(edge.source, edge.target)) {
                let lhs = rv.get(&edge.source).copied().unwrap_or(1) as u64 * p as u64;
                let rhs = rv.get(&edge.target).copied().unwrap_or(1) as u64 * c as u64;
                if lhs != rhs {
                    consistent = false;
                    let src = find_node(sub, edge.source);
                    let tgt = find_node(sub, edge.target);
                    let src_name = src.map(node_display_name).unwrap_or("?".into());
                    let tgt_name = tgt.map(node_display_name).unwrap_or("?".into());
                    self.error(
                        edge.span,
                        format!(
                            "SDF balance equation unsolvable at edge '{} -> {}' in task '{}': \
                             {}×{} ≠ {}×{}",
                            src_name,
                            tgt_name,
                            task_name,
                            rv.get(&edge.source).unwrap_or(&0),
                            p,
                            rv.get(&edge.target).unwrap_or(&0),
                            c
                        ),
                    );
                }
            }
        }

        if consistent {
            self.repetition_vectors
                .insert((task_name.to_string(), label.to_string()), rv);
        }
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
                    span,
                    format!("feedback loop detected at '{}' with no delay", cycle_desc),
                    "insert delay(N, init) to break the cycle".to_string(),
                );
            }
        }
    }

    fn find_node_in_any_subgraph(&self, node_id: NodeId) -> Option<&Node> {
        for task_graph in self.graph.tasks.values() {
            for sub in subgraphs_of(task_graph) {
                if let Some(node) = find_node(sub, node_id) {
                    return Some(node);
                }
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
                        // Modal task writers have variable throughput (mode
                        // switching), so rate mismatch is advisory only.
                        // Pipeline tasks get a hard error per §5.7.
                        let writer_is_modal = matches!(
                            self.graph.tasks.get(&edge.writer_task),
                            Some(TaskGraph::Modal { .. })
                        );
                        if writer_is_modal {
                            self.warning(span, msg);
                        } else {
                            self.error(span, msg);
                        }
                    }
                }
            }
        }
    }

    /// Look up the repetition vector value for a specific node in a task.
    fn get_rv_for_node(&self, task_name: &str, node_id: NodeId) -> Option<u32> {
        // Try "pipeline" label first
        if let Some(rv) = self
            .repetition_vectors
            .get(&(task_name.to_string(), "pipeline".to_string()))
        {
            if let Some(&val) = rv.get(&node_id) {
                return Some(val);
            }
        }
        // Try "control" and mode labels for modal tasks
        for ((tn, _label), rv) in &self.repetition_vectors {
            if tn == task_name {
                if let Some(&val) = rv.get(&node_id) {
                    return Some(val);
                }
            }
        }
        None
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
        if let Some((limit, span)) = self.get_mem_limit() {
            if self.total_memory > limit {
                self.error(
                    span,
                    format!(
                        "shared memory pool exceeded: required {} bytes, available {} bytes (set mem)",
                        self.total_memory, limit
                    ),
                );
            }
        }
    }

    // ── Phase 7: Param type vs RUNTIME_PARAM match ──────────────────────

    fn check_param_types(&mut self) {
        for task_graph in self.graph.tasks.values() {
            for sub in subgraphs_of(task_graph) {
                self.check_param_types_in_subgraph(sub);
            }
        }
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
        let param_entry = match self.resolved.params.get(&param_ident.name) {
            Some(e) => e,
            None => return,
        };
        let stmt = &self.program.statements[param_entry.stmt_index];
        let inferred = if let StatementKind::Param(p) = &stmt.kind {
            infer_param_type(&p.value)
        } else {
            return;
        };

        if let Some(inferred_type) = inferred {
            if !param_type_compatible(&inferred_type, &expected_type) {
                self.error(
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
        for stmt in &self.program.statements {
            let task = match &stmt.kind {
                StatementKind::Task(t) => t,
                _ => continue,
            };
            let modal = match &task.body {
                TaskBody::Modal(m) => m,
                _ => continue,
            };
            let ctrl_buffer_name = match &modal.switch.source {
                SwitchSource::Buffer(ident) => &ident.name,
                SwitchSource::Param(_) => continue, // param-based ctrl: no buffer type check
            };
            let control_sub = match self.graph.tasks.get(&task.name.name) {
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
                                    node.span,
                                    format!(
                                        "ctrl buffer '{}' in task '{}' has type {}, \
                                         but switch ctrl must be int32",
                                        buffer_name, task.name.name, wire_type
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
fn find_node(sub: &Subgraph, id: NodeId) -> Option<&Node> {
    sub.nodes.iter().find(|n| n.id == id)
}

/// Get all subgraphs from a TaskGraph.
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

/// Infer a ParamType from a scalar value.
fn infer_param_type(scalar: &Scalar) -> Option<ParamType> {
    match scalar {
        Scalar::Number(n, _) => {
            if *n == (*n as i64) as f64 {
                Some(ParamType::Int)
            } else {
                Some(ParamType::Float)
            }
        }
        _ => None,
    }
}

/// Check if inferred param type is compatible with expected actor param type.
/// Allows implicit Int→Float and Int→Double promotion (standard C/C++ behavior).
fn param_type_compatible(inferred: &ParamType, expected: &ParamType) -> bool {
    matches!(
        (inferred, expected),
        (ParamType::Float, ParamType::Float)
            | (ParamType::Float, ParamType::Double)
            | (ParamType::Double, ParamType::Double)
            | (ParamType::Int, ParamType::Int)
            | (ParamType::Int, ParamType::Float)
            | (ParamType::Int, ParamType::Double)
    )
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
        let example_actors = root.join("examples/example_actors.h");
        let std_sink = root.join("runtime/libpipit/include/std_sink.h");
        let std_source = root.join("runtime/libpipit/include/std_source.h");
        let mut reg = Registry::new();
        reg.load_header(&std_actors)
            .expect("failed to load std_actors.h");
        reg.load_header(&example_actors)
            .expect("failed to load example_actors.h");
        reg.load_header(&std_sink)
            .expect("failed to load std_sink.h");
        reg.load_header(&std_source)
            .expect("failed to load std_source.h");
        reg
    }

    /// Parse, resolve, build graph, and analyze.
    fn analyze_source(source: &str, registry: &Registry) -> AnalysisResult {
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
        analyze(
            &program,
            &resolve_result.resolved,
            &graph_result.graph,
            registry,
        )
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
        let result = analyze(
            &program,
            &resolve_result.resolved,
            &graph_result.graph,
            registry,
        );
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
        // fft outputs cfloat, but fir expects float → type mismatch
        let result = analyze_source(
            "clock 1kHz t {\n    constant(0.0) | fft(256) | fir(5) | stdout()\n}",
            &reg,
        );
        assert!(
            has_error(&result, "type mismatch"),
            "expected type mismatch error, got: {:#?}",
            result.diagnostics
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
    fn param_type_int_to_float_promoted() {
        let reg = test_registry();
        // param gain = 1 (int), mul has RUNTIME_PARAM(float, gain)
        // Int → Float promotion is allowed
        analyze_ok(
            "param gain = 1\nclock 1kHz t {\n    constant(0.0) | mul($gain) | stdout()\n}",
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
        // fir() outputs float -> ctrl is NOT int32 -> error
        let reg = test_registry();
        let result = analyze_source(
            concat!(
                "const coeff = [1.0]\n",
                "clock 1kHz t {\n",
                "    control {\n",
                "        constant(0.0) | fir(coeff) -> ctrl\n",
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
}
