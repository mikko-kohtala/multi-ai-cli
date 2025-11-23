use crate::config::ProjectConfig;
use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame, Terminal,
};
use std::io;
use std::process::Command;

// ============================================================================
// Data Structures
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
pub enum PaneType {
    AI(String),  // AI tool name
    Shell,
}

#[derive(Debug, Clone)]
pub struct PaneInfo {
    pub id: String,           // e.g., "%0"
    pub window: String,       // e.g., "claude" or "apps"
    pub pane_index: usize,
    pub pane_type: PaneType,
    pub current_command: String,
}

#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MessageType {
    Prompt,   // For AI tools
    Command,  // For shell panes
}

#[derive(Debug, Clone, PartialEq)]
pub enum TargetMode {
    All,      // All panes of the selected type
    Single,   // Specific pane(s) selected
}

#[derive(Debug, Clone, PartialEq)]
enum FocusedPanel {
    TextInput,
    SessionList,
    PaneList,
    Options,
}

struct SendState {
    sessions: Vec<SessionInfo>,
    selected_session_idx: usize,
    panes: Vec<PaneInfo>,
    selected_panes: Vec<bool>,  // Checkbox state for each pane
    input_text: String,
    cursor_position: usize,
    message_type: MessageType,
    target_mode: TargetMode,
    deep_thinking: bool,
    focused_panel: FocusedPanel,
    session_list_state: ListState,
    pane_list_state: ListState,
    should_quit: bool,
    error_message: Option<String>,
}

impl SendState {
    fn new(sessions: Vec<SessionInfo>) -> Result<Self> {
        if sessions.is_empty() {
            return Err(anyhow::anyhow!(
                "No multi-ai tmux sessions found. Run 'mai add <branch-prefix>' first."
            ));
        }

        let mut state = Self {
            sessions,
            selected_session_idx: 0,
            panes: Vec::new(),
            selected_panes: Vec::new(),
            input_text: String::new(),
            cursor_position: 0,
            message_type: MessageType::Prompt,
            target_mode: TargetMode::All,
            deep_thinking: false,
            focused_panel: FocusedPanel::TextInput,
            session_list_state: ListState::default(),
            pane_list_state: ListState::default(),
            should_quit: false,
            error_message: None,
        };

        // Load panes for the first session
        state.load_panes_for_current_session()?;

        Ok(state)
    }

    fn load_panes_for_current_session(&mut self) -> Result<()> {
        let session_name = &self.sessions[self.selected_session_idx].name;
        self.panes = list_panes_for_session(session_name)?;
        self.selected_panes = vec![false; self.panes.len()];
        Ok(())
    }

    fn get_selected_panes(&self) -> Vec<&PaneInfo> {
        match self.target_mode {
            TargetMode::All => {
                // Get all panes matching the message type
                self.panes
                    .iter()
                    .filter(|p| match self.message_type {
                        MessageType::Prompt => matches!(p.pane_type, PaneType::AI(_)),
                        MessageType::Command => matches!(p.pane_type, PaneType::Shell),
                    })
                    .collect()
            }
            TargetMode::Single => {
                // Get specifically selected panes
                self.panes
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| self.selected_panes[*i])
                    .map(|(_, p)| p)
                    .collect()
            }
        }
    }

    fn toggle_current_pane(&mut self) {
        if let Some(idx) = self.pane_list_state.selected() {
            if idx < self.selected_panes.len() {
                self.selected_panes[idx] = !self.selected_panes[idx];
                // Switch to single mode when manually selecting
                self.target_mode = TargetMode::Single;
            }
        }
    }
}

// ============================================================================
// Tmux Integration Functions
// ============================================================================

/// Discover all running tmux sessions
fn list_tmux_sessions() -> Result<Vec<SessionInfo>> {
    let output = Command::new("tmux")
        .args(["list-sessions", "-F", "#{session_name}"])
        .output();

    match output {
        Ok(output) if output.status.success() => {
            let sessions = String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(|line| SessionInfo {
                    name: line.to_string(),
                })
                .collect();
            Ok(sessions)
        }
        Ok(_) => Ok(Vec::new()), // No sessions running
        Err(_) => Err(anyhow::anyhow!(
            "tmux is not running or not installed. Please install tmux and ensure a session is running."
        )),
    }
}

