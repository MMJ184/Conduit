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
    /// Run tasks from tasks.toml.
    ///
    /// Worktree isolation: with multiple tasks at concurrency > 1, each task
    /// gets its own git worktree under .conduit/work/<task-id>/ so concurrent
    /// edits don't collide. Single-task or sequential runs (concurrency = 1,
    /// or only one task in tasks.toml) execute directly in the project root
    /// — the Code stage will modify your working tree. If a Code-stage diff
    /// is applied but a downstream stage fails, those changes remain.
    /// Commit or stash before running if you want a clean recovery point.
    Run {
        #[arg(long, help = "Run a specific task by id")]
        task: Option<String>,
        #[arg(long, help = "Use a named run profile (skips interactive selection)")]
        profile: Option<String>,
        #[arg(long, help = "Maximum number of tasks to run simultaneously (default: all)")]
        concurrency: Option<usize>,
        #[arg(long, help = "Override starting account for all stages")]
        account: Option<String>,
        #[arg(long, help = "Re-run all stages even if .conduit/tasks/<id>/*.md exists")]
        force: bool,
        #[arg(long, help = "Disable per-task git worktree isolation; Code stage edits the project working tree directly")]
        no_worktree: bool,
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
        Commands::Run { task, profile, concurrency, account, force, no_worktree } => {
            commands::run::run(&cwd, task.as_deref(), profile.as_deref(), concurrency, account.as_deref(), force, no_worktree)
        }
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
