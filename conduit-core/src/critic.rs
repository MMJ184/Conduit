use crate::error::ConduitError;
use crate::pipeline::Stage;
use crate::provider::Provider;
use std::path::Path;

pub struct CriticVerdict {
    pub approved: bool,
    pub feedback: String,
}

pub struct Critic<'a> {
    provider: &'a dyn Provider,
    project_dir: &'a Path,
}

impl<'a> Critic<'a> {
    pub fn new(provider: &'a dyn Provider, project_dir: &'a Path) -> Self {
        Self { provider, project_dir }
    }

    pub fn review(&self, stage: &Stage, stage_output: &str, task_description: &str) -> Result<CriticVerdict, ConduitError> {
        let prompt = build_critic_prompt(stage, stage_output, task_description);
        let response = self.provider.invoke("critic", &prompt, self.project_dir)?;
        Ok(parse_critic_response(&response))
    }
}

pub fn build_critic_prompt(stage: &Stage, stage_output: &str, task_description: &str) -> String {
    format!(
        "You are a strict reviewer.\n\nTask description: {task_description}\n\nA `{stage_name}` agent produced this output:\n\n---\n{stage_output}\n---\n\nReview it against these criteria:\n- Does it actually address the task description?\n- Is it specific and actionable (not vague or generic)?\n- Does it leave gaps a downstream agent will have to guess about?\n\nRespond on the first line with exactly `APPROVED` or `REJECTED`. If REJECTED, follow with 1-3 bullet points of specific, actionable feedback.",
        task_description = task_description,
        stage_name = stage.name(),
        stage_output = stage_output,
    )
}

pub fn parse_critic_response(response: &str) -> CriticVerdict {
    let trimmed = response.trim_start();
    if trimmed.starts_with("APPROVED") {
        CriticVerdict {
            approved: true,
            feedback: String::new(),
        }
    } else {
        let feedback = trimmed
            .strip_prefix("REJECTED")
            .unwrap_or(trimmed)
            .trim_start_matches('\n')
            .trim()
            .to_string();
        CriticVerdict { approved: false, feedback }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::MockProvider;
    use tempfile::tempdir;

    #[test]
    fn test_parse_approved_response() {
        let v = parse_critic_response("APPROVED\nlooks good");
        assert!(v.approved);
    }

    #[test]
    fn test_parse_rejected_response_extracts_feedback() {
        let v = parse_critic_response("REJECTED\n- missing edge case\n- vague API");
        assert!(!v.approved);
        assert!(v.feedback.contains("missing edge case"));
        assert!(v.feedback.contains("vague API"));
    }

    #[test]
    fn test_parse_unknown_response_treated_as_rejection() {
        let v = parse_critic_response("hmm, maybe");
        assert!(!v.approved);
        assert!(v.feedback.contains("hmm"));
    }

    #[test]
    fn test_critic_prompt_includes_stage_output_and_task() {
        let p = build_critic_prompt(&Stage::Doc, "the doc content", "build login form");
        assert!(p.contains("the doc content"));
        assert!(p.contains("build login form"));
        assert!(p.contains("APPROVED"));
        assert!(p.contains("REJECTED"));
    }

    #[test]
    fn test_critic_review_returns_verdict_from_provider() {
        let dir = tempdir().unwrap();
        let provider = MockProvider { response: "APPROVED".to_string() };
        let critic = Critic::new(&provider, dir.path());
        let v = critic.review(&Stage::Doc, "any output", "any task").unwrap();
        assert!(v.approved);
    }
}
