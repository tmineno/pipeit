# ADR-031: Default C++ Backend Compiler to Clang

## Context

`pcc` generates C++ and invokes a system C++ compiler for `--emit exe`.

The previous default was `--cc c++`, which resolves differently by environment (for example, GCC on one machine and Clang on another). That alias-level ambiguity weakens reproducibility and makes diagnostics/profile behavior less consistent across CI and developer workstations.

The project is also moving actor metadata generation to a Clang-based path, so keeping the generated-C++ backend on a toolchain-agnostic alias introduces avoidable drift between extraction and final compilation environments.

## Decision

- Change the `pcc` CLI default from `--cc c++` to `--cc clang++`.
- Keep `--cc` override support; users can still select `g++` (or other compilers) explicitly.
- Prefer `clang++` first in integration test compiler discovery order.
- Default example CMake/scripted builds to `clang++` unless the caller explicitly sets a compiler.
- Update user-facing docs/spec entries to reflect `clang++` as the default backend compiler.

## Consequences

- Backend toolchain behavior is more predictable across environments.
- Compiler diagnostics and generated-code compile behavior are aligned with the Clang-first roadmap.
- Existing users without `clang++` in `PATH` must pass `--cc <compiler>` explicitly.
- This is a CLI default change (behavioral), but not a surface-area change: the flag and override mechanism remain unchanged.

## Alternatives

- Keep `--cc c++` as default. Rejected: alias resolution is environment-dependent and non-hermetic.
- Force Clang without `--cc` override. Rejected: unnecessarily restrictive for environments that require GCC or custom wrappers.
- Auto-probe compilers at runtime (`clang++` then fallback). Rejected: implicit fallback hides configuration drift and can reduce build reproducibility.

## Exit criteria

- `pcc --help` reports `--cc` default as `clang++`.
- Examples/CMake helper flows use `clang++` by default.
- Docs/spec default compiler references are updated to `clang++`.
- Integration tests remain green with the updated default behavior.
