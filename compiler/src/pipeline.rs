// pipeline.rs — Compilation state and pass orchestration
//
// Holds all pass artifacts in a borrow-split struct (upstream/downstream)
// and runs the minimal set of passes for a given terminal PassId.
//
// Preconditions: Program and Registry must be set before calling run_pipeline.
// Postconditions: all artifacts for required passes are populated, or has_error is set.
// Failure modes: any pass emitting error-level diagnostics.
// Side effects: calls on_pass_complete callback after each pass for immediate display.
//
// See ADR-020 for design rationale.

use std::time::Instant;

use crate::analyze::AnalyzedProgram;
use crate::ast::Program;
use crate::codegen::{CodegenOptions, GeneratedCode};
use crate::diag::codes;
use crate::diag::{DiagLevel, Diagnostic};
use crate::graph::ProgramGraph;
use crate::hir::HirProgram;
use crate::id::IdAllocator;
use crate::lir::LirProgram;
use crate::lower::{Cert, LoweredProgram};
use crate::pass::{descriptor, required_passes, PassId, StageCert};
use crate::registry::Registry;
use crate::resolve::ResolvedProgram;
use crate::schedule::ScheduledProgram;
use crate::type_infer::TypedProgram;

// ── Artifact storage ───────────────────────────────────────────────────────

/// Artifacts that ThirContext borrows — set before the thir-block.
pub struct UpstreamArtifacts {
    pub registry: Registry,
    pub program: Program,
    pub resolved: Option<ResolvedProgram>,
    pub id_alloc: Option<IdAllocator>,
    pub hir: Option<HirProgram>,
    pub typed: Option<TypedProgram>,
    pub lowered: Option<LoweredProgram>,
    pub cert: Option<Cert>,
    pub graph: Option<ProgramGraph>,
}

/// Artifacts set while ThirContext is alive — separate struct for borrow safety.
pub struct DownstreamArtifacts {
    pub analysis: Option<AnalyzedProgram>,
    pub schedule: Option<ScheduledProgram>,
    pub lir: Option<LirProgram>,
    pub generated: Option<GeneratedCode>,
}

/// Provenance metadata for hermetic builds and cache-key use.
///
/// `source_hash`: SHA-256 of the raw `.pdl` source text.
/// `registry_fingerprint`: SHA-256 of canonical compact JSON from `Registry::canonical_json()`.
/// `compiler_version`: crate version from `Cargo.toml`.
#[derive(Debug, Clone)]
pub struct Provenance {
    pub source_hash: [u8; 32],
    pub registry_fingerprint: [u8; 32],
    pub compiler_version: &'static str,
}

impl Provenance {
    /// Hex string of the source hash (64 characters).
    pub fn source_hash_hex(&self) -> String {
        bytes_to_hex(&self.source_hash)
    }

    /// Hex string of the registry fingerprint (64 characters).
    pub fn registry_fingerprint_hex(&self) -> String {
        bytes_to_hex(&self.registry_fingerprint)
    }

    /// Serialize provenance as a JSON string for `--emit build-info`.
    pub fn to_json(&self) -> String {
        format!(
            "{{\n  \"source_hash\": \"{}\",\n  \"registry_fingerprint\": \"{}\",\n  \"manifest_schema_version\": 1,\n  \"compiler_version\": \"{}\"\n}}\n",
            self.source_hash_hex(),
            self.registry_fingerprint_hex(),
            self.compiler_version,
        )
    }
}

fn bytes_to_hex(bytes: &[u8; 32]) -> String {
    let mut s = String::with_capacity(64);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(s, "{:02x}", b);
    }
    s
}

/// Compute provenance from source text and registry.
///
/// Uses SHA-256 for both hashes. The registry fingerprint is computed from
/// `Registry::canonical_json()` (compact JSON, no whitespace) to ensure
/// stability independent of display formatting.
pub fn compute_provenance(source: &str, registry: &Registry) -> Provenance {
    use sha2::{Digest, Sha256};

    let source_hash = {
        let mut hasher = Sha256::new();
        hasher.update(source.as_bytes());
        let result = hasher.finalize();
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&result);
        hash
    };

    let registry_fingerprint = {
        let canonical = registry.canonical_json();
        let mut hasher = Sha256::new();
        hasher.update(canonical.as_bytes());
        let result = hasher.finalize();
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&result);
        hash
    };

    Provenance {
        source_hash,
        registry_fingerprint,
        compiler_version: env!("CARGO_PKG_VERSION"),
    }
}

/// Holds all compilation artifacts and accumulated diagnostics.
pub struct CompilationState {
    pub upstream: UpstreamArtifacts,
    pub downstream: DownstreamArtifacts,
    pub diagnostics: Vec<Diagnostic>,
    pub has_error: bool,
    pub provenance: Option<Provenance>,
}

