use serde::{Deserialize, Serialize};
use std::path::Path;
use crate::error::ConduitError;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Config {
    pub project: ProjectConfig,
    #[serde(default)]
    pub ai_account: Vec<AIAccount>,
    #[serde(default)]
    pub ollama: OllamaConfig,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ProjectConfig {
    pub name: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AIAccount {
    pub provider: String,
    pub api_key: String,
    pub daily_limit_usd: Option<f64>,
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
        Self {
            enabled: false,
            base_url: default_ollama_url(),
        }
    }
}

fn default_ollama_url() -> String {
    "http://localhost:11434".to_string()
}

pub fn load_config(dir: &Path) -> Result<Config, ConduitError> {
    let path = dir.join(".conduit").join("config.toml");
    if !path.exists() {
        return Err(ConduitError::ConfigNotFound);
    }
    let content = std::fs::read_to_string(&path)?;
    let config: Config = toml::from_str(&content)?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use std::fs;

    fn write_config(dir: &Path, content: &str) {
        let conduit_dir = dir.join(".conduit");
        fs::create_dir_all(&conduit_dir).unwrap();
        fs::write(conduit_dir.join("config.toml"), content).unwrap();
    }

    #[test]
    fn test_load_config_valid() {
        let dir = tempdir().unwrap();
        write_config(dir.path(), r#"
[project]
name = "test-project"

[[ai_account]]
provider = "claude"
api_key = "sk-test"
daily_limit_usd = 10.0
"#);
        let config = load_config(dir.path()).unwrap();
        assert_eq!(config.project.name, "test-project");
        assert_eq!(config.ai_account.len(), 1);
        assert_eq!(config.ai_account[0].provider, "claude");
        assert_eq!(config.ai_account[0].daily_limit_usd, Some(10.0));
    }

    #[test]
    fn test_load_config_not_found() {
        let dir = tempdir().unwrap();
        let err = load_config(dir.path()).unwrap_err();
        assert!(matches!(err, ConduitError::ConfigNotFound));
    }

    #[test]
    fn test_load_config_defaults() {
        let dir = tempdir().unwrap();
        write_config(dir.path(), r#"
[project]
name = "minimal"
"#);
        let config = load_config(dir.path()).unwrap();
        assert!(config.ai_account.is_empty());
        assert!(!config.ollama.enabled);
        assert_eq!(config.ollama.base_url, "http://localhost:11434");
    }

    #[test]
    fn test_load_config_multiple_accounts() {
        let dir = tempdir().unwrap();
        write_config(dir.path(), r#"
[project]
name = "multi"

[[ai_account]]
provider = "claude"
api_key = "sk-ant-1"

[[ai_account]]
provider = "openai"
api_key = "sk-oai-1"
daily_limit_usd = 5.0
"#);
        let config = load_config(dir.path()).unwrap();
        assert_eq!(config.ai_account.len(), 2);
        assert!(config.ai_account[0].daily_limit_usd.is_none());
        assert_eq!(config.ai_account[1].daily_limit_usd, Some(5.0));
    }

    #[test]
    fn test_load_config_ollama_enabled() {
        let dir = tempdir().unwrap();
        write_config(dir.path(), r#"
[project]
name = "test"

[ollama]
enabled = true
base_url = "http://localhost:11434"
"#);
        let config = load_config(dir.path()).unwrap();
        assert!(config.ollama.enabled);
        assert_eq!(config.ollama.base_url, "http://localhost:11434");
    }
}