/// Get all panes for a specific session with metadata
fn list_panes_for_session(session_name: &str) -> Result<Vec<PaneInfo>> {
    let output = Command::new("tmux")
        .args([
            "list-panes",
            "-t",
            session_name,
            "-a",
            "-F",
            "#{pane_id}|#{window_name}|#{pane_index}|#{pane_current_command}|#{pane_current_path}",
        ])
        .output()?;

    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "Failed to get panes for session: {}",
            session_name
        ));
    }

    let panes: Vec<PaneInfo> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('|').collect();
            if parts.len() >= 5 {
                let pane_id = parts[0].to_string();
                let window_name = parts[1].to_string();
                let pane_index = parts[2].parse::<usize>().ok()?;
                let current_command = parts[3].to_string();
                let current_path = parts[4].to_string();

                let pane_type = classify_pane(&window_name, pane_index, &current_command, &current_path);

                Some(PaneInfo {
                    id: pane_id,
                    window: window_name,
                    pane_index,
                    pane_type,
                    current_command,
                })
            } else {
                None
            }
        })
        .collect();

    Ok(panes)
}

/// Classify a pane as AI or Shell based on metadata
/// Uses position-based logic and extracts AI tool name from directory path
fn classify_pane(window_name: &str, pane_index: usize, _current_command: &str, current_path: &str) -> PaneType {
    // Helper function to extract AI tool name from worktree path
    // Paths look like: /path/to/project/branch-prefix-aitool
    let extract_ai_from_path = |path: &str| -> String {
        std::path::Path::new(path)
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| {
                // Extract the last part after the last hyphen
                // e.g., "feature-branch-claude" -> "claude"
                name.rsplit('-').next()
            })
            .unwrap_or("AI")
            .to_string()
    };

    // For single-window layout ("apps"), use pane index (even = AI, odd = Shell)
    // Panes are created as: 0=AI, 1=Shell, 2=AI, 3=Shell, etc.
    if window_name == "apps" {
        if pane_index % 2 == 0 {
            // Even index = AI pane
            // Extract AI name from directory path
            let ai_name = extract_ai_from_path(current_path);
            return PaneType::AI(ai_name);
        } else {
            // Odd index = Shell pane
            return PaneType::Shell;
        }
    }

    // For multi-window layout, each window is named after the AI tool
    // Pane index 0 is AI (left), pane index 1 is Shell (right)
    if pane_index == 0 {
        // Window name IS the AI tool name in multi-window layout
        PaneType::AI(window_name.to_string())
    } else {
        PaneType::Shell
    }
}

/// Send text to a specific tmux pane
fn send_to_pane(pane_id: &str, text: &str, send_enter: bool) -> Result<()> {
    let args = vec!["send-keys", "-t", pane_id, "-l", text];

    if send_enter {
        // First send the text literally
        Command::new("tmux")
            .args(&args)
            .output()?;

        // Then send Enter separately
        Command::new("tmux")
            .args(["send-keys", "-t", pane_id, "Enter"])
            .output()?;
    } else {
        // Just send the text without Enter
        Command::new("tmux")
            .args(&args)
            .output()?;
    }

    Ok(())
}

// ============================================================================
// TUI Rendering
// ============================================================================

fn render_ui(f: &mut Frame, state: &mut SendState) {
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(3)])
        .split(f.area());

    let content_area = main_chunks[0];
    let help_area = main_chunks[1];

    // Split vertically: top (text input) and bottom (targets + options)
    let vertical_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(content_area);

    let text_area = vertical_chunks[0];
    let bottom_area = vertical_chunks[1];

    // Split bottom area horizontally into targets (left) and options (right)
    let bottom_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(bottom_area);

    let target_area = bottom_chunks[0];
    let options_area = bottom_chunks[1];

    // Render text input area (top)
    render_text_input(f, text_area, state);

    // Render target selection (bottom left)
    render_target_selection(f, target_area, state);

    // Render options (bottom right)
    render_options(f, options_area, state);

    // Render help bar
    render_help_bar(f, help_area);
}

