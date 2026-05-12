use anyhow::Result;
use colored::Colorize;
use conduit_core::{config::load_global_config, spend::SpendTracker};

pub fn run() -> Result<()> {
    let config = load_global_config()?;
    let spend = SpendTracker::load().unwrap_or_else(|_| SpendTracker::new_empty());

    println!("{}:", "AI Accounts".bold());
    if config.ai_account.is_empty() {
        println!("  (none configured)");
    } else {
        for account in &config.ai_account {
            let limit_str = account
                .daily_limit_usd
                .map(|l| format!("  ${:.2}/day", l))
                .unwrap_or_else(|| "  no limit".to_string());

            let spend_str = if account.cost_per_run.is_some() {
                let est = spend.estimated_spend(account);
                format!("  ~${:.2} used today", est)
            } else if account.daily_limit_usd.is_some() {
                "  cost tracking not configured".to_string()
            } else {
                String::new()
            };

            let switch_str = match account.auto_switch {
                Some(true) => {
                    let trigger = account.switch_on.as_deref().unwrap_or("error");
                    format!("  auto-switch: {}", trigger)
                }
                _ => "  auto-switch: off".to_string(),
            };

            println!(
                "  {}  ({}){}{}{}",
                account.name.cyan(),
                account.provider,
                limit_str,
                spend_str,
                switch_str,
            );
        }
    }

    println!("\n{}:", "Profiles".bold());
    if config.profile.is_empty() {
        println!("  (none configured)");
    } else {
        for profile in &config.profile {
            println!("  {}", profile.name.cyan());
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
