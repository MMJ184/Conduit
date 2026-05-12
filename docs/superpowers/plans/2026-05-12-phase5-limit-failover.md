# Phase 5: Limit Monitoring + Account Failover Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add per-account spend tracking and automatic failover so `conduit run` skips over-limit accounts and retries failed providers without changing `ParallelRunner` or `PipelineRunner`.

**Architecture:** `SpendTracker` records daily invocations in `~/.conduit/spend.toml` (one TOML section per date). `FallbackProvider` wraps a prioritised candidate list and applies limit/error failover at invocation time. `FallbackResolver` builds that list (same-provider first, then any) and replaces `ProfileResolver` in `run.rs`. `--account` overrides the starting account.

**Tech Stack:** Rust workspace (`conduit-core` + `conduit-cli`), `chrono = "0.4"` (new), `toml`, `rayon`, `Arc<Mutex<>>`, `which`, `dialoguer`, `clap`

**Spec:** `docs/superpowers/specs/2026-05-12-phase5-limit-failover-design.md`

---

## File Map

| Action | Path | Purpose |
|---|---|---|
| Modify | `conduit-core/Cargo.toml` | Add `chrono` dependency |
| Modify | `conduit-core/src/config.rs` | Add `auto_switch`, `switch_on`, `cost_per_run` to `AIAccount` + helper methods |
| Modify | `conduit-core/src/error.rs` | Add `AllAccountsExhausted` variant |
| Create | `conduit-core/src/spend.rs` | `SpendTracker` — reads/writes `~/.conduit/spend.toml` |
| Modify | `conduit-core/src/lib.rs` | Export `pub mod spend` |
| Modify | `conduit-core/src/provider.rs` | Add `FallbackProvider` + `FallbackResolver` + `build_candidate_order` |
| Modify | `conduit-cli/src/main.rs` | Add `--account` arg to `Run` variant |
| Modify | `conduit-cli/src/commands/run.rs` | Validate account, load `SpendTracker`, use `FallbackResolver` |
| Modify | `conduit-cli/src/commands/providers.rs` | Add auto-switch prompts to `add()` + update `AIAccount` literal |
| Modify | `conduit-cli/src/commands/status.rs` | Show spend data per account |
| Modify | `conduit-cli/tests/cli.rs` | Integration tests for `--account` flag |

---

## Task 1: `AIAccount` Phase 5 fields + `chrono` dependency

**Files:**
- Modify: `conduit-core/Cargo.toml`
- Modify: `conduit-core/src/config.rs`
- Modify: `conduit-core/src/provider.rs` (update test helper `make_config`)
- Modify: `conduit-cli/src/commands/providers.rs` (update `AIAccount` struct literal in `add()`)

- [ ] **Step 1: Write failing tests for AIAccount helper methods**

Add the following test block inside the existing `#[cfg(test)] mod tests { ... }` at the bottom of `conduit-core/src/config.rs`, before the closing `}`:

```rust
    #[test]
    fn test_ai_account_auto_switch_enabled_defaults_false() {
        let a = AIAccount {
            name: "a".to_string(), provider: "claude".to_string(),
            daily_limit_usd: None, auto_switch: None, switch_on: None, cost_per_run: None,
        };
        assert!(!a.auto_switch_enabled());
    }

    #[test]
    fn test_ai_account_auto_switch_enabled_true() {
        let a = AIAccount {
            name: "a".to_string(), provider: "claude".to_string(),
            daily_limit_usd: None, auto_switch: Some(true), switch_on: None, cost_per_run: None,
        };
        assert!(a.auto_switch_enabled());
    }

    #[test]
    fn test_ai_account_switch_on_error_defaults_true_when_auto_switch_enabled() {
        let a = AIAccount {
            name: "a".to_string(), provider: "claude".to_string(),
            daily_limit_usd: None, auto_switch: Some(true), switch_on: None, cost_per_run: None,
        };
        assert!(a.switch_on_error());
    }

    #[test]
    fn test_ai_account_switch_on_error_false_when_auto_switch_disabled() {
        let a = AIAccount {
            name: "a".to_string(), provider: "claude".to_string(),
            daily_limit_usd: None, auto_switch: Some(false), switch_on: Some("error".to_string()), cost_per_run: None,
        };
        assert!(!a.switch_on_error());
    }

    #[test]
    fn test_ai_account_switch_on_error_false_when_switch_on_limit() {
        let a = AIAccount {
            name: "a".to_string(), provider: "claude".to_string(),
            daily_limit_usd: None, auto_switch: Some(true), switch_on: Some("limit".to_string()), cost_per_run: None,
        };
        assert!(!a.switch_on_error());
    }

    #[test]
    fn test_ai_account_switch_on_limit_true_when_all_set() {
        let a = AIAccount {
            name: "a".to_string(), provider: "claude".to_string(),
            daily_limit_usd: Some(10.0), auto_switch: Some(true),
            switch_on: Some("limit".to_string()), cost_per_run: Some(0.1),
        };
        assert!(a.switch_on_limit());
    }

    #[test]
    fn test_ai_account_switch_on_limit_false_when_cost_per_run_missing() {
        let a = AIAccount {
            name: "a".to_string(), provider: "claude".to_string(),
            daily_limit_usd: Some(10.0), auto_switch: Some(true),
            switch_on: Some("limit".to_string()), cost_per_run: None,
        };
        assert!(!a.switch_on_limit());
    }

    #[test]
    fn test_ai_account_switch_on_limit_false_when_daily_limit_missing() {
        let a = AIAccount {
            name: "a".to_string(), provider: "claude".to_string(),
            daily_limit_usd: None, auto_switch: Some(true),
            switch_on: Some("limit".to_string()), cost_per_run: Some(0.1),
        };
        assert!(!a.switch_on_limit());
    }

    #[test]
    fn test_ai_account_switch_on_both_enables_error_and_limit() {
        let a = AIAccount {
            name: "a".to_string(), provider: "claude".to_string(),
            daily_limit_usd: Some(5.0), auto_switch: Some(true),
            switch_on: Some("both".to_string()), cost_per_run: Some(0.1),
        };
        assert!(a.switch_on_error());
        assert!(a.switch_on_limit());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

```powershell
cd D:\demo\Conduit
cargo test -p conduit-core test_ai_account 2>&1
```

Expected: compile error — `AIAccount` struct has no `auto_switch` field, `auto_switch_enabled` method doesn't exist.

- [ ] **Step 3: Add `chrono` to `conduit-core/Cargo.toml`**

In `conduit-core/Cargo.toml`, add to `[dependencies]`:

```toml
chrono = { version = "0.4", features = ["serde"] }
```

Final `[dependencies]` block:

```toml
[dependencies]
serde = { version = "1", features = ["derive"] }
toml = "0.8"
thiserror = "1"
which = "6"
dirs = "5"
rayon = "1"
chrono = { version = "0.4", features = ["serde"] }
```

- [ ] **Step 4: Update `AIAccount` struct and add helper methods in `conduit-core/src/config.rs`**

Replace the existing `AIAccount` struct (lines 17–22):

```rust
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AIAccount {
    pub name: String,
    pub provider: String,
    pub daily_limit_usd: Option<f64>,
    pub auto_switch: Option<bool>,
    pub switch_on: Option<String>,
    pub cost_per_run: Option<f64>,
}

