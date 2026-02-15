use criterion::{black_box, criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion};
use pcc::*;
use std::path::Path;

// ── Existing sample PDL programs ────────────────────────────────────────────

const SIMPLE_PIPELINE: &str = r#"
clock 1kHz task {
    adc(0) | mul(2.0) | stdout()
}
"#;

const MEDIUM_PIPELINE: &str = r#"
const coeff = [0.1, 0.2, 0.4, 0.2, 0.1]
param gain = 1.0

clock 10MHz capture {
    adc(0) | mul($gain) | fft(256) | :raw | fir(coeff) | ?filtered -> signal
    :raw | mag() | stdout()
}

clock 1kHz drain {
    @signal | decimate(10000) | csvwrite("output.csv")
}
"#;

const COMPLEX_PIPELINE: &str = r#"
const sync_coeff = [1.0, -1.0, 1.0, -1.0]
const demod_coeff = [0.05, 0.1, 0.15, 0.2, 0.25, 0.2, 0.15, 0.1, 0.05]
param carrier_freq = 1000.0
param gain = 1.0

define process() {
    mul($gain) | fft(256) | c2r() | fir(5, sync_coeff)
}

clock 1MHz rx {
    adc(0) | :raw | process() | ?demod -> baseband
    :raw | mag() | stdout()
}

clock 100kHz demod {
    @baseband | fir(9, demod_coeff) | decimate(10) -> symbols
}

clock 10kHz decode {
    @symbols | detect() | csvwrite("output.csv")
}

clock 1kHz stats {
    @symbols | rms() | stdout()
}
"#;

const MODAL_PIPELINE: &str = r#"
param mode_sel = 0

clock 1kHz adaptive {
    control {
        const(0) | delay(1, $mode_sel)
    }
    mode 0 {
        adc(0) | mul(1.0) | stdout()
    }
    mode 1 {
        adc(0) | mul(2.0) | fft(256) | c2r() | stdout()
    }
    switch
}
"#;

// ── v0.2.1 stress-test pipeline generators ──────────────────────────────────

/// Generate a large pipeline with many actors across multiple tasks.
///
/// Layout: n_tasks tasks, each with a linear chain of actors_per_task actors.
/// Adjacent tasks communicate via shared buffers.
fn generate_large_pipeline(n_tasks: usize, actors_per_task: usize) -> String {
    let mut pdl = String::new();
    pdl.push_str("param gain = 1.0\n\n");

    for t in 0..n_tasks {
        let freq = if t % 2 == 0 { "100kHz" } else { "50kHz" };
        pdl.push_str(&format!("clock {} task_{} {{\n", freq, t));

        // Source: either adc or shared buffer from previous task
        if t == 0 {
            pdl.push_str("    adc(0)");
        } else {
            pdl.push_str(&format!("    @buf_{}", t - 1));
        }

        // Chain of actors: alternate mul/add for variety
        for a in 0..actors_per_task {
            if a % 2 == 0 {
                pdl.push_str(" | mul($gain)");
            } else {
                pdl.push_str(" | add(1.0)");
            }
        }

        // Sink: either shared buffer to next task or stdout
        if t < n_tasks - 1 {
            pdl.push_str(&format!(" -> buf_{}\n", t));
        } else {
            pdl.push_str(" | stdout()\n");
        }

        pdl.push_str("}\n\n");
    }

    pdl
}

/// Generate a deeply nested pipeline using define blocks (5+ levels).
fn generate_deep_nesting(n_levels: usize) -> String {
    let mut pdl = String::new();

    pdl.push_str("define level_0() {\n    mul(1.0) | add(0.5)\n}\n\n");

    for level in 1..n_levels {
        pdl.push_str(&format!(
            "define level_{}() {{\n    level_{}() | mul(1.{}) | add(0.{})\n}}\n\n",
            level,
            level - 1,
            level,
            level
        ));
    }

    pdl.push_str(&format!(
        "clock 1kHz task {{\n    adc(0) | level_{}() | stdout()\n}}\n",
        n_levels - 1
    ));

    pdl
}

