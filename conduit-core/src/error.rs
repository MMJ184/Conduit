use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConduitError {
    #[error("tasks.toml not found in current directory. Create one or run `conduit init`.")]
    TasksNotFound,
    #[error(".conduit/config.toml not found. Run `conduit init` to set up your project.")]
    ConfigNotFound,
    #[error("Failed to parse tasks.toml: {0}")]
    TasksParseError(#[from] toml::de::Error),
    #[error("Failed to read file: {0}")]
    Io(#[from] std::io::Error),
    #[error("Task `{0}` not found in tasks.toml")]
    TaskNotFound(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tasks_not_found_message() {
        let e = ConduitError::TasksNotFound;
        assert!(e.to_string().contains("tasks.toml not found"));
    }

    #[test]
    fn test_config_not_found_message() {
        let e = ConduitError::ConfigNotFound;
        assert!(e.to_string().contains("config.toml not found"));
    }

    #[test]
    fn test_task_not_found_message() {
        let e = ConduitError::TaskNotFound("my-task".to_string());
        assert!(e.to_string().contains("my-task"));
    }
}
