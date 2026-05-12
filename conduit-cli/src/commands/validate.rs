use anyhow::Result;
use colored::Colorize;
use conduit_core::tasks::load_tasks;
use std::path::Path;

pub fn run(dir: &Path) -> Result<()> {
    let tasks = load_tasks(dir)?;
    println!("{} Found {} task(s):", "✓".green(), tasks.len());
    for task in &tasks {
        println!("  {} — {}", task.id.bold(), task.description);
    }
    Ok(())
}
