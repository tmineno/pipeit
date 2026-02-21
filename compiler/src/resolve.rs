// resolve.rs — Name resolution for Pipit AST
//
// Walks the parsed AST, resolves all name references against global symbol
// tables and the actor registry, and reports diagnostics for unknown or
// duplicate names.
//
// Preconditions: `program` is a well-formed AST from the parser.
//                `registry` is populated with actor metadata from C++ headers.
// Postconditions: returns resolution tables plus all accumulated diagnostics.
// Failure modes: unknown names, duplicate definitions, constraint violations
//                produce `Diagnostic` entries. Resolution continues past errors.
// Side effects: none.

use std::collections::HashMap;
use std::fmt;

use crate::ast::*;
use crate::id::{CallId, DefId, IdAllocator, TaskId};
use crate::registry::Registry;

// ── Public types ────────────────────────────────────────────────────────────

/// Result of name resolution.
#[derive(Debug)]
pub struct ResolveResult {
    pub resolved: ResolvedProgram,
    pub diagnostics: Vec<Diagnostic>,
}

/// Resolution tables produced by name resolution.
/// Downstream phases use these alongside the original AST.
#[derive(Debug)]
pub struct ResolvedProgram {
    pub consts: HashMap<String, ConstEntry>,
    pub params: HashMap<String, ParamEntry>,
    pub defines: HashMap<String, DefineEntry>,
    pub tasks: HashMap<String, TaskEntry>,
    pub buffers: HashMap<String, BufferInfo>,
    pub call_resolutions: HashMap<Span, CallResolution>,
    pub task_resolutions: HashMap<String, TaskResolution>,
    pub probes: Vec<ProbeEntry>,

    // ── Stable IDs (ADR-021) ──────────────────────────────────────────────
    /// Span → CallId lookup for actor call sites.
    pub call_ids: HashMap<Span, CallId>,
    /// CallId → Span reverse lookup.
    pub call_spans: HashMap<CallId, Span>,
    /// Name → DefId for consts, params, and defines.
    pub def_ids: HashMap<String, DefId>,
    /// Task name → TaskId.
    pub task_ids: HashMap<String, TaskId>,
}

impl ResolvedProgram {
    /// Look up the CallId assigned to a call site span.
    pub fn call_id_for_span(&self, span: Span) -> CallId {
        *self
            .call_ids
            .get(&span)
            .expect("internal: no CallId for span")
    }
}

#[derive(Debug, Clone)]
pub struct ConstEntry {
    pub stmt_index: usize,
    pub name_span: Span,
}

#[derive(Debug, Clone)]
pub struct ParamEntry {
    pub stmt_index: usize,
    pub name_span: Span,
}

#[derive(Debug, Clone)]
pub struct DefineEntry {
    pub stmt_index: usize,
    pub name_span: Span,
    pub param_names: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct TaskEntry {
    pub stmt_index: usize,
    pub name_span: Span,
}

#[derive(Debug, Clone)]
pub struct BufferInfo {
    pub writer_task: String,
    pub writer_span: Span,
    pub readers: Vec<(String, Span)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallResolution {
    Actor,
    Define,
}

#[derive(Debug, Clone)]
pub struct TaskResolution {
    pub taps: HashMap<String, TapInfo>,
    pub modes: HashMap<String, Span>,
}

#[derive(Debug, Clone)]
pub struct TapInfo {
    pub decl_span: Span,
    pub consumed: bool,
}

#[derive(Debug, Clone)]
pub struct ProbeEntry {
    pub name: String,
    pub span: Span,
    pub context: String,
}

/// A name resolution diagnostic.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub level: DiagLevel,
    pub span: Span,
    pub message: String,
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagLevel {
    Error,
    Warning,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let level = match self.level {
            DiagLevel::Error => "error",
            DiagLevel::Warning => "warning",
        };
        write!(f, "{}: {}", level, self.message)?;
        if let Some(hint) = &self.hint {
            write!(f, "\n  hint: {}", hint)?;
        }
        Ok(())
    }
}

// ── Public entry point ──────────────────────────────────────────────────────

/// Resolve all names in a parsed Pipit program.
pub fn resolve(program: &Program, registry: &Registry) -> ResolveResult {
    let mut ctx = ResolveCtx::new(registry);

    // Pass 1: collect global definitions
    ctx.collect_globals(program);

    // Pass 2: resolve references in define and task bodies
    ctx.resolve_bodies(program);

    // Post-pass: validate buffer readers and tap consumption
    ctx.validate_buffers();
    ctx.validate_taps();

    ResolveResult {
        resolved: ctx.resolved,
        diagnostics: ctx.diagnostics,
    }
}

// ── Internal context ────────────────────────────────────────────────────────

struct PendingTapRef {
    tap_name: String,
    scope: String,
    span: Span,
    context_name: String,
}

struct ResolveCtx<'a> {
    registry: &'a Registry,
    resolved: ResolvedProgram,
    diagnostics: Vec<Diagnostic>,
    /// Pending buffer reads to validate after all tasks processed.
    pending_buffer_reads: Vec<(String, String, Span)>, // (buffer_name, task_name, span)
    /// Pending tap-ref consumptions (from Arg::TapRef) that may be forward references.
    pending_tap_refs: Vec<PendingTapRef>,
    /// Stable ID allocator (ADR-021).
    id_alloc: IdAllocator,
}

