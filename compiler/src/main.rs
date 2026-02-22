use clap::Parser;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

const EXIT_OK: i32 = 0;
const EXIT_COMPILE_ERROR: i32 = 1;
const EXIT_USAGE_ERROR: i32 = 2;
const EXIT_SYSTEM_ERROR: i32 = 3;

#[derive(Debug, Clone, clap::ValueEnum)]
enum EmitStage {
    Exe,
    Cpp,
    Ast,
    Graph,
    GraphDot,
    Schedule,
    TimingChart,
}

#[derive(Parser, Debug)]
#[command(
    name = "pcc",
    version,
    about = "Pipit Compiler Collection — compiles .pdl pipeline definitions to native executables"
)]
struct Cli {
    /// Input .pdl source file
    source: PathBuf,

    /// Output file path
    #[arg(short, long, default_value = "a.out")]
    output: PathBuf,

    /// Actor header file or search directory (repeatable)
    #[arg(short = 'I', long = "include")]
    include: Vec<PathBuf>,

    /// Actor search directory (repeatable)
    #[arg(long)]
    actor_path: Vec<PathBuf>,

    /// Actor metadata manifest file (actors.meta.json)
    #[arg(long)]
    actor_meta: Option<PathBuf>,

    /// Output stage
    #[arg(long, value_enum, default_value_t = EmitStage::Exe)]
    emit: EmitStage,

    /// Release build: strip probes, enable optimizations
    #[arg(long)]
    release: bool,

    /// C++ compiler command
    #[arg(long, default_value = "c++")]
    cc: String,

    /// Additional C++ compiler flags (overrides default optimization flags)
    #[arg(long)]
    cflags: Option<String>,

    /// Print compiler phases and timing
    #[arg(long)]
    verbose: bool,
}

