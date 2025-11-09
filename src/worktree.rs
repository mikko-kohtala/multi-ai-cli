use crate::error::{MultiAiError, Result};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader};

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

        let mut child = Command::new("gwt")
            .arg("add")
            .arg(branch_name)
            .current_dir(&self.project_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| MultiAiError::CommandFailed(format!("Failed to execute gwt: {}", e)))?;

        // Stream stdout
        if let Some(stdout) = child.stdout.take() {
            let reader = BufReader::new(stdout);
            for line in reader.lines().map_while(|r| r.ok()) {
                println!("    {}", line);
            }
        }

        // Wait for the process to complete and check status
        let status = child.wait()
            .map_err(|e| MultiAiError::CommandFailed(format!("Failed to wait for gwt: {}", e)))?;

        if !status.success() {
            // Capture any stderr output
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
                if stderr_msg.is_empty() { "Unknown error" } else { &stderr_msg }
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

    pub fn remove_worktree(&self, branch_name: &str) -> Result<()> {
        if !self.has_gwt_cli() {
            return Err(MultiAiError::Worktree(
                "gwt CLI is not installed or not in PATH".to_string()
            ));
        }

        let mut child = Command::new("gwt")
            .arg("remove")
            .arg(branch_name)
            .arg("--force")
            .current_dir(&self.project_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| MultiAiError::CommandFailed(format!("Failed to execute gwt remove: {}", e)))?;

        // Stream stdout
        if let Some(stdout) = child.stdout.take() {
            let reader = BufReader::new(stdout);
            for line in reader.lines().map_while(|r| r.ok()) {
                println!("    {}", line);
            }
        }

        // Wait for the process to complete and check status
        let status = child.wait()
            .map_err(|e| MultiAiError::CommandFailed(format!("Failed to wait for gwt remove: {}", e)))?;

        if !status.success() {
            // Capture any stderr output
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
                if stderr_msg.is_empty() { "Unknown error" } else { &stderr_msg }
            )));
        }

        Ok(())
    }

    pub fn is_gwt_project(&self) -> bool {
        // Check if git-worktree-config.jsonc exists (gwt configuration file)
        let gwt_config_jsonc = self.project_path.join("git-worktree-config.jsonc");
        if gwt_config_jsonc.exists() {
            return true;
        }
        
        // Also check for .yaml for backward compatibility
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