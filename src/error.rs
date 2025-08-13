use thiserror::Error;

#[derive(Error, Debug)]
pub enum MultiAiError {
    #[error("Configuration error: {0}")]
    Config(String),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("JSON parsing error: {0}")]
    Json(#[from] serde_json::Error),
    
    #[error("JSONC parsing error: {0}")]
    JsonC(#[from] jsonc_parser::errors::ParseError),
    
    #[error("Git worktree error: {0}")]
    Worktree(String),
    
    #[error("Tmux error: {0}")]
    Tmux(String),
    
    #[error("Project not found: {0}")]
    ProjectNotFound(String),
    
    #[error("Command execution failed: {0}")]
    CommandFailed(String),
}

pub type Result<T> = std::result::Result<T, MultiAiError>;