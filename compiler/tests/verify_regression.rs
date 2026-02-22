// Regression corpus for the verification framework.
//
// Ensures all example .pdl files compile through the pipeline with all
// verification certs (H1-H3, L1-L5, S1-S2, R1-R2) passing, and that
// known failure classes produce expected diagnostics.

use std::path::{Path, PathBuf};
use std::process::Command;

fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn pcc_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_pcc"))
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
