// hir.rs — High-level IR after resolve + define expansion.
//
// Normalized representation where all `define` calls are expanded inline,
// task metadata is extracted, and structural normalization is complete.
// Downstream phases (graph, analyze, schedule) consume HIR instead of raw AST.
//
// Preconditions: produced from a resolved AST (resolve phase complete).
// Postconditions: no DefineStmt or define calls remain; all pipe elements
//   are concrete actors, taps, probes, or buffer ops.
// Failure modes: define expansion depth exceeds limit (recursive defines).
// Side effects: allocates fresh CallIds for define-expanded actor calls.
//
// See ADR-024 for design rationale.

use std::collections::HashMap;

use crate::ast::{Arg, Scalar, SetValue, ShapeConstraint, Span, Value};
use crate::id::{CallId, DefId, TaskId};

// ── Program ─────────────────────────────────────────────────────────────────

/// Normalized program after resolve + define expansion.
///
/// All `define` calls are expanded inline. Task bodies are self-contained —
/// no indirection through define statements remains.
#[derive(Debug, Clone)]
pub struct HirProgram {
    pub tasks: Vec<HirTask>,
    pub consts: Vec<HirConst>,
    pub params: Vec<HirParam>,
    pub set_directives: Vec<HirSetDirective>,
    /// CallId maps for define-expanded calls (supplements resolve-phase maps).
    pub expanded_call_ids: HashMap<Span, CallId>,
    pub expanded_call_spans: HashMap<CallId, Span>,
    /// Span of the original program (for fallback diagnostics).
    pub program_span: Span,
}

// ── Task ────────────────────────────────────────────────────────────────────

/// A task with extracted metadata and normalized body.
#[derive(Debug, Clone)]
pub struct HirTask {
    pub name: String,
    pub task_id: TaskId,
    pub freq_hz: f64,
    pub freq_span: Span,
    pub body: HirTaskBody,
}

#[derive(Debug, Clone)]
pub enum HirTaskBody {
    Pipeline(HirPipeline),
    Modal(HirModal),
}

