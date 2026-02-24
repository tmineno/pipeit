// Property-based tests for compiler invariants.
//
// Three categories:
// 1. Parser → HIR roundtrip: generated PDL programs parse, resolve, and verify
// 2. Widening transitivity: exhaustive check over all PipitType triples
// 3. Scheduler invariants: generated programs schedule and verify correctly
//
// Uses proptest with explicit configuration to prevent CI flakiness.

use pcc::pass::StageCert;
use proptest::prelude::*;
use std::path::{Path, PathBuf};

// ── Test helpers ────────────────────────────────────────────────────────────

fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn load_full_registry() -> pcc::registry::Registry {
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
    registry
}

// ── PDL generator ───────────────────────────────────────────────────────────

/// Generate a small valid PDL program using only actors known to resolve.
/// Grammar: constant(<f64>) (| (mul|add)(<f64>))* (| stdout())?
/// stdout() is terminal-only (void output cannot feed downstream actors).
fn arb_pdl_program() -> impl Strategy<Value = String> {
    // PDL syntax: `clock <freq_with_unit> <name> { ... }`
    let freq = prop_oneof![Just("1kHz"), Just("10kHz"), Just("48kHz"),];

    // Use bounded floats to avoid extremely small/large literals that
    // produce problematic decimal representations for the parser.
    let bounded_f64 = -1000.0f64..1000.0f64;

    // Generate a single pipe chain
    let pipe_chain = (
        bounded_f64.clone(),
        prop::collection::vec(
            (prop_oneof![Just("mul"), Just("add")], bounded_f64.clone()),
            0..=2,
        ),
        prop::bool::ANY,
    )
        .prop_map(|(init_val, mid_actors, has_stdout)| {
            let mut chain = format!("constant({})", format_f64(init_val));
            for (actor, val) in &mid_actors {
                chain.push_str(&format!(" | {}({})", actor, format_f64(*val)));
            }
            if has_stdout {
                chain.push_str(" | stdout()");
            }
            chain
        });

    // Generate optional const/param declarations
    let has_const = prop::bool::ANY;
    let has_param = prop::bool::ANY;
    let const_val = bounded_f64.clone();
    let param_val = bounded_f64;

    (
        has_const,
        const_val,
        has_param,
        param_val,
        prop::collection::vec(
            (
                freq.clone(),
                prop::collection::vec(pipe_chain.clone(), 1..=2),
            ),
            1..=2,
        ),
    )
        .prop_map(|(has_const, const_val, has_param, param_val, tasks)| {
            let mut pdl = String::new();

            if has_const {
                pdl.push_str(&format!("const c = {}\n", format_f64(const_val)));
            }
            if has_param {
                pdl.push_str(&format!("param p = {}\n", format_f64(param_val)));
            }

            for (i, (freq, chains)) in tasks.iter().enumerate() {
                pdl.push_str(&format!("clock {} t{} {{\n", freq, i));
                for chain in chains {
                    pdl.push_str(&format!("  {}\n", chain));
                }
                pdl.push_str("}\n");
            }

            pdl
        })
}

/// Format f64 as a valid PDL literal, avoiding special values.
fn format_f64(v: f64) -> String {
    if v.is_nan() || v.is_infinite() || v == 0.0 {
        "0".to_string()
    } else {
        format!("{}", v)
    }
}

// ── 5a. Parser → HIR roundtrip ─────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 100,
        max_shrink_iters: 200,
        .. ProptestConfig::default()
    })]

    #[test]
    fn parser_hir_roundtrip(pdl in arb_pdl_program()) {
        let registry = load_full_registry();

        // Parse succeeds
        let parse_result = pcc::parser::parse(&pdl);
        prop_assert!(
            parse_result.program.is_some(),
            "parse failed for PDL:\n{}\nerrors: {:?}",
            pdl,
            parse_result.errors
        );
        let program = parse_result.program.unwrap();

        // Resolve produces no error-level diagnostics
        let mut resolve_result = pcc::resolve::resolve(&program, &registry);
        let resolve_errors: Vec<_> = resolve_result
            .diagnostics
            .iter()
            .filter(|d| d.level == pcc::diag::DiagLevel::Error)
            .collect();
        prop_assert!(
            resolve_errors.is_empty(),
            "resolve errors for PDL:\n{}\nerrors: {:?}",
            pdl,
            resolve_errors
        );

        // build_hir succeeds and verify_hir passes
        let hir = pcc::hir::build_hir(
            &program,
            &resolve_result.resolved,
            &mut resolve_result.id_alloc,
        );
        let cert = pcc::hir::verify_hir(&hir, &resolve_result.resolved);
        prop_assert!(
            cert.all_pass(),
            "HIR verification failed for PDL:\n{}\nobligations: {:?}",
            pdl,
            cert.obligations()
        );
    }
}

// ── 5b. Widening transitivity (exhaustive) ─────────────────────────────────