impl AIAccount {
    pub fn auto_switch_enabled(&self) -> bool {
        self.auto_switch.unwrap_or(false)
    }

    pub fn switch_on_error(&self) -> bool {
        matches!(self.switch_on.as_deref(), Some("error") | Some("both") | None)
            && self.auto_switch_enabled()
    }

    pub fn switch_on_limit(&self) -> bool {
        matches!(self.switch_on.as_deref(), Some("limit") | Some("both"))
            && self.auto_switch_enabled()
            && self.cost_per_run.is_some()
            && self.daily_limit_usd.is_some()
    }
}
```

- [ ] **Step 5: Update existing `AIAccount` struct literal in `conduit-core/src/config.rs` test**

In the test `test_save_and_reload_global_config`, update the struct literal to include the new fields:

```rust
        let config = Config {
            ai_account: vec![AIAccount {
                name: "my-claude".to_string(),
                provider: "claude".to_string(),
                daily_limit_usd: None,
                auto_switch: None,
                switch_on: None,
                cost_per_run: None,
            }],
            ..Config::default()
        };
```

- [ ] **Step 6: Update `AIAccount` struct literal in `conduit-core/src/provider.rs` test helper**

In the test helper `make_config` in `conduit-core/src/provider.rs`, update:

```rust
    fn make_config(account_name: &str, provider: &str) -> Config {
        Config {
            ai_account: vec![AIAccount {
                name: account_name.to_string(),
                provider: provider.to_string(),
                daily_limit_usd: None,
                auto_switch: None,
                switch_on: None,
                cost_per_run: None,
            }],
            ..Config::default()
        }
    }
```

- [ ] **Step 7: Update `AIAccount` struct literal in `conduit-cli/src/commands/providers.rs`**

In the `add()` function, update the `config.ai_account.push(...)` call:

```rust
    config.ai_account.push(AIAccount {
        name: name.clone(),
        provider: provider_type.to_string(),
        daily_limit_usd: None,
        auto_switch: None,
        switch_on: None,
        cost_per_run: None,
    });
```

- [ ] **Step 8: Run tests to verify they pass**

```powershell
cargo test -p conduit-core 2>&1
```

Expected: all tests pass (includes the new 9 `test_ai_account_*` tests).

- [ ] **Step 9: Commit**

```powershell
git add conduit-core/Cargo.toml conduit-core/src/config.rs conduit-core/src/provider.rs conduit-cli/src/commands/providers.rs
git commit -m "feat: add AIAccount auto_switch/switch_on/cost_per_run fields with helper methods"
```

---

## Task 2: `AllAccountsExhausted` error variant

**Files:**
- Modify: `conduit-core/src/error.rs`

- [ ] **Step 1: Write failing test**

Add to the `#[cfg(test)] mod tests` in `conduit-core/src/error.rs`:

```rust
    #[test]
    fn test_all_accounts_exhausted_message() {
        let e = ConduitError::AllAccountsExhausted { stage: "doc".to_string() };
        let msg = e.to_string();
        assert!(msg.contains("doc"));
        assert!(msg.contains("exhausted") || msg.contains("All accounts"));
    }
```

- [ ] **Step 2: Run test to verify it fails**

```powershell
cargo test -p conduit-core test_all_accounts_exhausted 2>&1
```

Expected: compile error — variant `AllAccountsExhausted` does not exist.

- [ ] **Step 3: Add the variant to `ConduitError`**

Add after the `ConfigSerializeError` variant in `conduit-core/src/error.rs`:

```rust
    #[error("All accounts exhausted for stage `{stage}`: every configured account is over its limit or failed")]
    AllAccountsExhausted { stage: String },
```

- [ ] **Step 4: Run tests to verify they pass**

```powershell
cargo test -p conduit-core 2>&1
```

Expected: all tests pass.

- [ ] **Step 5: Commit**

```powershell
git add conduit-core/src/error.rs
git commit -m "feat: add AllAccountsExhausted error variant"
```

---

## Task 3: `SpendTracker`

**Files:**
- Create: `conduit-core/src/spend.rs`
- Modify: `conduit-core/src/lib.rs`

- [ ] **Step 1: Add `pub mod spend` to `conduit-core/src/lib.rs`**

Replace the contents of `conduit-core/src/lib.rs`:

```rust
pub mod config;
pub mod error;
pub mod parallel;
pub mod pipeline;
pub mod provider;
pub mod spend;
pub mod tasks;
```

