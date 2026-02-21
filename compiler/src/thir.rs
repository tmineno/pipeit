// thir.rs — Typed HIR context: unified query interface for downstream phases
//
// Wraps phase outputs (HIR, resolved, typed, lowered) with precomputed
// metadata tables so downstream phases (graph, analyze, schedule) never
// access the raw AST.
//
// Preconditions: HIR and graph must be constructed first; param C++ type
//   resolution requires scanning graph nodes for actor param type info.
// Postconditions: all downstream-phase queries are serviced from this context.
// Failure modes: none (construction is infallible).
// Side effects: none (read-only wrapper).
//
// See ADR-024 for design rationale.

use std::collections::HashMap;

use crate::ast::{Arg, Scalar, SetValue, ShapeDim, Value};
use crate::graph::{NodeKind, ProgramGraph};
use crate::hir::{HirConst, HirParam, HirProgram, HirSetDirective, HirTask};
use crate::id::CallId;
use crate::lower::LoweredProgram;
use crate::registry::{ActorMeta, ParamType, Registry};
use crate::resolve::ResolvedProgram;
use crate::subgraph_index::subgraphs_of;
use crate::type_infer::TypedProgram;

// ── ThirContext ─────────────────────────────────────────────────────────────

/// Unified query interface wrapping phase outputs + precomputed metadata.
///
/// Downstream phases (analyze, schedule) consume this instead of the raw AST.
/// Provides fast indexed lookups for tasks, consts, params, and set directives.
pub struct ThirContext<'a> {
    // ── Phase outputs ──
    pub resolved: &'a ResolvedProgram,
    pub typed: &'a TypedProgram,
    pub lowered: &'a LoweredProgram,
    pub registry: &'a Registry,
    pub hir: &'a HirProgram,

    // ── Precomputed indices ──
    task_index: HashMap<String, usize>,
    const_index: HashMap<String, usize>,
    param_index: HashMap<String, usize>,
    set_index: HashMap<String, usize>,

    // ── Precomputed set-directive values ──
    pub mem_bytes: u64,
    pub tick_rate_hz: f64,
    pub timer_spin: Option<f64>,
    pub overrun_policy: String,

    // ── Precomputed param C++ types ──
    pub param_cpp_types: HashMap<String, &'static str>,
}

// ── Construction ────────────────────────────────────────────────────────────

/// Build a ThirContext from phase outputs + graph.
///
/// The graph is needed for param C++ type resolution (scanning actor nodes
/// to determine what type each runtime param resolves to).
pub fn build_thir_context<'a>(
    hir: &'a HirProgram,
    resolved: &'a ResolvedProgram,
    typed: &'a TypedProgram,
    lowered: &'a LoweredProgram,
    registry: &'a Registry,
    graph: &ProgramGraph,
) -> ThirContext<'a> {
    // Build lookup indices
    let task_index: HashMap<String, usize> = hir
        .tasks
        .iter()
        .enumerate()
        .map(|(i, t)| (t.name.clone(), i))
        .collect();
    let const_index: HashMap<String, usize> = hir
        .consts
        .iter()
        .enumerate()
        .map(|(i, c)| (c.name.clone(), i))
        .collect();
    let param_index: HashMap<String, usize> = hir
        .params
        .iter()
        .enumerate()
        .map(|(i, p)| (p.name.clone(), i))
        .collect();
    let set_index: HashMap<String, usize> = hir
        .set_directives
        .iter()
        .enumerate()
        .map(|(i, s)| (s.name.clone(), i))
        .collect();

    // Extract common set-directive values
    let mem_bytes =
        find_set_size(&hir.set_directives, &set_index, "mem").unwrap_or(64 * 1024 * 1024);
    let tick_rate_hz =
        find_set_freq(&hir.set_directives, &set_index, "tick_rate").unwrap_or(1_000_000.0);
    let timer_spin = find_set_number(&hir.set_directives, &set_index, "timer_spin");
    let overrun_policy = find_set_ident(&hir.set_directives, &set_index, "overrun")
        .unwrap_or("stop")
        .to_string();

    // Resolve param C++ types by scanning graph nodes
    let param_cpp_types = resolve_param_cpp_types(hir, lowered, registry, graph);

    ThirContext {
        resolved,
        typed,
        lowered,
        registry,
        hir,
        task_index,
        const_index,
        param_index,
        set_index,
        mem_bytes,
        tick_rate_hz,
        timer_spin,
        overrun_policy,
        param_cpp_types,
    }
}

// ── Query methods ───────────────────────────────────────────────────────────

impl<'a> ThirContext<'a> {
    /// Look up a task by name.
    pub fn task_info(&self, name: &str) -> Option<&HirTask> {
        self.task_index.get(name).map(|&i| &self.hir.tasks[i])
    }

