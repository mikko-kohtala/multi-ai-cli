use crate::config::AiApp;
use crate::error::{MultiAiError, Result};
use std::process::Command;
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub enum TmuxLayout {
    MultiWindow,
    SingleWindow,
}

// TmuxLayout is now internal only - TerminalMode in config.rs is used for external configuration

pub struct TmuxManager {
    session_name: String,
    layout: TmuxLayout,
}

impl TmuxManager {
    pub fn new(project_name: &str, branch_prefix: &str, layout: TmuxLayout) -> Self {
        let session_name = format!("{}-{}", project_name, branch_prefix);
        Self { session_name, layout }
    }

    pub fn create_session(&self, ai_apps: &[AiApp], worktree_paths: &[(AiApp, String)]) -> Result<()> {
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

        match self.layout {
            TmuxLayout::MultiWindow => self.create_session_multiwindow(worktree_paths),
            TmuxLayout::SingleWindow => self.create_session_singlewindow(ai_apps, worktree_paths),
        }
    }

    fn create_session_multiwindow(&self, worktree_paths: &[(AiApp, String)]) -> Result<()> {
        let first = &worktree_paths[0];
        self.create_initial_window(&first.0, &first.1)?;

        for (ai_app, worktree_path) in worktree_paths.iter().skip(1) {
            self.add_window(ai_app, worktree_path)?;
        }

        self.select_first_window()?;

        Ok(())
    }

    fn create_session_singlewindow(&self, _ai_apps: &[AiApp], worktree_paths: &[(AiApp, String)]) -> Result<()> {
        // Create base session with first AI app's worktree
        let first = &worktree_paths[0];
        let output = Command::new("tmux")
            .args([
                "new-session",
                "-d",
                "-s", &self.session_name,
                "-n", "all-apps",
                "-c", &first.1,
            ])
            .output()
            .map_err(|e| MultiAiError::CommandFailed(format!("Failed to create tmux session: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(MultiAiError::Tmux(format!("Failed to create session: {}", stderr)));
        }

        // Small delay to ensure session is fully initialized
        thread::sleep(Duration::from_millis(100));

        // Create horizontal splits for additional AI apps (creating columns side-by-side)
        // NOTE: tmux panes are indexed starting from 1, not 0!
        // We always target the leftmost pane (pane 1) and split it to create equal columns
        for (idx, (_, worktree_path)) in worktree_paths.iter().enumerate().skip(1) {
            let num_apps = worktree_paths.len();
            let percentage = self.calculate_split_percentage(idx, num_apps);

            // Split pane 1 horizontally (this creates a new pane to the right)
            let output = Command::new("tmux")
                .args([
                    "split-window",
                    "-h",  // Horizontal split creates columns (left-right)
                    "-t", &format!("{}:all-apps.1", self.session_name),  // Always split pane 1 (leftmost)
                    "-c", worktree_path,
                    "-p", &percentage.to_string(),
                ])
                .output()
                .map_err(|e| MultiAiError::CommandFailed(format!("Failed to split horizontally: {}", e)))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(MultiAiError::Tmux(format!("Failed to split window horizontally: {}", stderr)));
            }

            // Select pane 1 again to ensure next split targets the leftmost pane
            Command::new("tmux")
                .args([
                    "select-pane",
                    "-t", &format!("{}:all-apps.1", self.session_name),
                ])
                .output()
                .map_err(|e| MultiAiError::CommandFailed(format!("Failed to select pane: {}", e)))?;
        }

        // Now split each column vertically to add a shell pane below the AI pane
        // After the horizontal splits above, we have panes 1, 2, 3, ... (left to right)
        // NOTE: tmux uses 1-based indexing!
        for (col_idx, (ai_app, worktree_path)) in worktree_paths.iter().enumerate() {
            let pane_num = col_idx + 1;  // Convert 0-based index to 1-based pane number

            // Split this column vertically (top-bottom)
            let output = Command::new("tmux")
                .args([
                    "split-window",
                    "-v",  // Vertical split creates rows (top-bottom)
                    "-t", &format!("{}:all-apps.{}", self.session_name, pane_num),
                    "-c", worktree_path,
                    "-p", "50",  // 50% split
                ])
                .output()
                .map_err(|e| MultiAiError::CommandFailed(format!("Failed to split column vertically: {}", e)))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(MultiAiError::Tmux(format!("Failed to split column {}: {}", pane_num, stderr)));
            }

            // After splitting vertically, the pane numbering changes:
            // If we had panes [1, 2, 3] and split pane 1, we now have [1(top), 2(bottom), 3, 4]
            // After splitting pane 3 (which was originally pane 2), we have [1, 2, 3(top), 4(bottom), 5]
            // The top pane for each column is at: (col_idx * 2) + 1

            thread::sleep(Duration::from_millis(500));

            // Send the AI command to the top pane of this column
            let top_pane_idx = (col_idx * 2) + 1;
            let launch_command = format!("cd {} && {}", worktree_path, ai_app.command());
            let output = Command::new("tmux")
                .args([
                    "send-keys",
                    "-t", &format!("{}:all-apps.{}", self.session_name, top_pane_idx),
                    &launch_command,
                    "Enter",
                ])
                .output()
                .map_err(|e| MultiAiError::CommandFailed(format!("Failed to send command: {}", e)))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(MultiAiError::Tmux(format!("Failed to send AI command: {}", stderr)));
            }
        }

        // Select the first pane (top-left)
        Command::new("tmux")
            .args([
                "select-pane",
                "-t", &format!("{}:all-apps.1", self.session_name),  // Pane 1, not 0
            ])
            .output()
            .map_err(|e| MultiAiError::CommandFailed(format!("Failed to select pane: {}", e)))?;

        Ok(())
    }

    fn calculate_split_percentage(&self, current_idx: usize, total: usize) -> usize {
        // Calculate the percentage for the new pane in a split
        // This ensures equal distribution of space
        // When we split pane 0, the new pane should take up 1/remaining of the current pane's space
        let remaining_panes = total - current_idx;
        100 / remaining_panes
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
        // Split the window horizontally (creates pane 1 on the right, keeps focus on pane 0)
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

        // Wait for shell to initialize
        thread::sleep(Duration::from_millis(500));

        // Launch the AI app in the left pane (pane 1)
        let launch_command = format!("cd {} && {}", worktree_path, ai_app.command());
        let output = Command::new("tmux")
            .args([
                "send-keys",
                "-t", &format!("{}:{}.1", self.session_name, ai_app.as_str()),
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
            .map_err(|e| MultiAiError::CommandFailed(format!("Failed to kill tmux session: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(MultiAiError::Tmux(format!("Failed to kill session: {}", stderr)));
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
}