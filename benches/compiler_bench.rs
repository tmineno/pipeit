use criterion::{black_box, criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion};
use pcc::*;
use std::path::Path;

// KPI-aligned benchmark scenarios.
// All scenarios are valid with std_actors + example_actors loaded.

const SIMPLE_PIPELINE: &str = r#"
clock 1kHz task {
    stdin() | abs() | stdout()
}
"#;

const MULTITASK_PIPELINE: &str = r#"
param gain = 1.0

clock 10kHz capture {
    constant(0.0)[256] | mul($gain) | fft(256) | c2r() | mean(256) -> signal
}

clock 1kHz drain {
    @signal | decimate(10) | stdout()
}
"#;

const COMPLEX_PIPELINE: &str = r#"
const coeff = [0.1, 0.2, 0.4, 0.2, 0.1]
param gain = 1.0

define preprocess() {
    mul($gain) | fir(coeff)
}

clock 20kHz producer {
    constant(0.0)[10] | preprocess() | decimate(10) -> shared
}

clock 2kHz consumer {
    @shared | decimate(10) | stdout()
}
"#;

const MODAL_PIPELINE: &str = r#"
clock 1kHz adaptive {
    control {
        constant(0.0) | detect() -> ctrl
    }
    mode low {
        constant(0.0) | abs() | stdout()
    }
    mode high {
        constant(0.0)[256] | mul(1.0) | fft(256) | c2r() | mean(256) | stdout()
    }
    switch(ctrl, low, high) default low
}
"#;

fn scenarios() -> [(&'static str, &'static str); 4] {
    [
        ("simple", SIMPLE_PIPELINE),
        ("multitask", MULTITASK_PIPELINE),
        ("complex", COMPLEX_PIPELINE),
        ("modal", MODAL_PIPELINE),
    ]
}

/// Parse-scaling generator used for compile scalability KPI.
/// All tasks are rate-compatible and use only known actors.
fn generate_scaling_pipeline(n_tasks: usize) -> String {
    let mut pdl = String::new();

    for t in 0..n_tasks {
        pdl.push_str(&format!("clock 1kHz task_{} {{\n", t));

        if t == 0 {
            pdl.push_str("    stdin()");
        } else {
            pdl.push_str(&format!("    @buf_{}", t - 1));
        }

        // 1:1 actor chain keeps SDF/rate constraints simple and valid.
        pdl.push_str(" | abs()");

        if t < n_tasks - 1 {
            pdl.push_str(&format!(" -> buf_{}\n", t));
        } else {
            pdl.push_str(" | stdout()\n");
        }

        pdl.push_str("}\n\n");
    }

    pdl
}

fn create_loaded_registry() -> registry::Registry {
    let mut reg = registry::Registry::new();
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let project_root = manifest_dir.parent().unwrap();
    let include_dir = project_root
        .join("runtime")
        .join("libpipit")
        .join("include");
    let std_actors = include_dir.join("std_actors.h");
    let std_math = include_dir.join("std_math.h");
    let example_actors = project_root.join("examples").join("example_actors.h");

    if std_actors.exists() {
        let _ = reg.load_header(&std_actors);
    }
    if std_math.exists() {
        let _ = reg.load_header(&std_math);
    }
    if example_actors.exists() {
        let _ = reg.load_header(&example_actors);
    }

    reg
}

fn has_errors(diags: &[diag::Diagnostic]) -> bool {
    diags
        .iter()
        .any(|d| matches!(d.level, diag::DiagLevel::Error))
}

fn assert_no_errors(stage: &str, diags: &[diag::Diagnostic]) {
    assert!(
        !has_errors(diags),
        "{stage} produced diagnostics: {diags:#?}"
    );
}

fn bench_phase<I, Setup, Run>(c: &mut Criterion, phase: &str, mut setup: Setup, mut run: Run)
where
    Setup: FnMut() -> I,
    Run: FnMut(I),
{
    let mut group = c.benchmark_group(format!("kpi/phase_latency/{phase}"));
    group.bench_function("complex", |b| {
        b.iter_batched(&mut setup, &mut run, BatchSize::SmallInput);
    });
    group.finish();
}