    /// Look up a const by name.
    pub fn const_info(&self, name: &str) -> Option<&HirConst> {
        self.const_index.get(name).map(|&i| &self.hir.consts[i])
    }

    /// Look up a param by name.
    pub fn param_info(&self, name: &str) -> Option<&HirParam> {
        self.param_index.get(name).map(|&i| &self.hir.params[i])
    }

    /// Look up a set directive by name.
    pub fn set_directive(&self, name: &str) -> Option<&HirSetDirective> {
        self.set_index
            .get(name)
            .map(|&i| &self.hir.set_directives[i])
    }

    /// Get the C++ type for a runtime param. Falls back to type inferred from
    /// the param's default value if no graph-based resolution is available.
    pub fn param_cpp_type(&self, name: &str) -> &'static str {
        if let Some(&t) = self.param_cpp_types.get(name) {
            return t;
        }
        // Fallback: infer from default value
        if let Some(p) = self.param_info(name) {
            return scalar_cpp_type(&p.default_value);
        }
        "double"
    }

    /// Look up concrete actor metadata (lowered → registry fallback).
    pub fn concrete_actor(&self, actor_name: &str, call_id: CallId) -> Option<&ActorMeta> {
        if let Some(meta) = self.lowered.concrete_actors.get(&call_id) {
            return Some(meta);
        }
        self.registry.lookup(actor_name)
    }

    /// Resolve a const name to a u32 value (for dimension resolution).
    pub fn resolve_const_to_u32(&self, name: &str) -> Option<u32> {
        let c = self.const_info(name)?;
        match &c.value {
            Value::Scalar(Scalar::Number(n, _, _)) => Some(*n as u32),
            _ => None,
        }
    }

    /// Resolve a const name to its array length (for dimension resolution).
    pub fn resolve_const_array_len(&self, name: &str) -> Option<u32> {
        let c = self.const_info(name)?;
        match &c.value {
            Value::Array(elems, _) => Some(elems.len() as u32),
            Value::Scalar(Scalar::Number(n, _, _)) => Some(*n as u32),
            _ => None,
        }
    }

    // ── Dimension resolution (replaces dim_resolve.rs Program access) ───

    /// Resolve a ShapeDim to a concrete u32 value.
    pub fn resolve_shape_dim(&self, dim: &ShapeDim) -> Option<u32> {
        match dim {
            ShapeDim::Literal(n, _) => Some(*n),
            ShapeDim::ConstRef(ident) => self.resolve_const_to_u32(&ident.name),
        }
    }

    /// Resolve an Arg to a u32 value (number, array length, or const ref).
    pub fn resolve_arg_to_u32(&self, arg: &Arg) -> Option<u32> {
        match arg {
            Arg::Value(Value::Scalar(Scalar::Number(n, _, _))) => Some(*n as u32),
            Arg::Value(Value::Array(elems, _)) => Some(elems.len() as u32),
            Arg::ConstRef(ident) => self.resolve_const_array_len(&ident.name),
            _ => None,
        }
    }
}

// ── Internal helpers ────────────────────────────────────────────────────────

fn find_set_size(
    directives: &[HirSetDirective],
    index: &HashMap<String, usize>,
    name: &str,
) -> Option<u64> {
    let &i = index.get(name)?;
    match &directives[i].value {
        SetValue::Size(v, _) => Some(*v),
        _ => None,
    }
}

fn find_set_freq(
    directives: &[HirSetDirective],
    index: &HashMap<String, usize>,
    name: &str,
) -> Option<f64> {
    let &i = index.get(name)?;
    match &directives[i].value {
        SetValue::Freq(f, _) => Some(*f),
        _ => None,
    }
}

fn find_set_number(
    directives: &[HirSetDirective],
    index: &HashMap<String, usize>,
    name: &str,
) -> Option<f64> {
    let &i = index.get(name)?;
    match &directives[i].value {
        SetValue::Number(n, _) => Some(*n),
        _ => None,
    }
}

fn find_set_ident<'a>(
    directives: &'a [HirSetDirective],
    index: &HashMap<String, usize>,
    name: &str,
) -> Option<&'a str> {
    let &i = index.get(name)?;
    match &directives[i].value {
        SetValue::Ident(ident) => Some(&ident.name),
        _ => None,
    }
}

/// Infer C++ type from a scalar default value.
fn scalar_cpp_type(s: &Scalar) -> &'static str {
    match s {
        Scalar::Number(_, _, is_int) if *is_int => "int",
        Scalar::Number(..) => "double",
        Scalar::StringLit(..) => "const char*",
        _ => "double",
    }
}

