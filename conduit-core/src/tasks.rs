use serde::Deserialize;
use std::path::Path;
use crate::error::ConduitError;

#[derive(Debug, Deserialize, Clone)]
struct TasksFile {
    task: Vec<Task>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Task {
    pub id: String,
    pub description: String,
    pub options: Option<toml::Value>,
}

pub fn load_tasks(dir: &Path) -> Result<Vec<Task>, ConduitError> {
    let path = dir.join("tasks.toml");
    if !path.exists() {
        return Err(ConduitError::TasksNotFound);
    }
    let content = std::fs::read_to_string(&path)?;
    let file: TasksFile = toml::from_str(&content)?;
    let mut seen = std::collections::HashSet::new();
    for task in &file.task {
        if !seen.insert(&task.id) {
            return Err(ConduitError::DuplicateTaskId(task.id.clone()));
        }
    }
    Ok(file.task)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use std::fs;

    #[test]
    fn test_load_tasks_valid() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("tasks.toml"), r#"
[[task]]
id = "task-1"
description = "First task"

[[task]]
id = "task-2"
description = "Second task"
"#).unwrap();
        let tasks = load_tasks(dir.path()).unwrap();
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].id, "task-1");
        assert_eq!(tasks[1].description, "Second task");
    }

    #[test]
    fn test_load_tasks_not_found() {
        let dir = tempdir().unwrap();
        let err = load_tasks(dir.path()).unwrap_err();
        assert!(matches!(err, ConduitError::TasksNotFound));
    }

    #[test]
    fn test_load_tasks_with_options() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("tasks.toml"), r#"
[[task]]
id = "task-1"
description = "Task with options"

[task.options]
language = "rust"
output_dir = "src"
"#).unwrap();
        let tasks = load_tasks(dir.path()).unwrap();
        assert!(tasks[0].options.is_some());
    }

    #[test]
    fn test_load_tasks_bad_toml() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("tasks.toml"), "not valid [[[").unwrap();
        let err = load_tasks(dir.path()).unwrap_err();
        assert!(matches!(err, ConduitError::TasksParseError(_)));
    }

    #[test]
    fn test_load_tasks_missing_required_field() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("tasks.toml"), r#"
[[task]]
description = "Missing id field"
"#).unwrap();
        let err = load_tasks(dir.path()).unwrap_err();
        assert!(matches!(err, ConduitError::TasksParseError(_)));
    }

    #[test]
    fn test_load_tasks_duplicate_id() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("tasks.toml"), r#"
[[task]]
id = "task-1"
description = "First"

[[task]]
id = "task-1"
description = "Duplicate"
"#).unwrap();
        let err = load_tasks(dir.path()).unwrap_err();
        assert!(matches!(err, ConduitError::DuplicateTaskId(_)));
    }
}
