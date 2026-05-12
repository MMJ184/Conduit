use anyhow::{bail, Result};
use colored::Colorize;
use conduit_core::config::{AIAccount, Config, OllamaConfig, ProjectConfig};
use dialoguer::{Input, MultiSelect};
use std::path::Path;

const STARTER_TASKS: &str = r#"[[task]]
id = "hello-world"
description = "Create a hello world example"

# Add more tasks below:
# [[task]]
# id = "my-feature"
# description = "Describe what you want to build"
#
# [task.options]
# language = "rust"
# output_dir = "src"
"#;

pub fn check_no_existing_config(dir: &Path, force: bool) -> Result<()> {
    let config_path = dir.join(".conduit").join("config.toml");
    if config_path.exists() && !force {
        bail!(".conduit/config.toml already exists. Use --force to overwrite.");
    }
    Ok(())
}

pub fn write_config_file(dir: &Path, config: &Config) -> Result<()> {
    let toml_str = toml::to_string_pretty(config)?;
    std::fs::create_dir_all(dir.join(".conduit"))?;
    std::fs::write(dir.join(".conduit").join("config.toml"), toml_str)?;
    Ok(())
}

pub fn write_starter_tasks(dir: &Path) -> Result<()> {
    let tasks_path = dir.join("tasks.toml");
    if !tasks_path.exists() {
        std::fs::write(&tasks_path, STARTER_TASKS)?;
    }
    Ok(())
}

pub fn run(dir: &Path, force: bool) -> Result<()> {
    check_no_existing_config(dir, force)?;

    println!("{}", "Conduit Init".bold());
    println!("Setting up your project...\n");

    let default_name = dir
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let name: String = Input::new()
        .with_prompt("Project name")
        .default(default_name)
        .interact_text()?;

    let providers = &[
        "Claude (Anthropic)",
        "OpenAI Codex",
        "Google Gemini",
        "Ollama (local)",
    ];
    let selections = MultiSelect::new()
        .with_prompt("Which AI providers do you want to configure? (Space to select, Enter to confirm)")
        .items(providers)
        .interact()?;

    let mut ai_accounts: Vec<AIAccount> = Vec::new();
    let mut ollama = OllamaConfig::default();

    for &i in &selections {
        match i {
            0 => {
                let key: String = Input::new()
                    .with_prompt("Claude API key")
                    .interact_text()?;
                let limit: String = Input::new()
                    .with_prompt("Daily limit USD (leave blank for none)")
                    .allow_empty(true)
                    .interact_text()?;
                ai_accounts.push(AIAccount {
                    provider: "claude".to_string(),
                    api_key: key,
                    daily_limit_usd: limit.parse().ok(),
                });
            }
            1 => {
                let key: String = Input::new()
                    .with_prompt("OpenAI API key")
                    .interact_text()?;
                let limit: String = Input::new()
                    .with_prompt("Daily limit USD (leave blank for none)")
                    .allow_empty(true)
                    .interact_text()?;
                ai_accounts.push(AIAccount {
                    provider: "openai".to_string(),
                    api_key: key,
                    daily_limit_usd: limit.parse().ok(),
                });
            }
            2 => {
                let key: String = Input::new()
                    .with_prompt("Gemini API key")
                    .interact_text()?;
                let limit: String = Input::new()
                    .with_prompt("Daily limit USD (leave blank for none)")
                    .allow_empty(true)
                    .interact_text()?;
                ai_accounts.push(AIAccount {
                    provider: "gemini".to_string(),
                    api_key: key,
                    daily_limit_usd: limit.parse().ok(),
                });
            }
            3 => {
                let url: String = Input::new()
                    .with_prompt("Ollama base URL")
                    .default("http://localhost:11434".to_string())
                    .interact_text()?;
                ollama = OllamaConfig {
                    enabled: true,
                    base_url: url,
                };
            }
            _ => {}
        }
    }

    let config = Config {
        project: ProjectConfig { name },
        ai_account: ai_accounts,
        ollama,
    };

    write_config_file(dir, &config)?;
    let tasks_existed = dir.join("tasks.toml").exists();
    write_starter_tasks(dir)?;

    println!("\n{} Created .conduit/config.toml", "✓".green());
    if !tasks_existed {
        println!("{} Created tasks.toml (starter template)", "✓".green());
    }
    println!("\nRun {} to get started.", "`conduit validate`".cyan());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use conduit_core::config::{AIAccount, Config, OllamaConfig, ProjectConfig};
    use tempfile::tempdir;

    #[test]
    fn test_write_config_creates_file() {
        let dir = tempdir().unwrap();
        let config = Config {
            project: ProjectConfig { name: "test".to_string() },
            ai_account: vec![AIAccount {
                provider: "claude".to_string(),
                api_key: "sk-test".to_string(),
                daily_limit_usd: Some(10.0),
            }],
            ollama: OllamaConfig::default(),
        };
        write_config_file(dir.path(), &config).unwrap();
        let written = std::fs::read_to_string(
            dir.path().join(".conduit").join("config.toml"),
        ).unwrap();
        assert!(written.contains("claude"));
        assert!(written.contains("sk-test"));
    }

    #[test]
    fn test_write_config_creates_conduit_dir() {
        let dir = tempdir().unwrap();
        let config = Config {
            project: ProjectConfig { name: "test".to_string() },
            ai_account: vec![],
            ollama: OllamaConfig::default(),
        };
        write_config_file(dir.path(), &config).unwrap();
        assert!(dir.path().join(".conduit").is_dir());
    }

    #[test]
    fn test_write_starter_tasks_skips_if_exists() {
        let dir = tempdir().unwrap();
        let tasks_path = dir.path().join("tasks.toml");
        std::fs::write(&tasks_path, "existing content").unwrap();
        write_starter_tasks(dir.path()).unwrap();
        let content = std::fs::read_to_string(&tasks_path).unwrap();
        assert_eq!(content, "existing content");
    }

    #[test]
    fn test_init_fails_if_config_exists_without_force() {
        let dir = tempdir().unwrap();
        let conduit_dir = dir.path().join(".conduit");
        std::fs::create_dir_all(&conduit_dir).unwrap();
        std::fs::write(conduit_dir.join("config.toml"), "existing").unwrap();
        let err = check_no_existing_config(dir.path(), false).unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn test_init_succeeds_with_force() {
        let dir = tempdir().unwrap();
        let conduit_dir = dir.path().join(".conduit");
        std::fs::create_dir_all(&conduit_dir).unwrap();
        std::fs::write(conduit_dir.join("config.toml"), "existing").unwrap();
        check_no_existing_config(dir.path(), true).unwrap();
    }
}
