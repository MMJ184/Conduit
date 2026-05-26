use std::path::{Path, PathBuf};
use crate::critic::Critic;
use crate::error::ConduitError;
use crate::provider::{Provider, ProviderResolver};
use crate::tasks::Task;

const STAGE_RETRY_BUDGET: usize = 2;

pub enum Stage {
    Orchestrator,
    Doc,
    Architecture,
    Code,
    Test,
}

impl Stage {
    pub fn name(&self) -> &str {
        match self {
            Stage::Orchestrator => "orchestrator",
            Stage::Doc => "doc",
            Stage::Architecture => "architecture",
            Stage::Code => "code",
            Stage::Test => "test",
        }
    }

    pub fn output_filename(&self) -> &str {
        match self {
            Stage::Orchestrator => "orchestrator.md",
            Stage::Doc => "requirements.md",
            Stage::Architecture => "architecture.md",
            Stage::Code => "code.md",
            Stage::Test => "tests.md",
        }
    }

    pub fn json_filename(&self) -> &str {
        match self {
            Stage::Orchestrator => "orchestrator.json",
            Stage::Doc => "requirements.json",
            Stage::Architecture => "architecture.json",
            Stage::Code => "code.json",
            Stage::Test => "tests.json",
        }
    }

    pub fn display_name(&self) -> &str {
        match self {
            Stage::Orchestrator => "Orchestrator",
            Stage::Doc => "Doc",
            Stage::Architecture => "Architecture",
            Stage::Code => "Code",
            Stage::Test => "Tests",
        }
    }

    pub fn all() -> [Stage; 5] {
        [Stage::Orchestrator, Stage::Doc, Stage::Architecture, Stage::Code, Stage::Test]
    }
}

pub struct PipelineRunner<'a> {
    task: &'a Task,
    resolver: &'a dyn ProviderResolver,
    project_dir: &'a Path,
    force: bool,
}

impl<'a> PipelineRunner<'a> {
    pub fn new(task: &'a Task, resolver: &'a dyn ProviderResolver, project_dir: &'a Path) -> Self {
        Self { task, resolver, project_dir, force: false }
    }

    pub fn with_force(mut self, force: bool) -> Self {
        self.force = force;
        self
    }

    pub fn task_dir(&self) -> PathBuf {
        self.project_dir.join(".conduit").join("tasks").join(&self.task.id)
    }

    fn load_reference_docs(&self) -> String {
        let docs_dir = self.project_dir.join(".conduit").join("docs");
        if !docs_dir.exists() {
            return String::new();
        }
        let mut docs = String::new();
        if let Ok(entries) = std::fs::read_dir(&docs_dir) {
            for entry in entries.flatten() {
                if let Ok(content) = std::fs::read_to_string(entry.path()) {
                    docs.push_str(&format!(
                        "--- {} ---\n{}\n\n",
                        entry.file_name().to_string_lossy(),
                        content
                    ));
                }
            }
        }
        docs
    }

    fn read_stage_output(&self, stage: &Stage) -> String {
        std::fs::read_to_string(self.task_dir().join(stage.output_filename()))
            .unwrap_or_default()
    }

    fn write_stage_output(&self, stage: &Stage, content: &str) -> Result<(), ConduitError> {
        let task_dir = self.task_dir();
        std::fs::create_dir_all(&task_dir)?;
        std::fs::write(task_dir.join(stage.output_filename()), content)?;
        Ok(())
    }

