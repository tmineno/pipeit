// dim_resolve.rs — Shared dimension/rate resolution helpers
//
// Extracted from analyze.rs and codegen.rs to eliminate duplication.
// Used by both analysis (SDF balance equations) and codegen (C++ param emission).

use std::collections::HashSet;

use crate::ast::*;
use crate::registry::{ActorMeta, ParamKind, ParamType, PortShape, TokenCount};
use crate::resolve::ResolvedProgram;

/// Resolve a shape dimension to a concrete u32 value.
pub fn resolve_shape_dim(
    dim: &ShapeDim,
    resolved: &ResolvedProgram,
    program: &Program,
) -> Option<u32> {
    match dim {
        ShapeDim::Literal(n, _) => Some(*n),
        ShapeDim::ConstRef(ident) => {
            let entry = resolved.consts.get(&ident.name)?;
            let stmt = &program.statements[entry.stmt_index];
            if let StatementKind::Const(c) = &stmt.kind {
                match &c.value {
                    Value::Scalar(Scalar::Number(n, _, _)) => Some(*n as u32),
                    _ => None,
                }
            } else {
                None
            }
        }
    }
}

/// Resolve an Arg to a u32 value (for token count resolution).
pub fn resolve_arg_to_u32(arg: &Arg, resolved: &ResolvedProgram, program: &Program) -> Option<u32> {
    match arg {
        Arg::Value(Value::Scalar(Scalar::Number(n, _, _))) => Some(*n as u32),
        Arg::Value(Value::Array(elems, _)) => Some(elems.len() as u32),
        Arg::ConstRef(ident) => {
            let entry = resolved.consts.get(&ident.name)?;
            let stmt = &program.statements[entry.stmt_index];
            if let StatementKind::Const(c) = &stmt.kind {
                match &c.value {
                    Value::Scalar(Scalar::Number(n, _, _)) => Some(*n as u32),
                    Value::Array(elems, _) => Some(elems.len() as u32),
                    _ => None,
                }
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Infer a symbolic dimension parameter value from span-typed arguments.
///
/// If the actor has a span param (e.g., `std::span<const float>` for coefficients)
/// whose compile-time length is known, and the target dimension is the first
/// unresolved symbolic dim in declaration order, bind the span length to it.
pub fn infer_dim_param_from_span_args(
    dim_name: &str,
    actor_meta: &ActorMeta,
    actor_args: &[Arg],
    resolved: &ResolvedProgram,
    program: &Program,
) -> Option<u32> {
    // Only applies to compile-time integer dimension params.
    let dim_param = actor_meta.params.iter().find(|p| p.name == dim_name)?;
    if dim_param.kind != ParamKind::Param || dim_param.param_type != ParamType::Int {
        return None;
    }
    // Find a span length source (first span arg with known compile-time length).
    let span_len = actor_meta
        .params
        .iter()
        .enumerate()
        .find_map(|(idx, param)| {
            if param.kind != ParamKind::Param {
                return None;
            }
            if !matches!(
                param.param_type,
                ParamType::SpanFloat | ParamType::SpanChar | ParamType::SpanTypeParam(_)
            ) {
                return None;
            }
            actor_args
                .get(idx)
                .and_then(|arg| resolve_arg_to_u32(arg, resolved, program))
        })?;

    // Disambiguation rule: bind span length only to the first unresolved
    // dimension param (in actor param declaration order).
    let mut dim_names: HashSet<&str> = HashSet::new();
    for dim in actor_meta
        .in_shape
        .dims
        .iter()
        .chain(actor_meta.out_shape.dims.iter())
    {
        if let TokenCount::Symbolic(sym) = dim {
            dim_names.insert(sym.as_str());
        }
    }
    let first_unresolved_dim = actor_meta.params.iter().enumerate().find_map(|(idx, p)| {
        if p.kind != ParamKind::Param || p.param_type != ParamType::Int {
            return None;
        }
        if !dim_names.contains(p.name.as_str()) {
            return None;
        }
        let explicit = actor_args
            .get(idx)
            .and_then(|arg| resolve_arg_to_u32(arg, resolved, program));
        if explicit.is_some() {
            return None;
        }
        Some(p.name.as_str())
    })?;

    if first_unresolved_dim == dim_name {
        Some(span_len)
    } else {
        None
    }
}

/// Return the span-argument-derived length for a dimension, ignoring whether the
/// dimension already has an explicit argument.  Used for conflict detection only:
/// the caller compares this against explicit args / shape constraints to emit
/// diagnostics when the sources disagree.
pub fn span_arg_length_for_dim(
    dim_name: &str,
    actor_meta: &ActorMeta,
    actor_args: &[Arg],
    resolved: &ResolvedProgram,
    program: &Program,
) -> Option<u32> {
    // Only applies to compile-time integer dimension params.
    let dim_param = actor_meta.params.iter().find(|p| p.name == dim_name)?;
    if dim_param.kind != ParamKind::Param || dim_param.param_type != ParamType::Int {
        return None;
    }
    // Find the first span arg with known compile-time length.
    let span_len = actor_meta
        .params
        .iter()
        .enumerate()
        .find_map(|(idx, param)| {
            if param.kind != ParamKind::Param {
                return None;
            }
            if !matches!(
                param.param_type,
                ParamType::SpanFloat | ParamType::SpanChar | ParamType::SpanTypeParam(_)
            ) {
                return None;
            }
            actor_args
                .get(idx)
                .and_then(|arg| resolve_arg_to_u32(arg, resolved, program))
        })?;

    // Check that this dim is the first symbolic dim param in declaration order
    // (the one the span length would naturally bind to).
    let mut dim_names: HashSet<&str> = HashSet::new();
    for dim in actor_meta
        .in_shape
        .dims
        .iter()
        .chain(actor_meta.out_shape.dims.iter())
    {
        if let TokenCount::Symbolic(sym) = dim {
            dim_names.insert(sym.as_str());
        }
    }
    let first_sym_dim = actor_meta.params.iter().find_map(|p| {
        if p.kind != ParamKind::Param || p.param_type != ParamType::Int {
            return None;
        }
        if !dim_names.contains(p.name.as_str()) {
            return None;
        }
        Some(p.name.as_str())
    })?;

    if first_sym_dim == dim_name {
        Some(span_len)
    } else {
        None
    }
}

/// Resolve a PortShape to a concrete rate (product of all dimensions).
///
/// Resolution precedence per dimension:
/// 1. Literal values in shape definition
/// 2. Explicit actor arguments
/// 3. Call-site shape constraint
/// 4. Span-derived inference
///
/// Returns None if any dimension cannot be resolved.
pub fn resolve_port_rate(
    shape: &PortShape,
    actor_meta: &ActorMeta,
    actor_args: &[Arg],
    shape_constraint: Option<&[ShapeDim]>,
    resolved: &ResolvedProgram,
    program: &Program,
) -> Option<u32> {
    let mut rate: u32 = 1;
    for (dim_idx, dim) in shape.dims.iter().enumerate() {
        let dim_val = match dim {
            TokenCount::Literal(n) => *n,
            TokenCount::Symbolic(name) => {
                // 1) Explicit arg
                let from_arg = actor_meta
                    .params
                    .iter()
                    .position(|p| p.name == *name)
                    .and_then(|idx| actor_args.get(idx))
                    .and_then(|arg| resolve_arg_to_u32(arg, resolved, program));
                if let Some(v) = from_arg {
                    v
                } else if let Some(v) = shape_constraint
                    .and_then(|sc| sc.get(dim_idx))
                    .and_then(|sd| resolve_shape_dim(sd, resolved, program))
                {
                    // 2) Shape constraint
                    v
                } else if let Some(v) =
                    infer_dim_param_from_span_args(name, actor_meta, actor_args, resolved, program)
                {
                    // 3) Span-derived
                    v
                } else {
                    return None;
                }
            }
        };
        rate = rate.checked_mul(dim_val)?;
    }
    Some(rate)
}
