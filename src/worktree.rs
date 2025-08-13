use crate::error::{MultiAiError, Result};
use std::path::PathBuf;
use std::process::Command;

pub struct WorktreeManager {
    project_path: PathBuf,
}

impl WorktreeManager {
    pub fn new(project_path: PathBuf) -> Self {
        Self { project_path }
    }

    pub fn add_worktree(&self, branch_name: &str) -> Result<PathBuf> {
        let worktree_path = self.project_path.join(branch_name);
        
        if !self.has_gwt_cli() {
            return Err(MultiAiError::Worktree(
                "gwt CLI is not installed or not in PATH".to_string()
            ));
        }

        let output = Command::new("gwt")
            .arg("add")
            .arg(branch_name)
            .current_dir(&self.project_path)
            .output()
            .map_err(|e| MultiAiError::CommandFailed(format!("Failed to execute gwt: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(MultiAiError::Worktree(format!(
                "Failed to create worktree: {}",
                stderr
            )));
        }

        Ok(worktree_path)
    }

    pub fn has_gwt_cli(&self) -> bool {
        Command::new("gwt")
            .arg("--version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    pub fn is_gwt_project(&self) -> bool {
        // Check if git-worktree-config.yaml exists (gwt configuration file)
        let gwt_config_yaml = self.project_path.join("git-worktree-config.yaml");
        if gwt_config_yaml.exists() {
            return true;
        }
        
        // Also try running gwt list to see if it's a valid gwt project
        Command::new("gwt")
            .arg("list")
            .current_dir(&self.project_path)
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }
}