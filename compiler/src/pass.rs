// pass.rs — Pass descriptor module: metadata, dependency resolution, artifact IDs
//
// Declares the compiler's 9 semantic passes (parse is outside the runner),
// their dependency edges, and the artifacts they produce. Used by the pipeline
// runner to compute minimal pass subsets for each --emit target.
//
// See ADR-020 for design rationale.

use std::collections::HashSet;

// ── Pass and Artifact identifiers ──────────────────────────────────────────

/// Identifies each compiler pass (parse excluded — handled before the runner).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PassId {
    Resolve,
    BuildHir,
    TypeInfer,
    Lower,
    BuildGraph,
    Analyze,
    Schedule,
    BuildLir,
    Codegen,
}

/// Machine-readable artifact identifiers. Each maps to a concrete type
/// in the compilation state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArtifactId {
    Resolved,  // ResolvedProgram
    IdAlloc,   // IdAllocator
    Hir,       // HirProgram
    Typed,     // TypedProgram
    Lowered,   // LoweredProgram
    Cert,      // Cert
    Graph,     // ProgramGraph
    Analysis,  // AnalyzedProgram
    Schedule,  // ScheduledProgram
    Lir,       // LirProgram
    Generated, // GeneratedCode
}

// ── Pass descriptor ────────────────────────────────────────────────────────

/// Static metadata about a compiler pass.
pub struct PassDescriptor {
    /// Human-readable name for diagnostics/verbose output.
    pub name: &'static str,
    /// Pass dependencies (other passes whose outputs this pass consumes).
    pub inputs: &'static [PassId],
    /// Artifacts this pass produces.
    pub outputs: &'static [ArtifactId],
    /// Placeholder — describes what invalidates this pass's output.
    /// Will become a deterministic hash in Phase 3b.
    pub invalidation_key: &'static str,
    /// Placeholder — pre/post conditions (documentation only for now).
    pub invariants: &'static str,
}

/// Return the static descriptor for a given pass.
pub fn descriptor(id: PassId) -> PassDescriptor {
    match id {
        PassId::Resolve => PassDescriptor {
            name: "resolve",
            inputs: &[],
            outputs: &[ArtifactId::Resolved, ArtifactId::IdAlloc],
            invalidation_key: "source + registry actors",
            invariants: "all names resolved, call_ids assigned",
        },
        PassId::BuildHir => PassDescriptor {
            name: "build_hir",
            inputs: &[PassId::Resolve],
            outputs: &[ArtifactId::Hir],
            invalidation_key: "program + resolved + id_alloc",
            invariants: "defines expanded, all calls have fresh CallIds",
        },
        PassId::TypeInfer => PassDescriptor {
            name: "type_infer",
            inputs: &[PassId::BuildHir],
            outputs: &[ArtifactId::Typed],
            invalidation_key: "hir + resolved + registry",
            invariants: "all calls monomorphized, widenings identified",
        },
        PassId::Lower => PassDescriptor {
            name: "lower",
            inputs: &[PassId::TypeInfer],
            outputs: &[ArtifactId::Lowered, ArtifactId::Cert],
            invalidation_key: "hir + resolved + typed + registry",
            invariants: "L1-L5 obligations verified, concrete_actors populated",
        },
        PassId::BuildGraph => PassDescriptor {
            name: "build_graph",
            inputs: &[PassId::BuildHir],
            outputs: &[ArtifactId::Graph],
            invalidation_key: "hir + resolved + registry",
            invariants: "SDF graph is acyclic per subgraph",
        },
        PassId::Analyze => PassDescriptor {
            name: "analyze",
            inputs: &[PassId::Lower, PassId::BuildGraph],
            outputs: &[ArtifactId::Analysis],
            invalidation_key: "thir_context + graph",
            invariants: "repetition vectors computed, shapes inferred",
        },
        PassId::Schedule => PassDescriptor {
            name: "schedule",
            inputs: &[PassId::Analyze],
            outputs: &[ArtifactId::Schedule],
            invalidation_key: "thir_context + graph + analysis",
            invariants: "all tasks scheduled, edge buffers sized",
        },
        PassId::BuildLir => PassDescriptor {
            name: "build_lir",
            inputs: &[PassId::Schedule],
            outputs: &[ArtifactId::Lir],
            invalidation_key: "thir_context + graph + analysis + schedule",
            invariants: "all codegen data pre-resolved into LIR",
        },
        PassId::Codegen => PassDescriptor {
            name: "codegen",
            inputs: &[PassId::BuildLir],
            outputs: &[ArtifactId::Generated],
            invalidation_key: "graph + schedule + lir + codegen_options",
            invariants: "valid C++ emitted",
        },
    }
}

