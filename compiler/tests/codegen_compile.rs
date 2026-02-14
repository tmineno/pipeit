// Integration tests: verify that generated C++ code compiles with a C++ compiler.
//
// Two categories:
//   1. Example files: compile each examples/*.pdl end-to-end
//   2. Inline snippets: targeted coverage of individual language features
//
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
    let actors_h = root.join("examples").join("actors.h");
    let runtime_include = root.join("runtime").join("libpipit").join("include");

    assert!(pdl_path.exists(), "missing {}", pdl_path.display());

    let pcc = pcc_binary();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let cpp_out = std::env::temp_dir().join(format!("pipit_gen_{}.cpp", n));
    let gen = Command::new(&pcc)
        .arg(pdl_path.to_str().unwrap())
        .arg("-I")
        .arg(actors_h.to_str().unwrap())
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
    let actors_h = root.join("examples").join("actors.h");
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
        .arg(actors_h.to_str().unwrap())
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

// ═══════════════════════════════════════════════════════════════════════════
// 2. Inline feature tests — each targets a single language construct
// ═══════════════════════════════════════════════════════════════════════════

// ── Source / Sink actors ────────────────────────────────────────────────

#[test]
fn source_only() {
    // Source actor (void input) piped to sink
    assert_inline_compiles("clock 1kHz t { adc(0) | stdout() }", "source_only");
}

#[test]
fn sink_with_void_output() {
    // Ensure void output actors produce nullptr for out pointer
    assert_inline_compiles(
        "clock 1kHz t { adc(0) | mul(1.0) | stdout() }",
        "sink_void_output",
    );
}

// ── Linear pipeline ────────────────────────────────────────────────────

#[test]
fn linear_chain_three_actors() {
    assert_inline_compiles(
        "clock 1kHz t { adc(0) | mul(2.0) | mul(0.5) | stdout() }",
        "linear_chain_3",
    );
}

// ── Const scalar ───────────────────────────────────────────────────────

#[test]
fn const_scalar() {
    assert_inline_compiles(
        "const fft_size = 256\nclock 1kHz t { adc(0) | fft(fft_size) | c2r() | stdout() }",
        "const_scalar",
    );
}

// ── Const array ────────────────────────────────────────────────────────

#[test]
fn const_array() {
    assert_inline_compiles(
        "const coeff = [0.1, 0.2, 0.4, 0.2, 0.1]\nclock 1kHz t { adc(0) | fir(5, coeff) | stdout() }",
        "const_array",
    );
}

// ── Runtime param ($param) ─────────────────────────────────────────────

#[test]
fn runtime_param() {
    assert_inline_compiles(
        "param gain = 2.5\nclock 1kHz t { adc(0) | mul($gain) | stdout() }",
        "runtime_param",
    );
}

#[test]
fn runtime_param_integer_default() {
    // Param with integer default, consumed by float actor
    assert_inline_compiles(
        "param gain = 1\nclock 1kHz t { adc(0) | mul($gain) | stdout() }",
        "runtime_param_int",
    );
}

// ── Fork / Tap ─────────────────────────────────────────────────────────

#[test]
fn fork_two_consumers() {
    assert_inline_compiles(
        "clock 1kHz t {\n  adc(0) | :raw | stdout()\n  :raw | stdout()\n}",
        "fork_two_consumers",
    );
}

#[test]
fn fork_three_consumers() {
    assert_inline_compiles(
        "clock 1kHz t {\n  adc(0) | :sig | stdout()\n  :sig | mul(2.0) | stdout()\n  :sig | mul(0.5) | stdout()\n}",
        "fork_three_consumers",
    );
}

// ── Probe ──────────────────────────────────────────────────────────────

#[test]
fn probe_passthrough() {
    assert_inline_compiles(
        "clock 1kHz t { adc(0) | mul(1.0) | ?debug | stdout() }",
        "probe_passthrough",
    );
}

// ── Complex types (cfloat) ─────────────────────────────────────────────

#[test]
fn complex_type_chain() {
    // fft produces cfloat, mag consumes cfloat → float
    assert_inline_compiles(
        "clock 1kHz t { adc(0) | fft(256) | mag() | stdout() }",
        "complex_type_chain",
    );
}

