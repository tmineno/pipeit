// Integration tests: verify that generated C++ code compiles and runs correctly.
//
// Two categories:
//   1. Example files: compile each examples/*.pdl end-to-end
//   2. Inline snippets: targeted coverage of individual language features
//
// Complements the unit tests in codegen.rs which verify structural properties
// of generated C++ strings without invoking a compiler.
// Skipped automatically if no C++ compiler is found.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn pcc_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_pcc"))
}

fn find_cxx_compiler() -> Option<String> {
    for compiler in &["c++", "g++", "clang++"] {
        if Command::new(compiler)
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return Some(compiler.to_string());
        }
    }
    None
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Unique counter for temp file names (avoids collisions in parallel tests).
static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Run pcc on an example .pdl file and syntax-check the output.
fn assert_pdl_file_compiles(pdl_name: &str) {
    let cxx = match find_cxx_compiler() {
        Some(c) => c,
        None => {
            eprintln!("SKIP: no C++ compiler found");
            return;
        }
    };

    let root = project_root();
    let pdl_path = root.join("examples").join(pdl_name);
    let std_actors_h = root
        .join("runtime")
        .join("libpipit")
        .join("include")
        .join("std_actors.h");
    let example_actors_h = root.join("examples").join("example_actors.h");
    let runtime_include = root.join("runtime").join("libpipit").join("include");

    assert!(pdl_path.exists(), "missing {}", pdl_path.display());

    let pcc = pcc_binary();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let cpp_out = std::env::temp_dir().join(format!("pipit_gen_{}.cpp", n));
    let gen = Command::new(&pcc)
        .arg(pdl_path.to_str().unwrap())
        .arg("-I")
        .arg(std_actors_h.to_str().unwrap())
        .arg("-I")
        .arg(example_actors_h.to_str().unwrap())
        .arg("--emit")
        .arg("cpp")
        .arg("-o")
        .arg(cpp_out.to_str().unwrap())
        .output()
        .expect("failed to run pcc");

    assert!(
        gen.status.success(),
        "pcc failed for {}:\n{}",
        pdl_name,
        String::from_utf8_lossy(&gen.stderr)
    );

    let cpp = std::fs::read_to_string(&cpp_out).expect("failed to read generated cpp");
    let _ = std::fs::remove_file(&cpp_out);
    assert!(!cpp.is_empty(), "empty output for {}", pdl_name);

    compile_cpp(
        &cxx,
        &cpp,
        pdl_name,
        &runtime_include,
        &root.join("examples"),
    );
}

/// Run pcc on inline PDL source and syntax-check the output.
fn assert_inline_compiles(pdl_source: &str, test_name: &str) {
    let cxx = match find_cxx_compiler() {
        Some(c) => c,
        None => {
            eprintln!("SKIP: no C++ compiler found");
            return;
        }
    };

    let root = project_root();
    let include_dir = root.join("runtime").join("libpipit").join("include");
    let std_actors_h = include_dir.join("std_actors.h");
    let std_sink_h = include_dir.join("std_sink.h");
    let std_source_h = include_dir.join("std_source.h");
    let runtime_include = root.join("runtime").join("libpipit").join("include");

    // Write PDL to temp file
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp_dir = std::env::temp_dir();
    let pdl_file = tmp_dir.join(format!("pipit_inline_{}.pdl", n));
    std::fs::write(&pdl_file, pdl_source).expect("write pdl temp");

    let pcc = pcc_binary();
    let n2 = COUNTER.fetch_add(1, Ordering::Relaxed);
    let cpp_out = std::env::temp_dir().join(format!("pipit_gen_inline_{}.cpp", n2));
    let gen = Command::new(&pcc)
        .arg(pdl_file.to_str().unwrap())
        .arg("-I")
        .arg(std_actors_h.to_str().unwrap())
        .arg("-I")
        .arg(std_sink_h.to_str().unwrap())
        .arg("-I")
        .arg(std_source_h.to_str().unwrap())
        .arg("--emit")
        .arg("cpp")
        .arg("-o")
        .arg(cpp_out.to_str().unwrap())
        .output()
        .expect("failed to run pcc");

    let _ = std::fs::remove_file(&pdl_file);

    assert!(
        gen.status.success(),
        "pcc failed for '{}':\n{}",
        test_name,
        String::from_utf8_lossy(&gen.stderr)
    );

    let cpp = std::fs::read_to_string(&cpp_out).expect("failed to read generated cpp");
    let _ = std::fs::remove_file(&cpp_out);
    assert!(!cpp.is_empty(), "empty output for '{}'", test_name);

    compile_cpp(
        &cxx,
        &cpp,
        test_name,
        &runtime_include,
        &root.join("examples"),
    );
}

/// Run pcc on inline PDL source and assert that either pcc or C++ compilation fails.
fn assert_inline_fails(pdl_source: &str, test_name: &str) {
    let cxx = match find_cxx_compiler() {
        Some(c) => c,
        None => {
            eprintln!("SKIP: no C++ compiler found");
            return;
        }
    };

    let root = project_root();
    let include_dir = root.join("runtime").join("libpipit").join("include");
    let std_actors_h = include_dir.join("std_actors.h");
    let std_sink_h = include_dir.join("std_sink.h");
    let std_source_h = include_dir.join("std_source.h");
    let runtime_include = root.join("runtime").join("libpipit").join("include");

    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp_dir = std::env::temp_dir();
    let pdl_file = tmp_dir.join(format!("pipit_inline_fail_{}.pdl", n));
    std::fs::write(&pdl_file, pdl_source).expect("write pdl temp");

    let pcc = pcc_binary();
    let n2 = COUNTER.fetch_add(1, Ordering::Relaxed);
    let cpp_out = std::env::temp_dir().join(format!("pipit_gen_inline_fail_{}.cpp", n2));
    let gen = Command::new(&pcc)
        .arg(pdl_file.to_str().unwrap())
        .arg("-I")
        .arg(std_actors_h.to_str().unwrap())
        .arg("-I")
        .arg(std_sink_h.to_str().unwrap())
        .arg("-I")
        .arg(std_source_h.to_str().unwrap())
        .arg("--emit")
        .arg("cpp")
        .arg("-o")
        .arg(cpp_out.to_str().unwrap())
        .output()
        .expect("failed to run pcc");

    let _ = std::fs::remove_file(&pdl_file);
    if !gen.status.success() {
        let _ = std::fs::remove_file(&cpp_out);
        return;
    }

    let cpp = std::fs::read_to_string(&cpp_out).expect("failed to read generated cpp");
    let _ = std::fs::remove_file(&cpp_out);
    assert!(!cpp.is_empty(), "empty output for '{}'", test_name);

    let n3 = COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp_cpp = tmp_dir.join(format!("pipit_fail_cxx_{}.cpp", n3));
    std::fs::write(&tmp_cpp, &cpp).expect("write cpp temp");
    let out = Command::new(&cxx)
        .arg("-std=c++20")
        .arg("-fsyntax-only")
        .arg("-I")
        .arg(runtime_include.to_str().unwrap())
        .arg("-I")
        .arg(root.join("examples").to_str().unwrap())
        .arg(tmp_cpp.to_str().unwrap())
        .output()
        .expect("failed to run C++ compiler");
    let _ = std::fs::remove_file(&tmp_cpp);
    assert!(
        !out.status.success(),
        "expected failure for '{}', but C++ compile succeeded.\nSource:\n{}",
        test_name,
        cpp
    );
}

