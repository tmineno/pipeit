// Regression corpus for the verification framework.
//
// Ensures all example .pdl files compile through the pipeline with all
// verification certs (H1-H3, L1-L5, S1-S2, R1-R2) passing, and that
// known failure classes produce expected diagnostics.

use std::path::{Path, PathBuf};
use std::process::Command;
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

/// Generate a shared manifest once per test binary.
fn shared_manifest() -> &'static Path {
    static MANIFEST: OnceLock<PathBuf> = OnceLock::new();
    MANIFEST.get_or_init(|| {
        let path = std::env::temp_dir().join("pcc_verify_regression_manifest.json");
        let root = project_root();
        let output = Command::new(pcc_binary())
            .arg("--emit")
            .arg("manifest")
            .arg("-I")
            .arg(root.join("runtime/libpipit/include"))
            .arg("-I")
            .arg(root.join("examples"))
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

/// All 7 example .pdl files compile with --emit cpp and required -I flags.
/// Since verification is always-on in the pipeline, a successful compilation
/// implies all stage certs (H1-H3, L1-L5, S1-S2, R1-R2) passed.
#[test]
fn all_examples_pass_verification() {
    let actors_dir = project_root().join("runtime/libpipit/include");
    let examples_dir = project_root().join("examples");

    for pdl in &[
        "example",
        "gain",
        "feedback",
        "receiver",
        "complex",
        "multirate",
        "socket_stream",
    ] {
        let path = examples_dir.join(format!("{pdl}.pdl"));
        let output = Command::new(pcc_binary())
            .arg("--emit")
            .arg("cpp")
            .arg("--actor-meta")
            .arg(shared_manifest())
            .arg("-I")
            .arg(&actors_dir)
            .arg("-I")
            .arg(examples_dir.join("../examples"))
            .arg(&path)
            .output()
            .unwrap_or_else(|e| panic!("failed to run pcc for {pdl}: {e}"));
        assert!(
            output.status.success(),
            "{pdl}.pdl failed:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

/// An inline .pdl with an unknown actor should produce an error diagnostic.
#[test]
fn unknown_actor_produces_error() {
    let tmp_dir = std::env::temp_dir();
    let pdl_path = tmp_dir.join("verify_regression_unknown_actor.pdl");
    std::fs::write(
        &pdl_path,
        "clock 1kHz t {\n    nonexistent_actor_xyz() | stdout()\n}\n",
    )
    .expect("write tmp pdl");

    let actors_dir = project_root().join("runtime/libpipit/include");
    let output = Command::new(pcc_binary())
        .arg("--emit")
        .arg("cpp")
        .arg("--actor-meta")
        .arg(shared_manifest())
        .arg("-I")
        .arg(&actors_dir)
        .arg(&pdl_path)
        .output()
        .expect("failed to run pcc");

    assert!(
        !output.status.success(),
        "expected failure for unknown actor"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("nonexistent_actor_xyz") || stderr.contains("unknown"),
        "expected error mentioning unknown actor, got: {stderr}"
    );
    let _ = std::fs::remove_file(&pdl_path);
}

/// JSON output for semantic errors: each line is valid JSON with the unified schema.
#[test]
fn json_output_semantic_error() {
    let tmp_dir = std::env::temp_dir();
    let pdl_path = tmp_dir.join("verify_regression_json_semantic.pdl");
    std::fs::write(
        &pdl_path,
        "clock 1kHz t {\n    nonexistent_actor_xyz() | stdout()\n}\n",
    )
    .expect("write tmp pdl");

    let actors_dir = project_root().join("runtime/libpipit/include");
    let output = Command::new(pcc_binary())
        .arg("--emit")
        .arg("cpp")
        .arg("--actor-meta")
        .arg(shared_manifest())
        .arg("--diagnostic-format")
        .arg("json")
        .arg("-I")
        .arg(&actors_dir)
        .arg(&pdl_path)
        .output()
        .expect("failed to run pcc");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    // Each non-empty line should be valid JSONL
    for line in stderr.lines().filter(|l| !l.is_empty()) {
        let v: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("invalid JSON line: {e}\nline: {line}"));
        assert_eq!(v["kind"], "semantic", "expected kind=semantic, got: {v}");
        assert!(
            !v["code"].is_null(),
            "semantic diagnostic should have a code: {v}"
        );
    }
    let _ = std::fs::remove_file(&pdl_path);
}

/// JSON output for parse errors: each line uses the unified schema with kind=parse.
#[test]
fn json_output_parse_error() {
    let tmp_dir = std::env::temp_dir();
    let pdl_path = tmp_dir.join("verify_regression_json_parse.pdl");
    // Syntactically invalid: unclosed brace
    std::fs::write(&pdl_path, "clock 1kHz t {\n    stdout()\n").expect("write tmp pdl");

    let actors_dir = project_root().join("runtime/libpipit/include");
    let output = Command::new(pcc_binary())
        .arg("--emit")
        .arg("cpp")
        .arg("--actor-meta")
        .arg(shared_manifest())
        .arg("--diagnostic-format")
        .arg("json")
        .arg("-I")
        .arg(&actors_dir)
        .arg(&pdl_path)
        .output()
        .expect("failed to run pcc");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut found_parse = false;
    for line in stderr.lines().filter(|l| !l.is_empty()) {
        let v: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("invalid JSON line: {e}\nline: {line}"));
        assert_eq!(v["kind"], "parse", "expected kind=parse for parse error");
        assert!(v["code"].is_null(), "parse error should have null code");
        assert!(v["span"].is_object(), "parse error should have span object");
        found_parse = true;
    }
    assert!(
        found_parse,
        "expected at least one parse error in JSON output"
    );
    let _ = std::fs::remove_file(&pdl_path);
}
