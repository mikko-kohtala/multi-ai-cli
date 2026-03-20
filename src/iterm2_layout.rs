//! Shared iTerm2 layout creation using the `it2` CLI tool.
//!
//! Used by both the review and plan commands to create column-based layouts
//! where each AI tool gets a vertical column with an AI pane (top) and shell
//! pane (bottom). Panes are targeted by session UUID, which eliminates the
//! "wrong window" problem that plagued the old AppleScript approach.

use crate::config::AiApp;
use crate::error::{MultiAiError, Result};
use std::process::Command;
use std::thread;
use std::time::Duration;

/// Whether a column hosts a regular AI reviewer or a meta reviewer.
#[derive(Clone, Copy, PartialEq)]
pub enum PaneTag {
    Ai,
    Meta,
}

/// Describes a single column in the iTerm2 layout.
pub struct ColumnSpec {
    pub app: AiApp,
    pub worktree_path: String,
    pub tag: PaneTag,
}

/// Full specification for an iTerm2 layout.
pub struct LayoutSpec {
    pub columns: Vec<ColumnSpec>,
    pub ai_prompt: String,
    pub meta_prompt: Option<String>,
    pub send_prompts: bool,
    pub tab_title: String,
}

// ---------------------------------------------------------------------------
// it2 CLI helpers
// ---------------------------------------------------------------------------

fn find_it2() -> Result<String> {
    // Check PATH first
    if let Ok(output) = Command::new("which").arg("it2").output()
        && output.status.success()
    {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !path.is_empty() {
            return Ok(path);
        }
    }

    // Check common install locations
    if let Some(home) = dirs::home_dir() {
        let local_bin = home.join(".local/bin/it2");
        if local_bin.exists() {
            return Ok(local_bin.to_string_lossy().to_string());
        }
    }

    Err(MultiAiError::ITerm2(
        "it2 CLI not found. Install with: uvx install it2".to_string(),
    ))
}

fn run_it2(it2_path: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(it2_path)
        .args(args)
        .output()
        .map_err(|e| MultiAiError::ITerm2(format!("Failed to execute it2: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(MultiAiError::ITerm2(format!(
            "it2 {} failed: {}",
            args.join(" "),
            stderr
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Parse an ID from it2 output like "Created new tab: W0:T42" or "Created new pane: <uuid>".
fn parse_created_id(output: &str) -> Result<String> {
    output
        .trim()
        .rsplit(": ")
        .next()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| MultiAiError::ITerm2(format!("Could not parse ID from it2 output: {output}")))
}

/// Find the session ID belonging to a given tab by querying `it2 session list --json`.
fn find_session_for_tab(it2_path: &str, tab_id: &str) -> Result<String> {
    let json_out = run_it2(it2_path, &["session", "list", "--json"])?;
    let sessions: Vec<serde_json::Value> = serde_json::from_str(&json_out)
        .map_err(|e| MultiAiError::ITerm2(format!("Failed to parse session list: {}", e)))?;

    for s in &sessions {
        if s.get("tab_id").and_then(|v| v.as_str()) == Some(tab_id)
            && let Some(id) = s.get("id").and_then(|v| v.as_str())
        {
            return Ok(id.to_string());
        }
    }

    Err(MultiAiError::ITerm2(format!(
        "No session found for tab {tab_id}"
    )))
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Create an iTerm2 tab layout using the `it2` CLI.
///
/// Each column gets a vertical split (AI tool on top, shell on bottom).
/// After the tools start, optional prompts are sent to the AI panes.
pub fn create_iterm2_layout(spec: &LayoutSpec) -> Result<()> {
    if spec.columns.is_empty() {
        return Ok(());
    }

    let it2 = find_it2()?;

    // 1. Create a new tab and discover its initial session.
    let tab_output = run_it2(&it2, &["tab", "new"])?;
    let tab_id = parse_created_id(&tab_output)?;
    // Small pause for the tab to initialise before querying sessions.
    thread::sleep(Duration::from_millis(500));
    let first_session = find_session_for_tab(&it2, &tab_id)?;

    // 2. Create vertical splits for additional columns.
    //    Each new column is created by splitting the *previous* column.
    let mut ai_panes: Vec<String> = Vec::with_capacity(spec.columns.len());
    ai_panes.push(first_session);

    for i in 1..spec.columns.len() {
        let new_pane = run_it2(
            &it2,
            &["session", "split", "-v", "-s", &ai_panes[i - 1]],
        )?;
        ai_panes.push(parse_created_id(&new_pane)?);
    }

    // 3. Create horizontal splits for shell panes within each column.
    let mut shell_panes: Vec<String> = Vec::with_capacity(spec.columns.len());
    for ai_pane in &ai_panes {
        let shell = run_it2(&it2, &["session", "split", "-s", ai_pane])?;
        shell_panes.push(parse_created_id(&shell)?);
    }

    // 4. Launch AI tools and cd into worktree paths.
    //    Small delay to let shells initialise before sending commands.
    thread::sleep(Duration::from_secs(1));

    for (i, col) in spec.columns.iter().enumerate() {
        let cd_and_cmd = format!("cd {} && {}", col.worktree_path, col.app.command());
        run_it2(&it2, &["session", "run", "-s", &ai_panes[i], &cd_and_cmd])?;

        let cd_cmd = format!("cd {}", col.worktree_path);
        run_it2(&it2, &["session", "run", "-s", &shell_panes[i], &cd_cmd])?;
    }

    // 5. Send prompts after a delay so AI tools have time to start.
    if spec.send_prompts {
        thread::sleep(Duration::from_secs(5));

        for (i, col) in spec.columns.iter().enumerate() {
            match col.tag {
                PaneTag::Ai => {
                    // Send prompt with newline (auto-submits).
                    run_it2(
                        &it2,
                        &["session", "run", "-s", &ai_panes[i], &spec.ai_prompt],
                    )?;
                    thread::sleep(Duration::from_millis(500));
                    // Send empty line to confirm (matches previous behavior).
                    run_it2(&it2, &["session", "run", "-s", &ai_panes[i], ""])?;
                }
                PaneTag::Meta => {
                    if let Some(ref meta) = spec.meta_prompt {
                        thread::sleep(Duration::from_secs(1));
                        // Send without newline so the user can review before pressing Enter.
                        run_it2(
                            &it2,
                            &["session", "send", "-s", &ai_panes[i], meta],
                        )?;
                    }
                }
            }
        }
    }

    // 6. Set the tab title via the first session's name.
    run_it2(
        &it2,
        &["session", "set-name", "-s", &ai_panes[0], &spec.tab_title],
    )?;

    Ok(())
}