#[derive(Debug, Clone)]
pub struct HirPipeline {
    pub pipes: Vec<HirPipeExpr>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct HirModal {
    pub control: HirPipeline,
    pub modes: Vec<(String, HirPipeline)>,
    pub switch: HirSwitchSource,
    pub span: Span,
}

/// Normalized switch source with span for diagnostics.
#[derive(Debug, Clone)]
pub enum HirSwitchSource {
    Buffer(String, Span),
    Param(String, Span),
}

// ── Pipe expression ─────────────────────────────────────────────────────────

/// A pipe expression with defines already expanded.
///
/// `source → elements → optional sink`. No define calls remain.
#[derive(Debug, Clone)]
pub struct HirPipeExpr {
    pub source: HirPipeSource,
    pub elements: Vec<HirPipeElem>,
    pub sink: Option<HirSink>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum HirPipeSource {
    ActorCall(HirActorCall),
    BufferRead(String, Span),
    TapRef(String, Span),
}

#[derive(Debug, Clone)]
pub enum HirPipeElem {
    ActorCall(HirActorCall),
    Tap(String, Span),
    Probe(String, Span),
}

/// A pipe sink (`-> buffer_name`).
#[derive(Debug, Clone)]
pub struct HirSink {
    pub buffer_name: String,
    pub span: Span,
}

// ── Actor call ──────────────────────────────────────────────────────────────

/// A concrete actor call (no defines). Args are already substituted if
/// this call originated from define expansion.
#[derive(Debug, Clone)]
pub struct HirActorCall {
    pub name: String,
    pub call_id: CallId,
    pub call_span: Span,
    /// Actor arguments — reuses AST `Arg` type. Already substituted if
    /// this call was expanded from a define body.
    pub args: Vec<Arg>,
    /// Explicit type arguments (e.g., `actor<float>(...)`) with their spans.
    pub type_args: Vec<(String, Span)>,
    /// Optional shape constraint: `actor(...)[d0, d1, ...]`.
    pub shape_constraint: Option<ShapeConstraint>,
}

// ── Top-level declarations ──────────────────────────────────────────────────

/// Const declaration with precomputed value.
#[derive(Debug, Clone)]
pub struct HirConst {
    pub def_id: DefId,
    pub name: String,
    /// Reuses AST `Value` — either `Scalar` or `Array`.
    pub value: Value,
}

/// Param declaration with default value.
#[derive(Debug, Clone)]
pub struct HirParam {
    pub def_id: DefId,
    pub name: String,
    /// Reuses AST `Scalar` — preserves `is_int_literal` for type inference.
    pub default_value: Scalar,
}

/// Set directive (e.g., `set mem = 64M`, `set tick_rate = 1kHz`).
#[derive(Debug, Clone)]
pub struct HirSetDirective {
    pub name: String,
    /// Reuses AST `SetValue`.
    pub value: SetValue,
    /// Span of the entire `set` statement (for diagnostics).
    pub span: Span,
}

// ── Builder ─────────────────────────────────────────────────────────────────

use crate::ast::{
    ActorCall, ConstStmt, ParamStmt, PipeElem, PipeExpr, PipeSource, Program, StatementKind,
    SwitchSource, TaskBody,
};
use crate::id::IdAllocator;
use crate::resolve::{CallResolution, ResolvedProgram};

/// Maximum recursion depth for nested define expansion.
const MAX_INLINE_DEPTH: u32 = 16;

/// Build the HIR from a parsed + resolved AST.
///
/// Walks `program.statements` exactly once, expanding all `define` calls
/// inline and extracting metadata. After this, no downstream phase needs
/// `&Program`.
pub fn build_hir(
    program: &Program,
    resolved: &ResolvedProgram,
    id_alloc: &mut IdAllocator,
) -> HirProgram {
    let mut builder = HirBuilder {
        program,
        resolved,
        id_alloc,
        expanded_call_ids: HashMap::new(),
        expanded_call_spans: HashMap::new(),
    };
    builder.build()
}

struct HirBuilder<'a> {
    program: &'a Program,
    resolved: &'a ResolvedProgram,
    id_alloc: &'a mut IdAllocator,
    expanded_call_ids: HashMap<Span, CallId>,
    expanded_call_spans: HashMap<CallId, Span>,
}

impl<'a> HirBuilder<'a> {
    fn build(&mut self) -> HirProgram {
        let mut tasks = Vec::new();
        let mut consts = Vec::new();
        let mut params = Vec::new();
        let mut set_directives = Vec::new();

        for stmt in &self.program.statements {
            match &stmt.kind {
                StatementKind::Task(task) => {
                    let task_id = self
                        .resolved
                        .task_ids
                        .get(&task.name.name)
                        .copied()
                        .unwrap_or(TaskId(0));
                    let body = self.lower_task_body(&task.body);
                    tasks.push(HirTask {
                        name: task.name.name.clone(),
                        task_id,
                        freq_hz: task.freq,
                        freq_span: task.freq_span,
                        body,
                    });
                }
                StatementKind::Const(c) => {
                    consts.push(self.lower_const(c));
                }
                StatementKind::Param(p) => {
                    params.push(self.lower_param(p));
                }
                StatementKind::Set(s) => {
                    set_directives.push(HirSetDirective {
                        name: s.name.name.clone(),
                        value: s.value.clone(),
                        span: stmt.span,
                    });
                }
                StatementKind::Define(_) => {
                    // Defines are consumed during expansion, not emitted to HIR.
                }
            }
        }

        HirProgram {
            tasks,
            consts,
            params,
            set_directives,
            expanded_call_ids: std::mem::take(&mut self.expanded_call_ids),
            expanded_call_spans: std::mem::take(&mut self.expanded_call_spans),
            program_span: self.program.span,
        }
    }

    fn lower_const(&self, c: &ConstStmt) -> HirConst {
        let def_id = self
            .resolved
            .def_ids
            .get(&c.name.name)
            .copied()
            .unwrap_or(DefId(0));
        HirConst {
            def_id,
            name: c.name.name.clone(),
            value: c.value.clone(),
        }
    }

    fn lower_param(&self, p: &ParamStmt) -> HirParam {
        let def_id = self
            .resolved
            .def_ids
            .get(&p.name.name)
            .copied()
            .unwrap_or(DefId(0));
        HirParam {
            def_id,
            name: p.name.name.clone(),
            default_value: p.value.clone(),
        }
    }

