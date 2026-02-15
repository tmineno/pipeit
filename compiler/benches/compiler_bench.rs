use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use pcc::*;

// Sample PDL programs of varying complexity
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

fn create_test_registry() -> registry::Registry {
    // In benchmarks, we won't load actual headers - just use an empty registry
    // This measures parsing/analysis overhead without I/O
    registry::Registry::new()
}

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
                // Parse
                let parse_result = parser::parse(black_box(source));

                if let Some(ref ast) = parse_result.program {
                    // Resolve (skip actor loading for benchmark purity)
                    let resolve_result = resolve::resolve(ast, &registry);
                    if resolve_result
                        .diagnostics
                        .iter()
                        .any(|d| matches!(d.level, resolve::DiagLevel::Error))
                    {
                        return; // Skip if resolution fails (expected without real actors)
                    }

                    // Graph construction
                    let graph_result = graph::build_graph(ast, &resolve_result.resolved, &registry);
                    if graph_result
                        .diagnostics
                        .iter()
                        .any(|d| matches!(d.level, resolve::DiagLevel::Error))
                    {
                        return;
                    }

                    // Analysis would require valid actors, so we stop here for pure compiler overhead
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
                // Measure generated code size as a proxy for codegen complexity
                // In a real scenario, we'd compile and measure the full pipeline
                // Here we just estimate based on AST size
                let parse_result = parser::parse(black_box(source));

                if let Some(ref ast) = parse_result.program {
                    // Count statements as a proxy for code size
                    let statement_count = ast.statements.len();
                    black_box(statement_count >= expected_min_lines / 10); // Rough heuristic
                }
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_parse,
    bench_full_pipeline,
    bench_codegen_size
);
criterion_main!(benches);
