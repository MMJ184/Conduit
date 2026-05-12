use anyhow::Result;
use colored::Colorize;
use conduit_core::{
    config::{load_global_config, save_global_config, Config, Profile},
    error::ConduitError,
    pipeline::PipelineRunner,
    provider::ProfileResolver,
    tasks::load_tasks,
};
use dialoguer::{Input, Select};
use std::path::Path;

pub fn run(dir: &Path, task_id: Option<&str>, profile_name: Option<&str>) -> Result<()> {
    let mut tasks = load_tasks(dir)?;

    if let Some(id) = task_id {
        tasks.retain(|t| t.id == id);
        if tasks.is_empty() {
            return Err(ConduitError::TaskNotFound(id.to_string()).into());
        }
    }

    let config = load_global_config()?;

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

    let resolver = ProfileResolver { profile: &profile, config: &config };

    for task in &tasks {
        println!("{} {}", "[running]".cyan().bold(), task.id.bold());
        let runner = PipelineRunner::new(task, &resolver, dir);
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