fn render_text_input(f: &mut Frame, area: Rect, state: &mut SendState) {
    let is_focused = state.focused_panel == FocusedPanel::TextInput;
    let border_style = if is_focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default()
    };

    let block = Block::default()
        .title(" Message ")
        .borders(Borders::ALL)
        .border_style(border_style);

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Render the text content
    let text = if state.input_text.is_empty() {
        Line::from(vec![Span::styled(
            "Type your message here...",
            Style::default().fg(Color::DarkGray),
        )])
    } else {
        Line::from(state.input_text.clone())
    };

    let paragraph = Paragraph::new(text).wrap(Wrap { trim: false });
    f.render_widget(paragraph, inner);

    // Show cursor if focused
    if is_focused {
        let cursor_x = inner.x + (state.cursor_position as u16 % inner.width);
        let cursor_y = inner.y + (state.cursor_position as u16 / inner.width);
        f.set_cursor_position((cursor_x, cursor_y));
    }
}

fn render_target_selection(f: &mut Frame, area: Rect, state: &mut SendState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(5), Constraint::Min(5)])
        .split(area);

    let session_area = chunks[0];
    let pane_area = chunks[1];

    // Render sessions
    render_sessions(f, session_area, state);

    // Render panes
    render_panes(f, pane_area, state);
}

fn render_sessions(f: &mut Frame, area: Rect, state: &mut SendState) {
    let is_focused = state.focused_panel == FocusedPanel::SessionList;
    let border_style = if is_focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default()
    };

    let block = Block::default()
        .title(" Session ")
        .borders(Borders::ALL)
        .border_style(border_style);

    let inner = block.inner(area);
    f.render_widget(block, area);

    if !state.sessions.is_empty() {
        let selected_session = &state.sessions[state.selected_session_idx];
        let text = Line::from(vec![
            Span::raw("→ "),
            Span::styled(
                &selected_session.name,
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
        ]);
        let paragraph = Paragraph::new(text);
        f.render_widget(paragraph, inner);
    }
}

fn render_panes(f: &mut Frame, area: Rect, state: &mut SendState) {
    let is_focused = state.focused_panel == FocusedPanel::PaneList;
    let border_style = if is_focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default()
    };

    let title = format!(" Panes ({} total) ", state.panes.len());
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(border_style);

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Create list items
    let items: Vec<ListItem> = state
        .panes
        .iter()
        .enumerate()
        .map(|(idx, pane)| {
            let checkbox = if state.target_mode == TargetMode::Single && state.selected_panes[idx]
            {
                "[✓]"
            } else {
                "[ ]"
            };

            let (pane_label, color) = match &pane.pane_type {
                PaneType::AI(name) => (format!("{} (AI)", name), Color::Yellow),
                PaneType::Shell => ("Shell".to_string(), Color::Blue),
            };

            let content = format!("{} {} {}", checkbox, pane_label, pane.id);

            ListItem::new(Line::from(vec![Span::styled(
                content,
                Style::default().fg(color),
            )]))
        })
        .collect();

    let list = List::new(items)
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("► ");

    f.render_stateful_widget(list, inner, &mut state.pane_list_state);
}

fn render_options(f: &mut Frame, area: Rect, state: &mut SendState) {
    let is_focused = state.focused_panel == FocusedPanel::Options;
    let border_style = if is_focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default()
    };

    let block = Block::default()
        .title(" Options ")
        .borders(Borders::ALL)
        .border_style(border_style);

    let inner = block.inner(area);
    f.render_widget(block, area);

    let msg_type_indicator = match state.message_type {
        MessageType::Prompt => Span::styled("Prompt", Style::default().fg(Color::Yellow)),
        MessageType::Command => Span::styled("Command", Style::default().fg(Color::Blue)),
    };

    let target_indicator = match state.target_mode {
        TargetMode::All => Span::styled("All", Style::default().fg(Color::Green)),
        TargetMode::Single => Span::styled("Selected", Style::default().fg(Color::Magenta)),
    };

    let deep_think_checkbox = if state.deep_thinking { "[✓]" } else { "[ ]" };

    let lines = vec![
        Line::from(vec![
            Span::raw("Type: "),
            msg_type_indicator,
            Span::raw(" | Target: "),
            target_indicator,
        ]),
        Line::from(""),
        Line::from(vec![
            Span::raw(deep_think_checkbox),
            Span::raw(" Deep Thinking (D)"),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Press T to toggle type, M for mode",
            Style::default().fg(Color::DarkGray),
        )]),
    ];

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, inner);
}

