use assert_cmd::Command;

use std::fs;
use std::path::Path;
use tempfile::tempdir;

/// Initialize a minimal git repo so that git worktree commands will work.
fn init_git_repo(dir: &Path) {
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(dir)
        .output()
        .expect("git init failed");
    std::process::Command::new("git")
        .args(["config", "user.email", "test@conduit.local"])
        .current_dir(dir)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(dir)
        .output()
        .unwrap();
    fs::write(dir.join("README.md"), "init").unwrap();
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(dir)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(dir)
        .output()
        .unwrap();
}

fn conduit() -> Command {
    Command::cargo_bin("conduit").unwrap()
}

// Helper: write global config to a temp file and return (TempDir, path string)
// Caller must keep TempDir alive for the duration of the test
fn write_global_config(content: &str) -> (tempfile::TempDir, String) {
    let dir = tempdir().unwrap();
    let path = dir.path().join("global.toml");
    fs::write(&path, content).unwrap();
    let path_str = path.to_str().unwrap().to_string();
    (dir, path_str)
}

fn nonexistent_config_path() -> String {
    std::env::temp_dir()
        .join("nonexistent_conduit_xyz_abc123")
        .join("config.toml")
        .to_str()
        .unwrap()
        .to_string()
}

// --- help / basic ---

#[test]
fn test_help_exits_zero() {
    conduit().arg("--help").assert().success();
}

#[test]
fn test_no_args_fails() {
    conduit().assert().failure();
}

// --- validate ---

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

// --- run ---

#[test]
fn test_run_missing_tasks_file() {
    let dir = tempdir().unwrap();
    let (_cfg_dir, cfg_path) = write_global_config("");
    conduit()
        .arg("run")
        .current_dir(dir.path())
        .env("CONDUIT_GLOBAL_CONFIG", &cfg_path)
        .assert()
        .failure()
        .stderr(predicates::str::contains("tasks.toml not found"));
}

#[test]
fn test_run_unknown_task_id() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("tasks.toml"), r#"
[[task]]
id = "task-a"
description = "First task"
"#).unwrap();
    let (_cfg_dir, cfg_path) = write_global_config("");
    conduit()
        .arg("run")
        .arg("--task")
        .arg("nonexistent")
        .current_dir(dir.path())
        .env("CONDUIT_GLOBAL_CONFIG", &cfg_path)
        .assert()
        .failure()
        .stderr(predicates::str::contains("nonexistent"));
}

#[test]
fn test_run_requires_global_config() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("tasks.toml"), r#"
[[task]]
id = "task-a"
description = "First task"
"#).unwrap();
    conduit()
        .arg("run")
        .arg("--profile")
        .arg("anything")
        .current_dir(dir.path())
        .env("CONDUIT_GLOBAL_CONFIG", nonexistent_config_path())
        .assert()
        .failure()
        .stderr(predicates::str::contains("config.toml not found"));
}

#[test]
fn test_run_no_providers_configured() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("tasks.toml"), r#"
[[task]]
id = "task-a"
description = "First task"
"#).unwrap();
    let (_cfg_dir, cfg_path) = write_global_config(""); // empty config = no accounts
    conduit()
        .arg("run")
        .arg("--profile")
        .arg("anything")
        .current_dir(dir.path())
        .env("CONDUIT_GLOBAL_CONFIG", &cfg_path)
        .assert()
        .failure()
        .stderr(predicates::str::contains("No providers configured"));
}

#[test]
fn test_run_profile_not_found() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("tasks.toml"), r#"
[[task]]
id = "task-a"
description = "First task"
"#).unwrap();
    let (_cfg_dir, cfg_path) = write_global_config(r#"
[[ai_account]]
name = "claude-work"
provider = "claude"
"#);
    conduit()
        .arg("run")
        .arg("--profile")
        .arg("nonexistent-profile")
        .current_dir(dir.path())
        .env("CONDUIT_GLOBAL_CONFIG", &cfg_path)
        .assert()
        .failure()
        .stderr(predicates::str::contains("nonexistent-profile"));
}

#[test]
fn test_run_no_provider_available() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("tasks.toml"), r#"
[[task]]
id = "task-a"
description = "First task"
"#).unwrap();
    let (_cfg_dir, cfg_path) = write_global_config(r#"
[[ai_account]]
name = "fake-account"
provider = "nonexistent-ai-xyz-12345"

[[profile]]
name = "fake-profile"
provider = "fake-account"
"#);
    conduit()
        .arg("run")
        .arg("--profile")
        .arg("fake-profile")
        .current_dir(dir.path())
        .env("CONDUIT_GLOBAL_CONFIG", &cfg_path)
        .assert()
        .failure()
        .stderr(predicates::str::contains("No AI provider available"));
}

// --- status ---

#[test]
fn test_status_no_config() {
    conduit()
        .arg("status")
        .env("CONDUIT_GLOBAL_CONFIG", nonexistent_config_path())
        .assert()
        .failure()
        .stderr(predicates::str::contains("config.toml not found"));
}

