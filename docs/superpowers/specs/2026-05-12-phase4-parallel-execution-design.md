# Conduit — Phase 4: Parallel Task Execution Design

**Date:** 2026-05-12
**Scope:** `--concurrency` flag, `ParallelRunner`, prefixed live output, per-task error collection
**Status:** Approved
**Depends on:** Phase 3 (ProviderResolver, ProfileResolver, global config)

---

## Overview

Phase 4 adds parallel task execution to `conduit run`. Currently tasks are processed sequentially; Phase 4 lets multiple tasks run concurrently, each going through its own 5-stage pipeline simultaneously. The user controls concurrency with `--concurrency N`. Output is prefixed per-task so progress from multiple tasks is readable in real-time. Task-specific failures do not cancel other running tasks; systemic failures (config missing, no accounts) still fail immediately before parallelism starts.

---

## CLI Changes

### New `--concurrency` flag

```
conduit run [--task <id>] [--profile <name>] [--concurrency <n>]
```

- `--concurrency N` — maximum number of tasks running at once.
- Default: all tasks run in parallel (no artificial limit — pool sized to task count).
- `--concurrency 1` gives sequential behavior identical to Phase 3.
- `--task <id>` selects a single task; `--concurrency` is irrelevant in that case.

---

## Architecture

### Dependency

```toml
# conduit-core/Cargo.toml
rayon = "1"
```

Rayon provides a work-stealing thread pool. The pool is sized to `min(concurrency, task_count)` so no excess threads are created.

### `ProviderResolver` trait update

```rust
pub trait ProviderResolver: std::fmt::Debug + Send + Sync {
    fn resolve(&self, stage: &Stage) -> Result<Box<dyn Provider>, ConduitError>;
}
```

`Send + Sync` supertraits are required so `&dyn ProviderResolver` can be shared across rayon threads. `ProfileResolver<'a>` satisfies this automatically (it holds only immutable borrows of `Profile` and `Config`, both `Sync`).

### New `conduit-core/src/parallel.rs`

#### `TaskEvent`

Typed events emitted from parallel worker threads back to the CLI via a callback:

```rust
pub enum TaskEvent {
    Started(String),
    StageComplete { task_id: String, completed: usize, total: usize, stage: String },
    Finished(String),
    Failed { task_id: String, error: String },
}
```

#### `TaskResult`

```rust
pub struct TaskResult {
    pub task_id: String,
    pub error: Option<ConduitError>,
}
```

#### `ParallelRunner`

```rust
pub struct ParallelRunner<'a> {
    tasks: &'a [Task],
    resolver: &'a (dyn ProviderResolver + Send + Sync),
    project_dir: &'a Path,
    concurrency: usize,
}

impl<'a> ParallelRunner<'a> {
    pub fn new(
        tasks: &'a [Task],
        resolver: &'a (dyn ProviderResolver + Send + Sync),
        project_dir: &'a Path,
        concurrency: usize,
    ) -> Self;

    pub fn run(
        &self,
        on_event: impl Fn(TaskEvent) + Send + Sync,
    ) -> Vec<TaskResult>;
}
```

`run` builds a rayon `ThreadPool` with `num_threads = min(self.concurrency, self.tasks.len())`, then uses `pool.install(|| self.tasks.par_iter().map(...).collect())` to execute tasks in parallel. Each task creates its own `PipelineRunner` and calls its `run` method. Events are sent to `on_event` (which is `Send + Sync` so it can be called from any thread).

---

## Output Format

### Multiple tasks (parallel)

Each output line is prefixed with `[task-id]`:

```
[task-1] running...
[task-2] running...
[task-1]   [1/5] Orchestrator  ✓
[task-2]   [1/5] Orchestrator  ✓
[task-1]   [2/5] Doc  ✓
[task-2]   [2/5] Doc  ✓
[task-1] done ✓
[task-2] done ✓
```

### Single task (or `--concurrency 1`)

Existing format, no prefix — no regression for current users:

```
[running] task-1
  [1/5] Orchestrator  ✓
  [2/5] Doc  ✓
  ...
[done] task-1
```

### Print lock

The `on_event` callback in `run.rs` wraps all `println!` calls with an `Arc<Mutex<()>>` guard to prevent interleaved output from concurrent threads.

---

## Error Handling

### Systemic errors (fail immediately)

Errors that occur before `ParallelRunner::run` is called:
- `ConfigNotFound`, `NoProvidersConfigured` — config/account setup broken
- `ProfileNotFound`, `ProfileIncomplete` — profile missing or invalid
- `TasksNotFound`, `TasksParseError` — tasks file missing or malformed

These fail immediately, same as Phase 3.

### Task-specific errors (collect and continue)

Errors returned by `PipelineRunner::run` for an individual task:
- `AgentInvocationFailed` — provider CLI errored on a specific stage
- `NoProviderAvailable` — binary not on PATH for this task's profile

Other running tasks continue. After all tasks complete, `run.rs` checks `TaskResult` for any errors.

### Failure summary

If any tasks failed, print a summary and exit non-zero:

```
[task-1] done ✓
[task-2] ✗  Agent `claude` failed at stage `doc`: exit status 1

Results: 1 completed, 1 failed.
```

`conduit run` returns `Ok(())` only if all tasks succeeded.

---

## Data Flow

```
conduit run --concurrency 3
  └─ load_tasks()           → Vec<Task>
  └─ load_global_config()   → Config
  └─ select profile         → Profile
  └─ ProfileResolver        → &dyn ProviderResolver + Send + Sync
  └─ ParallelRunner::run()
       ├─ rayon pool (3 threads)
       ├─ task-1: PipelineRunner → [Orchestrator→Doc→Arch→Code→Test]
       ├─ task-2: PipelineRunner → [Orchestrator→Doc→Arch→Code→Test]
       └─ task-3: PipelineRunner → [Orchestrator→Doc→Arch→Code→Test]
  └─ Vec<TaskResult> → print summary → exit code
```

---

## Testing

### Unit tests (`conduit-core`)

- `ParallelRunner` with `MockProviderResolver`: N tasks complete, all output files written
- `ParallelRunner` with one `FailingResolver` task: other tasks complete, failure collected
- `ParallelRunner` with `concurrency = 1`: tasks run sequentially, same results as `PipelineRunner`
- `ProviderResolver + Send + Sync` — compile-time check (no runtime test needed)

### Integration tests (`conduit-cli`)

- `conduit run --concurrency 2` with 2 tasks and unknown provider → both fail with "No AI provider available"
- `conduit run --concurrency 1` behaves identically to Phase 3 sequential run
- `conduit run --concurrency 0` → error: concurrency must be at least 1

---

## Dependencies Added

| Crate | Where | Used for |
|---|---|---|
| `rayon = "1"` | `conduit-core/Cargo.toml` | Parallel thread pool |

---

## Out of Scope for Phase 4

- Per-account concurrency limits (Phase 5)
- Failover between accounts when one hits a rate limit (Phase 5)
- Context memory across task runs (Phase 6)
- Cost tracking per parallel run (Phase 7)
- TUI live dashboard (Phase 8)

---

## Success Criteria

Phase 4 is complete when:

1. `conduit run --concurrency N` runs up to N tasks simultaneously
2. Default (no flag) runs all tasks in parallel
3. `--concurrency 1` is identical to Phase 3 sequential behavior
4. Output is prefixed `[task-id]` when multiple tasks run in parallel
5. One task failing does not cancel other running tasks
6. All Phase 1–3 tests still pass
7. New unit and integration tests for parallel execution pass
