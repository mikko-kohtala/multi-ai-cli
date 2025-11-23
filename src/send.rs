use crate::config::{AiApp, Mode};
use crate::error::{MultiAiError, Result};
use crate::load_project_config;
use crate::tmux::{PaneInfo, TmuxManager};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
    MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Tabs, Wrap};
use ratatui::{Frame, Terminal};
use std::collections::BTreeMap;
use std::io;
use std::time::Duration;

const TARGET_WINDOW: &str = "apps";

#[derive(Clone)]
struct ColumnTarget {
    app: AiApp,
    top_pane: Option<String>,
    command_pane: Option<String>,
}

#[derive(Copy, Clone, PartialEq)]
enum Focus {
    Input,
    Sessions,
    Apps,
    Mode,
    Options,
}

#[derive(Copy, Clone, PartialEq)]
enum SendMode {
    Prompt,
    Command,
}

#[derive(Default, Clone, Copy)]
struct LayoutSlots {
    input: Rect,
    sessions: Rect,
    apps: Rect,
    mode: Rect,
    ultrathink: Rect,
    clear: Rect,
    send: Rect,
}

pub fn run_send() -> Result<()> {
    if !TmuxManager::is_tmux_installed() {
        return Err(MultiAiError::Tmux(
            "tmux is not installed or not in PATH".to_string(),
        ));
    }

    let project_path = std::env::current_dir()
        .map_err(|e| MultiAiError::Config(format!("Failed to get current directory: {}", e)))?;
    let project_config = load_project_config(&project_path)?;
    let project_name = project_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| MultiAiError::Config("Invalid project path".to_string()))?;

    if project_config.ai_apps.is_empty() {
        return Err(MultiAiError::Config(
            "No ai_apps configured in multi-ai-config.jsonc".to_string(),
        ));
    }

    let sessions = TmuxManager::list_sessions()?;
    if sessions.is_empty() {
        return Err(MultiAiError::Tmux(
            "No tmux sessions found. Start a multi-ai session first.".to_string(),
        ));
    }

    let prefix = format!("{}-", project_name);
    let filtered: Vec<String> = sessions
        .iter()
        .filter(|s| s.starts_with(&prefix) || *s == project_name)
        .cloned()
        .collect();
    let sessions = if filtered.is_empty() {
        sessions
    } else {
        filtered
    };

    let mut state = SendState::new(project_config.ai_apps, sessions, project_config.mode);
    if let Err(err) = state.refresh_targets() {
        state.error = Some(err.to_string());
    }

    let mut terminal = setup_terminal()?;
    let result = run_app(&mut terminal, &mut state);
    cleanup_terminal(&mut terminal)?;
    result
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(io::stdout());
    Ok(Terminal::new(backend)?)
}

fn cleanup_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    Ok(())
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut SendState,
) -> Result<()> {
    while !state.should_quit {
        terminal.draw(|f| render(f, state))?;
        handle_event(state)?;
    }
    Ok(())
}

struct SendState {
    input: String,
    cursor: usize,
    sessions: Vec<String>,
    session_idx: usize,
    apps: Vec<AiApp>,
    app_idx: usize,
    send_mode: SendMode,
    apply_ultrathink: bool,
    clear_after_send: bool,
    status: String,
    error: Option<String>,
    targets: Vec<ColumnTarget>,
    focus: Focus,
    option_idx: usize,
    layouts: LayoutSlots,
    should_quit: bool,
    configured_mode: Option<Mode>,
}

impl SendState {
    fn new(apps: Vec<AiApp>, sessions: Vec<String>, configured_mode: Option<Mode>) -> Self {
        Self {
            input: String::new(),
            cursor: 0,
            sessions,
            session_idx: 0,
            apps,
            app_idx: 0,
            send_mode: SendMode::Prompt,
            apply_ultrathink: false,
            clear_after_send: false,
            status: String::from("Select a target and press Ctrl+S to send"),
            error: None,
            targets: Vec::new(),
            focus: Focus::Input,
            option_idx: 0,
            layouts: LayoutSlots::default(),
            should_quit: false,
            configured_mode,
        }
    }

