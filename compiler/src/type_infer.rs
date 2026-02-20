// type_infer.rs — Type Inference & Monomorphization (pcc-spec §9, phase 4)
//
// Walks the resolved AST, collects type constraints from actor signatures
// and pipe connections, and resolves polymorphic actor calls to concrete types.
//
// Preconditions: AST is name-resolved; registry contains actor metadata.
// Postconditions: every actor call has a concrete type assignment; widening
//   points are identified; ambiguities produce diagnostics.
// Failure modes: unresolvable type parameters, ambiguous polymorphic calls,
//   cross-family widening attempts.
// Side effects: none.

use std::collections::HashMap;

use crate::ast::*;
use crate::registry::{ActorMeta, PipitType, Registry, TypeExpr};
use crate::resolve::{CallResolution, DiagLevel, Diagnostic, ResolvedProgram};

// ── Widening chains (spec §3.4) ─────────────────────────────────────────────

/// Safe implicit widening chains.
/// Real: int8 → int16 → int32 → float → double
/// Complex: cfloat → cdouble
/// Cross-family (real ↔ complex): NEVER implicit.
fn widening_rank(t: PipitType) -> Option<(u8, u8)> {
    // (family, rank) — family 0 = real, family 1 = complex
    match t {
        PipitType::Int8 => Some((0, 0)),
        PipitType::Int16 => Some((0, 1)),
        PipitType::Int32 => Some((0, 2)),
        PipitType::Float => Some((0, 3)),
        PipitType::Double => Some((0, 4)),
        PipitType::Cfloat => Some((1, 0)),
        PipitType::Cdouble => Some((1, 1)),
        PipitType::Void => None,
    }
}

/// Check if `from` can be safely widened to `to`.
pub fn can_widen(from: PipitType, to: PipitType) -> bool {
    if from == to {
        return true;
    }
    match (widening_rank(from), widening_rank(to)) {
        (Some((fam_from, rank_from)), Some((fam_to, rank_to))) => {
            fam_from == fam_to && rank_from < rank_to
        }
        _ => false,
    }
}

/// Find the common widening type for two types (least upper bound in widening chain).
/// Used by future type inference passes for multi-source unification.
#[allow(dead_code)]
fn common_widening_type(a: PipitType, b: PipitType) -> Option<PipitType> {
    if a == b {
        return Some(a);
    }
    if can_widen(a, b) {
        return Some(b);
    }
    if can_widen(b, a) {
        return Some(a);
    }
    None
}

// ── Output types ────────────────────────────────────────────────────────────

/// Result of type inference for the entire program.
pub struct TypeInferResult {
    pub typed: TypedProgram,
    pub diagnostics: Vec<Diagnostic>,
}

/// Concrete type assignments for all actor calls and widening points.
pub struct TypedProgram {
    /// For each actor call (keyed by its AST span), the resolved concrete type
    /// assignments for its type parameters. Empty map for non-polymorphic actors.
    pub type_assignments: HashMap<Span, Vec<PipitType>>,

    /// Widening insertions needed: (source_span, from_type, to_type).
    /// Each entry indicates a pipe edge where implicit widening should be applied.
    pub widenings: Vec<WideningPoint>,

    /// Monomorphized actor metadata: for each polymorphic call, the concrete
    /// ActorMeta with type parameters substituted.
    pub mono_actors: HashMap<Span, ActorMeta>,
}

/// A point in the pipeline where implicit widening should be inserted.
#[derive(Debug, Clone)]
pub struct WideningPoint {
    /// Span of the target actor call (consumer side).
    pub target_span: Span,
    /// Type produced by upstream actor.
    pub from_type: PipitType,
    /// Type expected by downstream actor.
    pub to_type: PipitType,
}

// ── Type inference engine ───────────────────────────────────────────────────