fn main() {
    let cli = Cli::parse();

    if cli.verbose {
        eprintln!("pcc: source = {}", cli.source.display());
        eprintln!("pcc: output = {}", cli.output.display());
        eprintln!("pcc: emit   = {:?}", cli.emit);
    }

    // ── Read and parse source ──
    let source = match std::fs::read_to_string(&cli.source) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {}: {}", cli.source.display(), e);
            std::process::exit(EXIT_USAGE_ERROR);
        }
    };

    let parse_result = pcc::parser::parse(&source);
    if !parse_result.errors.is_empty() {
        for err in &parse_result.errors {
            let span = err.span();
            print_span_diagnostic(
                "error",
                &format!("{}", err),
                &cli.source,
                &source,
                span.start,
                span.end,
                None,
            );
        }
        std::process::exit(EXIT_COMPILE_ERROR);
    }

    let program = match parse_result.program {
        Some(p) => p,
        None => {
            eprintln!("error: parse failed with no output");
            std::process::exit(EXIT_COMPILE_ERROR);
        }
    };

    if cli.verbose {
        eprintln!("pcc: parsed {} statements", program.statements.len());
    }

    if matches!(cli.emit, EmitStage::Ast) {
        println!("{:#?}", program);
        std::process::exit(EXIT_OK);
    }

    // ── Load actor registry ──
    let (registry, loaded_headers) = match load_actor_registry(&cli) {
        Ok(v) => v,
        Err((msg, code)) => {
            eprintln!("error: {}", msg);
            std::process::exit(code);
        }
    };

    if cli.verbose {
        eprintln!("pcc: {} actors registered", registry.len());
    }

    // ── Map EmitStage to terminal PassId ──
    let terminal = match cli.emit {
        EmitStage::Ast => unreachable!(), // handled above
        EmitStage::GraphDot => pcc::pass::PassId::BuildGraph,
        EmitStage::Graph | EmitStage::Schedule | EmitStage::TimingChart => {
            pcc::pass::PassId::Schedule
        }
        EmitStage::Cpp | EmitStage::Exe => pcc::pass::PassId::Codegen,
    };

    // ── Run pipeline ──
    let codegen_options = pcc::codegen::CodegenOptions {
        release: cli.release,
        include_paths: loaded_headers.clone(),
    };
    let mut state = pcc::pipeline::CompilationState::new(program, registry);
    let mut has_errors = false;
    let result = pcc::pipeline::run_pipeline(
        &mut state,
        terminal,
        &codegen_options,
        cli.verbose,
        |_pass_id, diags| {
            has_errors |= print_pipeline_diags(&cli.source, &source, diags);
        },
    );

    if has_errors || result.is_err() {
        std::process::exit(EXIT_COMPILE_ERROR);
    }

    // ── Emit-specific output ──
    match cli.emit {
        EmitStage::Ast => unreachable!(),
        EmitStage::GraphDot => {
            print!(
                "{}",
                pcc::dot::emit_dot(state.upstream.graph.as_ref().unwrap())
            );
            std::process::exit(EXIT_OK);
        }
        EmitStage::Graph => {
            print!(
                "{}",
                emit_graph_dump(
                    state.upstream.graph.as_ref().unwrap(),
                    state.downstream.analysis.as_ref().unwrap(),
                    state.downstream.schedule.as_ref().unwrap(),
                )
            );
            std::process::exit(EXIT_OK);
        }
        EmitStage::Schedule => {
            print!("{}", state.downstream.schedule.as_ref().unwrap());
            std::process::exit(EXIT_OK);
        }
        EmitStage::TimingChart => {
            print!(
                "{}",
                pcc::timing::emit_timing_chart(
                    state.downstream.schedule.as_ref().unwrap(),
                    state.upstream.graph.as_ref().unwrap()
                )
            );
            std::process::exit(EXIT_OK);
        }
        EmitStage::Cpp => {
            let cpp_source = &state.downstream.generated.as_ref().unwrap().cpp_source;
            if cli.output == Path::new("-") {
                print!("{}", cpp_source);
            } else if let Err(e) = std::fs::write(&cli.output, cpp_source) {
                eprintln!("error: failed to write {}: {}", cli.output.display(), e);
                std::process::exit(EXIT_SYSTEM_ERROR);
            }

            if cli.verbose {
                eprintln!("pcc: wrote {}", cli.output.display());
            }
            std::process::exit(EXIT_OK);
        }
        EmitStage::Exe => {
            // Write generated C++ to temp file
            let tmp_dir = std::env::temp_dir();
            let tmp_cpp = tmp_dir.join(format!("pcc_generated_{}.cpp", std::process::id()));
            let cpp_source = &state.downstream.generated.as_ref().unwrap().cpp_source;
            if let Err(e) = std::fs::write(&tmp_cpp, cpp_source) {
                eprintln!(
                    "error: failed to write temp file {}: {}",
                    tmp_cpp.display(),
                    e
                );
                std::process::exit(EXIT_SYSTEM_ERROR);
            }

            // Build compiler command
            let mut cmd = std::process::Command::new(&cli.cc);
            cmd.arg("-std=c++17");

            if let Some(flags) = &cli.cflags {
                for flag in flags.split_whitespace() {
                    cmd.arg(flag);
                }
            } else if cli.release {
                cmd.arg("-O2");
            } else {
                cmd.arg("-O0").arg("-g");
            }

            if cli.release {
                cmd.arg("-DNDEBUG");
            }

            // Runtime headers live at workspace/runtime/libpipit/include.
            let runtime_include = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("runtime")
                .join("libpipit")
                .join("include");
            if runtime_include.exists() {
                cmd.arg("-I").arg(&runtime_include);
            }

            // Include directories for actor headers (needed for emitted #include "..." lines).
            let mut include_dirs = BTreeSet::new();
            for path in &loaded_headers {
                if let Some(dir) = path.parent() {
                    include_dirs.insert(dir.to_path_buf());
                }
            }
            for dir in include_dirs {
                cmd.arg("-I").arg(dir);
            }

            // Force-include actor headers discovered from both -I and --actor-path.
            for path in &loaded_headers {
                cmd.arg("-include").arg(path);
            }

            cmd.arg("-lpthread");
            cmd.arg("-o").arg(&cli.output);
            cmd.arg(&tmp_cpp);

            if cli.verbose {
                eprintln!("pcc: running {:?}", cmd);
            }

            let status = match cmd.status() {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("error: failed to run C++ compiler '{}': {}", cli.cc, e);
                    let _ = std::fs::remove_file(&tmp_cpp);
                    std::process::exit(EXIT_SYSTEM_ERROR);
                }
            };

            // Clean up temp file
            let _ = std::fs::remove_file(&tmp_cpp);

            if !status.success() {
                eprintln!("error: C++ compilation failed");
                std::process::exit(EXIT_COMPILE_ERROR);
            }

            if cli.verbose {
                eprintln!("pcc: wrote {}", cli.output.display());
            }

            std::process::exit(EXIT_OK);
        }
    }
}