    fn selected_session(&self) -> Option<&str> {
        self.sessions.get(self.session_idx).map(|s| s.as_str())
    }

    fn selected_target(&self) -> Option<&ColumnTarget> {
        self.targets.get(self.app_idx)
    }

    fn current_ultrathink(&self) -> Option<&str> {
        self.apps.get(self.app_idx).and_then(|a| a.ultrathink())
    }

    fn mode_label(&self) -> &'static str {
        match self.configured_mode {
            Some(Mode::Iterm2) => "iterm2",
            Some(Mode::TmuxMultiWindow) => "tmux-multi-window",
            Some(Mode::TmuxSingleWindow) => "tmux-single-window",
            None => "auto",
        }
    }

    fn refresh_targets(&mut self) -> Result<()> {
        let session = self
            .selected_session()
            .ok_or_else(|| MultiAiError::Tmux("No tmux session selected".to_string()))?
            .to_string();

        let tmux = TmuxManager::from_session_name(&session);
        let panes = tmux.list_panes_in_window(TARGET_WINDOW)?;

        let mut grouped: BTreeMap<u32, Vec<PaneInfo>> = BTreeMap::new();
        for pane in panes {
            grouped.entry(pane.left).or_default().push(pane);
        }

        let mut columns: Vec<Vec<PaneInfo>> = grouped
            .into_iter()
            .map(|(_, mut panes)| {
                panes.sort_by_key(|p| p.top);
                panes
            })
            .collect();
        columns.sort_by_key(|panes| panes.first().map(|p| p.left).unwrap_or(0));

        let mut targets: Vec<ColumnTarget> = Vec::with_capacity(self.apps.len());
        for (idx, app) in self.apps.iter().enumerate() {
            if let Some(panes) = columns.get(idx) {
                let top = panes.get(0).map(|p| p.id.clone());
                let command = panes.get(1).map(|p| p.id.clone());
                targets.push(ColumnTarget {
                    app: app.clone(),
                    top_pane: top,
                    command_pane: command,
                });
            } else {
                targets.push(ColumnTarget {
                    app: app.clone(),
                    top_pane: None,
                    command_pane: None,
                });
            }
        }

        self.targets = targets;
        self.status = format!(
            "Session '{}' mapped: {} column(s) | mode: {}",
            session,
            columns.len(),
            self.mode_label()
        );

        if columns.len() < self.apps.len() {
            self.error = Some(format!(
                "Found {} column(s) but config lists {} apps.",
                columns.len(),
                self.apps.len()
            ));
        } else {
            self.error = None;
        }

        self.apply_ultrathink =
            matches!(self.send_mode, SendMode::Prompt) && self.current_ultrathink().is_some();

        Ok(())
    }

    fn focus_next(&mut self) {
        self.focus = match self.focus {
            Focus::Input => Focus::Sessions,
            Focus::Sessions => Focus::Apps,
            Focus::Apps => Focus::Mode,
            Focus::Mode => {
                self.option_idx = 0;
                Focus::Options
            }
            Focus::Options => Focus::Input,
        };
    }

    fn focus_prev(&mut self) {
        self.focus = match self.focus {
            Focus::Input => Focus::Options,
            Focus::Sessions => Focus::Input,
            Focus::Apps => Focus::Sessions,
            Focus::Mode => Focus::Apps,
            Focus::Options => Focus::Mode,
        };
    }

    fn move_option_focus(&mut self, delta: i32) {
        let items = 3;
        let current = self.option_idx as i32 + delta;
        self.option_idx = ((current % items + items) % items) as usize;
    }

    fn toggle_send_mode(&mut self) {
        self.send_mode = match self.send_mode {
            SendMode::Prompt => SendMode::Command,
            SendMode::Command => SendMode::Prompt,
        };
        if self.send_mode == SendMode::Command {
            self.apply_ultrathink = false;
        } else if self.current_ultrathink().is_some() {
            self.apply_ultrathink = true;
        }
    }

    fn insert_char(&mut self, ch: char) {
        self.input.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let new_cursor = self.input[..self.cursor]
            .char_indices()
            .last()
            .map(|(idx, _)| idx)
            .unwrap_or(0);
        self.input.drain(new_cursor..self.cursor);
        self.cursor = new_cursor;
    }

    fn delete(&mut self) {
        if self.cursor >= self.input.len() {
            return;
        }
        let end = self.input[self.cursor..]
            .char_indices()
            .nth(1)
            .map(|(idx, _)| self.cursor + idx)
            .unwrap_or_else(|| self.input.len());
        self.input.drain(self.cursor..end);
    }

    fn move_left(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.cursor = self.input[..self.cursor]
            .char_indices()
            .last()
            .map(|(idx, _)| idx)
            .unwrap_or(0);
    }

    fn move_right(&mut self) {
        if self.cursor >= self.input.len() {
            return;
        }
        self.cursor = self.input[self.cursor..]
            .char_indices()
            .nth(1)
            .map(|(idx, _)| self.cursor + idx)
            .unwrap_or_else(|| self.input.len());
    }

    fn move_vertical(&mut self, delta: i32) {
        let (row, col) = self.cursor_row_col();
        let lines: Vec<&str> = self.input.split('\n').collect();
        if lines.is_empty() {
            return;
        }
        let current_row = row as i32 + delta;
        if current_row < 0 {
            self.cursor = 0;
            return;
        }
        if current_row as usize >= lines.len() {
            self.cursor = self.input.len();
            return;
        }

        let target_line = lines[current_row as usize];
        let target_col = col.min(target_line.chars().count());
        let mut new_cursor = 0usize;
        for (idx, line) in lines.iter().enumerate() {
            if idx < current_row as usize {
                new_cursor += line.len() + 1; // +1 for newline
            }
        }
        let mut chars = target_line.chars();
        for _ in 0..target_col {
            if let Some(c) = chars.next() {
                new_cursor += c.len_utf8();
            }
        }
        self.cursor = new_cursor;
    }

    fn cursor_row_col(&self) -> (usize, usize) {
        let mut row = 0usize;
        let mut col = 0usize;
        for ch in self.input[..self.cursor].chars() {
            if ch == '\n' {
                row += 1;
                col = 0;
            } else {
                col += 1;
            }
        }
        (row, col)
    }

    fn cursor_position(&self, area: Rect) -> (u16, u16) {
        let (row, col) = self.cursor_row_col();
        let x = area.x.saturating_add(1).saturating_add(col as u16);
        let y = area.y.saturating_add(1).saturating_add(row as u16);
        (x, y)
    }

    fn set_app_idx(&mut self, idx: usize) {
        if idx < self.apps.len() {
            self.app_idx = idx;
            self.apply_ultrathink =
                matches!(self.send_mode, SendMode::Prompt) && self.current_ultrathink().is_some();
        }
    }

    fn send(&mut self) -> Result<()> {
        let target = self
            .selected_target()
            .ok_or_else(|| MultiAiError::Tmux("No target found for selected app".to_string()))?;

        let session = self
            .selected_session()
            .ok_or_else(|| MultiAiError::Tmux("No tmux session selected".to_string()))?;

        let pane_id = match self.send_mode {
            SendMode::Prompt => target.top_pane.as_ref().ok_or_else(|| {
                MultiAiError::Tmux("Top pane not found for selected app".to_string())
            })?,
            SendMode::Command => target.command_pane.as_ref().ok_or_else(|| {
                MultiAiError::Tmux("Command pane (second terminal) not found for app".to_string())
            })?,
        };

        if self.input.trim().is_empty() {
            self.error = Some("Enter text to send first.".to_string());
            return Ok(());
        }

        let mut payload = self.input.clone();
        if matches!(self.send_mode, SendMode::Prompt) && self.apply_ultrathink {
            if let Some(hint) = self.current_ultrathink() {
                if !payload.ends_with('\n') {
                    payload.push('\n');
                }
                payload.push('\n');
                payload.push_str(hint);
            }
        }

        let tmux = TmuxManager::from_session_name(session);
        tmux.paste_text_to_pane(pane_id, &payload, true)?;

        self.status = format!(
            "Sent {} to {} in session {}",
            match self.send_mode {
                SendMode::Prompt => "prompt",
                SendMode::Command => "command",
            },
            target.app.name,
            session
        );
        self.error = None;

        if self.clear_after_send {
            self.input.clear();
            self.cursor = 0;
        }

        Ok(())
    }
}

