use std::path::Path;
use std::process::Command;
use crate::config::{Config, Profile};
use crate::error::ConduitError;
use crate::pipeline::Stage;

pub trait Provider: Send + Sync + std::fmt::Debug {
    fn name(&self) -> &str;
    fn invoke(&self, stage: &str, prompt: &str, work_dir: &Path) -> Result<String, ConduitError>;
}

pub trait ProviderResolver: std::fmt::Debug + Send + Sync {
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
    use crate::config::{AIAccount, Config, Profile};
    use tempfile::tempdir;

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

    #[test]
    fn test_provider_resolver_send_sync() {
        fn assert_send_sync<T: ?Sized + Send + Sync>() {}
        assert_send_sync::<dyn ProviderResolver>();
    }
}
