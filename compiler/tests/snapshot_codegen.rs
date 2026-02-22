// Snapshot tests: lock generated C++ output to detect unintended behavior changes.
//
// Uses the library API (parse → resolve → type_infer → lower → graph → analyze
// → schedule → codegen) directly. Snapshots are managed by `insta` and stored
// under `compiler/tests/snapshots/`.
//
// Run `cargo insta review` after intentional output changes to update baselines.

use std::path::{Path, PathBuf};

fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

/// Load a registry with all standard library + example actor headers.
fn load_full_registry() -> (pcc::registry::Registry, Vec<PathBuf>) {
    let root = project_root();
    let include_dir = root.join("runtime/libpipit/include");

    let headers: Vec<PathBuf> = vec![
        include_dir.join("std_actors.h"),
        include_dir.join("std_math.h"),
        include_dir.join("std_sink.h"),
        include_dir.join("std_source.h"),
        root.join("examples/example_actors.h"),
    ];

    let mut registry = pcc::registry::Registry::new();
    for h in &headers {
        if h.exists() {
            registry
                .load_header(h)
                .unwrap_or_else(|e| panic!("failed to load {}: {:?}", h.display(), e));
        }
    }
    (registry, headers)
}

/// Run the full compiler pipeline on PDL source and return generated C++.
fn full_pipeline_cpp(source: &str, registry: &pcc::registry::Registry) -> String {
    let parse_result = pcc::parser::parse(source);
    assert!(
        parse_result.errors.is_empty(),
        "parse errors: {:?}",
        parse_result.errors
    );
    let program = parse_result.program.unwrap();

    let mut resolve_result = pcc::resolve::resolve(&program, registry);
    assert!(
        resolve_result
            .diagnostics
            .iter()
            .all(|d| d.level != pcc::resolve::DiagLevel::Error),
        "resolve errors: {:?}",
        resolve_result.diagnostics
    );

    let hir = pcc::hir::build_hir(
        &program,
        &resolve_result.resolved,
        &mut resolve_result.id_alloc,
    );
    let type_result = pcc::type_infer::type_infer(&hir, &resolve_result.resolved, registry);
    assert!(
        type_result
            .diagnostics
            .iter()
            .all(|d| d.level != pcc::resolve::DiagLevel::Error),
        "type_infer errors: {:?}",
        type_result.diagnostics
    );

    let lower_result = pcc::lower::lower_and_verify(
        &program,
        &resolve_result.resolved,
        &type_result.typed,
        registry,
    );
    assert!(
        !lower_result.has_errors(),
        "lower errors: {:?}",
        lower_result.diagnostics
    );
    assert!(
        lower_result.cert.all_pass(),
        "lowering verification failed (L1-L5)"
    );

    let hir_program = pcc::hir::build_hir(
        &program,
        &resolve_result.resolved,
        &mut resolve_result.id_alloc,
    );
    let graph_result = pcc::graph::build_graph(&hir_program, &resolve_result.resolved, registry);
    assert!(
        graph_result
            .diagnostics
            .iter()
            .all(|d| d.level != pcc::resolve::DiagLevel::Error),
        "graph errors: {:?}",
        graph_result.diagnostics
    );

    let thir = pcc::thir::build_thir_context(
        &hir_program,
        &resolve_result.resolved,
        &type_result.typed,
        &lower_result.lowered,
        registry,
        &graph_result.graph,
    );
    let analysis_result = pcc::analyze::analyze(&thir, &graph_result.graph);
    assert!(
        analysis_result
            .diagnostics
            .iter()
            .all(|d| d.level != pcc::resolve::DiagLevel::Error),
        "analysis errors: {:?}",
        analysis_result.diagnostics
    );

    let schedule_result =
        pcc::schedule::schedule(&thir, &graph_result.graph, &analysis_result.analysis);
    assert!(
        schedule_result
            .diagnostics
            .iter()
            .all(|d| d.level != pcc::resolve::DiagLevel::Error),
        "schedule errors: {:?}",
        schedule_result.diagnostics
    );

    let opts = pcc::codegen::CodegenOptions {
        release: false,
        include_paths: vec![],
    };
    let lir = pcc::lir::build_lir(
        &thir,
        &graph_result.graph,
        &analysis_result.analysis,
        &schedule_result.schedule,
    );
    let codegen_result =
        pcc::codegen::codegen_from_lir(&graph_result.graph, &schedule_result.schedule, &opts, &lir);
    assert!(
        codegen_result
            .diagnostics
            .iter()
            .all(|d| d.level != pcc::resolve::DiagLevel::Error),
        "codegen errors: {:?}",
        codegen_result.diagnostics
    );

    codegen_result.generated.cpp_source
}

fn snapshot_example(name: &str) {
    let root = project_root();
    let pdl_path = root.join("examples").join(name);
    let source = std::fs::read_to_string(&pdl_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {}", pdl_path.display(), e));
    let (registry, _headers) = load_full_registry();
    let cpp = full_pipeline_cpp(&source, &registry);
    assert!(!cpp.is_empty(), "empty C++ output for {}", name);
    let snap_name = name.replace('.', "_");
    insta::assert_snapshot!(snap_name, cpp);
}

// ── Per-example snapshot tests ────────────────────────────────────────────

#[test]
fn snapshot_gain() {
    snapshot_example("gain.pdl");
}

#[test]
fn snapshot_multirate() {
    snapshot_example("multirate.pdl");
}

#[test]
fn snapshot_feedback() {
    snapshot_example("feedback.pdl");
}

#[test]
fn snapshot_example_pdl() {
    snapshot_example("example.pdl");
}

#[test]
fn snapshot_receiver() {
    snapshot_example("receiver.pdl");
}

#[test]
fn snapshot_complex() {
    snapshot_example("complex.pdl");
}

#[test]
fn snapshot_socket_stream() {
    snapshot_example("socket_stream.pdl");
}