fn handle_event(state: &mut SendState) -> Result<()> {
    if !event::poll(Duration::from_millis(32))? {
        return Ok(());
    }

    match event::read()? {
        Event::Key(key) => handle_key_event(state, key)?,
        Event::Mouse(mouse) => handle_mouse(state, mouse)?,
        Event::Resize(_, _) => {}
        Event::FocusGained | Event::FocusLost | Event::Paste(_) => {}
    }

    Ok(())
}

fn handle_key_event(state: &mut SendState, key: KeyEvent) -> Result<()> {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        state.should_quit = true;
        return Ok(());
    }

    if key.code == KeyCode::Esc {
        state.should_quit = true;
        return Ok(());
    }

    if key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL) {
        if let Err(err) = state.send() {
            state.error = Some(err.to_string());
        }
        return Ok(());
    }

    match state.focus {
        Focus::Input => handle_input_keys(state, key),
        Focus::Sessions => handle_session_keys(state, key)?,
        Focus::Apps => handle_app_keys(state, key),
        Focus::Mode => handle_mode_keys(state, key),
        Focus::Options => handle_option_keys(state, key)?,
    }

    Ok(())
}

fn handle_input_keys(state: &mut SendState, key: KeyEvent) {
    match key.code {
        KeyCode::Enter => state.insert_char('\n'),
        KeyCode::Backspace => state.backspace(),
        KeyCode::Delete => state.delete(),
        KeyCode::Left => state.move_left(),
        KeyCode::Right => state.move_right(),
        KeyCode::Up => state.move_vertical(-1),
        KeyCode::Down => state.move_vertical(1),
        KeyCode::Home => state.cursor = 0,
        KeyCode::End => state.cursor = state.input.len(),
        KeyCode::Tab => state.focus_next(),
        KeyCode::BackTab => state.focus_prev(),
        KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.insert_char(ch)
        }
        _ => {}
    }
}

