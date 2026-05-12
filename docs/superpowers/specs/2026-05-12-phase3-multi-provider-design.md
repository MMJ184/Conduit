# Conduit — Phase 3: Multi-Provider + Run Profiles Design

**Date:** 2026-05-12
**Scope:** Global config, named accounts, run profiles, interactive profile selection, `conduit providers` command
**Status:** Approved
**Depends on:** Phase 2 (pipeline runner, Provider trait)

---

## Overview

Phase 3 moves provider configuration from per-project to **global** (`~/.conduit/config.toml`), adds **named accounts** to distinguish multiple accounts of the same provider type, and introduces **named run profiles** that define which account handles which pipeline stage. At `conduit run` time, the user selects a profile interactively or passes `--profile <name>` to skip the prompt.

Conduit does not store API keys. Each provider is a CLI tool (`claude`, `codex`, `gemini`) that manages its own authentication. `conduit providers add` runs the CLI's own login command with the terminal handed to it.

---

## Global Config

### Path

| Platform | Path |
|---|---|
| Windows | `%USERPROFILE%\.conduit\config.toml` |
| macOS / Linux | `~/.conduit/config.toml` |

Resolved via `dirs::home_dir().join(".conduit/config.toml")`. Returns `ConduitError::GlobalConfigDirNotFound` if home dir cannot be determined.

### Format

```toml
[[ai_account]]
name = "claude-work"
provider = "claude"
daily_limit_usd = 10.0

[[ai_account]]
name = "claude-personal"
provider = "claude"

[[ai_account]]
name = "codex-main"
provider = "openai"

[[ai_account]]
name = "gemini-free"
provider = "gemini"

[defaults]
orchestrator = "claude-work"

[[profile]]
name = "all-claude"
provider = "claude-work"          # single account — all stages

[[profile]]
name = "mixed"
orchestrator = "claude-work"
doc = "claude-work"
architecture = "claude-work"
code = "codex-main"
test = "gemini-free"
```

### Rules

- `name` must be unique across all `[[ai_account]]` entries.
- `provider` must be one of: `"claude"`, `"openai"`, `"gemini"`.
- `daily_limit_usd` is optional; used in Phase 5 (limit monitoring).
- A profile with a `provider` field uses that account for **all** stages (single-provider profile).
- A profile with per-stage fields must specify all 5 stages: `orchestrator`, `doc`, `architecture`, `code`, `test`. Missing stages are an error.
- Profile stage values are account **names**, not provider types.
- `[defaults] orchestrator` sets which account runs the Orchestrator stage when no profile is specified.

---

## Per-Project `.conduit/` Folder

The per-project `.conduit/config.toml` is **removed**. The `.conduit/` folder still exists in each project for:

```
.conduit/
  tasks/<task-id>/     ← stage output files (unchanged from Phase 2)
  docs/                ← reference docs prepended to prompts (unchanged)
```

`conduit init` creates `.conduit/` in the current directory but no longer writes a config file there.

---

## Data Model Changes

### `AIAccount` (breaking change from Phase 1/2)

```rust
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AIAccount {
    pub name: String,
    pub provider: String,
    pub daily_limit_usd: Option<f64>,
}
```

`api_key` is removed. Authentication is handled by the CLI tool itself.

### New types

```rust
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct Defaults {
    pub orchestrator: Option<String>,   // account name
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Profile {
    pub name: String,
    pub provider: Option<String>,       // single-provider shorthand
    pub orchestrator: Option<String>,
    pub doc: Option<String>,
    pub architecture: Option<String>,
    pub code: Option<String>,
    pub test: Option<String>,
}
```

### Updated `Config`

```rust
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Config {
    pub project: ProjectConfig,      // kept for project name (written by init)
    #[serde(default)]
    pub ai_account: Vec<AIAccount>,
    #[serde(default)]
    pub defaults: Defaults,
    #[serde(default)]
    pub profile: Vec<Profile>,
    #[serde(default)]
    pub ollama: OllamaConfig,
}
```

### New config functions

```rust
pub fn global_config_path() -> Result<PathBuf, ConduitError>;
pub fn load_global_config() -> Result<Config, ConduitError>;
pub fn save_global_config(config: &Config) -> Result<(), ConduitError>;
```

`load_config(dir)` is updated to call `load_global_config()` — the `dir` argument is no longer used for config loading but kept for API compatibility during the transition.

---

## Provider Selection

### `select_provider_for_stage`

```rust
pub fn select_provider_for_stage(
    stage: &Stage,
    profile: &Profile,
    config: &Config,
) -> Result<Box<dyn Provider>, ConduitError>
```

Looks up the account name for the given stage from the profile, finds the matching `AIAccount`, checks the binary is on PATH via `which::which`, returns the correct `Provider` implementation.

Errors:
- `ProfileProviderNotConfigured(account_name, profile_name)` — account name in profile not found in `ai_account` list
- `NoProviderAvailable` — binary not found on PATH

### `PipelineRunner` update

`PipelineRunner` currently holds a single `provider: &dyn Provider`. Phase 3 changes it to hold a `profile: &Profile` and `config: &Config`, resolving the provider per-stage inside `run_stage`.

```rust
pub struct PipelineRunner<'a> {
    task: &'a Task,
    profile: &'a Profile,
    config: &'a Config,
    project_dir: &'a Path,
}
```

