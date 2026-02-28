// Parser for Pipit .pdl source files.
//
// Parses a token stream (from the lexer) into an AST per the BNF grammar in
// the Pipit Language Specification §10. Uses chumsky combinators.
//
// Preconditions: input is a valid token stream from `lexer::lex()`.
// Postconditions: returns an AST plus any parse errors (non-fatal).
// Failure modes: syntax errors produce `Rich` diagnostics; parsing continues.
// Side effects: none.

use chumsky::input::{Stream, ValueInput};
use chumsky::prelude::*;
use chumsky::span::SimpleSpan;

use crate::ast::*;
use crate::lexer::Token;

/// Result of parsing: AST plus any errors.
#[derive(Debug)]
pub struct ParseResult {
    pub program: Option<Program>,
    pub errors: Vec<Rich<'static, Token, SimpleSpan>>,
}

/// Parse a Pipit source string. Lexes then parses.
///
/// Returns an AST (if parsing succeeded) plus any errors.
pub fn parse(source: &str) -> ParseResult {
    let lex_result = crate::lexer::lex(source);
    let len = source.len();

    // Convert lexer output to chumsky stream.
    let token_iter = lex_result.tokens.into_iter().map(|(tok, span)| {
        let cspan: SimpleSpan = (span.start..span.end).into();
        (tok, cspan)
    });
    let eoi: SimpleSpan = (len..len).into();
    let stream = Stream::from_iter(token_iter).map(eoi, |(t, s): (_, _)| (t, s));

    let parser = program_parser(source);
    let (program, parse_errors) = parser.parse(stream).into_output_errors();

    // Merge lex errors + parse errors.
    let mut all_errors: Vec<Rich<'static, Token, SimpleSpan>> = lex_result
        .errors
        .into_iter()
        .map(|e| {
            let span: SimpleSpan = (e.span.start..e.span.end).into();
            Rich::custom(span, e.message)
        })
        .collect();
    all_errors.extend(parse_errors.into_iter().map(|e| e.into_owned()));

    ParseResult {
        program,
        errors: all_errors,
    }
}

// ── Main parser builder ──
//
// All grammar rules are built inside `program_parser` so that the `source`
// reference is captured once and shared by all combinators. This avoids
// complex lifetime annotations on per-rule helper functions.