impl CompilationState {
    pub fn new(program: Program, registry: Registry) -> Self {
        Self {
            upstream: UpstreamArtifacts {
                registry,
                program,
                resolved: None,
                id_alloc: None,
                hir: None,
                typed: None,
                lowered: None,
                cert: None,
                graph: None,
            },
            downstream: DownstreamArtifacts {
                analysis: None,
                schedule: None,
                lir: None,
                generated: None,
            },
            diagnostics: Vec::new(),
            has_error: false,
            provenance: None,
        }
    }
}

// ── Error type ─────────────────────────────────────────────────────────────

/// Pipeline execution failed due to error-level diagnostics in a pass.
/// The specific diagnostics are available in `CompilationState.diagnostics`.
#[derive(Debug)]
pub struct PipelineError {
    /// The pass that produced the error.
    pub failing_pass: PassId,
}

// ── Helper: check diagnostics for errors ───────────────────────────────────

fn has_error_diags(diags: &[Diagnostic]) -> bool {
    diags.iter().any(|d| d.level == DiagLevel::Error)
}

/// Per-pass post-processing: callback, accumulate, verbose, error check.
/// Takes split borrows to avoid conflicting with ThirContext borrows on upstream.
/// Returns Err(()) if error diagnostics found.
fn finish_pass_core(
    all_diags: &mut Vec<Diagnostic>,
    has_error: &mut bool,
    pass_id: PassId,
    diags: Vec<Diagnostic>,
    elapsed: std::time::Duration,
    verbose: bool,
    on_pass_complete: &mut impl FnMut(PassId, &[Diagnostic]),
) -> Result<(), PipelineError> {
    on_pass_complete(pass_id, &diags);
    let is_err = has_error_diags(&diags);
    all_diags.extend(diags);
    if verbose {
        eprintln!(
            "pcc: {} complete, {:.1}ms",
            descriptor(pass_id).name,
            elapsed.as_secs_f64() * 1000.0
        );
    }
    if is_err {
        *has_error = true;
        return Err(PipelineError {
            failing_pass: pass_id,
        });
    }
    Ok(())
}

/// Convenience wrapper for finish_pass_core with full CompilationState access.
fn finish_pass(
    state: &mut CompilationState,
    pass_id: PassId,
    diags: Vec<Diagnostic>,
    elapsed: std::time::Duration,
    verbose: bool,
    on_pass_complete: &mut impl FnMut(PassId, &[Diagnostic]),
) -> Result<(), PipelineError> {
    finish_pass_core(
        &mut state.diagnostics,
        &mut state.has_error,
        pass_id,
        diags,
        elapsed,
        verbose,
        on_pass_complete,
    )
}

/// Per-pass post-processing for passes that produce no diagnostics.
fn finish_pass_no_diags(
    pass_id: PassId,
    elapsed: std::time::Duration,
    verbose: bool,
    on_pass_complete: &mut impl FnMut(PassId, &[Diagnostic]),
) {
    on_pass_complete(pass_id, &[]);
    if verbose {
        eprintln!(
            "pcc: {} complete, {:.1}ms",
            descriptor(pass_id).name,
            elapsed.as_secs_f64() * 1000.0
        );
    }
}

// ── Pipeline runner ────────────────────────────────────────────────────────

