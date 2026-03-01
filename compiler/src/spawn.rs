// Spawn expansion: AST → AST rewrite.
//
// Expands `clock freq name[idx=begin..end] { body }` into N independent tasks,
// one per index value in the range [begin, end). Runs before name resolution so
// that the resolver sees only plain (non-spawned) tasks.
//
// Preconditions: valid AST from the parser; all `ConstStmt` entries precede
//   `TaskStmt` entries that reference their values.
// Postconditions: no `TaskStmt` with a `SpawnClause` remains in the output.
//   Index variable is substituted in `Arg::ConstRef`, `BufferIndex::Ident`,
//   and `ShapeDim::ConstRef` positions.
// Failure modes: unresolvable spawn bounds produce diagnostics.
// Side effects: none (pure function).

use std::collections::HashMap;

use crate::ast::*;
use crate::diag::{codes, DiagLevel, Diagnostic};

/// Result of spawn expansion.
pub struct SpawnResult {
    pub program: Program,
    pub diagnostics: Vec<Diagnostic>,
}

/// Expand all spawn clauses in the program.
///
/// Const declarations are scanned first to build a name→value map for
/// resolving symbolic spawn bounds.
pub fn expand_spawns(program: &Program) -> SpawnResult {
    let mut diags = Vec::new();

    // Phase 1: collect integer consts for bound resolution.
    let consts = collect_integer_consts(&program.statements);

    // Phase 2: validate shared decl sizes.
    for stmt in &program.statements {
        if let StatementKind::Shared(decl) = &stmt.kind {
            validate_shared_size(decl, &consts, &mut diags);
        }
    }

    // Phase 3: expand spawn tasks.
    let mut out_stmts = Vec::new();
    for stmt in &program.statements {
        match &stmt.kind {
            StatementKind::Task(task) => {
                if let Some(ref spawn) = task.spawn {
                    expand_one_task(task, spawn, &consts, &mut out_stmts, &mut diags, stmt.span);
                } else {
                    out_stmts.push(stmt.clone());
                }
            }
            _ => out_stmts.push(stmt.clone()),
        }
    }

    SpawnResult {
        program: Program {
            statements: out_stmts,
            span: program.span,
        },
        diagnostics: diags,
    }
}

// ── Const collection ────────────────────────────────────────────────

/// Scan const statements and extract those with integer scalar values.
fn collect_integer_consts(stmts: &[Statement]) -> HashMap<String, u32> {
    let mut map = HashMap::new();
    for stmt in stmts {
        if let StatementKind::Const(c) = &stmt.kind {
            if let Value::Scalar(Scalar::Number(n, _, is_int)) = &c.value {
                if *is_int && *n >= 0.0 && *n <= u32::MAX as f64 {
                    map.insert(c.name.name.clone(), *n as u32);
                }
            }
        }
    }
    map
}

// ── Shared-size validation ──────────────────────────────────────────

fn validate_shared_size(
    decl: &SharedDecl,
    consts: &HashMap<String, u32>,
    diags: &mut Vec<Diagnostic>,
) {
    match &decl.size {
        ShapeDim::Literal(n, span) => {
            if *n == 0 {
                diags.push(
                    Diagnostic::new(DiagLevel::Error, *span, "shared array size must be > 0")
                        .with_code(codes::E0028),
                );
            }
        }
        ShapeDim::ConstRef(ident) => {
            match consts.get(&ident.name) {
                None => {
                    diags.push(
                        Diagnostic::new(
                            DiagLevel::Error,
                            ident.span,
                            format!("unknown const '{}' in shared array size", ident.name),
                        )
                        .with_code(codes::E0030),
                    );
                }
                Some(0) => {
                    diags.push(
                        Diagnostic::new(
                            DiagLevel::Error,
                            ident.span,
                            format!(
                                "const '{}' resolves to 0; shared array size must be > 0",
                                ident.name
                            ),
                        )
                        .with_code(codes::E0028),
                    );
                }
                Some(_) => { /* valid */ }
            }
        }
    }
}

