use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProjectConfig {
    pub ai_apps: Vec<AiApp>,
    #[serde(default = "default_terminals_per_column")]
    pub terminals_per_column: usize,
    #[serde(default)]
    pub mode: Option<Mode>,
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
        let mut parsed = jsonc_parser::parse_to_serde_value(content, &Default::default())?
            .ok_or_else(|| anyhow::anyhow!("Failed to parse JSON/JSONC content"))?;

        // Prior to introducing the required `mode` field, configs didn't include it.
        // Inject `null` so serde will populate `None` rather than erroring.
        if let serde_json::Value::Object(ref mut map) = parsed {
            map.entry("mode").or_insert(serde_json::Value::Null);
        }

        Ok(serde_json::from_value(parsed)?)
    }
}