fn load_actor_registry(
    cli: &Cli,
) -> Result<(pcc::registry::Registry, Vec<PathBuf>), (String, i32)> {
    // If --actor-meta is provided, load directly from manifest
    if let Some(ref meta_path) = cli.actor_meta {
        let meta_path = std::fs::canonicalize(meta_path)
            .map_err(|e| (format!("{}: {}", meta_path.display(), e), EXIT_USAGE_ERROR))?;

        let mut registry = pcc::registry::Registry::new();
        registry
            .load_manifest(&meta_path)
            .map_err(map_registry_error)?;

        if cli.verbose {
            eprintln!(
                "pcc: loaded {} actors from manifest {}",
                registry.len(),
                meta_path.display()
            );
        }

        // Still collect headers for -include flags during C++ compilation
        let all_headers = collect_all_headers(cli)?;
        return Ok((registry, all_headers));
    }

    // Otherwise, load from headers (existing behavior)
    load_actor_registry_from_headers(cli)
}

/// Collect all header paths from -I and --actor-path for C++ compilation.
fn collect_all_headers(cli: &Cli) -> Result<Vec<PathBuf>, (String, i32)> {
    let canonicalized_includes = canonicalize_all(&cli.include, EXIT_USAGE_ERROR)?;
    let mut include_headers = Vec::new();
    for path in canonicalized_includes {
        if path.is_dir() {
            let mut discovered = BTreeSet::new();
            discover_headers_recursive(&path, &mut discovered)?;
            include_headers.extend(discovered);
        } else {
            include_headers.push(path);
        }
    }
    let actor_path_headers = discover_actor_headers(&cli.actor_path)?;

    let mut all_headers = Vec::new();
    all_headers.extend(actor_path_headers);
    all_headers.extend(include_headers);

    let mut dedup = BTreeSet::new();
    all_headers.retain(|p| dedup.insert(p.clone()));

    Ok(all_headers)
}

/// Load actor registry from header scanning (pre-v0.3.0 behavior).
fn load_actor_registry_from_headers(
    cli: &Cli,
) -> Result<(pcc::registry::Registry, Vec<PathBuf>), (String, i32)> {
    let canonicalized_includes = canonicalize_all(&cli.include, EXIT_USAGE_ERROR)?;
    let mut include_headers = Vec::new();
    for path in canonicalized_includes {
        if path.is_dir() {
            let mut discovered = BTreeSet::new();
            discover_headers_recursive(&path, &mut discovered)?;
            include_headers.extend(discovered);
        } else {
            include_headers.push(path);
        }
    }
    let actor_path_headers = discover_actor_headers(&cli.actor_path)?;

    let mut include_registry = pcc::registry::Registry::new();
    for path in &include_headers {
        include_registry
            .load_header(path)
            .map_err(map_registry_error)?;

        if cli.verbose {
            eprintln!("pcc: loaded actors from include {}", path.display());
        }
    }

    let mut actor_path_registry = pcc::registry::Registry::new();
    for path in &actor_path_headers {
        actor_path_registry
            .load_header(path)
            .map_err(map_registry_error)?;

        if cli.verbose {
            eprintln!("pcc: loaded actors from actor-path {}", path.display());
        }
    }

    // --actor-path is base registry; -I overlays with precedence.
    actor_path_registry.overlay_from(&include_registry);

    let mut all_headers = Vec::new();
    all_headers.extend(actor_path_headers);
    all_headers.extend(include_headers);

    let mut dedup = BTreeSet::new();
    all_headers.retain(|p| dedup.insert(p.clone()));

    Ok((actor_path_registry, all_headers))
}

