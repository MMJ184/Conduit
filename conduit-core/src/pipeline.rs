use std::path::{Path, PathBuf};
use crate::error::ConduitError;
use crate::provider::Provider;
use crate::tasks::Task;

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
        [
            Stage::Orchestrator,
            Stage::Doc,
            Stage::Architecture,
            Stage::Code,
            Stage::Test,
        ]
    }
}

pub struct PipelineRunner<'a> {
    task: &'a Task,
    provider: &'a dyn Provider,
    project_dir: &'a Path,
}

impl<'a> PipelineRunner<'a> {
    pub fn new(task: &'a Task, provider: &'a dyn Provider, project_dir: &'a Path) -> Self {
        Self { task, provider, project_dir }
    }

    pub fn task_dir(&self) -> PathBuf {
        self.project_dir
            .join(".conduit")
            .join("tasks")
            .join(&self.task.id)
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
        let path = self.task_dir().join(stage.output_filename());
        std::fs::read_to_string(path).unwrap_or_default()
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
        let options_line = self
            .task
            .options
            .as_ref()
            .map(|v| format!("\nOptions: {}", v))
            .unwrap_or_default();

        match stage {
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
        }
    }

    fn run_stage(&self, stage: &Stage, reference_docs: &str) -> Result<(), ConduitError> {
        let prompt = self.build_prompt(stage, reference_docs);
        let output = self.provider.invoke(stage.name(), &prompt, self.project_dir)?;
        self.write_stage_output(stage, &output)?;
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
            self.run_stage(stage, &reference_docs)?;
            on_stage_complete(i + 1, total, stage);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::MockProvider;
    use crate::tasks::Task;
    use std::fs;
    use tempfile::tempdir;

    fn make_task(id: &str, desc: &str) -> Task {
        Task {
            id: id.to_string(),
            description: desc.to_string(),
            options: None,
        }
    }

    #[test]
    fn test_orchestrator_prompt_contains_task_info() {
        let task = make_task("auth-feature", "Build a login form");
        let dir = tempdir().unwrap();
        let provider = MockProvider { response: "mock".to_string() };
        let runner = PipelineRunner::new(&task, &provider, dir.path());
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
        let provider = MockProvider { response: "mock".to_string() };
        let runner = PipelineRunner::new(&task, &provider, dir.path());
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
        let provider = MockProvider { response: "mock".to_string() };
        let runner = PipelineRunner::new(&task, &provider, dir.path());
        let prompt = runner.build_prompt(&Stage::Orchestrator, "API spec: GET /users");
        assert!(prompt.contains("API spec: GET /users"));
        assert!(prompt.contains("Reference documentation"));
    }

    #[test]
    fn test_run_writes_all_five_output_files() {
        let task = make_task("my-task", "test task");
        let dir = tempdir().unwrap();
        let provider = MockProvider { response: "stage output content".to_string() };
        let runner = PipelineRunner::new(&task, &provider, dir.path());
        runner.run(|_, _, _| {}).unwrap();
        let task_dir = dir.path().join(".conduit").join("tasks").join("my-task");
        for filename in &[
            "orchestrator.md",
            "requirements.md",
            "architecture.md",
            "code.md",
            "tests.md",
        ] {
            assert!(task_dir.join(filename).exists(), "Missing: {}", filename);
            let content = fs::read_to_string(task_dir.join(filename)).unwrap();
            assert_eq!(content, "stage output content");
        }
    }

    #[test]
    fn test_run_callback_called_with_correct_indices() {
        let task = make_task("t", "desc");
        let dir = tempdir().unwrap();
        let provider = MockProvider { response: "output".to_string() };
        let runner = PipelineRunner::new(&task, &provider, dir.path());
        let mut calls: Vec<(usize, usize, String)> = Vec::new();
        runner
            .run(|completed, total, stage| {
                calls.push((completed, total, stage.name().to_string()));
            })
            .unwrap();
        assert_eq!(calls.len(), 5);
        assert_eq!(calls[0], (1, 5, "orchestrator".to_string()));
        assert_eq!(calls[2], (3, 5, "architecture".to_string()));
        assert_eq!(calls[4], (5, 5, "test".to_string()));
    }

    #[test]
    fn test_run_stops_on_provider_error() {
        struct FailingProvider;
        impl std::fmt::Debug for FailingProvider {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "FailingProvider")
            }
        }
        impl Provider for FailingProvider {
            fn name(&self) -> &str {
                "failing"
            }
            fn invoke(
                &self,
                stage: &str,
                _prompt: &str,
                _work_dir: &Path,
            ) -> Result<String, ConduitError> {
                Err(ConduitError::AgentInvocationFailed {
                    provider: "failing".to_string(),
                    stage: stage.to_string(),
                    reason: "intentional failure".to_string(),
                })
            }
        }
        let task = make_task("t", "desc");
        let dir = tempdir().unwrap();
        let provider = FailingProvider;
        let runner = PipelineRunner::new(&task, &provider, dir.path());
        let mut callback_count = 0usize;
        let err = runner.run(|_, _, _| { callback_count += 1; }).unwrap_err();
        assert_eq!(callback_count, 0);
        assert!(matches!(err, ConduitError::AgentInvocationFailed { .. }));
    }
}
