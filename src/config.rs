use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProjectConfig {
    pub ai_apps: Vec<AiApp>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AiApp {
    Claude,
    Codex,
    Amp,
    Gemini,
}

impl AiApp {
    pub fn as_str(&self) -> &str {
        match self {
            AiApp::Claude => "claude",
            AiApp::Codex => "codex",
            AiApp::Amp => "amp",
            AiApp::Gemini => "gemini",
        }
    }

    pub fn command(&self) -> &str {
        match self {
            AiApp::Claude => "claude",
            AiApp::Codex => "codex",
            AiApp::Amp => "amp",
            AiApp::Gemini => "gemini",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UserConfig {
    pub code_root: PathBuf,
}

impl ProjectConfig {
    pub fn from_json(content: &str) -> anyhow::Result<Self> {
        // Parse JSONC (JSON with Comments) which also handles regular JSON
        let parsed = jsonc_parser::parse_to_serde_value(content, &Default::default())?
            .ok_or_else(|| anyhow::anyhow!("Failed to parse JSON/JSONC content"))?;
        Ok(serde_json::from_value(parsed)?)
    }
}

impl UserConfig {
    pub fn from_json(content: &str) -> anyhow::Result<Self> {
        // Parse JSONC (JSON with Comments) which also handles regular JSON
        let parsed = jsonc_parser::parse_to_serde_value(content, &Default::default())?
            .ok_or_else(|| anyhow::anyhow!("Failed to parse user config JSON/JSONC content"))?;
        Ok(serde_json::from_value(parsed)?)
    }

    pub fn config_path() -> PathBuf {
        dirs::home_dir()
            .expect("Could not find home directory")
            .join(".config")
            .join("multi-ai")
            .join("settings.jsonc")
    }

    pub fn expand_path(&self) -> PathBuf {
        PathBuf::from(shellexpand::tilde(&self.code_root.to_string_lossy()).to_string())
    }
}