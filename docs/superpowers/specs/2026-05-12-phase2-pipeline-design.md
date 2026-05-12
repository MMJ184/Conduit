# Conduit — Phase 2: Single-Agent Pipeline Runner Design

**Date:** 2026-05-12
**Scope:** Provider trait, 5-stage pipeline execution, output file structure, updated `conduit run`
**Status:** Approved
**Depends on:** Phase 1 (conduit-core types, conduit-cli scaffold)

---

## Overview

Phase 2 wires up the actual AI execution pipeline. When a user runs `conduit run`, each task in `tasks.toml` now passes through a 5-stage pipeline:

1. **Orchestrator** — reads the task description, produces per-agent instructions
2. **Doc** — produces a detailed requirements document
3. **Architecture** — produces an architecture plan
4. **Code** — implements the code (writes files to project directory)
5. **Test** — writes and runs tests

Each stage invokes a CLI AI agent (`claude`, `codex`, or `gemini`) as a child subprocess in non-interactive mode, captures stdout as the stage result, and writes it to a file under `.conduit/tasks/<task-id>/`. Each subsequent stage reads the previous stage's output as context.

Phase 2 uses a single provider across all stages — whichever AI account appears first in `.conduit/config.toml`. Multi-provider selection and smart model picking are Phase 3.

---

## New Modules

Two modules added to `conduit-core`. No new workspace crate — providers extract to `conduit-providers` in Phase 3.

```
conduit-core/src/
  lib.rs            ← add: pub mod pipeline; pub mod provider;
  provider.rs       ← Provider trait + ClaudeProvider, CodexProvider, GeminiProvider
  pipeline.rs       ← Stage enum, PipelineRunner, prompt builders, output file writer
conduit-cli/src/commands/
  run.rs            ← replace "[queued]" stub with actual pipeline execution
```

---

## Provider Trait

```rust
pub trait Provider: Send + Sync {
    fn name(&self) -> &str;
    fn invoke(&self, prompt: &str, work_dir: &Path) -> Result<String, ConduitError>;
}
```

Each provider spawns its CLI binary as a child process in non-interactive mode, captures stdout as the result string, and returns an error if the process exits non-zero or the binary is not found.

### CLI Invocation

| Provider | Struct | Binary | Command |
|---|---|---|---|
| Anthropic Claude | `ClaudeProvider` | `claude` | `claude -p "<prompt>"` |
| OpenAI Codex | `CodexProvider` | `codex` | `codex "<prompt>"` |
| Google Gemini | `GeminiProvider` | `gemini` | `gemini -p "<prompt>"` |

The `work_dir` is passed as `Command::current_dir()` so the agent operates in the project root and can read/write project files.

### Provider Selection

`fn select_provider(config: &Config) -> Result<Box<dyn Provider>, ConduitError>`

Iterates `config.ai_account` in order, returns the first provider whose binary is found on `PATH` (via `which::which` or `std::process::Command` probe). Returns `ConduitError::NoProviderAvailable` if none are found.

### New Error Variants

```rust
#[error("No AI provider available. Run `conduit init` to configure one.")]
NoProviderAvailable,

#[error("Agent `{provider}` failed at stage `{stage}`: {reason}")]
AgentInvocationFailed { provider: String, stage: String, reason: String },
```

---

## Pipeline Stages

### Output File Structure

For each task, stage outputs are written to:

```
.conduit/tasks/<task-id>/
  orchestrator.md    ← Stage 1: work plan + per-agent instructions
  requirements.md    ← Stage 2: detailed requirements document
  architecture.md    ← Stage 3: architecture plan
  code.md            ← Stage 4: implementation summary (agent also writes code to project)
  tests.md           ← Stage 5: test summary (agent also writes test files to project)
```

These files are created fresh on each run (overwrite if re-running). The `.conduit/tasks/` directory is created automatically.

### Stage Definitions

```rust
pub enum Stage {
    Orchestrator,
    Doc,
    Architecture,
    Code,
    Test,
}
```

### Prompt Construction

Each stage prompt has three parts:
1. **Reference docs** (optional) — contents of any files in `.conduit/docs/`, prepended as context
2. **Prior stage outputs** — contents of previous stage output files
3. **Stage instruction** — the specific instruction for this stage

**Stage 1 — Orchestrator**
```
{reference_docs}

Task: {task.id}
Description: {task.description}
Options: {task.options if present}

You are an AI orchestration agent. Break this task into a structured work plan.
Produce specific instructions for each of the following agents:
- Documentation agent: what requirements to capture
- Architecture agent: what design decisions to make
- Code agent: what to implement and where
- Test agent: what to test and how

Output a clear, numbered plan each agent can follow independently.
```