    pub fn build_prompt(&self, stage: &Stage, reference_docs: &str) -> String {
        let ref_section = if reference_docs.is_empty() {
            String::new()
        } else {
            format!("Reference documentation:\n{}\n\n", reference_docs)
        };
        let options_line = self.task.options.as_ref()
            .map(|v| format!("\nOptions: {}", v))
            .unwrap_or_default();

        let json_instructions = match stage {
            Stage::Orchestrator => "\n\nAt the end of your response, emit a fenced ```json``` block with this schema:\n{\"plan_steps\": [\"...\"], \"agents_invoked\": [\"doc\",\"architecture\",\"code\",\"test\"]}",
            Stage::Doc => "\n\nAt the end, emit a fenced ```json``` block: {\"requirements\": [\"...\"], \"acceptance_criteria\": [\"...\"]}",
            Stage::Architecture => "\n\nAt the end, emit a fenced ```json``` block: {\"components\": [\"...\"], \"file_layout\": [\"...\"], \"decisions\": [\"...\"]}",
            Stage::Code => "\n\nAt the end, emit a fenced ```json``` block: {\"files_changed\": [\"path/to/file\"], \"summary\": \"...\"}",
            Stage::Test => "\n\nAt the end, emit a fenced ```json``` block: {\"tests_added\": [\"...\"], \"all_passing\": true}",
        };

        let base = match stage {
            Stage::Orchestrator => format!(
                "{ref_section}Task: {id}\nDescription: {desc}{options}\n\nYou are an AI orchestration agent. Break this task into a structured work plan.\nProduce specific instructions for each of the following agents:\n- Documentation agent: what requirements to capture\n- Architecture agent: what design decisions to make\n- Code agent: what to implement and where\n- Test agent: what to test and how\n\nOutput a clear, numbered plan each agent can follow independently.",
                ref_section = ref_section,
                id = self.task.id,
                desc = self.task.description,
                options = options_line,
            ),
            Stage::Doc => format!(
                "{ref_section}Orchestrator plan:\n{orchestrator}\n\nYou are a documentation agent. Following the orchestrator's instructions,\nproduce a detailed requirements document covering: functional requirements,\ninputs/outputs, constraints, and acceptance criteria.",
                ref_section = ref_section,
                orchestrator = self.read_stage_output(&Stage::Orchestrator),
            ),
            Stage::Architecture => format!(
                "{ref_section}Requirements:\n{requirements}\n\nYou are an architecture agent. Following the requirements, produce a\ntechnical architecture plan covering: component breakdown, data flow,\nfile structure, key interfaces, and technology choices.",
                ref_section = ref_section,
                requirements = self.read_stage_output(&Stage::Doc),
            ),
            Stage::Code => format!(
                "{ref_section}Requirements:\n{requirements}\n\nArchitecture:\n{architecture}\n\nYou are a code implementation agent. Implement the code as described in\nthe requirements and architecture plan. Write all files to the project\ndirectory. After writing, output a summary of what was created.",
                ref_section = ref_section,
                requirements = self.read_stage_output(&Stage::Doc),
                architecture = self.read_stage_output(&Stage::Architecture),
            ),
            Stage::Test => format!(
                "{ref_section}Requirements:\n{requirements}\n\nImplementation summary:\n{code}\n\nYou are a testing agent. Write tests for the implemented code. Run the\ntests and report results. Output a summary of tests written and their status.",
                ref_section = ref_section,
                requirements = self.read_stage_output(&Stage::Doc),
                code = self.read_stage_output(&Stage::Code),
            ),
        };

        format!("{}{}", base, json_instructions)
    }

    fn capture_git_changes(&self) -> Option<String> {
        let status = std::process::Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(self.project_dir)
            .output()
            .ok()?;
        if !status.status.success() {
            return None;
        }
        let status_text = String::from_utf8_lossy(&status.stdout);
        if status_text.trim().is_empty() {
            return Some(String::from("(no uncommitted changes detected)"));
        }

        let diff_stat = std::process::Command::new("git")
            .args(["diff", "--stat", "HEAD"])
            .current_dir(self.project_dir)
            .output()
            .ok()?;
        let diff_text = if diff_stat.status.success() {
            String::from_utf8_lossy(&diff_stat.stdout).to_string()
        } else {
            String::new()
        };

        Some(format!(
            "## Filesystem changes after Code stage\n\n### `git status --porcelain`\n```\n{}```\n\n### `git diff --stat HEAD`\n```\n{}```\n",
            status_text, diff_text
        ))
    }

    pub fn extract_json_sidecar(raw: &str) -> Option<serde_json::Value> {
        let start_marker = "```json";
        let start = raw.find(start_marker)?;
        let after_marker = &raw[start + start_marker.len()..];
        let end_rel = after_marker.find("```")?;
        let json_text = after_marker[..end_rel].trim();
        serde_json::from_str(json_text).ok()
    }

    fn write_json_sidecar(&self, stage: &Stage, raw_output: &str) -> Result<(), ConduitError> {
        let task_dir = self.task_dir();
        std::fs::create_dir_all(&task_dir)?;
        let payload = Self::extract_json_sidecar(raw_output).unwrap_or_else(|| {
            serde_json::json!({ "raw": raw_output })
        });
        let pretty = serde_json::to_string_pretty(&payload)
            .map_err(|e| ConduitError::JsonParseError {
                stage: stage.name().to_string(),
                reason: e.to_string(),
            })?;
        std::fs::write(task_dir.join(stage.json_filename()), pretty)?;
        Ok(())
    }

