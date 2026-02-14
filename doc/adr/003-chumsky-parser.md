# ADR-003: Use chumsky for parser implementation

## Context

The pcc compiler needs a parser to transform the token stream from the lexer (ADR-002) into an AST per the BNF grammar in spec §10. The grammar has ~25 productions and is LL(1). Options evaluated:

| Criterion | chumsky | lalrpop | Hand-written RD |
|-----------|---------|---------|-----------------|
| Approach | combinator library | grammar DSL → table-driven | manual functions |
| Error recovery | built-in (`recover_with`) | limited | manual |
| Span tracking | automatic (`map_with`) | via `@L`/`@R` | manual |
| Diagnostics | `Rich` error type with context | basic | manual |
| Boilerplate | moderate (combinators) | low (grammar file) | high |
| Type safety | full (Rust generics) | full | full |
| Learning curve | moderate (generic lifetimes) | low-moderate | low |

## Decision

Use the `chumsky` crate (v0.10) for parser implementation.

## Consequences

- Grammar rules map directly to combinator expressions — easy to audit against spec §10
- Automatic span tracking via `map_with` provides accurate source locations on every AST node
- `Rich` error type produces detailed diagnostics (expected tokens, found token, context)
- Built-in error recovery allows parsing to continue after syntax errors
- Complex generic lifetime annotations required — mitigated by building all combinators inside a single function scope
- Token-based parsing via `Stream::from_iter` integrates cleanly with the logos lexer
- `select!` macro handles pattern matching on token variants with data (Number, Freq, etc.)

## Alternatives

- **lalrpop**: Grammar DSL generates table-driven parser. Less boilerplate for grammar definition, but weaker error recovery and diagnostics. Separate `.lalrpop` file breaks locality with Rust code.
- **Hand-written recursive descent**: Maximum control, no dependencies, simpler types. But requires manual span tracking, manual error recovery, and significantly more code (~3-5x). Pipit's grammar is simple enough that RD would work, but chumsky provides better diagnostics for free.
- **pest / tree-sitter**: PEG-based. Good for syntax highlighting but produce concrete syntax trees rather than typed ASTs, requiring a separate AST conversion pass.

## Exit criteria

Revisit if:

- chumsky's generic lifetime complexity becomes unmanageable as the parser grows
- A future grammar extension requires backtracking or context-sensitivity beyond chumsky's capabilities
- Compile times for the parser module exceed 30 seconds (chumsky's monomorphization can be heavy)