/// Run type inference and monomorphization on the program.
///
/// For non-polymorphic programs with matching types, this is a no-op pass
/// that produces empty type_assignments and widenings.
pub fn type_infer(
    program: &Program,
    resolved: &ResolvedProgram,
    registry: &Registry,
) -> TypeInferResult {
    let mut engine = TypeInferEngine {
        resolved,
        registry,
        typed: TypedProgram {
            type_assignments: HashMap::new(),
            widenings: Vec::new(),
            mono_actors: HashMap::new(),
        },
        diagnostics: Vec::new(),
        buffer_types: HashMap::new(),
        tap_types: HashMap::new(),
        program: Some(program),
    };

    engine.infer_program(program);

    TypeInferResult {
        typed: engine.typed,
        diagnostics: engine.diagnostics,
    }
}

struct TypeInferEngine<'a> {
    resolved: &'a ResolvedProgram,
    registry: &'a Registry,
    typed: TypedProgram,
    diagnostics: Vec<Diagnostic>,
    /// Output types of shared buffers, keyed by buffer name.
    /// Populated as pipe chains with buffer writes are processed.
    buffer_types: HashMap<String, PipitType>,
    /// Output types of tap/fork points, keyed by tap label name.
    tap_types: HashMap<String, PipitType>,
    /// Program AST reference for define body lookup.
    program: Option<&'a Program>,
}

impl<'a> TypeInferEngine<'a> {
    fn infer_program(&mut self, program: &Program) {
        // Pass 1: Process all tasks (collects buffer/tap output types along the way).
        // Define bodies are processed on-demand via define_output_type when
        // a define call is encountered, using the call site's upstream type context.
        for stmt in &program.statements {
            if let StatementKind::Task(task) = &stmt.kind {
                self.infer_task(task);
            }
        }

        // Pass 2: Re-process tasks that have unresolved polymorphic calls from
        // buffer reads (the writer task may have been processed after the reader)
        for stmt in &program.statements {
            if let StatementKind::Task(task) = &stmt.kind {
                self.reinfer_task_buffer_reads(task);
            }
        }
    }

    fn infer_task(&mut self, task: &TaskStmt) {
        match &task.body {
            TaskBody::Pipeline(body) => self.infer_pipeline_body(body),
            TaskBody::Modal(modal) => {
                self.infer_pipeline_body(&modal.control.body);
                for mode in &modal.modes {
                    self.infer_pipeline_body(&mode.body);
                }
            }
        }
    }

    fn infer_pipeline_body(&mut self, body: &PipelineBody) {
        for pipe in &body.lines {
            self.infer_pipe_expr(pipe);
        }
    }

    fn infer_pipe_expr(&mut self, pipe: &PipeExpr) {
        self.infer_pipe_expr_with_upstream(pipe, None);
    }

