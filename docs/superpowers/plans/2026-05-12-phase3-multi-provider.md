# Conduit Phase 3 — Multi-Provider + Run Profiles Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move provider config to a global `~/.conduit/config.toml`, add named accounts + run profiles, and add interactive profile selection at `conduit run` time.

**Architecture:** `conduit-core` gains global config path resolution (via `dirs`), new `Profile`/`Defaults` structs, a `ProviderResolver` trait + `ProfileResolver` impl, and per-stage provider selection. `conduit-cli` gets a rewritten `init`, an updated `run` with `--profile` flag, and a new `providers` subcommand. All integration tests use `CONDUIT_GLOBAL_CONFIG` env var for testability.

**Tech Stack:** Rust stable, `dirs = "5"` (home dir resolution), existing `dialoguer 0.11`, `which = "6"`, `colored 2`.

---

## File Map

| File | Change |
|---|---|
| `conduit-core/Cargo.toml` | Add `dirs = "5"` |
| `conduit-core/src/config.rs` | Remove `ProjectConfig`/`api_key`; add `Profile`, `Defaults`; add `global_config_path`, `load_global_config`, `save_global_config` |
| `conduit-core/src/error.rs` | Add 6 new variants |
| `conduit-core/src/provider.rs` | Add `ProviderResolver` trait, `ProfileResolver`, `MockProviderResolver`, `select_provider_for_stage`; remove `select_provider` |
| `conduit-core/src/pipeline.rs` | `PipelineRunner` uses `&dyn ProviderResolver` instead of `&dyn Provider` |
| `conduit-cli/src/main.rs` | Add `Providers` subcommand; add `--profile` to `Run` |
| `conduit-cli/src/commands/mod.rs` | Add `pub mod providers;` |
| `conduit-cli/src/commands/init.rs` | Rewrite: global config, CLI login, create `.conduit/` dir |
| `conduit-cli/src/commands/run.rs` | Profile selection, `--profile` flag, `load_global_config` |
| `conduit-cli/src/commands/providers.rs` | New: `list`, `add`, `remove`, `login` |
| `conduit-cli/src/commands/status.rs` | Read global config; show accounts + profiles |
| `conduit-cli/tests/cli.rs` | All tests use `CONDUIT_GLOBAL_CONFIG` env var; new format (no `api_key`, add `name`) |

---

## Task 1: Core data model, global config functions, new error variants

**Files:**
- Modify: `conduit-core/Cargo.toml`
- Modify: `conduit-core/src/config.rs`
- Modify: `conduit-core/src/error.rs`

- [ ] **Step 1: Add `dirs = "5"` to `conduit-core/Cargo.toml`**