fn compile_full(source: &str, registry: &registry::Registry, opts: &codegen::CodegenOptions) {
    let parse_result = parser::parse(source);
    let ast = parse_result
        .program
        .as_ref()
        .expect("benchmark scenario must parse");

    let mut resolve_result = resolve::resolve(ast, registry);
    assert_no_errors("resolve", &resolve_result.diagnostics);

    let hir_program = hir::build_hir(ast, &resolve_result.resolved, &mut resolve_result.id_alloc);
    let graph_result = graph::build_graph(&hir_program, &resolve_result.resolved, registry);
    assert_no_errors("graph", &graph_result.diagnostics);

    let type_result = type_infer::type_infer(&hir_program, &resolve_result.resolved, registry);
    let lower_result = lower::lower_and_verify(
        &hir_program,
        &resolve_result.resolved,
        &type_result.typed,
        registry,
    );
    let thir = thir::build_thir_context(
        &hir_program,
        &resolve_result.resolved,
        &type_result.typed,
        &lower_result.lowered,
        registry,
        &graph_result.graph,
    );
    let analysis_result = analyze::analyze(&thir, &graph_result.graph);
    assert_no_errors("analyze", &analysis_result.diagnostics);

    let schedule_result = schedule::schedule(&thir, &graph_result.graph, &analysis_result.analysis);
    assert_no_errors("schedule", &schedule_result.diagnostics);

    let lir = lir::build_lir(
        &thir,
        &graph_result.graph,
        &analysis_result.analysis,
        &schedule_result.schedule,
    );
    let generated =
        codegen::codegen_from_lir(&graph_result.graph, &schedule_result.schedule, opts, &lir);

    black_box(generated);
}

fn parse_ast(source: &str) -> ast::Program {
    parser::parse(source)
        .program
        .expect("benchmark scenario must parse")
}

fn bench_parse_phase(c: &mut Criterion, source: &str) {
    bench_phase(
        c,
        "parse",
        || source,
        |src| {
            let r = parser::parse(black_box(src));
            black_box(&r.program);
        },
    );
}

fn bench_resolve_phase(c: &mut Criterion, source: &str, registry: &registry::Registry) {
    bench_phase(
        c,
        "resolve",
        || parse_ast(source),
        |ast| {
            let r = resolve::resolve(black_box(&ast), registry);
            black_box(&r.resolved);
        },
    );
}

fn bench_graph_phase(c: &mut Criterion, source: &str, registry: &registry::Registry) {
    bench_phase(
        c,
        "graph",
        || {
            let ast = parse_ast(source);
            let mut rr = resolve::resolve(&ast, registry);
            let hir = hir::build_hir(&ast, &rr.resolved, &mut rr.id_alloc);
            (hir, rr)
        },
        |(hir, rr)| {
            assert_no_errors("resolve", &rr.diagnostics);
            let r = graph::build_graph(black_box(&hir), black_box(&rr.resolved), registry);
            black_box(&r.graph);
        },
    );
}

fn bench_analyze_phase(c: &mut Criterion, source: &str, registry: &registry::Registry) {
    // Build all upstream artifacts once; ThirContext borrows from them so we
    // cannot use iter_batched (self-referential).  analyze is a pure function
    // of immutable inputs, so reusing the same inputs is valid.
    let ast = parse_ast(source);
    let mut rr = resolve::resolve(&ast, registry);
    let hir = hir::build_hir(&ast, &rr.resolved, &mut rr.id_alloc);
    let gr = graph::build_graph(&hir, &rr.resolved, registry);
    assert_no_errors("resolve", &rr.diagnostics);
    assert_no_errors("graph", &gr.diagnostics);
    let tr = type_infer::type_infer(&hir, &rr.resolved, registry);
    let lr = lower::lower_and_verify(&hir, &rr.resolved, &tr.typed, registry);
    let thir = thir::build_thir_context(
        &hir,
        &rr.resolved,
        &tr.typed,
        &lr.lowered,
        registry,
        &gr.graph,
    );

    let mut group = c.benchmark_group("kpi/phase_latency/analyze");
    group.bench_function("complex", |b| {
        b.iter(|| {
            let r = analyze::analyze(black_box(&thir), black_box(&gr.graph));
            black_box(&r.analysis);
        });
    });
    group.finish();
}

fn bench_schedule_phase(c: &mut Criterion, source: &str, registry: &registry::Registry) {
    bench_phase(
        c,
        "schedule",
        || {
            let ast = parse_ast(source);
            let mut rr = resolve::resolve(&ast, registry);
            let hir = hir::build_hir(&ast, &rr.resolved, &mut rr.id_alloc);
            let gr = graph::build_graph(&hir, &rr.resolved, registry);
            let tr = type_infer::type_infer(&hir, &rr.resolved, registry);
            let lr = lower::lower_and_verify(&hir, &rr.resolved, &tr.typed, registry);
            // Build ThirContext + analyze in setup (ThirContext rebuilt in run closure)
            let thir_setup = thir::build_thir_context(
                &hir,
                &rr.resolved,
                &tr.typed,
                &lr.lowered,
                registry,
                &gr.graph,
            );
            let ar = analyze::analyze(&thir_setup, &gr.graph);
            (rr, hir, gr, tr, lr, ar)
        },
        |(rr, hir, gr, tr, lr, ar)| {
            assert_no_errors("resolve", &rr.diagnostics);
            assert_no_errors("graph", &gr.diagnostics);
            assert_no_errors("analyze", &ar.diagnostics);
            let thir = thir::build_thir_context(
                &hir,
                &rr.resolved,
                &tr.typed,
                &lr.lowered,
                registry,
                &gr.graph,
            );
            let r = schedule::schedule(
                black_box(&thir),
                black_box(&gr.graph),
                black_box(&ar.analysis),
            );
            black_box(&r.schedule);
        },
    );
}