fn handle_session_keys(state: &mut SendState, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Up => {
            if state.session_idx > 0 {
                state.session_idx -= 1;
                if let Err(err) = state.refresh_targets() {
                    state.error = Some(err.to_string());
                }
            }
        }
        KeyCode::Down => {
            if state.session_idx + 1 < state.sessions.len() {
                state.session_idx += 1;
                if let Err(err) = state.refresh_targets() {
                    state.error = Some(err.to_string());
                }
            }
        }
        KeyCode::Tab => state.focus_next(),
        KeyCode::BackTab => state.focus_prev(),
        _ => {}
    }
    Ok(())
}

fn handle_app_keys(state: &mut SendState, key: KeyEvent) {
    match key.code {
        KeyCode::Up => {
            if state.app_idx > 0 {
                state.set_app_idx(state.app_idx - 1);
            }
        }
        KeyCode::Down => {
            if state.app_idx + 1 < state.apps.len() {
                state.set_app_idx(state.app_idx + 1);
            }
        }
        KeyCode::Tab => state.focus_next(),
        KeyCode::BackTab => state.focus_prev(),
        _ => {}
    }
}

fn handle_mode_keys(state: &mut SendState, key: KeyEvent) {
    match key.code {
        KeyCode::Left | KeyCode::Right | KeyCode::Enter => state.toggle_send_mode(),
        KeyCode::Tab => {
            state.option_idx = 0;
            state.focus_next();
        }
        KeyCode::BackTab => state.focus_prev(),
        _ => {}
    }
}