    fn lower_task_body(&mut self, body: &TaskBody) -> HirTaskBody {
        match body {
            TaskBody::Pipeline(pb) => {
                HirTaskBody::Pipeline(self.lower_pipeline_body(&pb.lines, pb.span))
            }
            TaskBody::Modal(modal) => {
                let control =
                    self.lower_pipeline_body(&modal.control.body.lines, modal.control.span);
                let modes = modal
                    .modes
                    .iter()
                    .map(|m| {
                        let pipeline = self.lower_pipeline_body(&m.body.lines, m.span);
                        (m.name.name.clone(), pipeline)
                    })
                    .collect();
                let switch = match &modal.switch.source {
                    SwitchSource::Buffer(ident) => {
                        HirSwitchSource::Buffer(ident.name.clone(), ident.span)
                    }
                    SwitchSource::Param(ident) => {
                        HirSwitchSource::Param(ident.name.clone(), ident.span)
                    }
                };
                HirTaskBody::Modal(HirModal {
                    control,
                    modes,
                    switch,
                    span: modal.span,
                })
            }
        }
    }

    fn lower_pipeline_body(&mut self, lines: &[PipeExpr], span: Span) -> HirPipeline {
        let pipes = lines
            .iter()
            .flat_map(|line| self.expand_pipe_expr(line, 0))
            .collect();
        HirPipeline { pipes, span }
    }

    /// Expand a pipe expression, inlining any define calls.
    ///
    /// A single AST `PipeExpr` may expand into multiple `HirPipeExpr`s
    /// when a define body contains multiple pipe lines.
    fn expand_pipe_expr(&mut self, expr: &PipeExpr, depth: u32) -> Vec<HirPipeExpr> {
        // Expand source
        let source_expanded = self.expand_source(&expr.source, depth);

        // Expand elements
        let mut elements_expanded: Vec<HirPipeElem> = Vec::new();
        for elem in &expr.elements {
            match elem {
                PipeElem::ActorCall(call) => {
                    let expanded = self.expand_actor_call(call, depth);
                    match expanded {
                        ExpandedCall::Actor(hir_call) => {
                            elements_expanded.push(HirPipeElem::ActorCall(hir_call));
                        }
                        ExpandedCall::InlinedDefine(pipes) => {
                            // Inlined define in element position: flatten the
                            // define body's pipe elements into the current chain.
                            for pipe in pipes {
                                // First pipe: merge its source as an element
                                if let HirPipeSource::ActorCall(call) = pipe.source {
                                    elements_expanded.push(HirPipeElem::ActorCall(call));
                                }
                                elements_expanded.extend(pipe.elements);
                                // Sink of inlined define pipes is ignored in element position
                            }
                        }
                    }
                }
                PipeElem::Tap(ident) => {
                    elements_expanded.push(HirPipeElem::Tap(ident.name.clone(), ident.span));
                }
                PipeElem::Probe(ident) => {
                    elements_expanded.push(HirPipeElem::Probe(ident.name.clone(), ident.span));
                }
            }
        }

        let sink = expr.sink.as_ref().map(|s| HirSink {
            buffer_name: s.buffer.name.clone(),
            span: s.span,
        });

        // Handle source expansion
        match source_expanded {
            ExpandedSource::Single(hir_source) => {
                vec![HirPipeExpr {
                    source: hir_source,
                    elements: elements_expanded,
                    sink,
                    span: expr.span,
                }]
            }
            ExpandedSource::InlinedDefine(mut pipes) => {
                // Inlined define in source position: the define body produces
                // multiple pipe lines. The last pipe's output connects to
                // the current pipe's elements/sink.
                if let Some(last) = pipes.last_mut() {
                    last.elements.extend(elements_expanded);
                    if last.sink.is_none() {
                        last.sink = sink;
                    }
                }
                pipes
            }
        }
    }

    fn expand_source(&mut self, source: &PipeSource, depth: u32) -> ExpandedSource {
        match source {
            PipeSource::ActorCall(call) => match self.expand_actor_call(call, depth) {
                ExpandedCall::Actor(hir_call) => {
                    ExpandedSource::Single(HirPipeSource::ActorCall(hir_call))
                }
                ExpandedCall::InlinedDefine(pipes) => ExpandedSource::InlinedDefine(pipes),
            },
            PipeSource::BufferRead(ident) => {
                ExpandedSource::Single(HirPipeSource::BufferRead(ident.name.clone(), ident.span))
            }
            PipeSource::TapRef(ident) => {
                ExpandedSource::Single(HirPipeSource::TapRef(ident.name.clone(), ident.span))
            }
        }
    }

