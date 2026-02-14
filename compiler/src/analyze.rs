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
use crate::registry::{ActorMeta, ParamKind, ParamType, PipitType, Registry, TokenCount};
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
        }
    }

    fn error(&mut self, span: Span, message: String) {
        self.diagnostics.push(Diagnostic {
            level: DiagLevel::Error,
            span,
            message,
        });
    }

    fn warning(&mut self, span: Span, message: String) {
        self.diagnostics.push(Diagnostic {
            level: DiagLevel::Warning,
            span,
            message,
        });
    }

    fn build_result(self) -> AnalysisResult {
        AnalysisResult {
            analysis: AnalyzedProgram {
                repetition_vectors: self.repetition_vectors,
                inter_task_buffers: self.inter_buffers,
                total_memory: self.total_memory,
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
    fn production_rate(&self, node: &Node) -> Option<u32> {
        match &node.kind {
            NodeKind::Actor { name, args, .. } => {
                let meta = self.actor_meta(name)?;
                self.resolve_token_count(&meta.out_count, meta, args)
            }
            _ => Some(1),
        }
    }

    /// Get consumption rate of a node (in_count). Passthrough nodes return 1.
    fn consumption_rate(&self, node: &Node) -> Option<u32> {
        match &node.kind {
            NodeKind::Actor { name, args, .. } => {
                let meta = self.actor_meta(name)?;
                self.resolve_token_count(&meta.in_count, meta, args)
            }
            _ => Some(1),
        }
    }

    /// Resolve a TokenCount to a concrete u32 value.
    fn resolve_token_count(
        &self,
        count: &TokenCount,
        actor_meta: &ActorMeta,
        actor_args: &[Arg],
    ) -> Option<u32> {
        match count {
            TokenCount::Literal(n) => Some(*n),
            TokenCount::Symbolic(sym) => {
                let idx = actor_meta.params.iter().position(|p| p.name == *sym)?;
                let arg = actor_args.get(idx)?;
                self.resolve_arg_to_u32(arg)
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
                    self.error(
                        edge.span,
                        format!(
                            "type mismatch at pipe '{} -> {}': {} outputs {}, but {} expects {}",
                            src_name, tgt_name, src_name, st, tgt_name, tt
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
                self.error(
                    span,
                    format!(
                        "feedback loop detected at '{}' with no delay; \
                         hint: insert delay(N, init) to break the cycle",
                        cycle_desc
                    ),
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
                        self.warning(
                            span,
                            format!(
                                "rate mismatch at shared buffer '{}': \
                                 writer '{}' produces {:.0} tokens/sec, \
                                 reader '{}' consumes {:.0} tokens/sec",
                                edge.buffer_name,
                                edge.writer_task,
                                writer_rate,
                                edge.reader_task,
                                reader_rate,
                            ),
                        );
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
                                self.error(
                                    node.span,
                                    format!(
                                        "ctrl buffer '{}' in task '{}' has type {}, \
                                         but switch ctrl must be int32",
                                        buffer_name, task.name.name, wire_type
                                    ),
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
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("examples/actors.h");
        let mut reg = Registry::new();
        reg.load_header(&path).expect("failed to load actors.h");
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

    // ── Phase 1: Type checking tests ────────────────────────────────────

    #[test]
    fn type_check_ok_linear() {
        let reg = test_registry();
        // adc(void→float) | fft(float→cfloat) | mag(cfloat→float) | stdout(float→void)
        analyze_ok(
            "clock 1kHz t {\n    adc(0) | fft(256) | mag() | stdout()\n}",
            &reg,
        );
    }

    #[test]
    fn type_check_mismatch() {
        let reg = test_registry();
        // fft outputs cfloat, but fir expects float → type mismatch
        let result = analyze_source(
            "clock 1kHz t {\n    adc(0) | fft(256) | fir(5) | stdout()\n}",
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
        analyze_ok("clock 1kHz t {\n    adc(0) | stdout()\n}", &reg);
    }

    #[test]
    fn type_check_through_fork() {
        let reg = test_registry();
        // Fork passes float through
        analyze_ok(
            "clock 1kHz t {\n    adc(0) | :raw | stdout()\n    :raw | stdout()\n}",
            &reg,
        );
    }

    #[test]
    fn type_check_through_probe() {
        let reg = test_registry();
        // Probe passes float through
        analyze_ok("clock 1kHz t {\n    adc(0) | ?mon | stdout()\n}", &reg);
    }

    // ── Phase 2: SDF balance equation tests ─────────────────────────────

    #[test]
    fn balance_uniform_rate() {
        let reg = test_registry();
        let result = analyze_ok("clock 1kHz t {\n    adc(0) | stdout()\n}", &reg);
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
        // adc(0) | fft(256) | c2r() | stdout()
        // fft: IN(float,256), OUT(cfloat,256). c2r: IN(cfloat,1), OUT(float,1)
        let result = analyze_ok(
            "clock 1kHz t {\n    adc(0) | fft(256) | c2r() | stdout()\n}",
            &reg,
        );
        let rv = result
            .analysis
            .repetition_vectors
            .get(&("t".to_string(), "pipeline".to_string()))
            .expect("rv missing");
        // All nodes should have consistent rv
        assert!(!rv.is_empty(), "repetition vector should have entries");
    }

    #[test]
    fn balance_fir_symbolic() {
        let reg = test_registry();
        // fir with const coeff array → N = array length = 5
        let result = analyze_ok(
            concat!(
                "const coeff = [0.1, 0.2, 0.4, 0.2, 0.1]\n",
                "clock 1kHz t {\n    adc(0) | fir(coeff) | stdout()\n}",
            ),
            &reg,
        );
        let rv = result
            .analysis
            .repetition_vectors
            .get(&("t".to_string(), "pipeline".to_string()))
            .expect("rv missing");
        // adc→fir: rv[adc]*1 == rv[fir]*5 → rv[adc]=5, rv[fir]=1
        // fir→stdout: rv[fir]*1 == rv[stdout]*1 → rv[stdout]=1
        assert!(!rv.is_empty());
    }

    #[test]
    fn balance_with_fork() {
        let reg = test_registry();
        // Fork creates a branch: adc | :raw | stdout + :raw | mag() needs cfloat input...
        // Actually mag expects cfloat, adc outputs float. Let's use a valid chain:
        // adc(0) | :raw | stdout()
        // :raw | stdout()
        let result = analyze_ok(
            "clock 1kHz t {\n    adc(0) | :raw | stdout()\n    :raw | stdout()\n}",
            &reg,
        );
        let rv = result
            .analysis
            .repetition_vectors
            .get(&("t".to_string(), "pipeline".to_string()))
            .expect("rv missing");
        assert!(!rv.is_empty());
    }

    // ── Phase 3: Feedback delay tests ───────────────────────────────────

    #[test]
    fn feedback_with_delay_ok() {
        let reg = test_registry();
        // Feedback loop with delay — should pass
        analyze_ok(
            concat!(
                "clock 1kHz t {\n",
                "    adc(0) | add(:fb) | :out | stdout()\n",
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
                "    adc(0) | add(:fb) | :out | stdout()\n",
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

    // ── Phase 7: Param type tests ───────────────────────────────────────

    #[test]
    fn param_type_float_ok() {
        let reg = test_registry();
        // param gain = 1.0 (float), mul has RUNTIME_PARAM(float, gain) → match
        analyze_ok(
            "param gain = 1.0\nclock 1kHz t {\n    adc(0) | mul($gain) | stdout()\n}",
            &reg,
        );
    }

    #[test]
    fn param_type_int_to_float_promoted() {
        let reg = test_registry();
        // param gain = 1 (int), mul has RUNTIME_PARAM(float, gain)
        // Int → Float promotion is allowed
        analyze_ok(
            "param gain = 1\nclock 1kHz t {\n    adc(0) | mul($gain) | stdout()\n}",
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
                "        adc(0) | detect() -> ctrl\n",
                "    }\n",
                "    mode a {\n        adc(0) | stdout()\n    }\n",
                "    mode b {\n        adc(0) | stdout()\n    }\n",
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
                "        adc(0) | fir(coeff) -> ctrl\n",
                "    }\n",
                "    mode a {\n        adc(0) | stdout()\n    }\n",
                "    mode b {\n        adc(0) | stdout()\n    }\n",
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
