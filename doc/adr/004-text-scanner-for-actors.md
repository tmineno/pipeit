# ADR-004: Text-level scanner for ACTOR() macro extraction

## Context

The compiler needs actor metadata (name, input/output types and token counts, parameters) to validate actor calls during name resolution and static analysis. Actors are defined via `ACTOR()` C++ macros in header files. The compiler must extract this metadata from those headers.

## Decision

Use a text-level scanner that finds `ACTOR(` invocations, extracts balanced parentheses content, and parses the fixed positional fields via string operations (comma splitting, pattern matching).

No parser combinator library (chumsky) or C++ parser (libclang, tree-sitter) is used for this module.

## Consequences

- Simple implementation (~300 lines) with no additional dependencies
- Comment stripping (`//` and `/* */`) prevents false matches
- Balanced-paren extraction handles nested types like `std::span<const float>`
- Type string → enum mapping catches unknown types at load time
- Type *compatibility* validation is deferred to Static Analysis (§8 step 5), keeping the registry focused on extraction
- Error messages are file:line level — sufficient for developer-authored C++ macro headers

## Alternatives

- **chumsky parser**: Considered for consistency with the `.pdl` parser. Rejected — the ACTOR() macro has a fixed positional format (~5 fields) that doesn't benefit from combinator-based parsing. The grammar is closer to CSV than a programming language.
- **libclang / tree-sitter**: C++ parsers treat macro invocations as opaque — `ACTOR(...)` is seen as a function call, not structured data. Would still require custom extraction logic on top.
- **Separate manifest files (JSON/TOML)**: Would avoid C++ parsing entirely, but forces actor authors to maintain metadata in two places (macro + manifest), violating DRY.

## Exit criteria

Revisit if:

- Actor definitions require C++ expressions in macro arguments (e.g., `constexpr` computed token counts)
- A new macro variant is added that doesn't follow the fixed positional format
- The number of supported type patterns grows beyond simple pattern matching
