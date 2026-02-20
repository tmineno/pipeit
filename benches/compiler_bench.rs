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
    let std_actors = project_root
        .join("runtime")
        .join("libpipit")
        .join("include")
        .join("std_actors.h");
    let example_actors = project_root.join("examples").join("example_actors.h");

    if std_actors.exists() {
        let _ = reg.load_header(&std_actors);
    }
    if example_actors.exists() {
        let _ = reg.load_header(&example_actors);
    }

    reg
}

fn has_errors(diags: &[resolve::Diagnostic]) -> bool {
    diags
        .iter()
        .any(|d| matches!(d.level, resolve::DiagLevel::Error))
}

fn compile_full(source: &str, registry: &registry::Registry, opts: &codegen::CodegenOptions) {
    let parse_result = parser::parse(source);
    let ast = parse_result
        .program
        .as_ref()
        .expect("benchmark scenario must parse");

    let resolve_result = resolve::resolve(ast, registry);
    assert!(!has_errors(&resolve_result.diagnostics));

    let graph_result = graph::build_graph(ast, &resolve_result.resolved, registry);
    assert!(!has_errors(&graph_result.diagnostics));

    let analysis_result =
        analyze::analyze(ast, &resolve_result.resolved, &graph_result.graph, registry);
    assert!(!has_errors(&analysis_result.diagnostics));

    let schedule_result = schedule::schedule(
        ast,
        &resolve_result.resolved,
        &graph_result.graph,
        &analysis_result.analysis,
        registry,
    );
    assert!(!has_errors(&schedule_result.diagnostics));

    let generated = codegen::codegen(
        ast,
        &resolve_result.resolved,
        &graph_result.graph,
        &analysis_result.analysis,
        &schedule_result.schedule,
        registry,
        opts,
    );

    black_box(generated);
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
    };
    let source = COMPLEX_PIPELINE;

    // parse
    {
        let mut group = c.benchmark_group("kpi/phase_latency/parse");
        group.bench_function("complex", |b| {
            b.iter(|| {
                let r = parser::parse(black_box(source));
                black_box(&r.program);
            });
        });
        group.finish();
    }

    // resolve (setup: parse)
    {
        let mut group = c.benchmark_group("kpi/phase_latency/resolve");
        group.bench_function("complex", |b| {
            b.iter_batched(
                || parser::parse(source),
                |parse_result| {
                    let ast = parse_result
                        .program
                        .as_ref()
                        .expect("benchmark scenario must parse");
                    let r = resolve::resolve(black_box(ast), &registry);
                    black_box(&r.resolved);
                },
                BatchSize::SmallInput,
            );
        });
        group.finish();
    }

    // graph (setup: parse + resolve)
    {
        let mut group = c.benchmark_group("kpi/phase_latency/graph");
        group.bench_function("complex", |b| {
            b.iter_batched(
                || {
                    let pr = parser::parse(source);
                    let ast = pr.program.unwrap();
                    let rr = resolve::resolve(&ast, &registry);
                    (ast, rr)
                },
                |(ast, rr)| {
                    assert!(!has_errors(&rr.diagnostics));
                    let r = graph::build_graph(black_box(&ast), black_box(&rr.resolved), &registry);
                    black_box(&r.graph);
                },
                BatchSize::SmallInput,
            );
        });
        group.finish();
    }

    // analyze (setup: parse + resolve + graph)
    {
        let mut group = c.benchmark_group("kpi/phase_latency/analyze");
        group.bench_function("complex", |b| {
            b.iter_batched(
                || {
                    let pr = parser::parse(source);
                    let ast = pr.program.unwrap();
                    let rr = resolve::resolve(&ast, &registry);
                    let gr = graph::build_graph(&ast, &rr.resolved, &registry);
                    (ast, rr, gr)
                },
                |(ast, rr, gr)| {
                    assert!(!has_errors(&rr.diagnostics));
                    assert!(!has_errors(&gr.diagnostics));
                    let r = analyze::analyze(
                        black_box(&ast),
                        black_box(&rr.resolved),
                        black_box(&gr.graph),
                        &registry,
                    );
                    black_box(&r.analysis);
                },
                BatchSize::SmallInput,
            );
        });
        group.finish();
    }

    // schedule (setup: parse + resolve + graph + analyze)
    {
        let mut group = c.benchmark_group("kpi/phase_latency/schedule");
        group.bench_function("complex", |b| {
            b.iter_batched(
                || {
                    let pr = parser::parse(source);
                    let ast = pr.program.unwrap();
                    let rr = resolve::resolve(&ast, &registry);
                    let gr = graph::build_graph(&ast, &rr.resolved, &registry);
                    let ar = analyze::analyze(&ast, &rr.resolved, &gr.graph, &registry);
                    (ast, rr, gr, ar)
                },
                |(ast, rr, gr, ar)| {
                    assert!(!has_errors(&rr.diagnostics));
                    assert!(!has_errors(&gr.diagnostics));
                    assert!(!has_errors(&ar.diagnostics));
                    let r = schedule::schedule(
                        black_box(&ast),
                        black_box(&rr.resolved),
                        black_box(&gr.graph),
                        black_box(&ar.analysis),
                        &registry,
                    );
                    black_box(&r.schedule);
                },
                BatchSize::SmallInput,
            );
        });
        group.finish();
    }

    // codegen (setup: all prior phases)
    {
        let mut group = c.benchmark_group("kpi/phase_latency/codegen");
        group.bench_function("complex", |b| {
            b.iter_batched(
                || {
                    let pr = parser::parse(source);
                    let ast = pr.program.unwrap();
                    let rr = resolve::resolve(&ast, &registry);
                    let gr = graph::build_graph(&ast, &rr.resolved, &registry);
                    let ar = analyze::analyze(&ast, &rr.resolved, &gr.graph, &registry);
                    let sr =
                        schedule::schedule(&ast, &rr.resolved, &gr.graph, &ar.analysis, &registry);
                    (ast, rr, gr, ar, sr)
                },
                |(ast, rr, gr, ar, sr)| {
                    assert!(!has_errors(&rr.diagnostics));
                    assert!(!has_errors(&gr.diagnostics));
                    assert!(!has_errors(&ar.diagnostics));
                    assert!(!has_errors(&sr.diagnostics));
                    let result = codegen::codegen(
                        black_box(&ast),
                        black_box(&rr.resolved),
                        black_box(&gr.graph),
                        black_box(&ar.analysis),
                        black_box(&sr.schedule),
                        &registry,
                        &opts,
                    );
                    black_box(&result);
                },
                BatchSize::SmallInput,
            );
        });
        group.finish();
    }
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