fn render_help_bar(f: &mut Frame, area: Rect) {
    let help_text = Line::from(vec![
        Span::styled(" Enter", Style::default().fg(Color::Cyan)),
        Span::raw(": Send | "),
        Span::styled("Tab", Style::default().fg(Color::Cyan)),
        Span::raw(": Switch Panel | "),
        Span::styled("Space", Style::default().fg(Color::Cyan)),
        Span::raw(": Toggle Pane | "),
        Span::styled("Esc", Style::default().fg(Color::Cyan)),
        Span::raw(": Quit "),
    ]);

    let paragraph = Paragraph::new(help_text)
        .block(Block::default().borders(Borders::ALL))
        .style(Style::default().bg(Color::Black));

    f.render_widget(paragraph, area);
}

// ============================================================================
// Event Handling
// ============================================================================

fn handle_events(state: &mut SendState) -> Result<()> {
    if event::poll(std::time::Duration::from_millis(100))? {
        if let Event::Key(key) = event::read()? {
            match state.focused_panel {
                FocusedPanel::TextInput => handle_text_input_events(state, key),
                FocusedPanel::PaneList => handle_pane_list_events(state, key),
                FocusedPanel::Options => handle_options_events(state, key),
                _ => {}
            }

            // Global keybindings
            match key.code {
                KeyCode::Esc => state.should_quit = true,
                KeyCode::Tab => {
                    state.focused_panel = match state.focused_panel {
                        FocusedPanel::TextInput => FocusedPanel::PaneList,
                        FocusedPanel::PaneList => FocusedPanel::Options,
                        FocusedPanel::Options => FocusedPanel::TextInput,
                        _ => FocusedPanel::TextInput,
                    };
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn handle_text_input_events(state: &mut SendState, key: event::KeyEvent) {
    match key.code {
        KeyCode::Char(c) if key.modifiers.contains(KeyModifiers::SHIFT) && c == '\n' => {
            // Shift+Enter: new line
            state.input_text.push('\n');
            state.cursor_position += 1;
        }
        KeyCode::Char(c) => {
            state.input_text.push(c);
            state.cursor_position += 1;
        }
        KeyCode::Backspace => {
            if !state.input_text.is_empty() && state.cursor_position > 0 {
                state.input_text.pop();
                state.cursor_position = state.cursor_position.saturating_sub(1);
            }
        }
        KeyCode::Enter if !key.modifiers.contains(KeyModifiers::SHIFT) => {
            // Enter without shift: send message
            if !state.input_text.is_empty() {
                if let Err(e) = send_message(state) {
                    state.error_message = Some(format!("Error sending message: {}", e));
                } else {
                    state.input_text.clear();
                    state.cursor_position = 0;
                    // Optionally quit after sending
                    state.should_quit = true;
                }
            }
        }
        _ => {}
    }
}

fn handle_pane_list_events(state: &mut SendState, key: event::KeyEvent) {
    match key.code {
        KeyCode::Up => {
            let i = match state.pane_list_state.selected() {
                Some(i) => {
                    if i == 0 {
                        state.panes.len().saturating_sub(1)
                    } else {
                        i - 1
                    }
                }
                None => 0,
            };
            state.pane_list_state.select(Some(i));
        }
        KeyCode::Down => {
            let i = match state.pane_list_state.selected() {
                Some(i) => {
                    if i >= state.panes.len().saturating_sub(1) {
                        0
                    } else {
                        i + 1
                    }
                }
                None => 0,
            };
            state.pane_list_state.select(Some(i));
        }
        KeyCode::Char(' ') => {
            state.toggle_current_pane();
        }
        KeyCode::Char('a') | KeyCode::Char('A') => {
            // Toggle all AI panes
            state.target_mode = TargetMode::All;
            state.message_type = MessageType::Prompt;
        }
        KeyCode::Char('s') | KeyCode::Char('S') => {
            // Toggle all Shell panes
            state.target_mode = TargetMode::All;
            state.message_type = MessageType::Command;
        }
        _ => {}
    }
}

fn handle_options_events(state: &mut SendState, key: event::KeyEvent) {
    match key.code {
        KeyCode::Char('t') | KeyCode::Char('T') => {
            // Toggle message type
            state.message_type = match state.message_type {
                MessageType::Prompt => MessageType::Command,
                MessageType::Command => MessageType::Prompt,
            };
        }
        KeyCode::Char('m') | KeyCode::Char('M') => {
            // Toggle target mode
            state.target_mode = match state.target_mode {
                TargetMode::All => TargetMode::Single,
                TargetMode::Single => TargetMode::All,
            };
        }
        KeyCode::Char('d') | KeyCode::Char('D') => {
            // Toggle deep thinking
            state.deep_thinking = !state.deep_thinking;
        }
        _ => {}
    }
}

// ============================================================================
// Message Sending Logic
// ============================================================================

fn send_message(state: &SendState) -> Result<()> {
    let panes = state.get_selected_panes();

    if panes.is_empty() {
        return Err(anyhow::anyhow!("No panes selected to send to"));
    }

    let mut message = state.input_text.clone();

    // Add ultrathink suffix if enabled and sending to AI panes
    if state.deep_thinking && state.message_type == MessageType::Prompt {
        // We'll need to pass the config to get ultrathink phrases
        // For now, use default phrases
        message.push_str("\n\nultrathink");
    }

    for pane in panes {
        send_to_pane(&pane.id, &message, true)?;
    }

    Ok(())
}

/// Apply ultrathink phrase based on AI tool name
fn apply_ultrathink(message: &str, pane: &PaneInfo, config: &ProjectConfig) -> String {
    if let PaneType::AI(ai_name) = &pane.pane_type {
        // Find the AI app in config
        for app in &config.ai_apps {
            if app.name == *ai_name {
                if let Some(ultrathink_phrase) = &app.ultrathink {
                    return format!("{}\n\n{}", message, ultrathink_phrase);
                }
            }
        }

        // Fallback to defaults if not in config
        let default_phrase = match ai_name.as_str() {
            "claude" => "ultrathink",
            "amp" => "Use oracle and think heavily",
            _ => "Think deeply about this",
        };

        return format!("{}\n\n{}", message, default_phrase);
    }

    message.to_string()
}

// ============================================================================
// Public API
// ============================================================================

/// Run the interactive TUI for sending messages
pub fn run_interactive_send(target_session: Option<String>) -> Result<()> {
    // Discover tmux sessions
    let mut sessions = list_tmux_sessions()?;

    // Filter by target session if specified
    if let Some(target) = target_session {
        sessions.retain(|s| s.name.contains(&target));
        if sessions.is_empty() {
            return Err(anyhow::anyhow!(
                "Session '{}' not found. Available sessions: {}",
                target,
                list_tmux_sessions()?
                    .iter()
                    .map(|s| s.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }

    // Initialize state
    let mut state = SendState::new(sessions)?;
    state.pane_list_state.select(Some(0));

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Main loop
    let result = run_tui_loop(&mut terminal, &mut state);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

fn run_tui_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut SendState,
) -> Result<()> {
    loop {
        terminal.draw(|f| render_ui(f, state))?;

        handle_events(state)?;

        if state.should_quit {
            break;
        }
    }

    Ok(())
}

/// Non-interactive mode: send a message directly
pub fn send_non_interactive(
    session: Option<String>,
    message: String,
    pane_ids: Option<Vec<String>>,
    ultrathink: bool,
) -> Result<()> {
    let sessions = list_tmux_sessions()?;

    if sessions.is_empty() {
        return Err(anyhow::anyhow!(
            "No tmux sessions found. Run 'mai add <branch-prefix>' first."
        ));
    }

    // Determine target session
    let target_session = if let Some(name) = session {
        sessions
            .iter()
            .find(|s| s.name == name)
            .ok_or_else(|| anyhow::anyhow!("Session '{}' not found", name))?
    } else if sessions.len() == 1 {
        &sessions[0]
    } else {
        return Err(anyhow::anyhow!(
            "Multiple sessions found. Please specify --session. Available: {}",
            sessions
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    };

    // Get panes
    let all_panes = list_panes_for_session(&target_session.name)?;

    // Determine target panes
    let target_panes: Vec<&PaneInfo> = if let Some(ids) = pane_ids {
        all_panes
            .iter()
            .filter(|p| ids.contains(&p.id))
            .collect()
    } else {
        // Default to all AI panes
        all_panes
            .iter()
            .filter(|p| matches!(p.pane_type, PaneType::AI(_)))
            .collect()
    };

    if target_panes.is_empty() {
        return Err(anyhow::anyhow!("No target panes found"));
    }

    // Send message
    let mut final_message = message.clone();
    if ultrathink {
        final_message.push_str("\n\nultrathink");
    }

    let pane_count = target_panes.len();
    for pane in target_panes {
        send_to_pane(&pane.id, &final_message, true)?;
    }

    println!(
        "✓ Sent message to {} pane(s) in session '{}'",
        pane_count,
        target_session.name
    );

    Ok(())
}
