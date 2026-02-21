// lower.rs — Typed Lowering & Verification (pcc-spec §9.2)
//
// Lowers the type-inferred program to an explicit IR where all implicit widening
// is materialized as synthetic nodes and all polymorphic actors are fully concrete.
// Then verifies L1-L5 proof obligations.
//
// Preconditions: program is name-resolved; type_infer has produced TypedProgram.
// Postconditions: LoweredProgram with all actors concrete, all widening explicit,
//   and Cert evidence for L1-L5 obligations.
// Failure modes: obligation violations produce diagnostics and LowerResult.has_errors().
// Side effects: none.

use std::collections::HashMap;

use crate::ast::*;
use crate::id::CallId;
use crate::registry::{ActorMeta, PipitType, Registry, TokenCount};
use crate::resolve::{CallResolution, DiagLevel, Diagnostic, ResolvedProgram};
use crate::type_infer::{can_widen, TypedProgram};

// ── Output types ────────────────────────────────────────────────────────────

/// Result of typed lowering.
pub struct LowerResult {
    pub lowered: LoweredProgram,
    pub cert: Cert,
    pub diagnostics: Vec<Diagnostic>,
}

impl LowerResult {
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|d| d.level == DiagLevel::Error)
    }
}

/// The lowered program: all actors concrete, all widening explicit.
pub struct LoweredProgram {
    /// Concrete ActorMeta for each actor call span. Includes both originally
    /// concrete actors and monomorphized polymorphic actors.
    pub concrete_actors: HashMap<Span, ActorMeta>,

    /// Synthetic widening nodes inserted between pipe stages.
    /// Keyed by the target actor call span.
    pub widening_nodes: Vec<WideningNode>,

    /// Type instantiations for polymorphic actor calls.
    /// Maps call span → concrete types for each type parameter.
    /// Empty for non-polymorphic calls.
    pub type_instantiations: HashMap<Span, Vec<PipitType>>,

    // ── Stable ID dual-key maps (ADR-021) ─────────────────────────────────
    /// CallId-keyed concrete actors (mirrors `concrete_actors`).
    pub concrete_actors_by_id: HashMap<CallId, ActorMeta>,
    /// CallId-keyed type instantiations (mirrors `type_instantiations`).
    pub type_instantiations_by_id: HashMap<CallId, Vec<PipitType>>,
}

/// A synthetic widening node inserted between two pipe stages.
#[derive(Debug, Clone)]
pub struct WideningNode {
    /// Span of the target actor call (consumer side) — identifies the edge.
    pub target_span: Span,
    /// Source type (produced by upstream actor).
    pub from_type: PipitType,
    /// Target type (expected by downstream actor).
    pub to_type: PipitType,
    /// Synthetic actor name: `_widen_{from}_to_{to}`.
    pub synthetic_name: String,
}

/// Machine-checkable evidence for the L1-L5 proof obligations.
/// Each field records whether the corresponding obligation holds.
#[derive(Debug, Clone)]
pub struct Cert {
    /// L1: Every edge has matching source/target types.
    pub l1_type_consistency: bool,
    /// L2: Only allowed widening chains are used.
    pub l2_widening_safety: bool,
    /// L3: Widening nodes are 1:1, no rate/shape change.
    pub l3_rate_shape_preservation: bool,
    /// L4: Each polymorphic call → exactly one concrete instance.
    pub l4_monomorphization_soundness: bool,
    /// L5: No unresolved types remain.
    pub l5_no_fallback_typing: bool,
}

impl Cert {
    /// True if all obligations pass.
    pub fn all_pass(&self) -> bool {
        self.l1_type_consistency
            && self.l2_widening_safety
            && self.l3_rate_shape_preservation
            && self.l4_monomorphization_soundness
            && self.l5_no_fallback_typing
    }
}

// ── Public entry point ──────────────────────────────────────────────────────

/// Lower the typed program: insert widening nodes, build concrete actor map,
/// and verify L1-L5 obligations.
pub fn lower_and_verify(
    program: &Program,
    resolved: &ResolvedProgram,
    typed: &TypedProgram,
    registry: &Registry,
) -> LowerResult {
    let mut engine = LowerEngine {
        resolved,
        typed,
        registry,
        concrete_actors: HashMap::new(),
        widening_nodes: Vec::new(),
        diagnostics: Vec::new(),
    };

    // Phase 1: Build concrete actor map and insert widening nodes
    engine.lower_program(program);

    // Phase 2: Verify L1-L5 obligations
    let cert = engine.verify_obligations(program);

    // Copy type instantiations from the typed program
    let type_instantiations = typed.type_assignments.clone();

    // Populate CallId-keyed dual maps from span-keyed maps (ADR-021).
    let mut concrete_actors_by_id = HashMap::new();
    for (span, meta) in &engine.concrete_actors {
        if let Some(&call_id) = resolved.call_ids.get(span) {
            concrete_actors_by_id.insert(call_id, meta.clone());
        }
    }
    let mut type_instantiations_by_id = HashMap::new();
    for (span, types) in &type_instantiations {
        if let Some(&call_id) = resolved.call_ids.get(span) {
            type_instantiations_by_id.insert(call_id, types.clone());
        }
    }

    LowerResult {
        lowered: LoweredProgram {
            concrete_actors: engine.concrete_actors,
            widening_nodes: engine.widening_nodes,
            type_instantiations,
            concrete_actors_by_id,
            type_instantiations_by_id,
        },
        cert,
        diagnostics: engine.diagnostics,
    }
}

