use anyhow::Result;
use colored::Colorize;
use conduit_core::{
    config::{load_global_config, save_global_config, Config, Profile},
    error::ConduitError,
    parallel::{ParallelRunner, TaskEvent},
    pipeline::PipelineRunner,
    provider::FallbackResolver,
    spend::SpendTracker,
    tasks::load_tasks,
};
use dialoguer::{Input, Select};
use std::path::Path;
use std::sync::{Arc, Mutex};

pub fn run(
    dir: &Path,
    task_id: Option<&str>,
    profile_name: Option<&str>,
    concurrency: Option<usize>,
    account: Option<&str>,
    force: bool,
    no_worktree: bool,
) -> Result<()> {
    if let Some(0) = concurrency {
        anyhow::bail!("--concurrency must be at least 1");
    }

    // Load config first so we can validate --account before loading tasks
    let config = load_global_config()?;

    // Validate --account before loading tasks (fast fail with clear error)
    if let Some(acc_name) = account {
        if !config.ai_account.iter().any(|a| a.name == acc_name) {
            anyhow::bail!(
                "Account '{}' not found. Run `conduit providers list` to see available accounts.",
                acc_name
            );
        }
    }

    let mut tasks = load_tasks(dir)?;

    if let Some(id) = task_id {
        tasks.retain(|t| t.id == id);
        if tasks.is_empty() {
            return Err(ConduitError::TaskNotFound(id.to_string()).into());
        }
    }

    if config.ai_account.is_empty() {
        return Err(ConduitError::NoProvidersConfigured.into());
    }

    let profile = if let Some(name) = profile_name {
        config
            .profile
            .iter()
            .find(|p| p.name == name)
            .ok_or_else(|| ConduitError::ProfileNotFound(name.to_string()))?
            .clone()
    } else {
        select_profile_interactive(&config)?
    };

    // Load spend tracker; fall back to in-memory if file unreadable
    let spend = Arc::new(Mutex::new(
        SpendTracker::load().unwrap_or_else(|_| SpendTracker::new_empty()),
    ));

    let resolver = FallbackResolver::new(&profile, &config, Arc::clone(&spend), account);
    let concurrency = concurrency.unwrap_or(tasks.len().max(1));
    let use_parallel = tasks.len() > 1 && concurrency > 1;

    if use_parallel {
        let print_lock = Arc::new(Mutex::new(()));
        let runner = ParallelRunner::new(&tasks, &resolver, dir, concurrency)
            .with_worktree(!no_worktree)
            .with_force(force);
        let results = runner.run(|event| {
            let _guard = print_lock.lock().unwrap();
            match event {
                TaskEvent::Started(id) => println!("[{}] running...", id),
                TaskEvent::StageComplete { task_id, completed, total, stage } => {
                    println!("[{}]   [{}/{}] {}  {}", task_id, completed, total, stage, "✓".green());
                }
                TaskEvent::Finished(id) => {
                    println!("[{}] {} {}", id, "done".green().bold(), "✓".green());
                }
                TaskEvent::Failed { task_id, error } => {
                    eprintln!("[{}] {}  {}", task_id, "✗".red(), error);
                }
            }
        });

        let failed_count = results.iter().filter(|r| r.error.is_some()).count();
        let completed_count = results.len() - failed_count;
        if failed_count > 0 {
            println!("\nResults: {} completed, {} failed.", completed_count, failed_count);
            anyhow::bail!("{} task(s) failed", failed_count);
        }
    } else {
        for task in &tasks {
            println!("{} {}", "[running]".cyan().bold(), task.id.bold());
            let runner = PipelineRunner::new(task, &resolver, dir).with_force(force);
            let result = runner.run(|completed, total, stage| {
                println!(
                    "  [{}/{}] {}  {}",
                    completed, total, stage.display_name(), "✓".green()
                );
            });
            match result {
                Ok(()) => println!("{} {}", "[done]".green().bold(), task.id.bold()),
                Err(e) => {
                    eprintln!("  {} {}", "✗".red(), e);
                    return Err(e.into());
                }
            }
        }
    }
    Ok(())
}

fn select_profile_interactive(config: &Config) -> Result<Profile> {
    let account_names: Vec<&str> = config.ai_account.iter().map(|a| a.name.as_str()).collect();

    if config.profile.is_empty() {
        return configure_profile_interactive(config, &account_names);
    }

    let mut options: Vec<String> = config.profile.iter().map(|p| p.name.clone()).collect();
    options.push("Configure new...".to_string());

    let selection = Select::new()
        .with_prompt("Select a run profile")
        .items(&options)
        .default(0)
        .interact()?;

    if selection < config.profile.len() {
        Ok(config.profile[selection].clone())
    } else {
        configure_profile_interactive(config, &account_names)
    }
}

fn configure_profile_interactive(config: &Config, account_names: &[&str]) -> Result<Profile> {
    let mode_options = ["Single provider (all stages)", "Multiple providers (per stage)"];
    let mode = Select::new()
        .with_prompt("Use single provider or multiple?")
        .items(&mode_options)
        .default(0)
        .interact()?;

    let (provider_field, orchestrator, doc, architecture, code, test) = if mode == 0 {
        let idx = Select::new()
            .with_prompt("Provider account")
            .items(account_names)
            .default(0)
            .interact()?;
        (Some(account_names[idx].to_string()), None, None, None, None, None)
    } else {
        let o = Select::new().with_prompt("Orchestrator stage").items(account_names).default(0).interact()?;
        let d = Select::new().with_prompt("Doc stage").items(account_names).default(0).interact()?;
        let a = Select::new().with_prompt("Architecture stage").items(account_names).default(0).interact()?;
        let c = Select::new().with_prompt("Code stage").items(account_names).default(0).interact()?;
        let t = Select::new().with_prompt("Test stage").items(account_names).default(0).interact()?;
        (
            None,
            Some(account_names[o].to_string()),
            Some(account_names[d].to_string()),
            Some(account_names[a].to_string()),
            Some(account_names[c].to_string()),
            Some(account_names[t].to_string()),
        )
    };

    let save_name: String = Input::new()
        .with_prompt("Save as profile? (leave blank to skip)")
        .allow_empty(true)
        .interact_text()?;

    let profile = Profile {
        name: save_name.clone(),
        provider: provider_field,
        orchestrator,
        doc,
        architecture,
        code,
        test,
    };

    if !save_name.is_empty() {
        let mut updated = config.clone();
        updated.profile.push(profile.clone());
        save_global_config(&updated)?;
        println!("Profile \"{}\" saved.", save_name.green());
    }

    Ok(profile)
}
