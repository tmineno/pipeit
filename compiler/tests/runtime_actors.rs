//
// runtime_actors.rs — Integration test that runs C++ runtime actor tests
//
// This test builds and executes the C++ runtime tests for std_actors.h,
// integrating them into the Cargo test workflow.
//

use std::path::{Path, PathBuf};
use std::process::Command;

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn build_runtime_tests() -> Result<PathBuf, String> {
    let root = project_root();
    let runtime_tests = root.join("runtime/tests");
    let build_dir = runtime_tests.join("build");

    // Create build directory if it doesn't exist
    if !build_dir.exists() {
        std::fs::create_dir_all(&build_dir)
            .map_err(|e| format!("Failed to create build directory: {}", e))?;
    }

    // Run CMake to configure
    let cmake_output = Command::new("cmake")
        .current_dir(&build_dir)
        .arg("..")
        .output()
        .map_err(|e| format!("Failed to run cmake: {}", e))?;

    if !cmake_output.status.success() {
        return Err(format!(
            "CMake configuration failed:\n{}",
            String::from_utf8_lossy(&cmake_output.stderr)
        ));
    }

    // Build the tests
    let make_output = Command::new("make")
        .current_dir(&build_dir)
        .output()
        .map_err(|e| format!("Failed to run make: {}", e))?;

    if !make_output.status.success() {
        return Err(format!(
            "Make build failed:\n{}",
            String::from_utf8_lossy(&make_output.stderr)
        ));
    }

    Ok(build_dir)
}

fn run_test(build_dir: &Path, test_name: &str) -> Result<(), String> {
    let test_binary = build_dir.join(test_name);

    if !test_binary.exists() {
        return Err(format!("Test binary not found: {}", test_binary.display()));
    }

    let output = Command::new(&test_binary)
        .output()
        .map_err(|e| format!("Failed to execute {}: {}", test_name, e))?;

    if !output.status.success() {
        return Err(format!(
            "{} failed:\nstdout:\n{}\nstderr:\n{}",
            test_name,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(())
}

#[test]
fn runtime_arithmetic_actors() {
    let build_dir = build_runtime_tests().expect("Failed to build runtime tests");
    run_test(&build_dir, "test_arithmetic").expect("Arithmetic tests failed");
}

#[test]
fn runtime_statistics_actors() {
    let build_dir = build_runtime_tests().expect("Failed to build runtime tests");
    run_test(&build_dir, "test_statistics").expect("Statistics tests failed");
}

#[test]
fn runtime_fft_actor() {
    let build_dir = build_runtime_tests().expect("Failed to build runtime tests");
    run_test(&build_dir, "test_fft").expect("FFT tests failed");
}

#[test]
fn runtime_transform_actors() {
    let build_dir = build_runtime_tests().expect("Failed to build runtime tests");
    run_test(&build_dir, "test_transform").expect("Transform tests failed");
}

#[test]
fn runtime_utility_actors() {
    let build_dir = build_runtime_tests().expect("Failed to build runtime tests");
    run_test(&build_dir, "test_utility").expect("Utility tests failed");
}
