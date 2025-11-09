use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TerminalMode {
    Iterm2,
    TmuxMultiWindow,
    TmuxSingleWindow,
}

impl TerminalMode {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "iterm2" => Some(TerminalMode::Iterm2),
            "tmux-multi-window" => Some(TerminalMode::TmuxMultiWindow),
            "tmux-single-window" => Some(TerminalMode::TmuxSingleWindow),
            _ => None,
        }
    }

    pub fn system_default() -> Self {
        #[cfg(target_os = "macos")]
        {
            TerminalMode::Iterm2
        }
        #[cfg(not(target_os = "macos"))]
        {
            TerminalMode::TmuxSingleWindow
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProjectConfig {
    pub ai_apps: Vec<AiApp>,
    #[serde(default = "default_terminals_per_column")]
    pub terminals_per_column: usize,
    #[serde(default)]
    pub terminal_mode: Option<TerminalMode>,
}

fn default_terminals_per_column() -> usize {
    2
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AiApp {
    pub name: String,
    pub command: String,
}

impl AiApp {
    pub fn as_str(&self) -> &str {
        &self.name
    }

    pub fn command(&self) -> &str {
        &self.command
    }
}

impl ProjectConfig {
    pub fn from_json(content: &str) -> anyhow::Result<Self> {
        // Parse JSONC (JSON with Comments) which also handles regular JSON
        let parsed = jsonc_parser::parse_to_serde_value(content, &Default::default())?
            .ok_or_else(|| anyhow::anyhow!("Failed to parse JSON/JSONC content"))?;
        Ok(serde_json::from_value(parsed)?)
    }
}