- [ ] **Step 2: Write failing tests — create `conduit-core/src/spend.rs` with tests only**

Create `conduit-core/src/spend.rs` with the test module first (no implementation yet):

```rust
use crate::config::AIAccount;
use crate::error::ConduitError;
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

fn today_key() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

fn spend_path() -> Result<PathBuf, ConduitError> {
    if let Ok(path) = std::env::var("CONDUIT_GLOBAL_CONFIG") {
        let config_path = PathBuf::from(path);
        if let Some(parent) = config_path.parent() {
            return Ok(parent.join("spend.toml"));
        }
    }
    let home = dirs::home_dir().ok_or(ConduitError::GlobalConfigDirNotFound)?;
    Ok(home.join(".conduit").join("spend.toml"))
}

pub struct SpendTracker {
    // TODO
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn account_with_cost(name: &str, cost_per_run: f64, daily_limit_usd: f64) -> AIAccount {
        AIAccount {
            name: name.to_string(),
            provider: "claude".to_string(),
            daily_limit_usd: Some(daily_limit_usd),
            auto_switch: Some(true),
            switch_on: Some("both".to_string()),
            cost_per_run: Some(cost_per_run),
        }
    }

    fn account_no_cost(name: &str) -> AIAccount {
        AIAccount {
            name: name.to_string(),
            provider: "claude".to_string(),
            daily_limit_usd: Some(10.0),
            auto_switch: None,
            switch_on: None,
            cost_per_run: None,
        }
    }

    #[test]
    fn test_load_missing_file_returns_empty_tracker() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "").unwrap();
        std::env::set_var("CONDUIT_GLOBAL_CONFIG", path.to_str().unwrap());
        let result = SpendTracker::load();
        std::env::remove_var("CONDUIT_GLOBAL_CONFIG");
        let tracker = result.expect("load should succeed even with no spend.toml");
        assert_eq!(tracker.today_invocations("any-account"), 0);
    }

    #[test]
    fn test_record_increments_today_count() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "").unwrap();
        std::env::set_var("CONDUIT_GLOBAL_CONFIG", path.to_str().unwrap());
        let mut tracker = SpendTracker::load().unwrap();
        tracker.record("claude-work").unwrap();
        tracker.record("claude-work").unwrap();
        let count = tracker.today_invocations("claude-work");
        std::env::remove_var("CONDUIT_GLOBAL_CONFIG");
        assert_eq!(count, 2);
    }

    #[test]
    fn test_is_over_limit_false_when_cost_per_run_not_set() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "").unwrap();
        std::env::set_var("CONDUIT_GLOBAL_CONFIG", path.to_str().unwrap());
        let mut tracker = SpendTracker::load().unwrap();
        for _ in 0..100 {
            tracker.record("acct").unwrap();
        }
        let account = account_no_cost("acct");
        let over = tracker.is_over_limit(&account);
        std::env::remove_var("CONDUIT_GLOBAL_CONFIG");
        assert!(!over, "should not be over limit when cost_per_run is None");
    }

    #[test]
    fn test_is_over_limit_true_when_spend_exceeds_daily_limit() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "").unwrap();
        std::env::set_var("CONDUIT_GLOBAL_CONFIG", path.to_str().unwrap());
        let mut tracker = SpendTracker::load().unwrap();
        // 11 invocations × $0.10 = $1.10 >= $1.00 daily limit
        for _ in 0..11 {
            tracker.record("acct").unwrap();
        }
        let account = account_with_cost("acct", 0.10, 1.00);
        let over = tracker.is_over_limit(&account);
        std::env::remove_var("CONDUIT_GLOBAL_CONFIG");
        assert!(over);
    }

    #[test]
    fn test_yesterday_invocations_do_not_count_toward_today() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "").unwrap();
        std::env::set_var("CONDUIT_GLOBAL_CONFIG", path.to_str().unwrap());
        // Manually write a spend.toml with yesterday's data
        use chrono::Local;
        let yesterday = (Local::now() - chrono::Duration::days(1))
            .format("%Y-%m-%d").to_string();
        let spend_toml = format!("[{}]\nacct = 999\n", yesterday);
        let spend_path = dir.path().join("spend.toml");
        std::fs::write(&spend_path, spend_toml).unwrap();
        let tracker = SpendTracker::load().unwrap();
        let today_count = tracker.today_invocations("acct");
        std::env::remove_var("CONDUIT_GLOBAL_CONFIG");
        assert_eq!(today_count, 0, "yesterday's invocations must not count toward today");
    }

    #[test]
    fn test_save_and_reload_round_trip() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "").unwrap();
        std::env::set_var("CONDUIT_GLOBAL_CONFIG", path.to_str().unwrap());
        let mut tracker = SpendTracker::load().unwrap();
        tracker.record("acct-a").unwrap();
        tracker.record("acct-a").unwrap();
        tracker.record("acct-b").unwrap();
        // Reload from disk
        let reloaded = SpendTracker::load().unwrap();
        let a_count = reloaded.today_invocations("acct-a");
        let b_count = reloaded.today_invocations("acct-b");
        std::env::remove_var("CONDUIT_GLOBAL_CONFIG");
        assert_eq!(a_count, 2);
        assert_eq!(b_count, 1);
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

```powershell
cargo test -p conduit-core spend 2>&1
```

Expected: compile errors — `SpendTracker` struct is a stub, methods don't exist.

- [ ] **Step 4: Implement `SpendTracker` in `conduit-core/src/spend.rs`**

Replace the stub `SpendTracker` struct and add the full implementation. The file should now look like this (keep the test module at the bottom exactly as written in Step 2):

```rust
use crate::config::AIAccount;
use crate::error::ConduitError;
use chrono::Local;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

fn today_key() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