/// Generate a wide fan-out pipeline (single source → N consumers via taps).
fn generate_wide_fanout(n_consumers: usize) -> String {
    let mut pdl = String::new();
    pdl.push_str("clock 1kHz task {\n");
    pdl.push_str("    adc(0) | :src | stdout()\n");

    for i in 0..n_consumers {
        let factor = 1.0 + (i as f64) * 0.01;
        pdl.push_str(&format!("    :src | mul({:.2}) | stdout()\n", factor));
    }

    pdl.push_str("}\n");
    pdl
}

/// Generate a modal pipeline with many modes.
fn generate_modal_complex(n_modes: usize) -> String {
    let mut pdl = String::new();

    for i in 0..n_modes {
        pdl.push_str(&format!("param ctrl_{} = 0\n", i));
    }
    pdl.push('\n');

    pdl.push_str("clock 1kHz adaptive {\n");
    pdl.push_str("    control {\n");
    pdl.push_str("        const(0) | delay(1, $ctrl_0)\n");
    pdl.push_str("    }\n");

    for m in 0..n_modes {
        pdl.push_str(&format!("    mode {} {{\n", m));
        let factor = 1.0 + (m as f64) * 0.5;
        pdl.push_str(&format!(
            "        adc(0) | mul({:.1}) | add({:.1}) | stdout()\n",
            factor, m as f64
        ));
        pdl.push_str("    }\n");
    }

    pdl.push_str("    switch\n");
    pdl.push_str("}\n");

    pdl
}

// ── Registry setup ──────────────────────────────────────────────────────────

fn create_test_registry() -> registry::Registry {
    registry::Registry::new()
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

// ── Existing benchmarks (unchanged) ─────────────────────────────────────────

fn bench_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse");

    for (name, source) in [
        ("simple", SIMPLE_PIPELINE),
        ("medium", MEDIUM_PIPELINE),
        ("complex", COMPLEX_PIPELINE),
        ("modal", MODAL_PIPELINE),
    ] {
        group.bench_with_input(BenchmarkId::from_parameter(name), source, |b, source| {
            b.iter(|| {
                let result = parser::parse(black_box(source));
                black_box(&result.program);
            });
        });
    }

    group.finish();
}

fn bench_full_pipeline(c: &mut Criterion) {
    let mut group = c.benchmark_group("full_pipeline");

    let registry = create_test_registry();

    for (name, source) in [
        ("simple", SIMPLE_PIPELINE),
        ("medium", MEDIUM_PIPELINE),
        ("complex", COMPLEX_PIPELINE),
        ("modal", MODAL_PIPELINE),
    ] {
        group.bench_with_input(BenchmarkId::from_parameter(name), source, |b, source| {
            b.iter(|| {
                let parse_result = parser::parse(black_box(source));

                if let Some(ref ast) = parse_result.program {
                    let resolve_result = resolve::resolve(ast, &registry);
                    if has_errors(&resolve_result.diagnostics) {
                        return;
                    }

                    let graph_result = graph::build_graph(ast, &resolve_result.resolved, &registry);
                    if has_errors(&graph_result.diagnostics) {
                        return;
                    }

                    black_box(&graph_result);
                }
            });
        });
    }

    group.finish();
}

fn bench_codegen_size(c: &mut Criterion) {
    let mut group = c.benchmark_group("codegen_size");

    for (name, source, expected_min_lines) in [
        ("simple", SIMPLE_PIPELINE, 50),
        ("medium", MEDIUM_PIPELINE, 150),
        ("complex", COMPLEX_PIPELINE, 250),
    ] {
        group.bench_with_input(BenchmarkId::from_parameter(name), source, |b, _| {
            b.iter(|| {
                let parse_result = parser::parse(black_box(source));

                if let Some(ref ast) = parse_result.program {
                    let statement_count = ast.statements.len();
                    black_box(statement_count >= expected_min_lines / 10);
                }
            });
        });
    }

    group.finish();
}