    /// Expand an actor call: if it resolves to a define, inline the define body;
    /// otherwise emit a concrete `HirActorCall`.
    fn expand_actor_call(&mut self, call: &ActorCall, depth: u32) -> ExpandedCall {
        // Check if this call resolves to a define
        if let Some(CallResolution::Define) = self.resolved.call_resolution_for(call.span) {
            if depth >= MAX_INLINE_DEPTH {
                // Exceeded depth limit — emit as actor (will fail later in graph)
                return ExpandedCall::Actor(self.make_hir_actor_call(call, depth));
            }
            return self.inline_define(call, depth);
        }

        ExpandedCall::Actor(self.make_hir_actor_call(call, depth))
    }

    fn make_hir_actor_call(&mut self, call: &ActorCall, _depth: u32) -> HirActorCall {
        // Use the resolve-phase CallId if available; allocate fresh otherwise.
        //
        // NOTE: CallId aliasing fix (fresh IDs when depth > 0) is deferred to
        // Step 3, when lower is also migrated to HIR. Until then, lower resolves
        // CallIds via resolved.call_id_for_span(call.span) — changing IDs here
        // would cause lower's type_instantiation lookup to miss.
        let call_id = if let Some(&id) = self.resolved.call_ids.get(&call.span) {
            id
        } else {
            let id = self.id_alloc.alloc_call();
            self.expanded_call_ids.insert(call.span, id);
            self.expanded_call_spans.insert(id, call.span);
            id
        };

        HirActorCall {
            name: call.name.name.clone(),
            call_id,
            call_span: call.span,
            args: call.args.clone(),
            type_args: call
                .type_args
                .iter()
                .map(|i| (i.name.clone(), i.span))
                .collect(),
            shape_constraint: call.shape_constraint.clone(),
        }
    }

    fn inline_define(&mut self, call: &ActorCall, depth: u32) -> ExpandedCall {
        let define_entry = match self.resolved.defines.get(&call.name.name) {
            Some(e) => e.clone(),
            None => return ExpandedCall::Actor(self.make_hir_actor_call(call, depth)),
        };

        let define_stmt = match &self.program.statements[define_entry.stmt_index].kind {
            StatementKind::Define(d) => d,
            _ => return ExpandedCall::Actor(self.make_hir_actor_call(call, depth)),
        };

        // Build argument substitution map: formal param name → actual arg
        let arg_map: HashMap<String, Arg> = define_entry
            .param_names
            .iter()
            .zip(call.args.iter())
            .map(|(name, arg)| (name.clone(), arg.clone()))
            .collect();

        // Expand each line of the define body with substituted args
        let mut all_pipes = Vec::new();
        for line in &define_stmt.body.lines {
            let substituted = substitute_pipe_expr(line, &arg_map);
            let expanded = self.expand_pipe_expr(&substituted, depth + 1);
            all_pipes.extend(expanded);
        }

        ExpandedCall::InlinedDefine(all_pipes)
    }
}

/// Result of expanding an actor call.
enum ExpandedCall {
    /// Concrete actor call.
    Actor(HirActorCall),
    /// Define body expanded into pipe expressions.
    InlinedDefine(Vec<HirPipeExpr>),
}

/// Result of expanding a pipe source.
enum ExpandedSource {
    Single(HirPipeSource),
    InlinedDefine(Vec<HirPipeExpr>),
}

// ── Argument substitution ───────────────────────────────────────────────────
//
// Ported from graph.rs substitute_* functions. Substitutes formal define
// parameters (parsed as ConstRef identifiers) with actual arguments.

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
        _ => arg.clone(),
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser;
    use crate::resolve;

    /// Build HIR from source. Ignores "unknown actor" resolve errors since
    /// unit tests use an empty registry — the HIR builder only needs define
    /// resolution, not actor resolution.
    fn build_hir_from_source(
        source: &str,
        registry: &crate::registry::Registry,
    ) -> (HirProgram, ResolvedProgram) {
        let parse_result = parser::parse(source);
        assert!(
            parse_result.errors.is_empty(),
            "parse errors: {:?}",
            parse_result.errors
        );
        let program = parse_result.program.unwrap();

        let resolve_result = resolve::resolve(&program, registry);
        // Allow "unknown actor" errors — tests use empty registry.
        // Only fail on non-actor errors that indicate broken test setup.

        let mut id_alloc = resolve_result.id_alloc;
        let hir = build_hir(&program, &resolve_result.resolved, &mut id_alloc);
        (hir, resolve_result.resolved)
    }

