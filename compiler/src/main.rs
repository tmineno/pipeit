use clap::Parser;
use std::path::PathBuf;

#[derive(Debug, Clone, clap::ValueEnum)]
enum EmitStage {
    Exe,
    Cpp,
    Ast,
    Graph,
    GraphDot,
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

    /// Actor header file (repeatable)
    #[arg(short = 'I', long = "include")]
    include: Vec<PathBuf>,

    /// Actor search directory (repeatable)
    #[arg(long)]
    actor_path: Vec<PathBuf>,

    /// Output stage
    #[arg(long, value_enum, default_value_t = EmitStage::Exe)]
    emit: EmitStage,

    /// Release build: strip probes, enable optimizations
    #[arg(long)]
    release: bool,

    /// C++ compiler command
    #[arg(long, default_value = "c++")]
    cc: String,

    /// Additional C++ compiler flags
    #[arg(long, default_value = "-O2")]
    cflags: String,

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

    // ── Load actor registry ──
    let mut registry = pcc::registry::Registry::new();
    for path in &cli.include {
        match registry.load_header(path) {
            Ok(n) => {
                if cli.verbose {
                    eprintln!("pcc: loaded {} actors from {}", n, path.display());
                }
            }
            Err(e) => {
                eprintln!("pcc: error: {}", e);
                std::process::exit(2);
            }
        }
    }

    if cli.verbose {
        eprintln!("pcc: {} actors registered", registry.len());
    }

    // ── Read and parse source ──
    let source = match std::fs::read_to_string(&cli.source) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("pcc: error: {}: {}", cli.source.display(), e);
            std::process::exit(2);
        }
    };

    let parse_result = pcc::parser::parse(&source);
    if !parse_result.errors.is_empty() {
        for err in &parse_result.errors {
            eprintln!("pcc: parse error: {}", err);
        }
        std::process::exit(1);
    }
    let program = match parse_result.program {
        Some(p) => p,
        None => {
            eprintln!("pcc: parse failed with no output");
            std::process::exit(1);
        }
    };

    if cli.verbose {
        eprintln!("pcc: parsed {} statements", program.statements.len());
    }

    // ── Name resolution ──
    let resolve_result = pcc::resolve::resolve(&program, &registry);
    if !resolve_result.diagnostics.is_empty() {
        for diag in &resolve_result.diagnostics {
            eprintln!("pcc: {}", diag);
        }
        if resolve_result
            .diagnostics
            .iter()
            .any(|d| d.level == pcc::resolve::DiagLevel::Error)
        {
            std::process::exit(1);
        }
    }

    if cli.verbose {
        eprintln!(
            "pcc: resolved {} consts, {} params, {} buffers",
            resolve_result.resolved.consts.len(),
            resolve_result.resolved.params.len(),
            resolve_result.resolved.buffers.len(),
        );
    }

    // ── Graph construction ──
    let graph_result = pcc::graph::build_graph(&program, &resolve_result.resolved, &registry);
    if !graph_result.diagnostics.is_empty() {
        for diag in &graph_result.diagnostics {
            eprintln!("pcc: {}", diag);
        }
        if graph_result
            .diagnostics
            .iter()
            .any(|d| d.level == pcc::resolve::DiagLevel::Error)
        {
            std::process::exit(1);
        }
    }

    if cli.verbose {
        eprintln!("pcc: built {} task graphs", graph_result.graph.tasks.len());
    }

    // ── Emit graph (if requested) ──
    match cli.emit {
        EmitStage::Graph => {
            println!("{}", graph_result.graph);
            std::process::exit(0);
        }
        EmitStage::GraphDot => {
            print!("{}", pcc::dot::emit_dot(&graph_result.graph));
            std::process::exit(0);
        }
        _ => {}
    }

    // ── Static analysis ──
    let analysis_result = pcc::analyze::analyze(
        &program,
        &resolve_result.resolved,
        &graph_result.graph,
        &registry,
    );
    if !analysis_result.diagnostics.is_empty() {
        for diag in &analysis_result.diagnostics {
            eprintln!("pcc: {}", diag);
        }
        if analysis_result
            .diagnostics
            .iter()
            .any(|d| d.level == pcc::resolve::DiagLevel::Error)
        {
            std::process::exit(1);
        }
    }

    if cli.verbose {
        eprintln!(
            "pcc: analysis complete, {} repetition vectors computed",
            analysis_result.analysis.repetition_vectors.len(),
        );
    }

    // TODO: implement remaining compiler pipeline phases (schedule, codegen)
    eprintln!("pcc: not yet implemented (past static analysis)");
    std::process::exit(1);
}
