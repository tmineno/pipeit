# ADR-016: Actor Polymorphism, Monomorphization Strategy, and Safe Numeric Widening

## Context

Pipit v0.1–v0.2.2 required a separate `ACTOR` definition for each concrete type. For example, `fir` operating on `float` and `fir` operating on `double` needed two distinct actor names (`fir_f32`, `fir_f64`), and PDL users had to reference the concrete name. This led to:

1. **Duplicated actor implementations**: Identical algorithms re-declared for every wire type.
2. **Verbose PDL**: Users must know and write concrete actor names even when the type is obvious from context.
3. **No implicit numeric widening**: Connecting `int32`-producing and `float`-consuming actors required an explicit cast actor, even though the conversion is always safe.

The v0.2.0 design principle was **no implicit conversions**. This was maximally safe but created friction in common signal processing patterns (e.g., ADC `int16` → `float` processing chains).

## Decision

### 1. Polymorphic actor calls

PDL supports two invocation forms:

- **Explicit type arguments**: `actor<float>(...)` — user specifies the type parameter.
- **Inferred type arguments**: `actor(...)` — compiler resolves from pipe context and argument types.

If inference is ambiguous (multiple candidates), compilation fails with a diagnostic requesting explicit type arguments.

### 2. C++ polymorphic actor definition

Polymorphic actors are defined by placing `template <typename T>` before the `ACTOR` macro:

```cpp
template <typename T>
ACTOR(scale, IN(T, N), OUT(T, N), PARAM(T, gain) PARAM(int, N)) {
    for (int i = 0; i < N; ++i) out[i] = in[i] * gain;
    return ACTOR_OK;
}
```

`pcc` scans this pattern via regex, records that `scale` has type parameter `T`, and performs textual type substitution (`T` → `float`, etc.) at monomorphization time. The C++ compiler handles template instantiation when the generated code references `actor_scale<float>`.

No additional registration macro (e.g., `ACTOR_TYPES`) is required — the `template <typename T> ACTOR(...)` declaration is sufficient for both `pcc` discovery and C++ compilation.

### 3. Monomorphization strategy

`pcc` resolves all polymorphic calls during the **Type Inference & Monomorphization** phase (pcc-spec phase 4). Each `actor<T>` call is rewritten to exactly one concrete actor instance.

All downstream phases (graph, analyze, schedule, codegen) operate on a fully monomorphized, typed IR. The lowering produces a certificate (`Cert`) that is verified against five obligations (L1–L5, pcc-spec §9.2.2) before proceeding.

### 3. Safe numeric widening

The design principle is updated from "no implicit conversions" to "safe-first type conversions": only meaning-preserving numeric widening is implicit; semantic changes require explicit actors.

Two independent widening chains are defined:

```
Real:    int8 -> int16 -> int32 -> float -> double
Complex: cfloat -> cdouble
```

Cross-family conversions (real ↔ complex) are never implicit. Narrowing conversions are never implicit.

### 4. Codegen contract

`codegen` consumes `TypedScheduledIR` and performs syntax-directed serialization to C++. It MUST NOT re-run type inference, infer fallback wire types, or reinterpret unresolved parameter types. This prevents semantic drift between the analysis and code generation phases.

## Consequences

- **Reduced actor duplication**: A single `template <typename T> ACTOR(...)` replaces N independent concrete definitions.
- **Simpler PDL**: Users write `fir(coeff)` instead of `fir_f32(coeff)` in most cases.
- **Safe implicit widening**: `int16` ADC output connects directly to `float` processing without explicit cast actors, while `cfloat -> float` (semantic change) still requires `c2r()`.
- **Verified lowering**: L1–L5 obligations provide a machine-checkable guarantee that monomorphization and widening insertion are correct.
- **Design principle shift**: The old "no implicit conversions" guarantee is relaxed. Users who relied on the compiler catching all type differences at pipe boundaries may now miss unintended widening. Mitigated by the restricted widening chain (only within the same numeric family).
- **BNF extension**: `actor_call` grammar gains `type_args?` production (lang-spec §10).

## Alternatives

- **Keep no-implicit-conversion policy**: Rejected. The friction in common patterns (int→float chains) outweighs the safety benefit, especially since the allowed widening is mathematically lossless (except `int32→float` for |val| > 2^24, which is rare in signal processing).
- **Allow all C-style implicit conversions**: Rejected. Would permit silent `cfloat→float` (lossy) and `double→float` (narrowing), violating the safety-first principle.
- **Separate template kernel + N concrete ACTOR wrappers**: Rejected. Requires the user to manually write thin wrappers for each type. `template <typename T> ACTOR(...)` achieves the same with zero boilerplate.
- **`ACTOR_POLY` + `ACTOR_TYPES` (two-macro pattern)**: Rejected. Adds an unnecessary registration macro. Since `pcc` scans header text (not constexpr evaluation), it can directly recognize the `template` + `ACTOR` pattern without a separate instantiation list.
- **Whole-program type inference without monomorphization**: Rejected. Would require codegen to handle unresolved types, complicating the backend and losing the L1–L5 verification guarantee.

## Exit criteria

- [ ] v0.3.0 compiler implementation passes all Phase 2 tests (type inference, ambiguity diagnostics, codegen compile tests)
- [ ] No regression in existing v0.1/v0.2 programs (backward compatibility)
- [ ] L1–L5 lowering verifier integrated and exercised by negative tests