impl<'a> ResolveCtx<'a> {
    fn new(registry: &'a Registry) -> Self {
        ResolveCtx {
            registry,
            resolved: ResolvedProgram {
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
            },
            diagnostics: Vec::new(),
            pending_buffer_reads: Vec::new(),
            pending_tap_refs: Vec::new(),
            id_alloc: IdAllocator::new(),
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

    fn warning(&mut self, span: Span, message: String) {
        self.diagnostics.push(Diagnostic {
            level: DiagLevel::Warning,
            span,
            message,
            hint: None,
        });
    }

    // ── Pass 1: collect globals ─────────────────────────────────────────

    fn collect_globals(&mut self, program: &Program) {
        for (i, stmt) in program.statements.iter().enumerate() {
            match &stmt.kind {
                StatementKind::Const(c) => {
                    let name = &c.name.name;
                    if let Some(existing) = self.resolved.consts.get(name) {
                        self.error(
                            c.name.span,
                            format!(
                                "duplicate const '{}' (first defined at offset {})",
                                name, existing.name_span.start
                            ),
                        );
                    } else {
                        let def_id = self.id_alloc.alloc_def();
                        self.resolved.def_ids.insert(name.clone(), def_id);
                        self.resolved.consts.insert(
                            name.clone(),
                            ConstEntry {
                                stmt_index: i,
                                name_span: c.name.span,
                            },
                        );
                    }
                }
                StatementKind::Param(p) => {
                    let name = &p.name.name;
                    if let Some(existing) = self.resolved.params.get(name) {
                        self.error(
                            p.name.span,
                            format!(
                                "duplicate param '{}' (first defined at offset {})",
                                name, existing.name_span.start
                            ),
                        );
                    } else {
                        let def_id = self.id_alloc.alloc_def();
                        self.resolved.def_ids.insert(name.clone(), def_id);
                        self.resolved.params.insert(
                            name.clone(),
                            ParamEntry {
                                stmt_index: i,
                                name_span: p.name.span,
                            },
                        );
                    }
                }
                StatementKind::Define(d) => {
                    let name = &d.name.name;
                    if let Some(existing) = self.resolved.defines.get(name) {
                        self.error(
                            d.name.span,
                            format!(
                                "duplicate define '{}' (first defined at offset {})",
                                name, existing.name_span.start
                            ),
                        );
                    } else {
                        let def_id = self.id_alloc.alloc_def();
                        self.resolved.def_ids.insert(name.clone(), def_id);
                        self.resolved.defines.insert(
                            name.clone(),
                            DefineEntry {
                                stmt_index: i,
                                name_span: d.name.span,
                                param_names: d.params.iter().map(|p| p.name.clone()).collect(),
                            },
                        );
                    }
                }
                StatementKind::Task(t) => {
                    let name = &t.name.name;
                    if let Some(existing) = self.resolved.tasks.get(name) {
                        self.error(
                            t.name.span,
                            format!(
                                "duplicate task '{}' (first defined at offset {})",
                                name, existing.name_span.start
                            ),
                        );
                    } else {
                        let task_id = self.id_alloc.alloc_task();
                        self.resolved.task_ids.insert(name.clone(), task_id);
                        self.resolved.tasks.insert(
                            name.clone(),
                            TaskEntry {
                                stmt_index: i,
                                name_span: t.name.span,
                            },
                        );
                    }
                }
                StatementKind::Set(_) => {}
            }
        }

        // Cross-namespace collision checks — collect errors then emit
        let mut collision_errors: Vec<(Span, String)> = Vec::new();

        for (name, const_entry) in &self.resolved.consts {
            if let Some(param_entry) = self.resolved.params.get(name) {
                collision_errors.push((
                    param_entry.name_span,
                    format!(
                        "'{}' is defined as both a const (offset {}) and a param",
                        name, const_entry.name_span.start
                    ),
                ));
            }
        }

        for (name, define_entry) in &self.resolved.defines {
            if self.resolved.consts.contains_key(name) {
                collision_errors.push((
                    define_entry.name_span,
                    format!("'{}' is defined as both a const and a define", name),
                ));
            }
            if self.resolved.params.contains_key(name) {
                collision_errors.push((
                    define_entry.name_span,
                    format!("'{}' is defined as both a param and a define", name),
                ));
            }
        }

        for (span, message) in collision_errors {
            self.error(span, message);
        }
    }

    // ── Pass 2: resolve references ──────────────────────────────────────

    fn resolve_bodies(&mut self, program: &Program) {
        for stmt in &program.statements {
            match &stmt.kind {
                StatementKind::Define(d) => {
                    let formal_params: Vec<String> =
                        d.params.iter().map(|p| p.name.clone()).collect();
                    let scope = Scope::Define {
                        name: d.name.name.clone(),
                        formal_params,
                    };
                    let mut taps = HashMap::new();
                    self.resolve_pipeline_body(&d.body, &scope, &mut taps);
                    self.resolve_pending_tap_refs_for(&d.name.name, &mut taps);
                    // Check unused taps in define
                    for (tap_name, info) in &taps {
                        if !info.consumed {
                            self.error(
                                info.decl_span,
                                format!(
                                    "tap ':{tap_name}' declared but never consumed in define '{}'",
                                    d.name.name
                                ),
                            );
                        }
                    }
                }
                StatementKind::Task(t) => {
                    let task_name = t.name.name.clone();
                    let scope = Scope::Task {
                        name: task_name.clone(),
                    };

                    match &t.body {
                        TaskBody::Pipeline(body) => {
                            let mut taps = HashMap::new();
                            self.resolve_pipeline_body(body, &scope, &mut taps);
                            self.resolve_pending_tap_refs_for(&task_name, &mut taps);
                            self.resolved.task_resolutions.insert(
                                task_name,
                                TaskResolution {
                                    taps,
                                    modes: HashMap::new(),
                                },
                            );
                        }
                        TaskBody::Modal(modal) => {
                            let mut taps = HashMap::new();
                            let mut modes: HashMap<String, Span> = HashMap::new();

                            // Control block
                            self.resolve_pipeline_body(&modal.control.body, &scope, &mut taps);

                            // Collect buffers written in control block for switch validation
                            let control_buffers: Vec<String> = modal
                                .control
                                .body
                                .lines
                                .iter()
                                .filter_map(|line| line.sink.as_ref())
                                .map(|sink| sink.buffer.name.clone())
                                .collect();

                            // Mode blocks
                            for mode in &modal.modes {
                                if let Some(existing_span) = modes.get(&mode.name.name) {
                                    self.error(
                                        mode.name.span,
                                        format!(
                                            "duplicate mode '{}' in task '{}' (first at offset {})",
                                            mode.name.name, t.name.name, existing_span.start
                                        ),
                                    );
                                } else {
                                    modes.insert(mode.name.name.clone(), mode.name.span);
                                }
                                self.resolve_pipeline_body(&mode.body, &scope, &mut taps);
                            }

                            // Switch validation
                            self.validate_switch(
                                &modal.switch,
                                &t.name.name,
                                &modes,
                                &control_buffers,
                            );

                            self.resolve_pending_tap_refs_for(&task_name, &mut taps);
                            self.resolved
                                .task_resolutions
                                .insert(task_name, TaskResolution { taps, modes });
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn resolve_pipeline_body(
        &mut self,
        body: &PipelineBody,
        scope: &Scope,
        taps: &mut HashMap<String, TapInfo>,
    ) {
        let task_name = scope.context_name();

        for line in &body.lines {
            // Source
            match &line.source {
                PipeSource::ActorCall(call) => {
                    self.resolve_actor_call(call, scope, taps);
                }
                PipeSource::BufferRead(ident) => {
                    self.pending_buffer_reads.push((
                        ident.name.clone(),
                        task_name.clone(),
                        ident.span,
                    ));
                }
                PipeSource::TapRef(ident) => {
                    if let Some(info) = taps.get_mut(&ident.name) {
                        info.consumed = true;
                    } else {
                        self.error(
                            ident.span,
                            format!(
                                "undefined tap ':{name}' in {ctx}",
                                name = ident.name,
                                ctx = scope.description()
                            ),
                        );
                    }
                }
            }

            // Elements
            for elem in &line.elements {
                match elem {
                    PipeElem::ActorCall(call) => {
                        self.resolve_actor_call(call, scope, taps);
                    }
                    PipeElem::Tap(ident) => {
                        if taps.contains_key(&ident.name) {
                            self.error(
                                ident.span,
                                format!(
                                    "duplicate tap ':{name}' in {ctx}",
                                    name = ident.name,
                                    ctx = scope.description()
                                ),
                            );
                        } else {
                            taps.insert(
                                ident.name.clone(),
                                TapInfo {
                                    decl_span: ident.span,
                                    consumed: false,
                                },
                            );
                        }
                    }
                    PipeElem::Probe(ident) => {
                        self.resolved.probes.push(ProbeEntry {
                            name: ident.name.clone(),
                            span: ident.span,
                            context: task_name.clone(),
                        });
                    }
                }
            }

            // Sink
            if let Some(sink) = &line.sink {
                let buf_name = &sink.buffer.name;
                if let Some(existing) = self.resolved.buffers.get(buf_name) {
                    if existing.writer_task != task_name {
                        self.error(
                            sink.buffer.span,
                            format!(
                                "multiple writers to shared buffer '{}': first written by task '{}' (offset {})",
                                buf_name, existing.writer_task, existing.writer_span.start
                            ),
                        );
                    }
                    // Same task writing same buffer from multiple lines is OK
                } else {
                    self.resolved.buffers.insert(
                        buf_name.clone(),
                        BufferInfo {
                            writer_task: task_name.clone(),
                            writer_span: sink.buffer.span,
                            readers: Vec::new(),
                        },
                    );
                }
            }
        }
    }

    fn resolve_actor_call(
        &mut self,
        call: &ActorCall,
        scope: &Scope,
        taps: &mut HashMap<String, TapInfo>,
    ) {
        let name = &call.name.name;
        self.resolve_call_target(call, name);
        for arg in &call.args {
            self.resolve_call_arg(arg, scope, taps);
        }

        // Validate shape constraint dimensions (v0.2.0)
        if let Some(sc) = &call.shape_constraint {
            self.validate_shape_constraint(sc, scope);
        }
    }

    fn resolve_call_target(&mut self, call: &ActorCall, name: &str) {
        let is_define = self.resolved.defines.contains_key(name);
        let actor_meta = self.registry.lookup(name);

        if is_define {
            let call_id = self.id_alloc.alloc_call();
            self.resolved.call_ids.insert(call.span, call_id);
            self.resolved.call_spans.insert(call_id, call.span);
            self.resolved
                .call_resolutions
                .insert(call.span, CallResolution::Define);
            if actor_meta.is_some() {
                self.warning(
                    call.name.span,
                    format!("define '{}' shadows actor with the same name", name),
                );
            }
            return;
        }

        if let Some(meta) = actor_meta {
            let call_id = self.id_alloc.alloc_call();
            self.resolved.call_ids.insert(call.span, call_id);
            self.resolved.call_spans.insert(call_id, call.span);
            self.resolved
                .call_resolutions
                .insert(call.span, CallResolution::Actor);
            self.validate_actor_type_args(call, name, meta.type_params.len());
            return;
        }

        self.diagnostics.push(Diagnostic {
            level: DiagLevel::Error,
            span: call.name.span,
            message: format!("unknown actor or define '{}'", name),
            hint: Some("check actor header includes (-I flag)".to_string()),
        });
    }

    fn validate_actor_type_args(&mut self, call: &ActorCall, actor_name: &str, expected: usize) {
        if call.type_args.is_empty() {
            return;
        }
        if expected == 0 {
            self.error(
                call.name.span,
                format!(
                    "actor '{}' is not polymorphic but was called with type arguments",
                    actor_name
                ),
            );
            return;
        }
        if call.type_args.len() != expected {
            self.error(
                call.name.span,
                format!(
                    "actor '{}' expects {} type argument(s), found {}",
                    actor_name,
                    expected,
                    call.type_args.len()
                ),
            );
        }
    }

    fn resolve_call_arg(&mut self, arg: &Arg, scope: &Scope, taps: &mut HashMap<String, TapInfo>) {
        match arg {
            Arg::ParamRef(ident) => {
                if !self.resolved.params.contains_key(&ident.name) {
                    self.error(ident.span, format!("undefined param '${}'", ident.name));
                }
            }
            Arg::ConstRef(ident) => {
                if !scope_has_formal_param(scope, &ident.name)
                    && !self.resolved.consts.contains_key(&ident.name)
                {
                    self.error(ident.span, format!("undefined const '{}'", ident.name));
                }
            }
            Arg::Value(_) => {}
            Arg::TapRef(ident) => {
                if let Some(info) = taps.get_mut(&ident.name) {
                    info.consumed = true;
                } else {
                    self.pending_tap_refs.push(PendingTapRef {
                        tap_name: ident.name.clone(),
                        scope: scope.description(),
                        span: ident.span,
                        context_name: scope.context_name(),
                    });
                }
            }
        }
    }

    /// Validate that all dimensions in a shape constraint are compile-time
    /// constants (integer literals or const references). Runtime params are
    /// forbidden in shape constraints.
    fn validate_shape_constraint(
        &mut self,
        constraint: &crate::ast::ShapeConstraint,
        scope: &Scope,
    ) {
        for dim in &constraint.dims {
            if let crate::ast::ShapeDim::ConstRef(ident) = dim {
                if !scope_has_formal_param(scope, &ident.name) {
                    if self.resolved.params.contains_key(&ident.name) {
                        self.diagnostics.push(Diagnostic {
                            level: DiagLevel::Error,
                            span: ident.span,
                            message: format!(
                                "runtime param '${}' cannot be used as frame dimension",
                                ident.name
                            ),
                            hint: Some("use const or literal for shape constraints".to_string()),
                        });
                    } else if !self.resolved.consts.contains_key(&ident.name) {
                        self.error(
                            ident.span,
                            format!("unknown name '{}' in shape constraint", ident.name),
                        );
                    }
                }
            }
        }
    }

    fn validate_switch(
        &mut self,
        switch: &SwitchStmt,
        task_name: &str,
        modes: &HashMap<String, Span>,
        control_buffers: &[String],
    ) {
        // Validate source
        match &switch.source {
            SwitchSource::Buffer(ident) => {
                if !control_buffers.contains(&ident.name) {
                    // External/shared ctrl source: validate writer existence in post-pass.
                    // (supports forward refs where writer task appears later)
                    self.pending_buffer_reads.push((
                        ident.name.clone(),
                        task_name.to_string(),
                        ident.span,
                    ));
                }
            }
            SwitchSource::Param(ident) => {
                if !self.resolved.params.contains_key(&ident.name) {
                    self.error(
                        ident.span,
                        format!("undefined param '${}' in switch source", ident.name),
                    );
                }
            }
        }

        // Validate mode references
        for mode_ref in &switch.modes {
            if !modes.contains_key(&mode_ref.name) {
                self.error(
                    mode_ref.span,
                    format!(
                        "switch references undefined mode '{}' in task '{}'",
                        mode_ref.name, task_name
                    ),
                );
            }
        }

        // v0.2 soft-deprecation: keep parsing legacy `default` clause but
        // treat it as metadata only (warn + ignore at runtime).
        if let Some(default) = &switch.default {
            self.warning(
                default.span,
                format!(
                    "deprecated switch default clause in task '{}' is ignored in v0.2",
                    task_name
                ),
            );
        }

        // Validate mode index coverage: every defined mode must appear in
        // the switch list exactly once (contiguous 0..N-1 mapping).
        let switch_mode_names: Vec<&str> = switch.modes.iter().map(|m| m.name.as_str()).collect();
        for (mode_name, mode_span) in modes {
            if !switch_mode_names.contains(&mode_name.as_str()) {
                self.error(
                    *mode_span,
                    format!(
                        "mode '{}' defined in task '{}' but not listed in switch statement",
                        mode_name, task_name
                    ),
                );
            }
        }
        // Check for duplicate mode references in switch list
        let mut seen: std::collections::HashMap<&str, Span> = std::collections::HashMap::new();
        for mode_ref in &switch.modes {
            if let Some(&first_span) = seen.get(mode_ref.name.as_str()) {
                self.error(
                    mode_ref.span,
                    format!(
                        "mode '{}' listed multiple times in switch of task '{}' (first at offset {})",
                        mode_ref.name, task_name, first_span.start
                    ),
                );
            } else {
                seen.insert(&mode_ref.name, mode_ref.span);
            }
        }
    }

    // ── Deferred tap-ref validation ────────────────────────────────────

    fn resolve_pending_tap_refs_for(
        &mut self,
        context_name: &str,
        taps: &mut HashMap<String, TapInfo>,
    ) {
        let mut remaining = Vec::new();
        for ptr in std::mem::take(&mut self.pending_tap_refs) {
            if ptr.context_name == context_name {
                if let Some(info) = taps.get_mut(&ptr.tap_name) {
                    info.consumed = true;
                } else {
                    self.error(
                        ptr.span,
                        format!(
                            "undefined tap ':{name}' referenced as actor input in {scope}",
                            name = ptr.tap_name,
                            scope = ptr.scope,
                        ),
                    );
                }
            } else {
                remaining.push(ptr);
            }
        }
        self.pending_tap_refs = remaining;
    }

    // ── Post-pass ───────────────────────────────────────────────────────

    fn validate_buffers(&mut self) {
        let pending = std::mem::take(&mut self.pending_buffer_reads);
        for (buf_name, task_name, span) in &pending {
            if let Some(info) = self.resolved.buffers.get_mut(buf_name) {
                info.readers.push((task_name.clone(), *span));
            } else {
                self.error(
                    *span,
                    format!("shared buffer '@{}' has no writer", buf_name),
                );
            }
        }
    }

    fn validate_taps(&mut self) {
        let mut tap_errors: Vec<(Span, String)> = Vec::new();
        for (task_name, resolution) in &self.resolved.task_resolutions {
            for (tap_name, info) in &resolution.taps {
                if !info.consumed {
                    tap_errors.push((
                        info.decl_span,
                        format!(
                            "tap ':{tap_name}' declared but never consumed in task '{task_name}'"
                        ),
                    ));
                }
            }
        }
        for (span, message) in tap_errors {
            self.error(span, message);
        }
    }
}

/// Scope context for the current resolution walk.
enum Scope {
    Task {
        name: String,
    },
    Define {
        name: String,
        formal_params: Vec<String>,
    },
}

impl Scope {
    fn context_name(&self) -> String {
        match self {
            Scope::Task { name } => name.clone(),
            Scope::Define { name, .. } => name.clone(),
        }
    }

    fn description(&self) -> String {
        match self {
            Scope::Task { name } => format!("task '{}'", name),
            Scope::Define { name, .. } => format!("define '{}'", name),
        }
    }
}

fn scope_has_formal_param(scope: &Scope, name: &str) -> bool {
    match scope {
        Scope::Define { formal_params, .. } => formal_params.iter().any(|p| p == name),
        Scope::Task { .. } => false,
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Parse source and resolve with a given registry.
    fn resolve_source(source: &str, registry: &Registry) -> ResolveResult {
        let result = crate::parser::parse(source);
        assert!(
            result.errors.is_empty(),
            "parse errors in test: {:?}",
            result.errors
        );
        let program = result.program.expect("parse failed in test");
        resolve(&program, registry)
    }

    /// Parse source with empty registry, expect no errors.
    fn resolve_ok(source: &str) -> ResolvedProgram {
        let reg = Registry::new();
        let result = resolve_source(source, &reg);
        assert!(
            result
                .diagnostics
                .iter()
                .all(|d| d.level != DiagLevel::Error),
            "unexpected errors: {:#?}",
            result.diagnostics
        );
        result.resolved
    }

    /// Parse source with given registry, expect no errors.
    fn resolve_ok_with(source: &str, registry: &Registry) -> ResolvedProgram {
        let result = resolve_source(source, registry);
        assert!(
            result
                .diagnostics
                .iter()
                .all(|d| d.level != DiagLevel::Error),
            "unexpected errors: {:#?}",
            result.diagnostics
        );
        result.resolved
    }

    /// Get errors only from a ResolveResult.
    fn errors(result: &ResolveResult) -> Vec<&Diagnostic> {
        result
            .diagnostics
            .iter()
            .filter(|d| d.level == DiagLevel::Error)
            .collect()
    }

    /// Get warnings only from a ResolveResult.
    fn warnings(result: &ResolveResult) -> Vec<&Diagnostic> {
        result
            .diagnostics
            .iter()
            .filter(|d| d.level == DiagLevel::Warning)
            .collect()
    }

    /// Build a test registry from runtime/libpipit/include/std_actors.h.
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

    // ── Pass 1: global definitions ──────────────────────────────────────

    #[test]
    fn global_consts_collected() {
        let r = resolve_ok("const a = 1\nconst b = 2");
        assert!(r.consts.contains_key("a"));
        assert!(r.consts.contains_key("b"));
    }

    #[test]
    fn global_params_collected() {
        let r = resolve_ok("param gain = 1.0");
        assert!(r.params.contains_key("gain"));
    }

    #[test]
    fn global_defines_collected() {
        let reg = test_registry();
        let r = resolve_ok_with("define foo(n) {\n    constant(n)\n}", &reg);
        assert!(r.defines.contains_key("foo"));
        assert_eq!(r.defines["foo"].param_names, vec!["n"]);
    }

    #[test]
    fn global_tasks_collected() {
        let reg = test_registry();
        let r = resolve_ok_with("clock 1kHz t {\n    constant(0.0)\n}", &reg);
        assert!(r.tasks.contains_key("t"));
    }

    #[test]
    fn duplicate_const_error() {
        let reg = Registry::new();
        let result = resolve_source("const a = 1\nconst a = 2", &reg);
        let errs = errors(&result);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].message.contains("duplicate const 'a'"));
    }

    #[test]
    fn duplicate_param_error() {
        let reg = Registry::new();
        let result = resolve_source("param x = 1\nparam x = 2", &reg);
        let errs = errors(&result);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].message.contains("duplicate param 'x'"));
    }

    #[test]
    fn duplicate_task_error() {
        let reg = test_registry();
        let result = resolve_source(
            "clock 1kHz t {\n    constant(0.0)\n}\nclock 2kHz t {\n    constant(0.0)\n}",
            &reg,
        );
        let errs = errors(&result);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].message.contains("duplicate task 't'"));
    }

    #[test]
    fn const_param_collision() {
        let reg = Registry::new();
        let result = resolve_source("const x = 1\nparam x = 1.0", &reg);
        let errs = errors(&result);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].message.contains("both a const"));
    }

    // ── Actor/define resolution ─────────────────────────────────────────

    #[test]
    fn actor_resolved_from_registry() {
        let reg = test_registry();
        let r = resolve_ok_with("clock 1kHz t {\n    constant(0.0)\n}", &reg);
        assert!(r
            .call_resolutions
            .values()
            .any(|v| *v == CallResolution::Actor));
    }

    #[test]
    fn unknown_actor_error() {
        let reg = Registry::new();
        let result = resolve_source("clock 1kHz t {\n    unknown(0)\n}", &reg);
        let errs = errors(&result);
        assert_eq!(errs.len(), 1);
        assert!(errs[0]
            .message
            .contains("unknown actor or define 'unknown'"));
    }

    #[test]
    fn define_call_resolved() {
        let reg = test_registry();
        let r = resolve_ok_with(
            "define foo() {\n    constant(0.0)\n}\nclock 1kHz t {\n    foo()\n}",
            &reg,
        );
        assert!(r
            .call_resolutions
            .values()
            .any(|v| *v == CallResolution::Define));
    }

    #[test]
    fn define_shadows_actor_warning() {
        let reg = test_registry();
        let result = resolve_source(
            "define constant() {\n    mag()\n}\nclock 1kHz t {\n    constant()\n}",
            &reg,
        );
        // No errors, but should have a warning
        assert!(errors(&result).is_empty());
        let warnings: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.level == DiagLevel::Warning)
            .collect();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("shadows actor"));
    }

    // ── Param/const refs ────────────────────────────────────────────────

    #[test]
    fn param_ref_resolved() {
        let reg = test_registry();
        let _ = resolve_ok_with("param gain = 1.0\nclock 1kHz t {\n    mul($gain)\n}", &reg);
    }

    #[test]
    fn undefined_param_ref() {
        let reg = test_registry();
        let result = resolve_source("clock 1kHz t {\n    mul($unknown)\n}", &reg);
        let errs = errors(&result);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].message.contains("undefined param '$unknown'"));
    }

    #[test]
    fn const_ref_resolved() {
        let reg = test_registry();
        let _ = resolve_ok_with(
            "const coeff = [0.1, 0.2, 0.3]\nclock 1kHz t {\n    fir(coeff)\n}",
            &reg,
        );
    }

    #[test]
    fn undefined_const_ref() {
        let reg = test_registry();
        let result = resolve_source("clock 1kHz t {\n    fir(unknown)\n}", &reg);
        let errs = errors(&result);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].message.contains("undefined const 'unknown'"));
    }

    #[test]
    fn define_formal_param_as_const_ref() {
        let reg = test_registry();
        // 'n' is a formal param of the define, not a global const
        let _ = resolve_ok_with("define foo(n) {\n    fft(n)\n}", &reg);
    }

    // ── Shared buffers ──────────────────────────────────────────────────

    #[test]
    fn buffer_write_and_read() {
        let reg = test_registry();
        let r = resolve_ok_with(
            "clock 1kHz a {\n    constant(0.0) -> sig\n}\nclock 1kHz b {\n    @sig | stdout()\n}",
            &reg,
        );
        assert!(r.buffers.contains_key("sig"));
        assert_eq!(r.buffers["sig"].writer_task, "a");
        assert_eq!(r.buffers["sig"].readers.len(), 1);
        assert_eq!(r.buffers["sig"].readers[0].0, "b");
    }

    #[test]
    fn buffer_no_writer_error() {
        let reg = test_registry();
        let result = resolve_source("clock 1kHz b {\n    @sig | stdout()\n}", &reg);
        let errs = errors(&result);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].message.contains("'@sig' has no writer"));
    }

    #[test]
    fn multiple_writers_error() {
        let reg = test_registry();
        let result = resolve_source(
            "clock 1kHz a {\n    constant(0.0) -> sig\n}\nclock 1kHz b {\n    constant(0.0) -> sig\n}",
            &reg,
        );
        let errs = errors(&result);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].message.contains("multiple writers"));
    }

    // ── Taps ────────────────────────────────────────────────────────────

    #[test]
    fn tap_declare_and_consume() {
        let reg = test_registry();
        let _ = resolve_ok_with(
            "clock 1kHz t {\n    constant(0.0) | :raw | stdout()\n    :raw | stdout()\n}",
            &reg,
        );
    }

    #[test]
    fn tap_undeclared_error() {
        let reg = test_registry();
        let result = resolve_source("clock 1kHz t {\n    :unknown | stdout()\n}", &reg);
        let errs = errors(&result);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].message.contains("undefined tap ':unknown'"));
    }

    #[test]
    fn tap_duplicate_error() {
        let reg = test_registry();
        let result = resolve_source(
            "clock 1kHz t {\n    constant(0.0) | :raw | stdout()\n    constant(0.0) | :raw | stdout()\n}",
            &reg,
        );
        let errs = errors(&result);
        assert!(
            errs.iter()
                .any(|e| e.message.contains("duplicate tap ':raw'")),
            "expected duplicate tap error, got: {:#?}",
            errs
        );
    }

    #[test]
    fn tap_unused_error() {
        let reg = test_registry();
        let result = resolve_source(
            "clock 1kHz t {\n    constant(0.0) | :orphan | stdout()\n}",
            &reg,
        );
        let errs = errors(&result);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].message.contains("declared but never consumed"));
    }

    // ── Tap-ref as actor arg ───────────────────────────────────────────

    #[test]
    fn tap_ref_forward_reference_ok() {
        let reg = test_registry();
        // :fb is consumed (in add arg) before declared (end of line 2) — forward ref
        let _ = resolve_ok_with(
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
    fn tap_ref_undefined_error() {
        let reg = test_registry();
        let result = resolve_source("clock 1kHz t {\n    add(:nonexistent) | stdout()\n}", &reg);
        let errs = errors(&result);
        assert!(
            errs.iter()
                .any(|e| e.message.contains("undefined tap ':nonexistent'")),
            "expected undefined tap error, got: {:#?}",
            errs
        );
    }

    #[test]
    fn tap_ref_marks_consumed() {
        let reg = test_registry();
        // :fb is declared on line 2 and consumed via Arg::TapRef on line 1
        // Should NOT produce "declared but never consumed" error
        let result = resolve_source(
            concat!(
                "clock 1kHz t {\n",
                "    constant(0.0) | add(:fb) | stdout()\n",
                "    constant(0.0) | delay(1, 0.0) | :fb\n",
                "}",
            ),
            &reg,
        );
        let errs = errors(&result);
        assert!(
            !errs
                .iter()
                .any(|e| e.message.contains("declared but never consumed")),
            "tap :fb should be marked consumed via Arg::TapRef, got: {:#?}",
            errs
        );
    }

    // ── Modal body ──────────────────────────────────────────────────────

    #[test]
    fn switch_modes_resolved() {
        let reg = test_registry();
        let _ = resolve_ok_with(
            concat!(
                "clock 1kHz t {\n",
                "    control {\n        constant(0.0) | detect() -> ctrl\n    }\n",
                "    mode sync {\n        constant(0.0) | stdout()\n    }\n",
                "    mode data {\n        constant(0.0) | stdout()\n    }\n",
                "    switch(ctrl, sync, data) default sync\n",
                "}"
            ),
            &reg,
        );
    }

    #[test]
    fn switch_default_clause_warned_and_ignored() {
        let reg = test_registry();
        let result = resolve_source(
            concat!(
                "clock 1kHz t {\n",
                "    control {\n        constant(0.0) | detect() -> ctrl\n    }\n",
                "    mode a {\n        constant(0.0) | stdout()\n    }\n",
                "    mode b {\n        constant(0.0) | stdout()\n    }\n",
                "    switch(ctrl, a, b) default missing_legacy_mode\n",
                "}"
            ),
            &reg,
        );
        let errs = errors(&result);
        assert!(
            errs.is_empty(),
            "default clause should not be semantic error in v0.2: {:#?}",
            errs
        );
        let warns = warnings(&result);
        assert!(
            warns
                .iter()
                .any(|w| w.message.contains("deprecated switch default clause")
                    && w.message.contains("ignored in v0.2")),
            "expected deprecation warning for default clause: {:#?}",
            warns
        );
    }

    #[test]
    fn switch_undefined_mode_error() {
        let reg = test_registry();
        let result = resolve_source(
            concat!(
                "clock 1kHz t {\n",
                "    control {\n        constant(0.0) | detect() -> ctrl\n    }\n",
                "    mode sync {\n        constant(0.0) | stdout()\n    }\n",
                "    switch(ctrl, sync, missing) default sync\n",
                "}"
            ),
            &reg,
        );
        let errs = errors(&result);
        assert!(errs
            .iter()
            .any(|e| e.message.contains("undefined mode 'missing'")));
    }

    #[test]
    fn switch_param_source() {
        let reg = test_registry();
        let _ = resolve_ok_with(
            concat!(
                "param sel = 0\n",
                "clock 1kHz t {\n",
                "    control {\n        constant(0.0) | stdout()\n    }\n",
                "    mode a {\n        constant(0.0) | stdout()\n    }\n",
                "    mode b {\n        constant(0.0) | stdout()\n    }\n",
                "    switch($sel, a, b) default a\n",
                "}"
            ),
            &reg,
        );
    }

    #[test]
    fn switch_param_source_without_control_block() {
        let reg = test_registry();
        let _ = resolve_ok_with(
            concat!(
                "param sel = 0\n",
                "clock 1kHz t {\n",
                "    mode a {\n        constant(0.0) | stdout()\n    }\n",
                "    mode b {\n        constant(0.0) | stdout()\n    }\n",
                "    switch($sel, a, b) default a\n",
                "}"
            ),
            &reg,
        );
    }

    #[test]
    fn switch_external_buffer_source_without_control_block() {
        let reg = test_registry();
        let _ = resolve_ok_with(
            concat!(
                "clock 1kHz producer {\n",
                "    constant(0.0) | detect() -> ctrl\n",
                "}\n",
                "clock 1kHz t {\n",
                "    mode a {\n        constant(0.0) | stdout()\n    }\n",
                "    mode b {\n        constant(0.0) | stdout()\n    }\n",
                "    switch(ctrl, a, b) default a\n",
                "}"
            ),
            &reg,
        );
    }

    #[test]
    fn switch_buffer_source_missing_supplier_error() {
        let reg = test_registry();
        let result = resolve_source(
            concat!(
                "clock 1kHz t {\n",
                "    mode a {\n        constant(0.0) | stdout()\n    }\n",
                "    mode b {\n        constant(0.0) | stdout()\n    }\n",
                "    switch(ctrl, a, b) default a\n",
                "}"
            ),
            &reg,
        );
        let errs = errors(&result);
        assert!(
            errs.iter()
                .any(|e| e.message.contains("shared buffer '@ctrl' has no writer")),
            "expected missing switch supplier error, got: {:#?}",
            errs
        );
    }

    #[test]
    fn switch_param_source_undefined() {
        let reg = test_registry();
        let result = resolve_source(
            concat!(
                "clock 1kHz t {\n",
                "    control {\n        constant(0.0) | stdout()\n    }\n",
                "    mode a {\n        constant(0.0) | stdout()\n    }\n",
                "    mode b {\n        constant(0.0) | stdout()\n    }\n",
                "    switch($missing, a, b) default a\n",
                "}"
            ),
            &reg,
        );
        let errs = errors(&result);
        assert!(errs
            .iter()
            .any(|e| e.message.contains("undefined param '$missing'")));
    }

    // ── Mode coverage checks ─────────────────────────────────────────

    #[test]
    fn switch_missing_mode_error() {
        // mode 'c' is defined but not listed in switch → error
        let reg = test_registry();
        let result = resolve_source(
            concat!(
                "clock 1kHz t {\n",
                "    control {\n        constant(0.0) | detect() -> ctrl\n    }\n",
                "    mode a {\n        constant(0.0) | stdout()\n    }\n",
                "    mode b {\n        constant(0.0) | stdout()\n    }\n",
                "    mode c {\n        constant(0.0) | stdout()\n    }\n",
                "    switch(ctrl, a, b) default a\n",
                "}"
            ),
            &reg,
        );
        let errs = errors(&result);
        assert!(
            errs.iter()
                .any(|e| e.message.contains("mode 'c'") && e.message.contains("not listed")),
            "expected error about mode 'c' not in switch: {:#?}",
            errs
        );
    }

    #[test]
    fn switch_duplicate_mode_error() {
        let reg = test_registry();
        let result = resolve_source(
            concat!(
                "clock 1kHz t {\n",
                "    control {\n        constant(0.0) | detect() -> ctrl\n    }\n",
                "    mode a {\n        constant(0.0) | stdout()\n    }\n",
                "    mode b {\n        constant(0.0) | stdout()\n    }\n",
                "    switch(ctrl, a, b, a) default a\n",
                "}"
            ),
            &reg,
        );
        let errs = errors(&result);
        assert!(
            errs.iter()
                .any(|e| e.message.contains("mode 'a'") && e.message.contains("multiple times")),
            "expected error about duplicate mode 'a': {:#?}",
            errs
        );
    }

    // ── Shape constraint validation (v0.2.0) ──────────────────────────

    #[test]
    fn shape_constraint_literal_ok() {
        let reg = test_registry();
        let _ = resolve_ok_with("clock 1kHz t {\n    fft()[256]\n}", &reg);
    }

    #[test]
    fn shape_constraint_const_ref_ok() {
        let reg = test_registry();
        let _ = resolve_ok_with("const N = 256\nclock 1kHz t {\n    fft()[N]\n}", &reg);
    }

    #[test]
    fn shape_constraint_param_ref_error() {
        let reg = test_registry();
        let result = resolve_source("param N = 256\nclock 1kHz t {\n    fft()[N]\n}", &reg);
        let errs = errors(&result);
        assert!(
            errs.iter().any(|e| e.message.contains("runtime param '$N'")
                && e.message.contains("frame dimension")),
            "expected runtime param error, got: {:#?}",
            errs
        );
    }

    #[test]
    fn shape_constraint_unknown_name_error() {
        let reg = test_registry();
        let result = resolve_source("clock 1kHz t {\n    fft()[UNKNOWN]\n}", &reg);
        let errs = errors(&result);
        assert!(
            errs.iter()
                .any(|e| e.message.contains("unknown name 'UNKNOWN'")
                    && e.message.contains("shape constraint")),
            "expected unknown name error, got: {:#?}",
            errs
        );
    }

    #[test]
    fn shape_constraint_define_formal_param_ok() {
        let reg = test_registry();
        let _ = resolve_ok_with(
            "define my_fft(n) {\n    fft()[n]\n}\nclock 1kHz t {\n    my_fft(256)\n}",
            &reg,
        );
    }

    #[test]
    fn shape_constraint_multidim_ok() {
        let reg = test_registry();
        let _ = resolve_ok_with(
            "const H = 1080\nconst W = 1920\nclock 1kHz t {\n    stdout()[H, W, 3]\n}",
            &reg,
        );
    }

    // ── Integration tests ───────────────────────────────────────────────

    #[test]
    fn example_pdl_resolves() {
        let reg = test_registry();
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("examples/example.pdl");
        let source = std::fs::read_to_string(&path).expect("failed to read example.pdl");
        let result = resolve_source(&source, &reg);
        let errs = errors(&result);
        assert!(errs.is_empty(), "errors in example.pdl: {:#?}", errs);
    }

    #[test]
    fn receiver_pdl_resolves() {
        let reg = test_registry();
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("examples/receiver.pdl");
        let source = std::fs::read_to_string(&path).expect("failed to read receiver.pdl");
        let result = resolve_source(&source, &reg);
        let errs = errors(&result);
        assert!(errs.is_empty(), "errors in receiver.pdl: {:#?}", errs);
    }

    #[test]
    fn multiple_errors_accumulated() {
        let reg = Registry::new();
        let result = resolve_source(
            "clock 1kHz t {\n    unknown1(0)\n    unknown2($missing)\n}",
            &reg,
        );
        let errs = errors(&result);
        // Should have at least 3 errors: unknown1, unknown2, $missing
        assert!(errs.len() >= 3, "expected >=3 errors, got: {:#?}", errs);
    }
}
