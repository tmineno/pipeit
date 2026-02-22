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

use crate::ast::Span;
use crate::hir::{
    HirActorCall, HirPipeElem, HirPipeExpr, HirPipeSource, HirPipeline, HirProgram, HirTask,
    HirTaskBody,
};
use crate::id::CallId;
use crate::registry::{ActorMeta, PipitType, Registry, TokenCount};
use crate::resolve::{DiagLevel, Diagnostic, ResolvedProgram};
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
    /// Concrete ActorMeta for each actor call (keyed by CallId).
    pub concrete_actors: HashMap<CallId, ActorMeta>,

    /// Synthetic widening nodes inserted between pipe stages.
    pub widening_nodes: Vec<WideningNode>,

    /// Type instantiations for polymorphic actor calls (keyed by CallId).
    pub type_instantiations: HashMap<CallId, Vec<PipitType>>,
}

/// A synthetic widening node inserted between two pipe stages.
#[derive(Debug, Clone)]
pub struct WideningNode {
    /// Span of the target actor call (consumer side) — retained for diagnostics.
    pub target_span: Span,
    /// CallId of the target actor call — primary matching key.
    pub target_call_id: CallId,
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
    hir: &HirProgram,
    resolved: &ResolvedProgram,
    typed: &TypedProgram,
    registry: &Registry,
) -> LowerResult {
    let mut engine = LowerEngine {
        hir,
        resolved,
        typed,
        registry,
        concrete_actors: HashMap::new(),
        widening_nodes: Vec::new(),
        diagnostics: Vec::new(),
    };

    // Phase 1: Build concrete actor map and insert widening nodes
    engine.lower_program();

    // Phase 2: Verify L1-L5 obligations
    let cert = engine.verify_obligations();

    LowerResult {
        lowered: LoweredProgram {
            concrete_actors: engine.concrete_actors,
            widening_nodes: engine.widening_nodes,
            type_instantiations: typed.type_assignments.clone(),
        },
        cert,
        diagnostics: engine.diagnostics,
    }
}

// ── Lowering engine ─────────────────────────────────────────────────────────