#[test]
fn test_status_shows_accounts() {
    let (_cfg_dir, cfg_path) = write_global_config(r#"
[[ai_account]]
name = "claude-work"
provider = "claude"
daily_limit_usd = 10.0
"#);
    conduit()
        .arg("status")
        .env("CONDUIT_GLOBAL_CONFIG", &cfg_path)
        .assert()
        .success()
        .stdout(predicates::str::contains("claude-work"))
        .stdout(predicates::str::contains("$10.00"));
}

#[test]
fn test_status_no_accounts() {
    let (_cfg_dir, cfg_path) = write_global_config("");
    conduit()
        .arg("status")
        .env("CONDUIT_GLOBAL_CONFIG", &cfg_path)
        .assert()
        .success()
        .stdout(predicates::str::contains("none configured"));
}

// --- providers ---

#[test]
fn test_providers_list_no_config() {
    conduit()
        .arg("providers")
        .arg("list")
        .env("CONDUIT_GLOBAL_CONFIG", nonexistent_config_path())
        .assert()
        .failure()
        .stderr(predicates::str::contains("config.toml not found"));
}

#[test]
fn test_providers_list_shows_accounts_and_profiles() {
    let (_cfg_dir, cfg_path) = write_global_config(r#"
[[ai_account]]
name = "claude-work"
provider = "claude"

[[profile]]
name = "all-claude"
provider = "claude-work"
"#);
    conduit()
        .arg("providers")
        .arg("list")
        .env("CONDUIT_GLOBAL_CONFIG", &cfg_path)
        .assert()
        .success()
        .stdout(predicates::str::contains("claude-work"))
        .stdout(predicates::str::contains("all-claude"));
}

// --- parallel / concurrency ---

#[test]
fn test_run_concurrency_zero_rejected() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("tasks.toml"), r#"
[[task]]
id = "task-a"
description = "First task"
"#).unwrap();
    let (_cfg_dir, cfg_path) = write_global_config(r#"
[[ai_account]]
name = "fake-account"
provider = "nonexistent-ai-xyz-12345"

[[profile]]
name = "fake-profile"
provider = "fake-account"
"#);
    conduit()
        .arg("run")
        .arg("--profile").arg("fake-profile")
        .arg("--concurrency").arg("0")
        .current_dir(dir.path())
        .env("CONDUIT_GLOBAL_CONFIG", &cfg_path)
        .assert()
        .failure()
        .stderr(predicates::str::contains("--concurrency must be at least 1"));
}

#[test]
fn test_run_concurrency_2_two_tasks_both_fail() {
    let dir = tempdir().unwrap();
    // Parallel mode requires a git repo (worktree isolation); init one here.
    init_git_repo(dir.path());
    fs::write(dir.path().join("tasks.toml"), r#"
[[task]]
id = "task-a"
description = "First task"

[[task]]
id = "task-b"
description = "Second task"
"#).unwrap();
    let (_cfg_dir, cfg_path) = write_global_config(r#"
[[ai_account]]
name = "fake-account"
provider = "nonexistent-ai-xyz-12345"

[[profile]]
name = "fake-profile"
provider = "fake-account"
"#);
    conduit()
        .arg("run")
        .arg("--profile").arg("fake-profile")
        .arg("--concurrency").arg("2")
        .current_dir(dir.path())
        .env("CONDUIT_GLOBAL_CONFIG", &cfg_path)
        .assert()
        .failure()
        .stderr(predicates::str::contains("No AI provider available"))
        .stdout(predicates::str::contains("[task-a] running..."));
}

#[test]
fn test_run_concurrency_1_two_tasks_sequential() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("tasks.toml"), r#"
[[task]]
id = "task-a"
description = "First task"

[[task]]
id = "task-b"
description = "Second task"
"#).unwrap();
    let (_cfg_dir, cfg_path) = write_global_config(r#"
[[ai_account]]
name = "fake-account"
provider = "nonexistent-ai-xyz-12345"

[[profile]]
name = "fake-profile"
provider = "fake-account"
"#);
    conduit()
        .arg("run")
        .arg("--profile").arg("fake-profile")
        .arg("--concurrency").arg("1")
        .current_dir(dir.path())
        .env("CONDUIT_GLOBAL_CONFIG", &cfg_path)
        .assert()
        .failure()
        .stderr(predicates::str::contains("No AI provider available"))
        .stdout(predicates::str::contains("[running]"));
}

// --- run --account ---

#[test]
fn test_run_account_not_found_fails_before_loading_tasks() {
    let dir = tempdir().unwrap();
    // No tasks.toml — proves we fail before loading tasks
    let (_cfg_dir, cfg_path) = write_global_config(r#"
[[ai_account]]
name = "real-account"
provider = "claude"

[[profile]]
name = "p"
provider = "real-account"
"#);
    conduit()
        .arg("run")
        .arg("--account")
        .arg("nonexistent-account")
        .arg("--profile")
        .arg("p")
        .current_dir(dir.path())
        .env("CONDUIT_GLOBAL_CONFIG", &cfg_path)
        .assert()
        .failure()
        .stderr(predicates::str::contains("nonexistent-account"))
        .stderr(predicates::str::contains("not found"));
}

#[test]
fn test_run_account_flag_validates_before_task_loading() {
    // Same account-not-found error even when tasks.toml is missing (proves early exit)
    let dir = tempdir().unwrap();
    let (_cfg_dir, cfg_path) = write_global_config(r#"
[[ai_account]]
name = "claude-work"
provider = "claude"
"#);
    conduit()
        .arg("run")
        .arg("--account")
        .arg("does-not-exist")
        .current_dir(dir.path())
        .env("CONDUIT_GLOBAL_CONFIG", &cfg_path)
        .assert()
        .failure()
        .stderr(predicates::str::contains("does-not-exist"));
}

#[test]
fn test_status_shows_auto_switch_field() {
    let (_cfg_dir, cfg_path) = write_global_config(r#"
[[ai_account]]
name = "claude-work"
provider = "claude"
daily_limit_usd = 10.0
auto_switch = true
switch_on = "both"
cost_per_run = 0.05
"#);
    conduit()
        .arg("status")
        .env("CONDUIT_GLOBAL_CONFIG", &cfg_path)
        .assert()
        .success()
        .stdout(predicates::str::contains("claude-work"))
        .stdout(predicates::str::contains("auto-switch: both"))
        .stdout(predicates::str::contains("$10.00/day"));
}