    fn infer_pipe_expr_with_upstream(&mut self, pipe: &PipeExpr, upstream_type: Option<PipitType>) {
        // Collect the chain of actor calls in this pipe
        let mut calls: Vec<&ActorCall> = Vec::new();

        // Determine initial upstream type from pipe source
        let initial_type = match &pipe.source {
            PipeSource::ActorCall(call) => {
                calls.push(call);
                upstream_type // ActorCall source has no external upstream
            }
            PipeSource::BufferRead(ident) => {
                // Use tracked buffer type if available
                upstream_type.or_else(|| self.buffer_types.get(&ident.name).copied())
            }
            PipeSource::TapRef(ident) => {
                // Use tracked tap type if available
                upstream_type.or_else(|| self.tap_types.get(&ident.name).copied())
            }
        };

        // Track tap element positions so we can record their types after inference
        let mut tap_positions: Vec<(String, usize)> = Vec::new(); // (name, call_index)
        let mut call_index = if matches!(&pipe.source, PipeSource::ActorCall(_)) {
            1 // source call is at index 0
        } else {
            0
        };

        for elem in &pipe.elements {
            match elem {
                PipeElem::ActorCall(call) => {
                    calls.push(call);
                    call_index += 1;
                }
                PipeElem::Tap(ident) => {
                    // Record tap position — its type is the output of the most recent call
                    tap_positions.push((ident.name.clone(), call_index));
                }
                PipeElem::Probe(_) => {}
            }
        }

        // Phase 1: Resolve explicit type arguments for polymorphic calls
        for call in &calls {
            self.resolve_explicit_type_args(call);
        }

        // Phase 2: Infer type arguments from pipe context for unresolved calls
        let final_type = self.infer_from_pipe_context_with_initial(&calls, initial_type);

        // Phase 3: Check widening between pipe edges
        self.check_pipe_widening(&calls);

        // Record tap types — the type at a tap point is the output of the call
        // immediately before it (or the initial type if no call precedes it)
        for (tap_name, idx) in &tap_positions {
            let tap_type = if *idx == 0 {
                initial_type
            } else {
                // Get the output type of the call at idx-1
                self.call_output_type(calls[idx - 1])
            };
            if let Some(t) = tap_type {
                self.tap_types.insert(tap_name.clone(), t);
            }
        }

        // Track buffer output type if this pipe writes to a buffer
        if let Some(ref sink) = pipe.sink {
            if let Some(out_type) = final_type {
                self.buffer_types.insert(sink.buffer.name.clone(), out_type);
            }
        }
    }

    /// Resolve explicit type arguments (e.g., `fir<float>(coeff)`).
    fn resolve_explicit_type_args(&mut self, call: &ActorCall) {
        if call.type_args.is_empty() {
            return;
        }

        let resolution = self.resolved.call_resolutions.get(&call.span);
        if !matches!(resolution, Some(CallResolution::Actor)) {
            return;
        }

        let meta = match self.registry.lookup(&call.name.name) {
            Some(m) => m,
            None => return,
        };

        if meta.type_params.is_empty() {
            // Error already reported by resolver
            return;
        }

        if call.type_args.len() != meta.type_params.len() {
            // Error already reported by resolver
            return;
        }

        // Parse type argument names to PipitType
        let mut concrete_types = Vec::new();
        for type_arg in &call.type_args {
            match parse_type_name(&type_arg.name) {
                Some(t) => concrete_types.push(t),
                None => {
                    self.diagnostics.push(Diagnostic {
                        level: DiagLevel::Error,
                        span: type_arg.span,
                        message: format!("unknown type '{}'", type_arg.name),
                        hint: Some(
                            "valid types: int8, int16, int32, float, double, cfloat, cdouble"
                                .to_string(),
                        ),
                    });
                    return;
                }
            }
        }

        // Monomorphize: create a concrete ActorMeta with substituted types
        let mono = monomorphize_actor(meta, &concrete_types);
        self.typed
            .type_assignments
            .insert(call.span, concrete_types);
        self.typed.mono_actors.insert(call.span, mono);
    }