fn handle_option_keys(state: &mut SendState, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Up => state.move_option_focus(-1),
        KeyCode::Down => state.move_option_focus(1),
        KeyCode::Tab => state.focus_next(),
        KeyCode::BackTab => state.focus_prev(),
        KeyCode::Enter | KeyCode::Char(' ') => match state.option_idx {
            0 => {
                if state.current_ultrathink().is_some() && state.send_mode == SendMode::Prompt {
                    state.apply_ultrathink = !state.apply_ultrathink;
                }
            }
            1 => state.clear_after_send = !state.clear_after_send,
            2 => {
                if let Err(err) = state.send() {
                    state.error = Some(err.to_string());
                }
            }
            _ => {}
        },
        _ => {}
    }
    Ok(())
}

fn handle_mouse(state: &mut SendState, mouse: MouseEvent) -> Result<()> {
    if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
        return Ok(());
    }

    let x = mouse.column;
    let y = mouse.row;

    if contains(state.layouts.input, x, y) {
        state.focus = Focus::Input;
        return Ok(());
    }

    if contains(state.layouts.sessions, x, y) {
        let relative = y.saturating_sub(state.layouts.sessions.y + 1);
        let idx = relative as usize;
        if idx < state.sessions.len() {
            state.session_idx = idx;
            if let Err(err) = state.refresh_targets() {
                state.error = Some(err.to_string());
            }
        }
        state.focus = Focus::Sessions;
        return Ok(());
    }

    if contains(state.layouts.apps, x, y) {
        let relative = y.saturating_sub(state.layouts.apps.y + 1);
        let idx = relative as usize;
        if idx < state.apps.len() {
            state.set_app_idx(idx);
        }
        state.focus = Focus::Apps;
        return Ok(());
    }

    if contains(state.layouts.mode, x, y) {
        state.focus = Focus::Mode;
        state.toggle_send_mode();
        return Ok(());
    }

    if contains(state.layouts.ultrathink, x, y) {
        state.focus = Focus::Options;
        state.option_idx = 0;
        if state.send_mode == SendMode::Prompt && state.current_ultrathink().is_some() {
            state.apply_ultrathink = !state.apply_ultrathink;
        }
        return Ok(());
    }

    if contains(state.layouts.clear, x, y) {
        state.focus = Focus::Options;
        state.option_idx = 1;
        state.clear_after_send = !state.clear_after_send;
        return Ok(());
    }

    if contains(state.layouts.send, x, y) {
        state.focus = Focus::Options;
        state.option_idx = 2;
        if let Err(err) = state.send() {
            state.error = Some(err.to_string());
        }
    }

    Ok(())
}

fn contains(rect: Rect, x: u16, y: u16) -> bool {
    x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height
}

fn render(f: &mut Frame, state: &mut SendState) {
    let main_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
        .split(f.area());

    state.layouts.input = main_layout[0];
    render_input(f, main_layout[0], state);

    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(main_layout[1]);

    render_target_panel(f, right[0], state);
    render_options_panel(f, right[1], state);
}

fn render_input(f: &mut Frame, area: Rect, state: &mut SendState) {
    let title = " Text to send (Ctrl+S to send, Tab to navigate) ";
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(match state.focus {
            Focus::Input => Style::default().fg(Color::Cyan),
            _ => Style::default(),
        });

    let paragraph = Paragraph::new(state.input.as_str())
        .wrap(Wrap { trim: false })
        .block(block);

    f.render_widget(paragraph, area);

    if state.focus == Focus::Input {
        let (x, y) = state.cursor_position(area);
        f.set_cursor_position((x, y));
    }
}