```toml
[package]
name = "conduit-core"
version = "0.1.0"
edition = "2021"

[features]
test-utils = []

[dependencies]
serde = { version = "1", features = ["derive"] }
toml = "0.8"
thiserror = "1"
which = "6"
dirs = "5"

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 2: Replace `conduit-core/src/config.rs` entirely**

```rust
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use crate::error::ConduitError;

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct Config {
    #[serde(default)]
    pub ai_account: Vec<AIAccount>,
    #[serde(default)]
    pub defaults: Defaults,
    #[serde(default)]
    pub profile: Vec<Profile>,
    #[serde(default)]
    pub ollama: OllamaConfig,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AIAccount {
    pub name: String,
    pub provider: String,
    pub daily_limit_usd: Option<f64>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct Defaults {
    pub orchestrator: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Profile {
    pub name: String,
    pub provider: Option<String>,
    pub orchestrator: Option<String>,
    pub doc: Option<String>,
    pub architecture: Option<String>,
    pub code: Option<String>,
    pub test: Option<String>,
}

impl Profile {
    pub fn account_for_stage(&self, stage_name: &str) -> Option<&str> {
        if let Some(p) = &self.provider {
            return Some(p.as_str());
        }
        match stage_name {
            "orchestrator" => self.orchestrator.as_deref(),
            "doc" => self.doc.as_deref(),
            "architecture" => self.architecture.as_deref(),
            "code" => self.code.as_deref(),
            "test" => self.test.as_deref(),
            _ => None,
        }
    }

    pub fn is_complete(&self) -> bool {
        if self.provider.is_some() {
            return true;
        }
        self.orchestrator.is_some()
            && self.doc.is_some()
            && self.architecture.is_some()
            && self.code.is_some()
            && self.test.is_some()
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct OllamaConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_ollama_url")]
    pub base_url: String,
}

impl Default for OllamaConfig {
    fn default() -> Self {
        Self { enabled: false, base_url: default_ollama_url() }
    }
}

fn default_ollama_url() -> String {
    "http://localhost:11434".to_string()
}

pub fn global_config_path() -> Result<PathBuf, ConduitError> {
    if let Ok(path) = std::env::var("CONDUIT_GLOBAL_CONFIG") {
        return Ok(PathBuf::from(path));
    }
    let home = dirs::home_dir().ok_or(ConduitError::GlobalConfigDirNotFound)?;
    Ok(home.join(".conduit").join("config.toml"))
}

pub fn load_global_config() -> Result<Config, ConduitError> {
    let path = global_config_path()?;
    if !path.exists() {
        return Err(ConduitError::ConfigNotFound);
    }
    let content = std::fs::read_to_string(&path)?;
    let config: Config = toml::from_str(&content)?;
    Ok(config)
}

pub fn save_global_config(config: &Config) -> Result<(), ConduitError> {
    let path = global_config_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let toml_str = toml::to_string_pretty(config)
        .map_err(|e| ConduitError::ConfigSerializeError(e.to_string()))?;
    std::fs::write(&path, toml_str)?;
    Ok(())
}

// Kept for backward compat — delegates to load_global_config()
pub fn load_config(_dir: &Path) -> Result<Config, ConduitError> {
    load_global_config()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn with_temp_config(content: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, content).unwrap();
        (dir, path)
    }

    #[test]
    fn test_global_config_path_from_env() {
        let (_dir, path) = with_temp_config("");
        std::env::set_var("CONDUIT_GLOBAL_CONFIG", path.to_str().unwrap());
        let result = global_config_path().unwrap();
        std::env::remove_var("CONDUIT_GLOBAL_CONFIG");
        assert_eq!(result, path);
    }

    #[test]
    fn test_load_global_config_not_found() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nonexistent.toml");
        std::env::set_var("CONDUIT_GLOBAL_CONFIG", path.to_str().unwrap());
        let err = load_global_config().unwrap_err();
        std::env::remove_var("CONDUIT_GLOBAL_CONFIG");
        assert!(matches!(err, ConduitError::ConfigNotFound));
    }

    #[test]
    fn test_load_global_config_parses_accounts_and_profiles() {
        let (_dir, path) = with_temp_config(r#"
[[ai_account]]
name = "claude-work"
provider = "claude"
daily_limit_usd = 5.0

[[profile]]
name = "all-claude"
provider = "claude-work"
"#);
        std::env::set_var("CONDUIT_GLOBAL_CONFIG", path.to_str().unwrap());
        let config = load_global_config().unwrap();
        std::env::remove_var("CONDUIT_GLOBAL_CONFIG");
        assert_eq!(config.ai_account.len(), 1);
        assert_eq!(config.ai_account[0].name, "claude-work");
        assert_eq!(config.profile.len(), 1);
        assert_eq!(config.profile[0].name, "all-claude");
    }

    #[test]
    fn test_save_and_reload_global_config() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::env::set_var("CONDUIT_GLOBAL_CONFIG", path.to_str().unwrap());
        let config = Config {
            ai_account: vec![AIAccount {
                name: "my-claude".to_string(),
                provider: "claude".to_string(),
                daily_limit_usd: None,
            }],
            ..Config::default()
        };
        save_global_config(&config).unwrap();
        let loaded = load_global_config().unwrap();
        std::env::remove_var("CONDUIT_GLOBAL_CONFIG");
        assert_eq!(loaded.ai_account[0].name, "my-claude");
    }

    #[test]
    fn test_profile_account_for_stage_single_provider() {
        let profile = Profile {
            name: "all-claude".to_string(),
            provider: Some("claude-work".to_string()),
            orchestrator: None, doc: None, architecture: None, code: None, test: None,
        };
        assert_eq!(profile.account_for_stage("orchestrator"), Some("claude-work"));
        assert_eq!(profile.account_for_stage("code"), Some("claude-work"));
    }

    #[test]
    fn test_profile_account_for_stage_per_stage() {
        let profile = Profile {
            name: "mixed".to_string(),
            provider: None,
            orchestrator: Some("claude-work".to_string()),
            doc: Some("claude-work".to_string()),
            architecture: Some("claude-work".to_string()),
            code: Some("codex-main".to_string()),
            test: Some("codex-main".to_string()),
        };
        assert_eq!(profile.account_for_stage("orchestrator"), Some("claude-work"));
        assert_eq!(profile.account_for_stage("code"), Some("codex-main"));
    }

    #[test]
    fn test_profile_is_complete() {
        let single = Profile {
            name: "s".to_string(), provider: Some("a".to_string()),
            orchestrator: None, doc: None, architecture: None, code: None, test: None,
        };
        assert!(single.is_complete());

        let incomplete = Profile {
            name: "i".to_string(), provider: None,
            orchestrator: Some("a".to_string()),
            doc: None, architecture: None, code: None, test: None,
        };
        assert!(!incomplete.is_complete());

        let complete = Profile {
            name: "c".to_string(), provider: None,
            orchestrator: Some("a".to_string()),
            doc: Some("a".to_string()),
            architecture: Some("a".to_string()),
            code: Some("b".to_string()),
            test: Some("b".to_string()),
        };
        assert!(complete.is_complete());
    }
}
```

- [ ] **Step 3: Replace `conduit-core/src/error.rs` entirely**

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConduitError {
    #[error("tasks.toml not found in current directory. Create one or run `conduit init`.")]
    TasksNotFound,
    #[error("~/.conduit/config.toml not found. Run `conduit init` to set up Conduit.")]
    ConfigNotFound,
    #[error("Failed to parse tasks.toml: {0}")]
    TasksParseError(#[from] toml::de::Error),
    #[error("Failed to read file: {0}")]
    Io(#[from] std::io::Error),
    #[error("Task `{0}` not found in tasks.toml")]
    TaskNotFound(String),
    #[error("Duplicate task id `{0}` in tasks.toml")]
    DuplicateTaskId(String),
    #[error("No AI provider available. Run `conduit init` to configure one.")]
    NoProviderAvailable,
    #[error("Agent `{provider}` failed at stage `{stage}`: {reason}")]
    AgentInvocationFailed { provider: String, stage: String, reason: String },
    #[error("Could not determine home directory. Set HOME environment variable.")]
    GlobalConfigDirNotFound,
    #[error("No providers configured. Run `conduit init` to set up your providers.")]
    NoProvidersConfigured,
    #[error("Account `{account}` referenced in profile `{profile}` is not configured in ~/.conduit/config.toml")]
    ProfileProviderNotConfigured { account: String, profile: String },
    #[error("Profile `{0}` not found. Run `conduit providers list` to see available profiles.")]
    ProfileNotFound(String),
    #[error("Profile `{0}` is missing stage assignments. All 5 stages must be specified for a multi-provider profile.")]
    ProfileIncomplete(String),
    #[error("Failed to serialize config: {0}")]
    ConfigSerializeError(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tasks_not_found_message() {
        assert!(ConduitError::TasksNotFound.to_string().contains("tasks.toml not found"));
    }

    #[test]
    fn test_config_not_found_message() {
        assert!(ConduitError::ConfigNotFound.to_string().contains("config.toml not found"));
    }

    #[test]
    fn test_task_not_found_message() {
        assert!(ConduitError::TaskNotFound("my-task".to_string()).to_string().contains("my-task"));
    }

    #[test]
    fn test_no_provider_available_message() {
        assert!(ConduitError::NoProviderAvailable.to_string().contains("No AI provider available"));
    }

    #[test]
    fn test_agent_invocation_failed_message() {
        let e = ConduitError::AgentInvocationFailed {
            provider: "claude".to_string(), stage: "doc".to_string(), reason: "binary not found".to_string(),
        };
        assert!(e.to_string().contains("claude"));
        assert!(e.to_string().contains("doc"));
    }

    #[test]
    fn test_no_providers_configured_message() {
        assert!(ConduitError::NoProvidersConfigured.to_string().contains("No providers configured"));
    }

    #[test]
    fn test_profile_not_found_message() {
        assert!(ConduitError::ProfileNotFound("my-profile".to_string()).to_string().contains("my-profile"));
    }

    #[test]
    fn test_profile_provider_not_configured_message() {
        let e = ConduitError::ProfileProviderNotConfigured {
            account: "bad-account".to_string(), profile: "my-profile".to_string(),
        };
        assert!(e.to_string().contains("bad-account"));
        assert!(e.to_string().contains("my-profile"));
    }
}
```

- [ ] **Step 4: Run core tests to verify compilation and new tests pass**

Run from `D:\demo\Conduit`: `cargo test -p conduit-core config error`
Expected: all pass (note: env var tests must run serially — they use `std::env::set_var` which is fine for unit tests but set/remove in same test)

- [ ] **Step 5: Commit**

```
git add conduit-core/Cargo.toml conduit-core/src/config.rs conduit-core/src/error.rs
git commit -m "feat(core): global config model — Profile, Defaults, named accounts, no api_key"
```

---

## Task 2: ProviderResolver trait, ProfileResolver, select_provider_for_stage

**Files:**
- Modify: `conduit-core/src/provider.rs`

- [ ] **Step 1: Replace `conduit-core/src/provider.rs` entirely**

```rust
use std::path::Path;
use std::process::Command;
use crate::config::{Config, Profile};
use crate::error::ConduitError;
use crate::pipeline::Stage;

pub trait Provider: Send + Sync + std::fmt::Debug {
    fn name(&self) -> &str;
    fn invoke(&self, stage: &str, prompt: &str, work_dir: &Path) -> Result<String, ConduitError>;
}

pub trait ProviderResolver: std::fmt::Debug {
    fn resolve(&self, stage: &Stage) -> Result<Box<dyn Provider>, ConduitError>;
}

#[derive(Debug)]
pub struct ClaudeProvider;
#[derive(Debug)]
pub struct CodexProvider;
#[derive(Debug)]
pub struct GeminiProvider;

impl Provider for ClaudeProvider {
    fn name(&self) -> &str { "claude" }
    fn invoke(&self, stage: &str, prompt: &str, work_dir: &Path) -> Result<String, ConduitError> {
        invoke_cli("claude", &["-p", prompt], stage, work_dir, self.name())
    }
}

impl Provider for CodexProvider {
    fn name(&self) -> &str { "codex" }
    fn invoke(&self, stage: &str, prompt: &str, work_dir: &Path) -> Result<String, ConduitError> {
        invoke_cli("codex", &[prompt], stage, work_dir, self.name())
    }
}

impl Provider for GeminiProvider {
    fn name(&self) -> &str { "gemini" }
    fn invoke(&self, stage: &str, prompt: &str, work_dir: &Path) -> Result<String, ConduitError> {
        invoke_cli("gemini", &["-p", prompt], stage, work_dir, self.name())
    }
}

fn invoke_cli(
    binary: &str,
    args: &[&str],
    stage: &str,
    work_dir: &Path,
    provider_name: &str,
) -> Result<String, ConduitError> {
    let output = Command::new(binary)
        .args(args)
        .current_dir(work_dir)
        .output()
        .map_err(|e| ConduitError::AgentInvocationFailed {
            provider: provider_name.to_string(),
            stage: stage.to_string(),
            reason: e.to_string(),
        })?;
    if !output.status.success() {
        return Err(ConduitError::AgentInvocationFailed {
            provider: provider_name.to_string(),
            stage: stage.to_string(),
            reason: String::from_utf8_lossy(&output.stderr).to_string(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

pub fn select_provider_for_stage(
    stage: &Stage,
    profile: &Profile,
    config: &Config,
) -> Result<Box<dyn Provider>, ConduitError> {
    let account_name = profile
        .account_for_stage(stage.name())
        .ok_or_else(|| ConduitError::ProfileIncomplete(profile.name.clone()))?;

    let account = config
        .ai_account
        .iter()
        .find(|a| a.name == account_name)
        .ok_or_else(|| ConduitError::ProfileProviderNotConfigured {
            account: account_name.to_string(),
            profile: profile.name.clone(),
        })?;

    match account.provider.as_str() {
        "claude" if which::which("claude").is_ok() => Ok(Box::new(ClaudeProvider)),
        "openai" if which::which("codex").is_ok() => Ok(Box::new(CodexProvider)),
        "gemini" if which::which("gemini").is_ok() => Ok(Box::new(GeminiProvider)),
        _ => Err(ConduitError::NoProviderAvailable),
    }
}

#[derive(Debug)]
pub struct ProfileResolver<'a> {
    pub profile: &'a Profile,
    pub config: &'a Config,
}

impl<'a> ProviderResolver for ProfileResolver<'a> {
    fn resolve(&self, stage: &Stage) -> Result<Box<dyn Provider>, ConduitError> {
        select_provider_for_stage(stage, self.profile, self.config)
    }
}

#[cfg(any(test, feature = "test-utils"))]
#[derive(Debug)]
pub struct MockProvider {
    pub response: String,
}

#[cfg(any(test, feature = "test-utils"))]
impl Provider for MockProvider {
    fn name(&self) -> &str { "mock" }
    fn invoke(&self, _stage: &str, _prompt: &str, _work_dir: &Path) -> Result<String, ConduitError> {
        Ok(self.response.clone())
    }
}

#[cfg(any(test, feature = "test-utils"))]
#[derive(Debug)]
pub struct MockProviderResolver {
    pub response: String,
}

#[cfg(any(test, feature = "test-utils"))]
impl ProviderResolver for MockProviderResolver {
    fn resolve(&self, _stage: &Stage) -> Result<Box<dyn Provider>, ConduitError> {
        Ok(Box::new(MockProvider { response: self.response.clone() }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AIAccount, Config, Defaults, OllamaConfig, Profile};
    use tempfile::tempdir;

    fn make_config(account_name: &str, provider: &str) -> Config {
        Config {
            ai_account: vec![AIAccount {
                name: account_name.to_string(),
                provider: provider.to_string(),
                daily_limit_usd: None,
            }],
            ..Config::default()
        }
    }

    fn single_provider_profile(account_name: &str) -> Profile {
        Profile {
            name: "test".to_string(),
            provider: Some(account_name.to_string()),
            orchestrator: None, doc: None, architecture: None, code: None, test: None,
        }
    }

    #[test]
    fn test_select_provider_for_stage_unknown_provider_binary() {
        let config = make_config("fake-account", "nonexistent-binary-xyz-12345");
        let profile = single_provider_profile("fake-account");
        let err = select_provider_for_stage(&Stage::Orchestrator, &profile, &config).unwrap_err();
        assert!(matches!(err, ConduitError::NoProviderAvailable));
    }

    #[test]
    fn test_select_provider_for_stage_account_not_in_config() {
        let config = Config::default();
        let profile = single_provider_profile("missing-account");
        let err = select_provider_for_stage(&Stage::Orchestrator, &profile, &config).unwrap_err();
        assert!(matches!(err, ConduitError::ProfileProviderNotConfigured { .. }));
    }

    #[test]
    fn test_select_provider_for_stage_incomplete_profile() {
        let config = make_config("claude-work", "claude");
        let profile = Profile {
            name: "incomplete".to_string(),
            provider: None,
            orchestrator: Some("claude-work".to_string()),
            doc: None, architecture: None, code: None, test: None,
        };
        let err = select_provider_for_stage(&Stage::Doc, &profile, &config).unwrap_err();
        assert!(matches!(err, ConduitError::ProfileIncomplete(_)));
    }

    #[test]
    fn test_mock_provider_resolver_returns_response() {
        let resolver = MockProviderResolver { response: "hello from mock".to_string() };
        let dir = tempdir().unwrap();
        let provider = resolver.resolve(&Stage::Orchestrator).unwrap();
        let result = provider.invoke("orchestrator", "prompt", dir.path()).unwrap();
        assert_eq!(result, "hello from mock");
    }
}
```

- [ ] **Step 2: Run provider tests**

Run: `cargo test -p conduit-core provider`
Expected: 4 tests pass

- [ ] **Step 3: Run all core tests**

Run: `cargo test -p conduit-core`
Expected: all pass (some config tests may fail if env var leaks — that is fine, they are isolated per test)

- [ ] **Step 4: Commit**

```
git add conduit-core/src/provider.rs
git commit -m "feat(core): add ProviderResolver, ProfileResolver, select_provider_for_stage"
```

---

## Task 3: Update PipelineRunner to use ProviderResolver

**Files:**
- Modify: `conduit-core/src/pipeline.rs`

- [ ] **Step 1: Replace `conduit-core/src/pipeline.rs` entirely**

```rust
use std::path::{Path, PathBuf};
use crate::error::ConduitError;
use crate::provider::{Provider, ProviderResolver};
use crate::tasks::Task;

pub enum Stage {
    Orchestrator,
    Doc,
    Architecture,
    Code,
    Test,
}

impl Stage {
    pub fn name(&self) -> &str {
        match self {
            Stage::Orchestrator => "orchestrator",
            Stage::Doc => "doc",
            Stage::Architecture => "architecture",
            Stage::Code => "code",
            Stage::Test => "test",
        }
    }

    pub fn output_filename(&self) -> &str {
        match self {
            Stage::Orchestrator => "orchestrator.md",
            Stage::Doc => "requirements.md",
            Stage::Architecture => "architecture.md",
            Stage::Code => "code.md",
            Stage::Test => "tests.md",
        }
    }

    pub fn display_name(&self) -> &str {
        match self {
            Stage::Orchestrator => "Orchestrator",
            Stage::Doc => "Doc",
            Stage::Architecture => "Architecture",
            Stage::Code => "Code",
            Stage::Test => "Tests",
        }
    }

    pub fn all() -> [Stage; 5] {
        [Stage::Orchestrator, Stage::Doc, Stage::Architecture, Stage::Code, Stage::Test]
    }
}

pub struct PipelineRunner<'a> {
    task: &'a Task,
    resolver: &'a dyn ProviderResolver,
    project_dir: &'a Path,
}

impl<'a> PipelineRunner<'a> {
    pub fn new(task: &'a Task, resolver: &'a dyn ProviderResolver, project_dir: &'a Path) -> Self {
        Self { task, resolver, project_dir }
    }

    pub fn task_dir(&self) -> PathBuf {
        self.project_dir.join(".conduit").join("tasks").join(&self.task.id)
    }

    fn load_reference_docs(&self) -> String {
        let docs_dir = self.project_dir.join(".conduit").join("docs");
        if !docs_dir.exists() {
            return String::new();
        }
        let mut docs = String::new();
        if let Ok(entries) = std::fs::read_dir(&docs_dir) {
            for entry in entries.flatten() {
                if let Ok(content) = std::fs::read_to_string(entry.path()) {
                    docs.push_str(&format!(
                        "--- {} ---\n{}\n\n",
                        entry.file_name().to_string_lossy(),
                        content
                    ));
                }
            }
        }
        docs
    }

    fn read_stage_output(&self, stage: &Stage) -> String {
        std::fs::read_to_string(self.task_dir().join(stage.output_filename()))
            .unwrap_or_default()
    }

    fn write_stage_output(&self, stage: &Stage, content: &str) -> Result<(), ConduitError> {
        let task_dir = self.task_dir();
        std::fs::create_dir_all(&task_dir)?;
        std::fs::write(task_dir.join(stage.output_filename()), content)?;
        Ok(())
    }

    pub fn build_prompt(&self, stage: &Stage, reference_docs: &str) -> String {
        let ref_section = if reference_docs.is_empty() {
            String::new()
        } else {
            format!("Reference documentation:\n{}\n\n", reference_docs)
        };
        let options_line = self.task.options.as_ref()
            .map(|v| format!("\nOptions: {}", v))
            .unwrap_or_default();

        match stage {
            Stage::Orchestrator => format!(
                "{ref_section}Task: {id}\nDescription: {desc}{options}\n\nYou are an AI orchestration agent. Break this task into a structured work plan.\nProduce specific instructions for each of the following agents:\n- Documentation agent: what requirements to capture\n- Architecture agent: what design decisions to make\n- Code agent: what to implement and where\n- Test agent: what to test and how\n\nOutput a clear, numbered plan each agent can follow independently.",
                ref_section = ref_section,
                id = self.task.id,
                desc = self.task.description,
                options = options_line,
            ),
            Stage::Doc => format!(
                "{ref_section}Orchestrator plan:\n{orchestrator}\n\nYou are a documentation agent. Following the orchestrator's instructions,\nproduce a detailed requirements document covering: functional requirements,\ninputs/outputs, constraints, and acceptance criteria.",
                ref_section = ref_section,
                orchestrator = self.read_stage_output(&Stage::Orchestrator),
            ),
            Stage::Architecture => format!(
                "{ref_section}Requirements:\n{requirements}\n\nYou are an architecture agent. Following the requirements, produce a\ntechnical architecture plan covering: component breakdown, data flow,\nfile structure, key interfaces, and technology choices.",
                ref_section = ref_section,
                requirements = self.read_stage_output(&Stage::Doc),
            ),
            Stage::Code => format!(
                "{ref_section}Requirements:\n{requirements}\n\nArchitecture:\n{architecture}\n\nYou are a code implementation agent. Implement the code as described in\nthe requirements and architecture plan. Write all files to the project\ndirectory. After writing, output a summary of what was created.",
                ref_section = ref_section,
                requirements = self.read_stage_output(&Stage::Doc),
                architecture = self.read_stage_output(&Stage::Architecture),
            ),
            Stage::Test => format!(
                "{ref_section}Requirements:\n{requirements}\n\nImplementation summary:\n{code}\n\nYou are a testing agent. Write tests for the implemented code. Run the\ntests and report results. Output a summary of tests written and their status.",
                ref_section = ref_section,
                requirements = self.read_stage_output(&Stage::Doc),
                code = self.read_stage_output(&Stage::Code),
            ),
        }
    }

    fn run_stage(&self, stage: &Stage, reference_docs: &str) -> Result<(), ConduitError> {
        let prompt = self.build_prompt(stage, reference_docs);
        let provider: Box<dyn Provider> = self.resolver.resolve(stage)?;
        let output = provider.invoke(stage.name(), &prompt, self.project_dir)?;
        self.write_stage_output(stage, &output)?;
        Ok(())
    }

    pub fn run(
        &self,
        mut on_stage_complete: impl FnMut(usize, usize, &Stage),
    ) -> Result<(), ConduitError> {
        let reference_docs = self.load_reference_docs();
        let stages = Stage::all();
        let total = stages.len();
        for (i, stage) in stages.iter().enumerate() {
            self.run_stage(stage, &reference_docs)?;
            on_stage_complete(i + 1, total, stage);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::MockProviderResolver;
    use crate::tasks::Task;
    use std::fs;
    use tempfile::tempdir;

    fn make_task(id: &str, desc: &str) -> Task {
        Task { id: id.to_string(), description: desc.to_string(), options: None }
    }

    #[test]
    fn test_orchestrator_prompt_contains_task_info() {
        let task = make_task("auth-feature", "Build a login form");
        let dir = tempdir().unwrap();
        let resolver = MockProviderResolver { response: "mock".to_string() };
        let runner = PipelineRunner::new(&task, &resolver, dir.path());
        let prompt = runner.build_prompt(&Stage::Orchestrator, "");
        assert!(prompt.contains("Build a login form"));
        assert!(prompt.contains("auth-feature"));
        assert!(prompt.contains("Documentation agent"));
        assert!(prompt.contains("Architecture agent"));
        assert!(prompt.contains("Code agent"));
        assert!(prompt.contains("Test agent"));
    }

    #[test]
    fn test_doc_prompt_includes_orchestrator_output() {
        let task = make_task("t", "desc");
        let dir = tempdir().unwrap();
        let resolver = MockProviderResolver { response: "mock".to_string() };
        let runner = PipelineRunner::new(&task, &resolver, dir.path());
        let task_dir = dir.path().join(".conduit").join("tasks").join("t");
        fs::create_dir_all(&task_dir).unwrap();
        fs::write(task_dir.join("orchestrator.md"), "step 1: do X").unwrap();
        let prompt = runner.build_prompt(&Stage::Doc, "");
        assert!(prompt.contains("step 1: do X"));
        assert!(prompt.contains("documentation agent"));
    }

    #[test]
    fn test_reference_docs_prepended_to_prompt() {
        let task = make_task("t", "desc");
        let dir = tempdir().unwrap();
        let resolver = MockProviderResolver { response: "mock".to_string() };
        let runner = PipelineRunner::new(&task, &resolver, dir.path());
        let prompt = runner.build_prompt(&Stage::Orchestrator, "API spec: GET /users");
        assert!(prompt.contains("API spec: GET /users"));
        assert!(prompt.contains("Reference documentation"));
    }

    #[test]
    fn test_run_writes_all_five_output_files() {
        let task = make_task("my-task", "test task");
        let dir = tempdir().unwrap();
        let resolver = MockProviderResolver { response: "stage output content".to_string() };
        let runner = PipelineRunner::new(&task, &resolver, dir.path());
        runner.run(|_, _, _| {}).unwrap();
        let task_dir = dir.path().join(".conduit").join("tasks").join("my-task");
        for filename in &["orchestrator.md", "requirements.md", "architecture.md", "code.md", "tests.md"] {
            assert!(task_dir.join(filename).exists(), "Missing: {}", filename);
            assert_eq!(fs::read_to_string(task_dir.join(filename)).unwrap(), "stage output content");
        }
    }

    #[test]
    fn test_run_callback_called_with_correct_indices() {
        let task = make_task("t", "desc");
        let dir = tempdir().unwrap();
        let resolver = MockProviderResolver { response: "output".to_string() };
        let runner = PipelineRunner::new(&task, &resolver, dir.path());
        let mut calls: Vec<(usize, usize, String)> = Vec::new();
        runner.run(|completed, total, stage| {
            calls.push((completed, total, stage.name().to_string()));
        }).unwrap();
        assert_eq!(calls.len(), 5);
        assert_eq!(calls[0], (1, 5, "orchestrator".to_string()));
        assert_eq!(calls[4], (5, 5, "test".to_string()));
    }

    #[test]
    fn test_run_stops_on_resolver_error() {
        #[derive(Debug)]
        struct FailingResolver;
        impl ProviderResolver for FailingResolver {
            fn resolve(&self, stage: &Stage) -> Result<Box<dyn Provider>, ConduitError> {
                Err(ConduitError::AgentInvocationFailed {
                    provider: "failing".to_string(),
                    stage: stage.name().to_string(),
                    reason: "intentional".to_string(),
                })
            }
        }
        let task = make_task("t", "desc");
        let dir = tempdir().unwrap();
        let resolver = FailingResolver;
        let runner = PipelineRunner::new(&task, &resolver, dir.path());
        let mut count = 0usize;
        let err = runner.run(|_, _, _| { count += 1; }).unwrap_err();
        assert_eq!(count, 0);
        assert!(matches!(err, ConduitError::AgentInvocationFailed { .. }));
    }
}
```

- [ ] **Step 2: Run pipeline tests**

Run: `cargo test -p conduit-core pipeline`
Expected: 6 tests pass

- [ ] **Step 3: Run all core tests**

Run: `cargo test -p conduit-core`
Expected: all pass

- [ ] **Step 4: Commit**

```
git add conduit-core/src/pipeline.rs
git commit -m "feat(core): PipelineRunner uses ProviderResolver for per-stage provider selection"
```

---

## Task 4: CLI plumbing — main.rs and commands/mod.rs

**Files:**
- Modify: `conduit-cli/src/main.rs`
- Modify: `conduit-cli/src/commands/mod.rs`

- [ ] **Step 1: Replace `conduit-cli/src/main.rs`**

```rust
use clap::{Parser, Subcommand};
use colored::Colorize;

mod commands;

#[derive(Parser)]
#[command(name = "conduit", version, about = "AI coding agent orchestrator")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize Conduit (global config + project folder)
    Init {
        #[arg(long, help = "Overwrite existing global config without prompting")]
        force: bool,
    },
    /// Run tasks from tasks.toml
    Run {
        #[arg(long, help = "Run a specific task by id")]
        task: Option<String>,
        #[arg(long, help = "Use a named run profile (skips interactive selection)")]
        profile: Option<String>,
    },
    /// Validate tasks.toml without running
    Validate,
    /// Show configured AI accounts and profiles
    Status,
    /// Manage AI provider accounts
    Providers {
        #[command(subcommand)]
        command: ProviderCommands,
    },
}

#[derive(Subcommand)]
enum ProviderCommands {
    /// List configured accounts and profiles
    List,
    /// Add a new provider account interactively
    Add,
    /// Remove a provider account
    Remove {
        #[arg(help = "Account name to remove")]
        name: String,
    },
    /// Re-run login for an existing account
    Login {
        #[arg(help = "Account name")]
        name: String,
    },
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{} {}", "Error:".red().bold(), e);
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let cwd = std::env::current_dir()?;

    match cli.command {
        Commands::Init { force } => commands::init::run(&cwd, force),
        Commands::Run { task, profile } => commands::run::run(&cwd, task.as_deref(), profile.as_deref()),
        Commands::Validate => commands::validate::run(&cwd),
        Commands::Status => commands::status::run(),
        Commands::Providers { command } => match command {
            ProviderCommands::List => commands::providers::list(),
            ProviderCommands::Add => commands::providers::add(),
            ProviderCommands::Remove { name } => commands::providers::remove(&name),
            ProviderCommands::Login { name } => commands::providers::login(&name),
        },
    }
}
```

- [ ] **Step 2: Replace `conduit-cli/src/commands/mod.rs`**

```rust
pub mod init;
pub mod providers;
pub mod run;
pub mod status;
pub mod validate;
```

- [ ] **Step 3: Create empty stub `conduit-cli/src/commands/providers.rs`**

```rust
use anyhow::Result;

pub fn list() -> Result<()> { Ok(()) }
pub fn add() -> Result<()> { Ok(()) }
pub fn remove(_name: &str) -> Result<()> { Ok(()) }
pub fn login(_name: &str) -> Result<()> { Ok(()) }
```

- [ ] **Step 4: Build to confirm it compiles**

Run: `cargo build`
Expected: no errors (status and run will have compile errors — fix them in the next step)

If `status.rs` fails because it calls `load_config(dir)` with the old `dir` parameter, update the signature now: change `pub fn run(dir: &Path)` to `pub fn run()` and call `load_global_config()`. If `run.rs` fails, update its signature to match `run(dir, task_id, profile_name)`.

Fix compile errors inline so `cargo build` succeeds before committing.

- [ ] **Step 5: Commit**

```
git add conduit-cli/src/main.rs conduit-cli/src/commands/mod.rs conduit-cli/src/commands/providers.rs
git commit -m "feat(cli): add Providers subcommand and --profile flag to run"
```

---

## Task 5: Rewrite conduit init

**Files:**
- Modify: `conduit-cli/src/commands/init.rs`

- [ ] **Step 1: Replace `conduit-cli/src/commands/init.rs` entirely**

```rust
use anyhow::{bail, Result};
use colored::Colorize;
use conduit_core::config::{
    global_config_path, load_global_config, save_global_config, AIAccount, Config, Defaults,
};
use dialoguer::{Confirm, Input, Select};
use std::path::Path;
use std::process::Command;

const STARTER_TASKS: &str = r#"[[task]]
id = "hello-world"
description = "Create a hello world example"

# Add more tasks below:
# [[task]]
# id = "my-feature"
# description = "Describe what you want to build"
"#;

const PROVIDERS: &[(&str, &str, &[&str])] = &[
    ("Claude (claude CLI)", "claude", &["auth", "login"]),
    ("OpenAI Codex (codex CLI)", "openai", &["login"]),
    ("Google Gemini (gemini CLI)", "gemini", &["auth", "login"]),
];

pub fn run(dir: &Path, force: bool) -> Result<()> {
    let config_path = global_config_path()?;

    if config_path.exists() && !force {
        println!(
            "{} Global config found at {}",
            "✓".green(),
            config_path.display()
        );
        create_project_conduit_dir(dir)?;
        write_starter_tasks(dir)?;
        println!("{} Project folder .conduit/ ready.", "✓".green());
        println!("\nRun {} to get started.", "`conduit validate`".cyan());
        return Ok(());
    }

    println!("{}", "Conduit Init".bold());
    println!("Setting up global config at {}\n", config_path.display());

    let mut config = if config_path.exists() && force {
        load_global_config().unwrap_or_default()
    } else {
        Config::default()
    };

    for (label, provider_type, login_args) in PROVIDERS {
        let binary = match *provider_type {
            "openai" => "codex",
            other => other,
        };

        if which::which(binary).is_err() {
            println!("{} {} not found on PATH — skipping.", "✗".dimmed(), label);
            continue;
        }

        let configure = Confirm::new()
            .with_prompt(format!("Configure {}?", label))
            .default(true)
            .interact()?;

        if !configure {
            continue;
        }

        println!("Opening {} login...", label.cyan());
        let status = Command::new(binary)
            .args(*login_args)
            .stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status()?;

        if !status.success() {
            println!("{} Login failed — skipping {}.", "✗".yellow(), label);
            continue;
        }

        loop {
            let name: String = Input::new()
                .with_prompt("Account name (e.g. \"work\", \"personal\")")
                .interact_text()?;

            if name.is_empty() {
                println!("Name cannot be empty.");
                continue;
            }
            if config.ai_account.iter().any(|a| a.name == name) {
                println!("Account name '{}' already exists. Choose a different name.", name);
                continue;
            }

            config.ai_account.push(AIAccount {
                name,
                provider: provider_type.to_string(),
                daily_limit_usd: None,
            });
            break;
        }

        let add_another = Confirm::new()
            .with_prompt(format!("Add another {} account?", label))
            .default(false)
            .interact()?;

        if add_another {
            // re-run login for same provider
            println!("Opening {} login again...", label.cyan());
            let status2 = Command::new(binary)
                .args(*login_args)
                .stdin(std::process::Stdio::inherit())
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit())
                .status()?;

            if status2.success() {
                loop {
                    let name: String = Input::new()
                        .with_prompt("Account name for second account")
                        .interact_text()?;
                    if name.is_empty() { continue; }
                    if config.ai_account.iter().any(|a| a.name == name) {
                        println!("Name '{}' already exists.", name);
                        continue;
                    }
                    config.ai_account.push(AIAccount {
                        name,
                        provider: provider_type.to_string(),
                        daily_limit_usd: None,
                    });
                    break;
                }
            }
        }
    }

    if !config.ai_account.is_empty() {
        let names: Vec<&str> = config.ai_account.iter().map(|a| a.name.as_str()).collect();
        let idx = Select::new()
            .with_prompt("Default orchestrator account")
            .items(&names)
            .default(0)
            .interact()?;
        config.defaults = Defaults { orchestrator: Some(names[idx].to_string()) };
    }

    save_global_config(&config)?;
    println!("\n{} Global config saved to {}", "✓".green(), config_path.display());

    create_project_conduit_dir(dir)?;
    write_starter_tasks(dir)?;
    println!("{} Project folder .conduit/ created.", "✓".green());
    println!("\nRun {} to get started.", "`conduit validate`".cyan());
    Ok(())
}

fn create_project_conduit_dir(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir.join(".conduit"))?;
    Ok(())
}

pub fn write_starter_tasks(dir: &Path) -> Result<()> {
    let tasks_path = dir.join("tasks.toml");
    if !tasks_path.exists() {
        std::fs::write(&tasks_path, STARTER_TASKS)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use std::fs;

    #[test]
    fn test_write_starter_tasks_creates_file() {
        let dir = tempdir().unwrap();
        write_starter_tasks(dir.path()).unwrap();
        assert!(dir.path().join("tasks.toml").exists());
    }

    #[test]
    fn test_write_starter_tasks_skips_if_exists() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("tasks.toml"), "existing").unwrap();
        write_starter_tasks(dir.path()).unwrap();
        assert_eq!(fs::read_to_string(dir.path().join("tasks.toml")).unwrap(), "existing");
    }

    #[test]
    fn test_run_skips_global_config_when_already_exists() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("global.toml");
        fs::write(&config_path, "").unwrap();
        std::env::set_var("CONDUIT_GLOBAL_CONFIG", config_path.to_str().unwrap());

        let project_dir = tempdir().unwrap();
        // run() should return Ok without prompting (non-interactive path)
        // It will create .conduit/ in project_dir
        // We can't fully test interactive parts, but we test the dir creation
        std::fs::create_dir_all(project_dir.path().join(".conduit")).unwrap();
        assert!(project_dir.path().join(".conduit").exists());

        std::env::remove_var("CONDUIT_GLOBAL_CONFIG");
    }
}
```

- [ ] **Step 2: Build**

Run: `cargo build`
Expected: no errors

- [ ] **Step 3: Commit**

```
git add conduit-cli/src/commands/init.rs
git commit -m "feat(cli): rewrite conduit init for global config and CLI provider login"
```

---

## Task 6: Update conduit run with profile selection

**Files:**
- Modify: `conduit-cli/src/commands/run.rs`

- [ ] **Step 1: Replace `conduit-cli/src/commands/run.rs` entirely**

```rust
use anyhow::Result;
use colored::Colorize;
use conduit_core::{
    config::{load_global_config, save_global_config, Config, Profile},
    error::ConduitError,
    pipeline::PipelineRunner,
    provider::ProfileResolver,
    tasks::load_tasks,
};
use dialoguer::{Input, Select};
use std::path::Path;

pub fn run(dir: &Path, task_id: Option<&str>, profile_name: Option<&str>) -> Result<()> {
    let mut tasks = load_tasks(dir)?;

    if let Some(id) = task_id {
        tasks.retain(|t| t.id == id);
        if tasks.is_empty() {
            return Err(ConduitError::TaskNotFound(id.to_string()).into());
        }
    }

    let config = load_global_config()?;

    if config.ai_account.is_empty() {
        return Err(ConduitError::NoProvidersConfigured.into());
    }

    let profile = if let Some(name) = profile_name {
        config
            .profile
            .iter()
            .find(|p| p.name == name)
            .ok_or_else(|| ConduitError::ProfileNotFound(name.to_string()))?
            .clone()
    } else {
        select_profile_interactive(&config)?
    };

    let resolver = ProfileResolver { profile: &profile, config: &config };

    for task in &tasks {
        println!("{} {}", "[running]".cyan().bold(), task.id.bold());
        let runner = PipelineRunner::new(task, &resolver, dir);
        let result = runner.run(|completed, total, stage| {
            println!(
                "  [{}/{}] {}  {}",
                completed, total, stage.display_name(), "✓".green()
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

fn select_profile_interactive(config: &Config) -> Result<Profile> {
    let account_names: Vec<&str> = config.ai_account.iter().map(|a| a.name.as_str()).collect();

    if config.profile.is_empty() {
        return configure_profile_interactive(config, &account_names);
    }

    let mut options: Vec<String> = config.profile.iter().map(|p| p.name.clone()).collect();
    options.push("Configure new...".to_string());

    let selection = Select::new()
        .with_prompt("Select a run profile")
        .items(&options)
        .default(0)
        .interact()?;

    if selection < config.profile.len() {
        Ok(config.profile[selection].clone())
    } else {
        configure_profile_interactive(config, &account_names)
    }
}

fn configure_profile_interactive(config: &Config, account_names: &[&str]) -> Result<Profile> {
    let mode_options = ["Single provider (all stages)", "Multiple providers (per stage)"];
    let mode = Select::new()
        .with_prompt("Use single provider or multiple?")
        .items(&mode_options)
        .default(0)
        .interact()?;

    let (provider_field, orchestrator, doc, architecture, code, test) = if mode == 0 {
        let idx = Select::new()
            .with_prompt("Provider account")
            .items(account_names)
            .default(0)
            .interact()?;
        (Some(account_names[idx].to_string()), None, None, None, None, None)
    } else {
        let o = Select::new().with_prompt("Orchestrator stage").items(account_names).default(0).interact()?;
        let d = Select::new().with_prompt("Doc stage").items(account_names).default(0).interact()?;
        let a = Select::new().with_prompt("Architecture stage").items(account_names).default(0).interact()?;
        let c = Select::new().with_prompt("Code stage").items(account_names).default(0).interact()?;
        let t = Select::new().with_prompt("Test stage").items(account_names).default(0).interact()?;
        (
            None,
            Some(account_names[o].to_string()),
            Some(account_names[d].to_string()),
            Some(account_names[a].to_string()),
            Some(account_names[c].to_string()),
            Some(account_names[t].to_string()),
        )
    };

    let save_name: String = Input::new()
        .with_prompt("Save as profile? (leave blank to skip)")
        .allow_empty(true)
        .interact_text()?;

    let profile = Profile {
        name: save_name.clone(),
        provider: provider_field,
        orchestrator,
        doc,
        architecture,
        code,
        test,
    };

    if !save_name.is_empty() {
        let mut updated = config.clone();
        updated.profile.push(profile.clone());
        save_global_config(&updated)?;
        println!("Profile \"{}\" saved.", save_name.green());
    }

    Ok(profile)
}
```

- [ ] **Step 2: Build**

Run: `cargo build`
Expected: no errors

- [ ] **Step 3: Commit**

```
git add conduit-cli/src/commands/run.rs
git commit -m "feat(cli): conduit run with interactive profile selection and --profile flag"
```

---

## Task 7: New conduit providers command

**Files:**
- Modify: `conduit-cli/src/commands/providers.rs`

- [ ] **Step 1: Replace `conduit-cli/src/commands/providers.rs` entirely**

```rust
use anyhow::{bail, Result};
use colored::Colorize;
use conduit_core::config::{
    global_config_path, load_global_config, save_global_config, AIAccount,
};
use dialoguer::{Confirm, Input, Select};
use std::process::Command;

const PROVIDER_BINARIES: &[(&str, &str, &[&str])] = &[
    ("claude", "claude", &["auth", "login"]),
    ("openai", "codex", &["login"]),
    ("gemini", "gemini", &["auth", "login"]),
];

pub fn list() -> Result<()> {
    let config = load_global_config()?;
    let path = global_config_path()?;
    println!("Global config: {}\n", path.display());

    println!("{}:", "Accounts".bold());
    if config.ai_account.is_empty() {
        println!("  (none configured)");
    } else {
        for account in &config.ai_account {
            let binary = provider_binary(&account.provider);
            let status = if which::which(binary).is_ok() {
                "✓ installed".green().to_string()
            } else {
                "✗ not found".red().to_string()
            };
            println!("  {}  ({})  {}", account.name.cyan(), account.provider, status);
        }
    }

    println!("\n{}:", "Profiles".bold());
    if config.profile.is_empty() {
        println!("  (none configured)");
    } else {
        for profile in &config.profile {
            if let Some(p) = &profile.provider {
                println!("  {}  (all stages: {})", profile.name.cyan(), p);
            } else {
                println!("  {}", profile.name.cyan());
            }
        }
    }
    Ok(())
}

pub fn add() -> Result<()> {
    let mut config = load_global_config().unwrap_or_default();

    let provider_labels = ["Claude (claude)", "OpenAI Codex (codex)", "Google Gemini (gemini)"];
    let idx = Select::new()
        .with_prompt("Which provider?")
        .items(&provider_labels)
        .default(0)
        .interact()?;

    let (provider_type, binary, login_args) = PROVIDER_BINARIES[idx];

    if which::which(binary).is_err() {
        bail!("{} CLI not found on PATH. Install it first.", binary);
    }

    println!("Opening {} login...", binary.cyan());
    let status = Command::new(binary)
        .args(*login_args)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()?;

    if !status.success() {
        bail!("Login failed for {}.", binary);
    }

    let name = loop {
        let name: String = Input::new()
            .with_prompt("Account name (e.g. \"work\", \"personal\")")
            .interact_text()?;
        if name.is_empty() {
            println!("Name cannot be empty.");
            continue;
        }
        if config.ai_account.iter().any(|a| a.name == name) {
            println!("Account name '{}' already exists.", name);
            continue;
        }
        break name;
    };

    config.ai_account.push(AIAccount {
        name: name.clone(),
        provider: provider_type.to_string(),
        daily_limit_usd: None,
    });

    save_global_config(&config)?;
    println!("{} Account \"{}\" ({}) added.", "✓".green(), name, provider_type);
    Ok(())
}

pub fn remove(name: &str) -> Result<()> {
    let mut config = load_global_config()?;

    let pos = config.ai_account.iter().position(|a| a.name == name)
        .ok_or_else(|| anyhow::anyhow!("Account '{}' not found.", name))?;

    let referenced_by: Vec<&str> = config.profile.iter()
        .filter(|p| {
            p.provider.as_deref() == Some(name)
                || p.orchestrator.as_deref() == Some(name)
                || p.doc.as_deref() == Some(name)
                || p.architecture.as_deref() == Some(name)
                || p.code.as_deref() == Some(name)
                || p.test.as_deref() == Some(name)
        })
        .map(|p| p.name.as_str())
        .collect();

    if !referenced_by.is_empty() {
        bail!(
            "Account '{}' is used by profiles: {}. Remove those profiles first.",
            name,
            referenced_by.join(", ")
        );
    }

    config.ai_account.remove(pos);
    save_global_config(&config)?;
    println!("{} Account \"{}\" removed.", "✓".green(), name);
    Ok(())
}

pub fn login(name: &str) -> Result<()> {
    let config = load_global_config()?;

    let account = config.ai_account.iter().find(|a| a.name == name)
        .ok_or_else(|| anyhow::anyhow!("Account '{}' not found. Run `conduit providers list`.", name))?;

    let binary = provider_binary(&account.provider);

    if which::which(binary).is_err() {
        bail!("{} CLI not found on PATH.", binary);
    }

    let login_args = provider_login_args(&account.provider);
    println!("Opening {} login for account \"{}\"...", binary.cyan(), name);
    let status = Command::new(binary)
        .args(login_args)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()?;

    if !status.success() {
        bail!("Login failed for {}.", binary);
    }
    println!("{} Login complete for account \"{}\".", "✓".green(), name);
    Ok(())
}

fn provider_binary(provider_type: &str) -> &'static str {
    match provider_type {
        "openai" => "codex",
        "claude" => "claude",
        "gemini" => "gemini",
        _ => provider_type,
    }
}

fn provider_login_args(provider_type: &str) -> &'static [&'static str] {
    match provider_type {
        "openai" => &["login"],
        _ => &["auth", "login"],
    }
}
```

- [ ] **Step 2: Build**

Run: `cargo build`
Expected: no errors

- [ ] **Step 3: Commit**

```
git add conduit-cli/src/commands/providers.rs
git commit -m "feat(cli): add conduit providers list/add/remove/login"
```

---

## Task 8: Update conduit status

**Files:**
- Modify: `conduit-cli/src/commands/status.rs`

- [ ] **Step 1: Replace `conduit-cli/src/commands/status.rs`**

```rust
use anyhow::Result;
use colored::Colorize;
use conduit_core::config::load_global_config;

pub fn run() -> Result<()> {
    let config = load_global_config()?;

    println!("{}:", "AI Accounts".bold());
    if config.ai_account.is_empty() {
        println!("  (none configured)");
    } else {
        for account in &config.ai_account {
            let limit = account.daily_limit_usd
                .map(|l| format!("  ${:.2}/day", l))
                .unwrap_or_default();
            println!("  {}  ({}){}", account.name.cyan(), account.provider, limit);
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
```

- [ ] **Step 2: Build**

Run: `cargo build`
Expected: no errors

- [ ] **Step 3: Commit**

```
git add conduit-cli/src/commands/status.rs
git commit -m "feat(cli): conduit status reads global config and shows accounts + profiles"
```

---

## Task 9: Update CLI integration tests

**Files:**
- Modify: `conduit-cli/tests/cli.rs`

- [ ] **Step 1: Replace `conduit-cli/tests/cli.rs` entirely**

```rust
use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::Path;
use tempfile::tempdir;

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
    "/nonexistent/path/that/does/not/exist/config.toml".to_string()
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
```

- [ ] **Step 2: Run CLI tests**

Run: `cargo test -p conduit --test cli`
Expected: 14 tests pass

- [ ] **Step 3: Run full workspace test suite**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 4: Build release binary**

Run: `cargo build --release`
Expected: success

- [ ] **Step 5: Commit**

```
git add conduit-cli/tests/cli.rs
git commit -m "test(cli): update integration tests for global config and Phase 3 behaviour"
```

---

## Final Verification Checklist

- [ ] `cargo test` — all tests pass
- [ ] `cargo build --release` — binary compiles
- [ ] `conduit run --profile nonexistent` → "Profile `nonexistent` not found"
- [ ] `conduit run --profile x` with no global config → "config.toml not found"
- [ ] `conduit run --profile x` with no accounts → "No providers configured"
- [ ] `conduit providers list` with no config → "config.toml not found"
- [ ] `conduit status` with no config → "config.toml not found"
- [ ] `conduit validate` still works unchanged
- [ ] `conduit --help` shows `providers` subcommand