/// Run pcc on inline PDL source and return generated C++ source.
fn generate_inline_cpp(pdl_source: &str, test_name: &str) -> String {
    let root = project_root();
    let std_actors_h = root
        .join("runtime")
        .join("libpipit")
        .join("include")
        .join("std_actors.h");

    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp_dir = std::env::temp_dir();
    let pdl_file = tmp_dir.join(format!("pipit_inline_gen_{}.pdl", n));
    std::fs::write(&pdl_file, pdl_source).expect("write pdl temp");

    let pcc = pcc_binary();
    let n2 = COUNTER.fetch_add(1, Ordering::Relaxed);
    let cpp_out = std::env::temp_dir().join(format!("pipit_gen_check_{}.cpp", n2));
    let gen = Command::new(&pcc)
        .arg(pdl_file.to_str().unwrap())
        .arg("-I")
        .arg(std_actors_h.to_str().unwrap())
        .arg("--emit")
        .arg("cpp")
        .arg("-o")
        .arg(cpp_out.to_str().unwrap())
        .output()
        .expect("failed to run pcc");

    let _ = std::fs::remove_file(&pdl_file);

    assert!(
        gen.status.success(),
        "pcc failed for '{}':\n{}",
        test_name,
        String::from_utf8_lossy(&gen.stderr)
    );

    let cpp = std::fs::read_to_string(&cpp_out).expect("failed to read generated cpp");
    let _ = std::fs::remove_file(&cpp_out);
    assert!(!cpp.is_empty(), "empty output for '{}'", test_name);
    cpp
}

