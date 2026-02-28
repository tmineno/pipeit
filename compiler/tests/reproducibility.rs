// Reproducibility tests for hermetic builds.
//
// These tests verify that the compiler produces byte-identical outputs
// for identical inputs, satisfying the Phase 7 determinism requirements.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

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

/// Generate a shared manifest once per test binary (runtime + examples).
fn shared_manifest() -> &'static Path {
    static MANIFEST: OnceLock<PathBuf> = OnceLock::new();
    MANIFEST.get_or_init(|| {
        let path = std::env::temp_dir().join("pcc_reproducibility_manifest.json");
        let output = Command::new(pcc_binary())
            .arg("--emit")
            .arg("manifest")
            .arg("-I")
            .arg(runtime_include_dir())
            .arg("-I")
            .arg(examples_dir())
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

/// Compiling the same source with the same manifest produces byte-identical C++.
#[test]
fn same_source_same_manifest_identical_cpp() {
    let pdl = project_root().join("examples/gain.pdl");
    let pdl_str = pdl.to_str().unwrap();
    let rt = runtime_include_dir();
    let rt_str = rt.to_str().unwrap();
    let ex = examples_dir();
    let ex_str = ex.to_str().unwrap();
    let meta = shared_manifest().to_str().unwrap();

    let first = run_pcc(&[
        "--emit",
        "cpp",
        pdl_str,
        "--actor-meta",
        meta,
        "-I",
        rt_str,
        "-I",
        ex_str,
    ]);
    let second = run_pcc(&[
        "--emit",
        "cpp",
        pdl_str,
        "--actor-meta",
        meta,
        "-I",
        rt_str,
        "-I",
        ex_str,
    ]);

    assert_eq!(
        first, second,
        "C++ output should be byte-identical across runs"
    );
}

/// Two independently-generated manifests from the same headers produce
/// identical registry fingerprints in build-info.
#[test]
fn independently_generated_manifests_identical_fingerprint() {
    let pdl = project_root().join("examples/gain.pdl");
    let pdl_str = pdl.to_str().unwrap();
    let rt = runtime_include_dir();
    let rt_str = rt.to_str().unwrap();
    let ex = examples_dir();
    let ex_str = ex.to_str().unwrap();

    // Generate two manifests independently
    let manifest1_text = run_pcc(&["--emit", "manifest", "-I", rt_str, "-I", ex_str]);
    let manifest2_text = run_pcc(&["--emit", "manifest", "-I", rt_str, "-I", ex_str]);

    let tmp_dir = std::env::temp_dir();
    let manifest1_path = tmp_dir.join("pcc_repro_test_manifest1.json");
    let manifest2_path = tmp_dir.join("pcc_repro_test_manifest2.json");
    std::fs::write(&manifest1_path, &manifest1_text).unwrap();
    std::fs::write(&manifest2_path, &manifest2_text).unwrap();

    let info1 = run_pcc(&[
        "--emit",
        "build-info",
        pdl_str,
        "--actor-meta",
        manifest1_path.to_str().unwrap(),
    ]);
    let info2 = run_pcc(&[
        "--emit",
        "build-info",
        pdl_str,
        "--actor-meta",
        manifest2_path.to_str().unwrap(),
    ]);

    let _ = std::fs::remove_file(&manifest1_path);
    let _ = std::fs::remove_file(&manifest2_path);

    let json1: serde_json::Value = serde_json::from_str(&info1).unwrap();
    let json2: serde_json::Value = serde_json::from_str(&info2).unwrap();

    assert_eq!(
        json1["registry_fingerprint"], json2["registry_fingerprint"],
        "registry fingerprint should be identical across independent manifest generations"
    );
    assert_eq!(
        json1["source_hash"], json2["source_hash"],
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
    let meta = shared_manifest().to_str().unwrap();

    let first = run_pcc(&["--emit", "build-info", pdl_str, "--actor-meta", meta]);
    let second = run_pcc(&["--emit", "build-info", pdl_str, "--actor-meta", meta]);

    assert_eq!(
        first, second,
        "build-info output should be byte-identical across runs"
    );
}

/// Different source files produce different source_hash values.
#[test]
fn different_source_different_provenance() {
    let meta = shared_manifest().to_str().unwrap();

    let gain_pdl = project_root().join("examples/gain.pdl");
    let example_pdl = project_root().join("examples/example.pdl");

    let gain_info = run_pcc(&[
        "--emit",
        "build-info",
        gain_pdl.to_str().unwrap(),
        "--actor-meta",
        meta,
    ]);
    let example_info = run_pcc(&[
        "--emit",
        "build-info",
        example_pdl.to_str().unwrap(),
        "--actor-meta",
        meta,
    ]);

    let gain_json: serde_json::Value = serde_json::from_str(&gain_info).unwrap();
    let example_json: serde_json::Value = serde_json::from_str(&example_info).unwrap();

    assert_ne!(
        gain_json["source_hash"], example_json["source_hash"],
        "different source files should have different source_hash"
    );
}

/// Same source with different manifests produces different registry_fingerprint.
#[test]
fn different_registry_different_provenance() {
    let pdl = project_root().join("examples/gain.pdl");
    let pdl_str = pdl.to_str().unwrap();
    let rt = runtime_include_dir();
    let rt_str = rt.to_str().unwrap();
    let ex = examples_dir();
    let ex_str = ex.to_str().unwrap();
    let tmp_dir = std::env::temp_dir();

    // Full manifest (runtime + examples)
    let full_manifest_text = run_pcc(&["--emit", "manifest", "-I", rt_str, "-I", ex_str]);
    let full_manifest_path = tmp_dir.join("pcc_repro_full_manifest.json");
    std::fs::write(&full_manifest_path, &full_manifest_text).unwrap();

    // Partial manifest (examples only, no runtime headers)
    let partial_manifest_text = run_pcc(&["--emit", "manifest", "-I", ex_str]);
    let partial_manifest_path = tmp_dir.join("pcc_repro_partial_manifest.json");
    std::fs::write(&partial_manifest_path, &partial_manifest_text).unwrap();

    let full_info = run_pcc(&[
        "--emit",
        "build-info",
        pdl_str,
        "--actor-meta",
        full_manifest_path.to_str().unwrap(),
    ]);
    let partial_info = run_pcc(&[
        "--emit",
        "build-info",
        pdl_str,
        "--actor-meta",
        partial_manifest_path.to_str().unwrap(),
    ]);

    let _ = std::fs::remove_file(&full_manifest_path);
    let _ = std::fs::remove_file(&partial_manifest_path);

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