    /// Infer type arguments from pipe context, with an optional initial upstream type.
    /// Returns the output type of the last actor in the chain.
    fn infer_from_pipe_context_with_initial(
        &mut self,
        calls: &[&ActorCall],
        initial_type: Option<PipitType>,
    ) -> Option<PipitType> {
        // Walk the pipe chain and propagate types forward
        let mut current_output_type: Option<PipitType> = initial_type;

        for call in calls {
            let resolution = self.resolved.call_resolutions.get(&call.span);
            if !matches!(resolution, Some(CallResolution::Actor)) {
                // Define call — try to propagate type through the define body
                current_output_type = self.define_output_type(call, current_output_type);
                continue;
            }

            let meta = match self.get_effective_meta(call) {
                Some(m) => m,
                None => {
                    current_output_type = None;
                    continue;
                }
            };

            // For polymorphic actors without explicit type args, try to infer
            if !call.type_args.is_empty() || !meta.is_polymorphic() {
                // Already resolved or concrete — just propagate output type
                current_output_type = meta.out_type.as_concrete();
                continue;
            }

            // Polymorphic actor without explicit type args — infer from context
            if let Some(upstream_type) = current_output_type {
                // The input type is a type parameter — match it with upstream type
                if let TypeExpr::TypeParam(ref param_name) = meta.in_type {
                    // Find which type parameter this is
                    if let Some(param_idx) = meta.type_params.iter().position(|p| p == param_name) {
                        let mut concrete_types = vec![PipitType::Void; meta.type_params.len()];
                        concrete_types[param_idx] = upstream_type;

                        // Check if all type params are resolved
                        let all_resolved = concrete_types.iter().all(|t| *t != PipitType::Void);

                        if all_resolved {
                            let mono = monomorphize_actor(&meta, &concrete_types);
                            self.typed
                                .type_assignments
                                .insert(call.span, concrete_types);
                            current_output_type = mono.out_type.as_concrete();
                            self.typed.mono_actors.insert(call.span, mono);
                            continue;
                        }
                    }
                }

                // Could not fully infer — emit ambiguity error
                self.diagnostics.push(Diagnostic {
                    level: DiagLevel::Error,
                    span: call.name.span,
                    message: format!("ambiguous polymorphic actor call '{}'", call.name.name),
                    hint: Some(format!(
                        "specify type arguments explicitly, e.g. {}<float>({})",
                        call.name.name,
                        call.args
                            .iter()
                            .map(|_| "...")
                            .collect::<Vec<_>>()
                            .join(", ")
                    )),
                });
                current_output_type = None;
            } else {
                // No upstream type and polymorphic — try to infer from arguments
                let inferred = self.infer_type_from_args(call, &meta);
                if let Some(concrete_types) = inferred {
                    let mono = monomorphize_actor(&meta, &concrete_types);
                    current_output_type = mono.out_type.as_concrete();
                    self.typed
                        .type_assignments
                        .insert(call.span, concrete_types);
                    self.typed.mono_actors.insert(call.span, mono);
                } else {
                    self.diagnostics.push(Diagnostic {
                        level: DiagLevel::Error,
                        span: call.name.span,
                        message: format!("ambiguous polymorphic actor call '{}'", call.name.name),
                        hint: Some(format!(
                            "specify type arguments explicitly, e.g. {}<float>({})",
                            call.name.name,
                            call.args
                                .iter()
                                .map(|_| "...")
                                .collect::<Vec<_>>()
                                .join(", ")
                        )),
                    });
                    current_output_type = None;
                }
            }
        }
        current_output_type
    }

    /// Get the output type of a define call by looking up the define body's last actor.
    /// Also processes the define body with the upstream type context if provided.
    fn define_output_type(
        &mut self,
        call: &ActorCall,
        upstream_type: Option<PipitType>,
    ) -> Option<PipitType> {
        let define_entry = self.resolved.defines.get(&call.name.name)?;
        let program = self.program?;
        let define_stmt = match &program.statements[define_entry.stmt_index].kind {
            StatementKind::Define(d) => d,
            _ => return None,
        };

        // Process the define body with the upstream type context.
        // This allows polymorphic actors inside defines to resolve T from the
        // call site's pipe context (e.g., `constant(0.0) | amplify() | stdout()`
        // propagates float into amplify's body).
        let first_pipe = define_stmt.body.lines.first()?;

        // Collect calls from the first pipe line to infer with upstream context
        let mut calls: Vec<&ActorCall> = Vec::new();
        if let PipeSource::ActorCall(c) = &first_pipe.source {
            calls.push(c);
        }
        for elem in &first_pipe.elements {
            if let PipeElem::ActorCall(c) = elem {
                calls.push(c);
            }
        }

        // Resolve explicit type args first
        for c in &calls {
            self.resolve_explicit_type_args(c);
        }

        // Infer from upstream type context
        let body_output = self.infer_from_pipe_context_with_initial(&calls, upstream_type);

        // Process remaining pipe lines in the define body (if any)
        for pipe in define_stmt.body.lines.iter().skip(1) {
            self.infer_pipe_expr(pipe);
        }

        // Return the output type of the define body
        if body_output.is_some() {
            return body_output;
        }

        // Fallback: check the last actor in the last pipe line
        let last_pipe = define_stmt.body.lines.last()?;
        let mut last_call: Option<&ActorCall> = None;
        for elem in last_pipe.elements.iter().rev() {
            if let PipeElem::ActorCall(c) = elem {
                last_call = Some(c);
                break;
            }
        }
        if last_call.is_none() {
            if let PipeSource::ActorCall(c) = &last_pipe.source {
                last_call = Some(c);
            }
        }

        let last_call = last_call?;
        if let Some(mono) = self.typed.mono_actors.get(&last_call.span) {
            return mono.out_type.as_concrete();
        }
        let meta = self.registry.lookup(&last_call.name.name)?;
        meta.out_type.as_concrete()
    }

