// program_query.rs — Shared helpers for querying program-level AST data
//
// Eliminates duplicated `set`/task iteration patterns across phases.

use crate::ast::*;

/// Look up a `set` directive by name, returning the SetValue if found.
pub fn get_set_value<'a>(program: &'a Program, name: &str) -> Option<&'a SetValue> {
    program.statements.iter().find_map(|stmt| {
        if let StatementKind::Set(set) = &stmt.kind {
            if set.name.name == name {
                return Some(&set.value);
            }
        }
        None
    })
}

/// Get a `set` directive's Size value (e.g., `set mem = 64MB`).
pub fn get_set_size(program: &Program, name: &str) -> Option<u64> {
    match get_set_value(program, name)? {
        SetValue::Size(v, _) => Some(*v),
        _ => None,
    }
}

/// Get a `set` directive's Number value (e.g., `set ratio = 2.0`).
pub fn get_set_number(program: &Program, name: &str) -> Option<f64> {
    match get_set_value(program, name)? {
        SetValue::Number(n, _) => Some(*n),
        _ => None,
    }
}

/// Get a `set` directive's Freq value (e.g., `set tick_rate = 1MHz`).
pub fn get_set_freq(program: &Program, name: &str) -> Option<f64> {
    match get_set_value(program, name)? {
        SetValue::Freq(f, _) => Some(*f),
        _ => None,
    }
}

/// Get a `set` directive's Ident value (e.g., `set overrun = drop`).
pub fn get_set_ident<'a>(program: &'a Program, name: &str) -> Option<&'a str> {
    match get_set_value(program, name)? {
        SetValue::Ident(ident) => Some(&ident.name),
        _ => None,
    }
}

/// Get a `set` directive's SetValue along with its Span (for diagnostics).
pub fn get_set_value_with_span<'a>(
    program: &'a Program,
    name: &str,
) -> Option<(&'a SetValue, Span)> {
    program.statements.iter().find_map(|stmt| {
        if let StatementKind::Set(set) = &stmt.kind {
            if set.name.name == name {
                return Some((&set.value, stmt.span));
            }
        }
        None
    })
}