// ── v0.2.1: Stress-test parse benchmarks ────────────────────────────────────

fn bench_parse_stress(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_stress");

    let large = generate_large_pipeline(20, 5);
    let deep = generate_deep_nesting(5);
    let fanout = generate_wide_fanout(50);
    let modal = generate_modal_complex(10);

    for (name, source) in [
        ("large_20t_5a", large.as_str()),
        ("deep_5_levels", deep.as_str()),
        ("fanout_50", fanout.as_str()),
        ("modal_10_modes", modal.as_str()),
    ] {
        group.bench_with_input(BenchmarkId::from_parameter(name), &source, |b, source| {
            b.iter(|| {
                let result = parser::parse(black_box(source));
                black_box(&result.program);
            });
        });
    }

    group.finish();
}

// ── v0.2.1: Full pipeline with loaded registry ──────────────────────────────

fn bench_full_pipeline_loaded(c: &mut Criterion) {
    let mut group = c.benchmark_group("full_pipeline_loaded");

    let registry = create_loaded_registry();
    let opts = codegen::CodegenOptions {
        release: false,
        include_paths: vec![],
    };

    for (name, source) in [
        ("simple", SIMPLE_PIPELINE),
        ("medium", MEDIUM_PIPELINE),
        ("complex", COMPLEX_PIPELINE),
        ("modal", MODAL_PIPELINE),
    ] {
        group.bench_with_input(BenchmarkId::from_parameter(name), source, |b, source| {
            b.iter(|| {
                let parse_result = parser::parse(black_box(source));
                if let Some(ref ast) = parse_result.program {
                    let resolve_result = resolve::resolve(ast, &registry);
                    if has_errors(&resolve_result.diagnostics) {
                        return;
                    }
                    let graph_result = graph::build_graph(ast, &resolve_result.resolved, &registry);
                    if has_errors(&graph_result.diagnostics) {
                        return;
                    }
                    let analysis_result = analyze::analyze(
                        ast,
                        &resolve_result.resolved,
                        &graph_result.graph,
                        &registry,
                    );
                    if has_errors(&analysis_result.diagnostics) {
                        return;
                    }
                    let schedule_result = schedule::schedule(
                        ast,
                        &resolve_result.resolved,
                        &graph_result.graph,
                        &analysis_result.analysis,
                        &registry,
                    );
                    if has_errors(&schedule_result.diagnostics) {
                        return;
                    }
                    let result = codegen::codegen(
                        ast,
                        &resolve_result.resolved,
                        &graph_result.graph,
                        &analysis_result.analysis,
                        &schedule_result.schedule,
                        &registry,
                        &opts,
                    );
                    black_box(&result);
                }
            });
        });
    }

    group.finish();
}

// ── v0.2.1: Per-phase benchmarks ────────────────────────────────────────────
//
// Uses iter_batched to re-run preceding phases in setup (not timed), avoiding
// Clone requirements on intermediate types.