fn render_target_panel(f: &mut Frame, area: Rect, state: &mut SendState) {
    let target_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(6),
            Constraint::Min(6),
            Constraint::Length(4),
        ])
        .split(area);

    state.layouts.sessions = target_chunks[0];
    state.layouts.apps = target_chunks[1];
    state.layouts.mode = target_chunks[2];

    let session_items: Vec<ListItem> = state
        .sessions
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let marker = if i == state.session_idx { ">" } else { " " };
            let style = if i == state.session_idx {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(format!("{} {}", marker, s)).style(style)
        })
        .collect();
    let session_block = Block::default().borders(Borders::ALL).title(" Sessions ");
    f.render_widget(
        List::new(session_items).block(session_block),
        target_chunks[0],
    );

    let app_items: Vec<ListItem> = state
        .apps
        .iter()
        .enumerate()
        .map(|(i, app)| {
            let target = state.targets.get(i);
            let mut label = format!("{} ", app.name);
            if let Some(t) = target {
                if t.top_pane.is_none() {
                    label.push_str("(missing)");
                }
            }

            let style = if i == state.app_idx {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else if target
                .and_then(|t| if t.top_pane.is_none() { Some(()) } else { None })
                .is_some()
            {
                Style::default().fg(Color::Red)
            } else {
                Style::default()
            };
            ListItem::new(label).style(style)
        })
        .collect();
    let app_block = Block::default()
        .borders(Borders::ALL)
        .title(" Target app (column) ");
    f.render_widget(List::new(app_items).block(app_block), target_chunks[1]);

    let titles = vec!["Prompt", "Command"];
    let mode_block = Tabs::new(
        titles
            .iter()
            .map(|t| Line::from(Span::styled(*t, Style::default())))
            .collect::<Vec<_>>(),
    )
    .select(match state.send_mode {
        SendMode::Prompt => 0,
        SendMode::Command => 1,
    })
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Send type ")
            .border_style(match state.focus {
                Focus::Mode => Style::default().fg(Color::Cyan),
                _ => Style::default(),
            }),
    )
    .style(Style::default().fg(Color::White))
    .highlight_style(
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
    );

    f.render_widget(mode_block, target_chunks[2]);
}

fn render_options_panel(f: &mut Frame, area: Rect, state: &mut SendState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(area);

    state.layouts.ultrathink = chunks[0];
    state.layouts.clear = chunks[1];
    state.layouts.send = chunks[2];

    let ultrathink_available =
        state.send_mode == SendMode::Prompt && state.current_ultrathink().is_some();
    let ultra_label = if ultrathink_available {
        let hint = state.current_ultrathink().unwrap_or("");
        format!(
            "[{}] Append ultrathink hint ({})",
            if state.apply_ultrathink { "x" } else { " " },
            hint
        )
    } else {
        "[ ] Append ultrathink hint (not available)".to_string()
    };

    let ultra = Paragraph::new(ultra_label)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Deep thinking ")
                .border_style(option_border(state, 0)),
        )
        .style(if ultrathink_available {
            Style::default()
        } else {
            Style::default().fg(Color::DarkGray)
        });
    f.render_widget(ultra, chunks[0]);

    let clear = Paragraph::new(format!(
        "[{}] Clear input after send",
        if state.clear_after_send { "x" } else { " " }
    ))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(" After send ")
            .border_style(option_border(state, 1)),
    );
    f.render_widget(clear, chunks[1]);

    let send = Paragraph::new("Send now (Enter / click / Ctrl+S)")
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Action ")
                .border_style(option_border(state, 2)),
        )
        .alignment(ratatui::layout::Alignment::Center);
    f.render_widget(send, chunks[2]);

    let mut status_lines = vec![Line::from(Span::styled(
        &state.status,
        Style::default().fg(Color::Green),
    ))];

    if let Some(err) = &state.error {
        status_lines.push(Line::from(Span::styled(
            err,
            Style::default().fg(Color::Red),
        )));
    }

    let status = Paragraph::new(status_lines)
        .block(Block::default().borders(Borders::ALL).title(" Status "))
        .wrap(Wrap { trim: true });

    f.render_widget(status, chunks[3]);
}

fn option_border(state: &SendState, idx: usize) -> Style {
    if state.focus == Focus::Options && state.option_idx == idx {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default()
    }
}
