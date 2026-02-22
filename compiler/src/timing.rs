// timing.rs — Mermaid Gantt timing chart output for Pipit PASS schedules
//
// Transforms a ScheduledProgram + ProgramGraph into a Mermaid Gantt chart
// showing actor firing order and durations per task.
//
// Preconditions: `schedule` is a computed ScheduledProgram;
//                `graph` is the corresponding ProgramGraph.
// Postconditions: returns a valid Mermaid Gantt chart string.
// Failure modes: none (pure string formatting).
// Side effects: none.

use std::collections::HashMap;
use std::fmt::Write;

use crate::graph::*;
use crate::schedule::*;

/// Emit the PASS schedule as a Mermaid Gantt chart string.
///
/// Preconditions: `schedule` and `graph` correspond to the same program.
/// Postconditions: returns a complete, valid Mermaid Gantt chart.
/// Failure modes: none (pure string formatting; unknown nodes get fallback labels).
/// Side effects: none.
pub fn emit_timing_chart(schedule: &ScheduledProgram, graph: &ProgramGraph) -> String {
    let mut buf = String::new();
    writeln!(buf, "gantt").unwrap();
    writeln!(buf, "    title PASS Schedule Timing").unwrap();
    writeln!(buf, "    dateFormat x").unwrap();
    writeln!(buf, "    axisFormat %Q").unwrap();

    // Sort task names for deterministic output
    let mut task_names: Vec<&String> = schedule.tasks.keys().collect();
    task_names.sort();

    for task_name in &task_names {
        let meta = &schedule.tasks[*task_name];
        let task_graph = match graph.tasks.get(*task_name) {
            Some(g) => g,
            None => continue,
        };
        emit_task_section(&mut buf, task_name, meta, task_graph);
    }

    buf
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn emit_task_section(buf: &mut String, task_name: &str, meta: &TaskMeta, task_graph: &TaskGraph) {
    let freq = format_freq(meta.freq_hz);
    let prefix = sanitize(task_name);
    match (&meta.schedule, task_graph) {
        (TaskSchedule::Pipeline(sched), TaskGraph::Pipeline(sub)) => {
            writeln!(buf).unwrap();
            writeln!(
                buf,
                "    section {} [pipeline] (K={}, {})",
                task_name, meta.k_factor, freq
            )
            .unwrap();
            emit_subgraph_firings(buf, sched, sub, &prefix);
        }
        (
            TaskSchedule::Modal { control, modes },
            TaskGraph::Modal {
                control: ctrl_sub,
                modes: mode_subs,
            },
        ) => {
            writeln!(buf).unwrap();
            writeln!(
                buf,
                "    section {} [control] (K={}, {})",
                task_name, meta.k_factor, freq
            )
            .unwrap();
            emit_subgraph_firings(buf, control, ctrl_sub, &format!("{prefix}_ctrl"));

            for (mode_name, mode_sched) in modes {
                let mode_sub = mode_subs
                    .iter()
                    .find(|(n, _)| n == mode_name)
                    .map(|(_, s)| s);
                writeln!(buf).unwrap();
                writeln!(buf, "    section {} [mode: {}]", task_name, mode_name).unwrap();
                if let Some(sub) = mode_sub {
                    let mode_prefix = format!("{}_{}", prefix, sanitize(mode_name));
                    emit_subgraph_firings(buf, mode_sched, sub, &mode_prefix);
                }
            }
        }
        _ => {
            // Schedule/graph type mismatch — skip silently
        }
    }
}

/// Emit firing entries as Mermaid Gantt task lines using ASAP scheduling.
///
/// Uses `dateFormat x` with numeric start/end values.  Each node starts
/// at the earliest possible time: `max(end_time of predecessors)`.
/// Independent branches (e.g. after a fork) run in parallel.
/// Probes are zero-duration observation points and are omitted from output.
fn emit_subgraph_firings(
    buf: &mut String,
    sched: &SubgraphSchedule,
    sub: &Subgraph,
    id_prefix: &str,
) {
    if sched.firings.is_empty() {
        return;
    }

    // Build position map: node_id -> index in topological order
    let position: HashMap<NodeId, usize> = sched
        .firings
        .iter()
        .enumerate()
        .map(|(i, f)| (f.node_id, i))
        .collect();

    // Build forward-edge predecessor map (skip back-edges where source
    // appears after target in topological order — these are feedback edges)
    let mut predecessors: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
    for edge in &sub.edges {
        let src_pos = position.get(&edge.source);
        let tgt_pos = position.get(&edge.target);
        match (src_pos, tgt_pos) {
            (Some(&sp), Some(&tp)) if sp < tp => {
                predecessors
                    .entry(edge.target)
                    .or_default()
                    .push(edge.source);
            }
            _ => {} // back-edge or node not in schedule
        }
    }

    // Compute ASAP start/end times
    let mut end_time: HashMap<NodeId, u64> = HashMap::new();
    let mut task_index = 0usize;

    for entry in &sched.firings {
        let node = find_node(sub, entry.node_id);
        let is_probe = node.is_some_and(|n| matches!(n.kind, NodeKind::Probe { .. }));

        // start = max(end_time of forward predecessors), or 0 if none
        let start = predecessors
            .get(&entry.node_id)
            .map(|preds| {
                preds
                    .iter()
                    .filter_map(|p| end_time.get(p))
                    .max()
                    .copied()
                    .unwrap_or(0)
            })
            .unwrap_or(0);

        let duration = if is_probe {
            0
        } else {
            entry.repetition_count as u64
        };
        let end = start + duration;
        end_time.insert(entry.node_id, end);

        // Skip probes in output (zero-duration observation points)
        if is_probe {
            continue;
        }

        let label = node
            .map(|n| node_label(&n.kind))
            .unwrap_or_else(|| format!("node_{}", entry.node_id.0));

        let id = format!("{id_prefix}_{task_index}");
        writeln!(
            buf,
            "    {} x{} :{}, {}, {}",
            label, entry.repetition_count, id, start, end
        )
        .unwrap();
        task_index += 1;
    }
}

/// Return a Mermaid-safe display label for a given NodeKind.
///
/// Mermaid Gantt uses `:` as the task/metadata separator, so we replace
/// the pipit `:name` fork syntax with `fork(name)` and similar for other
/// special node types.
fn node_label(kind: &NodeKind) -> String {
    match kind {
        NodeKind::Actor { name, .. } => name.clone(),
        NodeKind::Fork { tap_name } => format!("fork({tap_name})"),
        NodeKind::Probe { probe_name } => format!("probe({probe_name})"),
        NodeKind::BufferRead { buffer_name } => format!("read({buffer_name})"),
        NodeKind::BufferWrite { buffer_name } => format!("write({buffer_name})"),
    }
}

/// Format frequency in engineering notation.
fn format_freq(freq_hz: f64) -> String {
    if freq_hz >= 1_000_000.0 {
        let mhz = freq_hz / 1_000_000.0;
        if mhz == mhz.floor() {
            format!("{}MHz", mhz as u64)
        } else {
            format!("{:.1}MHz", mhz)
        }
    } else if freq_hz >= 1_000.0 {
        let khz = freq_hz / 1_000.0;
        if khz == khz.floor() {
            format!("{}kHz", khz as u64)
        } else {
            format!("{:.1}kHz", khz)
        }
    } else {
        format!("{}Hz", freq_hz as u64)
    }
}

/// Sanitize a name to valid Mermaid identifier characters.
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

fn find_node(sub: &Subgraph, id: NodeId) -> Option<&Node> {
    sub.nodes.iter().find(|n| n.id == id)
}

// ── Tests ───────────────────────────────────────────────────────────────────

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

    /// Full pipeline: parse -> resolve -> graph -> analyze -> schedule -> timing chart
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
        let type_result =
            crate::type_infer::type_infer(&hir_program, &resolve_result.resolved, registry);
        let lower_result = crate::lower::lower_and_verify(
            &program,
            &resolve_result.resolved,
            &type_result.typed,
            registry,
        );
        let thir = crate::thir::build_thir_context(
            &hir_program,
            &resolve_result.resolved,
            &type_result.typed,
            &lower_result.lowered,
            registry,
            &graph_result.graph,
        );
        let analysis_result = crate::analyze::analyze(&thir, &graph_result.graph);
        assert!(
            analysis_result
                .diagnostics
                .iter()
                .all(|d| d.level != resolve::DiagLevel::Error),
            "analysis errors: {:?}",
            analysis_result.diagnostics
        );
        let schedule_result =
            crate::schedule::schedule(&thir, &graph_result.graph, &analysis_result.analysis);
        assert!(
            schedule_result
                .diagnostics
                .iter()
                .all(|d| d.level != resolve::DiagLevel::Error),
            "schedule errors: {:?}",
            schedule_result.diagnostics
        );
        emit_timing_chart(&schedule_result.schedule, &graph_result.graph)
    }

    /// Parse a task line like "    adc x256 :t_0, 0, 256" into (label, id, start, end).
    fn parse_task_line(line: &str) -> Option<(String, String, u64, u64)> {
        let trimmed = line.trim();
        // Skip empty, gantt, title, dateFormat, axisFormat, section lines
        if trimmed.is_empty()
            || trimmed == "gantt"
            || trimmed.starts_with("title ")
            || trimmed.starts_with("dateFormat ")
            || trimmed.starts_with("axisFormat ")
            || trimmed.starts_with("section ")
        {
            return None;
        }
        // Format: "<label> x<count> :<id>, <start>, <end>"
        let colon_pos = trimmed.find(':')?;
        let label_part = trimmed[..colon_pos].trim();
        let meta_part = trimmed[colon_pos + 1..].trim();
        let label = label_part.to_string();

        let parts: Vec<&str> = meta_part.split(',').map(|s| s.trim()).collect();
        if parts.len() != 3 {
            return None;
        }
        let id = parts[0].to_string();
        let start: u64 = parts[1].parse().ok()?;
        let end: u64 = parts[2].parse().ok()?;
        Some((label, id, start, end))
    }

    // ══════════════════════════════════════════════════════════════════════
    // Mermaid Gantt Syntax Validation (strict spec conformance)
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn spec_first_line_is_gantt() {
        let reg = test_registry();
        let chart = build_and_emit("clock 1kHz t {\n    constant(0.0) | stdout()\n}", &reg);
        let first_line = chart.lines().next().unwrap();
        assert_eq!(first_line, "gantt", "first line must be 'gantt'");
    }

    #[test]
    fn spec_dateformat_x_present() {
        let reg = test_registry();
        let chart = build_and_emit("clock 1kHz t {\n    constant(0.0) | stdout()\n}", &reg);
        assert!(
            chart.lines().any(|l| l.trim() == "dateFormat x"),
            "must contain 'dateFormat x' declaration"
        );
    }

    #[test]
    fn spec_title_present() {
        let reg = test_registry();
        let chart = build_and_emit("clock 1kHz t {\n    constant(0.0) | stdout()\n}", &reg);
        assert!(
            chart.lines().any(|l| l.trim().starts_with("title ")),
            "must contain a 'title' declaration"
        );
    }

    #[test]
    fn spec_axis_format_numeric() {
        let reg = test_registry();
        let chart = build_and_emit("clock 1kHz t {\n    constant(0.0) | stdout()\n}", &reg);
        assert!(
            chart.lines().any(|l| l.trim() == "axisFormat %Q"),
            "must contain 'axisFormat %Q' for numeric cycle axis"
        );
    }

    #[test]
    fn spec_section_header_syntax() {
        let reg = test_registry();
        let chart = build_and_emit("clock 1kHz t {\n    constant(0.0) | stdout()\n}", &reg);
        let sections: Vec<&str> = chart
            .lines()
            .filter(|l| l.trim().starts_with("section "))
            .collect();
        assert!(!sections.is_empty(), "must have at least one section");
        for section in &sections {
            let trimmed = section.trim();
            assert!(
                trimmed.starts_with("section "),
                "section line must start with 'section '"
            );
            let name = &trimmed["section ".len()..];
            assert!(
                !name.is_empty(),
                "section name must not be empty: {:?}",
                section
            );
        }
    }

    #[test]
    fn spec_task_line_format() {
        // Mermaid Gantt task: "taskName :[tags,] [taskID,] startDate, endDate"
        // Our format: "<label> x<count> :<id>, <start>, <end>"
        let reg = test_registry();
        let chart = build_and_emit(
            "clock 1kHz t {\n    constant(0.0) | fft(256) | c2r() | stdout()\n}",
            &reg,
        );
        let task_lines: Vec<&str> = chart
            .lines()
            .filter(|l| {
                let t = l.trim();
                !t.is_empty()
                    && t != "gantt"
                    && !t.starts_with("title ")
                    && !t.starts_with("dateFormat ")
                    && !t.starts_with("axisFormat ")
                    && !t.starts_with("section ")
            })
            .collect();

        assert!(!task_lines.is_empty(), "must have task lines");
        for line in &task_lines {
            let parsed = parse_task_line(line);
            assert!(
                parsed.is_some(),
                "task line must parse as '<label> :<id>, <start>, <end>': {:?}",
                line
            );
            let (label, id, start, end) = parsed.unwrap();
            // Label must not be empty
            assert!(!label.is_empty(), "label must not be empty: {:?}", line);
            // Label must not contain colon (breaks Mermaid parsing)
            assert!(
                !label.contains(':'),
                "label must not contain ':': {:?}",
                label
            );
            // ID must be alphanumeric + underscore
            assert!(
                id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
                "task ID must be alphanumeric/underscore: {:?}",
                id
            );
            // end >= start
            assert!(
                end >= start,
                "end ({}) must be >= start ({}): {:?}",
                end,
                start,
                line
            );
        }
    }

    #[test]
    fn spec_no_colon_in_labels() {
        // Colons in task labels break Mermaid parsing since ':' is the
        // task/metadata separator character.
        let reg = test_registry();
        // Test with fork (was previously `:name`), probe, buffer read/write
        let chart = build_and_emit(
            "clock 1kHz t {\n    constant(0.0) | :raw | stdout()\n    :raw | stdout()\n}",
            &reg,
        );
        let task_lines: Vec<&str> = chart
            .lines()
            .filter(|l| parse_task_line(l).is_some())
            .collect();
        for line in &task_lines {
            let (label, _, _, _) = parse_task_line(line).unwrap();
            assert!(
                !label.contains(':'),
                "label contains ':' which breaks Mermaid: {:?}",
                label
            );
        }
    }

    #[test]
    fn spec_task_ids_unique() {
        let reg = test_registry();
        let chart = build_and_emit(
            "clock 1kHz t {\n    constant(0.0) | fft(256) | c2r() | stdout()\n}",
            &reg,
        );
        let ids: Vec<String> = chart
            .lines()
            .filter_map(|l| parse_task_line(l).map(|(_, id, _, _)| id))
            .collect();
        let unique: std::collections::HashSet<&String> = ids.iter().collect();
        assert_eq!(
            ids.len(),
            unique.len(),
            "task IDs must be unique, got: {:?}",
            ids
        );
    }

    #[test]
    fn spec_start_end_non_negative_integers() {
        let reg = test_registry();
        let chart = build_and_emit(
            "clock 1kHz t {\n    constant(0.0) | fft(256) | c2r() | stdout()\n}",
            &reg,
        );
        for line in chart.lines() {
            if let Some((_, _, start, end)) = parse_task_line(line) {
                // With dateFormat x, values are non-negative integers
                assert!(
                    end >= start,
                    "end must be >= start: start={}, end={}",
                    start,
                    end
                );
            }
        }
    }

    // ══════════════════════════════════════════════════════════════════════
    // ASAP Parallel Scheduling
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn asap_first_node_starts_at_zero() {
        let reg = test_registry();
        let chart = build_and_emit("clock 1kHz t {\n    constant(0.0) | stdout()\n}", &reg);
        let first_task = chart.lines().find_map(parse_task_line).unwrap();
        assert_eq!(first_task.2, 0, "first node must start at time 0");
    }

    #[test]
    fn asap_dependent_starts_after_predecessor() {
        // In a linear chain adc | stdout, stdout starts after adc ends
        let reg = test_registry();
        let chart = build_and_emit("clock 1kHz t {\n    constant(0.0) | stdout()\n}", &reg);
        let tasks: Vec<_> = chart.lines().filter_map(parse_task_line).collect();
        assert_eq!(tasks.len(), 2);
        let adc = &tasks[0];
        let stdout = &tasks[1];
        assert_eq!(adc.2, 0, "adc starts at 0");
        assert_eq!(
            stdout.2, adc.3,
            "stdout start must equal adc end (dependency)"
        );
    }

    #[test]
    fn asap_parallel_branches_after_fork() {
        // multirate pattern: adc | :sig | fir(...) | stdout() and :sig | fft(...) | ...
        // After fork(sig), fir and fft should start at the same time
        let reg = test_registry();
        let chart = build_and_emit(
            concat!(
                "const lp_coeff = [0.25, 0.5, 0.25]\n",
                "clock 1kHz analyzer {\n",
                "    constant(0.0) | :sig | fir(lp_coeff) | stdout()\n",
                "    :sig | fft(64) | mag() | stdout()\n",
                "}",
            ),
            &reg,
        );
        let tasks: Vec<_> = chart.lines().filter_map(parse_task_line).collect();

        // Find fir and fft tasks
        let fir = tasks
            .iter()
            .find(|t| t.0.starts_with("fir "))
            .expect("fir task not found");
        let fft = tasks
            .iter()
            .find(|t| t.0.starts_with("fft "))
            .expect("fft task not found");

        assert_eq!(
            fir.2, fft.2,
            "fir and fft must start at the same time (parallel branches): fir={}, fft={}",
            fir.2, fft.2
        );
    }

    #[test]
    fn asap_linear_chain_sequential() {
        // adc | fft | c2r | stdout — strictly sequential
        let reg = test_registry();
        let chart = build_and_emit(
            "clock 1kHz t {\n    constant(0.0) | fft(256) | c2r() | stdout()\n}",
            &reg,
        );
        let tasks: Vec<_> = chart.lines().filter_map(parse_task_line).collect();
        assert_eq!(tasks.len(), 4);

        // Each task starts where the previous one ends
        for i in 1..tasks.len() {
            assert_eq!(
                tasks[i].2,
                tasks[i - 1].3,
                "task '{}' should start at end of '{}': expected {}, got {}",
                tasks[i].0,
                tasks[i - 1].0,
                tasks[i - 1].3,
                tasks[i].2
            );
        }
    }

    #[test]
    fn asap_fork_downstream_timing() {
        // After fork, both branches start at fork's end time,
        // and their downstream actors start after their respective predecessors
        let reg = test_registry();
        let chart = build_and_emit(
            "clock 1kHz t {\n    constant(0.0) | :raw | stdout()\n    :raw | stdout()\n}",
            &reg,
        );
        let tasks: Vec<_> = chart.lines().filter_map(parse_task_line).collect();

        // Find fork(raw)
        let fork = tasks
            .iter()
            .find(|t| t.0.starts_with("fork(raw)"))
            .expect("fork(raw) not found");

        // Both stdout tasks should start at fork's end time
        let stdouts: Vec<_> = tasks
            .iter()
            .filter(|t| t.0.starts_with("stdout "))
            .collect();
        assert_eq!(stdouts.len(), 2, "should have 2 stdout tasks");
        for s in &stdouts {
            assert_eq!(
                s.2, fork.3,
                "stdout should start at fork end time ({}), got {}",
                fork.3, s.2
            );
        }
    }

    // ══════════════════════════════════════════════════════════════════════
    // Probes (zero-duration, omitted from output)
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn probes_omitted_from_output() {
        let reg = test_registry();
        let chart = build_and_emit(
            "clock 1kHz t {\n    constant(0.0) | ?mon | stdout()\n}",
            &reg,
        );
        let task_labels: Vec<String> = chart
            .lines()
            .filter_map(|l| parse_task_line(l).map(|(label, _, _, _)| label))
            .collect();
        assert!(
            !task_labels.iter().any(|l| l.contains("probe")),
            "probes should be omitted from timing output, got: {:?}",
            task_labels
        );
    }

    #[test]
    fn probe_does_not_affect_timing() {
        // adc | ?mon | stdout — probe is zero-duration, stdout starts at adc's end
        let reg = test_registry();
        let chart = build_and_emit(
            "clock 1kHz t {\n    constant(0.0) | ?mon | stdout()\n}",
            &reg,
        );
        let tasks: Vec<_> = chart.lines().filter_map(parse_task_line).collect();
        assert_eq!(tasks.len(), 2, "should have only adc and stdout (no probe)");
        let adc = &tasks[0];
        let stdout = &tasks[1];
        assert_eq!(
            stdout.2, adc.3,
            "stdout should start immediately after adc (probe adds no time)"
        );
    }

    // ══════════════════════════════════════════════════════════════════════
    // Special Node Types
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn fork_label_mermaid_safe() {
        let reg = test_registry();
        let chart = build_and_emit(
            "clock 1kHz t {\n    constant(0.0) | :raw | stdout()\n    :raw | stdout()\n}",
            &reg,
        );
        assert!(
            chart.contains("fork(raw) x"),
            "fork should use 'fork(name)' label, not ':name'"
        );
    }

    #[test]
    fn buffer_nodes_in_chart() {
        let reg = test_registry();
        let chart = build_and_emit(
            concat!(
                "clock 1kHz a {\n    constant(0.0) -> sig\n}\n",
                "clock 1kHz b {\n    @sig | stdout()\n}",
            ),
            &reg,
        );
        assert!(
            chart.contains("write(sig) x"),
            "buffer write should use 'write(name)' label"
        );
        assert!(
            chart.contains("read(sig) x"),
            "buffer read should use 'read(name)' label"
        );
    }

    // ══════════════════════════════════════════════════════════════════════
    // Modal Task Tests
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn modal_sections() {
        let reg = test_registry();
        let chart = build_and_emit(
            concat!(
                "clock 1kHz t {\n",
                "    control {\n        constant(0.0) | detect() -> ctrl\n    }\n",
                "    mode sync {\n        constant(0.0) | stdout()\n    }\n",
                "    mode data {\n        constant(0.0) | stdout()\n    }\n",
                "    switch(ctrl, sync, data) default sync\n",
                "}",
            ),
            &reg,
        );
        assert!(
            chart.contains("section t [control]"),
            "missing control section"
        );
        assert!(
            chart.contains("section t [mode: sync]"),
            "missing sync mode section"
        );
        assert!(
            chart.contains("section t [mode: data]"),
            "missing data mode section"
        );
    }

    #[test]
    fn modal_modes_start_at_zero() {
        // Modal modes are mutually exclusive, each should start at offset 0
        let reg = test_registry();
        let chart = build_and_emit(
            concat!(
                "clock 1kHz t {\n",
                "    control {\n        constant(0.0) | detect() -> ctrl\n    }\n",
                "    mode sync {\n        constant(0.0) | stdout()\n    }\n",
                "    mode data {\n        constant(0.0) | stdout()\n    }\n",
                "    switch(ctrl, sync, data) default sync\n",
                "}",
            ),
            &reg,
        );

        // Find mode sections and verify their first task starts at 0
        let lines: Vec<&str> = chart.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if line.trim().starts_with("section ") && line.contains("[mode:") {
                // Next task line in this section should start at 0
                if let Some(next_task) = lines[i + 1..].iter().find_map(|l| parse_task_line(l)) {
                    assert_eq!(
                        next_task.2, 0,
                        "first task in mode section should start at 0: {:?}",
                        line
                    );
                }
            }
        }
    }

    // ══════════════════════════════════════════════════════════════════════
    // Frequency Formatting
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn freq_mhz_formatting() {
        let reg = test_registry();
        let chart = build_and_emit("clock 10MHz t {\n    constant(0.0) | stdout()\n}", &reg);
        assert!(chart.contains("10MHz"), "should format as MHz");
        assert!(chart.contains("K=10"), "K factor should be 10 for 10MHz");
    }

    #[test]
    fn format_freq_values() {
        assert_eq!(format_freq(10_000_000.0), "10MHz");
        assert_eq!(format_freq(1_000_000.0), "1MHz");
        assert_eq!(format_freq(1_000.0), "1kHz");
        assert_eq!(format_freq(44_100.0), "44.1kHz");
        assert_eq!(format_freq(500.0), "500Hz");
    }

    // ══════════════════════════════════════════════════════════════════════
    // Determinism
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn deterministic_output() {
        let reg = test_registry();
        let source = concat!(
            "clock 1kHz a {\n    constant(0.0) | stdout()\n}\n",
            "clock 1kHz b {\n    constant(0.0) | stdout()\n}\n",
        );
        let chart1 = build_and_emit(source, &reg);
        let chart2 = build_and_emit(source, &reg);
        assert_eq!(
            chart1, chart2,
            "timing chart output should be deterministic"
        );
    }

    // ══════════════════════════════════════════════════════════════════════
    // Feedback / Back-edge Tests
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn feedback_back_edges_dont_block() {
        // Feedback loops should not create timing dependencies
        // (back-edges are skipped in ASAP computation)
        let reg = test_registry();
        let chart = build_and_emit(
            concat!(
                "param alpha = 0.5\n",
                "clock 1kHz t {\n",
                "    constant(0.0) | add(:fb) | mul($alpha) | :out | stdout()\n",
                "    :out | delay(1, 0.0) | :fb\n",
                "}",
            ),
            &reg,
        );
        // Should produce a valid chart (not hang or error)
        assert!(chart.starts_with("gantt\n"));

        let tasks: Vec<_> = chart.lines().filter_map(parse_task_line).collect();
        // All tasks should have valid timing (no negative or infinite values)
        for task in &tasks {
            assert!(
                task.3 >= task.2,
                "end must be >= start for task '{}': start={}, end={}",
                task.0,
                task.2,
                task.3
            );
        }
    }

    // ══════════════════════════════════════════════════════════════════════
    // Integration Tests
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn example_pdl_timing_chart() {
        let reg = test_registry();
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("examples/example.pdl");
        let source = std::fs::read_to_string(&path).expect("failed to read example.pdl");
        let chart = build_and_emit(&source, &reg);
        assert!(chart.starts_with("gantt\n"));
        assert!(chart.contains("section capture [pipeline]"));
        assert!(chart.contains("section drain [pipeline]"));

        // Validate all task lines follow spec
        for line in chart.lines() {
            if let Some((label, id, start, end)) = parse_task_line(line) {
                assert!(!label.contains(':'), "label has colon: {}", label);
                assert!(
                    id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
                    "invalid id: {}",
                    id
                );
                assert!(end >= start, "end < start: {} < {}", end, start);
            }
        }
    }

    #[test]
    fn receiver_pdl_timing_chart() {
        let reg = test_registry();
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("examples/receiver.pdl");
        let source = std::fs::read_to_string(&path).expect("failed to read receiver.pdl");
        let chart = build_and_emit(&source, &reg);
        assert!(chart.starts_with("gantt\n"));
        assert!(
            chart.contains("[control]"),
            "receiver should have control section"
        );
        assert!(
            chart.contains("[mode: sync]"),
            "receiver should have sync mode"
        );
        assert!(
            chart.contains("[mode: data]"),
            "receiver should have data mode"
        );
    }

    #[test]
    fn multirate_pdl_timing_chart() {
        let reg = test_registry();
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("examples/multirate.pdl");
        let source = std::fs::read_to_string(&path).expect("failed to read multirate.pdl");
        let chart = build_and_emit(&source, &reg);

        // Validate parallel branches
        let tasks: Vec<_> = chart.lines().filter_map(parse_task_line).collect();

        let fir = tasks.iter().find(|t| t.0.starts_with("fir "));
        let fft = tasks.iter().find(|t| t.0.starts_with("fft "));
        if let (Some(fir), Some(fft)) = (fir, fft) {
            assert_eq!(
                fir.2, fft.2,
                "multirate: fir and fft should start in parallel"
            );
        }
    }

    #[test]
    fn feedback_pdl_timing_chart() {
        let reg = test_registry();
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("examples/feedback.pdl");
        let source = std::fs::read_to_string(&path).expect("failed to read feedback.pdl");
        let chart = build_and_emit(&source, &reg);
        assert!(chart.starts_with("gantt\n"));

        // Validate all task lines
        for line in chart.lines() {
            if let Some((label, _, start, end)) = parse_task_line(line) {
                assert!(!label.contains(':'), "colon in label: {}", label);
                assert!(end >= start, "invalid range: {} > {}", start, end);
            }
        }
    }
}
