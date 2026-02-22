// type_infer.rs — Type Inference & Monomorphization (pcc-spec §9, phase 4)
//
// Walks the HIR (defines expanded), collects type constraints from actor
// signatures and pipe connections, and resolves polymorphic actor calls to
// concrete types.
//
// Preconditions: HIR is built from resolved AST; registry contains actor metadata.
// Postconditions: every actor call has a concrete type assignment; widening
//   points are identified; ambiguities produce diagnostics.
// Failure modes: unresolvable type parameters, ambiguous polymorphic calls,
//   cross-family widening attempts.
// Side effects: none.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::ast::{Arg, Scalar, Span, Value};
use crate::hir::{
    HirActorCall, HirPipeElem, HirPipeExpr, HirPipeSource, HirPipeline, HirProgram, HirTask,
    HirTaskBody,
};
use crate::id::CallId;
use crate::registry::{ActorMeta, PipitType, Registry, TypeExpr};
use crate::resolve::{DiagLevel, Diagnostic, ResolvedProgram};

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
    /// For each actor call (keyed by CallId), the resolved concrete type
    /// assignments for its type parameters. Empty map for non-polymorphic actors.
    pub type_assignments: HashMap<CallId, Vec<PipitType>>,

    /// Widening insertions needed: (source_span, from_type, to_type).
    /// Each entry indicates a pipe edge where implicit widening should be applied.
    pub widenings: Vec<WideningPoint>,

    /// Monomorphized actor metadata: for each polymorphic call, the concrete
    /// ActorMeta with type parameters substituted.
    pub mono_actors: HashMap<CallId, ActorMeta>,
}

/// A point in the pipeline where implicit widening should be inserted.
#[derive(Debug, Clone)]
pub struct WideningPoint {
    /// Span of the target actor call (consumer side) — for diagnostics.
    pub target_span: Span,
    /// CallId of the target actor call — primary matching key for lower.
    pub target_call_id: CallId,
    /// Type produced by upstream actor.
    pub from_type: PipitType,
    /// Type expected by downstream actor.
    pub to_type: PipitType,
}

// ── Type inference engine ───────────────────────────────────────────────────

/// Run type inference and monomorphization on the HIR program.
///
/// For non-polymorphic programs with matching types, this is a no-op pass
/// that produces empty type_assignments and widenings.
pub fn type_infer(
    hir: &HirProgram,
    resolved: &ResolvedProgram,
    registry: &Registry,
) -> TypeInferResult {
    // Build param/const type lookup maps for O(1) access
    let param_types: HashMap<&str, PipitType> = hir
        .params
        .iter()
        .filter_map(|p| match &p.default_value {
            Scalar::Number(_, _, is_int) => Some((
                p.name.as_str(),
                if *is_int {
                    PipitType::Int32
                } else {
                    PipitType::Float
                },
            )),
            _ => None,
        })
        .collect();

    let const_types: HashMap<&str, PipitType> = hir
        .consts
        .iter()
        .filter_map(|c| match &c.value {
            Value::Scalar(Scalar::Number(_, _, is_int)) => Some((
                c.name.as_str(),
                if *is_int {
                    PipitType::Int32
                } else {
                    PipitType::Float
                },
            )),
            Value::Array(elems, _) => elems.first().and_then(|e| match e {
                Scalar::Number(_, _, is_int) => Some((
                    c.name.as_str(),
                    if *is_int {
                        PipitType::Int32
                    } else {
                        PipitType::Float
                    },
                )),
                _ => None,
            }),
            _ => None,
        })
        .collect();

    // `resolved` is no longer needed — all lookups go through HIR directly.
    let _ = resolved;

    let mut engine = TypeInferEngine {
        hir,
        registry,
        typed: TypedProgram {
            type_assignments: HashMap::new(),
            widenings: Vec::new(),
            mono_actors: HashMap::new(),
        },
        diagnostics: Vec::new(),
        buffer_types: HashMap::new(),
        tap_types: HashMap::new(),
        effective_registry_meta_cache: RefCell::new(HashMap::new()),
        param_types,
        const_types,
    };

    engine.infer_program();

    TypeInferResult {
        typed: engine.typed,
        diagnostics: engine.diagnostics,
    }
}

