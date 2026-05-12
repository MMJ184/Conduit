use anyhow::Result;
use colored::Colorize;
use conduit_core::config::load_global_config;

pub fn run() -> Result<()> {
    let config = load_global_config()?;
    println!("\nAI Accounts:");
    if config.ai_account.is_empty() {
        println!("  (none configured)");
    } else {
        for account in &config.ai_account {
            let limit = account
                .daily_limit_usd
                .map(|l| format!("${:.2}/day", l))
                .unwrap_or_else(|| "no limit".to_string());
            println!("  {} ({}) — {}", account.name.bold(), account.provider.cyan(), limit);
        }
    }
    let ollama_status = if config.ollama.enabled {
        "enabled".green().to_string()
    } else {
        "disabled".dimmed().to_string()
    };
    println!("\nOllama: {}", ollama_status);
    Ok(())
}
