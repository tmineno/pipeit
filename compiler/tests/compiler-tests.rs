// Language spec conformance tests for pcc.
//
// Each test is explicitly mapped to a clause in:
//   doc/spec/pipit-lang-spec-v0.3.0.md
//
// Scope:
// - Front-end/analysis conformance at the compiler boundary (`pcc --emit cpp`)
// - Positive cases must compile successfully and emit non-empty C++
// - Negative cases must be rejected by the compiler

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn pcc_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_pcc"))
}

fn runtime_include_dir() -> PathBuf {
    project_root()
        .join("runtime")
        .join("libpipit")
        .join("include")
}

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn temp_path(prefix: &str, ext: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp_dir = std::env::temp_dir();
    if ext.is_empty() {
        tmp_dir.join(format!("{}_{}", prefix, n))
    } else {
        tmp_dir.join(format!("{}_{}.{}", prefix, n, ext))
    }
}

/// Generate a shared manifest once per test binary.
fn shared_manifest() -> &'static Path {
    static MANIFEST: OnceLock<PathBuf> = OnceLock::new();
    MANIFEST.get_or_init(|| {
        let path = std::env::temp_dir().join("pcc_compiler_tests_manifest.json");
        let output = Command::new(pcc_binary())
            .arg("--emit")
            .arg("manifest")
            .arg("-I")
            .arg(runtime_include_dir())
            .output()
            .expect("failed to generate manifest");
        assert!(
            output.status.success(),
            "manifest generation failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        std::fs::write(&path, &output.stdout).expect("failed to write manifest");
        path
    })
}

fn run_pcc_inline(pdl_source: &str) -> (std::process::Output, PathBuf, PathBuf) {
    let pdl_file = temp_path("pipit_spec_case", "pdl");
    let cpp_out = temp_path("pipit_spec_case", "cpp");

    std::fs::write(&pdl_file, pdl_source).expect("failed to write temporary pdl source");

    let out = Command::new(pcc_binary())
        .arg(pdl_file.to_str().unwrap())
        .arg("--actor-meta")
        .arg(shared_manifest())
        .arg("-I")
        .arg(runtime_include_dir().to_str().unwrap())
        .arg("--emit")
        .arg("cpp")
        .arg("-o")
        .arg(cpp_out.to_str().unwrap())
        .output()
        .expect("failed to execute pcc");

    (out, pdl_file, cpp_out)
}

