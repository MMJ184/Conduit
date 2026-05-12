use anyhow::{bail, Result};
use colored::Colorize;
use conduit_core::config::{
    global_config_path, load_global_config, save_global_config, AIAccount,
};
use dialoguer::{Input, Select};
use std::process::Command;

const PROVIDER_BINARIES: &[(&str, &str, &[&str])] = &[
    ("claude", "claude", &["auth", "login"]),
    ("openai", "codex", &["login"]),
    ("gemini", "gemini", &["auth", "login"]),
];

pub fn list() -> Result<()> {
    let config = load_global_config()?;
    let path = global_config_path()?;
    println!("Global config: {}\n", path.display());

    println!("{}:", "Accounts".bold());
    if config.ai_account.is_empty() {
        println!("  (none configured)");
    } else {
        for account in &config.ai_account {
            let binary = provider_binary(&account.provider);
            let status = if which::which(binary).is_ok() {
                "✓ installed".green().to_string()
            } else {
                "✗ not found".red().to_string()
            };
            println!("  {}  ({})  {}", account.name.cyan(), account.provider, status);
        }
    }

    println!("\n{}:", "Profiles".bold());
    if config.profile.is_empty() {
        println!("  (none configured)");
    } else {
        for profile in &config.profile {
            if let Some(p) = &profile.provider {
                println!("  {}  (all stages: {})", profile.name.cyan(), p);
            } else {
                println!("  {}", profile.name.cyan());
            }
        }
    }
    Ok(())
}

pub fn add() -> Result<()> {
    let mut config = load_global_config().unwrap_or_default();

    let provider_labels = ["Claude (claude)", "OpenAI Codex (codex)", "Google Gemini (gemini)"];
    let idx = Select::new()
        .with_prompt("Which provider?")
        .items(&provider_labels)
        .default(0)
        .interact()?;

    let (provider_type, binary, login_args) = PROVIDER_BINARIES[idx];

    if which::which(binary).is_err() {
        bail!("{} CLI not found on PATH. Install it first.", binary);
    }

    println!("Opening {} login...", binary.cyan());
    let status = Command::new(binary)
        .args(login_args)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()?;

    if !status.success() {
        bail!("Login failed for {}.", binary);
    }

    let name = loop {
        let name: String = Input::new()
            .with_prompt("Account name (e.g. \"work\", \"personal\")")
            .interact_text()?;
        if name.is_empty() {
            println!("Name cannot be empty.");
            continue;
        }
        if config.ai_account.iter().any(|a| a.name == name) {
            println!("Account name '{}' already exists.", name);
            continue;
        }
        break name;
    };

    config.ai_account.push(AIAccount {
        name: name.clone(),
        provider: provider_type.to_string(),
        daily_limit_usd: None,
    });

    save_global_config(&config)?;
    println!("{} Account \"{}\" ({}) added.", "✓".green(), name, provider_type);
    Ok(())
}

pub fn remove(name: &str) -> Result<()> {
    let mut config = load_global_config()?;

    let pos = config.ai_account.iter().position(|a| a.name == name)
        .ok_or_else(|| anyhow::anyhow!("Account '{}' not found.", name))?;

    let referenced_by: Vec<&str> = config.profile.iter()
        .filter(|p| {
            p.provider.as_deref() == Some(name)
                || p.orchestrator.as_deref() == Some(name)
                || p.doc.as_deref() == Some(name)
                || p.architecture.as_deref() == Some(name)
                || p.code.as_deref() == Some(name)
                || p.test.as_deref() == Some(name)
        })
        .map(|p| p.name.as_str())
        .collect();

    if !referenced_by.is_empty() {
        bail!(
            "Account '{}' is used by profiles: {}. Remove those profiles first.",
            name,
            referenced_by.join(", ")
        );
    }

    config.ai_account.remove(pos);
    save_global_config(&config)?;
    println!("{} Account \"{}\" removed.", "✓".green(), name);
    Ok(())
}

pub fn login(name: &str) -> Result<()> {
    let config = load_global_config()?;

    let account = config.ai_account.iter().find(|a| a.name == name)
        .ok_or_else(|| anyhow::anyhow!("Account '{}' not found. Run `conduit providers list`.", name))?;

    let binary = provider_binary(&account.provider);

    if which::which(binary).is_err() {
        bail!("{} CLI not found on PATH.", binary);
    }

    let login_args = provider_login_args(&account.provider);
    println!("Opening {} login for account \"{}\"...", binary.cyan(), name);
    let status = Command::new(binary)
        .args(login_args)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()?;

    if !status.success() {
        bail!("Login failed for {}.", binary);
    }
    println!("{} Login complete for account \"{}\".", "✓".green(), name);
    Ok(())
}

fn provider_binary(provider_type: &str) -> &str {
    match provider_type {
        "openai" => "codex",
        "claude" => "claude",
        "gemini" => "gemini",
        _ => provider_type,
    }
}

fn provider_login_args(provider_type: &str) -> &'static [&'static str] {
    match provider_type {
        "openai" => &["login"],
        _ => &["auth", "login"],
    }
}
