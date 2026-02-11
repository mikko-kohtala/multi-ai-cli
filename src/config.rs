use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::git;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProjectConfig {
    #[serde(default)]
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
    pub slug: Option<String>,
    #[serde(default)]
    pub ultrathink: Option<String>,
    #[serde(default)]
    pub default: bool,
    #[serde(default)]
    pub meta_review: bool,
    #[serde(default)]
    pub description: Option<String>,
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

    /// Return a git-safe slug for use in branch names and worktree directories.
    /// Uses the explicit `slug` field if set, otherwise auto-generates from the command.
    pub fn slug(&self) -> String {
        if let Some(ref s) = self.slug {
            return s.clone();
        }
        slugify_command(&self.command)
    }
}

/// Turn a command string into a git-safe branch-name component.
/// e.g. "claude --permission-mode plan --allow-dangerously-skip-permissions" → "claude-plan"
/// e.g. "codex --yolo --model gpt-5.3-codex" → "codex-yolo-gpt-5.3-codex"
pub(crate) fn slugify_command(command: &str) -> String {
    let parts: Vec<&str> = command.split_whitespace().collect();
    if parts.is_empty() {
        return "unknown".to_string();
    }

    let binary = parts[0];
    let mut tokens = vec![binary.to_string()];

    let mut i = 1;
    while i < parts.len() {
        let part = parts[i];
        if part.starts_with("--") || part.starts_with('-') {
            let flag = part.trim_start_matches('-');
            // Skip verbose flags, keep meaningful short ones
            match flag {
                "dangerously-skip-permissions" | "allow-dangerously-skip-permissions"
                | "dangerously-allow-all" | "allow-all-tools" => {
                    tokens.push("yolo".to_string());
                }
                "yolo" | "force" => {
                    tokens.push(flag.to_string());
                }
                "permission-mode" => {
                    // Take the next arg as the value
                    if i + 1 < parts.len() && !parts[i + 1].starts_with('-') {
                        tokens.push(parts[i + 1].to_string());
                        i += 1;
                    }
                }
                "model" => {
                    if i + 1 < parts.len() && !parts[i + 1].starts_with('-') {
                        tokens.push(parts[i + 1].to_string());
                        i += 1;
                    }
                }
                "config" => {
                    // Skip config key=value pairs
                    if i + 1 < parts.len() {
                        i += 1;
                    }
                }
                _ => {
                    // Include short unknown flags
                    if flag.len() <= 12 {
                        tokens.push(flag.to_string());
                    }
                }
            }
        }
        i += 1;
    }

    // Join and sanitize for git branch names
    let raw = tokens.join("-");
    let sanitized: String = raw
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '.' || c == '_' { c } else { '-' })
        .collect();

    // Collapse multiple dashes and trim
    let mut result = String::new();
    let mut last_dash = false;
    for c in sanitized.chars() {
        if c == '-' {
            if !last_dash && !result.is_empty() {
                result.push('-');
                last_dash = true;
            }
        } else {
            result.push(c);
            last_dash = false;
        }
    }
    result.trim_end_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::slugify_command;

    #[test]
    fn test_slugify_basic_commands() {
        assert_eq!(slugify_command("claude"), "claude");
        assert_eq!(slugify_command("gemini"), "gemini");
        assert_eq!(slugify_command("codex"), "codex");
    }

    #[test]
    fn test_slugify_yolo_variants() {
        assert_eq!(slugify_command("gemini --yolo"), "gemini-yolo");
        assert_eq!(slugify_command("codex --yolo"), "codex-yolo");
        assert_eq!(
            slugify_command("claude --dangerously-skip-permissions"),
            "claude-yolo"
        );
        assert_eq!(
            slugify_command("amp --dangerously-allow-all"),
            "amp-yolo"
        );
        assert_eq!(
            slugify_command("copilot --allow-all-tools"),
            "copilot-yolo"
        );
        assert_eq!(
            slugify_command("cursor-agent --force"),
            "cursor-agent-force"
        );
    }

    #[test]
    fn test_slugify_permission_mode() {
        assert_eq!(
            slugify_command("claude --permission-mode plan --allow-dangerously-skip-permissions"),
            "claude-plan-yolo"
        );
    }

    #[test]
    fn test_slugify_model_variants() {
        assert_eq!(
            slugify_command("codex --yolo --model gpt-5.3-codex --config model_reasoning_effort='high'"),
            "codex-yolo-gpt-5.3-codex"
        );
        assert_eq!(
            slugify_command("codex --yolo --model gpt-5.1 --config model_reasoning_effort='high'"),
            "codex-yolo-gpt-5.1"
        );
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

    /// Returns the config directory (~/.config/multi-ai-cli)
    pub fn config_dir() -> anyhow::Result<PathBuf> {
        let home = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;
        Ok(home.join(".config").join("multi-ai-cli"))
    }

    /// Find a config file in ~/.config/multi-ai-cli/.
    ///
    /// Search order:
    /// 1. Git remote URL -> generate filename -> look up ~/.config/multi-ai-cli/{filename}.jsonc
    /// 2. Fallback: scan all .jsonc files for matching project_path or worktrees_path
    /// 3. Legacy: check deprecated ~/.config/multi-ai-cli/projects/ subdirectory
    ///
    /// Returns: (config_file_path, parsed_config, effective_project_path)
    pub fn find_config(start_dir: &Path) -> anyhow::Result<Option<ConfigFindResult>> {
        let config_dir = match Self::config_dir() {
            Ok(dir) => dir,
            Err(_) => return Ok(None),
        };

        if config_dir.exists() {
            // Strategy 1: Find by git remote URL
            if let Some(result) = Self::find_config_by_url(start_dir, &config_dir)? {
                return Ok(Some(result));
            }

            // Strategy 2: Scan all configs for matching project_path or worktrees_path
            if let Some(result) = Self::find_config_by_path(start_dir, &config_dir)? {
                return Ok(Some(result));
            }
        }

        // Legacy fallback: check deprecated projects/ subdirectory
        let legacy_dir = config_dir.join("projects");
        if legacy_dir.exists() {
            if let Some(result) = Self::find_config_by_url(start_dir, &legacy_dir)? {
                eprintln!(
                    "Warning: Config found in deprecated location ~/.config/multi-ai-cli/projects/.\n\
                     Please move it to ~/.config/multi-ai-cli/."
                );
                return Ok(Some(result));
            }
            if let Some(result) = Self::find_config_by_path(start_dir, &legacy_dir)? {
                eprintln!(
                    "Warning: Config found in deprecated location ~/.config/multi-ai-cli/projects/.\n\
                     Please move it to ~/.config/multi-ai-cli/."
                );
                return Ok(Some(result));
            }
        }

        Ok(None)
    }

    /// Find config by generating filename from git remote URL.
    fn find_config_by_url(
        start_dir: &Path,
        search_dir: &Path,
    ) -> anyhow::Result<Option<ConfigFindResult>> {
        let repo_url = match git::get_remote_origin_url(start_dir) {
            Some(url) => url,
            None => return Ok(None),
        };

        let config_filename = format!("{}.jsonc", git::generate_config_filename(&repo_url));
        let config_path = search_dir.join(&config_filename);

        if !config_path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&config_path)?;
        let config = Self::from_json(&content)?;

        if config.project_path.is_none() {
            anyhow::bail!(
                "Config file {} is missing required 'project_path' field.\n\
                 Add \"project_path\": \"/path/to/your/project\" to the config file.",
                config_path.display()
            );
        }

        let project_path = config.project_path.clone().unwrap();
        Ok(Some((config_path, config, project_path)))
    }

    /// Find config by scanning all .jsonc files for matching project_path or worktrees_path.
    fn find_config_by_path(
        start_dir: &Path,
        search_dir: &Path,
    ) -> anyhow::Result<Option<ConfigFindResult>> {
        let start_dir_canonical = start_dir.canonicalize().ok();

        for entry in fs::read_dir(search_dir)? {
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

                let matches = |config_path_opt: &Option<PathBuf>| -> bool {
                    if let Some(p) = config_path_opt {
                        if let Some(ref start_canonical) = start_dir_canonical
                            && let Ok(p_canonical) = p.canonicalize()
                            && (start_canonical == &p_canonical
                                || start_canonical.starts_with(&p_canonical))
                        {
                            return true;
                        }
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
