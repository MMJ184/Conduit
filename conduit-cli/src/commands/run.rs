use anyhow::Result;
use colored::Colorize;
use conduit_core::{
    config::load_config,
    error::ConduitError,
    pipeline::PipelineRunner,
    provider::select_provider,
    tasks::load_tasks,
};
use std::path::Path;

pub fn run(dir: &Path, task_id: Option<&str>) -> Result<()> {
    let mut tasks = load_tasks(dir)?;

    if let Some(id) = task_id {
        tasks.retain(|t| t.id == id);
        if tasks.is_empty() {
            return Err(ConduitError::TaskNotFound(id.to_string()).into());
        }
    }

    let config = load_config(dir)?;
    let provider = select_provider(&config)?;

    for task in &tasks {
        println!("{} {}", "[running]".cyan().bold(), task.id.bold());
        let runner = PipelineRunner::new(task, provider.as_ref(), dir);
        let result = runner.run(|completed, total, stage| {
            println!(
                "  [{}/{}] {}  {}",
                completed,
                total,
                stage.display_name(),
                "✓".green()
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
