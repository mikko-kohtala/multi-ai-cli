use crate::config::AiApp;
use crate::error::{MultiAiError, Result};
use std::process::Command;

pub struct TmuxManager {
    session_name: String,
}

impl TmuxManager {
    pub fn new(project_name: &str, branch_prefix: &str) -> Self {
        let session_name = format!("{}-{}", project_name, branch_prefix);
        Self { session_name }
    }

    pub fn create_session(&self, _ai_apps: &[AiApp], worktree_paths: &[(AiApp, String)]) -> Result<()> {
        if !self.is_tmux_installed() {
            return Err(MultiAiError::Tmux("tmux is not installed".to_string()));
        }

        if self.session_exists()? {
            return Err(MultiAiError::Tmux(format!(
                "Session '{}' already exists",
                self.session_name
            )));
        }

        if worktree_paths.is_empty() {
            return Err(MultiAiError::Tmux("No worktrees to create session for".to_string()));
        }

        let first = &worktree_paths[0];
        self.create_initial_window(&first.0, &first.1)?;

        for (ai_app, worktree_path) in worktree_paths.iter().skip(1) {
            self.add_window(ai_app, worktree_path)?;
        }

        self.select_first_window()?;

        Ok(())
    }

    fn create_initial_window(&self, ai_app: &AiApp, worktree_path: &str) -> Result<()> {
        let output = Command::new("tmux")
            .args([
                "new-session",
                "-d",
                "-s", &self.session_name,
                "-n", ai_app.as_str(),
                "-c", worktree_path,
            ])
            .output()
            .map_err(|e| MultiAiError::CommandFailed(format!("Failed to create tmux session: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(MultiAiError::Tmux(format!("Failed to create session: {}", stderr)));
        }

        self.split_window_for_ai(ai_app, worktree_path)?;

        Ok(())
    }

    fn add_window(&self, ai_app: &AiApp, worktree_path: &str) -> Result<()> {
        let output = Command::new("tmux")
            .args([
                "new-window",
                "-t", &format!("{}:", self.session_name),
                "-n", ai_app.as_str(),
                "-c", worktree_path,
            ])
            .output()
            .map_err(|e| MultiAiError::CommandFailed(format!("Failed to create window: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(MultiAiError::Tmux(format!("Failed to create window: {}", stderr)));
        }

        self.split_window_for_ai(ai_app, worktree_path)?;

        Ok(())
    }

    fn split_window_for_ai(&self, ai_app: &AiApp, worktree_path: &str) -> Result<()> {
        let output = Command::new("tmux")
            .args([
                "split-window",
                "-h",
                "-t", &format!("{}:{}", self.session_name, ai_app.as_str()),
                "-c", worktree_path,
                "-p", "50",
            ])
            .output()
            .map_err(|e| MultiAiError::CommandFailed(format!("Failed to split window: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(MultiAiError::Tmux(format!("Failed to split window: {}", stderr)));
        }

        let launch_command = format!("cd {} && {}", worktree_path, ai_app.command());
        let output = Command::new("tmux")
            .args([
                "send-keys",
                "-t", &format!("{}:{}.0", self.session_name, ai_app.as_str()),
                &launch_command,
                "Enter",
            ])
            .output()
            .map_err(|e| MultiAiError::CommandFailed(format!("Failed to launch AI app: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(MultiAiError::Tmux(format!("Failed to launch AI app: {}", stderr)));
        }

        Ok(())
    }

    fn select_first_window(&self) -> Result<()> {
        Command::new("tmux")
            .args([
                "select-window",
                "-t", &format!("{}:0", self.session_name),
            ])
            .output()
            .map_err(|e| MultiAiError::CommandFailed(format!("Failed to select window: {}", e)))?;

        Ok(())
    }

    pub fn attach_session(&self) -> Result<()> {
        let output = Command::new("tmux")
            .args(["attach-session", "-t", &self.session_name])
            .spawn()
            .map_err(|e| MultiAiError::CommandFailed(format!("Failed to attach to session: {}", e)))?
            .wait()
            .map_err(|e| MultiAiError::CommandFailed(format!("Failed to wait for session: {}", e)))?;

        if !output.success() {
            return Err(MultiAiError::Tmux("Failed to attach to session".to_string()));
        }

        Ok(())
    }

    fn session_exists(&self) -> Result<bool> {
        let output = Command::new("tmux")
            .args(["has-session", "-t", &self.session_name])
            .output()
            .map_err(|e| MultiAiError::CommandFailed(format!("Failed to check session: {}", e)))?;

        Ok(output.status.success())
    }

    fn is_tmux_installed(&self) -> bool {
        Command::new("tmux")
            .arg("-V")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }
}