// ── Spawn bound resolution ──────────────────────────────────────────

fn resolve_bound(
    bound: &SpawnBound,
    consts: &HashMap<String, u32>,
    diags: &mut Vec<Diagnostic>,
) -> Option<u32> {
    match bound {
        SpawnBound::Literal(n, _) => Some(*n),
        SpawnBound::ConstRef(ident) => match consts.get(&ident.name) {
            Some(v) => Some(*v),
            None => {
                diags.push(
                    Diagnostic::new(
                        DiagLevel::Error,
                        ident.span,
                        format!("unknown const '{}' in spawn bound", ident.name),
                    )
                    .with_code(codes::E0029),
                );
                None
            }
        },
    }
}

// ── Single-task expansion ───────────────────────────────────────────

fn expand_one_task(
    task: &TaskStmt,
    spawn: &SpawnClause,
    consts: &HashMap<String, u32>,
    out: &mut Vec<Statement>,
    diags: &mut Vec<Diagnostic>,
    stmt_span: Span,
) {
    let begin = resolve_bound(&spawn.begin, consts, diags);
    let end = resolve_bound(&spawn.end, consts, diags);

    let (begin, end) = match (begin, end) {
        (Some(b), Some(e)) => (b, e),
        _ => return, // bound resolution failed — diagnostics already emitted
    };

    if begin >= end {
        diags.push(
            Diagnostic::new(
                DiagLevel::Error,
                spawn.span,
                format!(
                    "spawn range {}..{} is empty (begin must be < end)",
                    begin, end
                ),
            )
            .with_code(codes::E0026),
        );
        return;
    }

    let idx_var = &spawn.index_var.name;

    for i in begin..end {
        let new_name = format!("{}__spawn_{}", task.name.name, i);
        let new_body = substitute_task_body(&task.body, idx_var, i);
        let new_task = TaskStmt {
            freq: task.freq,
            freq_span: task.freq_span,
            name: Ident {
                name: new_name,
                span: task.name.span,
            },
            spawn: None, // expanded — no longer a spawn
            body: new_body,
        };
        out.push(Statement {
            kind: StatementKind::Task(Box::new(new_task)),
            span: stmt_span,
        });
    }
}

// ── Substitution ────────────────────────────────────────────────────

fn substitute_task_body(body: &TaskBody, idx_var: &str, idx_val: u32) -> TaskBody {
    match body {
        TaskBody::Pipeline(pb) => {
            TaskBody::Pipeline(substitute_pipeline_body(pb, idx_var, idx_val))
        }
        TaskBody::Modal(mb) => TaskBody::Modal(substitute_modal_body(mb, idx_var, idx_val)),
    }
}

fn substitute_modal_body(mb: &ModalBody, idx_var: &str, idx_val: u32) -> ModalBody {
    ModalBody {
        control: ControlBlock {
            body: substitute_pipeline_body(&mb.control.body, idx_var, idx_val),
            span: mb.control.span,
        },
        modes: mb
            .modes
            .iter()
            .map(|m| ModeBlock {
                name: m.name.clone(),
                body: substitute_pipeline_body(&m.body, idx_var, idx_val),
                span: m.span,
            })
            .collect(),
        switch: mb.switch.clone(),
        span: mb.span,
    }
}

fn substitute_pipeline_body(pb: &PipelineBody, idx_var: &str, idx_val: u32) -> PipelineBody {
    PipelineBody {
        lines: pb
            .lines
            .iter()
            .map(|pe| substitute_pipe_expr(pe, idx_var, idx_val))
            .collect(),
        span: pb.span,
    }
}

fn substitute_pipe_expr(pe: &PipeExpr, idx_var: &str, idx_val: u32) -> PipeExpr {
    PipeExpr {
        source: substitute_pipe_source(&pe.source, idx_var, idx_val),
        elements: pe
            .elements
            .iter()
            .map(|e| substitute_pipe_elem(e, idx_var, idx_val))
            .collect(),
        sink: pe
            .sink
            .as_ref()
            .map(|s| substitute_sink(s, idx_var, idx_val)),
        span: pe.span,
    }
}

