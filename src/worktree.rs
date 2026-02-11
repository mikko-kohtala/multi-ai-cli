use crate::error::{MultiAiError, Result};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub struct WorktreeManager {
    project_path: PathBuf,
    worktrees_path: PathBuf,
}

impl WorktreeManager {
    pub fn new(project_path: PathBuf) -> Self {
        let worktrees_path =
            Self::read_worktrees_path(&project_path).unwrap_or_else(|| project_path.clone());
        Self {
            project_path,
            worktrees_path,
        }
    }

    /// Create a WorktreeManager with an explicit worktrees path override.
    /// The override takes precedence over gwt config discovery.
    pub fn with_worktrees_path(project_path: PathBuf, worktrees_path: PathBuf) -> Self {
        Self {
            project_path,
            worktrees_path,
        }
    }

    /// Read the worktreesPath from gwt config file (public wrapper for init)
    pub fn read_worktrees_path_public(project_path: &Path) -> Option<PathBuf> {
        Self::read_worktrees_path(project_path)
    }

    /// Read the worktreesPath from gwt config file
    fn read_worktrees_path(project_path: &Path) -> Option<PathBuf> {
        // Try local config first
        let local_config = project_path.join("git-worktree-config.jsonc");
        if let Some(path) = Self::parse_worktrees_path_from_file(&local_config) {
            return Some(path);
        }

        // Try ./main/ subdirectory
        let main_config = project_path.join("main").join("git-worktree-config.jsonc");
        if let Some(path) = Self::parse_worktrees_path_from_file(&main_config) {
            return Some(path);
        }

        // Try global gwt configs
        let home_dir = dirs::home_dir()?;
        let gwt_projects_dir = home_dir
            .join(".config")
            .join("git-worktree-cli")
            .join("projects");
        if !gwt_projects_dir.exists() {
            return None;
        }

        let project_path_canonical = project_path.canonicalize().ok();

        for entry in std::fs::read_dir(&gwt_projects_dir).ok()?.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "jsonc").unwrap_or(false) {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(Some(serde_json::Value::Object(map))) =
                        jsonc_parser::parse_to_serde_value(&content, &Default::default())
                    {
                        // Check if this config matches our project path
                        let matches = if let Some(serde_json::Value::String(proj_path)) =
                            map.get("projectPath")
                        {
                            let config_proj_path = PathBuf::from(proj_path);
                            if let Some(ref proj_canonical) = project_path_canonical {
                                if let Ok(config_canonical) = config_proj_path.canonicalize() {
                                    proj_canonical == &config_canonical
                                } else {
                                    project_path == &config_proj_path
                                }
                            } else {
                                project_path == &config_proj_path
                            }
                        } else {
                            false
                        };

                        if matches {
                            if let Some(serde_json::Value::String(wt_path)) =
                                map.get("worktreesPath")
                            {
                                return Some(PathBuf::from(wt_path));
                            }
                        }
                    }
                }
            }
        }

        None
    }

    fn parse_worktrees_path_from_file(config_path: &Path) -> Option<PathBuf> {
        if !config_path.exists() {
            return None;
        }
        let content = std::fs::read_to_string(config_path).ok()?;
        let parsed = jsonc_parser::parse_to_serde_value(&content, &Default::default()).ok()??;
        if let serde_json::Value::Object(map) = parsed {
            if let Some(serde_json::Value::String(wt_path)) = map.get("worktreesPath") {
                return Some(PathBuf::from(wt_path));
            }
        }
        None
    }

    pub fn project_path(&self) -> &Path {
        &self.project_path
    }

    pub fn worktrees_path(&self) -> &Path {
        &self.worktrees_path
    }

    pub fn add_worktree(&self, branch_name: &str) -> Result<PathBuf> {
        let worktree_path = self.worktrees_path.join(branch_name);

        if !self.has_gwt_cli() {
            return Err(MultiAiError::Worktree(
                "gwt CLI is not installed or not in PATH".to_string(),
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
        let status = child
            .wait()
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
                if stderr_msg.is_empty() {
                    "Unknown error"
                } else {
                    &stderr_msg
                }
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
                "gwt CLI is not installed or not in PATH".to_string(),
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
            .map_err(|e| {
                MultiAiError::CommandFailed(format!("Failed to execute gwt remove: {}", e))
            })?;

        // Stream stdout
        if let Some(stdout) = child.stdout.take() {
            let reader = BufReader::new(stdout);
            for line in reader.lines().map_while(|r| r.ok()) {
                println!("    {}", line);
            }
        }

        // Wait for the process to complete and check status
        let status = child.wait().map_err(|e| {
            MultiAiError::CommandFailed(format!("Failed to wait for gwt remove: {}", e))
        })?;

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
                if stderr_msg.is_empty() {
                    "Unknown error"
                } else {
                    &stderr_msg
                }
            )));
        }

        Ok(())
    }

    pub fn is_gwt_project(&self) -> bool {
        // Check if git-worktree-config.jsonc exists in current directory
        let gwt_config_jsonc = self.project_path.join("git-worktree-config.jsonc");
        if gwt_config_jsonc.exists() {
            return true;
        }

        // Also check in ./main/ subdirectory
        let gwt_config_jsonc_main = self
            .project_path
            .join("main")
            .join("git-worktree-config.jsonc");
        if gwt_config_jsonc_main.exists() {
            return true;
        }

        // Also check for .yaml for backward compatibility (current directory)
        let gwt_config_yaml = self.project_path.join("git-worktree-config.yaml");
        if gwt_config_yaml.exists() {
            return true;
        }

        // Also check for .yaml in ./main/ subdirectory
        let gwt_config_yaml_main = self
            .project_path
            .join("main")
            .join("git-worktree-config.yaml");
        if gwt_config_yaml_main.exists() {
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

    pub fn worktrees_exist(&self, branch_prefix: &str, ai_app_names: &[String]) -> bool {
        // Check if all worktree directories exist for the given branch prefix and AI apps
        ai_app_names.iter().all(|app_name| {
            let branch_name = format!("{}-{}", branch_prefix, app_name);
            let worktree_path = self.worktrees_path.join(&branch_name);
            worktree_path.exists() && worktree_path.is_dir()
        })
    }
}