#[test]
fn complex_c2r() {
    assert_inline_compiles(
        "clock 1kHz t { adc(0) | fft(256) | c2r() | stdout() }",
        "complex_c2r",
    );
}

// ── Rate conversion / decimation ───────────────────────────────────────

#[test]
fn decimation_actor() {
    assert_inline_compiles(
        "clock 1kHz t { adc(0) | decimate(4) | stdout() }",
        "decimation",
    );
}

// ── Multi-input actor (add) ────────────────────────────────────────────

#[test]
fn multi_input_add() {
    // add() takes 2 float inputs — uses tap ref as second input
    assert_inline_compiles(
        "clock 1kHz t {\n  adc(0) | :a | add(:a) | stdout()\n}",
        "multi_input_add",
    );
}

// ── Feedback loop with delay ───────────────────────────────────────────

#[test]
fn feedback_with_delay() {
    assert_inline_compiles(
        "clock 1kHz t {\n  adc(0) | add(:fb) | :out | delay(1, 0.0) | :fb\n  :out | stdout()\n}",
        "feedback_delay",
    );
}

// ── Define (macro) ─────────────────────────────────────────────────────

#[test]
fn define_simple() {
    assert_inline_compiles(
        "define amplify() { mul(2.0) | mul(0.5) }\nclock 1kHz t { adc(0) | amplify() | stdout() }",
        "define_simple",
    );
}

#[test]
fn define_with_params() {
    assert_inline_compiles(
        "define amp(g) { mul(g) }\nclock 1kHz t { adc(0) | amp(3.0) | stdout() }",
        "define_with_params",
    );
}

// ── Inter-task shared buffer ───────────────────────────────────────────

#[test]
fn inter_task_buffer() {
    assert_inline_compiles(
        "clock 1kHz producer { adc(0) -> sig }\nclock 1kHz consumer { @sig | stdout() }",
        "inter_task_buffer",
    );
}

#[test]
fn inter_task_buffer_rate_mismatch() {
    // Producer and consumer at different rates
    assert_inline_compiles(
        "clock 10kHz fast { adc(0) -> sig }\nclock 1kHz slow { @sig | decimate(10) | stdout() }",
        "inter_task_rate_mismatch",
    );
}

// ── Modal / Switch ─────────────────────────────────────────────────────

#[test]
fn modal_switch() {
    assert_inline_compiles(
        concat!(
            "clock 1kHz t {\n",
            "  control { adc(0) | correlate() | detect() -> ctrl }\n",
            "  mode sync { adc(0) | sync_process() | stdout() }\n",
            "  mode data { adc(0) | fir(4, [0.25, 0.25, 0.25, 0.25]) | stdout() }\n",
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
            "  control { adc(0) | correlate() | detect() -> ctrl }\n",
            "  mode idle { adc(0) | stdout() }\n",
            "  mode active { adc(0) | mul(2.0) | stdout() }\n",
            "  switch(ctrl, idle, active) default idle\n",
            "}\n",
        ),
        "modal_switch_default",
    );
}

// ── K-factor (high-frequency task) ─────────────────────────────────────

#[test]
fn k_factor_high_freq() {
    // 10 MHz → K = 10 iterations per tick
    assert_inline_compiles(
        "clock 10MHz t { adc(0) | mul(1.0) | stdout() }",
        "k_factor_high_freq",
    );
}

// ── Multi-task with threads ────────────────────────────────────────────

#[test]
fn multi_task_threads() {
    assert_inline_compiles(
        concat!(
            "clock 48kHz capture { adc(0) | mul(1.0) -> sig }\n",
            "clock 1kHz process { @sig | decimate(48) | stdout() }\n",
        ),
        "multi_task_threads",
    );
}

#[test]
fn three_tasks() {
    assert_inline_compiles(
        concat!(
            "clock 1kHz a { adc(0) -> buf1 }\n",
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
            "  adc(0) | mul($gain) | :raw | fir(5, coeff) | ?debug | stdout()\n",
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
            "  adc(0) | fft(256) | :spectrum | c2r() | stdout()\n",
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
            "clock 1kHz t { adc(0) | :in | process() | stdout()\n:in | stdout() }\n",
        ),
        "define_probe_tap",
    );
}
