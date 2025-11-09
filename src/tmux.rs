use crate::config::{AiApp, TmuxLayout};
use crate::error::{MultiAiError, Result};
use std::process::Command;
use std::thread;
use std::time::Duration;

pub struct TmuxManager {
    session_name: String,
}

impl TmuxManager {
    pub fn new(project_name: &str, branch_prefix: &str) -> Self {
        let session_name = format!("{}-{}", project_name, branch_prefix);
        Self { session_name }
    }

    pub fn create_session(
        &self,
        _ai_apps: &[AiApp],
        worktree_paths: &[(AiApp, String)],
        layout: TmuxLayout,
    ) -> Result<()> {
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
            return Err(MultiAiError::Tmux(
                "No worktrees to create session for".to_string(),
            ));
        }

        match layout {
            TmuxLayout::MultiWindow => {
                let first = &worktree_paths[0];
                self.create_initial_window(&first.0, &first.1)?;

                for (ai_app, worktree_path) in worktree_paths.iter().skip(1) {
                    self.add_window(ai_app, worktree_path)?;
                }

                self.select_window_by_name(&worktree_paths[0].0)?;
            }
            TmuxLayout::SingleWindow => {
                self.create_single_window(worktree_paths)?;
                self.select_window("apps")?;
            }
        }

