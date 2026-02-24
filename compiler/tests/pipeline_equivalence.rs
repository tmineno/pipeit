// Pipeline equivalence tests: verify that pass-manager orchestration produces
// identical C++ output to direct function-call chains.
//
// For each example PDL, compile two ways:
// 1. Direct: parse → resolve → build_hir → type_infer → lower → graph →
//    thir → analyze → schedule → build_lir → codegen_from_lir
// 2. Orchestrated: CompilationState::new → run_pipeline(Codegen)
//
// Assert byte-identical C++ output. This catches bugs in: pass ordering,
// artifact threading, borrow-split logic, ThirContext scoping, codegen
// option handling, and verification wiring.

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

/// Build CodegenOptions with realistic provenance and include paths.
fn build_codegen_options(
    source: &str,
    registry: &pcc::registry::Registry,
) -> pcc::codegen::CodegenOptions {
    let root = project_root();
    let provenance = pcc::pipeline::compute_provenance(source, registry);
    pcc::codegen::CodegenOptions {
        release: false,
        include_paths: vec![root.join("runtime/libpipit/include"), root.join("examples")],
        provenance: Some(provenance),
    }
}

/// Compile via direct function-call chain (same as snapshot_codegen::full_pipeline_cpp
/// but with realistic CodegenOptions).
fn direct_compile(source: &str, registry: &pcc::registry::Registry) -> String {
    let opts = build_codegen_options(source, registry);

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
    let analysis_result = pcc::analyze::analyze(&thir, &graph_result.graph);
    assert!(
        analysis_result
            .diagnostics
            .iter()
            .all(|d| d.level != pcc::diag::DiagLevel::Error),
        "analysis errors: {:?}",
        analysis_result.diagnostics
    );

    let schedule_result =
        pcc::schedule::schedule(&thir, &graph_result.graph, &analysis_result.analysis);
    assert!(
        schedule_result
            .diagnostics
            .iter()
            .all(|d| d.level != pcc::diag::DiagLevel::Error),
        "schedule errors: {:?}",
        schedule_result.diagnostics
    );

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
            .all(|d| d.level != pcc::diag::DiagLevel::Error),
        "codegen errors: {:?}",
        codegen_result.diagnostics
    );

    codegen_result.generated.cpp_source
}

/// Compile via pass-manager orchestration.
fn orchestrated_compile(
    source: &str,
    registry: pcc::registry::Registry,
    opts: &pcc::codegen::CodegenOptions,
) -> String {
    let parse_result = pcc::parser::parse(source);
    assert!(
        parse_result.errors.is_empty(),
        "parse errors: {:?}",
        parse_result.errors
    );
    let program = parse_result.program.unwrap();

    let mut state = pcc::pipeline::CompilationState::new(program, registry);
    let result = pcc::pipeline::run_pipeline(
        &mut state,
        pcc::pass::PassId::Codegen,
        opts,
        false,
        |_, _| {},
    );
    assert!(
        result.is_ok(),
        "pipeline error: {:?}, diagnostics: {:?}",
        result.err(),
        state.diagnostics
    );
    assert!(
        !state.has_error,
        "pipeline has_error: {:?}",
        state.diagnostics
    );

    state
        .downstream
        .generated
        .expect("codegen output missing")
        .cpp_source
}

fn assert_equivalence(name: &str) {
    let root = project_root();
    let pdl_path = root.join("examples").join(name);
    let source = std::fs::read_to_string(&pdl_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {}", pdl_path.display(), e));

    let (registry_direct, _) = load_full_registry();
    let direct_output = direct_compile(&source, &registry_direct);

    // Load a fresh registry for the orchestrated path (it takes ownership)
    let (registry_orch, _) = load_full_registry();
    let opts = build_codegen_options(&source, &registry_orch);
    let orch_output = orchestrated_compile(&source, registry_orch, &opts);

    assert_eq!(
        direct_output, orch_output,
        "Pipeline equivalence failed for {}: direct vs orchestrated C++ output differs",
        name
    );
}

#[test]
fn equivalence_gain() {
    assert_equivalence("gain.pdl");
}

#[test]
fn equivalence_multirate() {
    assert_equivalence("multirate.pdl");
}

#[test]
fn equivalence_feedback() {
    assert_equivalence("feedback.pdl");
}

#[test]
fn equivalence_example_pdl() {
    assert_equivalence("example.pdl");
}

#[test]
fn equivalence_receiver() {
    assert_equivalence("receiver.pdl");
}

#[test]
fn equivalence_complex() {
    assert_equivalence("complex.pdl");
}

#[test]
fn equivalence_socket_stream() {
    assert_equivalence("socket_stream.pdl");
}
