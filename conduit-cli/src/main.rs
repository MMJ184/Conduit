use clap::{Parser, Subcommand};
use colored::Colorize;

mod commands;

#[derive(Parser)]
#[command(name = "conduit", version, about = "AI coding agent orchestrator")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new Conduit project
    Init {
        #[arg(long, help = "Overwrite existing config without prompting")]
        force: bool,
    },
    /// Run tasks from tasks.toml
    Run {
        #[arg(long, help = "Run a specific task by id")]
        task: Option<String>,
    },
    /// Validate tasks.toml without running
    Validate,
    /// Show configured AI accounts and limits
    Status,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{} {}", "Error:".red().bold(), e);
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let cwd = std::env::current_dir()?;

    match cli.command {
        Commands::Init { force } => commands::init::run(&cwd, force),
        Commands::Run { task } => commands::run::run(&cwd, task.as_deref()),
        Commands::Validate => commands::validate::run(&cwd),
        Commands::Status => commands::status::run(&cwd),
    }
}