fn spend_path() -> Result<PathBuf, ConduitError> {
    if let Ok(path) = std::env::var("CONDUIT_GLOBAL_CONFIG") {
        let config_path = PathBuf::from(path);
        if let Some(parent) = config_path.parent() {
            return Ok(parent.join("spend.toml"));
        }
    }
    let home = dirs::home_dir().ok_or(ConduitError::GlobalConfigDirNotFound)?;
    Ok(home.join(".conduit").join("spend.toml"))
}

pub struct SpendTracker {
    path: PathBuf,
    data: BTreeMap<String, HashMap<String, u64>>,
}

impl SpendTracker {
    pub fn load() -> Result<Self, ConduitError> {
        let path = spend_path()?;
        if !path.exists() {
            return Ok(Self { path, data: BTreeMap::new() });
        }
        let content = fs::read_to_string(&path)?;
        let data: BTreeMap<String, HashMap<String, u64>> =
            toml::from_str(&content).unwrap_or_default();
        Ok(Self { path, data })
    }

    pub fn record(&mut self, account: &str) -> Result<(), ConduitError> {
        let today = today_key();
        *self
            .data
            .entry(today)
            .or_default()
            .entry(account.to_string())
            .or_insert(0) += 1;
        self.save()
    }

    pub fn today_invocations(&self, account: &str) -> u64 {
        let today = today_key();
        self.data
            .get(&today)
            .and_then(|d| d.get(account))
            .copied()
            .unwrap_or(0)
    }

    pub fn estimated_spend(&self, account: &AIAccount) -> f64 {
        match account.cost_per_run {
            Some(cost) => self.today_invocations(&account.name) as f64 * cost,
            None => 0.0,
        }
    }

    pub fn is_over_limit(&self, account: &AIAccount) -> bool {
        match (account.daily_limit_usd, account.cost_per_run) {
            (Some(limit), Some(_)) => self.estimated_spend(account) >= limit,
            _ => false,
        }
    }

    pub fn save(&self) -> Result<(), ConduitError> {
        if self.path.as_os_str().is_empty() {
            return Ok(());
        }
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let toml_str = toml::to_string_pretty(&self.data)
            .map_err(|e| ConduitError::ConfigSerializeError(e.to_string()))?;
        fs::write(&self.path, toml_str)?;
        Ok(())
    }

    /// Creates an in-memory tracker with no backing file. Saves are no-ops.
    /// Used when loading fails (permissions, etc.) — spend is not tracked but run continues.
    pub fn new_empty() -> Self {
        Self { path: PathBuf::new(), data: BTreeMap::new() }
    }
}
```

Note: `new_empty()` has `path = PathBuf::new()` (empty string path). `save()` checks for this and returns `Ok(())` silently, so records don't persist but the run doesn't fail.

- [ ] **Step 5: Run tests to verify they pass**

```powershell
cargo test -p conduit-core 2>&1
```

Expected: all tests pass, including the 6 new `spend::tests::*` tests.

- [ ] **Step 6: Commit**

```powershell
git add conduit-core/src/spend.rs conduit-core/src/lib.rs conduit-core/Cargo.toml
git commit -m "feat: add SpendTracker for daily invocation tracking in spend.toml"
```

---

## Task 4: `FallbackProvider` + `FallbackResolver`

**Files:**
- Modify: `conduit-core/src/provider.rs`

The core Phase 5 logic lives here. `FallbackProvider` implements `Provider` and does the per-invocation failover. `FallbackResolver` implements `ProviderResolver` and builds the candidate list.

- [ ] **Step 1: Write failing tests**

Add the following to the `#[cfg(test)] mod tests` block in `conduit-core/src/provider.rs` (after the existing tests):