/// Run the minimal set of passes to produce `terminal`.
///
/// Per-pass sequence: execute → on_pass_complete(callback) → verbose → error check.
///
/// Preconditions: `state.upstream.program` and `state.upstream.registry` are set.
/// Postconditions: artifacts for all passes in `required_passes(terminal)` are populated,
///   or `state.has_error` is true.
/// Failure modes: any pass producing error-level diagnostics; lower cert failure.
/// Side effects: calls `on_pass_complete` after each pass for immediate diagnostic display.
pub fn run_pipeline(
    state: &mut CompilationState,
    terminal: PassId,
    codegen_options: &CodegenOptions,
    verbose: bool,
    mut on_pass_complete: impl FnMut(PassId, &[Diagnostic]),
) -> Result<(), PipelineError> {
    let passes = required_passes(terminal);

    // Spawn expansion: AST → AST pre-pass (before name resolution).
    let spawn_result = crate::spawn::expand_spawns(&state.upstream.program);
    state.upstream.program = spawn_result.program;
    for d in &spawn_result.diagnostics {
        if d.level == DiagLevel::Error {
            state.has_error = true;
        }
    }
    state.diagnostics.extend(spawn_result.diagnostics);
    if state.has_error {
        return Ok(());
    }

    for &pass_id in &passes {
        // ThirContext-dependent passes run in a scoped block
        if matches!(
            pass_id,
            PassId::Analyze | PassId::Schedule | PassId::BuildLir | PassId::Codegen
        ) {
            return run_thir_and_downstream(
                state,
                &passes,
                codegen_options,
                verbose,
                &mut on_pass_complete,
            );
        }

        match pass_id {
            PassId::Resolve => {
                let t = Instant::now();
                let result =
                    crate::resolve::resolve(&state.upstream.program, &state.upstream.registry);
                let elapsed = t.elapsed();
                let diags = result.diagnostics;
                state.upstream.resolved = Some(result.resolved);
                state.upstream.id_alloc = Some(result.id_alloc);
                finish_pass(
                    state,
                    PassId::Resolve,
                    diags,
                    elapsed,
                    verbose,
                    &mut on_pass_complete,
                )?;
            }
            PassId::BuildHir => {
                let t = Instant::now();
                let hir = crate::hir::build_hir(
                    &state.upstream.program,
                    state.upstream.resolved.as_ref().unwrap(),
                    state.upstream.id_alloc.as_mut().unwrap(),
                );
                let elapsed = t.elapsed();
                state.upstream.hir = Some(hir);
                // Verify HIR postconditions (H1-H3)
                let hir_cert = crate::hir::verify_hir(
                    state.upstream.hir.as_ref().unwrap(),
                    state.upstream.resolved.as_ref().unwrap(),
                );
                if !hir_cert.all_pass() {
                    let failed: Vec<_> = hir_cert
                        .obligations()
                        .iter()
                        .filter(|(_, ok)| !ok)
                        .map(|(name, _)| *name)
                        .collect();
                    let diags = vec![Diagnostic::new(
                        DiagLevel::Error,
                        state.upstream.hir.as_ref().unwrap().program_span,
                        format!("HIR verification failed: {}", failed.join(", ")),
                    )
                    .with_code(codes::E0600)];
                    finish_pass(
                        state,
                        PassId::BuildHir,
                        diags,
                        elapsed,
                        verbose,
                        &mut on_pass_complete,
                    )?;
                } else {
                    finish_pass_no_diags(PassId::BuildHir, elapsed, verbose, &mut on_pass_complete);
                }
            }
            PassId::TypeInfer => {
                let t = Instant::now();
                let result = crate::type_infer::type_infer(
                    state.upstream.hir.as_ref().unwrap(),
                    state.upstream.resolved.as_ref().unwrap(),
                    &state.upstream.registry,
                );
                let elapsed = t.elapsed();
                let diags = result.diagnostics;
                state.upstream.typed = Some(result.typed);
                finish_pass(
                    state,
                    PassId::TypeInfer,
                    diags,
                    elapsed,
                    verbose,
                    &mut on_pass_complete,
                )?;
            }
            PassId::Lower => {
                let t = Instant::now();
                let result = crate::lower::lower_and_verify(
                    state.upstream.hir.as_ref().unwrap(),
                    state.upstream.resolved.as_ref().unwrap(),
                    state.upstream.typed.as_ref().unwrap(),
                    &state.upstream.registry,
                );
                let elapsed = t.elapsed();
                let mut diags = result.diagnostics;
                if !result.cert.all_pass() {
                    diags.push(
                        Diagnostic::new(
                            DiagLevel::Error,
                            state.upstream.hir.as_ref().unwrap().program_span,
                            "lowering verification failed (L1-L5 obligations not met)",
                        )
                        .with_code(codes::E0601),
                    );
                }
                state.upstream.cert = Some(result.cert);
                state.upstream.lowered = Some(result.lowered);
                finish_pass(
                    state,
                    PassId::Lower,
                    diags,
                    elapsed,
                    verbose,
                    &mut on_pass_complete,
                )?;
            }
            PassId::BuildGraph => {
                let t = Instant::now();
                let result = crate::graph::build_graph(
                    state.upstream.hir.as_ref().unwrap(),
                    state.upstream.resolved.as_ref().unwrap(),
                    &state.upstream.registry,
                );
                let elapsed = t.elapsed();
                let diags = result.diagnostics;
                state.upstream.graph = Some(result.graph);
                finish_pass(
                    state,
                    PassId::BuildGraph,
                    diags,
                    elapsed,
                    verbose,
                    &mut on_pass_complete,
                )?;
            }
            // ThirContext-dependent passes handled by run_thir_and_downstream
            PassId::Analyze | PassId::Schedule | PassId::BuildLir | PassId::Codegen => {
                unreachable!()
            }
        }
    }
    Ok(())
}

// ── ThirContext scoped block ───────────────────────────────────────────────

