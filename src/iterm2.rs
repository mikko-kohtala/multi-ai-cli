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

    /// Create a single tab with all AI apps in columns
    /// Each app gets a vertical column with 2 panes (top for AI command, bottom for shell)
    pub fn create_tabs_per_app(&self, _ai_apps: &[AiApp], worktree_paths: &[(AiApp, String)]) -> Result<()> {
        if worktree_paths.is_empty() {
            return Ok(());
        }

        // Build AppleScript for creating column-based layout
        let mut applescript = String::from(r#"
tell application "iTerm"
    tell current window
        -- Create a new tab
        create tab with default profile
        
        tell current session"#);

        let num_apps = worktree_paths.len();
        
        // Handle single app case
        if num_apps == 1 {
            let (app, path) = &worktree_paths[0];
            applescript.push_str(&format!(
                r#"
            -- Single app: {} (1x2 layout)
            -- Wait for shell to initialize
            delay 2
            write text "cd {} && {}"
            
            -- Split horizontally for shell
            set shellPane to (split horizontally with default profile)
            tell shellPane
                delay 1
                write text "cd {}"
            end tell"#,
                app.as_str(), path, app.command(), path
            ));
        } else {
            // Multiple apps: create dynamic column layout
            applescript.push_str(&format!(
                r#"
            -- {} apps: {}x2 layout
            -- Create {} columns"#,
                num_apps, num_apps, num_apps
            ));
            
            // Create vertical splits for columns (skip first column as it's the current session)
            for i in 2..=num_apps {
                if i == 2 {
                    applescript.push_str("\n            set col2 to (split vertically with default profile)");
                } else {
                    applescript.push_str(&format!(
                        "\n            tell col{}\n                set col{} to (split vertically with default profile)\n            end tell",
                        i - 1, i
                    ));
                }
            }
            
            // Split each column horizontally
            applescript.push_str("\n            \n            -- Split each column horizontally");
            applescript.push_str("\n            set col1Bottom to (split horizontally with default profile)");
            for i in 2..=num_apps {
                applescript.push_str(&format!(
                    "\n            tell col{}\n                set col{}Bottom to (split horizontally with default profile)\n            end tell",
                    i, i
                ));
            }
            
            // Populate panes
            applescript.push_str("\n            \n            -- Populate panes");
            for (i, (app, path)) in worktree_paths.iter().enumerate() {
                let col_num = i + 1;
                
                if i == 0 {
                    // First column uses current session
                    applescript.push_str(&format!(
                        r#"
            -- App {}: {} (column {}, top)
            delay 2
            write text "cd {} && {}"
            
            tell col1Bottom
                delay 1
                write text "cd {}"
            end tell"#,
                        i + 1, app.as_str(), col_num, path, app.command(), path
                    ));
                } else {
                    // Other columns use colN references
                    applescript.push_str(&format!(
                        r#"
            
            -- App {}: {} (column {}, top)
            tell col{}
                delay 1
                write text "cd {} && {}"
            end tell
            
            tell col{}Bottom
                delay 1
                write text "cd {}"
            end tell"#,
                        i + 1, app.as_str(), col_num, col_num, path, app.command(), col_num, path
                    ));
                }
            }
        }

        // Set the tab name
        applescript.push_str(&format!(
            r#"
            
            -- Set tab title
            set name to "{}""#,
            self.branch_prefix
        ));

        applescript.push_str(r#"
        end tell
    end tell
end tell"#);

        // Debug: Log the AppleScript being executed
        eprintln!("DEBUG: Executing AppleScript for {} apps", worktree_paths.len());
        
        // Execute the AppleScript
        let output = Command::new("osascript")
            .arg("-e")
            .arg(&applescript)
            .output()
            .map_err(|e| MultiAiError::ITerm2(format!("Failed to execute AppleScript: {}", e)))?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            eprintln!("DEBUG: AppleScript stderr: {}", error);
            eprintln!("DEBUG: AppleScript stdout: {}", String::from_utf8_lossy(&output.stdout));
            return Err(MultiAiError::ITerm2(format!("AppleScript failed: {}", error)));
        }

        Ok(())
    }
}