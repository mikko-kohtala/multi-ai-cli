use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProjectConfig {
    pub ai_apps: Vec<AiApp>,
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