fn bench_per_phase(c: &mut Criterion) {
    let registry = create_loaded_registry();
    let opts = codegen::CodegenOptions {
        release: false,
        include_paths: vec![],
    };

    let sources: &[(&str, &str)] = &[("simple", SIMPLE_PIPELINE), ("complex", COMPLEX_PIPELINE)];

    // Phase: parse
    {
        let mut group = c.benchmark_group("phase/parse");
        for &(name, source) in sources {
            group.bench_function(BenchmarkId::from_parameter(name), |b| {
                b.iter(|| {
                    let r = parser::parse(black_box(source));
                    black_box(&r.program);
                });
            });
        }
        group.finish();
    }

    // Phase: resolve (setup: parse)
    {
        let mut group = c.benchmark_group("phase/resolve");
        for &(name, source) in sources {
            group.bench_function(BenchmarkId::from_parameter(name), |b| {
                b.iter_batched(
                    || parser::parse(source),
                    |parse_result| {
                        if let Some(ref ast) = parse_result.program {
                            let r = resolve::resolve(black_box(ast), &registry);
                            black_box(&r.resolved);
                        }
                    },
                    BatchSize::SmallInput,
                );
            });
        }
        group.finish();
    }

    // Phase: graph (setup: parse + resolve)
    {
        let mut group = c.benchmark_group("phase/graph");
        for &(name, source) in sources {
            group.bench_function(BenchmarkId::from_parameter(name), |b| {
                b.iter_batched(
                    || {
                        let pr = parser::parse(source);
                        let ast = pr.program.unwrap();
                        let rr = resolve::resolve(&ast, &registry);
                        (ast, rr)
                    },
                    |(ast, rr)| {
                        if has_errors(&rr.diagnostics) {
                            return;
                        }
                        let r =
                            graph::build_graph(black_box(&ast), black_box(&rr.resolved), &registry);
                        black_box(&r.graph);
                    },
                    BatchSize::SmallInput,
                );
            });
        }
        group.finish();
    }

    // Phase: analyze (setup: parse + resolve + graph)
    {
        let mut group = c.benchmark_group("phase/analyze");
        for &(name, source) in sources {
            group.bench_function(BenchmarkId::from_parameter(name), |b| {
                b.iter_batched(
                    || {
                        let pr = parser::parse(source);
                        let ast = pr.program.unwrap();
                        let rr = resolve::resolve(&ast, &registry);
                        let gr = graph::build_graph(&ast, &rr.resolved, &registry);
                        (ast, rr, gr)
                    },
                    |(ast, rr, gr)| {
                        if has_errors(&rr.diagnostics) || has_errors(&gr.diagnostics) {
                            return;
                        }
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
        }
        group.finish();
    }

    // Phase: schedule (setup: parse + resolve + graph + analyze)
    {
        let mut group = c.benchmark_group("phase/schedule");
        for &(name, source) in sources {
            group.bench_function(BenchmarkId::from_parameter(name), |b| {
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
                        if has_errors(&rr.diagnostics)
                            || has_errors(&gr.diagnostics)
                            || has_errors(&ar.diagnostics)
                        {
                            return;
                        }
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
        }
        group.finish();
    }

    // Phase: codegen (setup: all prior phases)
    {
        let mut group = c.benchmark_group("phase/codegen");
        for &(name, source) in sources {
            group.bench_function(BenchmarkId::from_parameter(name), |b| {
                b.iter_batched(
                    || {
                        let pr = parser::parse(source);
                        let ast = pr.program.unwrap();
                        let rr = resolve::resolve(&ast, &registry);
                        let gr = graph::build_graph(&ast, &rr.resolved, &registry);
                        let ar = analyze::analyze(&ast, &rr.resolved, &gr.graph, &registry);
                        let sr = schedule::schedule(
                            &ast,
                            &rr.resolved,
                            &gr.graph,
                            &ar.analysis,
                            &registry,
                        );
                        (ast, rr, gr, ar, sr)
                    },
                    |(ast, rr, gr, ar, sr)| {
                        if has_errors(&rr.diagnostics)
                            || has_errors(&gr.diagnostics)
                            || has_errors(&ar.diagnostics)
                            || has_errors(&sr.diagnostics)
                        {
                            return;
                        }
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
        }
        group.finish();
    }
}

// ── v0.2.1: Pipeline scaling benchmark ──────────────────────────────────────

fn bench_parse_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_scaling");

    for n_tasks in [1, 5, 10, 20, 50] {
        let source = generate_large_pipeline(n_tasks, 5);
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
    bench_parse,
    bench_full_pipeline,
    bench_codegen_size,
    bench_parse_stress,
    bench_full_pipeline_loaded,
    bench_per_phase,
    bench_parse_scaling,
);
criterion_main!(benches);
