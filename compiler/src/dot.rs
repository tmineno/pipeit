// dot.rs — Graphviz DOT output for Pipit SDF graphs
//
// Transforms a ProgramGraph into DOT format suitable for rendering
// with `dot`, `neato`, or other Graphviz layout engines.
//
// Preconditions: `graph` is a fully constructed ProgramGraph.
// Postconditions: returns a valid DOT string representing the graph.
// Failure modes: none (pure string formatting).
// Side effects: none.

use std::collections::{HashMap, HashSet};
use std::fmt::Write;

use crate::graph::*;

/// Emit the program graph as a Graphviz DOT string.
pub fn emit_dot(graph: &ProgramGraph) -> String {
    let mut buf = String::new();
    writeln!(buf, "digraph pipit {{").unwrap();
    writeln!(buf, "    rankdir=LR;").unwrap();
    writeln!(buf, "    node [fontname=\"Helvetica\", fontsize=10];").unwrap();
    writeln!(buf, "    edge [fontname=\"Helvetica\", fontsize=9];").unwrap();

    // Sort task names for deterministic output
    let mut task_names: Vec<&String> = graph.tasks.keys().collect();
    task_names.sort();

    for task_name in &task_names {
        let task_graph = &graph.tasks[*task_name];
        let sanitized = sanitize(task_name);
        writeln!(buf).unwrap();
        match task_graph {
            TaskGraph::Pipeline(sub) => {
                let cycle_edges = cycle_edges_for_subgraph(sub, &graph.cycles);
                writeln!(buf, "    subgraph cluster_{sanitized} {{").unwrap();
                writeln!(buf, "        label=\"task: {task_name}\";").unwrap();
                writeln!(buf, "        style=rounded;").unwrap();
                writeln!(buf, "        color=gray50;").unwrap();
                write_subgraph_contents(&mut buf, &sanitized, "", sub, &cycle_edges, "        ");
                writeln!(buf, "    }}").unwrap();
            }
            TaskGraph::Modal { control, modes } => {
                writeln!(buf, "    subgraph cluster_{sanitized} {{").unwrap();
                writeln!(buf, "        label=\"task: {task_name}\";").unwrap();
                writeln!(buf, "        style=rounded;").unwrap();
                writeln!(buf, "        color=gray50;").unwrap();

                // Control subgraph
                let cycle_edges = cycle_edges_for_subgraph(control, &graph.cycles);
                writeln!(buf).unwrap();
                writeln!(buf, "        subgraph cluster_{sanitized}_control {{").unwrap();
                writeln!(buf, "            label=\"control\";").unwrap();
                writeln!(buf, "            style=dashed;").unwrap();
                writeln!(buf, "            color=gray70;").unwrap();
                write_subgraph_contents(
                    &mut buf,
                    &sanitized,
                    "control",
                    control,
                    &cycle_edges,
                    "            ",
                );
                writeln!(buf, "        }}").unwrap();

                // Mode subgraphs
                for (mode_name, sub) in modes {
                    let mode_san = sanitize(mode_name);
                    let cycle_edges = cycle_edges_for_subgraph(sub, &graph.cycles);
                    writeln!(buf).unwrap();
                    writeln!(buf, "        subgraph cluster_{sanitized}_{mode_san} {{").unwrap();
                    writeln!(buf, "            label=\"mode: {mode_name}\";").unwrap();
                    writeln!(buf, "            style=dashed;").unwrap();
                    writeln!(buf, "            color=gray70;").unwrap();
                    write_subgraph_contents(
                        &mut buf,
                        &sanitized,
                        &mode_san,
                        sub,
                        &cycle_edges,
                        "            ",
                    );
                    writeln!(buf, "        }}").unwrap();
                }

                writeln!(buf, "    }}").unwrap();
            }
        }
    }

    // Inter-task edges (outside any cluster)
    if !graph.inter_task_edges.is_empty() {
        writeln!(buf).unwrap();
        writeln!(buf, "    // Inter-task edges").unwrap();
        for ite in &graph.inter_task_edges {
            let writer_prefix = find_node_prefix(
                &graph.tasks[&ite.writer_task],
                &sanitize(&ite.writer_task),
                ite.writer_node,
            );
            let reader_prefix = find_node_prefix(
                &graph.tasks[&ite.reader_task],
                &sanitize(&ite.reader_task),
                ite.reader_node,
            );
            writeln!(
                buf,
                "    {}_n{} -> {}_n{} [label=\"{}\", style=dashed, color=red, penwidth=2];",
                writer_prefix, ite.writer_node.0, reader_prefix, ite.reader_node.0, ite.buffer_name,
            )
            .unwrap();
        }
    }

    writeln!(buf, "}}").unwrap();
    buf
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Sanitize a name to valid DOT identifier characters.
fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Build the DOT node ID: `<task>_<prefix>_n<id>` or `<task>_n<id>` when prefix is empty.
fn dot_node_id(task: &str, prefix: &str, node: NodeId) -> String {
    if prefix.is_empty() {
        format!("{task}_n{}", node.0)
    } else {
        format!("{task}_{prefix}_n{}", node.0)
    }
}

/// Return the node label for a given NodeKind.
fn node_label(kind: &NodeKind) -> String {
    match kind {
        NodeKind::Actor { name, .. } => name.clone(),
        NodeKind::Fork { tap_name } => format!(":{tap_name}"),
        NodeKind::Probe { probe_name } => format!("?{probe_name}"),
        NodeKind::BufferRead { buffer_name } => format!("@{buffer_name}"),
        NodeKind::BufferWrite { buffer_name } => format!("->{buffer_name}"),
    }
}

/// Return DOT attributes string for a node kind.
fn node_attrs(kind: &NodeKind) -> String {
    let (shape, color) = match kind {
        NodeKind::Actor { .. } => ("box", "lightblue"),
        NodeKind::Fork { .. } => ("diamond", "lightyellow"),
        NodeKind::Probe { .. } => ("circle", "lightgreen"),
        NodeKind::BufferRead { .. } => ("cylinder", "lightsalmon"),
        NodeKind::BufferWrite { .. } => ("cylinder", "lightsalmon"),
    };
    let label = node_label(kind);
    format!("shape={shape}, style=filled, fillcolor={color}, label=\"{label}\"")
}

/// Write all nodes and edges for a subgraph.
///
/// Probes are rendered as side-branches off the main dataflow rather than
/// inline passthrough nodes.  For a probe with edges `A → probe → B`,
/// the DOT output draws a bypass edge `A → B` (main flow) and a tap
/// edge `A → probe` (observation point).
fn write_subgraph_contents(
    buf: &mut String,
    task: &str,
    prefix: &str,
    sub: &Subgraph,
    cycle_edges: &HashSet<(u32, u32)>,
    indent: &str,
) {
    // Identify probe nodes and build their bypass mapping.
    let probe_ids: HashSet<u32> = sub
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Probe { .. }))
        .map(|n| n.id.0)
        .collect();

    // For each probe, find predecessor → probe and probe → successor.
    // predecessor: source of the edge whose target is the probe.
    // successor:   target of the edge whose source is the probe.
    let mut probe_pred: HashMap<u32, NodeId> = HashMap::new();
    let mut probe_succ: HashMap<u32, NodeId> = HashMap::new();
    for edge in &sub.edges {
        if probe_ids.contains(&edge.target.0) {
            probe_pred.insert(edge.target.0, edge.source);
        }
        if probe_ids.contains(&edge.source.0) {
            probe_succ.insert(edge.source.0, edge.target);
        }
    }

    // Nodes
    for node in &sub.nodes {
        let id = dot_node_id(task, prefix, node.id);
        let attrs = node_attrs(&node.kind);
        writeln!(buf, "{indent}{id} [{attrs}];").unwrap();
    }

    // Edges — skip original probe-through edges and emit bypass + tap instead.
    writeln!(buf).unwrap();

    // Collect edges entering or leaving a probe so we can skip them.
    let probe_edge: HashSet<(u32, u32)> = sub
        .edges
        .iter()
        .filter(|e| probe_ids.contains(&e.source.0) || probe_ids.contains(&e.target.0))
        .map(|e| (e.source.0, e.target.0))
        .collect();

    // Normal edges (not touching probes)
    for edge in &sub.edges {
        if probe_edge.contains(&(edge.source.0, edge.target.0)) {
            continue;
        }
        let src = dot_node_id(task, prefix, edge.source);
        let tgt = dot_node_id(task, prefix, edge.target);
        if cycle_edges.contains(&(edge.source.0, edge.target.0)) {
            writeln!(buf, "{indent}{src} -> {tgt} [style=bold, color=blue];").unwrap();
        } else {
            writeln!(buf, "{indent}{src} -> {tgt};").unwrap();
        }
    }

    // Probe bypass + tap edges
    for &pid in &probe_ids {
        let pred = probe_pred.get(&pid);
        let succ = probe_succ.get(&pid);

        // Bypass: predecessor → successor (main flow skips probe)
        if let (Some(&pred_id), Some(&succ_id)) = (pred, succ) {
            let src = dot_node_id(task, prefix, pred_id);
            let tgt = dot_node_id(task, prefix, succ_id);
            writeln!(buf, "{indent}{src} -> {tgt};").unwrap();
        }

        // Tap: predecessor → probe (side observation)
        if let Some(&pred_id) = pred {
            let src = dot_node_id(task, prefix, pred_id);
            let probe = dot_node_id(task, prefix, NodeId(pid));
            writeln!(
                buf,
                "{indent}{src} -> {probe} [style=dashed, constraint=false];"
            )
            .unwrap();
        }
    }
}

