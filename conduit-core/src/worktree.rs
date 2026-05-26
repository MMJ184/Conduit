use std::path::{Path, PathBuf};
use std::process::Command;
use crate::error::ConduitError;

#[derive(Debug)]
pub struct TaskWorktree {
    pub path: PathBuf,
    pub task_id: String,
    pub branch: String,
    keep_on_drop: bool,
}

impl TaskWorktree {
    /// Create a new git worktree at .conduit/work/<task-id>/ on a fresh branch.
    /// Returns NotAGitRepo if project_dir is not a git repo.
    pub fn create(project_dir: &Path, task_id: &str) -> Result<Self, ConduitError> {
        if !is_git_repo(project_dir) {
            return Err(ConduitError::NotAGitRepo);
        }
        let work_root = project_dir.join(".conduit").join("work");
        std::fs::create_dir_all(&work_root)?;
        let path = work_root.join(task_id);
        let branch = format!("conduit/task/{}", task_id);

        // If the path or branch already exists from a prior run, prune first.
        let _ = Command::new("git")
            .args(["worktree", "remove", "--force"])
            .arg(&path)
            .current_dir(project_dir)
            .output();
        let _ = Command::new("git")
            .args(["branch", "-D", &branch])
            .current_dir(project_dir)
            .output();

        let output = Command::new("git")
            .args(["worktree", "add", "-b", &branch])
            .arg(&path)
            .current_dir(project_dir)
            .output()
            .map_err(|e| ConduitError::WorktreeError {
                task_id: task_id.to_string(),
                reason: e.to_string(),
            })?;

        if !output.status.success() {
            return Err(ConduitError::WorktreeError {
                task_id: task_id.to_string(),
                reason: String::from_utf8_lossy(&output.stderr).to_string(),
            });
        }

        Ok(Self {
            path,
            task_id: task_id.to_string(),
            branch,
            keep_on_drop: false,
        })
    }

    /// Mark the worktree to be retained when this struct is dropped.
    /// Used when a task fails — caller wants to inspect the partial work.
    pub fn keep(&mut self) {
        self.keep_on_drop = true;
    }
}

impl Drop for TaskWorktree {
    fn drop(&mut self) {
        if self.keep_on_drop {
            return;
        }
        let project_root = find_project_root(&self.path);
        if let Some(root) = project_root {
            let _ = Command::new("git")
                .args(["worktree", "remove", "--force"])
                .arg(&self.path)
                .current_dir(&root)
                .output();
            let _ = Command::new("git")
                .args(["branch", "-D", &self.branch])
                .current_dir(&root)
                .output();
        }
    }
}

fn is_git_repo(dir: &Path) -> bool {
    Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .current_dir(dir)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn find_project_root(start: &Path) -> Option<PathBuf> {
    // .conduit/work/<task-id> is 3 dirs below the project root
    let mut p = start.to_path_buf();
    for _ in 0..3 {
        if !p.pop() {
            return None;
        }
    }
    Some(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn init_git_repo(dir: &Path) {
        Command::new("git").args(["init"]).current_dir(dir).output().unwrap();
        Command::new("git").args(["config", "user.email", "test@conduit.local"]).current_dir(dir).output().unwrap();
        Command::new("git").args(["config", "user.name", "Test"]).current_dir(dir).output().unwrap();
        fs::write(dir.join("README.md"), "init").unwrap();
        Command::new("git").args(["add", "."]).current_dir(dir).output().unwrap();
        Command::new("git").args(["commit", "-m", "init"]).current_dir(dir).output().unwrap();
    }

    #[test]
    fn test_create_worktree_in_non_git_dir_returns_not_a_git_repo() {
        let dir = tempdir().unwrap();
        let err = TaskWorktree::create(dir.path(), "task-a").unwrap_err();
        assert!(matches!(err, ConduitError::NotAGitRepo));
    }

    #[test]
    fn test_create_worktree_succeeds_in_git_repo() {
        let dir = tempdir().unwrap();
        init_git_repo(dir.path());
        let wt = TaskWorktree::create(dir.path(), "task-a").unwrap();
        assert!(wt.path.exists(), "worktree path should exist after create");
        assert_eq!(wt.task_id, "task-a");
        assert!(wt.branch.contains("task-a"));
    }

    #[test]
    fn test_worktree_cleaned_up_on_drop() {
        let dir = tempdir().unwrap();
        init_git_repo(dir.path());
        let wt_path = {
            let wt = TaskWorktree::create(dir.path(), "task-b").unwrap();
            let p = wt.path.clone();
            assert!(p.exists());
            p
        };
        assert!(!wt_path.exists(), "worktree should be removed when struct is dropped");
    }

    #[test]
    fn test_worktree_retained_when_keep_called() {
        let dir = tempdir().unwrap();
        init_git_repo(dir.path());
        let wt_path = {
            let mut wt = TaskWorktree::create(dir.path(), "task-c").unwrap();
            wt.keep();
            wt.path.clone()
        };
        assert!(wt_path.exists(), "worktree should be retained when keep() was called");
    }
}
