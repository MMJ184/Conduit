use std::path::Path;
use rayon::prelude::*;
use crate::error::ConduitError;
use crate::pipeline::PipelineRunner;
use crate::provider::ProviderResolver;
use crate::tasks::Task;

pub enum TaskEvent {
    Started(String),
    StageComplete { task_id: String, completed: usize, total: usize, stage: String },
    Finished(String),
    Failed { task_id: String, error: String },
}

pub struct TaskResult {
    pub task_id: String,
    pub error: Option<ConduitError>,
}

pub struct ParallelRunner<'a> {
    tasks: &'a [Task],
    resolver: &'a dyn ProviderResolver,
    project_dir: &'a Path,
    concurrency: usize,
}

impl<'a> ParallelRunner<'a> {
    pub fn new(
        tasks: &'a [Task],
        resolver: &'a dyn ProviderResolver,
        project_dir: &'a Path,
        concurrency: usize,
    ) -> Self {
        Self { tasks, resolver, project_dir, concurrency }
    }

    pub fn run(
        &self,
        on_event: impl Fn(TaskEvent) + Send + Sync,
    ) -> Vec<TaskResult> {
        let num_threads = self.concurrency.min(self.tasks.len()).max(1);
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(num_threads)
            .build()
            .expect("failed to build thread pool");

        pool.install(|| {
            self.tasks.par_iter().map(|task| {
                on_event(TaskEvent::Started(task.id.clone()));
                let runner = PipelineRunner::new(task, self.resolver, self.project_dir);
                let result = runner.run(|completed, total, stage| {
                    on_event(TaskEvent::StageComplete {
                        task_id: task.id.clone(),
                        completed,
                        total,
                        stage: stage.display_name().to_string(),
                    });
                });
                match result {
                    Ok(()) => {
                        on_event(TaskEvent::Finished(task.id.clone()));
                        TaskResult { task_id: task.id.clone(), error: None }
                    }
                    Err(e) => {
                        on_event(TaskEvent::Failed {
                            task_id: task.id.clone(),
                            error: e.to_string(),
                        });
                        TaskResult { task_id: task.id.clone(), error: Some(e) }
                    }
                }
            }).collect()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::Stage;
    use crate::provider::MockProviderResolver;
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;

    fn make_task(id: &str) -> Task {
        Task { id: id.to_string(), description: "test task".to_string(), options: None }
    }

    #[test]
    fn test_parallel_runner_all_tasks_complete() {
        let tasks = vec![make_task("task-a"), make_task("task-b")];
        let dir = tempdir().unwrap();
        let resolver = MockProviderResolver { response: "output".to_string() };
        let runner = ParallelRunner::new(&tasks, &resolver, dir.path(), 2);
        let results = runner.run(|_| {});
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.error.is_none()));
        for id in &["task-a", "task-b"] {
            let task_dir = dir.path().join(".conduit").join("tasks").join(id);
            assert!(task_dir.join("orchestrator.md").exists(), "missing orchestrator.md for {}", id);
            assert!(task_dir.join("tests.md").exists(), "missing tests.md for {}", id);
        }
    }

    #[test]
    fn test_parallel_runner_failing_resolver_collects_errors() {
        #[derive(Debug)]
        struct FailingResolver;
        impl ProviderResolver for FailingResolver {
            fn resolve(&self, _stage: &Stage) -> Result<Box<dyn crate::provider::Provider>, ConduitError> {
                Err(ConduitError::NoProviderAvailable)
            }
        }
        let tasks = vec![make_task("task-a"), make_task("task-b")];
        let dir = tempdir().unwrap();
        let resolver = FailingResolver;
        let runner = ParallelRunner::new(&tasks, &resolver, dir.path(), 2);
        let results = runner.run(|_| {});
        assert_eq!(results.len(), 2, "both TaskResults must be collected, not panicked");
        assert!(results.iter().all(|r| r.error.is_some()), "all tasks should report failure");
    }

    #[test]
    fn test_parallel_runner_concurrency_1_runs_all_tasks() {
        let tasks = vec![make_task("task-a"), make_task("task-b")];
        let dir = tempdir().unwrap();
        let resolver = MockProviderResolver { response: "output".to_string() };
        let runner = ParallelRunner::new(&tasks, &resolver, dir.path(), 1);
        let results = runner.run(|_| {});
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.error.is_none()));
    }

    #[test]
    fn test_parallel_runner_events_emitted_in_order() {
        let tasks = vec![make_task("task-a")];
        let dir = tempdir().unwrap();
        let resolver = MockProviderResolver { response: "out".to_string() };
        let runner = ParallelRunner::new(&tasks, &resolver, dir.path(), 1);
        let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let log_clone = Arc::clone(&log);
        runner.run(move |event| {
            let mut l = log_clone.lock().unwrap();
            match &event {
                TaskEvent::Started(id) => l.push(format!("started:{}", id)),
                TaskEvent::Finished(id) => l.push(format!("finished:{}", id)),
                TaskEvent::StageComplete { completed, .. } => l.push(format!("stage:{}", completed)),
                TaskEvent::Failed { task_id, .. } => l.push(format!("failed:{}", task_id)),
            }
        });
        let l = log.lock().unwrap();
        assert!(l.contains(&"started:task-a".to_string()));
        assert!(l.contains(&"finished:task-a".to_string()));
        assert_eq!(l.iter().filter(|s| s.starts_with("stage:")).count(), 5);
    }
}