fn bench_codegen_phase(
    c: &mut Criterion,
    source: &str,
    registry: &registry::Registry,
    opts: &codegen::CodegenOptions,
) {
    // Legacy composite benchmark (build_thir_context + build_lir + codegen_from_lir).
    // Kept for trend continuity during migration to decomposed buckets.
    bench_phase(
        c,
        "codegen",
        || {
            let ast = parse_ast(source);
            let mut rr = resolve::resolve(&ast, registry);
            let hir = hir::build_hir(&ast, &rr.resolved, &mut rr.id_alloc);
            let gr = graph::build_graph(&hir, &rr.resolved, registry);
            let tr = type_infer::type_infer(&hir, &rr.resolved, registry);
            let lr = lower::lower_and_verify(&hir, &rr.resolved, &tr.typed, registry);
            let thir_setup = thir::build_thir_context(
                &hir,
                &rr.resolved,
                &tr.typed,
                &lr.lowered,
                registry,
                &gr.graph,
            );
            let ar = analyze::analyze(&thir_setup, &gr.graph);
            let sr = schedule::schedule(&thir_setup, &gr.graph, &ar.analysis);
            // Build LIR in setup — ThirContext is rebuilt in run closure
            let lir_setup = lir::build_lir(&thir_setup, &gr.graph, &ar.analysis, &sr.schedule);
            (ast, rr, gr, ar, sr, hir, tr, lr, lir_setup)
        },
        |(_ast, rr, gr, ar, sr, hir, tr, lr, _lir_setup)| {
            assert_no_errors("resolve", &rr.diagnostics);
            assert_no_errors("graph", &gr.diagnostics);
            assert_no_errors("analyze", &ar.diagnostics);
            assert_no_errors("schedule", &sr.diagnostics);
            // Rebuild ThirContext + LIR in run closure so benchmark measures codegen
            let thir = thir::build_thir_context(
                &hir,
                &rr.resolved,
                &tr.typed,
                &lr.lowered,
                registry,
                &gr.graph,
            );
            let lir = lir::build_lir(&thir, &gr.graph, &ar.analysis, &sr.schedule);
            let result = codegen::codegen_from_lir(
                black_box(&gr.graph),
                black_box(&sr.schedule),
                opts,
                black_box(&lir),
            );
            black_box(&result);
        },
    );
}

// ── Decomposed codegen sub-phase benchmarks ────────────────────────────────
// Each bucket isolates exactly one function call so that latency attribution
// is unambiguous. See TODO.md v0.4.5 "Benchmark Decomposition".

fn bench_build_thir_context_phase(c: &mut Criterion, source: &str, registry: &registry::Registry) {
    bench_phase(
        c,
        "build_thir_context",
        || {
            let ast = parse_ast(source);
            let mut rr = resolve::resolve(&ast, registry);
            let hir = hir::build_hir(&ast, &rr.resolved, &mut rr.id_alloc);
            let gr = graph::build_graph(&hir, &rr.resolved, registry);
            let tr = type_infer::type_infer(&hir, &rr.resolved, registry);
            let lr = lower::lower_and_verify(&hir, &rr.resolved, &tr.typed, registry);
            (rr, hir, gr, tr, lr)
        },
        |(rr, hir, gr, tr, lr)| {
            assert_no_errors("resolve", &rr.diagnostics);
            assert_no_errors("graph", &gr.diagnostics);
            let thir = thir::build_thir_context(
                black_box(&hir),
                black_box(&rr.resolved),
                black_box(&tr.typed),
                black_box(&lr.lowered),
                registry,
                black_box(&gr.graph),
            );
            black_box(&thir);
        },
    );
}