/// Resolve param C++ types by scanning graph nodes for actor calls that
/// reference each param, then looking up the actor's parameter type.
fn resolve_param_cpp_types(
    hir: &HirProgram,
    lowered: &LoweredProgram,
    registry: &Registry,
    graph: &ProgramGraph,
) -> HashMap<String, &'static str> {
    let mut result = HashMap::new();

    // Collect param names for fast lookup
    let param_names: HashMap<&str, &Scalar> = hir
        .params
        .iter()
        .map(|p| (p.name.as_str(), &p.default_value))
        .collect();

    if param_names.is_empty() {
        return result;
    }

    // Scan all graph nodes for ParamRef args
    for task_graph in graph.tasks.values() {
        for sub in subgraphs_of(task_graph) {
            for node in &sub.nodes {
                if let NodeKind::Actor {
                    name,
                    args,
                    call_id,
                    ..
                } = &node.kind
                {
                    for (i, arg) in args.iter().enumerate() {
                        if let Arg::ParamRef(ident) = arg {
                            if result.contains_key(&ident.name) {
                                continue; // already resolved
                            }
                            if !param_names.contains_key(ident.name.as_str()) {
                                continue; // not a known param
                            }
                            // Look up actor metadata
                            let meta = lowered
                                .concrete_actors
                                .get(call_id)
                                .or_else(|| registry.lookup(name));
                            if let Some(meta) = meta {
                                if let Some(p) = meta.params.get(i) {
                                    let cpp_type = match p.param_type {
                                        ParamType::Int => "int",
                                        ParamType::Float => "float",
                                        ParamType::Double => "double",
                                        _ => {
                                            let fallback = param_names[ident.name.as_str()];
                                            scalar_cpp_type(fallback)
                                        }
                                    };
                                    result.insert(ident.name.clone(), cpp_type);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Fill in remaining params with fallback from default value
    for (name, default) in &param_names {
        result
            .entry((*name).to_string())
            .or_insert_with(|| scalar_cpp_type(default));
    }

    result
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Span;
    use crate::hir::{
        HirActorCall, HirPipeElem, HirPipeExpr, HirPipeSource, HirPipeline, HirTaskBody,
    };
    use crate::id::{DefId, TaskId};
    use crate::lower::LoweredProgram;
    use crate::type_infer::TypedProgram;
    use chumsky::span::Span as _;

    fn sp(start: usize, end: usize) -> Span {
        Span::new((), start..end)
    }

    fn empty_resolved() -> ResolvedProgram {
        ResolvedProgram {
            consts: HashMap::new(),
            params: HashMap::new(),
            defines: HashMap::new(),
            tasks: HashMap::new(),
            buffers: HashMap::new(),
            call_resolutions: HashMap::new(),
            task_resolutions: HashMap::new(),
            probes: Vec::new(),
            call_ids: HashMap::new(),
            call_spans: HashMap::new(),
            def_ids: HashMap::new(),
            task_ids: HashMap::new(),
        }
    }

    fn empty_typed() -> TypedProgram {
        TypedProgram {
            type_assignments: HashMap::new(),
            widenings: Vec::new(),
            mono_actors: HashMap::new(),
        }
    }

    fn empty_lowered() -> LoweredProgram {
        LoweredProgram {
            concrete_actors: HashMap::new(),
            widening_nodes: Vec::new(),
            type_instantiations: HashMap::new(),
        }
    }

    fn empty_graph() -> ProgramGraph {
        ProgramGraph {
            tasks: HashMap::new(),
            inter_task_edges: Vec::new(),
            cycles: Vec::new(),
        }
    }

    fn sample_hir() -> HirProgram {
        HirProgram {
            tasks: vec![HirTask {
                name: "main".to_string(),
                task_id: TaskId(0),
                freq_hz: 48000.0,
                freq_span: sp(0, 10),
                body: HirTaskBody::Pipeline(HirPipeline {
                    pipes: vec![HirPipeExpr {
                        source: HirPipeSource::ActorCall(HirActorCall {
                            name: "constant".to_string(),
                            call_id: CallId(0),
                            call_span: sp(20, 30),
                            args: vec![Arg::Value(Value::Scalar(Scalar::Number(
                                1.0,
                                sp(29, 32),
                                false,
                            )))],
                            type_args: vec![],
                            shape_constraint: None,
                        }),
                        elements: vec![HirPipeElem::ActorCall(HirActorCall {
                            name: "stdout".to_string(),
                            call_id: CallId(1),
                            call_span: sp(35, 41),
                            args: vec![],
                            type_args: vec![],
                            shape_constraint: None,
                        })],
                        sink: None,
                        span: sp(20, 41),
                    }],
                    span: sp(0, 50),
                }),
            }],
            consts: vec![HirConst {
                def_id: DefId(0),
                name: "N".to_string(),
                value: Value::Scalar(Scalar::Number(256.0, sp(60, 63), true)),
            }],
            params: vec![HirParam {
                def_id: DefId(1),
                name: "gain".to_string(),
                default_value: Scalar::Number(1.0, sp(70, 73), false),
            }],
            set_directives: vec![
                HirSetDirective {
                    name: "mem".to_string(),
                    value: SetValue::Size(64 * 1024 * 1024, sp(80, 85)),
                },
                HirSetDirective {
                    name: "tick_rate".to_string(),
                    value: SetValue::Freq(1000.0, sp(90, 95)),
                },
            ],
            expanded_call_ids: HashMap::new(),
            expanded_call_spans: HashMap::new(),
        }
    }

    #[test]
    fn thir_task_lookup() {
        let hir = sample_hir();
        let resolved = empty_resolved();
        let typed = empty_typed();
        let lowered = empty_lowered();
        let registry = Registry::empty();
        let graph = empty_graph();
        let thir = build_thir_context(&hir, &resolved, &typed, &lowered, &registry, &graph);

        let task = thir.task_info("main").unwrap();
        assert_eq!(task.freq_hz, 48000.0);
        assert!(thir.task_info("nonexistent").is_none());
    }

    #[test]
    fn thir_const_lookup() {
        let hir = sample_hir();
        let resolved = empty_resolved();
        let typed = empty_typed();
        let lowered = empty_lowered();
        let registry = Registry::empty();
        let graph = empty_graph();
        let thir = build_thir_context(&hir, &resolved, &typed, &lowered, &registry, &graph);

        assert_eq!(thir.resolve_const_to_u32("N"), Some(256));
        assert_eq!(thir.resolve_const_to_u32("missing"), None);
    }

    #[test]
    fn thir_param_cpp_type_fallback() {
        let hir = sample_hir();
        let resolved = empty_resolved();
        let typed = empty_typed();
        let lowered = empty_lowered();
        let registry = Registry::empty();
        let graph = empty_graph();
        let thir = build_thir_context(&hir, &resolved, &typed, &lowered, &registry, &graph);

        // No graph usage → falls back to default value type inference
        assert_eq!(thir.param_cpp_type("gain"), "double");
    }

    #[test]
    fn thir_set_directives() {
        let hir = sample_hir();
        let resolved = empty_resolved();
        let typed = empty_typed();
        let lowered = empty_lowered();
        let registry = Registry::empty();
        let graph = empty_graph();
        let thir = build_thir_context(&hir, &resolved, &typed, &lowered, &registry, &graph);

        assert_eq!(thir.mem_bytes, 64 * 1024 * 1024);
        assert_eq!(thir.tick_rate_hz, 1000.0);
        assert!(thir.timer_spin.is_none());
        assert_eq!(thir.overrun_policy, "stop");
    }

    #[test]
    fn thir_resolve_arg_to_u32() {
        let hir = sample_hir();
        let resolved = empty_resolved();
        let typed = empty_typed();
        let lowered = empty_lowered();
        let registry = Registry::empty();
        let graph = empty_graph();
        let thir = build_thir_context(&hir, &resolved, &typed, &lowered, &registry, &graph);

        // Number arg
        let num_arg = Arg::Value(Value::Scalar(Scalar::Number(42.0, sp(0, 2), true)));
        assert_eq!(thir.resolve_arg_to_u32(&num_arg), Some(42));

        // Array arg
        let arr_arg = Arg::Value(Value::Array(
            vec![
                Scalar::Number(1.0, sp(0, 1), false),
                Scalar::Number(2.0, sp(2, 3), false),
                Scalar::Number(3.0, sp(4, 5), false),
            ],
            sp(0, 6),
        ));
        assert_eq!(thir.resolve_arg_to_u32(&arr_arg), Some(3));

        // ConstRef arg (resolved from HIR const "N" = 256)
        let const_arg = Arg::ConstRef(crate::ast::Ident {
            name: "N".to_string(),
            span: sp(0, 1),
        });
        assert_eq!(thir.resolve_arg_to_u32(&const_arg), Some(256));

        // ParamRef → None
        let param_arg = Arg::ParamRef(crate::ast::Ident {
            name: "gain".to_string(),
            span: sp(0, 1),
        });
        assert_eq!(thir.resolve_arg_to_u32(&param_arg), None);
    }

    #[test]
    fn thir_resolve_shape_dim() {
        let hir = sample_hir();
        let resolved = empty_resolved();
        let typed = empty_typed();
        let lowered = empty_lowered();
        let registry = Registry::empty();
        let graph = empty_graph();
        let thir = build_thir_context(&hir, &resolved, &typed, &lowered, &registry, &graph);

        // Literal dim
        let lit = ShapeDim::Literal(128, sp(0, 3));
        assert_eq!(thir.resolve_shape_dim(&lit), Some(128));

        // ConstRef dim
        let cref = ShapeDim::ConstRef(crate::ast::Ident {
            name: "N".to_string(),
            span: sp(0, 1),
        });
        assert_eq!(thir.resolve_shape_dim(&cref), Some(256));
    }
}