// ── Dependency resolution ──────────────────────────────────────────────────

/// All 9 pass IDs in declaration order (used for iteration).
pub const ALL_PASSES: [PassId; 9] = [
    PassId::Resolve,
    PassId::BuildHir,
    PassId::TypeInfer,
    PassId::Lower,
    PassId::BuildGraph,
    PassId::Analyze,
    PassId::Schedule,
    PassId::BuildLir,
    PassId::Codegen,
];

/// Compute the minimal ordered set of passes needed to produce `terminal`.
/// Returns passes in topological (execution) order.
pub fn required_passes(terminal: PassId) -> Vec<PassId> {
    let mut visited = HashSet::new();
    let mut order = Vec::new();
    visit(terminal, &mut visited, &mut order);
    order
}

fn visit(id: PassId, visited: &mut HashSet<PassId>, order: &mut Vec<PassId>) {
    if !visited.insert(id) {
        return;
    }
    for &dep in descriptor(id).inputs {
        visit(dep, visited, order);
    }
    order.push(id);
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_passes_build_graph_skips_type_infer_and_lower() {
        let passes = required_passes(PassId::BuildGraph);
        assert_eq!(
            passes,
            vec![PassId::Resolve, PassId::BuildHir, PassId::BuildGraph]
        );
        assert!(!passes.contains(&PassId::TypeInfer));
        assert!(!passes.contains(&PassId::Lower));
        assert!(!passes.contains(&PassId::Analyze));
    }

    #[test]
    fn required_passes_codegen_includes_all() {
        let passes = required_passes(PassId::Codegen);
        assert_eq!(passes.len(), 9);
        assert_eq!(
            passes,
            vec![
                PassId::Resolve,
                PassId::BuildHir,
                PassId::TypeInfer,
                PassId::Lower,
                PassId::BuildGraph,
                PassId::Analyze,
                PassId::Schedule,
                PassId::BuildLir,
                PassId::Codegen,
            ]
        );
    }

    #[test]
    fn required_passes_schedule() {
        let passes = required_passes(PassId::Schedule);
        assert_eq!(
            passes,
            vec![
                PassId::Resolve,
                PassId::BuildHir,
                PassId::TypeInfer,
                PassId::Lower,
                PassId::BuildGraph,
                PassId::Analyze,
                PassId::Schedule,
            ]
        );
    }

    #[test]
    fn required_passes_resolve_is_minimal() {
        let passes = required_passes(PassId::Resolve);
        assert_eq!(passes, vec![PassId::Resolve]);
    }

    #[test]
    fn no_parse_in_pass_id() {
        // Parse is handled outside the runner; PassId has no Parse variant.
        for pass in &ALL_PASSES {
            assert_ne!(descriptor(*pass).name, "parse");
        }
    }

    #[test]
    fn all_descriptors_have_outputs() {
        for pass in &ALL_PASSES {
            let desc = descriptor(*pass);
            assert!(
                !desc.outputs.is_empty(),
                "pass {:?} has no outputs declared",
                pass
            );
        }
    }

    #[test]
    fn dependency_edges_are_consistent() {
        // Every pass listed as an input must be a valid PassId
        // (guaranteed by type system, but verify no cycles in small graph).
        for pass in &ALL_PASSES {
            let desc = descriptor(*pass);
            for dep in desc.inputs {
                // Dependency must come before this pass in topological order
                let dep_passes = required_passes(*pass);
                let dep_pos = dep_passes.iter().position(|p| p == dep);
                let self_pos = dep_passes.iter().position(|p| p == pass);
                assert!(
                    dep_pos.unwrap() < self_pos.unwrap(),
                    "{:?} depends on {:?} but it comes later in topological order",
                    pass,
                    dep
                );
            }
        }
    }
}