fn program_parser<'tokens, 'src: 'tokens, I>(
    source: &'src str,
) -> impl Parser<'tokens, I, Program, extra::Err<Rich<'tokens, Token, SimpleSpan>>> + 'src
where
    'tokens: 'src,
    I: ValueInput<'tokens, Token = Token, Span = SimpleSpan>,
{
    let classify_number = |span: SimpleSpan| {
        let lexeme = &source[span.start()..span.end()];
        let is_int_literal =
            !lexeme.contains('.') && !lexeme.contains('e') && !lexeme.contains('E');
        (span, is_int_literal)
    };

    // ── Newlines ──

    let nl = just(Token::Newline).repeated().ignored();

    // ── Identifier ──

    let ident = just(Token::Ident).map_with(move |_, e| {
        let span: SimpleSpan = e.span();
        Ident {
            name: source[span.start()..span.end()].to_string(),
            span,
        }
    });

    // ── Scalar ──

    let scalar = {
        let ident_scalar = ident.clone().map(Scalar::Ident);
        select! {
            Token::Number(n) = e => {
                let (span, is_int_literal) = classify_number(e.span());
                Scalar::Number(n, span, is_int_literal)
            },
            Token::Freq(f) = e => Scalar::Freq(f, e.span()),
            Token::Size(s) = e => Scalar::Size(s, e.span()),
            Token::StringLit(s) = e => Scalar::StringLit(s, e.span()),
        }
        .or(ident_scalar)
    };

    // ── Array ──

    let array = scalar
        .clone()
        .separated_by(just(Token::Comma))
        .at_least(1)
        .collect::<Vec<_>>()
        .delimited_by(just(Token::LBracket), just(Token::RBracket))
        .map_with(|scalars, e| Value::Array(scalars, e.span()));

    // ── Value = array | scalar ──

    let value = array.clone().or(scalar.clone().map(Value::Scalar));

    // ── Arg ──

    let arg = {
        let param_ref = just(Token::Dollar)
            .ignore_then(ident.clone())
            .map(Arg::ParamRef);

        let array_arg = scalar
            .clone()
            .separated_by(just(Token::Comma))
            .at_least(1)
            .collect::<Vec<_>>()
            .delimited_by(just(Token::LBracket), just(Token::RBracket))
            .map_with(|scalars, e| Arg::Value(Value::Array(scalars, e.span())));

        let literal_arg = select! {
            Token::Number(n) = e => {
                let (span, is_int_literal) = classify_number(e.span());
                Arg::Value(Value::Scalar(Scalar::Number(n, span, is_int_literal)))
            },
            Token::Freq(f) = e => Arg::Value(Value::Scalar(Scalar::Freq(f, e.span()))),
            Token::Size(s) = e => Arg::Value(Value::Scalar(Scalar::Size(s, e.span()))),
            Token::StringLit(s) = e => Arg::Value(Value::Scalar(Scalar::StringLit(s, e.span()))),
        };

        let tap_ref = just(Token::Colon)
            .ignore_then(ident.clone())
            .map(Arg::TapRef);

        let const_ref = ident.clone().map(Arg::ConstRef);

        param_ref
            .or(tap_ref)
            .or(array_arg)
            .or(literal_arg)
            .or(const_ref)
    };

    // ── Actor call: IDENT '(' args? ')' ──
    // `delay` is a keyword (§2.4) but also a built-in actor (§5.10),
    // so actor names accept Token::Ident or Token::Delay.

    let actor_name = just(Token::Ident)
        .or(just(Token::Delay))
        .map_with(move |_, e| {
            let span: SimpleSpan = e.span();
            Ident {
                name: source[span.start()..span.end()].to_string(),
                span,
            }
        });

    // ── Shape constraint: '[' shape_dim (',' shape_dim)* ']' ──

    let shape_dim = select! {
        Token::Number(n) if n > 0.0 && n.fract() == 0.0 && n <= u32::MAX as f64 => n,
    }
    .map_with(|n, e| ShapeDim::Literal(n as u32, e.span()))
    .or(ident.clone().map(ShapeDim::ConstRef));

    let shape_constraint = shape_dim
        .separated_by(just(Token::Comma))
        .at_least(1)
        .collect::<Vec<_>>()
        .delimited_by(just(Token::LBracket), just(Token::RBracket))
        .map_with(|dims, e| ShapeConstraint {
            dims,
            span: e.span(),
        });

    // ── Type argument: pipit type keyword parsed as Ident ──
    // Accepts: int8, int16, int32, float, double, cfloat, cdouble
    let type_arg = just(Token::Ident)
        .map_with(move |_, e| {
            let span: SimpleSpan = e.span();
            Ident {
                name: source[span.start()..span.end()].to_string(),
                span,
            }
        })
        .try_map(|id, span| {
            match id.name.as_str() {
                "int8" | "int16" | "int32" | "float" | "double" | "cfloat" | "cdouble" => Ok(id),
                _ => Err(chumsky::error::Rich::custom(span, format!("expected pipit type (int8, int16, int32, float, double, cfloat, cdouble), found '{}'", id.name))),
            }
        });

    // ── Type args: '<' type (',' type)* '>' (optional) ──
    let type_args = type_arg
        .separated_by(just(Token::Comma))
        .at_least(1)
        .collect::<Vec<_>>()
        .delimited_by(just(Token::Lt), just(Token::Gt));

    let actor_call = actor_name
        .clone()
        .then(type_args.or_not())
        .then(
            arg.separated_by(just(Token::Comma))
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LParen), just(Token::RParen)),
        )
        .then(shape_constraint.or_not())
        .map_with(|(((name, type_args), args), shape), e| ActorCall {
            name,
            type_args: type_args.unwrap_or_default(),
            args,
            shape_constraint: shape,
            span: e.span(),
        });

    // ── Pipe expression ──

    let pipe_source = {
        let buffer_read = just(Token::At)
            .ignore_then(ident.clone())
            .map(PipeSource::BufferRead);
        let tap_ref = just(Token::Colon)
            .ignore_then(ident.clone())
            .map(PipeSource::TapRef);
        let actor_src = actor_call.clone().map(PipeSource::ActorCall);
        buffer_read.or(tap_ref).or(actor_src)
    };

    let pipe_elem = {
        let tap = just(Token::Colon)
            .ignore_then(ident.clone())
            .map(PipeElem::Tap);
        let probe = just(Token::Question)
            .ignore_then(ident.clone())
            .map(PipeElem::Probe);
        let actor_elem = actor_call.clone().map(PipeElem::ActorCall);
        tap.or(probe).or(actor_elem)
    };

    let sink = just(Token::Arrow)
        .ignore_then(ident.clone())
        .map_with(|buffer, e| Sink {
            buffer,
            span: e.span(),
        });

    let pipe_expr = pipe_source
        .then(
            just(Token::Pipe)
                .ignore_then(pipe_elem)
                .repeated()
                .collect::<Vec<_>>(),
        )
        .then(sink.or_not())
        .map_with(|((src, elements), snk), e| PipeExpr {
            source: src,
            elements,
            sink: snk,
            span: e.span(),
        });

    // ── Pipeline body: newline-separated pipe exprs ──

    let pipeline_body = nl
        .clone()
        .ignore_then(
            pipe_expr
                .clone()
                .separated_by(just(Token::Newline).repeated().at_least(1))
                .allow_trailing()
                .collect::<Vec<_>>(),
        )
        .then_ignore(nl.clone())
        .map_with(|lines, e| PipelineBody {
            lines,
            span: e.span(),
        });

    // ── Set value ──

    let set_value = select! {
        Token::Number(n) = e => SetValue::Number(n, e.span()),
        Token::Freq(f) = e => SetValue::Freq(f, e.span()),
        Token::Size(s) = e => SetValue::Size(s, e.span()),
        Token::StringLit(s) = e => SetValue::StringLit(s, e.span()),
    }
    .or(ident.clone().map(SetValue::Ident));

    // ── Statements ──

    let set_stmt = just(Token::Set)
        .ignore_then(ident.clone())
        .then_ignore(just(Token::Equals))
        .then(set_value)
        .map(|(name, val)| StatementKind::Set(SetStmt { name, value: val }));

    let const_stmt = just(Token::Const)
        .ignore_then(ident.clone())
        .then_ignore(just(Token::Equals))
        .then(value)
        .map(|(name, val)| StatementKind::Const(ConstStmt { name, value: val }));

    let param_stmt = just(Token::Param)
        .ignore_then(ident.clone())
        .then_ignore(just(Token::Equals))
        .then(scalar.clone())
        .map(|(name, val)| StatementKind::Param(ParamStmt { name, value: val }));

    // ── Bind statement ──

    let bind_stmt = {
        // Ident-leading: could be Named(ident '=' scalar) or Positional(Scalar::Ident)
        let ident_bind_arg = ident
            .clone()
            .then(just(Token::Equals).ignore_then(scalar.clone()).or_not())
            .map(|(name, opt_val)| match opt_val {
                Some(val) => BindArg::Named(name, val),
                None => BindArg::Positional(Scalar::Ident(name)),
            });
        // Non-ident scalars are always positional
        let non_ident_bind_arg = select! {
            Token::Number(n) = e => {
                let (span, is_int_literal) = classify_number(e.span());
                Scalar::Number(n, span, is_int_literal)
            },
            Token::Freq(f) = e => Scalar::Freq(f, e.span()),
            Token::Size(s) = e => Scalar::Size(s, e.span()),
            Token::StringLit(s) = e => Scalar::StringLit(s, e.span()),
        }
        .map(BindArg::Positional);

        let bind_arg = ident_bind_arg.or(non_ident_bind_arg);

        let bind_endpoint = ident
            .clone()
            .then(
                bind_arg
                    .separated_by(just(Token::Comma))
                    .allow_trailing()
                    .collect::<Vec<_>>()
                    .delimited_by(just(Token::LParen), just(Token::RParen)),
            )
            .map_with(|(transport, args), e| BindEndpoint {
                transport,
                args,
                span: e.span(),
            });

        just(Token::Bind)
            .ignore_then(ident.clone())
            .then_ignore(just(Token::Equals))
            .then(bind_endpoint)
            .map(|(name, endpoint)| StatementKind::Bind(BindStmt { name, endpoint }))
    };

    let define_stmt = just(Token::Define)
        .ignore_then(ident.clone())
        .then(
            ident
                .clone()
                .separated_by(just(Token::Comma))
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LParen), just(Token::RParen)),
        )
        .then(
            pipeline_body
                .clone()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map(|((name, params), body)| StatementKind::Define(DefineStmt { name, params, body }));

    // ── Modal body ──

    let control_block = just(Token::Control)
        .ignore_then(
            pipeline_body
                .clone()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map_with(|body, e| ControlBlock {
            body,
            span: e.span(),
        });

    let mode_block = just(Token::Mode)
        .ignore_then(ident.clone())
        .then(
            pipeline_body
                .clone()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map_with(|(name, body), e| ModeBlock {
            name,
            body,
            span: e.span(),
        });

    let switch_source = {
        let param = just(Token::Dollar)
            .ignore_then(ident.clone())
            .map(SwitchSource::Param);
        let buffer = ident.clone().map(SwitchSource::Buffer);
        param.or(buffer)
    };

    let switch_stmt = just(Token::Switch)
        .ignore_then(
            switch_source
                .then_ignore(just(Token::Comma))
                .then(
                    ident
                        .clone()
                        .separated_by(just(Token::Comma))
                        .at_least(2)
                        .collect::<Vec<_>>(),
                )
                .delimited_by(just(Token::LParen), just(Token::RParen)),
        )
        .then(just(Token::Default).ignore_then(ident.clone()).or_not())
        .map_with(|((src, modes), default), e| SwitchStmt {
            source: src,
            modes,
            default,
            span: e.span(),
        });

    let modal_body = control_block
        .then_ignore(nl.clone())
        .or_not()
        .then_ignore(nl.clone())
        .then(
            mode_block
                .then_ignore(nl.clone())
                .repeated()
                .at_least(1)
                .collect::<Vec<_>>(),
        )
        .then(switch_stmt)
        .then_ignore(nl.clone())
        .map_with(|((control_opt, modes), switch), e| {
            let control = control_opt.unwrap_or(ControlBlock {
                body: PipelineBody {
                    lines: Vec::new(),
                    span: e.span(),
                },
                span: e.span(),
            });
            TaskBody::Modal(ModalBody {
                control,
                modes,
                switch,
                span: e.span(),
            })
        });

    // ── Task statement ──

    let freq = select! {
        Token::Freq(f) = e => (f, e.span()),
    };

    let task_body = nl
        .clone()
        .ignore_then(modal_body)
        .or(pipeline_body.map(TaskBody::Pipeline));

    let task_stmt = just(Token::Clock)
        .ignore_then(freq)
        .then(ident.clone())
        .then(task_body.delimited_by(just(Token::LBrace), just(Token::RBrace)))
        .map(|(((freq_val, freq_span), name), body)| {
            StatementKind::Task(TaskStmt {
                freq: freq_val,
                freq_span,
                name,
                body,
            })
        });

    // ── Statement dispatch ──

    let statement = choice((
        set_stmt,
        const_stmt,
        param_stmt,
        bind_stmt,
        define_stmt,
        task_stmt,
    ))
    .map_with(|kind, e| Statement {
        kind,
        span: e.span(),
    });

    // ── Program ──

    nl.clone()
        .ignore_then(
            statement
                .separated_by(just(Token::Newline).repeated().at_least(1))
                .allow_trailing()
                .collect::<Vec<_>>(),
        )
        .then_ignore(nl)
        .map_with(move |statements, e| Program {
            statements,
            span: e.span(),
        })
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ok(source: &str) -> Program {
        let result = parse(source);
        assert!(
            result.errors.is_empty(),
            "unexpected errors: {:#?}",
            result.errors
        );
        result.program.expect("expected program")
    }

    fn parse_all(source: &str) -> (Option<Program>, Vec<Rich<'static, Token, SimpleSpan>>) {
        let result = parse(source);
        (result.program, result.errors)
    }

    fn parse_one_stmt(source: &str) -> Statement {
        let prog = parse_ok(source);
        assert_eq!(prog.statements.len(), 1, "expected 1 statement");
        prog.statements.into_iter().next().unwrap()
    }

    // ── Empty / blank ──

    #[test]
    fn empty_program() {
        let prog = parse_ok("");
        assert!(prog.statements.is_empty());
    }

    #[test]
    fn blank_lines_only() {
        let prog = parse_ok("\n\n\n");
        assert!(prog.statements.is_empty());
    }

    // ── set_stmt ──

    #[test]
    fn set_number() {
        let s = parse_one_stmt("set x = 42");
        let StatementKind::Set(set) = &s.kind else {
            panic!("expected Set")
        };
        assert_eq!(set.name.name, "x");
        assert!(matches!(set.value, SetValue::Number(v, _) if v == 42.0));
    }

    #[test]
    fn set_size() {
        let s = parse_one_stmt("set mem = 64MB");
        let StatementKind::Set(set) = &s.kind else {
            panic!("expected Set")
        };
        assert_eq!(set.name.name, "mem");
        assert!(matches!(set.value, SetValue::Size(v, _) if v == 64 * 1024 * 1024));
    }

    #[test]
    fn set_freq() {
        let s = parse_one_stmt("set rate = 48kHz");
        let StatementKind::Set(set) = &s.kind else {
            panic!("expected Set")
        };
        assert!(matches!(set.value, SetValue::Freq(v, _) if v == 48_000.0));
    }

    #[test]
    fn set_string() {
        let s = parse_one_stmt(r#"set name = "hello""#);
        let StatementKind::Set(set) = &s.kind else {
            panic!("expected Set")
        };
        assert!(matches!(&set.value, SetValue::StringLit(v, _) if v == "hello"));
    }

    #[test]
    fn set_ident() {
        let s = parse_one_stmt("set scheduler = round_robin");
        let StatementKind::Set(set) = &s.kind else {
            panic!("expected Set")
        };
        assert!(matches!(&set.value, SetValue::Ident(id) if id.name == "round_robin"));
    }

    // ── const_stmt ──

    #[test]
    fn const_scalar() {
        let s = parse_one_stmt("const threshold = 0.5");
        let StatementKind::Const(c) = &s.kind else {
            panic!("expected Const")
        };
        assert_eq!(c.name.name, "threshold");
        assert!(matches!(c.value, Value::Scalar(Scalar::Number(v, _, _)) if v == 0.5));
    }

    #[test]
    fn const_array() {
        let s = parse_one_stmt("const c = [0.1, 0.4, 0.1]");
        let StatementKind::Const(c) = &s.kind else {
            panic!("expected Const")
        };
        assert!(matches!(&c.value, Value::Array(v, _) if v.len() == 3));
    }

    // ── param_stmt ──

    #[test]
    fn param_stmt() {
        let s = parse_one_stmt("param gain = 1.0");
        let StatementKind::Param(p) = &s.kind else {
            panic!("expected Param")
        };
        assert_eq!(p.name.name, "gain");
        assert!(matches!(p.value, Scalar::Number(v, _, _) if v == 1.0));
    }

    // ── define_stmt ──

    #[test]
    fn define_with_params() {
        let s = parse_one_stmt("define frontend(n) {\n  adc(n)\n}");
        let StatementKind::Define(d) = &s.kind else {
            panic!("expected Define")
        };
        assert_eq!(d.name.name, "frontend");
        assert_eq!(d.params.len(), 1);
        assert_eq!(d.params[0].name, "n");
        assert_eq!(d.body.lines.len(), 1);
    }

    #[test]
    fn define_no_params() {
        let s = parse_one_stmt("define noop() {\n  foo()\n}");
        let StatementKind::Define(d) = &s.kind else {
            panic!("expected Define")
        };
        assert!(d.params.is_empty());
        assert_eq!(d.body.lines.len(), 1);
    }

    // ── task_stmt ──

    #[test]
    fn task_simple_pipeline() {
        let s = parse_one_stmt("clock 48kHz audio {\n  constant(0.0) | fir(c)\n}");
        let StatementKind::Task(t) = &s.kind else {
            panic!("expected Task")
        };
        assert_eq!(t.freq, 48_000.0);
        assert_eq!(t.name.name, "audio");
        let TaskBody::Pipeline(p) = &t.body else {
            panic!("expected Pipeline")
        };
        assert_eq!(p.lines.len(), 1);
        assert!(
            matches!(&p.lines[0].source, PipeSource::ActorCall(a) if a.name.name == "constant")
        );
    }

    // ── pipe_expr variations ──

    #[test]
    fn pipe_with_buffer_read() {
        let s = parse_one_stmt("clock 1kHz t {\n  @signal | proc()\n}");
        let StatementKind::Task(t) = &s.kind else {
            panic!("expected Task")
        };
        let TaskBody::Pipeline(p) = &t.body else {
            panic!("expected Pipeline")
        };
        assert!(matches!(&p.lines[0].source, PipeSource::BufferRead(id) if id.name == "signal"));
    }

    #[test]
    fn pipe_with_tap() {
        let s = parse_one_stmt("clock 1kHz t {\n  adc(0) | :raw | fir(c)\n}");
        let StatementKind::Task(t) = &s.kind else {
            panic!("expected Task")
        };
        let TaskBody::Pipeline(p) = &t.body else {
            panic!("expected Pipeline")
        };
        assert_eq!(p.lines[0].elements.len(), 2);
        assert!(matches!(&p.lines[0].elements[0], PipeElem::Tap(id) if id.name == "raw"));
    }

    #[test]
    fn pipe_with_tap_ref_source() {
        let s = parse_one_stmt("clock 1kHz t {\n  :raw | mag()\n}");
        let StatementKind::Task(t) = &s.kind else {
            panic!("expected Task")
        };
        let TaskBody::Pipeline(p) = &t.body else {
            panic!("expected Pipeline")
        };
        assert!(matches!(&p.lines[0].source, PipeSource::TapRef(id) if id.name == "raw"));
    }

    #[test]
    fn pipe_with_probe() {
        let s = parse_one_stmt("clock 1kHz t {\n  adc(0) | ?debug | fir(c)\n}");
        let StatementKind::Task(t) = &s.kind else {
            panic!("expected Task")
        };
        let TaskBody::Pipeline(p) = &t.body else {
            panic!("expected Pipeline")
        };
        assert!(matches!(&p.lines[0].elements[0], PipeElem::Probe(id) if id.name == "debug"));
    }

    #[test]
    fn pipe_with_sink() {
        let s = parse_one_stmt("clock 1kHz t {\n  adc(0) | fir(c) -> signal\n}");
        let StatementKind::Task(t) = &s.kind else {
            panic!("expected Task")
        };
        let TaskBody::Pipeline(p) = &t.body else {
            panic!("expected Pipeline")
        };
        assert_eq!(p.lines[0].sink.as_ref().unwrap().buffer.name, "signal");
    }

    // ── actor_call args ──

    #[test]
    fn actor_call_no_args() {
        let s = parse_one_stmt("clock 1kHz t {\n  mag()\n}");
        let StatementKind::Task(t) = &s.kind else {
            panic!("expected Task")
        };
        let TaskBody::Pipeline(p) = &t.body else {
            panic!("expected Pipeline")
        };
        let PipeSource::ActorCall(a) = &p.lines[0].source else {
            panic!("expected ActorCall")
        };
        assert!(a.args.is_empty());
    }

    #[test]
    fn actor_call_with_param_ref() {
        let s = parse_one_stmt("clock 1kHz t {\n  mul($gain)\n}");
        let StatementKind::Task(t) = &s.kind else {
            panic!("expected Task")
        };
        let TaskBody::Pipeline(p) = &t.body else {
            panic!("expected Pipeline")
        };
        let PipeSource::ActorCall(a) = &p.lines[0].source else {
            panic!("expected ActorCall")
        };
        assert!(matches!(&a.args[0], Arg::ParamRef(id) if id.name == "gain"));
    }

    #[test]
    fn actor_call_mixed_args() {
        let s = parse_one_stmt("clock 1kHz t {\n  foo(42, $p, c)\n}");
        let StatementKind::Task(t) = &s.kind else {
            panic!("expected Task")
        };
        let TaskBody::Pipeline(p) = &t.body else {
            panic!("expected Pipeline")
        };
        let PipeSource::ActorCall(a) = &p.lines[0].source else {
            panic!("expected ActorCall")
        };
        assert_eq!(a.args.len(), 3);
        assert!(
            matches!(&a.args[0], Arg::Value(Value::Scalar(Scalar::Number(n, _, _))) if *n == 42.0)
        );
        assert!(matches!(&a.args[1], Arg::ParamRef(id) if id.name == "p"));
        assert!(matches!(&a.args[2], Arg::ConstRef(id) if id.name == "c"));
    }

    #[test]
    fn actor_call_with_tap_ref() {
        let s = parse_one_stmt("clock 1kHz t {\n  add(:fb)\n}");
        let StatementKind::Task(t) = &s.kind else {
            panic!("expected Task")
        };
        let TaskBody::Pipeline(p) = &t.body else {
            panic!("expected Pipeline")
        };
        let PipeSource::ActorCall(a) = &p.lines[0].source else {
            panic!("expected ActorCall")
        };
        assert_eq!(a.args.len(), 1);
        assert!(matches!(&a.args[0], Arg::TapRef(id) if id.name == "fb"));
    }

    #[test]
    fn actor_call_mixed_with_tap_ref() {
        let s = parse_one_stmt("clock 1kHz t {\n  foo(42, :fb, $p)\n}");
        let StatementKind::Task(t) = &s.kind else {
            panic!("expected Task")
        };
        let TaskBody::Pipeline(p) = &t.body else {
            panic!("expected Pipeline")
        };
        let PipeSource::ActorCall(a) = &p.lines[0].source else {
            panic!("expected ActorCall")
        };
        assert_eq!(a.args.len(), 3);
        assert!(
            matches!(&a.args[0], Arg::Value(Value::Scalar(Scalar::Number(n, _, _))) if *n == 42.0)
        );
        assert!(matches!(&a.args[1], Arg::TapRef(id) if id.name == "fb"));
        assert!(matches!(&a.args[2], Arg::ParamRef(id) if id.name == "p"));
    }

    // ── switch_stmt ──

    #[test]
    fn switch_basic() {
        let src = "clock 10MHz rx {\n  control {\n    adc(0) | detect() -> ctrl\n  }\n  mode sync {\n    adc(0) | fir(c)\n  }\n  mode data {\n    adc(0) | fft(256)\n  }\n  switch(ctrl, sync, data)\n}";
        let s = parse_one_stmt(src);
        let StatementKind::Task(t) = &s.kind else {
            panic!("expected Task")
        };
        let TaskBody::Modal(m) = &t.body else {
            panic!("expected Modal")
        };
        assert_eq!(m.modes.len(), 2);
        assert_eq!(m.switch.modes.len(), 2);
        assert!(m.switch.default.is_none());
    }

    #[test]
    fn switch_with_default() {
        let src = "clock 10MHz rx {\n  control {\n    adc(0) | detect() -> ctrl\n  }\n  mode sync {\n    adc(0) | fir(c)\n  }\n  mode data {\n    adc(0) | fft(256)\n  }\n  switch(ctrl, sync, data) default sync\n}";
        let s = parse_one_stmt(src);
        let StatementKind::Task(t) = &s.kind else {
            panic!("expected Task")
        };
        let TaskBody::Modal(m) = &t.body else {
            panic!("expected Modal")
        };
        assert_eq!(m.switch.default.as_ref().unwrap().name, "sync");
    }

    #[test]
    fn switch_param_source() {
        let src = "clock 10MHz rx {\n  control {\n    adc(0) | detect() -> ctrl\n  }\n  mode a {\n    foo()\n  }\n  mode b {\n    bar()\n  }\n  switch($sel, a, b)\n}";
        let s = parse_one_stmt(src);
        let StatementKind::Task(t) = &s.kind else {
            panic!("expected Task")
        };
        let TaskBody::Modal(m) = &t.body else {
            panic!("expected Modal")
        };
        assert!(matches!(&m.switch.source, SwitchSource::Param(id) if id.name == "sel"));
    }

    #[test]
    fn switch_param_source_without_control_block() {
        let src = "clock 10MHz rx {\n  mode a {\n    foo()\n  }\n  mode b {\n    bar()\n  }\n  switch($sel, a, b)\n}";
        let s = parse_one_stmt(src);
        let StatementKind::Task(t) = &s.kind else {
            panic!("expected Task")
        };
        let TaskBody::Modal(m) = &t.body else {
            panic!("expected Modal")
        };
        assert!(m.control.body.lines.is_empty());
        assert!(matches!(&m.switch.source, SwitchSource::Param(id) if id.name == "sel"));
    }

    // ── Multiple statements ──

    #[test]
    fn multiple_statements() {
        let prog = parse_ok("set mem = 64MB\nconst c = 1.0\nparam gain = 1.0");
        assert_eq!(prog.statements.len(), 3);
    }

    // ── Integration ──

    #[test]
    fn example_pdl() {
        let source = include_str!("../../examples/example.pdl");
        let prog = parse_ok(source);
        assert_eq!(prog.statements.len(), 8);
    }

    #[test]
    fn receiver_pdl() {
        let source = include_str!("../../examples/receiver.pdl");
        let prog = parse_ok(source);
        assert_eq!(prog.statements.len(), 6);

        let StatementKind::Task(t) = &prog.statements[4].kind else {
            panic!("expected Task")
        };
        let TaskBody::Modal(m) = &t.body else {
            panic!("expected Modal")
        };
        assert_eq!(m.modes.len(), 2);
        assert!(m.switch.default.is_some());
    }

    // ── Spans ──

    #[test]
    fn spans_set_stmt() {
        let source = "set mem = 64MB";
        let s = parse_one_stmt(source);
        assert_eq!(s.span.start, 0);
        assert_eq!(s.span.end, source.len());
    }

    #[test]
    fn spans_ident() {
        let s = parse_one_stmt("set mem = 64MB");
        let StatementKind::Set(set) = &s.kind else {
            panic!("expected Set")
        };
        assert_eq!(set.name.span.start, 4);
        assert_eq!(set.name.span.end, 7);
    }

    // ── Errors ──

    #[test]
    fn error_bad_statement() {
        let (_, errors) = parse_all("badtoken");
        assert!(!errors.is_empty());
    }

    #[test]
    fn error_missing_equals() {
        let (_, errors) = parse_all("set x 42");
        assert!(!errors.is_empty());
    }

    // ── delay keyword ──

    #[test]
    fn delay_actor_call() {
        let s = parse_one_stmt("clock 1kHz t {\n  adc(0) | delay(10, 0.0)\n}");
        let StatementKind::Task(t) = &s.kind else {
            panic!("expected Task")
        };
        let TaskBody::Pipeline(p) = &t.body else {
            panic!("expected Pipeline")
        };
        assert_eq!(p.lines[0].elements.len(), 1);
        assert!(
            matches!(&p.lines[0].elements[0], PipeElem::ActorCall(a) if a.name.name == "delay")
        );
    }

    #[test]
    fn delay_not_as_variable() {
        let (_, errors) = parse_all("set delay = 1");
        assert!(!errors.is_empty());
    }

    // ── BNF conformance ──

    #[test]
    fn trailing_comma_in_args_rejected() {
        let (_, errors) = parse_all("clock 1kHz t {\n  foo(1,)\n}");
        assert!(!errors.is_empty());
    }

    // ── Shape constraints (v0.2.0) ──

    #[test]
    fn actor_call_with_shape_constraint() {
        let s = parse_one_stmt("clock 1kHz t {\n  fft()[256]\n}");
        let StatementKind::Task(t) = &s.kind else {
            panic!("expected Task")
        };
        let TaskBody::Pipeline(p) = &t.body else {
            panic!("expected Pipeline")
        };
        let PipeSource::ActorCall(a) = &p.lines[0].source else {
            panic!("expected ActorCall")
        };
        let sc = a
            .shape_constraint
            .as_ref()
            .expect("expected shape constraint");
        assert_eq!(sc.dims.len(), 1);
        assert!(matches!(&sc.dims[0], ShapeDim::Literal(256, _)));
    }

    #[test]
    fn actor_call_with_multidim_shape() {
        let s = parse_one_stmt("clock 1kHz t {\n  img_norm()[1080, 1920, 3]\n}");
        let StatementKind::Task(t) = &s.kind else {
            panic!("expected Task")
        };
        let TaskBody::Pipeline(p) = &t.body else {
            panic!("expected Pipeline")
        };
        let PipeSource::ActorCall(a) = &p.lines[0].source else {
            panic!("expected ActorCall")
        };
        let sc = a
            .shape_constraint
            .as_ref()
            .expect("expected shape constraint");
        assert_eq!(sc.dims.len(), 3);
        assert!(matches!(&sc.dims[0], ShapeDim::Literal(1080, _)));
        assert!(matches!(&sc.dims[1], ShapeDim::Literal(1920, _)));
        assert!(matches!(&sc.dims[2], ShapeDim::Literal(3, _)));
    }

    #[test]
    fn actor_call_with_const_ref_shape() {
        let prog = parse_ok("const N = 256\nclock 1kHz t {\n  fft()[N]\n}");
        let StatementKind::Task(t) = &prog.statements[1].kind else {
            panic!("expected Task")
        };
        let TaskBody::Pipeline(p) = &t.body else {
            panic!("expected Pipeline")
        };
        let PipeSource::ActorCall(a) = &p.lines[0].source else {
            panic!("expected ActorCall")
        };
        let sc = a
            .shape_constraint
            .as_ref()
            .expect("expected shape constraint");
        assert_eq!(sc.dims.len(), 1);
        assert!(matches!(&sc.dims[0], ShapeDim::ConstRef(id) if id.name == "N"));
    }

    #[test]
    fn actor_call_without_shape() {
        let s = parse_one_stmt("clock 1kHz t {\n  fft(256)\n}");
        let StatementKind::Task(t) = &s.kind else {
            panic!("expected Task")
        };
        let TaskBody::Pipeline(p) = &t.body else {
            panic!("expected Pipeline")
        };
        let PipeSource::ActorCall(a) = &p.lines[0].source else {
            panic!("expected ActorCall")
        };
        assert!(a.shape_constraint.is_none());
    }

    #[test]
    fn actor_call_with_args_and_shape() {
        let s = parse_one_stmt("clock 1kHz t {\n  fft(256)[256]\n}");
        let StatementKind::Task(t) = &s.kind else {
            panic!("expected Task")
        };
        let TaskBody::Pipeline(p) = &t.body else {
            panic!("expected Pipeline")
        };
        let PipeSource::ActorCall(a) = &p.lines[0].source else {
            panic!("expected ActorCall")
        };
        assert_eq!(a.args.len(), 1);
        let sc = a
            .shape_constraint
            .as_ref()
            .expect("expected shape constraint");
        assert_eq!(sc.dims.len(), 1);
    }

    #[test]
    fn shape_constraint_in_pipe_element() {
        let s = parse_one_stmt("clock 1kHz t {\n  adc(0) | fft()[256] | stdout()\n}");
        let StatementKind::Task(t) = &s.kind else {
            panic!("expected Task")
        };
        let TaskBody::Pipeline(p) = &t.body else {
            panic!("expected Pipeline")
        };
        let PipeElem::ActorCall(a) = &p.lines[0].elements[0] else {
            panic!("expected ActorCall in pipe element")
        };
        let sc = a
            .shape_constraint
            .as_ref()
            .expect("expected shape constraint");
        assert_eq!(sc.dims.len(), 1);
        assert!(matches!(&sc.dims[0], ShapeDim::Literal(256, _)));
    }

    #[test]
    fn shape_constraint_rejects_non_positive_or_fractional_literals() {
        let (_, zero_errs) = parse_all("clock 1kHz t {\n  fft()[0]\n}");
        assert!(
            !zero_errs.is_empty(),
            "shape dim 0 should be rejected as parse error"
        );

        let (_, neg_errs) = parse_all("clock 1kHz t {\n  fft()[-1]\n}");
        assert!(
            !neg_errs.is_empty(),
            "negative shape dim should be rejected as parse error"
        );

        let (_, frac_errs) = parse_all("clock 1kHz t {\n  fft()[1.5]\n}");
        assert!(
            !frac_errs.is_empty(),
            "fractional shape dim should be rejected as parse error"
        );
    }

    // ── Polymorphic actor calls (v0.3.0) ────────────────────────────────

    #[test]
    fn actor_call_with_type_args() {
        let s = parse_one_stmt("clock 1kHz t {\n  fir<float>(coeff)\n}");
        let StatementKind::Task(t) = &s.kind else {
            panic!("expected Task")
        };
        let TaskBody::Pipeline(p) = &t.body else {
            panic!("expected Pipeline")
        };
        let PipeSource::ActorCall(a) = &p.lines[0].source else {
            panic!("expected ActorCall")
        };
        assert_eq!(a.name.name, "fir");
        assert_eq!(a.type_args.len(), 1);
        assert_eq!(a.type_args[0].name, "float");
        assert_eq!(a.args.len(), 1);
    }

    #[test]
    fn actor_call_with_type_args_and_shape() {
        let s = parse_one_stmt("clock 1kHz t {\n  scale<double>(2.0)[256]\n}");
        let StatementKind::Task(t) = &s.kind else {
            panic!("expected Task")
        };
        let TaskBody::Pipeline(p) = &t.body else {
            panic!("expected Pipeline")
        };
        let PipeSource::ActorCall(a) = &p.lines[0].source else {
            panic!("expected ActorCall")
        };
        assert_eq!(a.name.name, "scale");
        assert_eq!(a.type_args.len(), 1);
        assert_eq!(a.type_args[0].name, "double");
        assert!(a.shape_constraint.is_some());
    }

    #[test]
    fn actor_call_without_type_args_is_empty() {
        let s = parse_one_stmt("clock 1kHz t {\n  mul($gain)\n}");
        let StatementKind::Task(t) = &s.kind else {
            panic!("expected Task")
        };
        let TaskBody::Pipeline(p) = &t.body else {
            panic!("expected Pipeline")
        };
        let PipeSource::ActorCall(a) = &p.lines[0].source else {
            panic!("expected ActorCall")
        };
        assert!(a.type_args.is_empty());
    }

    #[test]
    fn actor_call_with_type_args_in_pipe() {
        let s = parse_one_stmt("clock 1kHz t {\n  adc(0) | scale<float>(2.0) | stdout()\n}");
        let StatementKind::Task(t) = &s.kind else {
            panic!("expected Task")
        };
        let TaskBody::Pipeline(p) = &t.body else {
            panic!("expected Pipeline")
        };
        let PipeElem::ActorCall(a) = &p.lines[0].elements[0] else {
            panic!("expected ActorCall in pipe element")
        };
        assert_eq!(a.name.name, "scale");
        assert_eq!(a.type_args.len(), 1);
        assert_eq!(a.type_args[0].name, "float");
    }

    #[test]
    fn actor_call_multiple_type_args() {
        let s = parse_one_stmt("clock 1kHz t {\n  convert<float, double>()\n}");
        let StatementKind::Task(t) = &s.kind else {
            panic!("expected Task")
        };
        let TaskBody::Pipeline(p) = &t.body else {
            panic!("expected Pipeline")
        };
        let PipeSource::ActorCall(a) = &p.lines[0].source else {
            panic!("expected ActorCall")
        };
        assert_eq!(a.type_args.len(), 2);
        assert_eq!(a.type_args[0].name, "float");
        assert_eq!(a.type_args[1].name, "double");
    }

    // ── bind_stmt ──

    #[test]
    fn bind_stmt_udp() {
        let s = parse_one_stmt(r#"bind iq = udp("127.0.0.1:9100", chan=10)"#);
        let StatementKind::Bind(b) = &s.kind else {
            panic!("expected Bind")
        };
        assert_eq!(b.name.name, "iq");
        assert_eq!(b.endpoint.transport.name, "udp");
        assert_eq!(b.endpoint.args.len(), 2);
        assert!(
            matches!(&b.endpoint.args[0], BindArg::Positional(Scalar::StringLit(s, _)) if s == "127.0.0.1:9100")
        );
        assert!(
            matches!(&b.endpoint.args[1], BindArg::Named(ident, Scalar::Number(n, _, _)) if ident.name == "chan" && *n == 10.0)
        );
    }

    #[test]
    fn bind_stmt_shm() {
        let s = parse_one_stmt(r#"bind iq2 = shm("rx.iq", slots=1024, slot_bytes=4096)"#);
        let StatementKind::Bind(b) = &s.kind else {
            panic!("expected Bind")
        };
        assert_eq!(b.name.name, "iq2");
        assert_eq!(b.endpoint.transport.name, "shm");
        assert_eq!(b.endpoint.args.len(), 3);
        assert!(
            matches!(&b.endpoint.args[0], BindArg::Positional(Scalar::StringLit(s, _)) if s == "rx.iq")
        );
        assert!(matches!(&b.endpoint.args[1], BindArg::Named(ident, _) if ident.name == "slots"));
        assert!(
            matches!(&b.endpoint.args[2], BindArg::Named(ident, _) if ident.name == "slot_bytes")
        );
    }

    #[test]
    fn bind_stmt_positional_only() {
        let s = parse_one_stmt(r#"bind x = udp("host:port")"#);
        let StatementKind::Bind(b) = &s.kind else {
            panic!("expected Bind")
        };
        assert_eq!(b.name.name, "x");
        assert_eq!(b.endpoint.transport.name, "udp");
        assert_eq!(b.endpoint.args.len(), 1);
        assert!(matches!(
            &b.endpoint.args[0],
            BindArg::Positional(Scalar::StringLit(..))
        ));
    }

    #[test]
    fn bind_stmt_ident_positional_arg() {
        let s = parse_one_stmt("bind x = udp(addr, chan=10)");
        let StatementKind::Bind(b) = &s.kind else {
            panic!("expected Bind")
        };
        assert_eq!(b.endpoint.args.len(), 2);
        assert!(
            matches!(&b.endpoint.args[0], BindArg::Positional(Scalar::Ident(id)) if id.name == "addr")
        );
        assert!(matches!(&b.endpoint.args[1], BindArg::Named(ident, _) if ident.name == "chan"));
    }
}