/// Syntax-check a C++ source string with the system compiler.
fn compile_cpp(
    cxx: &str,
    cpp_source: &str,
    label: &str,
    runtime_include: &Path,
    examples_dir: &Path,
) {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp_dir = std::env::temp_dir();
    let tmp_file = tmp_dir.join(format!("pipit_cxx_{}.cpp", n));
    std::fs::write(&tmp_file, cpp_source).expect("write cpp temp");

    let out = Command::new(cxx)
        .arg("-std=c++20")
        .arg("-fsyntax-only")
        .arg("-I")
        .arg(runtime_include.to_str().unwrap())
        .arg("-I")
        .arg(examples_dir.to_str().unwrap())
        .arg(tmp_file.to_str().unwrap())
        .output()
        .expect("failed to run C++ compiler");

    let _ = std::fs::remove_file(&tmp_file);

    assert!(
        out.status.success(),
        "C++ syntax error in '{}':\n{}\n\nSource:\n{}",
        label,
        String::from_utf8_lossy(&out.stderr),
        cpp_source
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 1. Example .pdl files
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn example_file_complex() {
    assert_pdl_file_compiles("complex.pdl");
}

#[test]
fn example_file_gain() {
    assert_pdl_file_compiles("gain.pdl");
}

#[test]
fn example_file_example() {
    assert_pdl_file_compiles("example.pdl");
}

#[test]
fn example_file_receiver() {
    assert_pdl_file_compiles("receiver.pdl");
}

#[test]
fn example_file_feedback() {
    assert_pdl_file_compiles("feedback.pdl");
}

// ── -I with directory path ──────────────────────────────────────────────

#[test]
fn include_directory_path() {
    // -I with a directory should discover all headers recursively
    let root = project_root();
    let pdl_path = root.join("examples").join("gain.pdl");
    let runtime_include = root.join("runtime").join("libpipit").join("include");

    let pcc = pcc_binary();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let cpp_out = std::env::temp_dir().join(format!("pipit_gen_dir_{}.cpp", n));
    let gen = Command::new(&pcc)
        .arg(pdl_path.to_str().unwrap())
        .arg("-I")
        .arg(runtime_include.to_str().unwrap())
        .arg("-I")
        .arg(root.join("examples").to_str().unwrap())
        .arg("--emit")
        .arg("cpp")
        .arg("-o")
        .arg(cpp_out.to_str().unwrap())
        .output()
        .expect("failed to run pcc");

    let _ = std::fs::remove_file(&cpp_out);

    assert!(
        gen.status.success(),
        "pcc failed with -I <directory>:\n{}",
        String::from_utf8_lossy(&gen.stderr)
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Inline feature tests — each targets a single language construct
// ═══════════════════════════════════════════════════════════════════════════

// ── Source / Sink actors ────────────────────────────────────────────────

#[test]
fn source_only() {
    // Source actor (void input) piped to sink
    assert_inline_compiles("clock 1kHz t { constant(0.0) | stdout() }", "source_only");
}

#[test]
fn sink_with_void_output() {
    // Ensure void output actors produce nullptr for out pointer
    assert_inline_compiles(
        "clock 1kHz t { constant(0.0) | mul(1.0) | stdout() }",
        "sink_void_output",
    );
}

// ── Linear pipeline ────────────────────────────────────────────────────

#[test]
fn linear_chain_three_actors() {
    assert_inline_compiles(
        "clock 1kHz t { constant(0.0) | mul(2.0) | mul(0.5) | stdout() }",
        "linear_chain_3",
    );
}

// ── Const scalar ───────────────────────────────────────────────────────

#[test]
fn const_scalar() {
    assert_inline_compiles(
        "const fft_size = 256\nclock 1kHz t { constant(0.0) | fft(fft_size) | c2r() | stdout() }",
        "const_scalar",
    );
}

// ── Const array ────────────────────────────────────────────────────────

#[test]
fn const_array() {
    assert_inline_compiles(
        "const coeff = [0.1, 0.2, 0.4, 0.2, 0.1]\nclock 1kHz t { constant(0.0) | fir(coeff) | stdout() }",
        "const_array",
    );
}

#[test]
fn fir_legacy_argument_order_rejected() {
    assert_inline_fails(
        "const coeff = [0.1, 0.2, 0.4, 0.2, 0.1]\nclock 1kHz t { constant(0.0) | fir(5, coeff) | stdout() }",
        "fir_legacy_argument_order",
    );
}

// ── Runtime param ($param) ─────────────────────────────────────────────

#[test]
fn runtime_param() {
    assert_inline_compiles(
        "param gain = 2.5\nclock 1kHz t { constant(0.0) | mul($gain) | stdout() }",
        "runtime_param",
    );
}

#[test]
fn runtime_param_integer_default() {
    // Param with integer default consumed by float actor should fail strict typing.
    assert_inline_fails(
        "param gain = 1\nclock 1kHz t { constant(0.0) | mul($gain) | stdout() }",
        "runtime_param_int",
    );
}

// ── Fork / Tap ─────────────────────────────────────────────────────────

#[test]
fn fork_two_consumers() {
    assert_inline_compiles(
        "clock 1kHz t {\n  constant(0.0) | :raw | stdout()\n  :raw | stdout()\n}",
        "fork_two_consumers",
    );
}

#[test]
fn fork_three_consumers() {
    assert_inline_compiles(
        "clock 1kHz t {\n  constant(0.0) | :sig | stdout()\n  :sig | mul(2.0) | stdout()\n  :sig | mul(0.5) | stdout()\n}",
        "fork_three_consumers",
    );
}

// ── Probe ──────────────────────────────────────────────────────────────

#[test]
fn probe_passthrough() {
    assert_inline_compiles(
        "clock 1kHz t { constant(0.0) | mul(1.0) | ?debug | stdout() }",
        "probe_passthrough",
    );
}

// ── Complex types (cfloat) ─────────────────────────────────────────────

#[test]
fn complex_type_chain() {
    // fft produces cfloat, mag consumes cfloat → float
    assert_inline_compiles(
        "clock 1kHz t { constant(0.0) | fft(256) | mag() | stdout() }",
        "complex_type_chain",
    );
}

#[test]
fn complex_c2r() {
    assert_inline_compiles(
        "clock 1kHz t { constant(0.0) | fft(256) | c2r() | stdout() }",
        "complex_c2r",
    );
}

// ── Rate conversion / decimation ───────────────────────────────────────

#[test]
fn decimation_actor() {
    assert_inline_compiles(
        "clock 1kHz t { constant(0.0) | decimate(4) | stdout() }",
        "decimation",
    );
}

// ── Multi-input actor (add) ────────────────────────────────────────────

#[test]
fn multi_input_add() {
    // add() takes 2 float inputs — uses tap ref as second input
    assert_inline_compiles(
        "clock 1kHz t {\n  constant(0.0) | :a | add(:a) | stdout()\n}",
        "multi_input_add",
    );
}

// ── Feedback loop with delay ───────────────────────────────────────────

#[test]
fn feedback_with_delay() {
    assert_inline_compiles(
        "clock 1kHz t {\n  constant(0.0) | add(:fb) | :out | delay(1, 0.0) | :fb\n  :out | stdout()\n}",
        "feedback_delay",
    );
}

// ── Define (macro) ─────────────────────────────────────────────────────

#[test]
fn define_simple() {
    assert_inline_compiles(
        "define amplify() { mul(2.0) | mul(0.5) }\nclock 1kHz t { constant(0.0) | amplify() | stdout() }",
        "define_simple",
    );
}

#[test]
fn define_with_params() {
    assert_inline_compiles(
        "define amp(g) { mul(g) }\nclock 1kHz t { constant(0.0) | amp(3.0) | stdout() }",
        "define_with_params",
    );
}

// ── Inter-task shared buffer ───────────────────────────────────────────

#[test]
fn inter_task_buffer() {
    assert_inline_compiles(
        "clock 1kHz producer { constant(0.0) -> sig }\nclock 1kHz consumer { @sig | stdout() }",
        "inter_task_buffer",
    );
}

#[test]
fn inter_task_buffer_rate_mismatch() {
    // Producer and consumer at different rates
    assert_inline_compiles(
        "clock 10kHz fast { constant(0.0) -> sig }\nclock 1kHz slow { @sig | decimate(10) | stdout() }",
        "inter_task_rate_mismatch",
    );
}

// ── Modal / Switch ─────────────────────────────────────────────────────

#[test]
fn modal_switch() {
    assert_inline_compiles(
        concat!(
            "const taps = [0.25, 0.25, 0.25, 0.25]\n",
            "clock 1kHz t {\n",
            "  control { constant(0.0) | threshold(0.5) -> ctrl }\n",
            "  mode sync { constant(0.0) | fir(taps) | stdout() }\n",
            "  mode data { constant(0.0) | fir(taps) | stdout() }\n",
            "  switch(ctrl, sync, data)\n",
            "}\n",
        ),
        "modal_switch",
    );
}

#[test]
fn modal_switch_default() {
    assert_inline_compiles(
        concat!(
            "clock 1kHz t {\n",
            "  control { constant(0.0) | threshold(0.5) -> ctrl }\n",
            "  mode idle { constant(0.0) | stdout() }\n",
            "  mode active { constant(0.0) | mul(2.0) | stdout() }\n",
            "  switch(ctrl, idle, active) default idle\n",
            "}\n",
        ),
        "modal_switch_default",
    );
}

// ── Core stdlib actors ─────────────────────────────────────────────────

#[test]
fn actor_constant() {
    assert_inline_compiles(
        "clock 1kHz t { constant(1.0) | stdout() }",
        "actor_constant",
    );
}

#[test]
fn actor_sine() {
    assert_inline_compiles("clock 1kHz t { sine(100.0, 1.0) | stdout() }", "actor_sine");
}

#[test]
fn actor_square() {
    assert_inline_compiles(
        "clock 1kHz t { square(100.0, 1.0) | stdout() }",
        "actor_square",
    );
}

#[test]
fn actor_sawtooth() {
    assert_inline_compiles(
        "clock 1kHz t { sawtooth(100.0, 1.0) | stdout() }",
        "actor_sawtooth",
    );
}

#[test]
fn actor_triangle() {
    assert_inline_compiles(
        "clock 1kHz t { triangle(100.0, 1.0) | stdout() }",
        "actor_triangle",
    );
}

#[test]
fn actor_noise() {
    assert_inline_compiles("clock 1kHz t { noise(1.0) | stdout() }", "actor_noise");
}

#[test]
fn actor_impulse() {
    assert_inline_compiles("clock 1kHz t { impulse(100) | stdout() }", "actor_impulse");
}

#[test]
fn actor_mul() {
    assert_inline_compiles(
        "clock 1kHz t { constant(2.0) | mul(3.0) | stdout() }",
        "actor_mul",
    );
}

#[test]
fn actor_add() {
    assert_inline_compiles(
        "clock 1kHz t { constant(1.0) | :a | add(:a) | stdout() }",
        "actor_add",
    );
}

#[test]
fn actor_fft() {
    assert_inline_compiles(
        "clock 1kHz t { constant(0.0) | delay(4, 0.0) | fft(4) | c2r() | stdout() }",
        "actor_fft",
    );
}

#[test]
fn actor_c2r() {
    assert_inline_compiles(
        "clock 1kHz t { constant(0.0) | delay(4, 0.0) | fft(4) | c2r() | stdout() }",
        "actor_c2r",
    );
}

#[test]
fn actor_mag() {
    assert_inline_compiles(
        "clock 1kHz t { constant(0.0) | delay(4, 0.0) | fft(4) | mag() | stdout() }",
        "actor_mag",
    );
}

#[test]
fn actor_fir() {
    assert_inline_compiles(
        concat!(
            "const taps = [0.33, 0.33, 0.34]\n",
            "clock 1kHz t { constant(0.0) | delay(3, 0.0) | fir(taps, 3) | stdout() }",
        ),
        "actor_fir",
    );
}

#[test]
fn actor_delay() {
    assert_inline_compiles(
        "clock 1kHz t { constant(0.0) | delay(5, 1.0) | stdout() }",
        "actor_delay",
    );
}

#[test]
fn actor_decimate() {
    assert_inline_compiles(
        "clock 1kHz t { constant(0.0) | delay(10, 0.0) | decimate(10) | stdout() }",
        "actor_decimate",
    );
}

#[test]
fn actor_stdout() {
    assert_inline_compiles("clock 1kHz t { constant(0.0) | stdout() }", "actor_stdout");
}

// ── New stdlib actors (Phase 2) ────────────────────────────────────────

#[test]
fn actor_sub() {
    assert_inline_compiles(
        "clock 1kHz t { constant(5.0) | :a | sub(:a) | stdout() }",
        "actor_sub",
    );
}

#[test]
fn actor_div() {
    assert_inline_compiles(
        "clock 1kHz t { constant(10.0) | :a | div(:a) | stdout() }",
        "actor_div",
    );
}

#[test]
fn actor_abs() {
    assert_inline_compiles(
        "clock 1kHz t { constant(-5.0) | abs() | stdout() }",
        "actor_abs",
    );
}

#[test]
fn actor_sqrt() {
    assert_inline_compiles(
        "clock 1kHz t { constant(16.0) | sqrt() | stdout() }",
        "actor_sqrt",
    );
}

#[test]
fn actor_threshold() {
    assert_inline_compiles(
        "clock 1kHz t { control { constant(0.7) | threshold(0.5) -> ctrl } mode a { constant(0.0) | stdout() } mode b { constant(1.0) | stdout() } switch(ctrl, a, b) default a }",
        "actor_threshold",
    );
}

#[test]
fn actor_mean() {
    assert_inline_compiles(
        "clock 1kHz t { constant(0.0) | delay(5, 0.0) | mean(5) | stdout() }",
        "actor_mean",
    );
}

#[test]
fn actor_rms() {
    assert_inline_compiles(
        "clock 1kHz t { constant(0.0) | delay(5, 0.0) | rms(5) | stdout() }",
        "actor_rms",
    );
}

#[test]
fn actor_min() {
    assert_inline_compiles(
        "clock 1kHz t { constant(0.0) | delay(5, 0.0) | min(5) | stdout() }",
        "actor_min",
    );
}

#[test]
fn actor_max() {
    assert_inline_compiles(
        "clock 1kHz t { constant(0.0) | delay(5, 0.0) | max(5) | stdout() }",
        "actor_max",
    );
}

#[test]
fn actor_stderr() {
    assert_inline_compiles("clock 1kHz t { constant(0.0) | stderr() }", "actor_stderr");
}

#[test]
fn actor_stdin() {
    assert_inline_compiles("clock 1kHz t { stdin() | stdout() }", "actor_stdin");
}

#[test]
fn actor_stdout_fmt_default() {
    assert_inline_compiles(
        "clock 1kHz t { constant(0.0) | stdout_fmt(\"default\") }",
        "actor_stdout_fmt_default",
    );
}

#[test]
fn actor_stdout_fmt_hex() {
    assert_inline_compiles(
        "clock 1kHz t { constant(0.0) | stdout_fmt(\"hex\") }",
        "actor_stdout_fmt_hex",
    );
}

#[test]
fn actor_stdout_fmt_scientific() {
    assert_inline_compiles(
        "clock 1kHz t { constant(0.0) | stdout_fmt(\"scientific\") }",
        "actor_stdout_fmt_scientific",
    );
}

#[test]
fn actor_binread_float() {
    assert_inline_compiles(
        "clock 1kHz t { binread(\"test.bin\", \"float\") | stdout() }",
        "actor_binread_float",
    );
}

#[test]
fn actor_binread_int16() {
    assert_inline_compiles(
        "clock 1kHz t { binread(\"test.bin\", \"int16\") | stdout() }",
        "actor_binread_int16",
    );
}

#[test]
fn actor_binread_int32() {
    assert_inline_compiles(
        "clock 1kHz t { binread(\"test.bin\", \"int32\") | stdout() }",
        "actor_binread_int32",
    );
}

#[test]
fn actor_binread_cfloat() {
    assert_inline_compiles(
        "clock 1kHz t { binread(\"test.bin\", \"cfloat\") | stdout() }",
        "actor_binread_cfloat",
    );
}

#[test]
fn actor_binwrite_float() {
    assert_inline_compiles(
        "clock 1kHz t { constant(0.0) | binwrite(\"test.bin\", \"float\") }",
        "actor_binwrite_float",
    );
}

#[test]
fn actor_binwrite_int16() {
    assert_inline_compiles(
        "clock 1kHz t { constant(0.0) | binwrite(\"test.bin\", \"int16\") }",
        "actor_binwrite_int16",
    );
}

#[test]
fn actor_binwrite_int32() {
    assert_inline_compiles(
        "clock 1kHz t { constant(0.0) | binwrite(\"test.bin\", \"int32\") }",
        "actor_binwrite_int32",
    );
}

#[test]
fn actor_binwrite_cfloat() {
    assert_inline_compiles(
        "clock 1kHz t { constant(0.0) | binwrite(\"test.bin\", \"cfloat\") }",
        "actor_binwrite_cfloat",
    );
}

// ── Socket actors (PPKT) ──────────────────────────────────────────────

#[test]
fn actor_socket_write() {
    assert_inline_compiles(
        "clock 1kHz t { stdin() | socket_write(\"localhost:9100\", 0) }",
        "actor_socket_write",
    );
}

#[test]
fn actor_socket_read() {
    assert_inline_compiles(
        "clock 1kHz t { socket_read(\"localhost:9200\") | stdout() }",
        "actor_socket_read",
    );
}

// ── K-factor (high-frequency task) ─────────────────────────────────────

#[test]
fn k_factor_high_freq() {
    // 10 MHz → K = 10 iterations per tick
    assert_inline_compiles(
        "clock 10MHz t { constant(0.0) | mul(1.0) | stdout() }",
        "k_factor_high_freq",
    );
}

#[test]
fn k_factor_custom_tick_rate() {
    // set tick_rate = 1kHz with 10kHz task → K = 10
    assert_inline_compiles(
        "set tick_rate = 1kHz\nclock 10kHz t { constant(0.0) | mul(1.0) | stdout() }",
        "k_factor_custom_tick_rate",
    );
}

// ── Multi-task with threads ────────────────────────────────────────────

#[test]
fn multi_task_threads() {
    assert_inline_compiles(
        concat!(
            "clock 48kHz capture { constant(0.0) | mul(1.0) -> sig }\n",
            "clock 1kHz process { @sig | decimate(48) | stdout() }\n",
        ),
        "multi_task_threads",
    );
}

#[test]
fn three_tasks() {
    assert_inline_compiles(
        concat!(
            "clock 1kHz a { constant(0.0) -> buf1 }\n",
            "clock 1kHz b { @buf1 | mul(2.0) -> buf2 }\n",
            "clock 1kHz c { @buf2 | stdout() }\n",
        ),
        "three_tasks",
    );
}

// ── Combined features ──────────────────────────────────────────────────

#[test]
fn const_param_fork_probe() {
    // Combines const, param, fork, and probe in one pipeline
    assert_inline_compiles(
        concat!(
            "const coeff = [0.1, 0.2, 0.4, 0.2, 0.1]\n",
            "param gain = 1.0\n",
            "clock 48kHz t {\n",
            "  constant(0.0) | mul($gain) | :raw | fir(coeff) | ?debug | stdout()\n",
            "  :raw | stdout()\n",
            "}\n",
        ),
        "const_param_fork_probe",
    );
}

#[test]
fn fork_into_different_types() {
    // Fork feeds both cfloat and float paths
    assert_inline_compiles(
        concat!(
            "clock 1kHz t {\n",
            "  constant(0.0) | fft(256) | :spectrum | c2r() | stdout()\n",
            "  :spectrum | mag() | stdout()\n",
            "}\n",
        ),
        "fork_into_types",
    );
}

#[test]
fn define_with_probe_and_tap() {
    assert_inline_compiles(
        concat!(
            "define process() { mul(2.0) | ?check | mul(0.5) }\n",
            "clock 1kHz t { constant(0.0) | :in | process() | stdout()\n:in | stdout() }\n",
        ),
        "define_probe_tap",
    );
}

// ── Shape constraints (v0.2.0) ──────────────────────────────────────────

#[test]
fn shape_constraint_literal() {
    // fft()[256] — dimension inferred from shape constraint literal
    assert_inline_compiles(
        "clock 1kHz t { constant(0.0) | fft()[256] | mag() | stdout() }",
        "shape_constraint_literal",
    );
}

#[test]
fn shape_constraint_const_ref() {
    // fft()[N] — dimension inferred from shape constraint const ref
    assert_inline_compiles(
        "const N = 256\nclock 1kHz t { constant(0.0) | fft()[N] | mag() | stdout() }",
        "shape_constraint_const_ref",
    );
}

#[test]
fn shape_constraint_with_arg() {
    // fft(256) with explicit arg — backward-compat, no shape constraint needed
    assert_inline_compiles(
        "clock 1kHz t { constant(0.0) | fft(256) | mag() | stdout() }",
        "shape_constraint_with_arg",
    );
}

#[test]
fn shape_constraint_infers_param_value() {
    // fft()[256] — dimension inferred from shape constraint, passed to C++ actor
    let cpp = generate_inline_cpp(
        "clock 1kHz t { constant(0.0) | fft()[256] | mag() | stdout() }",
        "shape_infers_param",
    );
    assert!(
        cpp.contains("Actor_fft{256}"),
        "expected Actor_fft{{256}} but got:\n{}",
        cpp
    );
}

#[test]
fn shape_constraint_const_ref_infers_param_value() {
    // fft()[N] — dimension from const ref inferred and passed to C++ actor
    let cpp = generate_inline_cpp(
        "const N = 256\nclock 1kHz t { constant(0.0) | fft()[N] | mag() | stdout() }",
        "shape_const_ref_infers_param",
    );
    assert!(
        cpp.contains("Actor_fft{256}"),
        "expected Actor_fft{{256}} but got:\n{}",
        cpp
    );
}

// ── SDF edge shape inference (§13.3.3) ──────────────────────────────────

#[test]
fn sdf_edge_infers_mag_dimension() {
    // mag() after fft()[256]: N inferred from upstream output shape
    let cpp = generate_inline_cpp(
        "clock 1kHz t { constant(0.0) | fft()[256] | mag() | stdout() }",
        "sdf_edge_infers_mag",
    );
    assert!(
        cpp.contains("Actor_mag{256}"),
        "expected Actor_mag{{256}} inferred from upstream fft, but got:\n{}",
        cpp
    );
}

#[test]
fn sdf_edge_infers_through_fork() {
    // mag() after fork from fft(256): N inferred through fork passthrough
    let cpp = generate_inline_cpp(
        concat!(
            "clock 1kHz t {\n",
            "    constant(0.0) | fft(256) | :raw | mag() | stdout()\n",
            "    :raw | c2r() | stdout()\n",
            "}",
        ),
        "sdf_edge_infers_through_fork",
    );
    assert!(
        cpp.contains("Actor_mag{256}"),
        "expected Actor_mag{{256}} inferred through fork, but got:\n{}",
        cpp
    );
}

// ── Shared-memory model correctness (regressions) ──────────────────────

#[test]
fn shared_buffer_multi_reader_has_independent_reader_indices() {
    let cpp = generate_inline_cpp(
        concat!(
            "clock 1kHz w { constant(0.0) -> sig }\n",
            "clock 1kHz r1 { @sig | stdout() }\n",
            "clock 1kHz r2 { @sig | stdout() }\n",
        ),
        "shared_multi_reader_indices",
    );

    assert!(
        cpp.contains("RingBuffer<float, 2, 2> _ringbuf_sig"),
        "expected 2-reader shared ring buffer, got:\n{}",
        cpp
    );
    assert!(
        cpp.contains("_ringbuf_sig.read(0,"),
        "first reader should use index 0, got:\n{}",
        cpp
    );
    assert!(
        cpp.contains("_ringbuf_sig.read(1,"),
        "second reader should use index 1, got:\n{}",
        cpp
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Overrun policy tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn overrun_policy_drop() {
    assert_inline_compiles(
        "set overrun = drop\nclock 1kHz t { constant(0.0) | stdout() }",
        "overrun_drop",
    );
}

#[test]
fn shared_buffer_io_checks_status_and_stops_on_failure() {
    let cpp = generate_inline_cpp(
        concat!(
            "clock 1kHz w { constant(0.0) -> sig }\n",
            "clock 1kHz r { @sig | stdout() }\n",
        ),
        "shared_io_status_checks",
    );

    assert!(
        cpp.contains("if (!_ringbuf_sig.write("),
        "write should check ring-buffer status, got:\n{}",
        cpp
    );
    assert!(
        cpp.contains("if (!_ringbuf_sig.read("),
        "read should check ring-buffer status, got:\n{}",
        cpp
    );
    assert!(
        cpp.contains("_stop.store(true, std::memory_order_release)"),
        "failed I/O should set stop flag, got:\n{}",
        cpp
    );
}

#[test]
fn overrun_policy_slip() {
    assert_inline_compiles(
        "set overrun = slip\nclock 1kHz t { constant(0.0) | stdout() }",
        "overrun_slip",
    );
}

#[test]
fn shared_buffer_io_uses_pointer_offsets_in_repetition_loops() {
    let cpp = generate_inline_cpp(
        concat!(
            "const coeff = [0.1, 0.2, 0.4, 0.2, 0.1]\n",
            "clock 1MHz w { constant(0.0) | fft(256) | c2r() | fir(coeff) -> sig }\n",
            "clock 1MHz r { @sig | decimate(256) | stdout() }\n",
        ),
        "shared_io_offsets",
    );

    assert!(
        cpp.contains("_ringbuf_sig.write(&_"),
        "writer should advance pointer with per-firing offset, got:\n{}",
        cpp
    );
    assert!(
        cpp.contains("_ringbuf_sig.read(0, &_"),
        "reader should advance pointer with per-firing offset, got:\n{}",
        cpp
    );
}

#[test]
fn overrun_policy_backlog() {
    assert_inline_compiles(
        "set overrun = backlog\nclock 1kHz t { constant(0.0) | stdout() }",
        "overrun_backlog",
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. End-to-end run tests — compile + execute generated binaries
// ═══════════════════════════════════════════════════════════════════════════

/// Compile a PDL file to a binary and run it with given args.
/// Returns (exit_code, stdout, stderr).
fn compile_and_run_pdl(pdl_name: &str, run_args: &[&str]) -> Option<(i32, String, String)> {
    let cxx = find_cxx_compiler()?;
    let root = project_root();
    let pdl_path = root.join("examples").join(pdl_name);
    let std_actors_h = root
        .join("runtime")
        .join("libpipit")
        .join("include")
        .join("std_actors.h");
    let example_actors_h = root.join("examples").join("example_actors.h");
    let runtime_include = root.join("runtime").join("libpipit").join("include");

    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp_dir = std::env::temp_dir();
    let cpp_file = tmp_dir.join(format!("pipit_run_{}.cpp", n));
    let bin_file = tmp_dir.join(format!("pipit_run_{}", n));

    let pcc = pcc_binary();
    let gen = Command::new(&pcc)
        .arg(pdl_path.to_str().unwrap())
        .arg("-I")
        .arg(std_actors_h.to_str().unwrap())
        .arg("-I")
        .arg(example_actors_h.to_str().unwrap())
        .arg("--emit")
        .arg("cpp")
        .arg("-o")
        .arg(cpp_file.to_str().unwrap())
        .output()
        .expect("failed to run pcc");

    if !gen.status.success() {
        panic!(
            "pcc failed for {}:\n{}",
            pdl_name,
            String::from_utf8_lossy(&gen.stderr)
        );
    }

    let cpp = std::fs::read_to_string(&cpp_file).expect("read generated cpp");

    let compile = Command::new(&cxx)
        .arg("-std=c++20")
        .arg("-O0")
        .arg("-I")
        .arg(runtime_include.to_str().unwrap())
        .arg("-I")
        .arg(root.join("examples").to_str().unwrap())
        .arg(&cpp_file)
        .arg("-lpthread")
        .arg("-o")
        .arg(&bin_file)
        .output()
        .expect("failed to compile");

    let _ = std::fs::remove_file(&cpp_file);

    if !compile.status.success() {
        let _ = std::fs::remove_file(&bin_file);
        panic!(
            "C++ compile failed for '{}':\n{}\n\nSource:\n{}",
            pdl_name,
            String::from_utf8_lossy(&compile.stderr),
            cpp
        );
    }

    let run = Command::new("timeout")
        .arg("10")
        .arg(&bin_file)
        .args(run_args)
        .output()
        .expect("failed to run binary");

    let _ = std::fs::remove_file(&bin_file);

    Some((
        run.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&run.stdout).to_string(),
        String::from_utf8_lossy(&run.stderr).to_string(),
    ))
}

/// Compile inline PDL to a binary and run it.
fn compile_and_run_inline(
    pdl_source: &str,
    test_name: &str,
    run_args: &[&str],
) -> Option<(i32, String, String)> {
    let cxx = find_cxx_compiler()?;
    let root = project_root();
    let std_actors_h = root
        .join("runtime")
        .join("libpipit")
        .join("include")
        .join("std_actors.h");
    let runtime_include = root.join("runtime").join("libpipit").join("include");

    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp_dir = std::env::temp_dir();
    let pdl_file = tmp_dir.join(format!("pipit_runinl_{}.pdl", n));
    let cpp_file = tmp_dir.join(format!("pipit_runinl_{}.cpp", n));
    let bin_file = tmp_dir.join(format!("pipit_runinl_{}", n));
    std::fs::write(&pdl_file, pdl_source).expect("write pdl");

    let pcc = pcc_binary();
    let gen = Command::new(&pcc)
        .arg(pdl_file.to_str().unwrap())
        .arg("-I")
        .arg(std_actors_h.to_str().unwrap())
        .arg("--emit")
        .arg("cpp")
        .arg("-o")
        .arg(cpp_file.to_str().unwrap())
        .output()
        .expect("failed to run pcc");

    let _ = std::fs::remove_file(&pdl_file);

    if !gen.status.success() {
        panic!(
            "pcc failed for '{}':\n{}",
            test_name,
            String::from_utf8_lossy(&gen.stderr)
        );
    }

    let cpp = std::fs::read_to_string(&cpp_file).expect("read generated cpp");

    let compile = Command::new(&cxx)
        .arg("-std=c++20")
        .arg("-O0")
        .arg("-I")
        .arg(runtime_include.to_str().unwrap())
        .arg("-I")
        .arg(root.join("examples").to_str().unwrap())
        .arg(&cpp_file)
        .arg("-lpthread")
        .arg("-o")
        .arg(&bin_file)
        .output()
        .expect("failed to compile");

    let _ = std::fs::remove_file(&cpp_file);

    if !compile.status.success() {
        let _ = std::fs::remove_file(&bin_file);
        panic!(
            "C++ compile failed for '{}':\n{}\n\nSource:\n{}",
            test_name,
            String::from_utf8_lossy(&compile.stderr),
            cpp
        );
    }

    let run = Command::new("timeout")
        .arg("10")
        .arg(&bin_file)
        .args(run_args)
        .output()
        .expect("failed to run binary");

    let _ = std::fs::remove_file(&bin_file);

    Some((
        run.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&run.stdout).to_string(),
        String::from_utf8_lossy(&run.stderr).to_string(),
    ))
}

#[test]
fn gain_pdl_runs() {
    if let Some((code, _stdout, stderr)) = compile_and_run_pdl("gain.pdl", &["--duration", "0.01"])
    {
        assert_eq!(code, 0, "gain.pdl exited with code {}: {}", code, stderr);
    }
}

#[test]
fn example_pdl_runs() {
    // example.pdl can emit very high stdout volume; use zero-duration smoke run.
    if let Some((code, _stdout, stderr)) = compile_and_run_pdl("example.pdl", &["--duration", "0"])
    {
        assert_eq!(code, 0, "example.pdl exited with code {}: {}", code, stderr);
    }
}

#[test]
fn feedback_pdl_runs() {
    if let Some((code, _stdout, stderr)) =
        compile_and_run_pdl("feedback.pdl", &["--duration", "0.01"])
    {
        assert_eq!(
            code, 0,
            "feedback.pdl exited with code {}: {}",
            code, stderr
        );
    }
}

#[test]
fn exit_code_2_on_unknown_flag() {
    if let Some((code, _stdout, _stderr)) = compile_and_run_pdl("gain.pdl", &["--nonexistent-flag"])
    {
        assert_eq!(
            code, 2,
            "expected exit code 2 for unknown flag, got {}",
            code
        );
    }
}

#[test]
fn stats_flag_produces_output() {
    if let Some((code, _stdout, stderr)) =
        compile_and_run_pdl("gain.pdl", &["--duration", "0.01", "--stats"])
    {
        assert_eq!(code, 0, "gain.pdl --stats exited {}: {}", code, stderr);
        assert!(
            stderr.contains("[stats]"),
            "expected [stats] in stderr, got: {}",
            stderr
        );
    }
}

#[test]
fn duration_with_suffix() {
    // Test --duration with 's' suffix
    if let Some((code, _stdout, stderr)) = compile_and_run_inline(
        "clock 1kHz t { constant(0.0) | stdout() }",
        "duration_suffix",
        &["--duration", "0.01s"],
    ) {
        assert_eq!(code, 0, "duration suffix test failed: {}", stderr);
    }
}

// ── Probe Tests ─────────────────────────────────────────────────────────────

#[test]
fn receiver_pdl_runs() {
    // Verify receiver.pdl compiles and runs without probes
    if let Some((code, _stdout, stderr)) =
        compile_and_run_pdl("receiver.pdl", &["--duration", "0.01"])
    {
        assert_eq!(code, 0, "receiver.pdl failed: {}", stderr);
    }
}

#[test]
fn stats_output_includes_task_and_buffer_stats() {
    // Verify --stats includes both task and buffer statistics
    if let Some((code, _stdout, stderr)) =
        compile_and_run_pdl("receiver.pdl", &["--duration", "0.01", "--stats"])
    {
        assert_eq!(code, 0, "receiver.pdl --stats failed: {}", stderr);
        assert!(
            stderr.contains("[stats] task"),
            "expected task stats in stderr, got: {}",
            stderr
        );
        assert!(
            stderr.contains("[stats] shared buffer"),
            "expected buffer stats in stderr, got: {}",
            stderr
        );
    }
}

#[test]
fn probe_emits_when_enabled() {
    // Verify enabled probe outputs data to stderr
    if let Some((code, _stdout, stderr)) = compile_and_run_pdl(
        "receiver.pdl",
        &["--duration", "0.01", "--probe", "sync_out"],
    ) {
        assert_eq!(code, 0, "receiver.pdl with probe failed: {}", stderr);
        assert!(
            stderr.contains("[probe:sync_out]"),
            "expected probe output in stderr, got: {}",
            stderr
        );
    }
}

#[test]
fn probe_silent_when_not_enabled() {
    // Verify disabled probe produces no output
    if let Some((code, _stdout, stderr)) =
        compile_and_run_pdl("receiver.pdl", &["--duration", "0.01"])
    {
        assert_eq!(code, 0, "receiver.pdl failed: {}", stderr);
        assert!(
            !stderr.contains("[probe:"),
            "unexpected probe output when not enabled: {}",
            stderr
        );
    }
}

#[test]
fn unknown_probe_exits_code_2() {
    // Verify unknown probe name validation
    if let Some((code, _stdout, stderr)) =
        compile_and_run_pdl("receiver.pdl", &["--probe", "nonexistent"])
    {
        assert_eq!(
            code, 2,
            "expected exit code 2 for unknown probe, got {}",
            code
        );
        assert!(
            stderr.contains("startup error: unknown probe 'nonexistent'"),
            "expected unknown probe error message, got: {}",
            stderr
        );
    }
}

#[test]
fn probe_output_missing_path_exits_code_2() {
    // Verify --probe-output requires path argument (existing CLI validation)
    if let Some((code, _stdout, stderr)) = compile_and_run_pdl("receiver.pdl", &["--probe-output"])
    {
        assert_eq!(
            code, 2,
            "expected exit code 2 for missing probe output path, got {}",
            code
        );
        assert!(
            stderr.contains("startup error: --probe-output requires a path"),
            "expected missing path error message, got: {}",
            stderr
        );
    }
}

#[test]
fn probe_output_open_failure_exits_code_2() {
    // Verify file open error handling
    if let Some((code, _stdout, stderr)) = compile_and_run_pdl(
        "receiver.pdl",
        &[
            "--probe",
            "sync_out",
            "--probe-output",
            "/nonexistent/directory/file.txt",
        ],
    ) {
        assert_eq!(
            code, 2,
            "expected exit code 2 for file open failure, got {}",
            code
        );
        assert!(
            stderr.contains("startup error: failed to open probe output file"),
            "expected file open error message, got: {}",
            stderr
        );
    }
}

#[test]
fn duplicate_probe_args_accepted() {
    // Verify duplicate probe names are idempotent (no startup error).
    // receiver.pdl's logger task may hit a shared-buffer runtime error (code 1)
    // due to the modal task staying in sync mode, so we accept code 0 or 1 —
    // only code 2 (startup error) would indicate duplicate probes are rejected.
    if let Some((code, _stdout, stderr)) = compile_and_run_pdl(
        "receiver.pdl",
        &[
            "--duration",
            "0.01",
            "--probe",
            "sync_out",
            "--probe",
            "sync_out",
        ],
    ) {
        assert_ne!(
            code, 2,
            "duplicate probe args caused startup error: {}",
            stderr
        );
    }
}

#[test]
fn probe_output_file_contains_data() {
    // Verify probe output file is created and contains data
    let tmp = std::env::temp_dir().join("pipit_probe_test.txt");
    let tmp_str = tmp.to_str().unwrap();

    if let Some((code, _stdout, _stderr)) = compile_and_run_pdl(
        "receiver.pdl",
        &[
            "--duration",
            "0.01",
            "--probe",
            "sync_out",
            "--probe-output",
            tmp_str,
        ],
    ) {
        assert_eq!(code, 0, "probe output to file failed");
        let contents = std::fs::read_to_string(&tmp).expect("failed to read probe output file");
        assert!(
            contents.contains("[probe:sync_out]"),
            "expected probe output in file, got: {}",
            contents
        );
        let _ = std::fs::remove_file(&tmp);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Polymorphic actor tests (v0.3.0)
// ═══════════════════════════════════════════════════════════════════════════

/// Run pcc on inline PDL with polymorphic actor headers and syntax-check the output.
fn assert_poly_inline_compiles(pdl_source: &str, test_name: &str) {
    let cxx = match find_cxx_compiler() {
        Some(c) => c,
        None => {
            eprintln!("SKIP: no C++ compiler found");
            return;
        }
    };

    let root = project_root();
    let include_dir = root.join("runtime").join("libpipit").join("include");
    let std_actors_h = include_dir.join("std_actors.h");
    let std_sink_h = include_dir.join("std_sink.h");
    let std_source_h = include_dir.join("std_source.h");
    let poly_actors_h = root.join("examples").join("poly_actors.h");
    let runtime_include = root.join("runtime").join("libpipit").join("include");

    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp_dir = std::env::temp_dir();
    let pdl_file = tmp_dir.join(format!("pipit_poly_{}.pdl", n));
    std::fs::write(&pdl_file, pdl_source).expect("write pdl temp");

    let pcc = pcc_binary();
    let n2 = COUNTER.fetch_add(1, Ordering::Relaxed);
    let cpp_out = std::env::temp_dir().join(format!("pipit_gen_poly_{}.cpp", n2));
    let gen = Command::new(&pcc)
        .arg(pdl_file.to_str().unwrap())
        .arg("-I")
        .arg(std_actors_h.to_str().unwrap())
        .arg("-I")
        .arg(std_sink_h.to_str().unwrap())
        .arg("-I")
        .arg(std_source_h.to_str().unwrap())
        .arg("-I")
        .arg(poly_actors_h.to_str().unwrap())
        .arg("--emit")
        .arg("cpp")
        .arg("-o")
        .arg(cpp_out.to_str().unwrap())
        .output()
        .expect("failed to run pcc");

    let _ = std::fs::remove_file(&pdl_file);

    assert!(
        gen.status.success(),
        "pcc failed for '{}':\n{}",
        test_name,
        String::from_utf8_lossy(&gen.stderr)
    );

    let cpp = std::fs::read_to_string(&cpp_out).expect("failed to read generated cpp");
    let _ = std::fs::remove_file(&cpp_out);
    assert!(!cpp.is_empty(), "empty output for '{}'", test_name);

    compile_cpp(
        &cxx,
        &cpp,
        test_name,
        &runtime_include,
        &root.join("examples"),
    );
}

/// Run pcc on inline PDL with poly actors and return generated C++ source.
fn generate_poly_cpp(pdl_source: &str, test_name: &str) -> String {
    let root = project_root();
    let include_dir = root.join("runtime").join("libpipit").join("include");
    let std_actors_h = include_dir.join("std_actors.h");
    let std_sink_h = include_dir.join("std_sink.h");
    let std_source_h = include_dir.join("std_source.h");
    let poly_actors_h = root.join("examples").join("poly_actors.h");

    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp_dir = std::env::temp_dir();
    let pdl_file = tmp_dir.join(format!("pipit_poly_gen_{}.pdl", n));
    std::fs::write(&pdl_file, pdl_source).expect("write pdl temp");

    let pcc = pcc_binary();
    let n2 = COUNTER.fetch_add(1, Ordering::Relaxed);
    let cpp_out = tmp_dir.join(format!("pipit_gen_poly_cpp_{}.cpp", n2));
    let gen = Command::new(&pcc)
        .arg(pdl_file.to_str().unwrap())
        .arg("-I")
        .arg(std_actors_h.to_str().unwrap())
        .arg("-I")
        .arg(std_sink_h.to_str().unwrap())
        .arg("-I")
        .arg(std_source_h.to_str().unwrap())
        .arg("-I")
        .arg(poly_actors_h.to_str().unwrap())
        .arg("--emit")
        .arg("cpp")
        .arg("-o")
        .arg(cpp_out.to_str().unwrap())
        .output()
        .expect("failed to run pcc");

    let _ = std::fs::remove_file(&pdl_file);

    assert!(
        gen.status.success(),
        "pcc failed for '{}':\n{}",
        test_name,
        String::from_utf8_lossy(&gen.stderr)
    );

    let cpp = std::fs::read_to_string(&cpp_out).expect("failed to read generated cpp");
    let _ = std::fs::remove_file(&cpp_out);
    cpp
}

#[test]
fn poly_explicit_type_arg_float() {
    // poly_scale<float>(...) — explicit type argument
    assert_poly_inline_compiles(
        "clock 1kHz t { constant(0.0) | poly_scale<float>(2.0) | stdout() }",
        "poly_explicit_float",
    );
}

#[test]
fn poly_explicit_type_arg_double() {
    // poly_pass<double> between two poly actors (both double)
    // Uses constant(float) -> poly_scale<float> -> stdout() since no double source/sink exist
    assert_poly_inline_compiles(
        "clock 1kHz t { constant(0.0) | poly_scale<float>(2.0) | poly_pass<float>() | stdout() }",
        "poly_explicit_chain",
    );
}

#[test]
fn poly_pass_through_float() {
    // poly_pass<float>() — identity passthrough
    assert_poly_inline_compiles(
        "clock 1kHz t { constant(0.0) | poly_pass<float>() | stdout() }",
        "poly_pass_float",
    );
}

#[test]
fn poly_template_instantiation_syntax() {
    // Verify that generated C++ uses Actor_poly_scale<float> template syntax
    let cpp = generate_poly_cpp(
        "clock 1kHz t { constant(0.0) | poly_scale<float>(2.0) | stdout() }",
        "poly_template_syntax",
    );
    assert!(
        cpp.contains("Actor_poly_scale<float>"),
        "generated C++ should use template instantiation syntax Actor_poly_scale<float>, got:\n{}",
        cpp
    );
}

#[test]
fn poly_chain_explicit() {
    // Chain of polymorphic actors with explicit type args
    assert_poly_inline_compiles(
        "clock 1kHz t { constant(0.0) | poly_pass<float>() | poly_scale<float>(3.0) | stdout() }",
        "poly_chain",
    );
}
