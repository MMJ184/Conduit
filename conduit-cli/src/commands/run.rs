use anyhow::Result;
use colored::Colorize;
use conduit_core::{error::ConduitError, tasks::load_tasks};
use std::path::Path;

pub fn run(dir: &Path, task_id: Option<&str>) -> Result<()> {
    let mut tasks = load_tasks(dir)?;

    if let Some(id) = task_id {
        tasks.retain(|t| t.id == id);
        if tasks.is_empty() {
            return Err(ConduitError::TaskNotFound(id.to_string()).into());
        }
    }

    for task in &tasks {
        println!("{} {}: {}", "[queued]".yellow(), task.id.bold(), task.description);
    }
    Ok(())
}
