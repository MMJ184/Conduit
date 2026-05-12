use std::path::Path;
use std::process::Command;
use crate::config::Config;
use crate::error::ConduitError;

pub trait Provider: Send + Sync + std::fmt::Debug {
    fn name(&self) -> &str;
    fn invoke(&self, stage: &str, prompt: &str, work_dir: &Path) -> Result<String, ConduitError>;
}

#[derive(Debug)]
pub struct ClaudeProvider;
#[derive(Debug)]
pub struct CodexProvider;
#[derive(Debug)]
pub struct GeminiProvider;

impl Provider for ClaudeProvider {
    fn name(&self) -> &str {
        "claude"
    }

    fn invoke(&self, stage: &str, prompt: &str, work_dir: &Path) -> Result<String, ConduitError> {
        let output = Command::new("claude")
            .arg("-p")
            .arg(prompt)
            .current_dir(work_dir)
            .output()
            .map_err(|e| ConduitError::AgentInvocationFailed {
                provider: self.name().to_string(),
                stage: stage.to_string(),
                reason: e.to_string(),
            })?;
        if !output.status.success() {
            return Err(ConduitError::AgentInvocationFailed {
                provider: self.name().to_string(),
                stage: stage.to_string(),
                reason: String::from_utf8_lossy(&output.stderr).to_string(),
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

impl Provider for CodexProvider {
    fn name(&self) -> &str {
        "codex"
    }

    fn invoke(&self, stage: &str, prompt: &str, work_dir: &Path) -> Result<String, ConduitError> {
        let output = Command::new("codex")
            .arg(prompt)
            .current_dir(work_dir)
            .output()
            .map_err(|e| ConduitError::AgentInvocationFailed {
                provider: self.name().to_string(),
                stage: stage.to_string(),
                reason: e.to_string(),
            })?;
        if !output.status.success() {
            return Err(ConduitError::AgentInvocationFailed {
                provider: self.name().to_string(),
                stage: stage.to_string(),
                reason: String::from_utf8_lossy(&output.stderr).to_string(),
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

impl Provider for GeminiProvider {
    fn name(&self) -> &str {
        "gemini"
    }

    fn invoke(&self, stage: &str, prompt: &str, work_dir: &Path) -> Result<String, ConduitError> {
        let output = Command::new("gemini")
            .arg("-p")
            .arg(prompt)
            .current_dir(work_dir)
            .output()
            .map_err(|e| ConduitError::AgentInvocationFailed {
                provider: self.name().to_string(),
                stage: stage.to_string(),
                reason: e.to_string(),
            })?;
        if !output.status.success() {
            return Err(ConduitError::AgentInvocationFailed {
                provider: self.name().to_string(),
                stage: stage.to_string(),
                reason: String::from_utf8_lossy(&output.stderr).to_string(),
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

pub fn select_provider(config: &Config) -> Result<Box<dyn Provider>, ConduitError> {
    for account in &config.ai_account {
        match account.provider.as_str() {
            "claude" if which::which("claude").is_ok() => return Ok(Box::new(ClaudeProvider)),
            "openai" if which::which("codex").is_ok() => return Ok(Box::new(CodexProvider)),
            "gemini" if which::which("gemini").is_ok() => return Ok(Box::new(GeminiProvider)),
            _ => continue,
        }
    }
    Err(ConduitError::NoProviderAvailable)
}

#[cfg(any(test, feature = "test-utils"))]
#[derive(Debug)]
pub struct MockProvider {
    pub response: String,
}

#[cfg(any(test, feature = "test-utils"))]
impl Provider for MockProvider {
    fn name(&self) -> &str {
        "mock"
    }

    fn invoke(
        &self,
        _stage: &str,
        _prompt: &str,
        _work_dir: &Path,
    ) -> Result<String, ConduitError> {
        Ok(self.response.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AIAccount, Config, OllamaConfig, ProjectConfig};
    use tempfile::tempdir;

    fn config_with_provider(provider: &str) -> Config {
        Config {
            project: ProjectConfig { name: "test".to_string() },
            ai_account: vec![AIAccount {
                provider: provider.to_string(),
                api_key: "sk-test".to_string(),
                daily_limit_usd: None,
            }],
            ollama: OllamaConfig::default(),
        }
    }

    #[test]
    fn test_select_provider_no_accounts() {
        let config = Config {
            project: ProjectConfig { name: "test".to_string() },
            ai_account: vec![],
            ollama: OllamaConfig::default(),
        };
        let err = select_provider(&config).unwrap_err();
        assert!(matches!(err, ConduitError::NoProviderAvailable));
    }

    #[test]
    fn test_select_provider_unknown_provider_name() {
        let config = config_with_provider("unknown-ai-xyz");
        let err = select_provider(&config).unwrap_err();
        assert!(matches!(err, ConduitError::NoProviderAvailable));
    }

    #[test]
    fn test_mock_provider_returns_configured_response() {
        let provider = MockProvider { response: "hello from mock".to_string() };
        let dir = tempdir().unwrap();
        let result = provider.invoke("orchestrator", "some prompt", dir.path()).unwrap();
        assert_eq!(result, "hello from mock");
    }
}
