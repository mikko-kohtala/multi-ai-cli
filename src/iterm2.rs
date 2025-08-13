use crate::config::AiApp;
use crate::error::{MultiAiError, Result};
use std::process::Command;

pub struct ITerm2Manager {
    #[allow(dead_code)]
    project: String,
    branch_prefix: String,
}

impl ITerm2Manager {
    pub fn new(project: &str, branch_prefix: &str) -> Self {
        Self {
            project: project.to_string(),
            branch_prefix: branch_prefix.to_string(),
        }
    }

    /// Create one tab per AI app with horizontal split (top/bottom)
    pub fn create_tabs_per_app(&self, _ai_apps: &[AiApp], worktree_paths: &[(AiApp, String)]) -> Result<()> {
        for (ai_app, worktree_path) in worktree_paths {
            let applescript = format!(
                r#"
tell application "iTerm"
    tell current window
        -- Create a new tab
        create tab with default profile
        
        tell current session
            -- Navigate to worktree directory
            write text "cd {}"
            delay 0.5
            
            -- Create horizontal split (top/bottom)
            set bottomPane to (split horizontally with default profile)
            
            -- Launch AI app in top pane
            write text "{}"
            
            -- Bottom pane just navigates to directory
            tell bottomPane
                write text "cd {}"
            end tell
            
            -- Set tab title
            set name to "{}-{}"
        end tell
    end tell
end tell
"#,
                worktree_path,
                ai_app.command(),
                worktree_path,
                self.branch_prefix,
                ai_app.as_str()
            );

            let output = Command::new("osascript")
                .arg("-e")
                .arg(&applescript)
                .output()
                .map_err(|e| MultiAiError::ITerm2(format!("Failed to execute AppleScript: {}", e)))?;

            if !output.status.success() {
                let error = String::from_utf8_lossy(&output.stderr);
                return Err(MultiAiError::ITerm2(format!("AppleScript failed for {}: {}", ai_app.as_str(), error)));
            }
        }

        Ok(())
    }
}