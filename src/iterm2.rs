use crate::config::AiApp;
use crate::error::{MultiAiError, Result};
use std::process::Command;

pub struct ITerm2Manager {
    #[allow(dead_code)]
    project: String,
    branch_prefix: String,
    terminals_per_column: usize,
}

impl ITerm2Manager {
    pub fn new(project: &str, branch_prefix: &str, terminals_per_column: usize) -> Self {
        Self {
            project: project.to_string(),
            branch_prefix: branch_prefix.to_string(),
            terminals_per_column,
        }
    }

    /// Create a single tab with all AI apps in columns
    /// Each app gets a vertical column with configurable number of panes (first for AI command, rest for shells)
    pub fn create_tabs_per_app(
        &self,
        _ai_apps: &[AiApp],
        worktree_paths: &[(AiApp, String)],
    ) -> Result<()> {
        if worktree_paths.is_empty() {
            return Ok(());
        }

        // Build AppleScript for creating column-based layout
        let mut applescript = String::from(
            r#"
tell application "iTerm"
    tell current window
        -- Create a new tab
        create tab with default profile
        
        tell current session"#,
        );

        let num_apps = worktree_paths.len();

        // Handle single app case
        if num_apps == 1 {
            let (app, path) = &worktree_paths[0];
            applescript.push_str(&format!(
                r#"
            -- Single app: {} (1x{} layout)
            -- Wait for shell to initialize
            delay 2
            write text "cd {} && {}""#,
                app.as_str(),
                self.terminals_per_column,
                path,
                app.command()
            ));

            // Create additional panes for shells
            if self.terminals_per_column > 1 {
                // Create the first split
                applescript.push_str(
                    "\n            \n            -- Split horizontally for additional shells",
                );

                // We need to keep track of pane references for nested splits
                let mut pane_refs = Vec::new();
                for i in 2..=self.terminals_per_column {
                    if i == 2 {
                        applescript.push_str(&format!(
                            r#"
            set pane{} to (split horizontally with default profile)
            tell pane{}
                delay 1
                write text "cd {}""#,
                            i, i, path
                        ));
                        pane_refs.push(format!("pane{}", i));
                    } else {
                        // Nested splits within the last pane
                        applescript.push_str(&format!(
                            r#"
                
                set pane{} to (split horizontally with default profile)
                tell pane{}
                    delay 1
                    write text "cd {}""#,
                            i, i, path
                        ));
                        pane_refs.push(format!("pane{}", i));
                    }
                }

                // Close all the nested tells
                for j in 2..=self.terminals_per_column {
                    if j > 2 {
                        applescript.push_str("\n                end tell");
                    }
                }
                if self.terminals_per_column > 1 {
                    applescript.push_str("\n            end tell");
                }
            }
        } else {
            // Multiple apps: create dynamic column layout
            applescript.push_str(&format!(
                r#"
            -- {} apps: {}x{} layout
            -- Create {} columns"#,
                num_apps, num_apps, self.terminals_per_column, num_apps
            ));

            // Create vertical splits for columns (skip first column as it's the current session)
            for i in 2..=num_apps {
                if i == 2 {
                    applescript.push_str(
                        "\n            set col2 to (split vertically with default profile)",
                    );
                } else {
                    applescript.push_str(&format!(
                        "\n            tell col{}\n                set col{} to (split vertically with default profile)\n            end tell",
                        i - 1, i
                    ));
                }
            }

            // Split each column horizontally to get the configured number of panes
            if self.terminals_per_column > 1 {
                applescript.push_str(&format!(
                    "\n            \n            -- Split each column horizontally for {} panes",
                    self.terminals_per_column
                ));

                // For column 1 (current session)
                for pane_idx in 2..=self.terminals_per_column {
                    if pane_idx == 2 {
                        applescript.push_str(&format!("\n            set col1Pane{} to (split horizontally with default profile)", pane_idx));
                    } else {
                        applescript.push_str(&format!("\n            tell col1Pane{}\n                set col1Pane{} to (split horizontally with default profile)\n            end tell", pane_idx - 1, pane_idx));
                    }
                }

                // For other columns
                for i in 2..=num_apps {
                    for pane_idx in 2..=self.terminals_per_column {
                        if pane_idx == 2 {
                            applescript.push_str(&format!("\n            tell col{}\n                set col{}Pane{} to (split horizontally with default profile)\n            end tell", i, i, pane_idx));
                        } else {
                            applescript.push_str(&format!("\n            tell col{}Pane{}\n                set col{}Pane{} to (split horizontally with default profile)\n            end tell", i, pane_idx - 1, i, pane_idx));
                        }
                    }
                }
            }

            // Populate panes
            applescript.push_str("\n            \n            -- Populate panes");
            for (i, (app, path)) in worktree_paths.iter().enumerate() {
                let col_num = i + 1;

                if i == 0 {
                    // First column uses current session
                    applescript.push_str(&format!(
                        r#"
            -- App {}: {} (column {})
            -- Top pane: AI command
            delay 2
            write text "cd {} && {}""#,
                        i + 1,
                        app.as_str(),
                        col_num,
                        path,
                        app.command()
                    ));

                    // Additional panes for shells
                    for pane_idx in 2..=self.terminals_per_column {
                        applescript.push_str(&format!(
                            r#"
            
            -- Pane {}: shell
            tell col1Pane{}
                delay 1
                write text "cd {}"
            end tell"#,
                            pane_idx, pane_idx, path
                        ));
                    }
                } else {
                    // Other columns use colN references
                    applescript.push_str(&format!(
                        r#"
            
            -- App {}: {} (column {})
            -- Top pane: AI command
            tell col{}
                delay 1
                write text "cd {} && {}"
            end tell"#,
                        i + 1,
                        app.as_str(),
                        col_num,
                        col_num,
                        path,
                        app.command()
                    ));

                    // Additional panes for shells
                    for pane_idx in 2..=self.terminals_per_column {
                        applescript.push_str(&format!(
                            r#"
            
            -- Pane {}: shell
            tell col{}Pane{}
                delay 1
                write text "cd {}"
            end tell"#,
                            pane_idx, col_num, pane_idx, path
                        ));
                    }
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

        applescript.push_str(
            r#"
        end tell
    end tell
end tell"#,
        );

        // Debug: Log the AppleScript being executed
        eprintln!(
            "DEBUG: Executing AppleScript for {} apps",
            worktree_paths.len()
        );

        // Execute the AppleScript
        let output = Command::new("osascript")
            .arg("-e")
            .arg(&applescript)
            .output()
            .map_err(|e| MultiAiError::ITerm2(format!("Failed to execute AppleScript: {}", e)))?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            eprintln!("DEBUG: AppleScript stderr: {}", error);
            eprintln!(
                "DEBUG: AppleScript stdout: {}",
                String::from_utf8_lossy(&output.stdout)
            );
            return Err(MultiAiError::ITerm2(format!(
                "AppleScript failed: {}",
                error
            )));
        }

        Ok(())
    }
}
