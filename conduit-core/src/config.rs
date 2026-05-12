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
    use std::sync::Mutex;
    use tempfile::tempdir;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_temp_config(content: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, content).unwrap();
        (dir, path)
    }

    #[test]
    fn test_global_config_path_from_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        let (_dir, path) = with_temp_config("");
        std::env::set_var("CONDUIT_GLOBAL_CONFIG", path.to_str().unwrap());
        let result = global_config_path().unwrap();
        std::env::remove_var("CONDUIT_GLOBAL_CONFIG");
        assert_eq!(result, path);
    }

    #[test]
    fn test_load_global_config_not_found() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempdir().unwrap();
        let path = dir.path().join("nonexistent.toml");
        std::env::set_var("CONDUIT_GLOBAL_CONFIG", path.to_str().unwrap());
        let err = load_global_config().unwrap_err();
        std::env::remove_var("CONDUIT_GLOBAL_CONFIG");
        assert!(matches!(err, ConduitError::ConfigNotFound));
    }

    #[test]
    fn test_load_global_config_parses_accounts_and_profiles() {
        let _guard = ENV_LOCK.lock().unwrap();
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
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::env::set_var("CONDUIT_GLOBAL_CONFIG", path.to_str().unwrap());
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