// ── Lowering engine ─────────────────────────────────────────────────────────

struct LowerEngine<'a> {
    resolved: &'a ResolvedProgram,
    typed: &'a TypedProgram,
    registry: &'a Registry,
    concrete_actors: HashMap<Span, ActorMeta>,
    widening_nodes: Vec<WideningNode>,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> LowerEngine<'a> {
    // ── Phase 1: Lowering ───────────────────────────────────────────────

    fn lower_program(&mut self, program: &Program) {
        for stmt in &program.statements {
            if let StatementKind::Task(task) = &stmt.kind {
                self.lower_task(task);
            }
        }
    }

    fn lower_task(&mut self, task: &TaskStmt) {
        match &task.body {
            TaskBody::Pipeline(body) => self.lower_pipeline_body(body),
            TaskBody::Modal(modal) => {
                self.lower_pipeline_body(&modal.control.body);
                for mode in &modal.modes {
                    self.lower_pipeline_body(&mode.body);
                }
            }
        }
    }

    fn lower_pipeline_body(&mut self, body: &PipelineBody) {
        for pipe in &body.lines {
            self.lower_pipe_expr(pipe);
        }
    }

    fn lower_pipe_expr(&mut self, pipe: &PipeExpr) {
        // Collect actor calls
        let mut calls: Vec<&ActorCall> = Vec::new();

        if let PipeSource::ActorCall(call) = &pipe.source {
            calls.push(call);
        }
        for elem in &pipe.elements {
            if let PipeElem::ActorCall(call) = elem {
                calls.push(call);
            }
        }

        // For each actor call, populate concrete_actors
        for call in &calls {
            self.lower_actor_call(call);
        }

        // Insert widening nodes for this pipe's widening points
        for wp in &self.typed.widenings {
            // Check if this widening point belongs to a call in this pipe
            if calls.iter().any(|c| c.span == wp.target_span) {
                let synthetic_name = format!("_widen_{}_to_{}", wp.from_type, wp.to_type);
                self.widening_nodes.push(WideningNode {
                    target_span: wp.target_span,
                    from_type: wp.from_type,
                    to_type: wp.to_type,
                    synthetic_name,
                });
            }
        }
    }

    fn lower_actor_call(&mut self, call: &ActorCall) {
        let resolution = self.resolved.call_resolutions.get(&call.span);
        if !matches!(resolution, Some(CallResolution::Actor)) {
            return; // define call — skip
        }

        // Check if type_infer provided a monomorphized meta
        if let Some(mono) = self.typed.mono_actors.get(&call.span) {
            self.concrete_actors.insert(call.span, mono.clone());
            return;
        }

        // Use registry meta directly (already concrete for non-polymorphic actors)
        if let Some(meta) = self.registry.lookup(&call.name.name) {
            self.concrete_actors.insert(call.span, meta.clone());
        }
    }

    // ── Phase 2: L1-L5 verification ─────────────────────────────────────

    fn verify_obligations(&mut self, program: &Program) -> Cert {
        let l1 = self.verify_l1_type_consistency(program);
        let l2 = self.verify_l2_widening_safety();
        let l3 = self.verify_l3_rate_shape_preservation();
        let l4 = self.verify_l4_monomorphization_soundness(program);
        let l5 = self.verify_l5_no_fallback_typing();

        Cert {
            l1_type_consistency: l1,
            l2_widening_safety: l2,
            l3_rate_shape_preservation: l3,
            l4_monomorphization_soundness: l4,
            l5_no_fallback_typing: l5,
        }
    }

    /// L1: Every edge has matching source/target types (after widening insertion).
    fn verify_l1_type_consistency(&mut self, program: &Program) -> bool {
        let mut ok = true;

        for stmt in &program.statements {
            if let StatementKind::Task(task) = &stmt.kind {
                if !self.verify_l1_task(task) {
                    ok = false;
                }
            }
        }

        ok
    }

    fn verify_l1_task(&mut self, task: &TaskStmt) -> bool {
        let mut ok = true;
        match &task.body {
            TaskBody::Pipeline(body) => {
                if !self.verify_l1_pipeline_body(body) {
                    ok = false;
                }
            }
            TaskBody::Modal(modal) => {
                if !self.verify_l1_pipeline_body(&modal.control.body) {
                    ok = false;
                }
                for mode in &modal.modes {
                    if !self.verify_l1_pipeline_body(&mode.body) {
                        ok = false;
                    }
                }
            }
        }
        ok
    }

    fn verify_l1_pipeline_body(&mut self, body: &PipelineBody) -> bool {
        let mut ok = true;
        for pipe in &body.lines {
            if !self.verify_l1_pipe(pipe) {
                ok = false;
            }
        }
        ok
    }

    fn verify_l1_pipe(&mut self, pipe: &PipeExpr) -> bool {
        let mut ok = true;

        let mut calls: Vec<&ActorCall> = Vec::new();
        if let PipeSource::ActorCall(call) = &pipe.source {
            calls.push(call);
        }
        for elem in &pipe.elements {
            if let PipeElem::ActorCall(call) = elem {
                calls.push(call);
            }
        }

        // Check each adjacent pair
        for window in calls.windows(2) {
            let src = window[0];
            let tgt = window[1];

            let src_out = self.get_output_type(src);
            let tgt_in = self.get_input_type(tgt);

            let (src_type, tgt_type) = match (src_out, tgt_in) {
                (Some(s), Some(t)) => (s, t),
                _ => continue, // Skip void or unresolved
            };

            if src_type == PipitType::Void || tgt_type == PipitType::Void {
                continue;
            }

            // Check if a widening node covers this edge
            let has_widening = self
                .widening_nodes
                .iter()
                .any(|w| w.target_span == tgt.span);

            if has_widening {
                // After widening insertion, the edge types should match:
                // src -> widen(src_type, tgt_type) -> tgt
                // The widening node's output is tgt_type, which matches tgt's input.
                // The widening node's input is src_type, which matches src's output.
                // So type consistency holds if the widening is valid (checked in L2).
                continue;
            }

            if src_type != tgt_type {
                self.diagnostics.push(Diagnostic {
                    level: DiagLevel::Error,
                    span: tgt.span,
                    message: format!(
                        "lowering verification failed (L1 type consistency): edge type mismatch {} -> {}",
                        src_type, tgt_type
                    ),
                    hint: None,
                });
                ok = false;
            }
        }

        ok
    }

    /// L2: Inserted conversion nodes are only from allowed widening chains.
    fn verify_l2_widening_safety(&mut self) -> bool {
        let mut ok = true;

        for wn in &self.widening_nodes {
            if !can_widen(wn.from_type, wn.to_type) {
                self.diagnostics.push(Diagnostic {
                    level: DiagLevel::Error,
                    span: wn.target_span,
                    message: format!(
                        "lowering verification failed (L2 widening safety): \
                         {} -> {} is not a safe widening chain",
                        wn.from_type, wn.to_type
                    ),
                    hint: Some(
                        "allowed chains: int8->int16->int32->float->double, cfloat->cdouble"
                            .to_string(),
                    ),
                });
                ok = false;
            }
        }

        ok
    }

    /// L3: Widening nodes are 1:1 and do not alter token rate or shape.
    fn verify_l3_rate_shape_preservation(&mut self) -> bool {
        // Widening nodes are synthetic identity actors with rate 1:1.
        // By construction they are always 1:1 (IN(from, 1), OUT(to, 1)).
        // We verify that the upstream and downstream shapes are compatible.
        let mut ok = true;

        for wn in &self.widening_nodes {
            // Get the downstream actor's metadata to check shape
            if let Some(tgt_meta) = self.concrete_actors.get(&wn.target_span) {
                // The widening node is 1:1 by construction.
                // Verify the downstream actor's in_count is compatible.
                // Widening nodes must not change rate — they are scalar 1:1.
                let in_count = &tgt_meta.in_count;
                // This is informational: widening nodes themselves are always 1:1,
                // so L3 is satisfied by construction. But we verify the target
                // hasn't been corrupted.
                if let TokenCount::Literal(n) = in_count {
                    if *n == 0 {
                        self.diagnostics.push(Diagnostic {
                            level: DiagLevel::Error,
                            span: wn.target_span,
                            message: "lowering verification failed (L3 rate/shape preservation): \
                                 widening target has zero-rate input"
                                .to_string(),
                            hint: None,
                        });
                        ok = false;
                    }
                }
            }
        }

        ok
    }

    /// L4: Each polymorphic call is rewritten to exactly one concrete instance.
    fn verify_l4_monomorphization_soundness(&mut self, program: &Program) -> bool {
        let mut ok = true;

        self.verify_l4_walk_program(program, &mut ok);

        ok
    }

    fn verify_l4_walk_program(&mut self, program: &Program, ok: &mut bool) {
        for stmt in &program.statements {
            if let StatementKind::Task(task) = &stmt.kind {
                self.verify_l4_walk_task(task, ok);
            }
        }
    }

    fn verify_l4_walk_task(&mut self, task: &TaskStmt, ok: &mut bool) {
        match &task.body {
            TaskBody::Pipeline(body) => self.verify_l4_walk_body(body, ok),
            TaskBody::Modal(modal) => {
                self.verify_l4_walk_body(&modal.control.body, ok);
                for mode in &modal.modes {
                    self.verify_l4_walk_body(&mode.body, ok);
                }
            }
        }
    }

    fn verify_l4_walk_body(&mut self, body: &PipelineBody, ok: &mut bool) {
        for pipe in &body.lines {
            if let PipeSource::ActorCall(call) = &pipe.source {
                self.verify_l4_call(call, ok);
            }
            for elem in &pipe.elements {
                if let PipeElem::ActorCall(call) = elem {
                    self.verify_l4_call(call, ok);
                }
            }
        }
    }

    fn verify_l4_call(&mut self, call: &ActorCall, ok: &mut bool) {
        let resolution = self.resolved.call_resolutions.get(&call.span);
        if !matches!(resolution, Some(CallResolution::Actor)) {
            return;
        }

        // Check if the registry actor is polymorphic
        if let Some(reg_meta) = self.registry.lookup(&call.name.name) {
            if reg_meta.is_polymorphic() {
                // Must have been monomorphized
                if let Some(concrete) = self.concrete_actors.get(&call.span) {
                    if concrete.is_polymorphic() {
                        self.diagnostics.push(Diagnostic {
                            level: DiagLevel::Error,
                            span: call.span,
                            message: format!(
                                "lowering verification failed (L4 monomorphization soundness): \
                                 polymorphic actor '{}' not fully monomorphized",
                                call.name.name
                            ),
                            hint: Some("specify type arguments explicitly".to_string()),
                        });
                        *ok = false;
                    }
                } else {
                    self.diagnostics.push(Diagnostic {
                        level: DiagLevel::Error,
                        span: call.span,
                        message: format!(
                            "lowering verification failed (L4 monomorphization soundness): \
                             polymorphic actor '{}' has no concrete instance",
                            call.name.name
                        ),
                        hint: Some("specify type arguments explicitly".to_string()),
                    });
                    *ok = false;
                }
            }
        }
    }

    /// L5: No unresolved types remain in the lowered IR.
    fn verify_l5_no_fallback_typing(&mut self) -> bool {
        let mut ok = true;

        for (span, meta) in &self.concrete_actors {
            if !meta.in_type.is_concrete() {
                self.diagnostics.push(Diagnostic {
                    level: DiagLevel::Error,
                    span: *span,
                    message: format!(
                        "lowering verification failed (L5 no fallback typing): \
                         actor '{}' has unresolved input type '{}'",
                        meta.name, meta.in_type
                    ),
                    hint: None,
                });
                ok = false;
            }
            if !meta.out_type.is_concrete() {
                self.diagnostics.push(Diagnostic {
                    level: DiagLevel::Error,
                    span: *span,
                    message: format!(
                        "lowering verification failed (L5 no fallback typing): \
                         actor '{}' has unresolved output type '{}'",
                        meta.name, meta.out_type
                    ),
                    hint: None,
                });
                ok = false;
            }
        }

        ok
    }

    // ── Helpers ──────────────────────────────────────────────────────────

    fn get_output_type(&self, call: &ActorCall) -> Option<PipitType> {
        self.concrete_actors
            .get(&call.span)
            .and_then(|m| m.out_type.as_concrete())
    }

    fn get_input_type(&self, call: &ActorCall) -> Option<PipitType> {
        self.concrete_actors
            .get(&call.span)
            .and_then(|m| m.in_type.as_concrete())
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{PortShape, TokenCount, TypeExpr};
    use chumsky::span::Span as _;

    fn dummy_span() -> Span {
        Span::new((), 0..0)
    }

    fn span(start: usize, end: usize) -> Span {
        Span::new((), start..end)
    }

    fn make_concrete_meta(name: &str, in_t: PipitType, out_t: PipitType) -> ActorMeta {
        ActorMeta {
            name: name.to_string(),
            type_params: Vec::new(),
            in_type: TypeExpr::Concrete(in_t),
            in_count: TokenCount::Literal(1),
            in_shape: PortShape::rank1(TokenCount::Literal(1)),
            out_type: TypeExpr::Concrete(out_t),
            out_count: TokenCount::Literal(1),
            out_shape: PortShape::rank1(TokenCount::Literal(1)),
            params: Vec::new(),
        }
    }

    fn make_polymorphic_meta(name: &str) -> ActorMeta {
        ActorMeta {
            name: name.to_string(),
            type_params: vec!["T".to_string()],
            in_type: TypeExpr::TypeParam("T".to_string()),
            in_count: TokenCount::Literal(1),
            in_shape: PortShape::rank1(TokenCount::Literal(1)),
            out_type: TypeExpr::TypeParam("T".to_string()),
            out_count: TokenCount::Literal(1),
            out_shape: PortShape::rank1(TokenCount::Literal(1)),
            params: Vec::new(),
        }
    }

    // ── L1 tests ────────────────────────────────────────────────────────

    #[test]
    fn l1_matching_types_pass() {
        let s1 = span(0, 10);
        let s2 = span(10, 20);

        let mut concrete_actors = HashMap::new();
        concrete_actors.insert(
            s1,
            make_concrete_meta("src", PipitType::Void, PipitType::Float),
        );
        concrete_actors.insert(
            s2,
            make_concrete_meta("sink", PipitType::Float, PipitType::Void),
        );

        let typed = TypedProgram {
            type_assignments: HashMap::new(),
            widenings: Vec::new(),
            mono_actors: HashMap::new(),
            type_assignments_by_id: HashMap::new(),
            mono_actors_by_id: HashMap::new(),
        };

        let resolved = ResolvedProgram {
            consts: HashMap::new(),
            params: HashMap::new(),
            defines: HashMap::new(),
            tasks: HashMap::new(),
            buffers: HashMap::new(),
            call_resolutions: {
                let mut m = HashMap::new();
                m.insert(s1, CallResolution::Actor);
                m.insert(s2, CallResolution::Actor);
                m
            },
            task_resolutions: HashMap::new(),
            probes: Vec::new(),
            call_ids: HashMap::new(),
            call_spans: HashMap::new(),
            def_ids: HashMap::new(),
            task_ids: HashMap::new(),
        };

        let program = Program {
            statements: vec![Statement {
                kind: StatementKind::Task(TaskStmt {
                    freq: 48000.0,
                    freq_span: dummy_span(),
                    name: Ident {
                        name: "main".to_string(),
                        span: dummy_span(),
                    },
                    body: TaskBody::Pipeline(PipelineBody {
                        lines: vec![PipeExpr {
                            source: PipeSource::ActorCall(ActorCall {
                                name: Ident {
                                    name: "src".to_string(),
                                    span: s1,
                                },
                                type_args: Vec::new(),
                                args: Vec::new(),
                                shape_constraint: None,
                                span: s1,
                            }),
                            elements: vec![PipeElem::ActorCall(ActorCall {
                                name: Ident {
                                    name: "sink".to_string(),
                                    span: s2,
                                },
                                type_args: Vec::new(),
                                args: Vec::new(),
                                shape_constraint: None,
                                span: s2,
                            })],
                            sink: None,
                            span: span(0, 20),
                        }],
                        span: span(0, 20),
                    }),
                }),
                span: span(0, 20),
            }],
            span: span(0, 20),
        };

        let registry = Registry::empty();

        let mut engine = LowerEngine {
            resolved: &resolved,
            typed: &typed,
            registry: &registry,
            concrete_actors,
            widening_nodes: Vec::new(),
            diagnostics: Vec::new(),
        };

        let cert = engine.verify_obligations(&program);
        assert!(
            cert.l1_type_consistency,
            "L1 should pass for matching types"
        );
        assert!(engine.diagnostics.is_empty());
    }

    #[test]
    fn l1_mismatched_types_fail() {
        let s1 = span(0, 10);
        let s2 = span(10, 20);

        let mut concrete_actors = HashMap::new();
        concrete_actors.insert(
            s1,
            make_concrete_meta("src", PipitType::Void, PipitType::Float),
        );
        concrete_actors.insert(
            s2,
            make_concrete_meta("sink", PipitType::Double, PipitType::Void),
        );

        let typed = TypedProgram {
            type_assignments: HashMap::new(),
            widenings: Vec::new(),
            mono_actors: HashMap::new(),
            type_assignments_by_id: HashMap::new(),
            mono_actors_by_id: HashMap::new(),
        };

        let resolved = ResolvedProgram {
            consts: HashMap::new(),
            params: HashMap::new(),
            defines: HashMap::new(),
            tasks: HashMap::new(),
            buffers: HashMap::new(),
            call_resolutions: {
                let mut m = HashMap::new();
                m.insert(s1, CallResolution::Actor);
                m.insert(s2, CallResolution::Actor);
                m
            },
            task_resolutions: HashMap::new(),
            probes: Vec::new(),
            call_ids: HashMap::new(),
            call_spans: HashMap::new(),
            def_ids: HashMap::new(),
            task_ids: HashMap::new(),
        };

        let program = Program {
            statements: vec![Statement {
                kind: StatementKind::Task(TaskStmt {
                    freq: 48000.0,
                    freq_span: dummy_span(),
                    name: Ident {
                        name: "main".to_string(),
                        span: dummy_span(),
                    },
                    body: TaskBody::Pipeline(PipelineBody {
                        lines: vec![PipeExpr {
                            source: PipeSource::ActorCall(ActorCall {
                                name: Ident {
                                    name: "src".to_string(),
                                    span: s1,
                                },
                                type_args: Vec::new(),
                                args: Vec::new(),
                                shape_constraint: None,
                                span: s1,
                            }),
                            elements: vec![PipeElem::ActorCall(ActorCall {
                                name: Ident {
                                    name: "sink".to_string(),
                                    span: s2,
                                },
                                type_args: Vec::new(),
                                args: Vec::new(),
                                shape_constraint: None,
                                span: s2,
                            })],
                            sink: None,
                            span: span(0, 20),
                        }],
                        span: span(0, 20),
                    }),
                }),
                span: span(0, 20),
            }],
            span: span(0, 20),
        };

        let registry = Registry::empty();

        let mut engine = LowerEngine {
            resolved: &resolved,
            typed: &typed,
            registry: &registry,
            concrete_actors,
            widening_nodes: Vec::new(),
            diagnostics: Vec::new(),
        };

        let cert = engine.verify_obligations(&program);
        assert!(
            !cert.l1_type_consistency,
            "L1 should fail for mismatched types"
        );
        assert!(engine.diagnostics.iter().any(|d| d.message.contains("L1")));
    }

    #[test]
    fn l1_widening_edge_passes() {
        let s1 = span(0, 10);
        let s2 = span(10, 20);

        let mut concrete_actors = HashMap::new();
        concrete_actors.insert(
            s1,
            make_concrete_meta("src", PipitType::Void, PipitType::Int32),
        );
        concrete_actors.insert(
            s2,
            make_concrete_meta("sink", PipitType::Float, PipitType::Void),
        );

        let typed = TypedProgram {
            type_assignments: HashMap::new(),
            widenings: Vec::new(),
            mono_actors: HashMap::new(),
            type_assignments_by_id: HashMap::new(),
            mono_actors_by_id: HashMap::new(),
        };

        let resolved = ResolvedProgram {
            consts: HashMap::new(),
            params: HashMap::new(),
            defines: HashMap::new(),
            tasks: HashMap::new(),
            buffers: HashMap::new(),
            call_resolutions: {
                let mut m = HashMap::new();
                m.insert(s1, CallResolution::Actor);
                m.insert(s2, CallResolution::Actor);
                m
            },
            task_resolutions: HashMap::new(),
            probes: Vec::new(),
            call_ids: HashMap::new(),
            call_spans: HashMap::new(),
            def_ids: HashMap::new(),
            task_ids: HashMap::new(),
        };

        let program = Program {
            statements: vec![Statement {
                kind: StatementKind::Task(TaskStmt {
                    freq: 48000.0,
                    freq_span: dummy_span(),
                    name: Ident {
                        name: "main".to_string(),
                        span: dummy_span(),
                    },
                    body: TaskBody::Pipeline(PipelineBody {
                        lines: vec![PipeExpr {
                            source: PipeSource::ActorCall(ActorCall {
                                name: Ident {
                                    name: "src".to_string(),
                                    span: s1,
                                },
                                type_args: Vec::new(),
                                args: Vec::new(),
                                shape_constraint: None,
                                span: s1,
                            }),
                            elements: vec![PipeElem::ActorCall(ActorCall {
                                name: Ident {
                                    name: "sink".to_string(),
                                    span: s2,
                                },
                                type_args: Vec::new(),
                                args: Vec::new(),
                                shape_constraint: None,
                                span: s2,
                            })],
                            sink: None,
                            span: span(0, 20),
                        }],
                        span: span(0, 20),
                    }),
                }),
                span: span(0, 20),
            }],
            span: span(0, 20),
        };

        let registry = Registry::empty();

        // Widening node covers the edge
        let widening_nodes = vec![WideningNode {
            target_span: s2,
            from_type: PipitType::Int32,
            to_type: PipitType::Float,
            synthetic_name: "_widen_int32_to_float".to_string(),
        }];

        let mut engine = LowerEngine {
            resolved: &resolved,
            typed: &typed,
            registry: &registry,
            concrete_actors,
            widening_nodes,
            diagnostics: Vec::new(),
        };

        let cert = engine.verify_obligations(&program);
        assert!(
            cert.l1_type_consistency,
            "L1 should pass when widening node covers the edge"
        );
    }

    // ── L2 tests ────────────────────────────────────────────────────────

    #[test]
    fn l2_valid_widening_passes() {
        let s1 = span(0, 10);
        let concrete_actors = HashMap::new();
        let typed = TypedProgram {
            type_assignments: HashMap::new(),
            widenings: Vec::new(),
            mono_actors: HashMap::new(),
            type_assignments_by_id: HashMap::new(),
            mono_actors_by_id: HashMap::new(),
        };
        let resolved = ResolvedProgram {
            consts: HashMap::new(),
            params: HashMap::new(),
            defines: HashMap::new(),
            tasks: HashMap::new(),
            buffers: HashMap::new(),
            call_resolutions: HashMap::new(),
            task_resolutions: HashMap::new(),
            probes: Vec::new(),
            call_ids: HashMap::new(),
            call_spans: HashMap::new(),
            def_ids: HashMap::new(),
            task_ids: HashMap::new(),
        };
        let registry = Registry::empty();

        let widening_nodes = vec![WideningNode {
            target_span: s1,
            from_type: PipitType::Int32,
            to_type: PipitType::Float,
            synthetic_name: "_widen_int32_to_float".to_string(),
        }];

        let empty_program = Program {
            statements: Vec::new(),
            span: dummy_span(),
        };

        let mut engine = LowerEngine {
            resolved: &resolved,
            typed: &typed,
            registry: &registry,
            concrete_actors,
            widening_nodes,
            diagnostics: Vec::new(),
        };

        let cert = engine.verify_obligations(&empty_program);
        assert!(cert.l2_widening_safety);
    }

    #[test]
    fn l2_cross_family_widening_fails() {
        let s1 = span(0, 10);
        let concrete_actors = HashMap::new();
        let typed = TypedProgram {
            type_assignments: HashMap::new(),
            widenings: Vec::new(),
            mono_actors: HashMap::new(),
            type_assignments_by_id: HashMap::new(),
            mono_actors_by_id: HashMap::new(),
        };
        let resolved = ResolvedProgram {
            consts: HashMap::new(),
            params: HashMap::new(),
            defines: HashMap::new(),
            tasks: HashMap::new(),
            buffers: HashMap::new(),
            call_resolutions: HashMap::new(),
            task_resolutions: HashMap::new(),
            probes: Vec::new(),
            call_ids: HashMap::new(),
            call_spans: HashMap::new(),
            def_ids: HashMap::new(),
            task_ids: HashMap::new(),
        };
        let registry = Registry::empty();

        let widening_nodes = vec![WideningNode {
            target_span: s1,
            from_type: PipitType::Float,
            to_type: PipitType::Cfloat,
            synthetic_name: "_widen_float_to_cfloat".to_string(),
        }];

        let empty_program = Program {
            statements: Vec::new(),
            span: dummy_span(),
        };

        let mut engine = LowerEngine {
            resolved: &resolved,
            typed: &typed,
            registry: &registry,
            concrete_actors,
            widening_nodes,
            diagnostics: Vec::new(),
        };

        let cert = engine.verify_obligations(&empty_program);
        assert!(!cert.l2_widening_safety);
        assert!(engine.diagnostics.iter().any(|d| d.message.contains("L2")));
    }

    // ── L4 tests ────────────────────────────────────────────────────────

    #[test]
    fn l4_monomorphized_passes() {
        let s1 = span(0, 10);

        let mut concrete_actors = HashMap::new();
        concrete_actors.insert(
            s1,
            make_concrete_meta("scale", PipitType::Float, PipitType::Float),
        );

        let typed = TypedProgram {
            type_assignments: HashMap::new(),
            widenings: Vec::new(),
            mono_actors: HashMap::new(),
            type_assignments_by_id: HashMap::new(),
            mono_actors_by_id: HashMap::new(),
        };

        let resolved = ResolvedProgram {
            consts: HashMap::new(),
            params: HashMap::new(),
            defines: HashMap::new(),
            tasks: HashMap::new(),
            buffers: HashMap::new(),
            call_resolutions: {
                let mut m = HashMap::new();
                m.insert(s1, CallResolution::Actor);
                m
            },
            task_resolutions: HashMap::new(),
            probes: Vec::new(),
            call_ids: HashMap::new(),
            call_spans: HashMap::new(),
            def_ids: HashMap::new(),
            task_ids: HashMap::new(),
        };

        let mut registry = Registry::empty();
        registry.insert(make_polymorphic_meta("scale"));

        let program = Program {
            statements: vec![Statement {
                kind: StatementKind::Task(TaskStmt {
                    freq: 48000.0,
                    freq_span: dummy_span(),
                    name: Ident {
                        name: "main".to_string(),
                        span: dummy_span(),
                    },
                    body: TaskBody::Pipeline(PipelineBody {
                        lines: vec![PipeExpr {
                            source: PipeSource::ActorCall(ActorCall {
                                name: Ident {
                                    name: "scale".to_string(),
                                    span: s1,
                                },
                                type_args: Vec::new(),
                                args: Vec::new(),
                                shape_constraint: None,
                                span: s1,
                            }),
                            elements: Vec::new(),
                            sink: None,
                            span: s1,
                        }],
                        span: s1,
                    }),
                }),
                span: s1,
            }],
            span: s1,
        };

        let mut engine = LowerEngine {
            resolved: &resolved,
            typed: &typed,
            registry: &registry,
            concrete_actors,
            widening_nodes: Vec::new(),
            diagnostics: Vec::new(),
        };

        let cert = engine.verify_obligations(&program);
        assert!(cert.l4_monomorphization_soundness);
    }

    #[test]
    fn l4_unmonomorphized_fails() {
        let s1 = span(0, 10);

        // Concrete actors map has the polymorphic meta (not monomorphized)
        let mut concrete_actors = HashMap::new();
        concrete_actors.insert(s1, make_polymorphic_meta("scale"));

        let typed = TypedProgram {
            type_assignments: HashMap::new(),
            widenings: Vec::new(),
            mono_actors: HashMap::new(),
            type_assignments_by_id: HashMap::new(),
            mono_actors_by_id: HashMap::new(),
        };

        let resolved = ResolvedProgram {
            consts: HashMap::new(),
            params: HashMap::new(),
            defines: HashMap::new(),
            tasks: HashMap::new(),
            buffers: HashMap::new(),
            call_resolutions: {
                let mut m = HashMap::new();
                m.insert(s1, CallResolution::Actor);
                m
            },
            task_resolutions: HashMap::new(),
            probes: Vec::new(),
            call_ids: HashMap::new(),
            call_spans: HashMap::new(),
            def_ids: HashMap::new(),
            task_ids: HashMap::new(),
        };

        let mut registry = Registry::empty();
        registry.insert(make_polymorphic_meta("scale"));

        let program = Program {
            statements: vec![Statement {
                kind: StatementKind::Task(TaskStmt {
                    freq: 48000.0,
                    freq_span: dummy_span(),
                    name: Ident {
                        name: "main".to_string(),
                        span: dummy_span(),
                    },
                    body: TaskBody::Pipeline(PipelineBody {
                        lines: vec![PipeExpr {
                            source: PipeSource::ActorCall(ActorCall {
                                name: Ident {
                                    name: "scale".to_string(),
                                    span: s1,
                                },
                                type_args: Vec::new(),
                                args: Vec::new(),
                                shape_constraint: None,
                                span: s1,
                            }),
                            elements: Vec::new(),
                            sink: None,
                            span: s1,
                        }],
                        span: s1,
                    }),
                }),
                span: s1,
            }],
            span: s1,
        };

        let mut engine = LowerEngine {
            resolved: &resolved,
            typed: &typed,
            registry: &registry,
            concrete_actors,
            widening_nodes: Vec::new(),
            diagnostics: Vec::new(),
        };

        let cert = engine.verify_obligations(&program);
        assert!(!cert.l4_monomorphization_soundness);
        assert!(engine.diagnostics.iter().any(|d| d.message.contains("L4")));
    }

    // ── L5 tests ────────────────────────────────────────────────────────

    #[test]
    fn l5_all_concrete_passes() {
        let s1 = span(0, 10);

        let mut concrete_actors = HashMap::new();
        concrete_actors.insert(
            s1,
            make_concrete_meta("scale", PipitType::Float, PipitType::Float),
        );

        let typed = TypedProgram {
            type_assignments: HashMap::new(),
            widenings: Vec::new(),
            mono_actors: HashMap::new(),
            type_assignments_by_id: HashMap::new(),
            mono_actors_by_id: HashMap::new(),
        };

        let resolved = ResolvedProgram {
            consts: HashMap::new(),
            params: HashMap::new(),
            defines: HashMap::new(),
            tasks: HashMap::new(),
            buffers: HashMap::new(),
            call_resolutions: HashMap::new(),
            task_resolutions: HashMap::new(),
            probes: Vec::new(),
            call_ids: HashMap::new(),
            call_spans: HashMap::new(),
            def_ids: HashMap::new(),
            task_ids: HashMap::new(),
        };

        let registry = Registry::empty();
        let empty_program = Program {
            statements: Vec::new(),
            span: dummy_span(),
        };

        let mut engine = LowerEngine {
            resolved: &resolved,
            typed: &typed,
            registry: &registry,
            concrete_actors,
            widening_nodes: Vec::new(),
            diagnostics: Vec::new(),
        };

        let cert = engine.verify_obligations(&empty_program);
        assert!(cert.l5_no_fallback_typing);
    }

    #[test]
    fn l5_unresolved_type_fails() {
        let s1 = span(0, 10);

        let mut concrete_actors = HashMap::new();
        concrete_actors.insert(s1, make_polymorphic_meta("scale")); // TypeParam, not concrete

        let typed = TypedProgram {
            type_assignments: HashMap::new(),
            widenings: Vec::new(),
            mono_actors: HashMap::new(),
            type_assignments_by_id: HashMap::new(),
            mono_actors_by_id: HashMap::new(),
        };

        let resolved = ResolvedProgram {
            consts: HashMap::new(),
            params: HashMap::new(),
            defines: HashMap::new(),
            tasks: HashMap::new(),
            buffers: HashMap::new(),
            call_resolutions: HashMap::new(),
            task_resolutions: HashMap::new(),
            probes: Vec::new(),
            call_ids: HashMap::new(),
            call_spans: HashMap::new(),
            def_ids: HashMap::new(),
            task_ids: HashMap::new(),
        };

        let registry = Registry::empty();
        let empty_program = Program {
            statements: Vec::new(),
            span: dummy_span(),
        };

        let mut engine = LowerEngine {
            resolved: &resolved,
            typed: &typed,
            registry: &registry,
            concrete_actors,
            widening_nodes: Vec::new(),
            diagnostics: Vec::new(),
        };

        let cert = engine.verify_obligations(&empty_program);
        assert!(!cert.l5_no_fallback_typing);
        assert!(engine.diagnostics.iter().any(|d| d.message.contains("L5")));
    }

    // ── Cert tests ──────────────────────────────────────────────────────

    #[test]
    fn cert_all_pass() {
        let cert = Cert {
            l1_type_consistency: true,
            l2_widening_safety: true,
            l3_rate_shape_preservation: true,
            l4_monomorphization_soundness: true,
            l5_no_fallback_typing: true,
        };
        assert!(cert.all_pass());
    }

    #[test]
    fn cert_one_failure() {
        let cert = Cert {
            l1_type_consistency: true,
            l2_widening_safety: false,
            l3_rate_shape_preservation: true,
            l4_monomorphization_soundness: true,
            l5_no_fallback_typing: true,
        };
        assert!(!cert.all_pass());
    }

    // ── Widening node insertion test ────────────────────────────────────

    #[test]
    fn widening_node_name_format() {
        let wn = WideningNode {
            target_span: span(10, 20),
            from_type: PipitType::Int32,
            to_type: PipitType::Float,
            synthetic_name: format!("_widen_{}_to_{}", PipitType::Int32, PipitType::Float),
        };
        assert_eq!(wn.synthetic_name, "_widen_int32_to_float");
    }
}
