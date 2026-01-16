use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::git;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProjectConfig {
    pub ai_apps: Vec<AiApp>,
    #[serde(default = "default_terminals_per_column")]
    pub terminals_per_column: usize,
    #[serde(default)]
    pub mode: Option<Mode>,
    /// Optional project path for global configs - where the main git repo lives
    #[serde(default)]
    pub project_path: Option<PathBuf>,
    /// Optional worktrees path for global configs - where worktrees should be created
    #[serde(default)]
    pub worktrees_path: Option<PathBuf>,
}

fn default_terminals_per_column() -> usize {
    2
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TmuxLayout {
    MultiWindow,
    SingleWindow,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    Iterm2,
    TmuxSingleWindow,
    TmuxMultiWindow,
}

impl Mode {
    /// Returns the default mode for the current platform
    pub fn default_for_platform() -> Self {
        #[cfg(target_os = "macos")]
        return Mode::Iterm2;

        #[cfg(not(target_os = "macos"))]
        return Mode::TmuxSingleWindow;
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AiApp {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub ultrathink: Option<String>,
}

impl AiApp {
    pub fn as_str(&self) -> &str {
        &self.name
    }

    pub fn command(&self) -> &str {
        &self.command
    }
    
    pub fn ultrathink(&self) -> Option<&str> {
        self.ultrathink.as_deref()
    }
}

/// Result of finding a config file
/// Contains: (config_file_path, parsed_config, effective_project_path)
pub type ConfigFindResult = (PathBuf, ProjectConfig, PathBuf);

impl ProjectConfig {
    pub fn from_json(content: &str) -> anyhow::Result<Self> {
        // Parse JSONC (JSON with Comments) which also handles regular JSON
        let mut parsed = jsonc_parser::parse_to_serde_value(content, &Default::default())?
            .ok_or_else(|| anyhow::anyhow!("Failed to parse JSON/JSONC content"))?;

        // For backward compatibility with configs that don't have the optional `mode` field.
        // Inject `null` so serde will populate `None` rather than erroring.
        if let serde_json::Value::Object(ref mut map) = parsed {
            map.entry("mode").or_insert(serde_json::Value::Null);
        }

        Ok(serde_json::from_value(parsed)?)
    }

    /// Returns the global config directory (~/.config/multi-ai-cli)
    pub fn global_config_dir() -> anyhow::Result<PathBuf> {
        let config_dir = dirs::config_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?;
        Ok(config_dir.join("multi-ai-cli"))
    }

    /// Returns the projects config directory (~/.config/multi-ai-cli/projects)
    pub fn projects_config_dir() -> anyhow::Result<PathBuf> {
        Ok(Self::global_config_dir()?.join("projects"))
    }

    /// Find a config file using the search order:
    /// 1. ./multi-ai-config.jsonc (current directory)
    /// 2. ./main/multi-ai-config.jsonc (main subdirectory)
    /// 3. Walk up directory tree, checking steps 1-2 at each level
    /// 4. Global config by repo URL: ~/.config/multi-ai-cli/projects/{repo-name}.jsonc
    /// 5. Global config by path match: search all global configs for matching project_path or worktrees_path
    ///
    /// Returns: (config_file_path, parsed_config, effective_project_path)
    pub fn find_config(start_dir: &Path) -> anyhow::Result<Option<ConfigFindResult>> {
        // First try local config (walking up directory tree)
        if let Some((config_path, config)) = Self::find_local_config(start_dir)? {
            // Determine effective project path:
            // - If config has project_path, use it
            // - Otherwise, use the directory where config was found (or parent if in ./main/)
            let effective_project_path = if let Some(ref proj_path) = config.project_path {
                proj_path.clone()
            } else {
                let config_dir = config_path.parent().unwrap();
                // If config is in a ./main/ subdirectory, use the parent
                if config_dir.file_name().map(|n| n == "main").unwrap_or(false) {
                    config_dir.parent().unwrap().to_path_buf()
                } else {
                    config_dir.to_path_buf()
                }
            };
            return Ok(Some((config_path, config, effective_project_path)));
        }

        // Then try global config
        if let Some((config_path, config, project_path)) = Self::find_global_config(start_dir)? {
            return Ok(Some((config_path, config, project_path)));
        }

        Ok(None)
    }

    /// Find a local config by checking current directory and walking up the tree
    fn find_local_config(start_dir: &Path) -> anyhow::Result<Option<(PathBuf, Self)>> {
        let mut current = start_dir.to_path_buf();

        loop {
            // Check current directory
            let config_path = current.join("multi-ai-config.jsonc");
            if config_path.exists() {
                let content = fs::read_to_string(&config_path)?;
                let config = Self::from_json(&content)?;
                return Ok(Some((config_path, config)));
            }

            // Check ./main/ subdirectory
            let main_config_path = current.join("main").join("multi-ai-config.jsonc");
            if main_config_path.exists() {
                let content = fs::read_to_string(&main_config_path)?;
                let config = Self::from_json(&content)?;
                return Ok(Some((main_config_path, config)));
            }

            // Move up one directory
            match current.parent() {
                Some(parent) => current = parent.to_path_buf(),
                None => break,
            }
        }

        Ok(None)
    }

    /// Find a global config by repo URL or path matching
    fn find_global_config(start_dir: &Path) -> anyhow::Result<Option<ConfigFindResult>> {
        let projects_dir = match Self::projects_config_dir() {
            Ok(dir) => dir,
            Err(_) => return Ok(None),
        };

        if !projects_dir.exists() {
            return Ok(None);
        }

        // First, try to find by repo URL
        if let Some(repo_url) = git::get_remote_origin_url(start_dir) {
            let config_filename = format!("{}.jsonc", git::generate_config_filename(&repo_url));
            let config_path = projects_dir.join(&config_filename);

            if config_path.exists() {
                let content = fs::read_to_string(&config_path)?;
                let config = Self::from_json(&content)?;

                // Use project_path from config, or worktrees_path, or start_dir
                let project_path = config
                    .project_path
                    .clone()
                    .or_else(|| config.worktrees_path.clone())
                    .unwrap_or_else(|| start_dir.to_path_buf());

                return Ok(Some((config_path, config, project_path)));
            }
        }

        // Then, search all global configs for matching project_path or worktrees_path
        let start_dir_canonical = start_dir.canonicalize().ok();

        for entry in fs::read_dir(&projects_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().map(|e| e == "jsonc").unwrap_or(false) {
                let content = match fs::read_to_string(&path) {
                    Ok(c) => c,
                    Err(_) => continue,
                };

                let config = match Self::from_json(&content) {
                    Ok(c) => c,
                    Err(_) => continue,
                };

                // Check if start_dir matches or is under project_path or worktrees_path
                let matches = |config_path: &Option<PathBuf>| -> bool {
                    if let Some(p) = config_path {
                        // Try canonical comparison
                        if let Some(ref start_canonical) = start_dir_canonical {
                            if let Ok(p_canonical) = p.canonicalize() {
                                if start_canonical == &p_canonical
                                    || start_canonical.starts_with(&p_canonical)
                                {
                                    return true;
                                }
                            }
                        }
                        // Try string comparison as fallback
                        if start_dir == p || start_dir.starts_with(p) {
                            return true;
                        }
                    }
                    false
                };

                if matches(&config.project_path) || matches(&config.worktrees_path) {
                    let project_path = config
                        .project_path
                        .clone()
                        .or_else(|| config.worktrees_path.clone())
                        .unwrap_or_else(|| start_dir.to_path_buf());

                    return Ok(Some((path, config, project_path)));
                }
            }
        }

        Ok(None)
    }
}
