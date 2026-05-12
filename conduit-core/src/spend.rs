use crate::config::AIAccount;
use crate::error::ConduitError;
use chrono::Local;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::PathBuf;

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

    /// Creates an in-memory tracker backed by the given path.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn new_empty_at(path: PathBuf) -> Self {
        Self { path, data: BTreeMap::new() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
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
