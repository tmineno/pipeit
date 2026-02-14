use clap::Parser;
use std::path::PathBuf;

#[derive(Debug, Clone, clap::ValueEnum)]
enum EmitStage {
    Exe,
    Cpp,
    Ast,
    Graph,
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

    // TODO: implement remaining compiler pipeline phases (graph, analysis, codegen)
    eprintln!("pcc: not yet implemented (past name resolution)");
    std::process::exit(1);
}
