use std::path::Path;
use std::process::Command;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use crate::config::{AIAccount, Config, Profile};
use crate::error::ConduitError;
use crate::pipeline::Stage;
use crate::spend::SpendTracker;

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

impl ClaudeProvider {
    pub fn command_args(&self, prompt: &str) -> Vec<String> {
        vec![
            "--permission-mode".to_string(),
            "acceptEdits".to_string(),
            "-p".to_string(),
            prompt.to_string(),
        ]
    }
}

impl CodexProvider {
    pub fn command_args(&self, prompt: &str) -> Vec<String> {
        vec![
            "exec".to_string(),
            "--ask-for-approval".to_string(),
            "never".to_string(),
            prompt.to_string(),
        ]
    }
}

impl GeminiProvider {
    pub fn command_args(&self, prompt: &str) -> Vec<String> {
        vec!["-p".to_string(), prompt.to_string()]
    }
}

impl Provider for ClaudeProvider {
    fn name(&self) -> &str { "claude" }
    fn invoke(&self, stage: &str, prompt: &str, work_dir: &Path) -> Result<String, ConduitError> {
        invoke_cli("claude", &self.command_args(prompt), stage, work_dir, self.name())
    }
}

impl Provider for CodexProvider {
    fn name(&self) -> &str { "codex" }
    fn invoke(&self, stage: &str, prompt: &str, work_dir: &Path) -> Result<String, ConduitError> {
        invoke_cli("codex", &self.command_args(prompt), stage, work_dir, self.name())
    }
}

impl Provider for GeminiProvider {
    fn name(&self) -> &str { "gemini" }
    fn invoke(&self, stage: &str, prompt: &str, work_dir: &Path) -> Result<String, ConduitError> {
        invoke_cli("gemini", &self.command_args(prompt), stage, work_dir, self.name())
    }
}

fn invoke_cli(
    binary: &str,
    args: &[String],
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

fn build_provider(account: &AIAccount) -> Result<Box<dyn Provider>, ConduitError> {
    match account.provider.as_str() {
        "claude" if which::which("claude").is_ok() => Ok(Box::new(ClaudeProvider)),
        "openai" if which::which("codex").is_ok() => Ok(Box::new(CodexProvider)),
        "gemini" if which::which("gemini").is_ok() => Ok(Box::new(GeminiProvider)),
        _ => Err(ConduitError::NoProviderAvailable),
    }
}

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
                return provider.invoke(stage, prompt, work_dir);
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
        let tracker = SpendTracker::new_empty_at(path);
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
        // acct-a: over limit (6 invocations × $0.10 = $0.60 >= $0.50)
        let acct_a = make_account_full("acct-a", "claude", Some(true), Some("limit"), Some(0.10), Some(0.50));
        let acct_b = make_account_full("acct-b", "claude", Some(true), Some("limit"), None, None);
        let (_dir, spend) = temp_spend();
        {
            let mut s = spend.lock().unwrap();
            for _ in 0..6 { s.record("acct-a").unwrap(); }
        }
        let fp = make_fp(
            vec![
                (acct_a, Box::new(OkProvider { response: "a".to_string() })),
                (acct_b, Box::new(OkProvider { response: "b".to_string() })),
            ],
            Arc::clone(&spend),
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
        let (_dir, spend) = temp_spend();
        {
            let mut s = spend.lock().unwrap();
            for _ in 0..6 { s.record("acct-a").unwrap(); } // acct-a over limit
        }
        let fp = make_fp(
            vec![
                (acct_a, Box::new(OkProvider { response: "a".to_string() })),
                (acct_b, Box::new(FailProvider)),  // acct-b: runtime error
                (acct_c, Box::new(OkProvider { response: "c".to_string() })),
            ],
            Arc::clone(&spend),
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
        let tail: Vec<&str> = order[2..].iter().map(|a| a.name.as_str()).collect();
        assert!(tail.contains(&"openai-a"));
        assert!(tail.contains(&"gemini-a"));
    }

    #[test]
    fn test_codex_provider_uses_exec_subcommand() {
        let provider = CodexProvider;
        let args = provider.command_args("write hello world");
        let exec_idx = args.iter().position(|a| a == "exec").expect("must contain exec");
        let prompt_idx = args.iter().position(|a| a == "write hello world").expect("must contain prompt");
        assert_eq!(exec_idx, 0, "exec must be the first arg (subcommand)");
        assert!(exec_idx < prompt_idx, "exec must come before the prompt");
        assert_eq!(args.last().unwrap(), "write hello world", "prompt must be last arg");
    }

    #[test]
    fn test_claude_provider_uses_p_flag() {
        let provider = ClaudeProvider;
        let args = provider.command_args("draft a doc");
        let p_idx = args.iter().position(|a| a == "-p").expect("must contain -p");
        let prompt_idx = args.iter().position(|a| a == "draft a doc").expect("must contain prompt");
        assert!(p_idx < prompt_idx, "-p must come before the prompt; got args = {:?}", args);
        assert_eq!(args.last().unwrap(), "draft a doc", "prompt must be last arg");
    }

    #[test]
    fn test_gemini_provider_uses_p_flag() {
        let provider = GeminiProvider;
        let args = provider.command_args("plan steps");
        assert_eq!(args[0], "-p");
        assert_eq!(args[1], "plan steps");
    }

    #[test]
    fn test_claude_provider_includes_permission_mode_flag() {
        let provider = ClaudeProvider;
        let args = provider.command_args("do something");
        assert!(
            args.iter().any(|a| a == "--permission-mode"),
            "Claude must run with --permission-mode for non-interactive subprocess use"
        );
        let mode_idx = args.iter().position(|a| a == "--permission-mode").unwrap();
        let mode_value = &args[mode_idx + 1];
        assert!(
            mode_value == "acceptEdits" || mode_value == "bypassPermissions",
            "permission-mode must be acceptEdits or bypassPermissions for subprocess use, got: {}",
            mode_value
        );
    }

    #[test]
    fn test_codex_provider_includes_approval_flag() {
        let provider = CodexProvider;
        let args = provider.command_args("do something");
        assert!(
            args.iter().any(|a| a == "--ask-for-approval"),
            "Codex must run with --ask-for-approval to avoid hanging on prompts"
        );
        let idx = args.iter().position(|a| a == "--ask-for-approval").unwrap();
        assert_eq!(args[idx + 1], "never");
    }

    #[test]
    fn test_prompt_is_last_arg_for_all_providers() {
        let prompt = "the actual prompt content";
        assert_eq!(ClaudeProvider.command_args(prompt).last().unwrap(), prompt);
        assert_eq!(CodexProvider.command_args(prompt).last().unwrap(), prompt);
        assert_eq!(GeminiProvider.command_args(prompt).last().unwrap(), prompt);
    }
}