```rust
    // ---- FallbackProvider tests ----

    use crate::spend::SpendTracker;
    use std::sync::{Arc, Mutex};

    fn make_account_full(
        name: &str, provider: &str,
        auto_switch: Option<bool>, switch_on: Option<&str>,
        cost_per_run: Option<f64>, daily_limit_usd: Option<f64>,
    ) -> AIAccount {
        AIAccount {
            name: name.to_string(),
            provider: provider.to_string(),
            daily_limit_usd,
            auto_switch,
            switch_on: switch_on.map(|s| s.to_string()),
            cost_per_run,
        }
    }

    #[derive(Debug)]
    struct OkProvider { response: String }
    impl Provider for OkProvider {
        fn name(&self) -> &str { "ok" }
        fn invoke(&self, _s: &str, _p: &str, _d: &Path) -> Result<String, ConduitError> {
            Ok(self.response.clone())
        }
    }

    #[derive(Debug)]
    struct FailProvider;
    impl Provider for FailProvider {
        fn name(&self) -> &str { "fail" }
        fn invoke(&self, stage: &str, _p: &str, _d: &Path) -> Result<String, ConduitError> {
            Err(ConduitError::AgentInvocationFailed {
                provider: "fail".to_string(),
                stage: stage.to_string(),
                reason: "test failure".to_string(),
            })
        }
    }

    fn temp_spend() -> (tempfile::TempDir, Arc<Mutex<SpendTracker>>) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spend.toml");
        // SpendTracker::new_empty() has no backing file — saves are no-ops.
        // For tests that need real recording, use a tracker loaded from temp dir.
        let tracker = SpendTracker::new_empty();
        (dir, Arc::new(Mutex::new(tracker)))
    }

    fn temp_spend_with_dir() -> (tempfile::TempDir, Arc<Mutex<SpendTracker>>) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spend.toml");
        // Create an empty file so the path is valid for round-trip saves
        std::fs::write(&path, "").unwrap();
        // We build with new_empty so we control the path
        let tracker = crate::spend::SpendTracker::new_empty_at(path);
        (dir, Arc::new(Mutex::new(tracker)))
    }

    fn make_fp(
        candidates: Vec<(AIAccount, Box<dyn Provider>)>,
        spend: Arc<Mutex<SpendTracker>>,
    ) -> FallbackProvider {
        FallbackProvider { candidates, spend, stage_name: "doc".to_string() }
    }

    #[test]
    fn test_fallback_auto_switch_disabled_returns_error_no_fallback() {
        let acct_a = make_account_full("acct-a", "claude", Some(false), None, None, None);
        let acct_b = make_account_full("acct-b", "claude", Some(true), Some("error"), None, None);
        let (_dir, spend) = temp_spend();
        let fp = make_fp(
            vec![
                (acct_a, Box::new(FailProvider)),
                (acct_b, Box::new(OkProvider { response: "second".to_string() })),
            ],
            spend,
        );
        let dir = tempdir().unwrap();
        let result = fp.invoke("doc", "prompt", dir.path());
        assert!(
            matches!(result, Err(ConduitError::AgentInvocationFailed { .. })),
            "auto_switch=false must return error immediately, not fall back to acct-b"
        );
    }

    #[test]
    fn test_fallback_switch_on_error_tries_next_on_failure() {
        let acct_a = make_account_full("acct-a", "claude", Some(true), Some("error"), None, None);
        let acct_b = make_account_full("acct-b", "claude", Some(true), Some("error"), None, None);
        let (_dir, spend) = temp_spend();
        let fp = make_fp(
            vec![
                (acct_a, Box::new(FailProvider)),
                (acct_b, Box::new(OkProvider { response: "second".to_string() })),
            ],
            spend,
        );
        let dir = tempdir().unwrap();
        let result = fp.invoke("doc", "prompt", dir.path());
        assert_eq!(result.unwrap(), "second");
    }

    #[test]
    fn test_fallback_switch_on_limit_skips_over_limit_account() {
        let acct_a = make_account_full("acct-a", "claude", Some(true), Some("limit"), Some(0.10), Some(0.50));
        let acct_b = make_account_full("acct-b", "claude", Some(true), Some("limit"), None, None);
        let (_dir, mut raw_spend) = temp_spend_with_dir();
        // Pre-record 6 invocations: 6 × $0.10 = $0.60 >= $0.50 → over limit
        {
            let spend = Arc::clone(&raw_spend);
            let mut s = spend.lock().unwrap();
            for _ in 0..6 {
                s.record("acct-a").unwrap();
            }
        }
        let fp = make_fp(
            vec![
                (acct_a, Box::new(OkProvider { response: "a".to_string() })),
                (acct_b, Box::new(OkProvider { response: "b".to_string() })),
            ],
            raw_spend,
        );
        let dir = tempdir().unwrap();
        let result = fp.invoke("doc", "prompt", dir.path());
        assert_eq!(result.unwrap(), "b", "over-limit account must be skipped");
    }

    #[test]
    fn test_fallback_switch_on_limit_no_failover_on_runtime_error() {
        let acct_a = make_account_full("acct-a", "claude", Some(true), Some("limit"), None, None);
        let acct_b = make_account_full("acct-b", "claude", Some(true), Some("limit"), None, None);
        let (_dir, spend) = temp_spend();
        let fp = make_fp(
            vec![
                (acct_a, Box::new(FailProvider)),
                (acct_b, Box::new(OkProvider { response: "b".to_string() })),
            ],
            spend,
        );
        let dir = tempdir().unwrap();
        let result = fp.invoke("doc", "prompt", dir.path());
        assert!(
            matches!(result, Err(ConduitError::AgentInvocationFailed { .. })),
            "switch_on=limit must not fall back on runtime errors"
        );
    }

    #[test]
    fn test_fallback_switch_on_both_skips_limit_and_falls_over_on_error() {
        let acct_a = make_account_full("acct-a", "claude", Some(true), Some("both"), Some(0.10), Some(0.50));
        let acct_b = make_account_full("acct-b", "claude", Some(true), Some("both"), None, None);
        let acct_c = make_account_full("acct-c", "openai", Some(true), Some("both"), None, None);
        let (_dir, mut raw_spend) = temp_spend_with_dir();
        {
            let spend = Arc::clone(&raw_spend);
            let mut s = spend.lock().unwrap();
            for _ in 0..6 { s.record("acct-a").unwrap(); } // acct-a over limit
        }
        let fp = make_fp(
            vec![
                (acct_a, Box::new(OkProvider { response: "a".to_string() })),
                (acct_b, Box::new(FailProvider)),  // acct-b: runtime error
                (acct_c, Box::new(OkProvider { response: "c".to_string() })),
            ],
            raw_spend,
        );
        let dir = tempdir().unwrap();
        let result = fp.invoke("doc", "prompt", dir.path());
        assert_eq!(result.unwrap(), "c", "both: skip limit account, fall over on error, succeed on third");
    }

    #[test]
    fn test_fallback_all_exhausted_returns_all_accounts_exhausted_error() {
        let acct = make_account_full("acct-a", "claude", Some(true), Some("error"), None, None);
        let (_dir, spend) = temp_spend();
        let fp = make_fp(
            vec![(acct, Box::new(FailProvider))],
            spend,
        );
        let dir = tempdir().unwrap();
        let result = fp.invoke("doc", "prompt", dir.path());
        assert!(
            matches!(result, Err(ConduitError::AllAccountsExhausted { ref stage }) if stage == "doc"),
            "all candidates exhausted must return AllAccountsExhausted"
        );
    }

    #[test]
    fn test_build_candidate_order_same_provider_first_then_others() {
        let config = Config {
            ai_account: vec![
                make_account_full("claude-a", "claude", None, None, None, None),
                make_account_full("openai-a", "openai", None, None, None, None),
                make_account_full("claude-b", "claude", None, None, None, None),
                make_account_full("gemini-a", "gemini", None, None, None, None),
            ],
            ..Config::default()
        };
        let order = build_candidate_order("claude-a", &config);
        assert_eq!(order[0].name, "claude-a", "primary first");
        assert_eq!(order[1].name, "claude-b", "same-provider second");
        assert_eq!(order.len(), 4, "all accounts in list");
        // openai-a and gemini-a are in positions 2 and 3 (any order)
        let tail: Vec<&str> = order[2..].iter().map(|a| a.name.as_str()).collect();
        assert!(tail.contains(&"openai-a"));
        assert!(tail.contains(&"gemini-a"));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

```powershell
cargo test -p conduit-core fallback 2>&1
cargo test -p conduit-core build_candidate 2>&1
```

Expected: compile errors — `FallbackProvider`, `FallbackResolver`, `build_candidate_order` don't exist.

- [ ] **Step 3: Add `new_empty_at` constructor to `SpendTracker` in `conduit-core/src/spend.rs`**

After the existing `new_empty()` method, add:

```rust
    /// Creates an in-memory tracker backed by the given path.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn new_empty_at(path: PathBuf) -> Self {
        Self { path, data: BTreeMap::new() }
    }
