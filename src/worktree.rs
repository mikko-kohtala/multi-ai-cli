use crate::error::{MultiAiError, Result};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Protected branches that should never be deleted by remove operations.
const PROTECTED_BRANCHES: &[&str] = &["main", "master", "develop", "dev"];

pub struct WorktreeManager {
    project_path: PathBuf,
    worktrees_path: PathBuf,
}

impl WorktreeManager {
    pub fn new(project_path: PathBuf) -> Self {
        let worktrees_path = project_path.clone();
        Self {
            project_path,
            worktrees_path,
        }
    }

    /// Create a WorktreeManager with an explicit worktrees path override.
    pub fn with_worktrees_path(project_path: PathBuf, worktrees_path: PathBuf) -> Self {
        Self {
            project_path,
            worktrees_path,
        }
    }

    pub fn project_path(&self) -> &Path {
        &self.project_path
    }

    pub fn worktrees_path(&self) -> &Path {
        &self.worktrees_path
    }

    pub fn add_worktree(&self, branch_name: &str) -> Result<PathBuf> {
        let worktree_path = self.worktrees_path.join(branch_name);

        // Determine branch existence and build the right git worktree add command
        let local_exists = self.branch_exists_locally(branch_name);
        let remote_exists = self.branch_exists_remotely(branch_name);

        let mut cmd = Command::new("git");
        cmd.arg("worktree").arg("add");

        if local_exists {
            // Local branch exists — just create worktree pointing to it
            cmd.arg(&worktree_path).arg(branch_name);
        } else if remote_exists {
            // Remote branch exists — create local tracking branch
            cmd.arg("-b")
                .arg(branch_name)
                .arg(&worktree_path)
                .arg(format!("origin/{}", branch_name));
        } else {
            // Brand new branch — create from default branch
            let default_branch = crate::git::get_default_branch(&self.project_path);
            cmd.arg("--no-track")
                .arg("-b")
                .arg(branch_name)
                .arg(&worktree_path)
                .arg(format!("origin/{}", default_branch));
        }

        let mut child = cmd
            .current_dir(&self.project_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                MultiAiError::CommandFailed(format!("Failed to execute git worktree add: {}", e))
            })?;

        // Stream stdout
        if let Some(stdout) = child.stdout.take() {
            let reader = BufReader::new(stdout);
            for line in reader.lines().map_while(|r| r.ok()) {
                println!("    {}", line);
            }
        }

        let status = child.wait().map_err(|e| {
            MultiAiError::CommandFailed(format!("Failed to wait for git worktree add: {}", e))
        })?;

        if !status.success() {
            let mut stderr_msg = String::new();
            if let Some(stderr) = child.stderr.take() {
                let reader = BufReader::new(stderr);
                for line in reader.lines().map_while(|r| r.ok()) {
                    stderr_msg.push_str(&line);
                    stderr_msg.push('\n');
                }
            }

            return Err(MultiAiError::Worktree(format!(
                "Failed to create worktree: {}",
                if stderr_msg.is_empty() {
                    "Unknown error"
                } else {
                    &stderr_msg
                }
            )));
        }

        Ok(worktree_path)
    }

    pub fn remove_worktree(&self, branch_name: &str) -> Result<()> {
        self.remove_worktree_impl(branch_name, true)
    }

    pub fn remove_worktree_quiet(&self, branch_name: &str) -> Result<()> {
        self.remove_worktree_impl(branch_name, false)
    }

    fn remove_worktree_impl(&self, branch_name: &str, verbose: bool) -> Result<()> {
        let worktree_path = self.worktrees_path.join(branch_name);

        // Remove the worktree
        let mut cmd = Command::new("git");
        cmd.arg("worktree")
            .arg("remove")
            .arg("--force")
            .arg(&worktree_path)
            .current_dir(&self.project_path);

        if verbose {
            cmd.stdout(Stdio::piped());
        } else {
            cmd.stdout(Stdio::null());
        }
        cmd.stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| {
            MultiAiError::CommandFailed(format!("Failed to execute git worktree remove: {}", e))
        })?;

        if verbose && let Some(stdout) = child.stdout.take() {
            let reader = BufReader::new(stdout);
            for line in reader.lines().map_while(|r| r.ok()) {
                println!("    {}", line);
            }
        }

        let status = child.wait().map_err(|e| {
            MultiAiError::CommandFailed(format!("Failed to wait for git worktree remove: {}", e))
        })?;

        if !status.success() {
            let mut stderr_msg = String::new();
            if let Some(stderr) = child.stderr.take() {
                let reader = BufReader::new(stderr);
                for line in reader.lines().map_while(|r| r.ok()) {
                    stderr_msg.push_str(&line);
                    stderr_msg.push('\n');
                }
            }

            return Err(MultiAiError::Worktree(format!(
                "Failed to remove worktree: {}",
                if stderr_msg.is_empty() {
                    "Unknown error"
                } else {
                    &stderr_msg
                }
            )));
        }

        // Delete the branch (skip protected branches)
        if !PROTECTED_BRANCHES.contains(&branch_name) {
            let delete_result = Command::new("git")
                .args(["branch", "-D", branch_name])
                .current_dir(&self.project_path)
                .output();

            if verbose {
                match delete_result {
                    Ok(output) if output.status.success() => {
                        println!("    Deleted branch {}", branch_name);
                    }
                    Ok(output) => {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        eprintln!("    Warning: could not delete branch: {}", stderr.trim());
                    }
                    Err(e) => {
                        eprintln!("    Warning: could not delete branch: {}", e);
                    }
                }
            }
        }

        // Prune stale worktree references
        let _ = Command::new("git")
            .args(["worktree", "prune"])
            .current_dir(&self.project_path)
            .output();

        Ok(())
    }

    pub fn worktrees_exist(&self, branch_prefix: &str, ai_app_names: &[String]) -> bool {
        ai_app_names.iter().all(|app_name| {
            let branch_name = format!("{}-{}", branch_prefix, app_name);
            let worktree_path = self.worktrees_path.join(&branch_name);
            worktree_path.exists() && worktree_path.is_dir()
        })
    }

    fn branch_exists_locally(&self, branch_name: &str) -> bool {
        Command::new("git")
            .args([
                "rev-parse",
                "--verify",
                &format!("refs/heads/{}", branch_name),
            ])
            .current_dir(&self.project_path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    fn branch_exists_remotely(&self, branch_name: &str) -> bool {
        Command::new("git")
            .args([
                "rev-parse",
                "--verify",
                &format!("refs/remotes/origin/{}", branch_name),
            ])
            .current_dir(&self.project_path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}
