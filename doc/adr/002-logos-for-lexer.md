# ADR-002: Use logos for lexer generation

## Context

The pcc compiler needs a lexer to tokenize `.pdl` source files per the Pipit Language Specification §2. The lexer must handle 10 keywords, 14 symbols, 4 literal types (number, frequency, size, string), identifiers, significant newlines, and line comments.

Options evaluated:

| Criterion | logos | lexgen | Hand-written |
|-----------|-------|--------|--------------|
| Approach | derive macro on enum | proc macro DSL | manual match/if |
| Performance | DFA with jump tables | DFA-based | varies |
| Ecosystem | most popular Rust lexer crate | niche | used by rustc |
| Boilerplate | minimal (attribute-driven) | moderate (DSL syntax) | high |
| Lookahead | no | yes (peek) | full control |
| Maintenance | low | low-medium | high |

## Decision

Use the `logos` crate (v0.15) for lexer generation.

## Consequences

- Token definitions are declarative `#[token]`/`#[regex]` attributes — easy to audit against spec §2
- Compile-time DFA generation provides high performance with no runtime cost
- Callbacks (`parse_freq`, `parse_size`, `parse_string`) handle value extraction during lexing
- No lookahead — acceptable because Pipit's lexical grammar requires none
- Adding new tokens is a one-line attribute addition
- Team members need basic familiarity with logos attributes

## Alternatives

- **lexgen**: More powerful (lookahead, user state), but these features are unnecessary for Pipit's simple lexical grammar. Smaller community.
- **Hand-written lexer**: Maximum flexibility and potentially ~20% faster with CPU-tuned branching, but significantly more code to write and maintain. Lexing is not a bottleneck for pcc's target workloads.
- **lrlex**: Traditional lex/flex approach. Less idiomatic Rust, smaller ecosystem.

## Exit criteria

Revisit if:

- logos cannot express a future lexical pattern (e.g. context-dependent tokens)
- Compile-time overhead of the derive macro becomes problematic
- A logos bug blocks development (switch to hand-written as fallback)
