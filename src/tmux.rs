use crate::config::{AiApp, TmuxLayout};
use crate::error::{MultiAiError, Result};
use std::io::Write;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct PaneInfo {
    pub id: String,
    pub left: u32,
    pub top: u32,
}

pub struct TmuxManager {
    session_name: String,
}

impl TmuxManager {
    pub fn new(project_name: &str, branch_prefix: &str) -> Self {
        let session_name = format!("{}-{}", project_name, branch_prefix);
        Self { session_name }
    }

    pub fn from_session_name(session_name: &str) -> Self {
        Self {
            session_name: session_name.to_string(),
        }
    }

    pub fn list_sessions() -> Result<Vec<String>> {
        if !Self::is_tmux_installed() {
            return Err(MultiAiError::Tmux(
                "tmux is not installed or not in PATH".to_string(),
            ));
        }

        let output = Command::new("tmux")
            .args(["list-sessions", "-F", "#S"])
            .output()
            .map_err(|e| MultiAiError::CommandFailed(format!("Failed to list sessions: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(MultiAiError::Tmux(format!(
                "Failed to list tmux sessions: {}",
                stderr.trim()
            )));
        }

        let sessions = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|s| s.to_string())
            .collect();

        Ok(sessions)
    }

    pub fn create_session(
        &self,
        _ai_apps: &[AiApp],
        worktree_paths: &[(AiApp, String)],
        layout: TmuxLayout,
    ) -> Result<()> {
        if !Self::is_tmux_installed() {
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

    pub fn list_panes_in_window(&self, window: &str) -> Result<Vec<PaneInfo>> {
        let target = format!("{}:{}", self.session_name, window);
        let output = Command::new("tmux")
            .args([
                "list-panes",
                "-F",
                "#{pane_id}\t#{pane_left}\t#{pane_top}",
                "-t",
                &target,
            ])
            .output()
            .map_err(|e| MultiAiError::CommandFailed(format!("Failed to list panes: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(MultiAiError::Tmux(format!(
                "Failed to list panes for {}: {}",
                target,
                stderr.trim()
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut panes = Vec::new();
        for line in stdout.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() < 3 {
                continue;
            }
            let left = parts[1].parse::<u32>().unwrap_or(0);
            let top = parts[2].parse::<u32>().unwrap_or(0);
            panes.push(PaneInfo {
                id: parts[0].to_string(),
                left,
                top,
            });
        }

        Ok(panes)
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

    pub fn paste_text_to_pane(&self, pane_id: &str, text: &str, send_enter: bool) -> Result<()> {
        let buffer_name = format!("mai-send-{}", self.session_name);

        // Load buffer with provided text
        let mut load = Command::new("tmux")
            .args(["load-buffer", "-b", &buffer_name, "-"])
            .stdin(Stdio::piped())
            .spawn()
            .map_err(|e| {
                MultiAiError::CommandFailed(format!("Failed to load tmux buffer: {}", e))
            })?;

        if let Some(stdin) = load.stdin.as_mut() {
            stdin.write_all(text.as_bytes()).map_err(|e| {
                MultiAiError::CommandFailed(format!("Failed to write to tmux buffer: {}", e))
            })?;
        }

        // Close stdin to signal EOF
        let _ = load.stdin.take();

        let status = load
            .wait()
            .map_err(|e| MultiAiError::CommandFailed(format!("Failed to load buffer: {}", e)))?;

        if !status.success() {
            return Err(MultiAiError::Tmux("tmux load-buffer failed".to_string()));
        }

        let paste = Command::new("tmux")
            .args(["paste-buffer", "-b", &buffer_name, "-t", pane_id])
            .output()
            .map_err(|e| MultiAiError::CommandFailed(format!("Failed to paste buffer: {}", e)))?;

        if !paste.status.success() {
            let stderr = String::from_utf8_lossy(&paste.stderr);
            return Err(MultiAiError::Tmux(format!(
                "Failed to paste buffer: {}",
                stderr.trim()
            )));
        }

        if send_enter {
            let output = Command::new("tmux")
                .args(["send-keys", "-t", pane_id, "Enter"])
                .output()
                .map_err(|e| {
                    MultiAiError::CommandFailed(format!("Failed to send enter key: {}", e))
                })?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(MultiAiError::Tmux(format!(
                    "Failed to send enter key: {}",
                    stderr.trim()
                )));
            }
        }

        // Best-effort cleanup
        let _ = Command::new("tmux")
            .args(["delete-buffer", "-b", &buffer_name])
            .output();

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
        if !Self::is_tmux_installed() {
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

    pub fn is_tmux_installed() -> bool {
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

        // Capture the initial pane id (leftmost/first column)
        let mut column_panes: Vec<String> = Vec::with_capacity(worktree_paths.len());
        let leftmost_pane = self.current_pane_id_in_window(window_name)?;
        column_panes.push(leftmost_pane.clone());

        // Create additional columns by repeatedly splitting the LEFTMOST pane.
        // Using percentages based on the remaining column count yields equal-width columns.
        // We insert each newly created pane just to the right of the leftmost entry so that
        // column_panes remains in left-to-right order matching worktree_paths.
        for (idx, (_app, path)) in worktree_paths.iter().enumerate().skip(1) {
            let total = worktree_paths.len();
            let percentage = self.calculate_split_percentage(idx, total);

            let output = Command::new("tmux")
                .args([
                    "split-window",
                    "-h",
                    "-t",
                    &leftmost_pane,
                    "-c",
                    path,
                    "-p",
                    &percentage.to_string(),
                ])
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

            // The new pane becomes active; capture its id as the top pane for this column
            let new_pane = self.current_pane_id_in_window(window_name)?;
            // Insert directly to the right of the leftmost entry to preserve left-to-right order
            column_panes.insert(1, new_pane.clone());
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

    // Calculate the percentage for equal-width columns when repeatedly splitting the leftmost pane.
    // For total=N columns, on the k-th split (current_idx = k, starting at 1), the leftmost pane
    // currently has width (N - k + 1)/N of the whole window. To create a new column of width 1/N
    // total, the new pane must take a fraction 1/(N - k + 1) of the leftmost pane.
    fn calculate_split_percentage(&self, current_idx: usize, total: usize) -> usize {
        let remaining = total - current_idx + 1; // remaining columns including the leftmost
        100 / remaining
    }
}
