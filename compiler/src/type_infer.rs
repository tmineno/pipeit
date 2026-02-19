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
}

impl<'a> TypeInferEngine<'a> {
    fn infer_program(&mut self, program: &Program) {
        for stmt in &program.statements {
            if let StatementKind::Task(task) = &stmt.kind {
                self.infer_task(task);
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
        // Collect the chain of actor calls in this pipe
        let mut calls: Vec<&ActorCall> = Vec::new();

        if let PipeSource::ActorCall(call) = &pipe.source {
            calls.push(call);
        }

        for elem in &pipe.elements {
            if let PipeElem::ActorCall(call) = elem {
                calls.push(call);
            }
        }

        // Phase 1: Resolve explicit type arguments for polymorphic calls
        for call in &calls {
            self.resolve_explicit_type_args(call);
        }

        // Phase 2: Infer type arguments from pipe context for unresolved calls
        self.infer_from_pipe_context(&calls);

        // Phase 3: Check widening between pipe edges
        self.check_pipe_widening(&calls);
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

    /// Infer type arguments from pipe context for polymorphic calls without explicit type args.
    fn infer_from_pipe_context(&mut self, calls: &[&ActorCall]) {
        // Walk the pipe chain and propagate types forward
        let mut current_output_type: Option<PipitType> = None;

        for call in calls {
            let resolution = self.resolved.call_resolutions.get(&call.span);
            if !matches!(resolution, Some(CallResolution::Actor)) {
                // Define call — no type to propagate
                current_output_type = None;
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
            Arg::ConstRef(_ident) => {
                // Const type inference requires access to the AST const value.
                // For now, return None — explicit type args are required when
                // the type can't be inferred from pipe context.
                None
            }
            _ => None,
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
