# ADR-007: Frame dimension inference and shape constraints for v0.2.0

## Context

The current Pipit compiler represents actor port sizes as a single scalar `TokenCount` — either a fixed integer or a symbolic parameter name. This suffices for rank-1 (vector) ports but cannot express higher-rank data (e.g., images with height × width × channels) or provide frame-level shape constraints at the call site.

The v0.2.0 language spec §13 proposes introducing multi-dimensional shape metadata to actor ports, enabling frame-based processing as a first-class concept while preserving SDF static schedulability.

## Decision

Adopt the following shape model for v0.2.0:

- **Shape representation**: Each port has a shape vector `S = [d0, d1, ..., dk-1]` where each `di` is a positive compile-time integer. The SDF token rate is `|S| = Π di` (product of all dimensions).
- **Backward compatibility**: Existing `IN(T, N)` / `OUT(T, N)` is treated as rank-1 shorthand `IN(T, SHAPE(N))`. All v0.1.x programs compile identically.
- **Actor declaration syntax** (C++ headers): `IN(type, SHAPE(d0, d1, ...))` and `OUT(type, SHAPE(d0, d1, ...))` where dimensions are literals or `PARAM` names.
- **Call-site shape constraints** (PDL): `actor(...)[d0, d1, ...]` with integer literals or `const` references.
- **Compile-time-only dimensions**: Shape dimensions must resolve at compile time. `$param` (runtime parameters) cannot be used as shape dimensions — this is a compile error.
- **Flat runtime buffers**: Runtime buffer layout remains 1D contiguous. Shape is purely compile-time metadata for rate computation and validation. No multi-dimensional buffer layout, no runtime shape discovery.
- **Dimension inference**: The compiler resolves symbolic dimensions from actor arguments, explicit `[...]` constraints, and SDF balance equations. Ambiguous or conflicting dimensions produce compile errors with hints.
- **Implicit dimension parameter resolution**: When an actor has `PARAM(int, N)` used in `SHAPE(N)`, the dimension can be inferred from the call-site `[...]` constraint without the user passing `N` as an explicit argument. The compiler auto-fills the parameter at codegen time. This reduces boilerplate — e.g., `frame_gain($gain)[256]` instead of `frame_gain(256, $gain)`.

## Consequences

- `PortShape` type added alongside existing `TokenCount` in the registry; `ActorMeta` carries both `in_shape`/`out_shape` and legacy `in_count`/`out_count` during transition
- Parser extended with optional `[...]` shape constraint on actor calls
- Analysis phase gains shape-aware rate resolution with fallback to scalar resolution for backward compatibility
- Generated C++ is unchanged in structure — buffer sizes use `product(shape)` instead of scalar count
- No runtime library changes required
- New diagnostic messages for unresolved dimensions, conflicting constraints, runtime param in shape, and rank mismatches

## Alternatives

- **Runtime shape metadata**: Pass shape info to actors at runtime. Rejected — breaks SDF static scheduling guarantee and adds runtime overhead.
- **Multi-dimensional buffer layout**: Use row-major/column-major layout in ring buffers. Rejected — adds complexity with no benefit since actors use flat indexing (`in[i]`, `out[i]`).
- **Implicit shape inference from downstream**: Infer upstream shapes from downstream consumer shapes. Deferred — increases solver complexity; explicit constraints are sufficient for v0.2.0.

## Exit criteria

Revisit if:

- Runtime shape discovery or dynamic reshaping is needed
- Non-flat buffer layouts become necessary for cache optimization
- Shape inference needs to propagate across task boundaries (inter-task shape unification)