fn substitute_pipe_source(src: &PipeSource, idx_var: &str, idx_val: u32) -> PipeSource {
    match src {
        PipeSource::BufferRead(br) => {
            PipeSource::BufferRead(substitute_buffer_ref(br, idx_var, idx_val))
        }
        PipeSource::TapRef(_) => src.clone(),
        PipeSource::ActorCall(ac) => {
            PipeSource::ActorCall(substitute_actor_call(ac, idx_var, idx_val))
        }
    }
}

fn substitute_pipe_elem(elem: &PipeElem, idx_var: &str, idx_val: u32) -> PipeElem {
    match elem {
        PipeElem::ActorCall(ac) => PipeElem::ActorCall(substitute_actor_call(ac, idx_var, idx_val)),
        PipeElem::Tap(_) | PipeElem::Probe(_) => elem.clone(),
    }
}

fn substitute_sink(sink: &Sink, idx_var: &str, idx_val: u32) -> Sink {
    Sink {
        buffer: substitute_buffer_ref(&sink.buffer, idx_var, idx_val),
        span: sink.span,
    }
}

fn substitute_buffer_ref(br: &BufferRef, idx_var: &str, idx_val: u32) -> BufferRef {
    let index = match &br.index {
        BufferIndex::Ident(ident) if ident.name == idx_var => {
            BufferIndex::Literal(idx_val, ident.span)
        }
        other => other.clone(),
    };
    BufferRef {
        name: br.name.clone(),
        index,
    }
}

fn substitute_actor_call(ac: &ActorCall, idx_var: &str, idx_val: u32) -> ActorCall {
    ActorCall {
        name: ac.name.clone(),
        type_args: ac.type_args.clone(),
        args: ac
            .args
            .iter()
            .map(|a| substitute_arg(a, idx_var, idx_val))
            .collect(),
        shape_constraint: ac
            .shape_constraint
            .as_ref()
            .map(|sc| substitute_shape_constraint(sc, idx_var, idx_val)),
        span: ac.span,
    }
}

fn substitute_arg(arg: &Arg, idx_var: &str, idx_val: u32) -> Arg {
    match arg {
        Arg::ConstRef(ident) if ident.name == idx_var => Arg::Value(Value::Scalar(Scalar::Number(
            idx_val as f64,
            ident.span,
            true,
        ))),
        _ => arg.clone(),
    }
}