---

## Updated `conduit init`

`conduit init` now sets up the **global** config. If global config already exists, it creates only the project `.conduit/` folder and exits (no overwrite). `--force` overwrites the global config.

### Flow

```
Setting up Conduit...

Global config not found. Let's configure your AI providers.

Configure Claude (claude CLI):
  Checking if claude is installed... ✓
  Account name: claude-work
  Opening Claude login...
  [claude auth login runs interactively]
  Login complete.

Add another Claude account? (y/N): n

Configure OpenAI Codex (codex CLI):
  Checking if codex is installed... ✓
  Account name: codex-main
  Opening Codex login...
  [codex login runs interactively]
  Login complete.

Add another Codex account? (y/N): n

Configure Gemini (gemini CLI):
  Checking if gemini is installed... ✗ not found
  Skipping. Install from: https://cloud.google.com/sdk

Default orchestrator account [claude-work]: (Enter)

Global config saved to ~/.conduit/config.toml
Project folder .conduit/ created.
```

If global config already exists:
```
Global config found at ~/.conduit/config.toml
Project folder .conduit/ created.
```

---

## New `conduit providers` Command

```
conduit providers list
conduit providers add
conduit providers remove <name>
conduit providers login <name>
```

### `conduit providers list`

```
Configured accounts:
  claude-work   (claude)   ✓ installed
  claude-personal (claude) ✓ installed
  codex-main    (openai)   ✓ installed
  gemini-free   (gemini)   ✗ not found

Profiles:
  all-claude
  mixed
```

### `conduit providers add`

Interactive — asks provider type, checks binary on PATH, runs CLI login, asks account name, saves to global config.

### `conduit providers remove <name>`

Removes the account from `[[ai_account]]`. If the account is referenced in any profile, lists those profiles and asks for confirmation before removing.

### `conduit providers login <name>`

Re-runs the CLI login command for an existing account (useful when a session expires).

---

## Updated `conduit run`

### New `--profile` flag

```
conduit run [--task <id>] [--profile <name>]
```

### Interactive flow (no `--profile` flag)

```
$ conduit run

Available profiles:
  1. all-claude
  2. mixed
  Enter number or press Enter to configure now: _
```

**If profile selected:** run immediately.

**If Enter (configure now):**

```
Use single provider or multiple? [single/multi]: multi

Orchestrator stage [claude-work / codex-main]: claude-work
Doc stage: claude-work
Architecture stage: claude-work
Code stage: codex-main
Test stage: codex-main

Save as profile? (leave blank to skip): my-setup
Profile "my-setup" saved.
```

**If single:**
```
Provider account [claude-work / codex-main]: claude-work
Save as profile? (leave blank to skip): all-claude-work
```

**No configured accounts:**
```
Error: No providers configured. Run `conduit init` to set up your providers.
```

**`--profile` flag with unknown profile name:**
```
Error: Profile `bad-name` not found. Run `conduit providers list` to see available profiles.
```

---

## New Error Variants

```rust
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
```

`ConduitError::ConfigNotFound` remains for when global config is missing at run time (distinct from `NoProvidersConfigured`).

---

## CLI Changes

### `conduit-cli/Cargo.toml`

Add: `dirs = "5"`

### `conduit-cli/src/main.rs`

Add `Providers` subcommand with `list`, `add`, `remove`, `login` sub-subcommands.

### `conduit-cli/src/commands/`

| File | Change |
|---|---|
| `init.rs` | Write to global path; configure all providers; per-provider login; `--force` flag |
| `run.rs` | Load global config; interactive profile selection; `--profile` flag; per-stage provider |
| `providers.rs` | New: `list`, `add`, `remove`, `login` |
| `status.rs` | Read from global config path |
| `validate.rs` | No change needed |

---

## Testing

### Unit tests (`conduit-core`)

- `global_config_path()` returns expected path
- `Profile` TOML parsing: single-provider, multi-provider, missing stages
- `select_provider_for_stage()`: correct provider returned per stage; error on unknown account name
- `AIAccount` deserialization without `api_key` field
- Duplicate account names → error

### Integration tests (`conduit-cli`)

- `conduit providers list` with no global config → error message
- `conduit run --profile all-claude` with unknown profile → `ProfileNotFound` error
- `conduit run --profile mixed` where account not on PATH → `NoProviderAvailable` error
- `conduit init` with existing global config → no overwrite, `.conduit/` created
- `conduit init --force` → overwrites global config

---

## Dependencies Added

| Crate | Where | Used for |
|---|---|---|
| `dirs = "5"` | `conduit-cli/Cargo.toml` | Resolve `~` home directory path |

---

## Out of Scope for Phase 3

- Limit monitoring and failover (Phase 5)
- Parallel task execution (Phase 4)
- Context memory (Phase 6)
- Cost tracking (Phase 7)
- TUI dashboard (Phase 8)

---

## Success Criteria

Phase 3 is complete when:

1. `~/.conduit/config.toml` is the single source of truth for providers and profiles
2. `conduit init` configures all providers interactively via CLI login; safe to re-run
3. `conduit providers list/add/remove/login` all work correctly
4. `conduit run` offers profile selection; `--profile` skips the prompt
5. Each pipeline stage invokes the account specified in the selected profile
6. All Phase 1 and Phase 2 tests still pass
7. New unit and integration tests for profile selection pass