/// Collect cycle edges that belong to a specific subgraph.
fn cycle_edges_for_subgraph(sub: &Subgraph, all_cycles: &[Vec<NodeId>]) -> HashSet<(u32, u32)> {
    let node_ids: HashSet<u32> = sub.nodes.iter().map(|n| n.id.0).collect();
    let mut edges = HashSet::new();
    for cycle in all_cycles {
        if cycle.iter().all(|id| node_ids.contains(&id.0)) {
            for window in cycle.windows(2) {
                edges.insert((window[0].0, window[1].0));
            }
            // Close the cycle: last -> first
            if let (Some(last), Some(first)) = (cycle.last(), cycle.first()) {
                edges.insert((last.0, first.0));
            }
        }
    }
    edges
}

/// Find the DOT node ID prefix for a node within a task graph.
/// For pipeline tasks, prefix is just the task name.
/// For modal tasks, we need to find which subgraph contains the node.
fn find_node_prefix(task_graph: &TaskGraph, task_sanitized: &str, node_id: NodeId) -> String {
    match task_graph {
        TaskGraph::Pipeline(_) => task_sanitized.to_string(),
        TaskGraph::Modal { control, modes } => {
            if control.nodes.iter().any(|n| n.id == node_id) {
                return format!("{task_sanitized}_control");
            }
            for (mode_name, sub) in modes {
                if sub.nodes.iter().any(|n| n.id == node_id) {
                    return format!("{}_{}", task_sanitized, sanitize(mode_name));
                }
            }
            // Fallback — should not happen with a well-formed graph
            task_sanitized.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::Registry;
    use crate::resolve;
    use std::path::PathBuf;

    fn test_registry() -> Registry {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_path_buf();
        let std_actors = root.join("runtime/libpipit/include/std_actors.h");
        let std_math = root.join("runtime/libpipit/include/std_math.h");
        let example_actors = root.join("examples/example_actors.h");
        let std_sink = root.join("runtime/libpipit/include/std_sink.h");
        let std_source = root.join("runtime/libpipit/include/std_source.h");
        let mut reg = Registry::new();
        reg.load_header(&std_actors)
            .expect("failed to load std_actors.h");
        reg.load_header(&std_math)
            .expect("failed to load std_math.h");
        reg.load_header(&example_actors)
            .expect("failed to load example_actors.h");
        reg.load_header(&std_sink)
            .expect("failed to load std_sink.h");
        reg.load_header(&std_source)
            .expect("failed to load std_source.h");
        reg
    }

    fn build_and_emit(source: &str, registry: &Registry) -> String {
        let parse_result = crate::parser::parse(source);
        assert!(
            parse_result.errors.is_empty(),
            "parse errors: {:?}",
            parse_result.errors
        );
        let program = parse_result.program.expect("parse failed");
        let mut resolve_result = resolve::resolve(&program, registry);
        assert!(
            resolve_result
                .diagnostics
                .iter()
                .all(|d| d.level != resolve::DiagLevel::Error),
            "resolve errors: {:?}",
            resolve_result.diagnostics
        );
        let hir_program = crate::hir::build_hir(
            &program,
            &resolve_result.resolved,
            &mut resolve_result.id_alloc,
        );
        let graph_result =
            crate::graph::build_graph(&hir_program, &resolve_result.resolved, registry);
        assert!(
            graph_result
                .diagnostics
                .iter()
                .all(|d| d.level != resolve::DiagLevel::Error),
            "graph errors: {:?}",
            graph_result.diagnostics
        );
        emit_dot(&graph_result.graph)
    }

    #[test]
    fn valid_dot_structure() {
        let reg = test_registry();
        let dot = build_and_emit(
            "clock 1kHz t {\n    constant(0.0) | fft(256) | stdout()\n}",
            &reg,
        );
        assert!(dot.starts_with("digraph pipit {"));
        assert!(dot.trim_end().ends_with('}'));
        assert!(dot.contains("subgraph cluster_t {"));
        assert!(dot.contains("label=\"task: t\""));
    }

    #[test]
    fn node_shapes_present() {
        let reg = test_registry();
        // Pipeline with tap, probe, and buffer write
        let dot = build_and_emit(
            "clock 1kHz t {\n    constant(0.0) | :tap1 | fir(coeff) | ?p -> buf\n    :tap1 | stdout()\n}\nconst coeff = [1.0]",
            &reg,
        );
        assert!(dot.contains("shape=box"), "missing actor box shape");
        assert!(dot.contains("shape=diamond"), "missing fork diamond shape");
        assert!(dot.contains("shape=circle"), "missing probe circle shape");
        assert!(
            dot.contains("shape=cylinder"),
            "missing buffer cylinder shape"
        );
    }

    #[test]
    fn probe_as_side_branch() {
        let reg = test_registry();
        // fir -> ?p -> ->buf  should become fir->->buf (bypass) + fir->?p (tap)
        let dot = build_and_emit(
            "clock 1kHz t {\n    constant(0.0) | fir(coeff) | ?p -> buf\n}\nconst coeff = [1.0]",
            &reg,
        );
        // Probe node is still declared
        assert!(dot.contains("label=\"?p\""), "probe node missing");

        // Find the probe node ID (invtriangle line)
        let probe_line = dot.lines().find(|l| l.contains("label=\"?p\"")).unwrap();
        let probe_id = probe_line.split_whitespace().next().unwrap();

        // There should be a dashed tap edge TO the probe (side branch)
        let tap_edge = format!("-> {probe_id} [style=dashed, constraint=false]");
        assert!(
            dot.contains(&tap_edge),
            "missing dashed tap edge to probe, dot:\n{dot}"
        );

        // There should NOT be a solid edge FROM the probe (it's not inline)
        let from_probe = format!("{probe_id} ->");
        let has_outgoing = dot.lines().any(|l| {
            let trimmed = l.trim();
            trimmed.starts_with(&from_probe) && !trimmed.contains("style=dashed")
        });
        assert!(
            !has_outgoing,
            "probe should not have outgoing solid edge, dot:\n{dot}"
        );
    }

    #[test]
    fn modal_nested_clusters() {
        let reg = test_registry();
        let dot = build_and_emit(
            concat!(
                "clock 1kHz recv {\n",
                "    control {\n",
                "        constant(0.0) | detect() -> ctrl\n",
                "    }\n",
                "    mode sync {\n",
                "        constant(0.0) | fir(sync_coeff) -> out\n",
                "    }\n",
                "    mode data {\n",
                "        constant(0.0) | fft(256) -> out2\n",
                "    }\n",
                "    switch(ctrl, sync, data) default sync\n",
                "}\n",
                "const sync_coeff = [1.0]\n",
            ),
            &reg,
        );
        assert!(
            dot.contains("subgraph cluster_recv {"),
            "missing outer task cluster"
        );
        assert!(
            dot.contains("subgraph cluster_recv_control {"),
            "missing control cluster"
        );
        assert!(
            dot.contains("subgraph cluster_recv_sync {"),
            "missing sync cluster"
        );
        assert!(
            dot.contains("subgraph cluster_recv_data {"),
            "missing data cluster"
        );
        assert!(dot.contains("label=\"control\""), "missing control label");
        assert!(dot.contains("label=\"mode: sync\""), "missing sync label");
        assert!(dot.contains("label=\"mode: data\""), "missing data label");
    }

    #[test]
    fn inter_task_edges() {
        let reg = test_registry();
        let dot = build_and_emit(
            concat!(
                "clock 1kHz writer {\n",
                "    constant(0.0) | fft(256) -> sig\n",
                "}\n",
                "clock 1kHz reader {\n",
                "    @sig | stdout()\n",
                "}\n",
            ),
            &reg,
        );
        assert!(
            dot.contains("style=dashed"),
            "missing dashed inter-task edge"
        );
        assert!(dot.contains("color=red"), "missing red inter-task edge");
        assert!(dot.contains("label=\"sig\""), "missing buffer name label");
    }

    #[test]
    fn unique_node_ids() {
        let reg = test_registry();
        let dot = build_and_emit(
            concat!(
                "clock 1kHz a {\n    constant(0.0) | fft(256) | stdout()\n}\n",
                "clock 1kHz b {\n    constant(0.0) | fft(256) | stdout()\n}\n",
            ),
            &reg,
        );
        // Extract all node declarations (lines with [...])
        let node_ids: Vec<&str> = dot
            .lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                if trimmed.contains('[') && trimmed.contains("shape=") {
                    Some(trimmed.split_whitespace().next().unwrap())
                } else {
                    None
                }
            })
            .collect();
        let unique: HashSet<&&str> = node_ids.iter().collect();
        assert_eq!(
            node_ids.len(),
            unique.len(),
            "duplicate node IDs found: {:?}",
            node_ids
        );
    }

    #[test]
    fn deterministic_output() {
        let reg = test_registry();
        let source = concat!(
            "clock 1kHz a {\n    constant(0.0) | fft(256) | stdout()\n}\n",
            "clock 1kHz b {\n    constant(0.0) | stdout()\n}\n",
        );
        let dot1 = build_and_emit(source, &reg);
        let dot2 = build_and_emit(source, &reg);
        assert_eq!(dot1, dot2, "DOT output is not deterministic");
    }
}