    fn run_stage(&self, stage: &Stage, reference_docs: &str) -> Result<(), ConduitError> {
        let provider: Box<dyn Provider> = self.resolver.resolve(stage)?;
        let critic_provider: Box<dyn Provider> = self.resolver.resolve(stage)?;
        let critic = Critic::new(critic_provider.as_ref(), self.project_dir);

        let mut feedback_prefix = String::new();
        for attempt in 0..STAGE_RETRY_BUDGET {
            let mut prompt = self.build_prompt(stage, reference_docs);
            if !feedback_prefix.is_empty() {
                prompt = format!(
                    "Previous attempt was REJECTED with this feedback:\n{}\n\nRevise your work. Original prompt follows.\n\n{}",
                    feedback_prefix, prompt
                );
            }

            let output = provider.invoke(stage.name(), &prompt, self.project_dir)?;
            self.write_stage_output(stage, &output)?;
            self.write_json_sidecar(stage, &output)?;

            if matches!(stage, Stage::Code) {
                if let Some(diff_section) = self.capture_git_changes() {
                    let path = self.task_dir().join(stage.output_filename());
                    let mut combined = std::fs::read_to_string(&path).unwrap_or_default();
                    combined.push_str("\n\n");
                    combined.push_str(&diff_section);
                    std::fs::write(&path, combined)?;
                }
            }

            let verdict = critic.review(stage, &output, &self.task.description)?;
            if verdict.approved {
                return Ok(());
            }
            feedback_prefix = verdict.feedback.clone();
            if attempt + 1 == STAGE_RETRY_BUDGET {
                return Err(ConduitError::CriticRejected {
                    stage: stage.name().to_string(),
                    attempts: STAGE_RETRY_BUDGET,
                    feedback: verdict.feedback,
                });
            }
        }
        Ok(())
    }