struct TypeInferEngine<'a> {
    hir: &'a HirProgram,
    registry: &'a Registry,
    typed: TypedProgram,
    diagnostics: Vec<Diagnostic>,
    /// Output types of shared buffers, keyed by buffer name.
    buffer_types: HashMap<String, PipitType>,
    /// Output types of tap/fork points, keyed by tap label name.
    tap_types: HashMap<String, PipitType>,
    /// Per-call cached registry metadata for non-monomorphized actor calls.
    effective_registry_meta_cache: RefCell<HashMap<CallId, Option<&'a ActorMeta>>>,
    /// Name→type lookup maps for O(1) access.
    param_types: HashMap<&'a str, PipitType>,
    const_types: HashMap<&'a str, PipitType>,
}

impl<'a> TypeInferEngine<'a> {
    fn store_monomorphized_actor(
        &mut self,
        call_id: CallId,
        concrete_types: Vec<PipitType>,
        mono: ActorMeta,
    ) {
        self.typed.type_assignments.insert(call_id, concrete_types);
        self.effective_registry_meta_cache
            .borrow_mut()
            .remove(&call_id);
        self.typed.mono_actors.insert(call_id, mono);
    }

    fn infer_program(&mut self) {
        // Pass 1: Process all tasks (collects buffer/tap output types along the way).
        // All defines are already expanded inline by HIR.
        let tasks: Vec<_> = self.hir.tasks.iter().collect();
        for task in &tasks {
            self.infer_task(task);
        }

        // Pass 2: Re-process tasks that have unresolved polymorphic calls from
        // buffer reads (the writer task may have been processed after the reader)
        let tasks: Vec<_> = self.hir.tasks.iter().collect();
        for task in &tasks {
            self.reinfer_task_buffer_reads(task);
        }
    }

    fn infer_task(&mut self, task: &HirTask) {
        match &task.body {
            HirTaskBody::Pipeline(pipeline) => self.infer_pipeline(pipeline),
            HirTaskBody::Modal(modal) => {
                self.infer_pipeline(&modal.control);
                for (_name, pipeline) in &modal.modes {
                    self.infer_pipeline(pipeline);
                }
            }
        }
    }

    fn infer_pipeline(&mut self, pipeline: &HirPipeline) {
        for pipe in &pipeline.pipes {
            self.infer_pipe_expr(pipe);
        }
    }

    fn infer_pipe_expr(&mut self, pipe: &HirPipeExpr) {
        self.infer_pipe_expr_with_upstream(pipe, None);
    }

    fn infer_pipe_expr_with_upstream(
        &mut self,
        pipe: &HirPipeExpr,
        upstream_type: Option<PipitType>,
    ) {
        // Collect the chain of actor calls in this pipe
        let mut calls: Vec<&HirActorCall> = Vec::new();

        // Determine initial upstream type from pipe source
        let initial_type = match &pipe.source {
            HirPipeSource::ActorCall(call) => {
                calls.push(call);
                upstream_type // ActorCall source has no external upstream
            }
            HirPipeSource::BufferRead(name, _span) => {
                upstream_type.or_else(|| self.buffer_types.get(name.as_str()).copied())
            }
            HirPipeSource::TapRef(name, _span) => {
                upstream_type.or_else(|| self.tap_types.get(name.as_str()).copied())
            }
        };

        // Track tap element positions so we can record their types after inference
        let mut tap_positions: Vec<(String, usize)> = Vec::new(); // (name, call_index)
        let mut call_index = if matches!(&pipe.source, HirPipeSource::ActorCall(_)) {
            1 // source call is at index 0
        } else {
            0
        };

        for elem in &pipe.elements {
            match elem {
                HirPipeElem::ActorCall(call) => {
                    calls.push(call);
                    call_index += 1;
                }
                HirPipeElem::Tap(name, _span) => {
                    tap_positions.push((name.clone(), call_index));
                }
                HirPipeElem::Probe(_, _) => {}
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
                self.call_output_type(calls[idx - 1])
            };
            if let Some(t) = tap_type {
                self.tap_types.insert(tap_name.clone(), t);
            }
        }

        // Track buffer output type if this pipe writes to a buffer
        if let Some(ref sink) = pipe.sink {
            if let Some(out_type) = final_type {
                self.buffer_types.insert(sink.buffer_name.clone(), out_type);
            }
        }
    }