fn substitute_shape_constraint(
    sc: &ShapeConstraint,
    idx_var: &str,
    idx_val: u32,
) -> ShapeConstraint {
    ShapeConstraint {
        dims: sc
            .dims
            .iter()
            .map(|d| match d {
                ShapeDim::ConstRef(ident) if ident.name == idx_var => {
                    ShapeDim::Literal(idx_val, ident.span)
                }
                other => other.clone(),
            })
            .collect(),
        span: sc.span,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    fn parse_program(src: &str) -> Program {
        let result = parse(src);
        assert!(
            result.errors.is_empty(),
            "parse errors: {:?}",
            result.errors
        );
        result.program.expect("no program produced")
    }

    // ── Simple spawn expansion ──────────────────────────────────────

    #[test]
    fn expand_simple_spawn() {
        let prog = parse_program("clock 1kHz t[ch=0..3] {\n  adc(ch) | proc()\n}");
        let result = expand_spawns(&prog);
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        // 3 tasks: t__spawn_0, t__spawn_1, t__spawn_2
        assert_eq!(result.program.statements.len(), 3);
        for (i, stmt) in result.program.statements.iter().enumerate() {
            let StatementKind::Task(t) = &stmt.kind else {
                panic!("expected Task");
            };
            assert_eq!(t.name.name, format!("t__spawn_{}", i));
            assert!(t.spawn.is_none());
        }
    }

    #[test]
    fn expand_spawn_with_const_bounds() {
        let prog = parse_program("const CH = 4\nclock 1kHz t[ch=0..CH] {\n  adc(ch) | proc()\n}");
        let result = expand_spawns(&prog);
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        // const + 4 tasks
        assert_eq!(result.program.statements.len(), 5);
        for i in 0..4 {
            let StatementKind::Task(t) = &result.program.statements[i + 1].kind else {
                panic!("expected Task");
            };
            assert_eq!(t.name.name, format!("t__spawn_{}", i));
        }
    }

    // ── Index substitution in actor args ─────────────────────────────

    #[test]
    fn substitution_in_actor_args() {
        let prog = parse_program("clock 1kHz t[ch=0..2] {\n  adc(ch) | proc()\n}");
        let result = expand_spawns(&prog);
        assert!(result.diagnostics.is_empty());

        // Check first expanded task: ch=0
        let StatementKind::Task(t0) = &result.program.statements[0].kind else {
            panic!("expected Task");
        };
        let TaskBody::Pipeline(pb) = &t0.body else {
            panic!("expected Pipeline");
        };
        let PipeSource::ActorCall(ac) = &pb.lines[0].source else {
            panic!("expected ActorCall source");
        };
        // adc(ch) → adc(0)
        match &ac.args[0] {
            Arg::Value(Value::Scalar(Scalar::Number(n, _, true))) => {
                assert_eq!(*n, 0.0);
            }
            other => panic!("expected substituted Number, got {:?}", other),
        }

        // Check second task: ch=1
        let StatementKind::Task(t1) = &result.program.statements[1].kind else {
            panic!("expected Task");
        };
        let TaskBody::Pipeline(pb1) = &t1.body else {
            panic!("expected Pipeline");
        };
        let PipeSource::ActorCall(ac1) = &pb1.lines[0].source else {
            panic!("expected ActorCall source");
        };
        match &ac1.args[0] {
            Arg::Value(Value::Scalar(Scalar::Number(n, _, true))) => {
                assert_eq!(*n, 1.0);
            }
            other => panic!("expected substituted Number, got {:?}", other),
        }
    }

    // ── Index substitution in buffer refs ────────────────────────────

    #[test]
    fn substitution_in_buffer_refs() {
        let prog = parse_program("clock 1kHz t[ch=0..2] {\n  @in[ch] | proc() -> out[ch]\n}");
        let result = expand_spawns(&prog);
        assert!(result.diagnostics.is_empty());

        let StatementKind::Task(t0) = &result.program.statements[0].kind else {
            panic!("expected Task");
        };
        let TaskBody::Pipeline(pb) = &t0.body else {
            panic!("expected Pipeline");
        };

        // @in[ch] → @in[0]
        let PipeSource::BufferRead(br) = &pb.lines[0].source else {
            panic!("expected BufferRead");
        };
        assert_eq!(br.name.name, "in");
        assert!(matches!(br.index, BufferIndex::Literal(0, _)));

        // -> out[ch] → -> out[0]
        let sink = pb.lines[0].sink.as_ref().unwrap();
        assert_eq!(sink.buffer.name.name, "out");
        assert!(matches!(sink.buffer.index, BufferIndex::Literal(0, _)));
    }

    // ── Star refs pass through ───────────────────────────────────────

    #[test]
    fn star_ref_passthrough() {
        let prog = parse_program("clock 1kHz t[ch=0..2] {\n  @in[*] | proc() -> out[*]\n}");
        let result = expand_spawns(&prog);
        assert!(result.diagnostics.is_empty());

        let StatementKind::Task(t0) = &result.program.statements[0].kind else {
            panic!("expected Task");
        };
        let TaskBody::Pipeline(pb) = &t0.body else {
            panic!("expected Pipeline");
        };

        // @in[*] should remain [*]
        let PipeSource::BufferRead(br) = &pb.lines[0].source else {
            panic!("expected BufferRead");
        };
        assert!(matches!(br.index, BufferIndex::Star(_)));

        // -> out[*] should remain [*]
        let sink = pb.lines[0].sink.as_ref().unwrap();
        assert!(matches!(sink.buffer.index, BufferIndex::Star(_)));
    }

    // ── Non-spawn task passthrough ───────────────────────────────────

    #[test]
    fn non_spawn_task_passthrough() {
        let prog = parse_program("clock 1kHz t {\n  adc(0) | proc()\n}");
        let result = expand_spawns(&prog);
        assert!(result.diagnostics.is_empty());
        assert_eq!(result.program.statements.len(), 1);
        let StatementKind::Task(t) = &result.program.statements[0].kind else {
            panic!("expected Task");
        };
        assert_eq!(t.name.name, "t");
        assert!(t.spawn.is_none());
    }

    // ── Range validation errors ──────────────────────────────────────

    #[test]
    fn empty_range_error() {
        let prog = parse_program("clock 1kHz t[ch=3..3] {\n  adc(ch) | proc()\n}");
        let result = expand_spawns(&prog);
        assert_eq!(result.diagnostics.len(), 1);
        assert_eq!(result.diagnostics[0].code, Some(codes::E0026));
    }

    #[test]
    fn reversed_range_error() {
        let prog = parse_program("clock 1kHz t[ch=5..2] {\n  adc(ch) | proc()\n}");
        let result = expand_spawns(&prog);
        assert_eq!(result.diagnostics.len(), 1);
        assert_eq!(result.diagnostics[0].code, Some(codes::E0026));
    }

    // ── Unknown const errors ─────────────────────────────────────────

    #[test]
    fn unknown_const_in_spawn_bound() {
        let prog = parse_program("clock 1kHz t[ch=0..UNKNOWN] {\n  adc(ch) | proc()\n}");
        let result = expand_spawns(&prog);
        assert_eq!(result.diagnostics.len(), 1);
        assert_eq!(result.diagnostics[0].code, Some(codes::E0029));
    }

    #[test]
    fn unknown_const_in_shared_size() {
        let prog = parse_program("shared buf[UNKNOWN]");
        let result = expand_spawns(&prog);
        assert_eq!(result.diagnostics.len(), 1);
        assert_eq!(result.diagnostics[0].code, Some(codes::E0030));
    }

    // ── Shared size validation ───────────────────────────────────────

    #[test]
    fn shared_size_zero_const() {
        let prog = parse_program("const Z = 0\nshared buf[Z]");
        let result = expand_spawns(&prog);
        assert_eq!(result.diagnostics.len(), 1);
        assert_eq!(result.diagnostics[0].code, Some(codes::E0028));
    }

    // ── Mixed statements preserved ───────────────────────────────────

    #[test]
    fn mixed_statements_preserved() {
        let prog = parse_program(
            "const CH = 2\nshared in[CH]\nclock 1kHz t[ch=0..CH] {\n  @in[ch] | proc()\n}\nclock 1kHz monitor {\n  adc(0) | stdout()\n}",
        );
        let result = expand_spawns(&prog);
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        // const + shared + 2 expanded tasks + monitor
        assert_eq!(result.program.statements.len(), 5);

        // const and shared pass through
        assert!(matches!(
            &result.program.statements[0].kind,
            StatementKind::Const(_)
        ));
        assert!(matches!(
            &result.program.statements[1].kind,
            StatementKind::Shared(_)
        ));

        // 2 expanded tasks
        let StatementKind::Task(t0) = &result.program.statements[2].kind else {
            panic!("expected Task");
        };
        assert_eq!(t0.name.name, "t__spawn_0");
        let StatementKind::Task(t1) = &result.program.statements[3].kind else {
            panic!("expected Task");
        };
        assert_eq!(t1.name.name, "t__spawn_1");

        // monitor passes through
        let StatementKind::Task(t_mon) = &result.program.statements[4].kind else {
            panic!("expected Task");
        };
        assert_eq!(t_mon.name.name, "monitor");
    }
}