struct LowerEngine<'a> {
    hir: &'a HirProgram,
    resolved: &'a ResolvedProgram,
    typed: &'a TypedProgram,
    registry: &'a Registry,
    concrete_actors: HashMap<CallId, ActorMeta>,
    widening_nodes: Vec<WideningNode>,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> LowerEngine<'a> {
    // ── Phase 1: Lowering ───────────────────────────────────────────────

    fn lower_program(&mut self) {
        for task in &self.hir.tasks {
            self.lower_task(task);
        }
    }

    fn lower_task(&mut self, task: &HirTask) {
        match &task.body {
            HirTaskBody::Pipeline(pipeline) => self.lower_pipeline(pipeline),
            HirTaskBody::Modal(modal) => {
                self.lower_pipeline(&modal.control);
                for (_name, mode_pipeline) in &modal.modes {
                    self.lower_pipeline(mode_pipeline);
                }
            }
        }
    }

    fn lower_pipeline(&mut self, pipeline: &HirPipeline) {
        for pipe in &pipeline.pipes {
            self.lower_pipe_expr(pipe);
        }
    }

    fn lower_pipe_expr(&mut self, pipe: &HirPipeExpr) {
        // Collect actor calls
        let mut calls: Vec<&HirActorCall> = Vec::new();

        if let HirPipeSource::ActorCall(call) = &pipe.source {
            calls.push(call);
        }
        for elem in &pipe.elements {
            if let HirPipeElem::ActorCall(call) = elem {
                calls.push(call);
            }
        }

        // For each actor call, populate concrete_actors
        for call in &calls {
            self.lower_actor_call(call);
        }

        // Insert widening nodes — match by CallId (handles define-expanded calls)
        for wp in &self.typed.widenings {
            if calls.iter().any(|c| c.call_id == wp.target_call_id) {
                let synthetic_name = format!("_widen_{}_to_{}", wp.from_type, wp.to_type);
                self.widening_nodes.push(WideningNode {
                    target_span: wp.target_span,
                    target_call_id: wp.target_call_id,
                    from_type: wp.from_type,
                    to_type: wp.to_type,
                    synthetic_name,
                });
            }
        }
    }

    fn lower_actor_call(&mut self, call: &HirActorCall) {
        // All HIR calls are actors (defines are expanded inline)
        let call_id = call.call_id;

        // Check if type_infer provided a monomorphized meta
        if let Some(mono) = self.typed.mono_actors.get(&call_id) {
            self.concrete_actors.insert(call_id, mono.clone());
            return;
        }

        // Use registry meta directly (already concrete for non-polymorphic actors)
        if let Some(meta) = self.registry.lookup(&call.name) {
            self.concrete_actors.insert(call_id, meta.clone());
        }
    }

    // ── Phase 2: L1-L5 verification ─────────────────────────────────────

    fn verify_obligations(&mut self) -> Cert {
        let l1 = self.verify_l1_type_consistency();
        let l2 = self.verify_l2_widening_safety();
        let l3 = self.verify_l3_rate_shape_preservation();
        let l4 = self.verify_l4_monomorphization_soundness();
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
    fn verify_l1_type_consistency(&mut self) -> bool {
        let mut ok = true;

        for task in &self.hir.tasks {
            if !self.verify_l1_task(task) {
                ok = false;
            }
        }

        ok
    }

    fn verify_l1_task(&mut self, task: &HirTask) -> bool {
        let mut ok = true;
        match &task.body {
            HirTaskBody::Pipeline(pipeline) => {
                if !self.verify_l1_pipeline(pipeline) {
                    ok = false;
                }
            }
            HirTaskBody::Modal(modal) => {
                if !self.verify_l1_pipeline(&modal.control) {
                    ok = false;
                }
                for (_name, mode_pipeline) in &modal.modes {
                    if !self.verify_l1_pipeline(mode_pipeline) {
                        ok = false;
                    }
                }
            }
        }
        ok
    }

    fn verify_l1_pipeline(&mut self, pipeline: &HirPipeline) -> bool {
        let mut ok = true;
        for pipe in &pipeline.pipes {
            if !self.verify_l1_pipe(pipe) {
                ok = false;
            }
        }
        ok
    }

    fn verify_l1_pipe(&mut self, pipe: &HirPipeExpr) -> bool {
        let mut ok = true;

        let mut calls: Vec<&HirActorCall> = Vec::new();
        if let HirPipeSource::ActorCall(call) = &pipe.source {
            calls.push(call);
        }
        for elem in &pipe.elements {
            if let HirPipeElem::ActorCall(call) = elem {
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

            // Check if a widening node covers this edge (match by CallId)
            let has_widening = self
                .widening_nodes
                .iter()
                .any(|w| w.target_call_id == tgt.call_id);

            if has_widening {
                continue;
            }

            if src_type != tgt_type {
                self.diagnostics.push(Diagnostic {
                    level: DiagLevel::Error,
                    span: tgt.call_span,
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
        let mut ok = true;

        for wn in &self.widening_nodes {
            if let Some(tgt_meta) = self.concrete_actors.get(&wn.target_call_id) {
                let in_count = &tgt_meta.in_count;
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
    fn verify_l4_monomorphization_soundness(&mut self) -> bool {
        let mut ok = true;

        for task in &self.hir.tasks {
            self.verify_l4_walk_task(task, &mut ok);
        }

        ok
    }

    fn verify_l4_walk_task(&mut self, task: &HirTask, ok: &mut bool) {
        match &task.body {
            HirTaskBody::Pipeline(pipeline) => self.verify_l4_walk_pipeline(pipeline, ok),
            HirTaskBody::Modal(modal) => {
                self.verify_l4_walk_pipeline(&modal.control, ok);
                for (_name, mode_pipeline) in &modal.modes {
                    self.verify_l4_walk_pipeline(mode_pipeline, ok);
                }
            }
        }
    }

    fn verify_l4_walk_pipeline(&mut self, pipeline: &HirPipeline, ok: &mut bool) {
        for pipe in &pipeline.pipes {
            if let HirPipeSource::ActorCall(call) = &pipe.source {
                self.verify_l4_call(call, ok);
            }
            for elem in &pipe.elements {
                if let HirPipeElem::ActorCall(call) = elem {
                    self.verify_l4_call(call, ok);
                }
            }
        }
    }

    fn verify_l4_call(&mut self, call: &HirActorCall, ok: &mut bool) {
        // All HIR calls are actors — no CallResolution check needed
        if let Some(reg_meta) = self.registry.lookup(&call.name) {
            if reg_meta.is_polymorphic() {
                let call_id = call.call_id;
                if let Some(concrete) = self.concrete_actors.get(&call_id) {
                    if concrete.is_polymorphic() {
                        self.diagnostics.push(Diagnostic {
                            level: DiagLevel::Error,
                            span: call.call_span,
                            message: format!(
                                "lowering verification failed (L4 monomorphization soundness): \
                                 polymorphic actor '{}' not fully monomorphized",
                                call.name
                            ),
                            hint: Some("specify type arguments explicitly".to_string()),
                        });
                        *ok = false;
                    }
                } else {
                    self.diagnostics.push(Diagnostic {
                        level: DiagLevel::Error,
                        span: call.call_span,
                        message: format!(
                            "lowering verification failed (L4 monomorphization soundness): \
                             polymorphic actor '{}' has no concrete instance",
                            call.name
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

        for (call_id, meta) in &self.concrete_actors {
            let span = self
                .resolved
                .call_spans
                .get(call_id)
                .or_else(|| self.hir.expanded_call_spans.get(call_id))
                .copied()
                .unwrap_or_else(|| {
                    use chumsky::span::Span as _;
                    Span::new((), 0..0)
                });
            if !meta.in_type.is_concrete() {
                self.diagnostics.push(Diagnostic {
                    level: DiagLevel::Error,
                    span,
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
                    span,
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

    fn get_output_type(&self, call: &HirActorCall) -> Option<PipitType> {
        self.concrete_actors
            .get(&call.call_id)
            .and_then(|m| m.out_type.as_concrete())
    }

    fn get_input_type(&self, call: &HirActorCall) -> Option<PipitType> {
        self.concrete_actors
            .get(&call.call_id)
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

    fn make_hir_actor_call(name: &str, call_id: CallId, call_span: Span) -> HirActorCall {
        HirActorCall {
            name: name.to_string(),
            call_id,
            call_span,
            args: Vec::new(),
            type_args: Vec::new(),
            shape_constraint: None,
        }
    }

    fn make_empty_hir() -> HirProgram {
        HirProgram {
            tasks: Vec::new(),
            consts: Vec::new(),
            params: Vec::new(),
            set_directives: Vec::new(),
            expanded_call_ids: HashMap::new(),
            expanded_call_spans: HashMap::new(),
            program_span: dummy_span(),
        }
    }

    fn make_hir_with_pipe(calls: Vec<HirActorCall>) -> HirProgram {
        use crate::hir::{HirPipeline, HirTask, HirTaskBody};
        use crate::id::TaskId;

        let source = calls.into_iter().next().unwrap();
        let elements: Vec<HirPipeElem> = Vec::new();

        // We'll build this properly for multi-call cases
        HirProgram {
            tasks: vec![HirTask {
                name: "main".to_string(),
                task_id: TaskId(0),
                freq_hz: 48000.0,
                freq_span: dummy_span(),
                body: HirTaskBody::Pipeline(HirPipeline {
                    pipes: vec![HirPipeExpr {
                        source: HirPipeSource::ActorCall(source),
                        elements,
                        sink: None,
                        span: span(0, 20),
                    }],
                    span: span(0, 20),
                }),
            }],
            consts: Vec::new(),
            params: Vec::new(),
            set_directives: Vec::new(),
            expanded_call_ids: HashMap::new(),
            expanded_call_spans: HashMap::new(),
            program_span: span(0, 20),
        }
    }

    fn make_hir_two_calls(src_call: HirActorCall, sink_call: HirActorCall) -> HirProgram {
        use crate::hir::{HirPipeline, HirTask, HirTaskBody};
        use crate::id::TaskId;

        HirProgram {
            tasks: vec![HirTask {
                name: "main".to_string(),
                task_id: TaskId(0),
                freq_hz: 48000.0,
                freq_span: dummy_span(),
                body: HirTaskBody::Pipeline(HirPipeline {
                    pipes: vec![HirPipeExpr {
                        source: HirPipeSource::ActorCall(src_call),
                        elements: vec![HirPipeElem::ActorCall(sink_call)],
                        sink: None,
                        span: span(0, 20),
                    }],
                    span: span(0, 20),
                }),
            }],
            consts: Vec::new(),
            params: Vec::new(),
            set_directives: Vec::new(),
            expanded_call_ids: HashMap::new(),
            expanded_call_spans: HashMap::new(),
            program_span: span(0, 20),
        }
    }

    fn make_resolved_for_calls(pairs: &[(Span, CallId)]) -> ResolvedProgram {
        let mut call_ids = HashMap::new();
        let mut call_spans = HashMap::new();
        for &(s, id) in pairs {
            call_ids.insert(s, id);
            call_spans.insert(id, s);
        }
        ResolvedProgram {
            consts: HashMap::new(),
            params: HashMap::new(),
            defines: HashMap::new(),
            tasks: HashMap::new(),
            buffers: HashMap::new(),
            call_resolutions: HashMap::new(),
            task_resolutions: HashMap::new(),
            probes: Vec::new(),
            call_ids,
            call_spans,
            def_ids: HashMap::new(),
            task_ids: HashMap::new(),
        }
    }

    fn make_empty_typed() -> TypedProgram {
        TypedProgram {
            type_assignments: HashMap::new(),
            widenings: Vec::new(),
            mono_actors: HashMap::new(),
        }
    }

    // ── L1 tests ────────────────────────────────────────────────────────

    #[test]
    fn l1_matching_types_pass() {
        let s1 = span(0, 10);
        let s2 = span(10, 20);
        let c1 = CallId(0);
        let c2 = CallId(1);

        let mut concrete_actors = HashMap::new();
        concrete_actors.insert(
            c1,
            make_concrete_meta("src", PipitType::Void, PipitType::Float),
        );
        concrete_actors.insert(
            c2,
            make_concrete_meta("sink", PipitType::Float, PipitType::Void),
        );

        let typed = make_empty_typed();
        let hir = make_hir_two_calls(
            make_hir_actor_call("src", c1, s1),
            make_hir_actor_call("sink", c2, s2),
        );
        let resolved = make_resolved_for_calls(&[(s1, c1), (s2, c2)]);
        let registry = Registry::empty();

        let mut engine = LowerEngine {
            hir: &hir,
            resolved: &resolved,
            typed: &typed,
            registry: &registry,
            concrete_actors,
            widening_nodes: Vec::new(),
            diagnostics: Vec::new(),
        };

        let cert = engine.verify_obligations();
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
        let c1 = CallId(0);
        let c2 = CallId(1);

        let mut concrete_actors = HashMap::new();
        concrete_actors.insert(
            c1,
            make_concrete_meta("src", PipitType::Void, PipitType::Float),
        );
        concrete_actors.insert(
            c2,
            make_concrete_meta("sink", PipitType::Double, PipitType::Void),
        );

        let typed = make_empty_typed();
        let hir = make_hir_two_calls(
            make_hir_actor_call("src", c1, s1),
            make_hir_actor_call("sink", c2, s2),
        );
        let resolved = make_resolved_for_calls(&[(s1, c1), (s2, c2)]);
        let registry = Registry::empty();

        let mut engine = LowerEngine {
            hir: &hir,
            resolved: &resolved,
            typed: &typed,
            registry: &registry,
            concrete_actors,
            widening_nodes: Vec::new(),
            diagnostics: Vec::new(),
        };

        let cert = engine.verify_obligations();
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
        let c1 = CallId(0);
        let c2 = CallId(1);

        let mut concrete_actors = HashMap::new();
        concrete_actors.insert(
            c1,
            make_concrete_meta("src", PipitType::Void, PipitType::Int32),
        );
        concrete_actors.insert(
            c2,
            make_concrete_meta("sink", PipitType::Float, PipitType::Void),
        );

        let typed = make_empty_typed();
        let hir = make_hir_two_calls(
            make_hir_actor_call("src", c1, s1),
            make_hir_actor_call("sink", c2, s2),
        );
        let resolved = make_resolved_for_calls(&[(s1, c1), (s2, c2)]);
        let registry = Registry::empty();

        // Widening node covers the edge (matched by CallId)
        let widening_nodes = vec![WideningNode {
            target_span: s2,
            target_call_id: c2,
            from_type: PipitType::Int32,
            to_type: PipitType::Float,
            synthetic_name: "_widen_int32_to_float".to_string(),
        }];

        let mut engine = LowerEngine {
            hir: &hir,
            resolved: &resolved,
            typed: &typed,
            registry: &registry,
            concrete_actors,
            widening_nodes,
            diagnostics: Vec::new(),
        };

        let cert = engine.verify_obligations();
        assert!(
            cert.l1_type_consistency,
            "L1 should pass when widening node covers the edge"
        );
    }

    // ── L2 tests ────────────────────────────────────────────────────────

    #[test]
    fn l2_valid_widening_passes() {
        let s1 = span(0, 10);
        let c1 = CallId(0);
        let concrete_actors = HashMap::new();
        let typed = make_empty_typed();
        let hir = make_empty_hir();
        let resolved = make_resolved_for_calls(&[]);
        let registry = Registry::empty();

        let widening_nodes = vec![WideningNode {
            target_span: s1,
            target_call_id: c1,
            from_type: PipitType::Int32,
            to_type: PipitType::Float,
            synthetic_name: "_widen_int32_to_float".to_string(),
        }];

        let mut engine = LowerEngine {
            hir: &hir,
            resolved: &resolved,
            typed: &typed,
            registry: &registry,
            concrete_actors,
            widening_nodes,
            diagnostics: Vec::new(),
        };

        let cert = engine.verify_obligations();
        assert!(cert.l2_widening_safety);
    }

    #[test]
    fn l2_cross_family_widening_fails() {
        let s1 = span(0, 10);
        let c1 = CallId(0);
        let concrete_actors = HashMap::new();
        let typed = make_empty_typed();
        let hir = make_empty_hir();
        let resolved = make_resolved_for_calls(&[]);
        let registry = Registry::empty();

        let widening_nodes = vec![WideningNode {
            target_span: s1,
            target_call_id: c1,
            from_type: PipitType::Float,
            to_type: PipitType::Cfloat,
            synthetic_name: "_widen_float_to_cfloat".to_string(),
        }];

        let mut engine = LowerEngine {
            hir: &hir,
            resolved: &resolved,
            typed: &typed,
            registry: &registry,
            concrete_actors,
            widening_nodes,
            diagnostics: Vec::new(),
        };

        let cert = engine.verify_obligations();
        assert!(!cert.l2_widening_safety);
        assert!(engine.diagnostics.iter().any(|d| d.message.contains("L2")));
    }

    // ── L4 tests ────────────────────────────────────────────────────────

    #[test]
    fn l4_monomorphized_passes() {
        let s1 = span(0, 10);
        let c1 = CallId(0);

        let mut concrete_actors = HashMap::new();
        concrete_actors.insert(
            c1,
            make_concrete_meta("scale", PipitType::Float, PipitType::Float),
        );

        let typed = make_empty_typed();
        let hir = make_hir_with_pipe(vec![make_hir_actor_call("scale", c1, s1)]);
        let resolved = make_resolved_for_calls(&[(s1, c1)]);

        let mut registry = Registry::empty();
        registry.insert(make_polymorphic_meta("scale"));

        let mut engine = LowerEngine {
            hir: &hir,
            resolved: &resolved,
            typed: &typed,
            registry: &registry,
            concrete_actors,
            widening_nodes: Vec::new(),
            diagnostics: Vec::new(),
        };

        let cert = engine.verify_obligations();
        assert!(cert.l4_monomorphization_soundness);
    }

    #[test]
    fn l4_unmonomorphized_fails() {
        let s1 = span(0, 10);
        let c1 = CallId(0);

        // Concrete actors map has the polymorphic meta (not monomorphized)
        let mut concrete_actors = HashMap::new();
        concrete_actors.insert(c1, make_polymorphic_meta("scale"));

        let typed = make_empty_typed();
        let hir = make_hir_with_pipe(vec![make_hir_actor_call("scale", c1, s1)]);
        let resolved = make_resolved_for_calls(&[(s1, c1)]);

        let mut registry = Registry::empty();
        registry.insert(make_polymorphic_meta("scale"));

        let mut engine = LowerEngine {
            hir: &hir,
            resolved: &resolved,
            typed: &typed,
            registry: &registry,
            concrete_actors,
            widening_nodes: Vec::new(),
            diagnostics: Vec::new(),
        };

        let cert = engine.verify_obligations();
        assert!(!cert.l4_monomorphization_soundness);
        assert!(engine.diagnostics.iter().any(|d| d.message.contains("L4")));
    }

    // ── L5 tests ────────────────────────────────────────────────────────

    #[test]
    fn l5_all_concrete_passes() {
        let s1 = span(0, 10);
        let c1 = CallId(0);

        let mut concrete_actors = HashMap::new();
        concrete_actors.insert(
            c1,
            make_concrete_meta("scale", PipitType::Float, PipitType::Float),
        );

        let typed = make_empty_typed();
        let hir = make_empty_hir();
        let resolved = make_resolved_for_calls(&[(s1, c1)]);
        let registry = Registry::empty();

        let mut engine = LowerEngine {
            hir: &hir,
            resolved: &resolved,
            typed: &typed,
            registry: &registry,
            concrete_actors,
            widening_nodes: Vec::new(),
            diagnostics: Vec::new(),
        };

        let cert = engine.verify_obligations();
        assert!(cert.l5_no_fallback_typing);
    }

    #[test]
    fn l5_unresolved_type_fails() {
        let s1 = span(0, 10);
        let c1 = CallId(0);

        let mut concrete_actors = HashMap::new();
        concrete_actors.insert(c1, make_polymorphic_meta("scale")); // TypeParam, not concrete

        let typed = make_empty_typed();
        let hir = make_empty_hir();
        let resolved = make_resolved_for_calls(&[(s1, c1)]);
        let registry = Registry::empty();

        let mut engine = LowerEngine {
            hir: &hir,
            resolved: &resolved,
            typed: &typed,
            registry: &registry,
            concrete_actors,
            widening_nodes: Vec::new(),
            diagnostics: Vec::new(),
        };

        let cert = engine.verify_obligations();
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
            target_call_id: CallId(0),
            from_type: PipitType::Int32,
            to_type: PipitType::Float,
            synthetic_name: format!("_widen_{}_to_{}", PipitType::Int32, PipitType::Float),
        };
        assert_eq!(wn.synthetic_name, "_widen_int32_to_float");
    }

    // ── Regression tests for Phase 2c ──────────────────────────────────

    fn lower_source(source: &str) -> LowerResult {
        use crate::registry::Registry;
        use crate::resolve::DiagLevel;
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
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
        let mut rr = crate::resolve::resolve(&program, &registry);
        assert!(
            rr.diagnostics.iter().all(|d| d.level != DiagLevel::Error),
            "resolve errors: {:#?}",
            rr.diagnostics
        );
        let hir = crate::hir::build_hir(&program, &rr.resolved, &mut rr.id_alloc);
        let tr = crate::type_infer::type_infer(&hir, &rr.resolved, &registry);
        lower_and_verify(&hir, &rr.resolved, &tr.typed, &registry)
    }

    #[test]
    fn define_expanded_calls_lowered() {
        // Define-expanded actor calls are visible to lower (all HIR calls are actors).
        // Verifies that concrete_actors is populated for expanded calls.
        let result = lower_source(
            r#"
            define amplify() { mul(2.0) }
            clock 1kHz t { constant(0.0) | amplify() | stdout() }
            "#,
        );
        assert!(
            result.cert.all_pass(),
            "cert should pass, diagnostics: {:#?}",
            result.diagnostics
        );
        // concrete_actors should include the mul from inside the define
        assert!(
            result.lowered.concrete_actors.len() >= 3,
            "expected at least 3 concrete actors (constant, mul, stdout), got {}",
            result.lowered.concrete_actors.len()
        );
    }
}