**Stage 2 — Doc**
```
{reference_docs}

Orchestrator plan:
{orchestrator.md}

You are a documentation agent. Following the orchestrator's instructions,
produce a detailed requirements document covering: functional requirements,
inputs/outputs, constraints, and acceptance criteria.
```

**Stage 3 — Architecture**
```
{reference_docs}

Requirements:
{requirements.md}

You are an architecture agent. Following the requirements, produce a
technical architecture plan covering: component breakdown, data flow,
file structure, key interfaces, and technology choices.
```

**Stage 4 — Code**
```
{reference_docs}

Requirements:
{requirements.md}

Architecture:
{architecture.md}

You are a code implementation agent. Implement the code as described in
the requirements and architecture plan. Write all files to the project
directory. After writing, output a summary of what was created.
```

**Stage 5 — Test**
```
{reference_docs}

Requirements:
{requirements.md}

Implementation summary:
{code.md}

You are a testing agent. Write tests for the implemented code. Run the
tests and report results. Output a summary of tests written and their status.
```

---

## PipelineRunner

```rust
pub struct PipelineRunner<'a> {
    task: &'a Task,
    provider: &'a dyn Provider,
    project_dir: &'a Path,
}

impl<'a> PipelineRunner<'a> {
    pub fn run(&self) -> Result<(), ConduitError>;
}
```

`run()` executes all 5 stages sequentially. On any stage failure, it stops and returns the error — partial output files remain in `.conduit/tasks/<task-id>/` for debugging.

Reference docs are loaded once at the start of `run()` from `.conduit/docs/` (if the directory exists). Missing `.conduit/docs/` is not an error — it is silently skipped. The `project_dir` is the directory containing `tasks.toml` (the current working directory when `conduit run` is invoked).

---

## Updated `conduit run` Behaviour

`conduit-cli/src/commands/run.rs` now:

1. Loads `tasks.toml` and `.conduit/config.toml`
2. Selects the active provider via `select_provider(&config)`
3. For each task (or the filtered task if `--task` is given):
   - Prints `[running] <task-id>`
   - Creates a `PipelineRunner` and calls `run()`
   - After each stage completes, prints `  [N/5] <Stage>  ✓`
   - On failure, prints `  [N/5] <Stage>  ✗  Error: <message>` and stops

**Terminal output example:**
```
[running] auth-feature
  [1/5] Orchestrator  ✓
  [2/5] Doc           ✓
  [3/5] Architecture  ✓
  [4/5] Code          ✓
  [5/5] Tests         ✓
[done] auth-feature
```

Progress printing happens between stages — the user sees each stage complete in real time.

---

## Error Handling

| Scenario | Error |
|---|---|
| No AI binary found on PATH | `ConduitError::NoProviderAvailable` |
| CLI process exits non-zero | `ConduitError::AgentInvocationFailed` |
| Output file cannot be written | `ConduitError::Io` |
| tasks.toml missing | `ConduitError::TasksNotFound` |
| config.toml missing | `ConduitError::ConfigNotFound` |

On error, partial output files in `.conduit/tasks/<task-id>/` are preserved for debugging. No cleanup on failure.

---

## Testing

- **Unit tests** (`conduit-core`): `MockProvider` that returns canned strings; tests for each stage prompt builder; test for `select_provider` logic; test that output files are written correctly
- **Integration tests** (`conduit-cli`): Test that `conduit run` fails gracefully when no provider is available; test that output files are created when a mock provider is used (via env var injection or binary stub)

---

## Dependencies Added

| Crate | Used for |
|---|---|
| `which = "6"` | Check if a CLI binary exists on PATH |

Added to `conduit-core/Cargo.toml`.

---

## Out of Scope for Phase 2

- Smart model picker (Phase 3)
- Multiple providers per task (Phase 3)
- Parallel task execution (Phase 4)
- Limit monitoring and failover (Phase 5)
- Context memory save/resume (Phase 6)
- Cost tracking (Phase 7)
- TUI dashboard (Phase 8)

---

## Success Criteria

Phase 2 is complete when:
1. `conduit run` executes the full 5-stage pipeline for each task
2. Each stage invokes the configured CLI agent and captures its output
3. Output files are written to `.conduit/tasks/<task-id>/`
4. Progress is printed per stage in real time
5. Failures stop the pipeline and display a clear error message
6. All existing Phase 1 tests still pass
7. New unit tests for provider and pipeline pass