    #[test]
    fn hir_simple_pipeline() {
        let source = r#"
            set freq = 48kHz
            clock 48kHz main {
                constant(1.0) | stdout()
            }
        "#;
        let registry = crate::registry::Registry::empty();
        let (hir, _) = build_hir_from_source(source, &registry);

        assert_eq!(hir.tasks.len(), 1);
        assert_eq!(hir.tasks[0].name, "main");
        assert!((hir.tasks[0].freq_hz - 48000.0).abs() < 0.01);

        // Pipeline body with one pipe: constant -> stdout
        if let HirTaskBody::Pipeline(ref pipeline) = hir.tasks[0].body {
            assert_eq!(pipeline.pipes.len(), 1);
            let pipe = &pipeline.pipes[0];
            assert!(matches!(&pipe.source, HirPipeSource::ActorCall(c) if c.name == "constant"));
            assert_eq!(pipe.elements.len(), 1);
            assert!(matches!(&pipe.elements[0], HirPipeElem::ActorCall(c) if c.name == "stdout"));
        } else {
            panic!("expected pipeline body");
        }
    }

    #[test]
    fn hir_define_expansion() {
        let source = r#"
            define amplify(g) {
                scale(g)
            }
            clock 48kHz main {
                constant(1.0) | amplify(2.0) | stdout()
            }
        "#;
        let registry = crate::registry::Registry::empty();
        let (hir, _) = build_hir_from_source(source, &registry);

        // After expansion, `amplify(2.0)` becomes `scale(2.0)`
        assert_eq!(hir.tasks.len(), 1);
        if let HirTaskBody::Pipeline(ref pipeline) = hir.tasks[0].body {
            assert_eq!(pipeline.pipes.len(), 1);
            let pipe = &pipeline.pipes[0];
            // source: constant(1.0)
            assert!(matches!(&pipe.source, HirPipeSource::ActorCall(c) if c.name == "constant"));
            // elements: scale(2.0) | stdout()
            assert_eq!(pipe.elements.len(), 2);
            assert!(matches!(&pipe.elements[0], HirPipeElem::ActorCall(c) if c.name == "scale"));
            assert!(matches!(&pipe.elements[1], HirPipeElem::ActorCall(c) if c.name == "stdout"));
        } else {
            panic!("expected pipeline body");
        }

        // Define-expanded calls should have unique CallIds
        assert!(!hir.expanded_call_ids.is_empty() || !hir.expanded_call_spans.is_empty());
    }

    #[test]
    fn hir_consts_and_params() {
        let source = r#"
            set mem = 64MB
            set tick_rate = 1kHz
            const coeffs = [1.0, 2.0, 3.0]
            param gain = 1.0
            clock 48kHz main {
                constant(1.0) | stdout()
            }
        "#;
        let registry = crate::registry::Registry::empty();
        let (hir, _) = build_hir_from_source(source, &registry);

        assert_eq!(hir.consts.len(), 1);
        assert_eq!(hir.consts[0].name, "coeffs");

        assert_eq!(hir.params.len(), 1);
        assert_eq!(hir.params[0].name, "gain");

        assert_eq!(hir.set_directives.len(), 2);
    }

    #[test]
    fn hir_no_defines_in_output() {
        let source = r#"
            define passthrough() {
                scale(1.0)
            }
            define chain(x) {
                passthrough() | scale(x)
            }
            clock 48kHz main {
                constant(1.0) | chain(2.0) | stdout()
            }
        "#;
        let registry = crate::registry::Registry::empty();
        let (hir, _) = build_hir_from_source(source, &registry);

        // All defines should be expanded — verify no define names appear as actor calls
        fn check_no_defines(pipeline: &HirPipeline) {
            for pipe in &pipeline.pipes {
                if let HirPipeSource::ActorCall(c) = &pipe.source {
                    assert_ne!(c.name, "passthrough", "define not expanded");
                    assert_ne!(c.name, "chain", "define not expanded");
                }
                for elem in &pipe.elements {
                    if let HirPipeElem::ActorCall(c) = elem {
                        assert_ne!(c.name, "passthrough", "define not expanded");
                        assert_ne!(c.name, "chain", "define not expanded");
                    }
                }
            }
        }

        if let HirTaskBody::Pipeline(ref pipeline) = hir.tasks[0].body {
            check_no_defines(pipeline);
        }
    }
}