fn bench_build_lir_phase(c: &mut Criterion, source: &str, registry: &registry::Registry) {
    // Build all upstream artifacts once; ThirContext borrows from them so we
    // cannot use iter_batched (self-referential).  build_lir is a pure
    // function of immutable inputs, so reusing the same inputs is valid.
    let ast = parse_ast(source);
    let mut rr = resolve::resolve(&ast, registry);
    let hir = hir::build_hir(&ast, &rr.resolved, &mut rr.id_alloc);
    let gr = graph::build_graph(&hir, &rr.resolved, registry);
    assert_no_errors("graph", &gr.diagnostics);
    let tr = type_infer::type_infer(&hir, &rr.resolved, registry);
    let lr = lower::lower_and_verify(&hir, &rr.resolved, &tr.typed, registry);
    let thir = thir::build_thir_context(
        &hir,
        &rr.resolved,
        &tr.typed,
        &lr.lowered,
        registry,
        &gr.graph,
    );
    let ar = analyze::analyze(&thir, &gr.graph);
    assert_no_errors("analyze", &ar.diagnostics);
    let sr = schedule::schedule(&thir, &gr.graph, &ar.analysis);
    assert_no_errors("schedule", &sr.diagnostics);

    let mut group = c.benchmark_group("kpi/phase_latency/build_lir");
    group.bench_function("complex", |b| {
        b.iter(|| {
            let lir = lir::build_lir(
                black_box(&thir),
                black_box(&gr.graph),
                black_box(&ar.analysis),
                black_box(&sr.schedule),
            );
            black_box(&lir);
        });
    });
    group.finish();
}

fn bench_emit_cpp_phase(
    c: &mut Criterion,
    source: &str,
    registry: &registry::Registry,
    opts: &codegen::CodegenOptions,
) {
    bench_phase(
        c,
        "emit_cpp",
        || {
            let ast = parse_ast(source);
            let mut rr = resolve::resolve(&ast, registry);
            let hir = hir::build_hir(&ast, &rr.resolved, &mut rr.id_alloc);
            let gr = graph::build_graph(&hir, &rr.resolved, registry);
            let tr = type_infer::type_infer(&hir, &rr.resolved, registry);
            let lr = lower::lower_and_verify(&hir, &rr.resolved, &tr.typed, registry);
            let thir = thir::build_thir_context(
                &hir,
                &rr.resolved,
                &tr.typed,
                &lr.lowered,
                registry,
                &gr.graph,
            );
            let ar = analyze::analyze(&thir, &gr.graph);
            let sr = schedule::schedule(&thir, &gr.graph, &ar.analysis);
            let lir = lir::build_lir(&thir, &gr.graph, &ar.analysis, &sr.schedule);
            (gr, sr, lir)
        },
        |(gr, sr, lir)| {
            let result = codegen::codegen_from_lir(
                black_box(&gr.graph),
                black_box(&sr.schedule),
                opts,
                black_box(&lir),
            );
            black_box(&result);
        },
    );
}

// KPI: parser latency for representative scenarios.
fn bench_kpi_parse_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("kpi/parse_latency");

    for (name, source) in scenarios() {
        group.bench_with_input(BenchmarkId::from_parameter(name), source, |b, source| {
            b.iter(|| {
                let result = parser::parse(black_box(source));
                black_box(&result.program);
            });
        });
    }

    group.finish();
}

// KPI: full compile latency (parse -> resolve -> graph -> analyze -> schedule -> codegen).
fn bench_kpi_full_compile_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("kpi/full_compile_latency");
    let registry = create_loaded_registry();
    let opts = codegen::CodegenOptions {
        release: false,
        include_paths: vec![],
        provenance: None,
        experimental: false,
    };

    for (name, source) in scenarios() {
        group.bench_with_input(BenchmarkId::from_parameter(name), source, |b, source| {
            b.iter(|| compile_full(black_box(source), &registry, &opts));
        });
    }

    group.finish();
}

// KPI: phase-level latency on a non-trivial program.
fn bench_kpi_phase_latency(c: &mut Criterion) {
    let registry = create_loaded_registry();
    let opts = codegen::CodegenOptions {
        release: false,
        include_paths: vec![],
        provenance: None,
        experimental: false,
    };
    let source = COMPLEX_PIPELINE;
    bench_parse_phase(c, source);
    bench_resolve_phase(c, source, &registry);
    bench_graph_phase(c, source, &registry);
    bench_analyze_phase(c, source, &registry);
    bench_schedule_phase(c, source, &registry);
    bench_codegen_phase(c, source, &registry, &opts);
    // Decomposed codegen sub-phases (v0.4.5)
    bench_build_thir_context_phase(c, source, &registry);
    bench_build_lir_phase(c, source, &registry);
    bench_emit_cpp_phase(c, source, &registry, &opts);
}

// KPI: parser scaling vs number of tasks.
fn bench_kpi_parse_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("kpi/parse_scaling");

    for n_tasks in [1_usize, 5, 10, 20, 40] {
        let source = generate_scaling_pipeline(n_tasks);
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}tasks", n_tasks)),
            &source,
            |b, source| {
                b.iter(|| {
                    let r = parser::parse(black_box(source.as_str()));
                    black_box(&r.program);
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_kpi_parse_latency,
    bench_kpi_full_compile_latency,
    bench_kpi_phase_latency,
    bench_kpi_parse_scaling,
);
criterion_main!(benches);