    /// Re-process pipes starting with buffer reads that may have had unresolved
    /// polymorphic actors on the first pass (the writer task may have been defined
    /// after the reader task in the source file).
    fn reinfer_task_buffer_reads(&mut self, task: &TaskStmt) {
        let pipes = match &task.body {
            TaskBody::Pipeline(body) => body.lines.iter().collect::<Vec<_>>(),
            TaskBody::Modal(modal) => {
                let mut v: Vec<&PipeExpr> = modal.control.body.lines.iter().collect();
                for mode in &modal.modes {
                    v.extend(mode.body.lines.iter());
                }
                v
            }
        };

        for pipe in pipes {
            if let PipeSource::BufferRead(ident) = &pipe.source {
                if let Some(&buf_type) = self.buffer_types.get(&ident.name) {
                    // Check if any actor in this pipe is still unresolved
                    let has_unresolved = pipe.elements.iter().any(|elem| {
                        if let PipeElem::ActorCall(call) = elem {
                            let meta = self.registry.lookup(&call.name.name);
                            if let Some(m) = meta {
                                m.is_polymorphic()
                                    && !self.typed.mono_actors.contains_key(&call.span)
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    });

                    if has_unresolved {
                        // Clear previous errors for these calls before re-inferring
                        let call_spans: Vec<Span> = pipe
                            .elements
                            .iter()
                            .filter_map(|e| {
                                if let PipeElem::ActorCall(c) = e {
                                    Some(c.name.span)
                                } else {
                                    None
                                }
                            })
                            .collect();
                        self.diagnostics.retain(|d| {
                            !(d.message.contains("ambiguous polymorphic")
                                && call_spans.contains(&d.span))
                        });

                        self.infer_pipe_expr_with_upstream(pipe, Some(buf_type));
                    }
                }
            }
        }
    }

    /// Try to infer type parameters from actor call arguments.
    fn infer_type_from_args(&self, call: &ActorCall, meta: &ActorMeta) -> Option<Vec<PipitType>> {
        let mut concrete_types = vec![PipitType::Void; meta.type_params.len()];

        for (i, arg) in call.args.iter().enumerate() {
            if i >= meta.params.len() {
                break;
            }
            let param = &meta.params[i];
            let param_type_name = match &param.param_type {
                crate::registry::ParamType::TypeParam(name) => Some(name.as_str()),
                _ => None,
            };

            if let Some(tp_name) = param_type_name {
                if let Some(idx) = meta.type_params.iter().position(|p| p == tp_name) {
                    // Try to determine the type from the argument
                    let arg_type = self.infer_arg_type(arg);
                    if let Some(t) = arg_type {
                        if concrete_types[idx] == PipitType::Void {
                            concrete_types[idx] = t;
                        }
                    }
                }
            }
        }

        if concrete_types.iter().all(|t| *t != PipitType::Void) {
            Some(concrete_types)
        } else {
            None
        }
    }

    /// Infer the type of an argument expression.
    fn infer_arg_type(&self, arg: &Arg) -> Option<PipitType> {
        match arg {
            Arg::Value(Value::Scalar(Scalar::Number(_, _, is_int))) => {
                if *is_int {
                    Some(PipitType::Int32) // Integer literals default to int32
                } else {
                    Some(PipitType::Float) // Float literals default to float
                }
            }
            Arg::ParamRef(ident) => {
                // Look up param's declared value type
                self.infer_param_type(&ident.name)
            }
            Arg::ConstRef(ident) => {
                // Look up const's declared value type
                self.infer_const_type(&ident.name)
            }
            _ => None,
        }
    }

    /// Get the output type of a call (actor or define).
    fn call_output_type(&self, call: &ActorCall) -> Option<PipitType> {
        // Check if it's a monomorphized actor
        if let Some(mono) = self.typed.mono_actors.get(&call.span) {
            return mono.out_type.as_concrete();
        }
        // Check actor registry
        if let Some(meta) = self.registry.lookup(&call.name.name) {
            return meta.out_type.as_concrete();
        }
        // Check if it's a define — get the define body's output type
        if let Some(define_entry) = self.resolved.defines.get(&call.name.name) {
            if let Some(program) = self.program {
                if let StatementKind::Define(def) =
                    &program.statements[define_entry.stmt_index].kind
                {
                    // Find the last actor call in the define body
                    if let Some(last_pipe) = def.body.lines.last() {
                        let mut last_call: Option<&ActorCall> = None;
                        for elem in last_pipe.elements.iter().rev() {
                            if let PipeElem::ActorCall(c) = elem {
                                last_call = Some(c);
                                break;
                            }
                        }
                        if last_call.is_none() {
                            if let PipeSource::ActorCall(c) = &last_pipe.source {
                                last_call = Some(c);
                            }
                        }
                        if let Some(lc) = last_call {
                            return self.call_output_type(lc);
                        }
                    }
                }
            }
        }
        None
    }

    /// Infer the type of a runtime param from its default value.
    fn infer_param_type(&self, name: &str) -> Option<PipitType> {
        let entry = self.resolved.params.get(name)?;
        let program = self.program?;
        let stmt = &program.statements[entry.stmt_index];
        if let StatementKind::Param(p) = &stmt.kind {
            match &p.value {
                Scalar::Number(_, _, is_int) => {
                    if *is_int {
                        Some(PipitType::Int32)
                    } else {
                        Some(PipitType::Float)
                    }
                }
                _ => None,
            }
        } else {
            None
        }
    }

    /// Infer the type of a const from its declared value.
    fn infer_const_type(&self, name: &str) -> Option<PipitType> {
        let entry = self.resolved.consts.get(name)?;
        let program = self.program?;
        let stmt = &program.statements[entry.stmt_index];
        if let StatementKind::Const(c) = &stmt.kind {
            match &c.value {
                Value::Scalar(Scalar::Number(_, _, is_int)) => {
                    if *is_int {
                        Some(PipitType::Int32)
                    } else {
                        Some(PipitType::Float)
                    }
                }
                Value::Array(elems, _) => {
                    // Infer from first element
                    if let Some(Scalar::Number(_, _, is_int)) = elems.first() {
                        if *is_int {
                            Some(PipitType::Int32)
                        } else {
                            Some(PipitType::Float)
                        }
                    } else {
                        None
                    }
                }
                _ => None,
            }
        } else {
            None
        }
    }

    /// Get the effective ActorMeta for a call — monomorphized if available, else registry.
    fn get_effective_meta(&self, call: &ActorCall) -> Option<ActorMeta> {
        if let Some(mono) = self.typed.mono_actors.get(&call.span) {
            return Some(mono.clone());
        }
        self.registry.lookup(&call.name.name).cloned()
    }

    /// Check for widening between adjacent actor calls in a pipe.
    fn check_pipe_widening(&mut self, calls: &[&ActorCall]) {
        if calls.len() < 2 {
            return;
        }

        for i in 0..calls.len() - 1 {
            let src_call = calls[i];
            let tgt_call = calls[i + 1];

            let src_meta = self.get_effective_meta(src_call);
            let tgt_meta = self.get_effective_meta(tgt_call);

            let (src_out, tgt_in) = match (src_meta, tgt_meta) {
                (Some(ref sm), Some(ref tm)) => {
                    (sm.out_type.as_concrete(), tm.in_type.as_concrete())
                }
                _ => continue,
            };

            let (src_type, tgt_type) = match (src_out, tgt_in) {
                (Some(s), Some(t)) => (s, t),
                _ => continue,
            };

            if src_type == tgt_type || src_type == PipitType::Void || tgt_type == PipitType::Void {
                continue; // Exact match or void passthrough
            }

            if can_widen(src_type, tgt_type) {
                self.typed.widenings.push(WideningPoint {
                    target_span: tgt_call.span,
                    from_type: src_type,
                    to_type: tgt_type,
                });
            }
            // Note: type mismatch errors are still caught by analyze.rs (Step 8 will
            // update analyze to accept widening-compatible types)
        }
    }
}

/// Parse a type name string to PipitType.
fn parse_type_name(name: &str) -> Option<PipitType> {
    match name {
        "int8" => Some(PipitType::Int8),
        "int16" => Some(PipitType::Int16),
        "int32" => Some(PipitType::Int32),
        "float" => Some(PipitType::Float),
        "double" => Some(PipitType::Double),
        "cfloat" => Some(PipitType::Cfloat),
        "cdouble" => Some(PipitType::Cdouble),
        _ => None,
    }
}

/// Create a concrete ActorMeta by substituting type parameters with concrete types.
fn monomorphize_actor(meta: &ActorMeta, concrete_types: &[PipitType]) -> ActorMeta {
    let subst: HashMap<&str, PipitType> = meta
        .type_params
        .iter()
        .zip(concrete_types.iter())
        .map(|(name, ty)| (name.as_str(), *ty))
        .collect();

    let substitute_type_expr = |te: &TypeExpr| -> TypeExpr {
        match te {
            TypeExpr::Concrete(t) => TypeExpr::Concrete(*t),
            TypeExpr::TypeParam(name) => {
                TypeExpr::Concrete(*subst.get(name.as_str()).unwrap_or(&PipitType::Void))
            }
        }
    };

    let substitute_param_type = |pt: &crate::registry::ParamType| -> crate::registry::ParamType {
        match pt {
            crate::registry::ParamType::TypeParam(name) => {
                if let Some(&concrete) = subst.get(name.as_str()) {
                    match concrete {
                        PipitType::Int32 => crate::registry::ParamType::Int,
                        PipitType::Float => crate::registry::ParamType::Float,
                        PipitType::Double => crate::registry::ParamType::Double,
                        _ => pt.clone(), // Keep as-is for unsupported param types
                    }
                } else {
                    pt.clone()
                }
            }
            crate::registry::ParamType::SpanTypeParam(name) => {
                if let Some(&concrete) = subst.get(name.as_str()) {
                    match concrete {
                        PipitType::Float => crate::registry::ParamType::SpanFloat,
                        _ => pt.clone(),
                    }
                } else {
                    pt.clone()
                }
            }
            other => other.clone(),
        }
    };

    ActorMeta {
        name: meta.name.clone(),
        type_params: Vec::new(), // Monomorphized — no type params
        in_type: substitute_type_expr(&meta.in_type),
        in_count: meta.in_count.clone(),
        in_shape: meta.in_shape.clone(),
        out_type: substitute_type_expr(&meta.out_type),
        out_count: meta.out_count.clone(),
        out_shape: meta.out_shape.clone(),
        params: meta
            .params
            .iter()
            .map(|p| crate::registry::ActorParam {
                kind: p.kind,
                param_type: substitute_param_type(&p.param_type),
                name: p.name.clone(),
            })
            .collect(),
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widening_same_type() {
        assert!(can_widen(PipitType::Float, PipitType::Float));
    }

    #[test]
    fn widening_real_chain() {
        assert!(can_widen(PipitType::Int8, PipitType::Int16));
        assert!(can_widen(PipitType::Int16, PipitType::Int32));
        assert!(can_widen(PipitType::Int32, PipitType::Float));
        assert!(can_widen(PipitType::Float, PipitType::Double));
        // Transitive
        assert!(can_widen(PipitType::Int8, PipitType::Double));
    }

    #[test]
    fn widening_complex_chain() {
        assert!(can_widen(PipitType::Cfloat, PipitType::Cdouble));
    }

    #[test]
    fn narrowing_rejected() {
        assert!(!can_widen(PipitType::Double, PipitType::Float));
        assert!(!can_widen(PipitType::Float, PipitType::Int32));
        assert!(!can_widen(PipitType::Cdouble, PipitType::Cfloat));
    }

    #[test]
    fn cross_family_rejected() {
        assert!(!can_widen(PipitType::Float, PipitType::Cfloat));
        assert!(!can_widen(PipitType::Cfloat, PipitType::Float));
        assert!(!can_widen(PipitType::Int32, PipitType::Cfloat));
        assert!(!can_widen(PipitType::Double, PipitType::Cdouble));
    }

    #[test]
    fn void_not_widenable() {
        assert!(!can_widen(PipitType::Void, PipitType::Float));
        assert!(!can_widen(PipitType::Float, PipitType::Void));
    }

    #[test]
    fn common_widening_type_same() {
        assert_eq!(
            common_widening_type(PipitType::Float, PipitType::Float),
            Some(PipitType::Float)
        );
    }

    #[test]
    fn common_widening_type_chain() {
        assert_eq!(
            common_widening_type(PipitType::Int32, PipitType::Float),
            Some(PipitType::Float)
        );
        assert_eq!(
            common_widening_type(PipitType::Float, PipitType::Int32),
            Some(PipitType::Float)
        );
    }

    #[test]
    fn common_widening_type_cross_family() {
        assert_eq!(
            common_widening_type(PipitType::Float, PipitType::Cfloat),
            None
        );
    }

    #[test]
    fn monomorphize_simple() {
        let meta = ActorMeta {
            name: "scale".to_string(),
            type_params: vec!["T".to_string()],
            in_type: TypeExpr::TypeParam("T".to_string()),
            in_count: crate::registry::TokenCount::Literal(1),
            in_shape: crate::registry::PortShape::rank1(crate::registry::TokenCount::Literal(1)),
            out_type: TypeExpr::TypeParam("T".to_string()),
            out_count: crate::registry::TokenCount::Literal(1),
            out_shape: crate::registry::PortShape::rank1(crate::registry::TokenCount::Literal(1)),
            params: vec![crate::registry::ActorParam {
                kind: crate::registry::ParamKind::Param,
                param_type: crate::registry::ParamType::TypeParam("T".to_string()),
                name: "gain".to_string(),
            }],
        };

        let mono = monomorphize_actor(&meta, &[PipitType::Float]);
        assert!(mono.type_params.is_empty());
        assert_eq!(mono.in_type, TypeExpr::Concrete(PipitType::Float));
        assert_eq!(mono.out_type, TypeExpr::Concrete(PipitType::Float));
        assert_eq!(mono.params[0].param_type, crate::registry::ParamType::Float);
    }

    #[test]
    fn parse_type_names() {
        assert_eq!(parse_type_name("float"), Some(PipitType::Float));
        assert_eq!(parse_type_name("double"), Some(PipitType::Double));
        assert_eq!(parse_type_name("int32"), Some(PipitType::Int32));
        assert_eq!(parse_type_name("cfloat"), Some(PipitType::Cfloat));
        assert_eq!(parse_type_name("unknown"), None);
    }
}
