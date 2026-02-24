// Snapshot tests: lock THIR precomputed metadata to detect unintended changes.
//
// ThirContext is a borrow-aggregation wrapper that performs three unique
// precomputed transformations not captured by HIR or LIR snapshots:
// 1. param_cpp_types — resolved C++ types for runtime params
// 2. Extracted set-directive values (mem_bytes, tick_rate_hz, etc.)
// 3. Index consistency (task/const/param/set lookup tables)
//
// Uses snapshot_summary() to serialize these layers into a deterministic string
// with all map-derived keys sorted alphabetically.
//
// Run `cargo insta review` after intentional output changes to update baselines.

use std::path::{Path, PathBuf};

fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

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

/// Run parse → resolve → build_hir → type_infer → lower → graph → build_thir_context
/// and return the THIR snapshot summary string.
fn thir_snapshot(source: &str, registry: &pcc::registry::Registry) -> String {
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
            .all(|d| d.level != pcc::diag::DiagLevel::Error),
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
            .all(|d| d.level != pcc::diag::DiagLevel::Error),
        "type_infer errors: {:?}",
        type_result.diagnostics
    );

    let lower_result =
        pcc::lower::lower_and_verify(&hir, &resolve_result.resolved, &type_result.typed, registry);
    assert!(
        !lower_result.has_errors(),
        "lower errors: {:?}",
        lower_result.diagnostics
    );

    let graph_result = pcc::graph::build_graph(&hir, &resolve_result.resolved, registry);
    assert!(
        graph_result
            .diagnostics
            .iter()
            .all(|d| d.level != pcc::diag::DiagLevel::Error),
        "graph errors: {:?}",
        graph_result.diagnostics
    );

    let thir = pcc::thir::build_thir_context(
        &hir,
        &resolve_result.resolved,
        &type_result.typed,
        &lower_result.lowered,
        registry,
        &graph_result.graph,
    );

    thir.snapshot_summary()
}

fn snapshot_example(name: &str) {
    let root = project_root();
    let pdl_path = root.join("examples").join(name);
    let source = std::fs::read_to_string(&pdl_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {}", pdl_path.display(), e));
    let (registry, _headers) = load_full_registry();
    let output = thir_snapshot(&source, &registry);
    assert!(!output.is_empty(), "empty THIR summary for {}", name);
    let snap_name = format!("thir_{}", name.replace('.', "_"));
    insta::assert_snapshot!(snap_name, output);
}

#[test]
fn snapshot_thir_gain() {
    snapshot_example("gain.pdl");
}

#[test]
fn snapshot_thir_multirate() {
    snapshot_example("multirate.pdl");
}

#[test]
fn snapshot_thir_feedback() {
    snapshot_example("feedback.pdl");
}

#[test]
fn snapshot_thir_example_pdl() {
    snapshot_example("example.pdl");
}

#[test]
fn snapshot_thir_receiver() {
    snapshot_example("receiver.pdl");
}

#[test]
fn snapshot_thir_complex() {
    snapshot_example("complex.pdl");
}

#[test]
fn snapshot_thir_socket_stream() {
    snapshot_example("socket_stream.pdl");
}