    /// Resolve explicit type arguments (e.g., `fir<float>(coeff)`).
    fn resolve_explicit_type_args(&mut self, call: &HirActorCall) {
        if call.type_args.is_empty() {
            return;
        }

        // All HIR calls are actors (defines already expanded)
        let meta = match self.registry.lookup(&call.name) {
            Some(m) => m,
            None => return,
        };

        if meta.type_params.is_empty() {
            return;
        }

        if call.type_args.len() != meta.type_params.len() {
            return;
        }

        // Parse type argument names to PipitType
        let mut concrete_types = Vec::new();
        for (type_name, type_span) in &call.type_args {
            match parse_type_name(type_name) {
                Some(t) => concrete_types.push(t),
                None => {
                    self.diagnostics.push(Diagnostic {
                        level: DiagLevel::Error,
                        span: *type_span,
                        message: format!("unknown type '{}'", type_name),
                        hint: Some(
                            "valid types: int8, int16, int32, float, double, cfloat, cdouble"
                                .to_string(),
                        ),
                    });
                    return;
                }
            }
        }

        let mono = monomorphize_actor(meta, &concrete_types);
        self.store_monomorphized_actor(call.call_id, concrete_types, mono);
    }

    /// Infer type arguments from pipe context, with an optional initial upstream type.
    /// Returns the output type of the last actor in the chain.
    ///
    /// All calls in HIR are concrete actors (defines already expanded inline).
    fn infer_from_pipe_context_with_initial(
        &mut self,
        calls: &[&HirActorCall],
        initial_type: Option<PipitType>,
    ) -> Option<PipitType> {
        let mut current_output_type: Option<PipitType> = initial_type;

        for call in calls {
            let meta = match self.get_effective_meta(call) {
                Some(m) => m,
                None => {
                    current_output_type = None;
                    continue;
                }
            };

            // For polymorphic actors without explicit type args, try to infer
            if !call.type_args.is_empty() || !meta.is_polymorphic() {
                current_output_type = meta.out_type.as_concrete();
                continue;
            }

            // Polymorphic actor without explicit type args — infer from context
            if let Some(upstream_type) = current_output_type {
                if let TypeExpr::TypeParam(ref param_name) = meta.in_type {
                    if let Some(param_idx) = meta.type_params.iter().position(|p| p == param_name) {
                        let mut concrete_types = vec![PipitType::Void; meta.type_params.len()];
                        concrete_types[param_idx] = upstream_type;

                        let all_resolved = concrete_types.iter().all(|t| *t != PipitType::Void);

                        if all_resolved {
                            let mono = monomorphize_actor(meta, &concrete_types);
                            current_output_type = mono.out_type.as_concrete();
                            self.store_monomorphized_actor(call.call_id, concrete_types, mono);
                            continue;
                        }
                    }
                }

                self.diagnostics.push(Diagnostic {
                    level: DiagLevel::Error,
                    span: call.call_span,
                    message: format!("ambiguous polymorphic actor call '{}'", call.name),
                    hint: Some(format!(
                        "specify type arguments explicitly, e.g. {}<float>({})",
                        call.name,
                        call.args
                            .iter()
                            .map(|_| "...")
                            .collect::<Vec<_>>()
                            .join(", ")
                    )),
                });
                current_output_type = None;
            } else {
                let inferred = self.infer_type_from_args(call, meta);
                if let Some(concrete_types) = inferred {
                    let mono = monomorphize_actor(meta, &concrete_types);
                    current_output_type = mono.out_type.as_concrete();
                    self.store_monomorphized_actor(call.call_id, concrete_types, mono);
                } else {
                    self.diagnostics.push(Diagnostic {
                        level: DiagLevel::Error,
                        span: call.call_span,
                        message: format!("ambiguous polymorphic actor call '{}'", call.name),
                        hint: Some(format!(
                            "specify type arguments explicitly, e.g. {}<float>({})",
                            call.name,
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

    /// Re-process pipes starting with buffer reads that may have had unresolved
    /// polymorphic actors on the first pass (the writer task may have been defined
    /// after the reader task in the source file).
    fn reinfer_task_buffer_reads(&mut self, task: &HirTask) {
        let pipes: Vec<&HirPipeExpr> = match &task.body {
            HirTaskBody::Pipeline(pipeline) => pipeline.pipes.iter().collect(),
            HirTaskBody::Modal(modal) => {
                let mut v: Vec<&HirPipeExpr> = modal.control.pipes.iter().collect();
                for (_name, pipeline) in &modal.modes {
                    v.extend(pipeline.pipes.iter());
                }
                v
            }
        };

        for pipe in pipes {
            if let HirPipeSource::BufferRead(name, _span) = &pipe.source {
                if let Some(&buf_type) = self.buffer_types.get(name.as_str()) {
                    let has_unresolved = pipe.elements.iter().any(|elem| {
                        if let HirPipeElem::ActorCall(call) = elem {
                            let meta = self.registry.lookup(&call.name);
                            if let Some(m) = meta {
                                m.is_polymorphic()
                                    && !self.typed.mono_actors.contains_key(&call.call_id)
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    });

                    if has_unresolved {
                        let call_spans: Vec<Span> = pipe
                            .elements
                            .iter()
                            .filter_map(|e| {
                                if let HirPipeElem::ActorCall(c) = e {
                                    Some(c.call_span)
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
    fn infer_type_from_args(
        &self,
        call: &HirActorCall,
        meta: &ActorMeta,
    ) -> Option<Vec<PipitType>> {
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

    /// Get the output type of an actor call.
    fn call_output_type(&self, call: &HirActorCall) -> Option<PipitType> {
        // Check if it's a monomorphized actor
        if let Some(mono) = self.typed.mono_actors.get(&call.call_id) {
            return mono.out_type.as_concrete();
        }
        // Check actor registry
        if let Some(meta) = self.registry.lookup(&call.name) {
            return meta.out_type.as_concrete();
        }
        None
    }

    /// Infer the type of a runtime param from its default value.
    fn infer_param_type(&self, name: &str) -> Option<PipitType> {
        self.param_types.get(name).copied()
    }

    /// Infer the type of a const from its declared value.
    fn infer_const_type(&self, name: &str) -> Option<PipitType> {
        self.const_types.get(name).copied()
    }

    /// Get the effective ActorMeta for a call — monomorphized if available, else registry.
    fn get_effective_meta(&self, call: &HirActorCall) -> Option<&ActorMeta> {
        if let Some(mono) = self.typed.mono_actors.get(&call.call_id) {
            return Some(mono);
        }

        if let Some(cached) = self
            .effective_registry_meta_cache
            .borrow()
            .get(&call.call_id)
            .copied()
        {
            return cached;
        }

        let looked_up = self.registry.lookup(&call.name);
        self.effective_registry_meta_cache
            .borrow_mut()
            .insert(call.call_id, looked_up);
        looked_up
    }

    /// Check for widening between adjacent actor calls in a pipe.
    fn check_pipe_widening(&mut self, calls: &[&HirActorCall]) {
        if calls.len() < 2 {
            return;
        }

        for i in 0..calls.len() - 1 {
            let src_call = calls[i];
            let tgt_call = calls[i + 1];

            let Some(src_out) = self
                .get_effective_meta(src_call)
                .and_then(|sm| sm.out_type.as_concrete())
            else {
                continue;
            };
            let Some(tgt_in) = self
                .get_effective_meta(tgt_call)
                .and_then(|tm| tm.in_type.as_concrete())
            else {
                continue;
            };

            let (src_type, tgt_type) = (src_out, tgt_in);

            if src_type == tgt_type || src_type == PipitType::Void || tgt_type == PipitType::Void {
                continue;
            }

            if can_widen(src_type, tgt_type) {
                self.typed.widenings.push(WideningPoint {
                    target_span: tgt_call.call_span,
                    target_call_id: tgt_call.call_id,
                    from_type: src_type,
                    to_type: tgt_type,
                });
            }
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
    let subst = |name: &str| -> Option<PipitType> {
        meta.type_params
            .iter()
            .position(|p| p == name)
            .and_then(|idx| concrete_types.get(idx).copied())
    };

    let substitute_type_expr = |te: &TypeExpr| -> TypeExpr {
        match te {
            TypeExpr::Concrete(t) => TypeExpr::Concrete(*t),
            TypeExpr::TypeParam(name) => {
                TypeExpr::Concrete(subst(name.as_str()).unwrap_or(PipitType::Void))
            }
        }
    };

    let substitute_param_type = |pt: &crate::registry::ParamType| -> crate::registry::ParamType {
        match pt {
            crate::registry::ParamType::TypeParam(name) => {
                if let Some(concrete) = subst(name.as_str()) {
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
                if let Some(concrete) = subst(name.as_str()) {
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

    fn infer_source(source: &str) -> TypeInferResult {
        use std::path::PathBuf;
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_path_buf();
        let mut registry = Registry::new();
        registry
            .load_header(&root.join("runtime/libpipit/include/std_actors.h"))
            .expect("load std_actors.h");
        registry
            .load_header(&root.join("runtime/libpipit/include/std_math.h"))
            .expect("load std_math.h");
        let parse_result = crate::parser::parse(source);
        assert!(
            parse_result.errors.is_empty(),
            "parse errors: {:?}",
            parse_result.errors
        );
        let program = parse_result.program.expect("parse failed");
        let mut resolve_result = crate::resolve::resolve(&program, &registry);
        assert!(
            resolve_result
                .diagnostics
                .iter()
                .all(|d| d.level != DiagLevel::Error),
            "resolve errors: {:#?}",
            resolve_result.diagnostics
        );
        let hir = crate::hir::build_hir(
            &program,
            &resolve_result.resolved,
            &mut resolve_result.id_alloc,
        );
        type_infer(&hir, &resolve_result.resolved, &registry)
    }

    #[test]
    fn ambiguous_poly_source_no_context() {
        // sine() has no T-typed params and no upstream — T is ambiguous.
        let result = infer_source("clock 1kHz t {\n    sine(100.0, 1.0) | stdout()\n}");
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.level == DiagLevel::Error
                    && d.message.contains("ambiguous polymorphic")),
            "expected 'ambiguous polymorphic' error, got: {:#?}",
            result.diagnostics
        );
    }

    #[test]
    fn poly_stdout_infers_cfloat_from_fft() {
        // constant(0.0) → T=float, fft → cfloat, stdout<T> → T=cfloat (valid)
        let result = infer_source("clock 1kHz t {\n    constant(0.0) | fft(256) | stdout()\n}");
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.level == DiagLevel::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "unexpected type_infer errors: {:#?}",
            errors
        );
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