```

- [ ] **Step 4: Implement `FallbackProvider`, `build_candidate_order`, and `FallbackResolver` in `conduit-core/src/provider.rs`**

Add the following imports at the top of `conduit-core/src/provider.rs` (after existing `use` lines):

```rust
use crate::spend::SpendTracker;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
```

Add `build_candidate_order` function (before the `#[cfg(test)]` block):

```rust
pub fn build_candidate_order<'a>(primary_name: &str, config: &'a Config) -> Vec<&'a AIAccount> {
    let primary = config.ai_account.iter().find(|a| a.name == primary_name);
    let primary_provider = primary.map(|a| a.provider.as_str());

    let mut seen: HashSet<String> = HashSet::new();
    let mut order: Vec<&AIAccount> = Vec::new();

    if let Some(acc) = primary {
        seen.insert(acc.name.clone());
        order.push(acc);
    }

    for acc in &config.ai_account {
        if !seen.contains(&acc.name) && Some(acc.provider.as_str()) == primary_provider {
            seen.insert(acc.name.clone());
            order.push(acc);
        }
    }

    for acc in &config.ai_account {
        if !seen.contains(&acc.name) {
            seen.insert(acc.name.clone());
            order.push(acc);
        }
    }

    order
}
```

Add `build_provider` helper (before `#[cfg(test)]` block):

```rust
fn build_provider(account: &AIAccount) -> Result<Box<dyn Provider>, ConduitError> {
    match account.provider.as_str() {
        "claude" if which::which("claude").is_ok() => Ok(Box::new(ClaudeProvider)),
        "openai" if which::which("codex").is_ok() => Ok(Box::new(CodexProvider)),
        "gemini" if which::which("gemini").is_ok() => Ok(Box::new(GeminiProvider)),
        _ => Err(ConduitError::NoProviderAvailable),
    }
}
```

Add `FallbackProvider` struct and its `Provider` impl (before `#[cfg(test)]` block):

```rust
pub struct FallbackProvider {
    pub candidates: Vec<(AIAccount, Box<dyn Provider>)>,
    pub spend: Arc<Mutex<SpendTracker>>,
    pub stage_name: String,
}

impl std::fmt::Debug for FallbackProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FallbackProvider")
            .field("stage_name", &self.stage_name)
            .field("num_candidates", &self.candidates.len())
            .finish()
    }
}

impl Provider for FallbackProvider {
    fn name(&self) -> &str { "fallback" }

    fn invoke(&self, stage: &str, prompt: &str, work_dir: &Path) -> Result<String, ConduitError> {
        for (account, provider) in &self.candidates {
            // Step 3a: auto_switch disabled — use this provider, return immediately, no fallback
            if !account.auto_switch_enabled() {
                let result = provider.invoke(stage, prompt, work_dir);
                return result;
            }

            // Step 3b: check daily limit before invoking
            if account.switch_on_limit() {
                let spend = self.spend.lock().unwrap();
                if spend.is_over_limit(account) {
                    eprintln!("[{}] skipped: over daily limit", account.name);
                    continue;
                }
            }

            // Step 3c: invoke the provider
            match provider.invoke(stage, prompt, work_dir) {
                Ok(output) => {
                    let mut spend = self.spend.lock().unwrap();
                    let _ = spend.record(&account.name);
                    return Ok(output);
                }
                Err(e @ ConduitError::AgentInvocationFailed { .. }) => {
                    {
                        let mut spend = self.spend.lock().unwrap();
                        let _ = spend.record(&account.name);
                    }
                    if account.switch_on_error() {
                        continue;
                    } else {
                        return Err(e);
                    }
                }
                Err(e) => return Err(e),
            }
        }

        Err(ConduitError::AllAccountsExhausted { stage: self.stage_name.clone() })
    }
}
```

Add `FallbackResolver` struct and its `ProviderResolver` impl (before `#[cfg(test)]` block):