    pub fn run(
        &self,
        mut on_stage_complete: impl FnMut(usize, usize, &Stage),
    ) -> Result<(), ConduitError> {
        let reference_docs = self.load_reference_docs();
        let stages = Stage::all();
        let total = stages.len();
        for (i, stage) in stages.iter().enumerate() {
            let out_path = self.task_dir().join(stage.output_filename());
            if !self.force && out_path.exists() {
                continue;
            }
            self.run_stage(stage, &reference_docs)?;
            on_stage_complete(i + 1, total, stage);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::MockProviderResolver;
    use crate::tasks::Task;
    use std::fs;
    use std::path::Path;
    use std::process::Command;
    use tempfile::tempdir;

    fn make_task(id: &str, desc: &str) -> Task {
        Task { id: id.to_string(), description: desc.to_string(), options: None }
    }

    #[test]
    fn test_orchestrator_prompt_contains_task_info() {
        let task = make_task("auth-feature", "Build a login form");
        let dir = tempdir().unwrap();
        let resolver = MockProviderResolver { response: "mock".to_string() };
        let runner = PipelineRunner::new(&task, &resolver, dir.path());
        let prompt = runner.build_prompt(&Stage::Orchestrator, "");
        assert!(prompt.contains("Build a login form"));
        assert!(prompt.contains("auth-feature"));
        assert!(prompt.contains("Documentation agent"));
        assert!(prompt.contains("Architecture agent"));
        assert!(prompt.contains("Code agent"));
        assert!(prompt.contains("Test agent"));
    }

    #[test]
    fn test_doc_prompt_includes_orchestrator_output() {
        let task = make_task("t", "desc");
        let dir = tempdir().unwrap();
        let resolver = MockProviderResolver { response: "mock".to_string() };
        let runner = PipelineRunner::new(&task, &resolver, dir.path());
        let task_dir = dir.path().join(".conduit").join("tasks").join("t");
        fs::create_dir_all(&task_dir).unwrap();
        fs::write(task_dir.join("orchestrator.md"), "step 1: do X").unwrap();
        let prompt = runner.build_prompt(&Stage::Doc, "");
        assert!(prompt.contains("step 1: do X"));
        assert!(prompt.contains("documentation agent"));
    }

    #[test]
    fn test_reference_docs_prepended_to_prompt() {
        let task = make_task("t", "desc");
        let dir = tempdir().unwrap();
        let resolver = MockProviderResolver { response: "mock".to_string() };
        let runner = PipelineRunner::new(&task, &resolver, dir.path());
        let prompt = runner.build_prompt(&Stage::Orchestrator, "API spec: GET /users");
        assert!(prompt.contains("API spec: GET /users"));
        assert!(prompt.contains("Reference documentation"));
    }

    #[test]
    fn test_run_writes_all_five_output_files() {
        let task = make_task("my-task", "test task");
        let dir = tempdir().unwrap();
        let resolver = MockProviderResolver { response: "APPROVED\nstage output content".to_string() };
        let runner = PipelineRunner::new(&task, &resolver, dir.path());
        runner.run(|_, _, _| {}).unwrap();
        let task_dir = dir.path().join(".conduit").join("tasks").join("my-task");
        for filename in &["orchestrator.md", "requirements.md", "architecture.md", "code.md", "tests.md"] {
            assert!(task_dir.join(filename).exists(), "Missing: {}", filename);
            assert_eq!(fs::read_to_string(task_dir.join(filename)).unwrap(), "APPROVED\nstage output content");
        }
    }

    #[test]
    fn test_run_callback_called_with_correct_indices() {
        let task = make_task("t", "desc");
        let dir = tempdir().unwrap();
        let resolver = MockProviderResolver { response: "APPROVED\noutput".to_string() };
        let runner = PipelineRunner::new(&task, &resolver, dir.path());
        let mut calls: Vec<(usize, usize, String)> = Vec::new();
        runner.run(|completed, total, stage| {
            calls.push((completed, total, stage.name().to_string()));
        }).unwrap();
        assert_eq!(calls.len(), 5);
        assert_eq!(calls[0], (1, 5, "orchestrator".to_string()));
        assert_eq!(calls[4], (5, 5, "test".to_string()));
    }

    #[test]
    fn test_run_stops_on_resolver_error() {
        #[derive(Debug)]
        struct FailingResolver;
        impl ProviderResolver for FailingResolver {
            fn resolve(&self, stage: &Stage) -> Result<Box<dyn Provider>, ConduitError> {
                Err(ConduitError::AgentInvocationFailed {
                    provider: "failing".to_string(),
                    stage: stage.name().to_string(),
                    reason: "intentional".to_string(),
                })
            }
        }
        let task = make_task("t", "desc");
        let dir = tempdir().unwrap();
        let resolver = FailingResolver;
        let runner = PipelineRunner::new(&task, &resolver, dir.path());
        let mut count = 0usize;
        let err = runner.run(|_, _, _| { count += 1; }).unwrap_err();
        assert_eq!(count, 0);
        assert!(matches!(err, ConduitError::AgentInvocationFailed { .. }));
    }

    fn init_git_in(dir: &Path) {
        Command::new("git").args(["init"]).current_dir(dir).output().unwrap();
        Command::new("git").args(["config", "user.email", "t@t"]).current_dir(dir).output().unwrap();
        Command::new("git").args(["config", "user.name", "T"]).current_dir(dir).output().unwrap();
        std::fs::write(dir.join("seed"), "").unwrap();
        Command::new("git").args(["add", "."]).current_dir(dir).output().unwrap();
        Command::new("git").args(["commit", "-m", "i"]).current_dir(dir).output().unwrap();
    }

    #[test]
    fn test_pipeline_skips_stage_when_output_exists() {
        let task = make_task("resume-test", "test resume");
        let dir = tempdir().unwrap();
        let task_dir = dir.path().join(".conduit").join("tasks").join("resume-test");
        std::fs::create_dir_all(&task_dir).unwrap();
        std::fs::write(task_dir.join("orchestrator.md"), "PRE-EXISTING orchestrator").unwrap();
        std::fs::write(task_dir.join("requirements.md"), "PRE-EXISTING requirements").unwrap();

        let resolver = MockProviderResolver { response: "APPROVED\nFRESH".to_string() };
        let runner = PipelineRunner::new(&task, &resolver, dir.path());

        let mut completed_stages: Vec<String> = Vec::new();
        runner.run(|_, _, stage| {
            completed_stages.push(stage.name().to_string());
        }).unwrap();

        assert_eq!(
            std::fs::read_to_string(task_dir.join("orchestrator.md")).unwrap(),
            "PRE-EXISTING orchestrator",
            "orchestrator.md should NOT be overwritten when it already exists"
        );
        assert_eq!(
            std::fs::read_to_string(task_dir.join("requirements.md")).unwrap(),
            "PRE-EXISTING requirements",
        );
        assert!(task_dir.join("architecture.md").exists());
        assert!(task_dir.join("code.md").exists());
        assert!(task_dir.join("tests.md").exists());

        assert_eq!(completed_stages.len(), 3, "expected 3 stage callbacks, got {:?}", completed_stages);
    }

    #[test]
    fn test_pipeline_force_reruns_all_stages() {
        let task = make_task("force-test", "test force");
        let dir = tempdir().unwrap();
        let task_dir = dir.path().join(".conduit").join("tasks").join("force-test");
        std::fs::create_dir_all(&task_dir).unwrap();
        std::fs::write(task_dir.join("orchestrator.md"), "OLD orchestrator").unwrap();

        let resolver = MockProviderResolver { response: "APPROVED\nFRESH".to_string() };
        let runner = PipelineRunner::new(&task, &resolver, dir.path()).with_force(true);

        let mut count = 0usize;
        runner.run(|_, _, _| { count += 1; }).unwrap();

        assert_eq!(count, 5, "force should run all 5 stages");
        assert_eq!(
            std::fs::read_to_string(task_dir.join("orchestrator.md")).unwrap(),
            "APPROVED\nFRESH",
            "with force=true, orchestrator.md should be overwritten"
        );
    }

    #[test]
    fn test_code_stage_handoff_includes_git_diff_when_in_repo() {
        let task = make_task("t", "desc");
        let dir = tempdir().unwrap();
        init_git_in(dir.path());

        #[derive(Debug)]
        struct CreatesFileResolver;
        impl ProviderResolver for CreatesFileResolver {
            fn resolve(&self, _stage: &Stage) -> Result<Box<dyn Provider>, ConduitError> {
                Ok(Box::new(crate::provider::MockProvider { response: "APPROVED\nagent output".to_string() }))
            }
        }

        let runner = PipelineRunner::new(&task, &CreatesFileResolver, dir.path());
        std::fs::write(dir.path().join("new_file.rs"), "fn foo() {}").unwrap();
        runner.run(|_, _, _| {}).unwrap();

        let task_dir = dir.path().join(".conduit").join("tasks").join("t");
        let code_md = std::fs::read_to_string(task_dir.join("code.md")).unwrap();
        assert!(code_md.contains("new_file.rs"), "code.md should contain git status output mentioning new_file.rs, got:\n{}", code_md);
    }

    #[test]
    fn test_extract_json_sidecar_from_fenced_block() {
        let raw = r#"Here is the analysis.

```json
{"decisions": ["use Rust"], "risks": ["scope creep"]}
```

And some trailing notes."#;
        let json = PipelineRunner::extract_json_sidecar(raw).unwrap();
        assert_eq!(json.get("decisions").unwrap()[0], "use Rust");
        assert_eq!(json.get("risks").unwrap()[0], "scope creep");
    }

    #[test]
    fn test_extract_json_sidecar_returns_none_when_no_block() {
        let raw = "no json here at all";
        assert!(PipelineRunner::extract_json_sidecar(raw).is_none());
    }

    #[test]
    fn test_extract_json_sidecar_malformed_block_returns_none() {
        let raw = "```json\n{not valid json\n```";
        assert!(PipelineRunner::extract_json_sidecar(raw).is_none());
    }

    #[test]
    fn test_pipeline_surfaces_critic_rejected_when_critic_always_rejects() {
        #[derive(Debug)]
        struct AlwaysRejectResolver;
        impl ProviderResolver for AlwaysRejectResolver {
            fn resolve(&self, _stage: &Stage) -> Result<Box<dyn Provider>, ConduitError> {
                Ok(Box::new(crate::provider::MockProvider {
                    response: "REJECTED\n- bad".to_string(),
                }))
            }
        }
        let task = make_task("crit-test", "desc");
        let dir = tempdir().unwrap();
        let runner = PipelineRunner::new(&task, &AlwaysRejectResolver, dir.path());
        let err = runner.run(|_, _, _| {}).unwrap_err();
        assert!(
            matches!(err, ConduitError::CriticRejected { .. }),
            "Pipeline must surface CriticRejected when critic keeps rejecting, got: {:?}",
            err
        );
    }

    #[test]
    fn test_run_writes_json_sidecar_when_agent_emits_one() {
        let task = make_task("json-task", "desc");
        let dir = tempdir().unwrap();
        let response = "APPROVED\nsummary text\n\n```json\n{\"decisions\":[\"x\"]}\n```\n";
        let resolver = MockProviderResolver { response: response.to_string() };
        let runner = PipelineRunner::new(&task, &resolver, dir.path());
        runner.run(|_, _, _| {}).unwrap();
        let task_dir = dir.path().join(".conduit").join("tasks").join("json-task");
        for stage_json in &["orchestrator.json", "requirements.json", "architecture.json", "code.json", "tests.json"] {
            assert!(task_dir.join(stage_json).exists(), "missing {}", stage_json);
        }
    }
}
