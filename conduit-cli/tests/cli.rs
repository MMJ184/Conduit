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
fn test_no_args_shows_help() {
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
fn test_run_all_tasks() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("tasks.toml"), r#"
[[task]]
id = "task-a"
description = "First task"

[[task]]
id = "task-b"
description = "Second task"
"#).unwrap();
    conduit()
        .arg("run")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("[queued]"))
        .stdout(predicates::str::contains("task-a"))
        .stdout(predicates::str::contains("task-b"));
}

#[test]
fn test_run_single_task() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("tasks.toml"), r#"
[[task]]
id = "task-a"
description = "First task"

[[task]]
id = "task-b"
description = "Second task"
"#).unwrap();
    conduit()
        .arg("run")
        .arg("--task")
        .arg("task-a")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("task-a"))
        .stdout(predicates::str::contains("task-b").not());
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