```rust
pub struct FallbackResolver<'a> {
    pub profile: &'a Profile,
    pub config: &'a Config,
    pub spend: Arc<Mutex<SpendTracker>>,
    pub account_override: Option<&'a str>,
}

impl<'a> FallbackResolver<'a> {
    pub fn new(
        profile: &'a Profile,
        config: &'a Config,
        spend: Arc<Mutex<SpendTracker>>,
        account_override: Option<&'a str>,
    ) -> Self {
        Self { profile, config, spend, account_override }
    }
}

impl<'a> std::fmt::Debug for FallbackResolver<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FallbackResolver")
            .field("account_override", &self.account_override)
            .finish()
    }
}

impl<'a> ProviderResolver for FallbackResolver<'a> {
    fn resolve(&self, stage: &Stage) -> Result<Box<dyn Provider>, ConduitError> {
        let primary_name = if let Some(ov) = self.account_override {
            ov
        } else {
            self.profile
                .account_for_stage(stage.name())
                .ok_or_else(|| ConduitError::ProfileIncomplete(self.profile.name.clone()))?
        };

        // Verify primary exists in config
        if !self.config.ai_account.iter().any(|a| a.name == primary_name) {
            return Err(ConduitError::ProfileProviderNotConfigured {
                account: primary_name.to_string(),
                profile: self.profile.name.clone(),
            });
        }

        let ordered = build_candidate_order(primary_name, self.config);

        let candidates: Vec<(AIAccount, Box<dyn Provider>)> = ordered
            .into_iter()
            .filter_map(|acc| build_provider(acc).ok().map(|p| (acc.clone(), p)))
            .collect();

        if candidates.is_empty() {
            return Err(ConduitError::NoProviderAvailable);
        }

        Ok(Box::new(FallbackProvider {
            candidates,
            spend: Arc::clone(&self.spend),
            stage_name: stage.name().to_string(),
        }))
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

```powershell
cargo test -p conduit-core 2>&1
```

Expected: all tests pass, including the 7 new `fallback_*` and `build_candidate_order` tests.

- [ ] **Step 6: Commit**

```powershell
git add conduit-core/src/provider.rs conduit-core/src/spend.rs
git commit -m "feat: add FallbackProvider and FallbackResolver for per-account failover"
```

---

## Task 5: `conduit run --account` flag

**Files:**
- Modify: `conduit-cli/src/main.rs`
- Modify: `conduit-cli/src/commands/run.rs`

- [ ] **Step 1: Add `--account` arg to `Run` in `conduit-cli/src/main.rs`**

Replace the `Run` variant in the `Commands` enum:

```rust
    /// Run tasks from tasks.toml
    Run {
        #[arg(long, help = "Run a specific task by id")]
        task: Option<String>,
        #[arg(long, help = "Use a named run profile (skips interactive selection)")]
        profile: Option<String>,
        #[arg(long, help = "Maximum number of tasks to run simultaneously (default: all)")]
        concurrency: Option<usize>,
        #[arg(long, help = "Override starting account for all stages")]
        account: Option<String>,
    },
```

Update the dispatch in `fn run()`:

```rust
        Commands::Run { task, profile, concurrency, account } => {
            commands::run::run(&cwd, task.as_deref(), profile.as_deref(), concurrency, account.as_deref())
        }
```

- [ ] **Step 2: Update `conduit-cli/src/commands/run.rs`**

Replace the entire file with the updated version. Key changes: new `account` parameter, early validation, `SpendTracker` load, `FallbackResolver` instead of `ProfileResolver`.

```rust
use anyhow::Result;
use colored::Colorize;
use conduit_core::{
    config::{load_global_config, save_global_config, Config, Profile},
    error::ConduitError,
    parallel::{ParallelRunner, TaskEvent},
    pipeline::PipelineRunner,
    provider::{FallbackResolver, ProfileResolver},
    spend::SpendTracker,
    tasks::load_tasks,
};
use dialoguer::{Input, Select};
use std::path::Path;
use std::sync::{Arc, Mutex};

