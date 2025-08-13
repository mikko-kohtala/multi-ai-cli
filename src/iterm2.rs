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

        match worktree_paths.len() {
            1 => {
                // Single app: 1x2 (one column with 2 rows)
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
            }
            2 => {
                // Two apps: 2x2 (two columns, each with 2 rows)
                let (app1, path1) = &worktree_paths[0];
                let (app2, path2) = &worktree_paths[1];
                
                applescript.push_str(&format!(
                    r#"
            -- Two apps: 2x2 layout
            -- Create the basic 2x2 grid first
            
            -- Split vertically to create two columns
            set rightColumn to (split vertically with default profile)
            
            -- Split each column horizontally
            set leftBottom to (split horizontally with default profile)
            tell rightColumn
                set rightBottom to (split horizontally with default profile)
            end tell
            
            -- Now populate each pane
            -- First app: {} (left column, top)
            delay 2
            write text "cd {} && {}"
            
            -- First app shell (left column, bottom)
            tell leftBottom
                delay 1
                write text "cd {}"
            end tell
            
            -- Second app: {} (right column, top)
            tell rightColumn
                delay 1
                write text "cd {} && {}"
            end tell
            
            -- Second app shell (right column, bottom)
            tell rightBottom
                delay 1
                write text "cd {}"
            end tell"#,
                    app1.as_str(), path1, app1.command(), path1,
                    app2.as_str(), path2, app2.command(), path2
                ));
            }
            3 => {
                // Three apps: 3x2 (three columns, each with 2 rows)
                let (app1, path1) = &worktree_paths[0];
                let (app2, path2) = &worktree_paths[1];
                let (app3, path3) = &worktree_paths[2];
                
                applescript.push_str(&format!(
                    r#"
            -- Three apps: 3x2 layout
            -- Create 3 columns first
            set col2 to (split vertically with default profile)
            tell col2
                set col3 to (split vertically with default profile)
            end tell
            
            -- Split each column horizontally
            set col1Bottom to (split horizontally with default profile)
            tell col2
                set col2Bottom to (split horizontally with default profile)
            end tell
            tell col3
                set col3Bottom to (split horizontally with default profile)
            end tell
            
            -- Populate panes
            -- First app: {} (column 1, top)
            delay 2
            write text "cd {} && {}"
            
            tell col1Bottom
                delay 1
                write text "cd {}"
            end tell
            
            -- Second app: {} (column 2, top)
            tell col2
                delay 1
                write text "cd {} && {}"
            end tell
            
            tell col2Bottom
                delay 1
                write text "cd {}"
            end tell
            
            -- Third app: {} (column 3, top)
            tell col3
                delay 1
                write text "cd {} && {}"
            end tell
            
            tell col3Bottom
                delay 1
                write text "cd {}"
            end tell"#,
                    app1.as_str(), path1, app1.command(), path1,
                    app2.as_str(), path2, app2.command(), path2,
                    app3.as_str(), path3, app3.command(), path3
                ));
            }
            _ => {
                // Four or more apps: 4x2 (four columns, each with 2 rows)
                let (app1, path1) = &worktree_paths[0];
                let (app2, path2) = &worktree_paths[1];
                let (app3, path3) = &worktree_paths[2];
                let (app4, path4) = &worktree_paths[3];
                
                applescript.push_str(&format!(
                    r#"
            -- Four apps: 4x2 layout
            -- First create 4 columns
            set col2 to (split vertically with default profile)
            tell col2
                set col3 to (split vertically with default profile)
                tell col3
                    set col4 to (split vertically with default profile)
                end tell
            end tell
            
            -- Now split each column horizontally to create 8 panes total
            set col1Bottom to (split horizontally with default profile)
            tell col2
                set col2Bottom to (split horizontally with default profile)
            end tell
            tell col3
                set col3Bottom to (split horizontally with default profile)
            end tell
            tell col4
                set col4Bottom to (split horizontally with default profile)
            end tell
            
            -- Wait for shells to initialize and populate each pane
            -- First app: {} (column 1)
            delay 2
            write text "cd {} && {}"
            
            tell col1Bottom
                delay 1
                write text "cd {}"
            end tell
            
            -- Second app: {} (column 2)
            tell col2
                delay 1
                write text "cd {} && {}"
            end tell
            
            tell col2Bottom
                delay 1
                write text "cd {}"
            end tell
            
            -- Third app: {} (column 3)
            tell col3
                delay 1
                write text "cd {} && {}"
            end tell
            
            tell col3Bottom
                delay 1
                write text "cd {}"
            end tell
            
            -- Fourth app: {} (column 4)
            tell col4
                delay 1
                write text "cd {} && {}"
            end tell
            
            tell col4Bottom
                delay 1
                write text "cd {}"
            end tell"#,
                    app1.as_str(), path1, app1.command(), path1,
                    app2.as_str(), path2, app2.command(), path2,
                    app3.as_str(), path3, app3.command(), path3,
                    app4.as_str(), path4, app4.command(), path4
                ));
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

        // Execute the AppleScript
        let output = Command::new("osascript")
            .arg("-e")
            .arg(&applescript)
            .output()
            .map_err(|e| MultiAiError::ITerm2(format!("Failed to execute AppleScript: {}", e)))?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(MultiAiError::ITerm2(format!("AppleScript failed: {}", error)));
        }

        Ok(())
    }
}