fn canonicalize_all(paths: &[PathBuf], err_code: i32) -> Result<Vec<PathBuf>, (String, i32)> {
    let mut out = Vec::new();
    for path in paths {
        let abs = std::fs::canonicalize(path)
            .map_err(|e| (format!("{}: {}", path.display(), e), err_code))?;
        out.push(abs);
    }
    Ok(out)
}

fn discover_actor_headers(actor_paths: &[PathBuf]) -> Result<Vec<PathBuf>, (String, i32)> {
    let mut discovered = BTreeSet::new();

    for path in actor_paths {
        let root = std::fs::canonicalize(path)
            .map_err(|e| (format!("{}: {}", path.display(), e), EXIT_USAGE_ERROR))?;

        if !root.is_dir() {
            return Err((
                format!("--actor-path expects a directory: {}", root.display()),
                EXIT_USAGE_ERROR,
            ));
        }

        discover_headers_recursive(&root, &mut discovered)?;
    }

    Ok(discovered.into_iter().collect())
}

fn discover_headers_recursive(
    dir: &Path,
    out: &mut BTreeSet<PathBuf>,
) -> Result<(), (String, i32)> {
    let entries = std::fs::read_dir(dir).map_err(|e| {
        (
            format!("failed to read {}: {}", dir.display(), e),
            EXIT_SYSTEM_ERROR,
        )
    })?;

    for entry in entries {
        let entry = entry.map_err(|e| {
            (
                format!("failed to read directory entry in {}: {}", dir.display(), e),
                EXIT_SYSTEM_ERROR,
            )
        })?;

        let path = entry.path();
        if path.is_dir() {
            // Skip vendored third-party directories — their headers are included
            // transitively via top-level headers, not directly.
            if path.file_name().and_then(|n| n.to_str()) == Some("third_party") {
                continue;
            }
            discover_headers_recursive(&path, out)?;
            continue;
        }

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase());

        if matches!(ext.as_deref(), Some("h" | "hh" | "hpp" | "hxx")) {
            let abs = std::fs::canonicalize(&path).map_err(|e| {
                (
                    format!("failed to canonicalize {}: {}", path.display(), e),
                    EXIT_SYSTEM_ERROR,
                )
            })?;
            out.insert(abs);
        }
    }

    Ok(())
}

fn map_registry_error(e: pcc::registry::RegistryError) -> (String, i32) {
    match e {
        pcc::registry::RegistryError::IoError { .. } => (format!("{}", e), EXIT_SYSTEM_ERROR),
        pcc::registry::RegistryError::ParseError { .. }
        | pcc::registry::RegistryError::DuplicateActor { .. } => {
            (format!("{}", e), EXIT_COMPILE_ERROR)
        }
    }
}

fn print_pipeline_diags(
    source_path: &Path,
    source: &str,
    diags: &[pcc::resolve::Diagnostic],
) -> bool {
    let mut has_error = false;

    for diag in diags {
        let (level, is_error) = match diag.level {
            pcc::resolve::DiagLevel::Error => ("error", true),
            pcc::resolve::DiagLevel::Warning => ("warning", false),
        };

        print_span_diagnostic(
            level,
            &diag.message,
            source_path,
            source,
            diag.span.start,
            diag.span.end,
            None,
        );

        has_error |= is_error;
    }

    has_error
}

