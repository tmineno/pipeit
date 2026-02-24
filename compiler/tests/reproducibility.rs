// Reproducibility tests for hermetic builds.
//
// These tests verify that the compiler produces byte-identical outputs
// for identical inputs, satisfying the Phase 7 determinism requirements.

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

fn run_pcc(args: &[&str]) -> String {
    let output = Command::new(pcc_binary())
        .args(args)
        .output()
        .expect("failed to run pcc");
    assert!(
        output.status.success(),
        "pcc failed with args {:?}\nstderr: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("non-UTF8 output")
}

/// Compiling the same source with the same headers produces byte-identical C++.
#[test]
fn same_source_same_headers_identical_cpp() {
    let pdl = project_root().join("examples/gain.pdl");
    let pdl_str = pdl.to_str().unwrap();
    let rt = runtime_include_dir();
    let rt_str = rt.to_str().unwrap();
    let ex = examples_dir();
    let ex_str = ex.to_str().unwrap();

    let first = run_pcc(&["--emit", "cpp", pdl_str, "-I", rt_str, "-I", ex_str]);
    let second = run_pcc(&["--emit", "cpp", pdl_str, "-I", rt_str, "-I", ex_str]);

    assert_eq!(
        first, second,
        "C++ output should be byte-identical across runs"
    );
}

/// Compiling via headers vs manifest produces identical registry fingerprints.
///
/// Full C++ text may differ (different #include lines), but the provenance
/// registry_fingerprint (based on canonical_json) should be identical.
#[test]
fn same_source_same_manifest_identical_fingerprint() {
    let pdl = project_root().join("examples/gain.pdl");
    let pdl_str = pdl.to_str().unwrap();
    let rt = runtime_include_dir();
    let rt_str = rt.to_str().unwrap();
    let ex = examples_dir();
    let ex_str = ex.to_str().unwrap();

    // Build-info via header scanning
    let header_info = run_pcc(&["--emit", "build-info", pdl_str, "-I", rt_str, "-I", ex_str]);

    // Generate manifest, then build-info via manifest
    let manifest = run_pcc(&["--emit", "manifest", "-I", rt_str, "-I", ex_str]);
    let tmp_dir = std::env::temp_dir();
    let manifest_path = tmp_dir.join("pcc_repro_test_manifest.json");
    std::fs::write(&manifest_path, &manifest).unwrap();

    let manifest_info = run_pcc(&[
        "--emit",
        "build-info",
        pdl_str,
        "--actor-meta",
        manifest_path.to_str().unwrap(),
    ]);

    let _ = std::fs::remove_file(&manifest_path);

    let header_json: serde_json::Value = serde_json::from_str(&header_info).unwrap();
    let manifest_json: serde_json::Value = serde_json::from_str(&manifest_info).unwrap();

    assert_eq!(
        header_json["registry_fingerprint"], manifest_json["registry_fingerprint"],
        "registry fingerprint should be identical whether loaded from headers or manifest"
    );
    assert_eq!(
        header_json["source_hash"], manifest_json["source_hash"],
        "source hash should be identical"
    );
}

/// `--emit manifest` produces byte-identical output across runs.
#[test]
fn manifest_output_is_stable() {
    let rt = runtime_include_dir();
    let rt_str = rt.to_str().unwrap();
    let ex = examples_dir();
    let ex_str = ex.to_str().unwrap();

    let first = run_pcc(&["--emit", "manifest", "-I", rt_str, "-I", ex_str]);
    let second = run_pcc(&["--emit", "manifest", "-I", rt_str, "-I", ex_str]);

    assert_eq!(
        first, second,
        "manifest output should be byte-identical across runs"
    );
}

/// `--emit build-info` produces byte-identical output across runs.
#[test]
fn build_info_deterministic_across_runs() {
    let pdl = project_root().join("examples/gain.pdl");
    let pdl_str = pdl.to_str().unwrap();
    let rt = runtime_include_dir();
    let rt_str = rt.to_str().unwrap();
    let ex = examples_dir();
    let ex_str = ex.to_str().unwrap();

    let first = run_pcc(&["--emit", "build-info", pdl_str, "-I", rt_str, "-I", ex_str]);
    let second = run_pcc(&["--emit", "build-info", pdl_str, "-I", rt_str, "-I", ex_str]);

    assert_eq!(
        first, second,
        "build-info output should be byte-identical across runs"
    );
}

/// Different source files produce different source_hash values.
#[test]
fn different_source_different_provenance() {
    let rt = runtime_include_dir();
    let rt_str = rt.to_str().unwrap();
    let ex = examples_dir();
    let ex_str = ex.to_str().unwrap();

    let gain_pdl = project_root().join("examples/gain.pdl");
    let example_pdl = project_root().join("examples/example.pdl");

    let gain_info = run_pcc(&[
        "--emit",
        "build-info",
        gain_pdl.to_str().unwrap(),
        "-I",
        rt_str,
        "-I",
        ex_str,
    ]);
    let example_info = run_pcc(&[
        "--emit",
        "build-info",
        example_pdl.to_str().unwrap(),
        "-I",
        rt_str,
        "-I",
        ex_str,
    ]);

    let gain_json: serde_json::Value = serde_json::from_str(&gain_info).unwrap();
    let example_json: serde_json::Value = serde_json::from_str(&example_info).unwrap();

    assert_ne!(
        gain_json["source_hash"], example_json["source_hash"],
        "different source files should have different source_hash"
    );
}

/// Same source with different registries produces different registry_fingerprint.
#[test]
fn different_registry_different_provenance() {
    let pdl = project_root().join("examples/gain.pdl");
    let pdl_str = pdl.to_str().unwrap();
    let rt = runtime_include_dir();
    let rt_str = rt.to_str().unwrap();
    let ex = examples_dir();
    let ex_str = ex.to_str().unwrap();

    // Full registry (runtime + examples)
    let full_info = run_pcc(&["--emit", "build-info", pdl_str, "-I", rt_str, "-I", ex_str]);

    // Partial registry (examples only, no runtime headers)
    let partial_info = run_pcc(&["--emit", "build-info", pdl_str, "-I", ex_str]);

    let full_json: serde_json::Value = serde_json::from_str(&full_info).unwrap();
    let partial_json: serde_json::Value = serde_json::from_str(&partial_info).unwrap();

    assert_ne!(
        full_json["registry_fingerprint"], partial_json["registry_fingerprint"],
        "different registries should produce different fingerprints"
    );
    // Source hash should be the same (same .pdl file)
    assert_eq!(
        full_json["source_hash"], partial_json["source_hash"],
        "same source file should have same source_hash regardless of registry"
    );
}
