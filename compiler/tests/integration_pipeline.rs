// Integration tests for pipeline pass manager behavior.
//
// These tests verify invariants introduced by the pass manager and Phase 7:
// - `--emit ast` is a parse-only path with no registry dependency
// - `--emit manifest` generates valid JSON without requiring .pdl source
// - `--emit build-info` computes provenance hashes
// - Minimal pass evaluation for each --emit target

use std::path::{Path, PathBuf};
use std::process::Command;

fn pcc_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_pcc"))
}

fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn runtime_include_dir() -> PathBuf {
    project_root()
        .join("runtime")
        .join("libpipit")
        .join("include")
}

fn examples_dir() -> PathBuf {
    project_root().join("examples")
}

/// `--emit ast` succeeds without any -I or --actor-path flags.
/// This locks the invariant that `--emit ast` is a parse-only path
/// with no registry dependency.
#[test]
fn emit_ast_does_not_need_registry() {
    let pdl = project_root().join("examples/example.pdl");
    let output = Command::new(pcc_binary())
        .arg("--emit")
        .arg("ast")
        .arg(&pdl)
        .output()
        .expect("failed to run pcc");

    assert!(
        output.status.success(),
        "pcc --emit ast should succeed without registry flags.\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    // AST output should contain program structure
    assert!(
        !stdout.is_empty(),
        "pcc --emit ast should produce non-empty output"
    );
}

// ── --emit manifest tests ──────────────────────────────────────────────────

/// `--emit manifest` generates valid JSON with schema: 1 and sorted actors.
#[test]
fn emit_manifest_generates_valid_json() {
    let output = Command::new(pcc_binary())
        .arg("--emit")
        .arg("manifest")
        .arg("-I")
        .arg(runtime_include_dir())
        .arg("-I")
        .arg(examples_dir())
        .output()
        .expect("failed to run pcc");

    assert!(
        output.status.success(),
        "pcc --emit manifest should succeed.\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("manifest should be valid JSON: {}\noutput: {}", e, stdout));

    assert_eq!(json["schema"], 1, "manifest schema should be 1");
    let actors = json["actors"]
        .as_array()
        .expect("actors should be an array");
    assert!(!actors.is_empty(), "manifest should contain actors");

    // Verify actors are sorted alphabetically by name
    let names: Vec<&str> = actors.iter().map(|a| a["name"].as_str().unwrap()).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted, "actors should be sorted by name");
}

/// `--emit manifest` does not require a .pdl source file.
#[test]
fn emit_manifest_does_not_require_source() {
    let output = Command::new(pcc_binary())
        .arg("--emit")
        .arg("manifest")
        .arg("-I")
        .arg(examples_dir())
        .output()
        .expect("failed to run pcc");

    assert!(
        output.status.success(),
        "pcc --emit manifest should succeed without source.\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// `--emit manifest` produces stable output across multiple runs.
#[test]
fn emit_manifest_stable_output() {
    let run = || {
        let output = Command::new(pcc_binary())
            .arg("--emit")
            .arg("manifest")
            .arg("-I")
            .arg(runtime_include_dir())
            .arg("-I")
            .arg(examples_dir())
            .output()
            .expect("failed to run pcc");
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap()
    };

    let first = run();
    let second = run();
    assert_eq!(
        first, second,
        "manifest output should be byte-identical across runs"
    );
}

/// `--emit manifest` + `--actor-meta` is a usage error (exit code 2).
#[test]
fn emit_manifest_rejects_actor_meta() {
    let output = Command::new(pcc_binary())
        .arg("--emit")
        .arg("manifest")
        .arg("--actor-meta")
        .arg("nonexistent.json")
        .output()
        .expect("failed to run pcc");

    assert_eq!(
        output.status.code(),
        Some(2),
        "combining --emit manifest with --actor-meta should be exit code 2.\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// ── --emit build-info tests ────────────────────────────────────────────────

/// `--emit build-info` generates valid provenance JSON.
#[test]
fn emit_build_info_generates_valid_json() {
    let pdl = project_root().join("examples/gain.pdl");
    let output = Command::new(pcc_binary())
        .arg("--emit")
        .arg("build-info")
        .arg(&pdl)
        .arg("-I")
        .arg(runtime_include_dir())
        .arg("-I")
        .arg(examples_dir())
        .output()
        .expect("failed to run pcc");

    assert!(
        output.status.success(),
        "pcc --emit build-info should succeed.\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("build-info should be valid JSON: {}\noutput: {}", e, stdout));

    // Verify all expected fields
    assert!(json["source_hash"].is_string(), "should have source_hash");
    assert!(
        json["registry_fingerprint"].is_string(),
        "should have registry_fingerprint"
    );
    assert_eq!(
        json["manifest_schema_version"], 1,
        "should have manifest_schema_version"
    );
    assert!(
        json["compiler_version"].is_string(),
        "should have compiler_version"
    );

    // Hashes should be 64-char hex strings
    let source_hash = json["source_hash"].as_str().unwrap();
    assert_eq!(source_hash.len(), 64, "source_hash should be 64 hex chars");
    assert!(
        source_hash.chars().all(|c| c.is_ascii_hexdigit()),
        "source_hash should be hex"
    );
}

/// `--emit build-info` requires a source file.
#[test]
fn emit_build_info_requires_source() {
    let output = Command::new(pcc_binary())
        .arg("--emit")
        .arg("build-info")
        .arg("-I")
        .arg(examples_dir())
        .output()
        .expect("failed to run pcc");

    assert!(
        !output.status.success(),
        "pcc --emit build-info without source should fail"
    );
    assert_eq!(output.status.code(), Some(2));
}

/// `--emit build-info` produces deterministic output across runs.
#[test]
fn emit_build_info_deterministic() {
    let pdl = project_root().join("examples/gain.pdl");
    let run = || {
        let output = Command::new(pcc_binary())
            .arg("--emit")
            .arg("build-info")
            .arg(&pdl)
            .arg("-I")
            .arg(runtime_include_dir())
            .arg("-I")
            .arg(examples_dir())
            .output()
            .expect("failed to run pcc");
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap()
    };

    let first = run();
    let second = run();
    assert_eq!(
        first, second,
        "build-info output should be byte-identical across runs"
    );
}

/// `--emit build-info` succeeds even with a parse-invalid source file.
/// Provenance is about "what went in", not "does it compile".
#[test]
fn emit_build_info_succeeds_with_parse_invalid_source() {
    // Create a temporary file with invalid PDL syntax
    let tmp_dir = std::env::temp_dir();
    let bad_pdl = tmp_dir.join("pcc_test_bad_syntax.pdl");
    std::fs::write(&bad_pdl, "this is not valid { pdl [ syntax !!!").unwrap();

    let output = Command::new(pcc_binary())
        .arg("--emit")
        .arg("build-info")
        .arg(&bad_pdl)
        .arg("-I")
        .arg(examples_dir())
        .output()
        .expect("failed to run pcc");

    let _ = std::fs::remove_file(&bad_pdl);

    assert!(
        output.status.success(),
        "pcc --emit build-info should succeed with invalid source.\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value =
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("should be valid JSON: {}", e));
    assert!(json["source_hash"].is_string());
}

// ── Manifest round-trip tests ─────────────────────────────────────────────

/// Two-step manifest workflow: generate manifest, then compile with --actor-meta.
/// This mirrors the CMake build integration from Phase 7b.
#[test]
fn manifest_then_compile_produces_valid_cpp() {
    let tmp_dir = std::env::temp_dir();
    let manifest_path = tmp_dir.join("pcc_test_manifest_roundtrip.json");

    // Step 1: Generate manifest from headers
    let manifest_output = Command::new(pcc_binary())
        .arg("--emit")
        .arg("manifest")
        .arg("-I")
        .arg(runtime_include_dir())
        .arg("-I")
        .arg(examples_dir())
        .arg("-o")
        .arg(&manifest_path)
        .output()
        .expect("failed to run pcc --emit manifest");

    assert!(
        manifest_output.status.success(),
        "manifest generation should succeed.\nstderr: {}",
        String::from_utf8_lossy(&manifest_output.stderr)
    );
    assert!(manifest_path.exists(), "manifest file should be created");

    // Step 2: Compile PDL using manifest (hermetic, no header scanning for metadata)
    let pdl = project_root().join("examples/gain.pdl");
    let cpp_output = Command::new(pcc_binary())
        .arg(&pdl)
        .arg("--actor-meta")
        .arg(&manifest_path)
        .arg("-I")
        .arg(examples_dir())
        .arg("-I")
        .arg(runtime_include_dir())
        .arg("--emit")
        .arg("cpp")
        .output()
        .expect("failed to run pcc with --actor-meta");

    let _ = std::fs::remove_file(&manifest_path);

    assert!(
        cpp_output.status.success(),
        "compilation with --actor-meta should succeed.\nstderr: {}",
        String::from_utf8_lossy(&cpp_output.stderr)
    );

    let cpp = String::from_utf8_lossy(&cpp_output.stdout);
    assert!(!cpp.is_empty(), "generated C++ should be non-empty");
    assert!(
        cpp.contains("// pcc provenance:"),
        "generated C++ should contain provenance comment"
    );
    assert!(
        cpp.contains("pipit::shell_main"),
        "generated C++ should contain shell_main call"
    );
}
