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

    // TODO: implement remaining compiler pipeline phases
    eprintln!("pcc: not yet implemented");
    std::process::exit(1);
}
