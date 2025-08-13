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
            write text "cd {} && {}"
            
            -- Split horizontally for shell
            set shellPane to (split horizontally with default profile)
            tell shellPane
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
            -- First app: {} (left column, top)
            write text "cd {} && {}"
            
            -- Split horizontally for first app's shell (left column, bottom)
            set leftShell to (split horizontally with default profile)
            tell leftShell
                write text "cd {}"
            end tell
            
            -- Split vertically from top-left to create right column
            set rightColumn to (split vertically with default profile)
            tell rightColumn
                -- Second app: {} (right column, top)
                write text "cd {} && {}"
                
                -- Split horizontally for second app's shell (right column, bottom)
                set rightShell to (split horizontally with default profile)
                tell rightShell
                    write text "cd {}"
                end tell
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
            -- First app: {} (left column, top)
            write text "cd {} && {}"
            
            -- Split horizontally for first app's shell
            set leftShell to (split horizontally with default profile)
            tell leftShell
                write text "cd {}"
            end tell
            
            -- Split vertically from top-left to create middle column
            set middleColumn to (split vertically with default profile)
            tell middleColumn
                -- Second app: {} (middle column, top)
                write text "cd {} && {}"
                
                -- Split horizontally for second app's shell
                set middleShell to (split horizontally with default profile)
                tell middleShell
                    write text "cd {}"
                end tell
                
                -- Split vertically from middle-top to create right column
                set rightColumn to (split vertically with default profile)
                tell rightColumn
                    -- Third app: {} (right column, top)
                    write text "cd {} && {}"
                    
                    -- Split horizontally for third app's shell
                    set rightShell to (split horizontally with default profile)
                    tell rightShell
                        write text "cd {}"
                    end tell
                end tell
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
            -- First app: {} (first column, top)
            write text "cd {} && {}"
            
            -- Split horizontally for first app's shell
            set col1Shell to (split horizontally with default profile)
            tell col1Shell
                write text "cd {}"
            end tell
            
            -- Split vertically from top of column 1 to create column 2
            set col2Top to (split vertically with default profile)
            tell col2Top
                -- Second app: {} (second column, top)
                write text "cd {} && {}"
                
                -- Split horizontally for second app's shell
                set col2Shell to (split horizontally with default profile)
                tell col2Shell
                    write text "cd {}"
                end tell
                
                -- Split vertically from top of column 2 to create column 3
                set col3Top to (split vertically with default profile)
                tell col3Top
                    -- Third app: {} (third column, top)
                    write text "cd {} && {}"
                    
                    -- Split horizontally for third app's shell
                    set col3Shell to (split horizontally with default profile)
                    tell col3Shell
                        write text "cd {}"
                    end tell
                    
                    -- Split vertically from top of column 3 to create column 4
                    set col4Top to (split vertically with default profile)
                    tell col4Top
                        -- Fourth app: {} (fourth column, top)
                        write text "cd {} && {}"
                        
                        -- Split horizontally for fourth app's shell
                        set col4Shell to (split horizontally with default profile)
                        tell col4Shell
                            write text "cd {}"
                        end tell
                    end tell
                end tell
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