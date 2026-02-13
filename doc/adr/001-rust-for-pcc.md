# ADR-001: Use Rust for pcc compiler

## Context

The Pipit compiler `pcc` needs an implementation language. It reads `.pdl` files, performs static analysis (SDF balance equations, type checking), and generates C++ code. The runtime library `libpipit` is C++ since generated code links against it.

## Decision

Implement `pcc` in Rust.

## Consequences

- Strong type system (enums, pattern matching) suits compiler IR and AST design
- `Result`/`Option` provide explicit error handling, important for a diagnostic-rich compiler
- Cargo ecosystem offers proven libraries for compiler frontends (e.g., `logos` for lexing, `chumsky`/`lalrpop` for parsing, `clap` for CLI)
- Clear boundary: Rust compiler outputs C++ source, which is compiled by the system C++ compiler and linked against `libpipit`
- No need for Rust-C++ FFI — `pcc` reads actor metadata from header files, not by linking C++ code
- Team members need Rust proficiency to contribute to the compiler

## Alternatives

- **C++**: Same language as runtime, but lacks Rust's safety and ergonomics for compiler internals
- **Python**: Fastest prototyping, but performance concerns for large graphs and harder to distribute
- **TypeScript**: Good tooling, but unusual for compiler projects and adds Node.js dependency

## Exit criteria

Revisit if:

- Rust build times become a bottleneck for development iteration
- Actor registry reading requires deep C++ parsing (currently only `constexpr` metadata)