#[test]
fn widening_transitivity_and_antisymmetry() {
    use pcc::registry::PipitType;
    use pcc::type_infer::can_widen;

    let all_types = [
        PipitType::Int8,
        PipitType::Int16,
        PipitType::Int32,
        PipitType::Float,
        PipitType::Double,
        PipitType::Cfloat,
        PipitType::Cdouble,
        PipitType::Void,
    ];

    // Transitivity: if can_widen(a, b) && can_widen(b, c) then can_widen(a, c)
    for &a in &all_types {
        for &b in &all_types {
            for &c in &all_types {
                if can_widen(a, b) && can_widen(b, c) {
                    assert!(
                        can_widen(a, c),
                        "transitivity violated: can_widen({:?}, {:?}) && can_widen({:?}, {:?}) \
                         but !can_widen({:?}, {:?})",
                        a,
                        b,
                        b,
                        c,
                        a,
                        c,
                    );
                }
            }
        }
    }

    // Antisymmetry: if can_widen(a, b) && a != b then !can_widen(b, a)
    for &a in &all_types {
        for &b in &all_types {
            if can_widen(a, b) && a != b {
                assert!(
                    !can_widen(b, a),
                    "antisymmetry violated: can_widen({:?}, {:?}) && {:?} != {:?} \
                     but can_widen({:?}, {:?})",
                    a,
                    b,
                    a,
                    b,
                    b,
                    a,
                );
            }
        }
    }
}

// ── 5c. Scheduler invariants ────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 50,
        max_shrink_iters: 100,
        .. ProptestConfig::default()
    })]

    #[test]
    fn scheduler_invariants(pdl in arb_pdl_program()) {
        let registry = load_full_registry();

        // Parse
        let parse_result = pcc::parser::parse(&pdl);
        prop_assert!(parse_result.program.is_some());
        let program = parse_result.program.unwrap();

        // Resolve
        let mut resolve_result = pcc::resolve::resolve(&program, &registry);
        let has_resolve_error = resolve_result
            .diagnostics
            .iter()
            .any(|d| d.level == pcc::diag::DiagLevel::Error);
        prop_assume!(!has_resolve_error);

        // HIR
        let hir = pcc::hir::build_hir(
            &program,
            &resolve_result.resolved,
            &mut resolve_result.id_alloc,
        );

        // Type infer
        let type_result = pcc::type_infer::type_infer(
            &hir,
            &resolve_result.resolved,
            &registry,
        );
        let has_type_error = type_result
            .diagnostics
            .iter()
            .any(|d| d.level == pcc::diag::DiagLevel::Error);
        prop_assume!(!has_type_error);

        // Lower
        let lower_result = pcc::lower::lower_and_verify(
            &hir,
            &resolve_result.resolved,
            &type_result.typed,
            &registry,
        );
        prop_assume!(!lower_result.has_errors());

        // Graph
        let graph_result = pcc::graph::build_graph(
            &hir,
            &resolve_result.resolved,
            &registry,
        );
        let has_graph_error = graph_result
            .diagnostics
            .iter()
            .any(|d| d.level == pcc::diag::DiagLevel::Error);
        prop_assume!(!has_graph_error);

        // ThirContext
        let thir = pcc::thir::build_thir_context(
            &hir,
            &resolve_result.resolved,
            &type_result.typed,
            &lower_result.lowered,
            &registry,
            &graph_result.graph,
        );

        // Analyze
        let analysis_result = pcc::analyze::analyze(&thir, &graph_result.graph);
        let has_analysis_error = analysis_result
            .diagnostics
            .iter()
            .any(|d| d.level == pcc::diag::DiagLevel::Error);
        prop_assume!(!has_analysis_error);

        // Schedule
        let schedule_result = pcc::schedule::schedule(
            &thir,
            &graph_result.graph,
            &analysis_result.analysis,
        );
        let has_sched_error = schedule_result
            .diagnostics
            .iter()
            .any(|d| d.level == pcc::diag::DiagLevel::Error);
        prop_assume!(!has_sched_error);

        // Property: verify_schedule passes
        let task_names: Vec<String> = hir.tasks.iter().map(|t| t.name.clone()).collect();
        let sched_cert = pcc::schedule::verify_schedule(
            &schedule_result.schedule,
            &graph_result.graph,
            &task_names,
        );
        prop_assert!(
            sched_cert.all_pass(),
            "schedule verification failed for PDL:\n{}\nobligations: {:?}",
            pdl,
            sched_cert.obligations()
        );

        // Property: K-factor >= 1 for every task
        for (name, task_sched) in &schedule_result.schedule.tasks {
            prop_assert!(
                task_sched.k_factor >= 1,
                "K-factor < 1 for task '{}' in PDL:\n{}",
                name,
                pdl
            );
        }

        // Property: every task name in schedule matches a task name in HIR
        let hir_task_names: std::collections::HashSet<&str> =
            hir.tasks.iter().map(|t| t.name.as_str()).collect();
        for name in schedule_result.schedule.tasks.keys() {
            prop_assert!(
                hir_task_names.contains(name.as_str()),
                "schedule task '{}' not in HIR for PDL:\n{}",
                name,
                pdl
            );
        }
    }
}
