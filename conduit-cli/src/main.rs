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
    /// Initialize Conduit (global config + project folder)
    Init {
        #[arg(long, help = "Overwrite existing global config without prompting")]
        force: bool,
    },
    /// Run tasks from tasks.toml
    Run {
        #[arg(long, help = "Run a specific task by id")]
        task: Option<String>,
        #[arg(long, help = "Use a named run profile (skips interactive selection)")]
        profile: Option<String>,
    },
    /// Validate tasks.toml without running
    Validate,
    /// Show configured AI accounts and profiles
    Status,
    /// Manage AI provider accounts
    Providers {
        #[command(subcommand)]
        command: ProviderCommands,
    },
}

#[derive(Subcommand)]
enum ProviderCommands {
    /// List configured accounts and profiles
    List,
    /// Add a new provider account interactively
    Add,
    /// Remove a provider account
    Remove {
        #[arg(help = "Account name to remove")]
        name: String,
    },
    /// Re-run login for an existing account
    Login {
        #[arg(help = "Account name")]
        name: String,
    },
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
        Commands::Run { task, profile } => commands::run::run(&cwd, task.as_deref(), profile.as_deref()),
        Commands::Validate => commands::validate::run(&cwd),
        Commands::Status => commands::status::run(),
        Commands::Providers { command } => match command {
            ProviderCommands::List => commands::providers::list(),
            ProviderCommands::Add => commands::providers::add(),
            ProviderCommands::Remove { name } => commands::providers::remove(&name),
            ProviderCommands::Login { name } => commands::providers::login(&name),
        },
    }
}