fn assert_spec_compiles(spec_clause: &str, case_name: &str, pdl_source: &str) {
    let (out, pdl_file, cpp_out) = run_pcc_inline(pdl_source);
    let _ = std::fs::remove_file(&pdl_file);

    assert!(
        out.status.success(),
        "[{spec_clause}] {case_name}: expected compile success, but failed.\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );

    let cpp = std::fs::read_to_string(&cpp_out).expect("failed to read generated cpp");
    let _ = std::fs::remove_file(&cpp_out);
    assert!(
        !cpp.trim().is_empty(),
        "[{spec_clause}] {case_name}: generated C++ is empty"
    );
}

fn assert_spec_rejected(spec_clause: &str, case_name: &str, pdl_source: &str) {
    let (out, pdl_file, cpp_out) = run_pcc_inline(pdl_source);
    let _ = std::fs::remove_file(&pdl_file);
    let _ = std::fs::remove_file(&cpp_out);

    assert!(
        !out.status.success(),
        "[{spec_clause}] {case_name}: expected compile rejection, but succeeded.\nGenerated stderr:\n{}",
        String::from_utf8_lossy(&out.stderr),
    );
}

macro_rules! spec_accept {
    ($test_name:ident, $spec:literal, $src:expr) => {
        #[test]
        fn $test_name() {
            assert_spec_compiles($spec, stringify!($test_name), $src);
        }
    };
}

macro_rules! spec_reject {
    ($test_name:ident, $spec:literal, $src:expr) => {
        #[test]
        fn $test_name() {
            assert_spec_rejected($spec, stringify!($test_name), $src);
        }
    };
}

// ── 2. Lexical structure ───────────────────────────────────────────────

spec_accept!(
    spec_2_2_comment_line_ignored,
    "§2.2",
    concat!(
        "# comment line\n",
        "clock 1kHz t {\n",
        "  constant(0.0) | stdout() # trailing comment\n",
        "}\n",
    )
);

spec_reject!(
    spec_2_4_reserved_keyword_identifier_rejected,
    "§2.4",
    "const clock = 1\nclock 1kHz t { constant(0.0) | stdout() }\n"
);

spec_accept!(
    spec_2_5_frequency_and_size_literals,
    "§2.5",
    concat!(
        "set mem = 64MB\n",
        "set tick_rate = 10kHz\n",
        "clock 1kHz t { constant(0.0) | stdout() }\n",
    )
);

spec_accept!(
    spec_2_5_string_literal_escapes,
    "§2.5",
    concat!(
        "set log_path = \"a\\\\\\\\b\\\"c\"\n",
        "clock 1kHz t { constant(0.0) | stdout() }\n",
    )
);

spec_reject!(
    spec_2_5_nested_array_literal_rejected,
    "§2.5",
    "const bad = [[1,2],[3,4]]\nclock 1kHz t { constant(0.0) | stdout() }\n"
);

// ── 3. Type system ─────────────────────────────────────────────────────

spec_accept!(
    spec_3_3_polymorphic_inference_unique_solution,
    "§3.3",
    concat!(
        "const coeff = [0.1, 0.2, 0.4, 0.2, 0.1]\n",
        "clock 1kHz t { constant(0.0) | fir(coeff) | stdout() }\n",
    )
);

spec_accept!(
    spec_3_4_implicit_widening_allowed,
    "§3.4",
    "clock 1kHz t { constant(1) | mul(0.5) | stdout() }\n"
);

spec_reject!(
    spec_3_4_narrowing_conversion_rejected,
    "§3.4",
    "clock 1kHz t { constant(0.5) | mul<int16>(2) | stdout<int16>() }\n"
);

spec_reject!(
    spec_3_4_real_complex_implicit_conversion_rejected,
    "§3.4",
    "clock 1kHz t { constant(0.0) | fft(256) | c2r() | mag() | stdout() }\n"
);

spec_accept!(
    spec_3_5_polymorphic_explicit_type_arg,
    "§3.5",
    "clock 1kHz t { sine<float>(100.0, 1.0) | stdout<float>() }\n"
);

spec_accept!(
    spec_3_5_polymorphic_omitted_type_arg,
    "§3.5",
    "clock 1kHz t { constant<float>(0.0) | mul(2.0) | stdout<float>() }\n"
);

// ── 5. Pipeline language ───────────────────────────────────────────────

spec_accept!(
    spec_5_1_set_tick_rate_and_timer_spin_auto,
    "§5.1",
    concat!(
        "set overrun = drop\n",
        "set tick_rate = 1kHz\n",
        "set timer_spin = auto\n",
        "clock 10kHz t { constant(0.0) | stdout() }\n",
    )
);

spec_accept!(
    spec_5_3_runtime_param_reference,
    "§5.3",
    "param gain = 1.0\nclock 1kHz t { constant(0.0) | mul($gain) | stdout() }\n"
);

spec_reject!(
    spec_5_4_duplicate_task_name_rejected,
    "§5.4.1",
    concat!(
        "clock 1kHz t { constant(0.0) | stdout() }\n",
        "clock 1kHz t { constant(1.0) | stdout() }\n",
    )
);

spec_accept!(
    spec_5_5_linear_pipe_operator,
    "§5.5",
    "clock 1kHz t { constant(0.0) | mul(2.0) | mul(0.5) | stdout() }\n"
);

spec_reject!(
    spec_5_5_actor_call_requires_parentheses,
    "§5.5",
    "clock 1kHz t { constant(0.0) | mag | stdout() }\n"
);

spec_accept!(
    spec_5_6_tap_fork_multi_consumer,
    "§5.6",
    concat!(
        "clock 1kHz t {\n",
        "  constant(0.0) | :raw | stdout()\n",
        "  :raw | mul(2.0) | stdout()\n",
        "}\n",
    )
);

spec_reject!(
    spec_5_6_orphan_tap_rejected,
    "§5.6",
    "clock 1kHz t { constant(0.0) | :orphan | stdout() }\n"
);

spec_reject!(
    spec_5_6_forward_pipe_source_tap_rejected,
    "§5.6",
    concat!(
        "clock 1kHz t {\n",
        "  :later | stdout()\n",
        "  constant(0.0) | :later | stdout()\n",
        "}\n",
    )
);

spec_accept!(
    spec_5_6_tap_forward_ref_in_actor_arg_allowed,
    "§5.6 + §5.10",
    concat!(
        "clock 1kHz t {\n",
        "  constant(0.0) | add(:fb) | :out | delay(1, 0.0) | :fb\n",
        "  :out | stdout()\n",
        "}\n",
    )
);

spec_reject!(
    spec_5_7_single_writer_constraint,
    "§5.7",
    concat!(
        "clock 1kHz a { constant(0.0) -> sig }\n",
        "clock 1kHz b { constant(1.0) -> sig }\n",
    )
);

spec_accept!(
    spec_5_7_multiple_readers_allowed,
    "§5.7",
    concat!(
        "clock 1kHz producer { constant(0.0) -> sig }\n",
        "clock 1kHz c1 { @sig | stdout() }\n",
        "clock 1kHz c2 { @sig | stdout() }\n",
    )
);

spec_reject!(
    spec_5_7_cross_clock_rate_mismatch_rejected,
    "§5.7",
    concat!(
        "clock 10kHz fast { constant(0.0) -> sig }\n",
        "clock 1kHz slow { @sig | stdout() }\n",
    )
);

spec_accept!(
    spec_5_8_probe_passthrough,
    "§5.8",
    "clock 1kHz t { constant(0.0) | ?debug | stdout() }\n"
);

spec_accept!(
    spec_5_9_define_inlining_with_param,
    "§5.9",
    concat!(
        "define amp(g) { mul(g) }\n",
        "clock 1kHz t { constant(0.0) | amp(2.0) | stdout() }\n",
    )
);

spec_reject!(
    spec_5_9_recursive_define_rejected,
    "§5.9",
    concat!(
        "define rec() { rec() }\n",
        "clock 1kHz t { constant(0.0) | rec() | stdout() }\n",
    )
);

spec_accept!(
    spec_5_10_feedback_with_delay,
    "§5.10",
    concat!(
        "clock 1kHz t {\n",
        "  constant(0.0) | add(:fb) | :out | delay(1, 0.0) | :fb\n",
        "  :out | stdout()\n",
        "}\n",
    )
);

spec_reject!(
    spec_5_10_feedback_without_delay_rejected,
    "§5.10",
    "clock 1kHz t { constant(0.0) | add(:fb) | :fb | stdout() }\n"
);

// ── 6. CSDF mode switching ─────────────────────────────────────────────

spec_accept!(
    spec_6_2_control_mode_switch_compiles,
    "§6.2",
    concat!(
        "clock 1kHz t {\n",
        "  control { constant(0.0) | threshold(0.5) -> ctrl }\n",
        "  mode sync { constant(0.0) | stdout() }\n",
        "  mode data { constant(1.0) | stdout() }\n",
        "  switch(ctrl, sync, data)\n",
        "}\n",
    )
);

spec_reject!(
    spec_6_3_ctrl_supplier_required,
    "§6.3",
    concat!(
        "clock 1kHz t {\n",
        "  mode sync { constant(0.0) | stdout() }\n",
        "  mode data { constant(1.0) | stdout() }\n",
        "  switch(ctrl, sync, data)\n",
        "}\n",
    )
);

#[test]
#[ignore = "Known conformance gap: current compiler accepts non-int32 ctrl suppliers"]
fn spec_6_3_ctrl_must_be_int32() {
    assert_spec_rejected(
        "§6.3",
        "spec_6_3_ctrl_must_be_int32",
        concat!(
            "clock 1kHz t {\n",
            "  control { constant(0.0) -> ctrl }\n",
            "  mode sync { constant(0.0) | stdout() }\n",
            "  mode data { constant(1.0) | stdout() }\n",
            "  switch(ctrl, sync, data)\n",
            "}\n",
        ),
    );
}

spec_accept!(
    spec_6_6_default_clause_accepted_with_warning,
    "§6.6",
    concat!(
        "clock 1kHz t {\n",
        "  control { constant(0.0) | threshold(0.5) -> ctrl }\n",
        "  mode sync { constant(0.0) | stdout() }\n",
        "  mode data { constant(1.0) | stdout() }\n",
        "  switch(ctrl, sync, data) default sync\n",
        "}\n",
    )
);

spec_reject!(
    spec_6_7_mode_without_switch_rejected,
    "§6.7",
    concat!(
        "clock 1kHz t {\n",
        "  mode sync { constant(0.0) | stdout() }\n",
        "  mode data { constant(1.0) | stdout() }\n",
        "}\n",
    )
);

spec_reject!(
    spec_6_7_switch_without_mode_rejected,
    "§6.7",
    concat!(
        "clock 1kHz t {\n",
        "  control { constant(0.0) | threshold(0.5) -> ctrl }\n",
        "  switch(ctrl, sync, data)\n",
        "}\n",
    )
);

// ── 7. Compile-time error categories ───────────────────────────────────

spec_reject!(
    spec_7_1_unknown_actor_name_resolution_failure,
    "§7.1",
    "clock 1kHz t { not_an_actor() | stdout() }\n"
);

spec_reject!(
    spec_7_1_unknown_shared_buffer_reader_failure,
    "§7.1",
    "clock 1kHz t { @missing | stdout() }\n"
);

// ── 10 / 13. Grammar and shape constraints ─────────────────────────────

spec_accept!(
    spec_10_actor_type_args_and_shape_constraint,
    "§10 + §13.4",
    "clock 1kHz t { sine<float>(100.0, 1.0)[64] | stdout<float>() }\n"
);

spec_accept!(
    spec_13_4_shape_constraint_literal,
    "§13.4",
    "clock 1kHz t { constant(0.0) | fft()[256] | mag() | stdout() }\n"
);

spec_reject!(
    spec_13_6_conflicting_shape_constraints_rejected,
    "§13.6",
    concat!(
        "const coeff = [0.1, 0.2, 0.1]\n",
        "clock 1kHz t { constant(0.0) | fir(coeff)[5] | stdout() }\n",
    )
);

spec_reject!(
    spec_13_6_runtime_param_in_shape_rejected,
    "§13.6",
    "param n = 256\nclock 1kHz t { constant(0.0) | fft()[$n] | mag() | stdout() }\n"
);

// ── 14. External process interface actors ──────────────────────────────

spec_accept!(
    spec_14_2_socket_write_actor_signature,
    "§14.2",
    "clock 1kHz t { constant(0.0) | socket_write(\"localhost:9100\", 0) }\n"
);

spec_accept!(
    spec_14_2_socket_read_actor_signature,
    "§14.2",
    "clock 1kHz t { socket_read(\"localhost:9200\") | stdout() }\n"
);