fn run_thir_and_downstream(
    state: &mut CompilationState,
    passes: &[PassId],
    codegen_options: &CodegenOptions,
    verbose: bool,
    on_pass_complete: &mut impl FnMut(PassId, &[Diagnostic]),
) -> Result<(), PipelineError> {
    // Build ThirContext — borrows from upstream (immutable).
    let thir = crate::thir::build_thir_context(
        state.upstream.hir.as_ref().unwrap(),
        state.upstream.resolved.as_ref().unwrap(),
        state.upstream.typed.as_ref().unwrap(),
        state.upstream.lowered.as_ref().unwrap(),
        &state.upstream.registry,
        state.upstream.graph.as_ref().unwrap(),
    );

    if passes.contains(&PassId::Analyze) {
        let t = Instant::now();
        let result = crate::analyze::analyze(&thir, state.upstream.graph.as_ref().unwrap());
        let elapsed = t.elapsed();
        let diags = result.diagnostics;
        state.downstream.analysis = Some(result.analysis);
        finish_pass_core(
            &mut state.diagnostics,
            &mut state.has_error,
            PassId::Analyze,
            diags,
            elapsed,
            verbose,
            on_pass_complete,
        )?;
    }

    if passes.contains(&PassId::Schedule) {
        let t = Instant::now();
        let result = crate::schedule::schedule(
            &thir,
            state.upstream.graph.as_ref().unwrap(),
            state.downstream.analysis.as_ref().unwrap(),
        );
        let elapsed = t.elapsed();
        let mut diags = result.diagnostics;
        state.downstream.schedule = Some(result.schedule);
        // Verify schedule postconditions (S1-S2) — before finish_pass_core
        // so cert failure diagnostics go through callback
        let task_names: Vec<String> = thir.hir.tasks.iter().map(|t| t.name.clone()).collect();
        let sched_cert = crate::schedule::verify_schedule(
            state.downstream.schedule.as_ref().unwrap(),
            state.upstream.graph.as_ref().unwrap(),
            &task_names,
        );
        if !sched_cert.all_pass() {
            let failed: Vec<_> = sched_cert
                .obligations()
                .iter()
                .filter(|(_, ok)| !ok)
                .map(|(name, _)| *name)
                .collect();
            diags.push(
                Diagnostic::new(
                    DiagLevel::Error,
                    thir.hir.program_span,
                    format!("schedule verification failed: {}", failed.join(", ")),
                )
                .with_code(codes::E0602),
            );
        }
        finish_pass_core(
            &mut state.diagnostics,
            &mut state.has_error,
            PassId::Schedule,
            diags,
            elapsed,
            verbose,
            on_pass_complete,
        )?;
    }

    if passes.contains(&PassId::BuildLir) {
        let t = Instant::now();
        state.downstream.lir = Some(crate::lir::build_lir(
            &thir,
            state.upstream.graph.as_ref().unwrap(),
            state.downstream.analysis.as_ref().unwrap(),
            state.downstream.schedule.as_ref().unwrap(),
        ));
        let elapsed = t.elapsed();
        // Verify LIR postconditions (R1-R2)
        let lir_cert = crate::lir::verify_lir(
            state.downstream.lir.as_ref().unwrap(),
            state.downstream.schedule.as_ref().unwrap(),
        );
        if !lir_cert.all_pass() {
            let failed: Vec<_> = lir_cert
                .obligations()
                .iter()
                .filter(|(_, ok)| !ok)
                .map(|(name, _)| *name)
                .collect();
            let diags = vec![Diagnostic::new(
                DiagLevel::Error,
                thir.hir.program_span,
                format!("LIR verification failed: {}", failed.join(", ")),
            )
            .with_code(codes::E0603)];
            finish_pass_core(
                &mut state.diagnostics,
                &mut state.has_error,
                PassId::BuildLir,
                diags,
                elapsed,
                verbose,
                on_pass_complete,
            )?;
        } else {
            finish_pass_no_diags(PassId::BuildLir, elapsed, verbose, on_pass_complete);
        }
    }
    // thir drops here — upstream borrows released

    if passes.contains(&PassId::Codegen) {
        let t = Instant::now();
        let result = crate::codegen::codegen_from_lir(
            state.upstream.graph.as_ref().unwrap(),
            state.downstream.schedule.as_ref().unwrap(),
            codegen_options,
            state.downstream.lir.as_ref().unwrap(),
        );
        let elapsed = t.elapsed();
        let diags = result.diagnostics;
        state.downstream.generated = Some(result.generated);
        finish_pass_core(
            &mut state.diagnostics,
            &mut state.has_error,
            PassId::Codegen,
            diags,
            elapsed,
            verbose,
            on_pass_complete,
        )?;
    }

    Ok(())
}
