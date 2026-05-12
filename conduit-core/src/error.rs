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
    #[error("All accounts exhausted for stage `{stage}`: every configured account is over its limit or failed")]
    AllAccountsExhausted { stage: String },
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

    #[test]
    fn test_all_accounts_exhausted_message() {
        let e = ConduitError::AllAccountsExhausted { stage: "doc".to_string() };
        let msg = e.to_string();
        assert!(msg.contains("doc"));
        assert!(msg.contains("exhausted") || msg.contains("All accounts"));
    }
}