pub fn run(
    dir: &Path,
    task_id: Option<&str>,
    profile_name: Option<&str>,
    concurrency: Option<usize>,
    account: Option<&str>,
) -> Result<()> {
    if let Some(0) = concurrency {
        anyhow::bail!("--concurrency must be at least 1");
    }

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

    // Validate --account before loading tasks (fast fail)
    if let Some(acc_name) = account {
        if !config.ai_account.iter().any(|a| a.name == acc_name) {
            anyhow::bail!(
                "Account '{}' not found. Run `conduit providers list` to see available accounts.",
                acc_name
            );
        }
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

    // Load spend tracker; fall back to in-memory if file unreadable
    let spend = Arc::new(Mutex::new(
        SpendTracker::load().unwrap_or_else(|_| SpendTracker::new_empty()),
    ));

    let resolver = FallbackResolver::new(&profile, &config, Arc::clone(&spend), account);
    let concurrency = concurrency.unwrap_or(tasks.len().max(1));
    let use_parallel = tasks.len() > 1 && concurrency > 1;

    if use_parallel {
        let print_lock = Arc::new(Mutex::new(()));
        let runner = ParallelRunner::new(&tasks, &resolver, dir, concurrency);
        let results = runner.run(|event| {
            let _guard = print_lock.lock().unwrap();
            match event {
                TaskEvent::Started(id) => println!("[{}] running...", id),
                TaskEvent::StageComplete { task_id, completed, total, stage } => {
                    println!("[{}]   [{}/{}] {}  {}", task_id, completed, total, stage, "✓".green());
                }
                TaskEvent::Finished(id) => {
                    println!("[{}] {} {}", id, "done".green().bold(), "✓".green());
                }
                TaskEvent::Failed { task_id, error } => {
                    eprintln!("[{}] {}  {}", task_id, "✗".red(), error);
                }
            }
        });

        let failed_count = results.iter().filter(|r| r.error.is_some()).count();
        let completed_count = results.len() - failed_count;
        if failed_count > 0 {
            println!("\nResults: {} completed, {} failed.", completed_count, failed_count);
            anyhow::bail!("{} task(s) failed", failed_count);
        }
    } else {
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

- [ ] **Step 3: Verify the full test suite still passes**

```powershell
cargo test 2>&1
```

Expected: all tests pass (57 existing + new core tests).

- [ ] **Step 4: Commit**

```powershell
git add conduit-cli/src/main.rs conduit-cli/src/commands/run.rs
git commit -m "feat: add --account flag to conduit run, wire FallbackResolver"
```

---

## Task 6: `conduit providers add` extended prompts

**Files:**
- Modify: `conduit-cli/src/commands/providers.rs`

After successful login and name entry, prompt the user for auto-switch settings. The new prompts happen between the existing `break name;` and `config.ai_account.push(...)`.

- [ ] **Step 1: Replace the `add()` function in `conduit-cli/src/commands/providers.rs`**

Replace the entire `add()` function (lines 50–101) with:

```rust
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
        .args(login_args)
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

    // --- Phase 5: auto-switch prompts ---
    let enable_auto_switch = Select::new()
        .with_prompt("Enable auto-switch for this account?")
        .items(&["No", "Yes"])
        .default(0)
        .interact()?;

    let (auto_switch, switch_on, daily_limit_usd, cost_per_run) = if enable_auto_switch == 0 {
        (Some(false), None, None, None)
    } else {
        let switch_options = [
            "On error (provider CLI fails)",
            "On daily limit (estimated spend reaches daily_limit_usd)",
            "Both",
        ];
        let switch_idx = Select::new()
            .with_prompt("Switch when?")
            .items(&switch_options)
            .default(0)
            .interact()?;

        let switch_on_str = match switch_idx {
            0 => "error",
            1 => "limit",
            _ => "both",
        };

        let needs_cost = switch_idx == 1 || switch_idx == 2;

        let daily_limit = if needs_cost {
            let limit_str: String = Input::new()
                .with_prompt("Daily limit (USD, e.g. 10.0)")
                .interact_text()?;
            limit_str.parse::<f64>().ok()
        } else {
            None
        };

        let cost = if needs_cost {
            let cost_str: String = Input::new()
                .with_prompt("Estimated cost per stage run (USD, e.g. 0.05)")
                .interact_text()?;
            cost_str.parse::<f64>().ok()
        } else {
            None
        };

        (Some(true), Some(switch_on_str.to_string()), daily_limit, cost)
    };

    config.ai_account.push(AIAccount {
        name: name.clone(),
        provider: provider_type.to_string(),
        daily_limit_usd,
        auto_switch,
        switch_on,
        cost_per_run,
    });

    save_global_config(&config)?;
    println!("{} Account \"{}\" ({}) added.", "✓".green(), name, provider_type);
    Ok(())
}
```

- [ ] **Step 2: Verify the project still compiles and tests pass**

```powershell
cargo test 2>&1
```

Expected: all tests pass.

- [ ] **Step 3: Commit**

```powershell
git add conduit-cli/src/commands/providers.rs
git commit -m "feat: add auto-switch prompts to conduit providers add"
```

---

## Task 7: `conduit status` spend display

**Files:**
- Modify: `conduit-cli/src/commands/status.rs`

The updated output format:
```
AI Accounts:
  claude-work  (claude)  $10.00/day  ~$0.70 used today  auto-switch: both
  openai-personal  (openai)  no limit  auto-switch: off
  gemini-free  (gemini)  $5.00/day  cost tracking not configured  auto-switch: error
```

- [ ] **Step 1: Replace `conduit-cli/src/commands/status.rs`**

```rust
use anyhow::Result;
use colored::Colorize;
use conduit_core::{config::load_global_config, spend::SpendTracker};

pub fn run() -> Result<()> {
    let config = load_global_config()?;
    let spend = SpendTracker::load().unwrap_or_else(|_| SpendTracker::new_empty());

    println!("{}:", "AI Accounts".bold());
    if config.ai_account.is_empty() {
        println!("  (none configured)");
    } else {
        for account in &config.ai_account {
            let limit_str = account
                .daily_limit_usd
                .map(|l| format!("  ${:.2}/day", l))
                .unwrap_or_else(|| "  no limit".to_string());

            let spend_str = if account.cost_per_run.is_some() {
                let est = spend.estimated_spend(account);
                format!("  ~${:.2} used today", est)
            } else if account.daily_limit_usd.is_some() {
                "  cost tracking not configured".to_string()
            } else {
                String::new()
            };

            let switch_str = match account.auto_switch {
                Some(true) => {
                    let trigger = account.switch_on.as_deref().unwrap_or("error");
                    format!("  auto-switch: {}", trigger)
                }
                _ => "  auto-switch: off".to_string(),
            };

            println!(
                "  {}  ({}){}{}{}",
                account.name.cyan(),
                account.provider,
                limit_str,
                spend_str,
                switch_str,
            );
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

- [ ] **Step 2: Verify the project compiles and tests pass**

```powershell
cargo test 2>&1
```

Expected: all tests pass.

- [ ] **Step 3: Commit**

```powershell
git add conduit-cli/src/commands/status.rs
git commit -m "feat: show estimated daily spend per account in conduit status"
```

---

## Task 8: Integration tests

**Files:**
- Modify: `conduit-cli/tests/cli.rs`

- [ ] **Step 1: Write the 3 new integration tests**

Add the following to `conduit-cli/tests/cli.rs` after the existing `-- run --` section:

```rust
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
```

- [ ] **Step 2: Run the new tests to verify they pass**

```powershell
cargo test -p conduit -- test_run_account test_status_shows 2>&1
```

Expected: all 3 new tests pass.

- [ ] **Step 3: Run the full test suite**

```powershell
cargo test 2>&1
```

Expected: all tests pass (57 existing + new tests from Tasks 1–8).

- [ ] **Step 4: Commit**

```powershell
git add conduit-cli/tests/cli.rs
git commit -m "test: add integration tests for --account flag and status spend display"
```

---

## Self-Review Checklist

Before declaring Phase 5 done, verify:

- [ ] `auto_switch`, `switch_on`, `cost_per_run` fields configurable in `~/.conduit/config.toml`
- [ ] `conduit providers add` prompts for auto-switch settings
- [ ] `SpendTracker` records invocations per account per day in `~/.conduit/spend.toml`
- [ ] `FallbackResolver` respects `switch_on = "error"`, `"limit"`, `"both"`
- [ ] Failover tries same-provider accounts first, then any provider
- [ ] `conduit run --account <name>` overrides starting account
- [ ] `conduit status` shows estimated daily spend per account
- [ ] `AllAccountsExhausted` error when no candidate succeeds
- [ ] All Phase 1–4 tests still pass
- [ ] New unit and integration tests for Phase 5 pass
