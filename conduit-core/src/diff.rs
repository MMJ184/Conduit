use crate::error::ConduitError;
use std::path::Path;
use std::process::Command;

/// Extract a unified diff from a fenced ```diff``` block in the agent's response.
/// Returns the diff text without fences, or None if no diff block is found.
pub fn extract_diff_block(raw: &str) -> Option<String> {
    let start_marker = "```diff";
    let start = raw.find(start_marker)?;
    let after_marker = &raw[start + start_marker.len()..];
    let body_start = after_marker.find('\n')? + 1;
    let body = &after_marker[body_start..];
    let end_rel = body.find("```")?;
    Some(body[..end_rel].to_string())
}

/// Run `git apply --check` against the diff to validate. Returns Ok(()) if applies cleanly.
pub fn check_diff(diff_text: &str, project_dir: &Path) -> Result<(), ConduitError> {
    use std::io::Write;
    let mut child = Command::new("git")
        .args(["apply", "--check", "-"])
        .current_dir(project_dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| ConduitError::DiffApplyFailed { reason: format!("git apply --check spawn failed: {}", e) })?;
    {
        let stdin = child.stdin.as_mut()
            .ok_or_else(|| ConduitError::DiffApplyFailed { reason: "no stdin".to_string() })?;
        stdin.write_all(diff_text.as_bytes())
            .map_err(|e| ConduitError::DiffApplyFailed { reason: e.to_string() })?;
    }
    let output = child.wait_with_output()
        .map_err(|e| ConduitError::DiffApplyFailed { reason: e.to_string() })?;
    if !output.status.success() {
        return Err(ConduitError::DiffApplyFailed {
            reason: String::from_utf8_lossy(&output.stderr).to_string(),
        });
    }
    Ok(())
}

/// Apply the diff (no --check). Caller should call check_diff first.
pub fn apply_diff(diff_text: &str, project_dir: &Path) -> Result<(), ConduitError> {
    use std::io::Write;
    let mut child = Command::new("git")
        .args(["apply", "-"])
        .current_dir(project_dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| ConduitError::DiffApplyFailed { reason: format!("git apply spawn failed: {}", e) })?;
    {
        let stdin = child.stdin.as_mut()
            .ok_or_else(|| ConduitError::DiffApplyFailed { reason: "no stdin".to_string() })?;
        stdin.write_all(diff_text.as_bytes())
            .map_err(|e| ConduitError::DiffApplyFailed { reason: e.to_string() })?;
    }
    let output = child.wait_with_output()
        .map_err(|e| ConduitError::DiffApplyFailed { reason: e.to_string() })?;
    if !output.status.success() {
        return Err(ConduitError::DiffApplyFailed {
            reason: String::from_utf8_lossy(&output.stderr).to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn init_git_repo_with_file(dir: &Path) {
        Command::new("git").args(["init"]).current_dir(dir).output().unwrap();
        Command::new("git").args(["config", "user.email", "t@t"]).current_dir(dir).output().unwrap();
        Command::new("git").args(["config", "user.name", "T"]).current_dir(dir).output().unwrap();
        fs::write(dir.join("hello.txt"), "hello\n").unwrap();
        Command::new("git").args(["add", "."]).current_dir(dir).output().unwrap();
        Command::new("git").args(["commit", "-m", "i"]).current_dir(dir).output().unwrap();
    }

    #[test]
    fn test_extract_diff_block_simple() {
        let raw = "Here is the change:\n\n```diff\ndiff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-old\n+new\n```\n";
        let d = extract_diff_block(raw).unwrap();
        assert!(d.contains("diff --git a/x b/x"));
        assert!(d.contains("+new"));
    }

    #[test]
    fn test_extract_diff_block_no_block_returns_none() {
        assert!(extract_diff_block("no diff here").is_none());
    }

    #[test]
    fn test_check_diff_clean_succeeds() {
        let dir = tempdir().unwrap();
        init_git_repo_with_file(dir.path());
        let diff = "diff --git a/hello.txt b/hello.txt\n--- a/hello.txt\n+++ b/hello.txt\n@@ -1 +1 @@\n-hello\n+goodbye\n";
        check_diff(diff, dir.path()).unwrap();
    }

    #[test]
    fn test_check_diff_invalid_fails() {
        let dir = tempdir().unwrap();
        init_git_repo_with_file(dir.path());
        let diff = "diff --git a/missing.txt b/missing.txt\n--- a/missing.txt\n+++ b/missing.txt\n@@ -1 +1 @@\n-old\n+new\n";
        let err = check_diff(diff, dir.path()).unwrap_err();
        assert!(matches!(err, ConduitError::DiffApplyFailed { .. }));
    }

    #[test]
    fn test_apply_diff_modifies_file() {
        let dir = tempdir().unwrap();
        init_git_repo_with_file(dir.path());
        let diff = "diff --git a/hello.txt b/hello.txt\n--- a/hello.txt\n+++ b/hello.txt\n@@ -1 +1 @@\n-hello\n+goodbye\n";
        apply_diff(diff, dir.path()).unwrap();
        let content = fs::read_to_string(dir.path().join("hello.txt")).unwrap();
        assert_eq!(content.trim_end_matches(['\n', '\r']), "goodbye");
    }
}
