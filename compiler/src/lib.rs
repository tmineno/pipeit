// pcc — Pipit Compiler Collection
//
// Library root. Compiler phases will be added as modules here.

pub mod analyze;
pub mod ast;
pub mod codegen;
pub mod dim_resolve;
pub mod dot;
pub mod graph;
pub mod hir;
pub mod id;
pub mod lexer;
pub mod lower;
pub mod parser;
pub mod program_query;
pub mod registry;
pub mod resolve;
pub mod schedule;
pub mod subgraph_index;
pub mod thir;
pub mod timing;
pub mod type_infer;