        Ok(())
    }

    fn select_window(&self, window: &str) -> Result<()> {
        let output = Command::new("tmux")
            .args([
                "select-window",
                "-t",
                &format!("{}:{}", self.session_name, window),
            ])
            .output()
            .map_err(|e| MultiAiError::CommandFailed(format!("Failed to select window: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(MultiAiError::Tmux(format!(
                "Failed to select window: {}",
                stderr
            )));
        }

        Ok(())
    }

    fn create_initial_window(&self, ai_app: &AiApp, worktree_path: &str) -> Result<()> {
        let output = Command::new("tmux")
            .args([
                "new-session",
                "-d",
                "-s",
                &self.session_name,
                "-n",
                ai_app.as_str(),
                "-c",
                worktree_path,
            ])
            .output()
            .map_err(|e| {
                MultiAiError::CommandFailed(format!("Failed to create tmux session: {}", e))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(MultiAiError::Tmux(format!(
                "Failed to create session: {}",
                stderr
            )));
        }

        self.split_window_for_ai(ai_app, worktree_path)?;

        Ok(())
    }

    fn add_window(&self, ai_app: &AiApp, worktree_path: &str) -> Result<()> {
        let output = Command::new("tmux")
            .args([
                "new-window",
                "-t",
                &format!("{}:", self.session_name),
                "-n",
                ai_app.as_str(),
                "-c",
                worktree_path,
            ])
            .output()
            .map_err(|e| MultiAiError::CommandFailed(format!("Failed to create window: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(MultiAiError::Tmux(format!(
                "Failed to create window: {}",
                stderr
            )));
        }

        self.split_window_for_ai(ai_app, worktree_path)?;

        Ok(())
    }

    fn split_window_for_ai(&self, ai_app: &AiApp, worktree_path: &str) -> Result<()> {
        // Capture the current (left) pane id before split so we can target it robustly
        let left_pane_id = self.current_pane_id(ai_app)?;

        // Split the window horizontally (creates a new pane to the right, focus stays on current)
        let output = Command::new("tmux")
            .args([
                "split-window",
                "-h",
                "-t",
                &format!("{}:{}", self.session_name, ai_app.as_str()),
                "-c",
                worktree_path,
                "-p",
                "50",
            ])
            .output()
            .map_err(|e| MultiAiError::CommandFailed(format!("Failed to split window: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(MultiAiError::Tmux(format!(
                "Failed to split window: {}",
                stderr
            )));
        }

        // Wait for shell to initialize
        thread::sleep(Duration::from_millis(500));

        // Launch the AI app in the left/original pane by id
        let launch_command = format!("cd {} && {}", worktree_path, ai_app.command());
        let output = Command::new("tmux")
            .args(["send-keys", "-t", &left_pane_id, &launch_command, "Enter"])
            .output()
            .map_err(|e| MultiAiError::CommandFailed(format!("Failed to launch AI app: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(MultiAiError::Tmux(format!(
                "Failed to launch AI app: {}",
                stderr
            )));
        }

        Ok(())
    }

    fn select_window_by_name(&self, ai_app: &AiApp) -> Result<()> {
        let output = Command::new("tmux")
            .args([
                "select-window",
                "-t",
                &format!("{}:{}", self.session_name, ai_app.as_str()),
            ])
            .output()
            .map_err(|e| MultiAiError::CommandFailed(format!("Failed to select window: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(MultiAiError::Tmux(format!(
                "Failed to select window: {}",
                stderr
            )));
        }

        Ok(())
    }

    pub fn attach_session(&self) -> Result<()> {
        let output = Command::new("tmux")
            .args(["attach-session", "-t", &self.session_name])
            .spawn()
            .map_err(|e| {
                MultiAiError::CommandFailed(format!("Failed to attach to session: {}", e))
            })?
            .wait()
            .map_err(|e| {
                MultiAiError::CommandFailed(format!("Failed to wait for session: {}", e))
            })?;

        if !output.success() {
            return Err(MultiAiError::Tmux(
                "Failed to attach to session".to_string(),
            ));
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

    pub fn kill_session(&self) -> Result<()> {
        if !self.is_tmux_installed() {
            return Err(MultiAiError::Tmux("tmux is not installed".to_string()));
        }

        if !self.session_exists()? {
            // Session doesn't exist, which is fine for remove command
            return Ok(());
        }

        let output = Command::new("tmux")
            .args(["kill-session", "-t", &self.session_name])
            .output()
            .map_err(|e| {
                MultiAiError::CommandFailed(format!("Failed to kill tmux session: {}", e))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(MultiAiError::Tmux(format!(
                "Failed to kill session: {}",
                stderr
            )));
        }

        Ok(())
    }

    fn is_tmux_installed(&self) -> bool {
        Command::new("tmux")
            .arg("-V")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    fn current_pane_id(&self, ai_app: &AiApp) -> Result<String> {
        let output = Command::new("tmux")
            .args([
                "display-message",
                "-p",
                "-t",
                &format!("{}:{}", self.session_name, ai_app.as_str()),
                "#{pane_id}",
            ])
            .output()
            .map_err(|e| MultiAiError::CommandFailed(format!("Failed to get pane id: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(MultiAiError::Tmux(format!(
                "Failed to get pane id: {}",
                stderr
            )));
        }

        let id = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(id)
    }

    fn current_pane_id_in_window(&self, window: &str) -> Result<String> {
        let output = Command::new("tmux")
            .args([
                "display-message",
                "-p",
                "-t",
                &format!("{}:{}", self.session_name, window),
                "#{pane_id}",
            ])
            .output()
            .map_err(|e| MultiAiError::CommandFailed(format!("Failed to get pane id: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(MultiAiError::Tmux(format!(
                "Failed to get pane id: {}",
                stderr
            )));
        }

        let id = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(id)
    }

    fn create_single_window(&self, worktree_paths: &[(AiApp, String)]) -> Result<()> {
        // Create a detached session with a single window named 'apps'
        let first = &worktree_paths[0];
        let window_name = "apps";
        let output = Command::new("tmux")
            .args([
                "new-session",
                "-d",
                "-s",
                &self.session_name,
                "-n",
                window_name,
                "-c",
                &first.1,
            ])
            .output()
            .map_err(|e| {
                MultiAiError::CommandFailed(format!("Failed to create tmux session: {}", e))
            })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(MultiAiError::Tmux(format!(
                "Failed to create session: {}",
                stderr
            )));
        }

        // Capture the initial pane id (first column)
        let mut column_panes: Vec<String> = Vec::with_capacity(worktree_paths.len());
        let first_pane = self.current_pane_id_in_window(window_name)?;
        column_panes.push(first_pane.clone());
        let mut last_col_pane = first_pane;

        // Create additional columns (one per remaining app)
        for (_i, (_app, path)) in worktree_paths.iter().enumerate().skip(1) {
            let output = Command::new("tmux")
                .args(["split-window", "-h", "-t", &last_col_pane, "-c", path])
                .output()
                .map_err(|e| {
                    MultiAiError::CommandFailed(format!("Failed to split column: {}", e))
                })?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(MultiAiError::Tmux(format!(
                    "Failed to split column: {}",
                    stderr
                )));
            }

            // The new pane becomes active; capture its id
            let new_pane = self.current_pane_id_in_window(window_name)?;
            column_panes.push(new_pane.clone());
            last_col_pane = new_pane;
        }

        // For each column, split vertically to create shell pane and launch AI in the top pane
        for (i, (ai_app, path)) in worktree_paths.iter().enumerate() {
            let top_pane = &column_panes[i];
            let output = Command::new("tmux")
                .args(["split-window", "-v", "-t", top_pane, "-c", path, "-p", "50"])
                .output()
                .map_err(|e| MultiAiError::CommandFailed(format!("Failed to split row: {}", e)))?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(MultiAiError::Tmux(format!(
                    "Failed to split row: {}",
                    stderr
                )));
            }

            // Allow shell to initialize
            thread::sleep(Duration::from_millis(500));

            // Launch AI command in the top pane
            let launch_command = format!("cd {} && {}", path, ai_app.command());
            let output = Command::new("tmux")
                .args(["send-keys", "-t", top_pane, &launch_command, "Enter"])
                .output()
                .map_err(|e| {
                    MultiAiError::CommandFailed(format!("Failed to launch AI app: {}", e))
                })?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(MultiAiError::Tmux(format!(
                    "Failed to launch AI app: {}",
                    stderr
                )));
            }
        }

        Ok(())
    }
}
