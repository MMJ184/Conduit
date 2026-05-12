use assert_cmd::Command;
use predicates::prelude::*;

fn conduit() -> Command {
    Command::cargo_bin("conduit").unwrap()
}

#[test]
fn test_help_exits_zero() {
    conduit().arg("--help").assert().success();
}

#[test]
fn test_no_args_fails() {
    conduit().assert().failure();
}

use tempfile::tempdir;
use std::fs;

#[test]
fn test_validate_valid_tasks() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("tasks.toml"), r#"
[[task]]
id = "test-task"
description = "A test task"
"#).unwrap();
    conduit()
        .arg("validate")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("test-task"));
}

#[test]
fn test_validate_missing_tasks_file() {
    let dir = tempdir().unwrap();
    conduit()
        .arg("validate")
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicates::str::contains("tasks.toml not found"));
}

#[test]
fn test_validate_bad_toml() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("tasks.toml"), "[[[ invalid").unwrap();
    conduit()
        .arg("validate")
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicates::str::contains("Failed to parse"));
}

#[test]
fn test_run_requires_config() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("tasks.toml"), r#"
[[task]]
id = "task-a"
description = "First task"
"#).unwrap();
    conduit()
        .arg("run")
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicates::str::contains("config.toml not found"));
}

#[test]
fn test_run_no_provider_available() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("tasks.toml"), r#"
[[task]]
id = "task-a"
description = "First task"
"#).unwrap();
    write_config(dir.path(), r#"
[project]
name = "test"

[[ai_account]]
provider = "nonexistent-ai-xyz"
api_key = "sk-test"
"#);
    conduit()
        .arg("run")
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicates::str::contains("No AI provider available"));
}

#[test]
fn test_run_unknown_task_id() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("tasks.toml"), r#"
[[task]]
id = "task-a"
description = "First task"
"#).unwrap();
    conduit()
        .arg("run")
        .arg("--task")
        .arg("nonexistent")
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicates::str::contains("nonexistent"));
}

#[test]
fn test_run_missing_tasks_file() {
    let dir = tempdir().unwrap();
    conduit()
        .arg("run")
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicates::str::contains("tasks.toml not found"));
}

fn write_config(dir: &std::path::Path, content: &str) {
    let conduit_dir = dir.join(".conduit");
    fs::create_dir_all(&conduit_dir).unwrap();
    fs::write(conduit_dir.join("config.toml"), content).unwrap();
}

#[test]
fn test_status_with_config() {
    let dir = tempdir().unwrap();
    write_config(dir.path(), r#"
[project]
name = "test-project"

[[ai_account]]
provider = "claude"
api_key = "sk-test"
daily_limit_usd = 10.0
"#);
    conduit()
        .arg("status")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("test-project"))
        .stdout(predicates::str::contains("claude"))
        .stdout(predicates::str::contains("$10.00"));
}

#[test]
fn test_status_no_config() {
    let dir = tempdir().unwrap();
    conduit()
        .arg("status")
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicates::str::contains("config.toml not found"));
}

#[test]
fn test_status_no_accounts() {
    let dir = tempdir().unwrap();
    write_config(dir.path(), r#"
[project]
name = "empty-project"
"#);
    conduit()
        .arg("status")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("none configured"));
}
