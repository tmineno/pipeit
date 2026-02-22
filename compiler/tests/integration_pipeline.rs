// Integration tests for pipeline pass manager behavior.
//
// These tests verify invariants introduced by the pass manager (Phase 3):
// - `--emit ast` is a parse-only path with no registry dependency
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