fn print_span_diagnostic(
    level: &str,
    message: &str,
    source_path: &Path,
    source: &str,
    span_start: usize,
    span_end: usize,
    hint: Option<&str>,
) {
    let start = span_start.min(source.len());
    let end = span_end.min(source.len());

    let line_start = source[..start].rfind('\n').map_or(0, |i| i + 1);
    let line_end = source[start..]
        .find('\n')
        .map_or(source.len(), |i| start + i);
    let line_text = &source[line_start..line_end];

    let line_no = source[..line_start].bytes().filter(|b| *b == b'\n').count() + 1;
    let col_no = source[line_start..start].chars().count() + 1;

    let mut caret_width = if end > start {
        let caret_end = end.min(line_end);
        source[start..caret_end].chars().count().max(1)
    } else {
        1
    };

    if line_text.is_empty() {
        caret_width = 1;
    }

    eprintln!("{}: {}", level, message);
    eprintln!("  at {}:{}:{}", source_path.display(), line_no, col_no);
    eprintln!("  {}", line_text);
    eprintln!(
        "  {}{}",
        " ".repeat(col_no.saturating_sub(1)),
        "^".repeat(caret_width)
    );
    if let Some(h) = hint {
        eprintln!("  hint: {}", h);
    }
}

fn emit_graph_dump(
    graph: &pcc::graph::ProgramGraph,
    analysis: &pcc::analyze::AnalyzedProgram,
    schedule: &pcc::schedule::ScheduledProgram,
) -> String {
    let mut out = String::new();

    let _ = writeln!(out, "{}", graph);

    // repetition_vector entries
    let mut rv = BTreeMap::new();
    for ((task, label), counts) in &analysis.repetition_vectors {
        let mut nodes: Vec<(u32, u32)> = counts.iter().map(|(id, c)| (id.0, *c)).collect();
        nodes.sort_unstable_by_key(|(id, _)| *id);
        rv.insert((task.clone(), label.clone()), nodes);
    }

    if !rv.is_empty() {
        let _ = writeln!(out, "repetition_vectors:");
        for ((task, label), nodes) in rv {
            let parts = nodes
                .iter()
                .map(|(id, c)| format!("n{}={}", id, c))
                .collect::<Vec<_>>()
                .join(", ");
            let _ = writeln!(out, "  repetition_vector {}.{}: {}", task, label, parts);
        }
    }

    // inter-task buffer sizes (bytes)
    if !analysis.inter_task_buffers.is_empty() {
        let mut inter: Vec<_> = analysis.inter_task_buffers.iter().collect();
        inter.sort_by(|a, b| a.0.cmp(b.0));

        let _ = writeln!(out, "buffer_sizes:");
        for (name, bytes) in inter {
            let _ = writeln!(out, "  buffer_size inter.{}: {}", name, bytes);
        }
    }

    // intra-task edge buffer sizes (tokens)
    let mut task_names: Vec<_> = schedule.tasks.keys().cloned().collect();
    task_names.sort();

    for task in task_names {
        let Some(meta) = schedule.tasks.get(&task) else {
            continue;
        };

        match &meta.schedule {
            pcc::schedule::TaskSchedule::Pipeline(sub) => {
                emit_subgraph_buffer_sizes(&mut out, &task, "pipeline", sub);
            }
            pcc::schedule::TaskSchedule::Modal { control, modes } => {
                emit_subgraph_buffer_sizes(&mut out, &task, "control", control);
                let mut sorted_modes = modes.clone();
                sorted_modes.sort_by(|a, b| a.0.cmp(&b.0));
                for (mode, sub) in sorted_modes {
                    emit_subgraph_buffer_sizes(&mut out, &task, &mode, &sub);
                }
            }
        }
    }

    out
}

fn emit_subgraph_buffer_sizes(
    out: &mut String,
    task: &str,
    label: &str,
    sub: &pcc::schedule::SubgraphSchedule,
) {
    if sub.edge_buffers.is_empty() {
        return;
    }

    let mut edges: Vec<_> = sub.edge_buffers.iter().collect();
    edges.sort_by_key(|((src, dst), _)| (src.0, dst.0));

    for ((src, dst), tokens) in edges {
        let _ = writeln!(
            out,
            "  buffer_size edge {}.{} n{}->n{}: {}",
            task, label, src.0, dst.0, tokens
        );
    }
}
