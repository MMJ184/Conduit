use anyhow::{bail, Result};
use colored::Colorize;
use conduit_core::config::{
    global_config_path, load_global_config, save_global_config, AIAccount, Config, Defaults,
};
use dialoguer::{Confirm, Input, Select};
use std::path::Path;
use std::process::Command;

const STARTER_TASKS: &str = r#"[[task]]
id = "hello-world"
description = "Create a hello world example"

# Add more tasks below:
# [[task]]
# id = "my-feature"
# description = "Describe what you want to build"
"#;

const PROVIDERS: &[(&str, &str, &[&str])] = &[
    ("Claude (claude CLI)", "claude", &["auth", "login"]),
    ("OpenAI Codex (codex CLI)", "openai", &["login"]),
    ("Google Gemini (gemini CLI)", "gemini", &["auth", "login"]),
];

pub fn run(dir: &Path, force: bool) -> Result<()> {
    let config_path = global_config_path()?;

    if config_path.exists() && !force {
        println!(
            "{} Global config found at {}",
            "✓".green(),
            config_path.display()
        );
        create_project_conduit_dir(dir)?;
        write_starter_tasks(dir)?;
        println!("{} Project folder .conduit/ ready.", "✓".green());
        println!("\nRun {} to get started.", "`conduit validate`".cyan());
        return Ok(());
    }

    println!("{}", "Conduit Init".bold());
    println!("Setting up global config at {}\n", config_path.display());

    let mut config = if config_path.exists() && force {
        load_global_config().unwrap_or_default()
    } else {
        Config::default()
    };

    for (label, provider_type, login_args) in PROVIDERS {
        let binary = match *provider_type {
            "openai" => "codex",
            other => other,
        };

        if which::which(binary).is_err() {
            println!("{} {} not found on PATH — skipping.", "✗".dimmed(), label);
            continue;
        }

        let configure = Confirm::new()
            .with_prompt(format!("Configure {}?", label))
            .default(true)
            .interact()?;

        if !configure {
            continue;
        }

        println!("Opening {} login...", label.cyan());
        let status = Command::new(binary)
            .args(*login_args)
            .stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status()?;

        if !status.success() {
            println!("{} Login failed — skipping {}.", "✗".yellow(), label);
            continue;
        }

        loop {
            let name: String = Input::new()
                .with_prompt("Account name (e.g. \"work\", \"personal\")")
                .interact_text()?;

            if name.is_empty() {
                println!("Name cannot be empty.");
                continue;
            }
            if config.ai_account.iter().any(|a| a.name == name) {
                println!("Account name '{}' already exists. Choose a different name.", name);
                continue;
            }

            config.ai_account.push(AIAccount {
                name,
                provider: provider_type.to_string(),
                daily_limit_usd: None,
            });
            break;
        }

        let add_another = Confirm::new()
            .with_prompt(format!("Add another {} account?", label))
            .default(false)
            .interact()?;

        if add_another {
            println!("Opening {} login again...", label.cyan());
            let status2 = Command::new(binary)
                .args(*login_args)
                .stdin(std::process::Stdio::inherit())
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit())
                .status()?;

            if status2.success() {
                loop {
                    let name: String = Input::new()
                        .with_prompt("Account name for second account")
                        .interact_text()?;
                    if name.is_empty() { continue; }
                    if config.ai_account.iter().any(|a| a.name == name) {
                        println!("Name '{}' already exists.", name);
                        continue;
                    }
                    config.ai_account.push(AIAccount {
                        name,
                        provider: provider_type.to_string(),
                        daily_limit_usd: None,
                    });
                    break;
                }
            }
        }
    }

    if !config.ai_account.is_empty() {
        let names: Vec<&str> = config.ai_account.iter().map(|a| a.name.as_str()).collect();
        let idx = Select::new()
            .with_prompt("Default orchestrator account")
            .items(&names)
            .default(0)
            .interact()?;
        config.defaults = Defaults { orchestrator: Some(names[idx].to_string()) };
    }

    save_global_config(&config)?;
    println!("\n{} Global config saved to {}", "✓".green(), config_path.display());

    create_project_conduit_dir(dir)?;
    write_starter_tasks(dir)?;
    println!("{} Project folder .conduit/ created.", "✓".green());
    println!("\nRun {} to get started.", "`conduit validate`".cyan());
    Ok(())
}

fn create_project_conduit_dir(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir.join(".conduit"))?;
    Ok(())
}

pub fn write_starter_tasks(dir: &Path) -> Result<()> {
    let tasks_path = dir.join("tasks.toml");
    if !tasks_path.exists() {
        std::fs::write(&tasks_path, STARTER_TASKS)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use std::fs;

    #[test]
    fn test_write_starter_tasks_creates_file() {
        let dir = tempdir().unwrap();
        write_starter_tasks(dir.path()).unwrap();
        assert!(dir.path().join("tasks.toml").exists());
    }

    #[test]
    fn test_write_starter_tasks_skips_if_exists() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("tasks.toml"), "existing").unwrap();
        write_starter_tasks(dir.path()).unwrap();
        assert_eq!(fs::read_to_string(dir.path().join("tasks.toml")).unwrap(), "existing");
    }
}
