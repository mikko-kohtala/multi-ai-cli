use crate::config::{AiApp, ProjectConfig};
use crate::error::{MultiAiError, Result};
use ratatui::crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers,
            KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags},
    execute, queue,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame, Terminal,
};
use std::io;
use std::process::Command;

#[derive(Clone, Copy, PartialEq)]
enum TargetType {
    Prompt,
    Command,
}

#[derive(Clone, Copy, PartialEq)]
enum FocusedWindow {
    Input,
    SessionList,
    AppList,
    Settings,
}

struct TuiState {
    input: String,
    // Simple cursor tracking (byte index)
    cursor_position: usize,
    
    sessions: Vec<String>,
    session_list_state: ListState,
    
    apps: Vec<AiApp>,
    app_list_state: ListState,
    
    target_type: TargetType,
    ultrathink: bool,
    
    focused: FocusedWindow,
    confirm_clear: bool,
    settings_list_state: ListState,
}

impl TuiState {
    fn new(sessions: Vec<String>, apps: Vec<AiApp>) -> Self {
        let mut session_list_state = ListState::default();
        if !sessions.is_empty() {
            session_list_state.select(Some(0));
        }
        
        let mut app_list_state = ListState::default();
        if !apps.is_empty() {
            app_list_state.select(Some(0));
        }
        
        let mut settings_list_state = ListState::default();
        settings_list_state.select(Some(0));

        Self {
            input: String::new(),
            cursor_position: 0,
            sessions,
            session_list_state,
            apps,
            app_list_state,
            target_type: TargetType::Prompt,
            ultrathink: false,
            focused: FocusedWindow::Input,
            confirm_clear: false,
            settings_list_state,
        }
    }

    fn insert_newline(&mut self) {
        self.input.insert(self.cursor_position, '\n');
        self.cursor_position += 1;
    }

    fn on_key(&mut self, key: KeyCode, modifiers: KeyModifiers) {
        match self.focused {
            FocusedWindow::Input => match key {
                KeyCode::Enter => {
                    // Only Shift+Enter creates a newline in the input field
                    // Plain Enter and Ctrl+Enter are handled by the main loop for sending
                    if modifiers.contains(KeyModifiers::SHIFT) {
                        self.insert_newline();
                    }
                    // Otherwise, do nothing - let main loop handle sending
                }
                KeyCode::Char(c) => {
                    if self.cursor_position >= self.input.len() {
                        self.input.push(c);
                    } else {
                        self.input.insert(self.cursor_position, c);
                    }
                    self.cursor_position += c.len_utf8();
                }
                KeyCode::Backspace => {
                    if self.cursor_position > 0 {
                        // Find char boundary
                        let mut prev = self.cursor_position - 1;
                        while !self.input.is_char_boundary(prev) {
                            prev -= 1;
                        }
                        self.input.remove(prev);
                        self.cursor_position = prev;
                    }
                }
                KeyCode::Delete => {
                    if self.cursor_position < self.input.len() {
                        self.input.remove(self.cursor_position);
                    }
                }
                KeyCode::Left => {
                    if self.cursor_position > 0 {
                        let mut prev = self.cursor_position - 1;
                        while !self.input.is_char_boundary(prev) {
                            prev -= 1;
                        }
                        self.cursor_position = prev;
                    }
                }
                KeyCode::Right => {
                    if self.cursor_position < self.input.len() {
                        let mut next = self.cursor_position + 1;
                        while next < self.input.len() && !self.input.is_char_boundary(next) {
                            next += 1;
                        }
                        self.cursor_position = next;
                    }
                }
                KeyCode::Tab => self.focused = FocusedWindow::SessionList,
                _ => {}
            },
            FocusedWindow::SessionList => match key {
                KeyCode::Up => {
                    if let Some(selected) = self.session_list_state.selected() {
                        if selected > 0 {
                            self.session_list_state.select(Some(selected - 1));
                        }
                    }
                }
                KeyCode::Down => {
                    if let Some(selected) = self.session_list_state.selected() {
                        if selected < self.sessions.len() - 1 {
                            self.session_list_state.select(Some(selected + 1));
                        }
                    }
                }
                KeyCode::Tab => self.focused = FocusedWindow::AppList,
                _ => {}
            },
            FocusedWindow::AppList => match key {
                KeyCode::Up => {
                    if let Some(selected) = self.app_list_state.selected() {
                        if selected > 0 {
                            self.app_list_state.select(Some(selected - 1));
                        }
                    }
                }
                KeyCode::Down => {
                    if let Some(selected) = self.app_list_state.selected() {
                        // We have 1 extra item (All Tools) + apps
                        if selected < self.apps.len() {
                            self.app_list_state.select(Some(selected + 1));
                        }
                    }
                }
                KeyCode::Tab => self.focused = FocusedWindow::Settings,
                _ => {}
            },
            FocusedWindow::Settings => match key {
                KeyCode::Up => {
                    if let Some(selected) = self.settings_list_state.selected() {
                         let new_selected = if selected == 0 {
                             2 // Loop to last item
                         } else {
                             selected - 1
                         };
                         self.settings_list_state.select(Some(new_selected));
                    }
                }
                KeyCode::Down => {
                    if let Some(selected) = self.settings_list_state.selected() {
                        let new_selected = if selected >= 2 {
                            0 // Loop to first item
                        } else {
                            selected + 1
                        };
                        self.settings_list_state.select(Some(new_selected));
                    }
                }
                KeyCode::Char(' ') | KeyCode::Enter => {
                     if let Some(selected) = self.settings_list_state.selected() {
                         match selected {
                             0 => self.target_type = TargetType::Prompt,
                             1 => self.target_type = TargetType::Command,
                             2 => self.ultrathink = !self.ultrathink,
                             _ => {}
                         }
                     }
                }
                KeyCode::Tab => self.focused = FocusedWindow::Input,
                _ => {}
            }
        }
    }

    fn create_send_action(&self) -> Option<SendAction> {
        if let Some(session_idx) = self.session_list_state.selected() {
            if let Some(list_idx) = self.app_list_state.selected() {
                let app_index = if list_idx == 0 {
                    None
                } else {
                    Some(list_idx - 1)
                };

                return Some(SendAction {
                    session_name: self.sessions[session_idx].clone(),
                    app_index,
                    target_type: self.target_type,
                    text: self.input.clone(),
                    ultrathink: self.ultrathink,
                    apps: self.apps.clone(),
                });
            }
        }
        None
    }

    // Handling mouse clicks (simplified)
    fn on_click(&mut self, column: u16, row: u16, rects: &LayoutRects) {
        let position = Position { x: column, y: row };
        if rects.input.contains(position) {
            self.focused = FocusedWindow::Input;
        } else if rects.sessions.contains(position) {
            self.focused = FocusedWindow::SessionList;
            // Logic to select session based on row could be added here
        } else if rects.apps.contains(position) {
            self.focused = FocusedWindow::AppList;
        } else if rects.settings.contains(position) {
            self.focused = FocusedWindow::Settings;
        }
    }
}

struct LayoutRects {
    input: Rect,
    sessions: Rect,
    apps: Rect,
    settings: Rect,
}

impl LayoutRects {
    // Helper not needed as we use Rect::contains directly
}

// Removed RectExt impl

pub fn run_send_command(project_config: ProjectConfig, project_name: String) -> Result<()> {
    // 1. Find active sessions matching the project
    let sessions = find_active_sessions(&project_name)?;
    if sessions.is_empty() {
        return Err(MultiAiError::Tmux("No active sessions found for this project".to_string()));
    }

    // 2. Setup terminal
    enable_raw_mode().map_err(|e| MultiAiError::CommandFailed(format!("Failed to enable raw mode: {}", e)))?;
    let mut stdout = io::stdout();

    // Enable keyboard enhancement protocol for proper Shift+Enter detection
    // This allows terminals to send modifier information with special keys
    let mut keyboard_enhancement_enabled = false;
    if matches!(ratatui::crossterm::terminal::supports_keyboard_enhancement(), Ok(true)) {
        queue!(
            stdout,
            PushKeyboardEnhancementFlags(
                KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
                | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
            )
        ).map(|_| keyboard_enhancement_enabled = true)
        .map_err(|e| MultiAiError::CommandFailed(format!("Failed to enable keyboard enhancement: {}", e)))?;
    }

    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)
        .map_err(|e| MultiAiError::CommandFailed(format!("Failed to setup terminal: {}", e)))?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)
        .map_err(|e| MultiAiError::CommandFailed(format!("Failed to create terminal: {}", e)))?;

    // 3. Create state
    let mut state = TuiState::new(sessions, project_config.ai_apps.clone());

    // 4. Run loop (sends are executed inside the loop now)
    let result = run_app(&mut terminal, &mut state);

    // 5. Restore terminal
    disable_raw_mode().map_err(|_| MultiAiError::CommandFailed("Failed to disable raw mode".to_string()))?;

    if keyboard_enhancement_enabled {
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture,
            PopKeyboardEnhancementFlags
        ).map_err(|_| MultiAiError::CommandFailed("Failed to restore terminal".to_string()))?;
    } else {
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture,
        ).map_err(|_| MultiAiError::CommandFailed("Failed to restore terminal".to_string()))?;
    }
    terminal.show_cursor().map_err(|_| MultiAiError::CommandFailed("Failed to show cursor".to_string()))?;

    // 6. Handle any errors from the TUI loop
    result
}

struct SendAction {
    session_name: String,
    app_index: Option<usize>, // None means All
    target_type: TargetType,
    text: String,
    ultrathink: bool,
    apps: Vec<AiApp>,
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, state: &mut TuiState) -> Result<()> {
    loop {
        let _layout_rects = terminal.draw(|f| ui(f, state))
            .map_err(|e| MultiAiError::CommandFailed(format!("Failed to draw TUI: {}", e)))?
            .clone(); // Recalculate on click instead

        // Handle events
        if event::poll(std::time::Duration::from_millis(100)).map_err(|e| MultiAiError::CommandFailed(format!("Poll error: {}", e)))? {
            match event::read().map_err(|e| MultiAiError::CommandFailed(format!("Read error: {}", e)))? {
                Event::Key(key) => {
                    // Only process key press events, not release (for cross-platform consistency)
                    use ratatui::crossterm::event::KeyEventKind;
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }

                    // Ctrl+C handling
                    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                        if state.focused == FocusedWindow::Input && !state.input.is_empty() {
                            if state.confirm_clear {
                                state.input.clear();
                                state.cursor_position = 0;
                                state.confirm_clear = false;
                            } else {
                                state.confirm_clear = true;
                            }
                            // Consume the event (don't quit)
                            continue;
                        } else {
                            return Ok(());
                        }
                    }

                    // Reset confirm_clear if any other key is pressed
                    if state.confirm_clear {
                        state.confirm_clear = false;
                    }
                    
                    if state.focused != FocusedWindow::Input {
                         if key.code == KeyCode::Char('q') {
                             return Ok(());
                         }
                    }

                    // Handle Shift+Enter (and common fallbacks) as newline insertion before send logic
                    let is_newline = state.focused == FocusedWindow::Input && match key.code {
                        KeyCode::Enter => key.modifiers.contains(KeyModifiers::SHIFT) || key.modifiers.contains(KeyModifiers::ALT),
                        KeyCode::Char('\n') | KeyCode::Char('\r') => true,
                        // Ctrl+J is a common terminal newline fallback
                        KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => true,
                        _ => false,
                    };
                    if is_newline {
                        state.insert_newline();
                        continue;
                    }

                    // Handle Enter key for sending (both plain Enter and Ctrl+Enter)
                    // NOTE: Keyboard enhancement protocol allows Shift+Enter to be detected.
                    // When enabled, Shift+Enter will have the SHIFT modifier and insert newline.
                    if key.code == KeyCode::Enter && state.focused == FocusedWindow::Input {
                        // Plain Enter or Ctrl+Enter sends the message
                        // Shift+Enter is handled by on_key() to insert newline
                        if !key.modifiers.contains(KeyModifiers::SHIFT) {
                            if let Some(action) = state.create_send_action() {
                                // Execute send immediately without exiting TUI
                                if let Err(e) = execute_send_action(action) {
                                    // On error, continue running TUI (user can try again)
                                    eprintln!("Failed to send: {}", e);
                                }
                                // Text stays in input field, TUI stays open for more messages
                            }
                            // Don't pass Enter to on_key() to avoid inserting newline
                            continue;
                        }
                    }

                    state.on_key(key.code, key.modifiers);
                }
                Event::Mouse(mouse) => {
                    if mouse.kind == event::MouseEventKind::Down(event::MouseButton::Left) {
                        let size = terminal.size()
                            .map(|s| Rect::from((Position::default(), s)))
                            .unwrap_or(Rect::default());
                        let rects = calculate_layout(size);
                        state.on_click(mouse.column, mouse.row, &rects);
                    }
                }
                _ => {}
            }
        }
    }
}

fn calculate_layout(area: Rect) -> LayoutRects {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(60), // Input takes top 60%
            Constraint::Percentage(40), // Bottom area
        ].as_ref())
        .split(area);

    let input_area = chunks[0];
    let bottom_area = chunks[1];

    // Split bottom area into two columns: Left (Sessions+Apps) and Right (Settings)
    let bottom_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50), // Left column
            Constraint::Percentage(50), // Right column
        ].as_ref())
        .split(bottom_area);

    let left_col = bottom_cols[0];
    let right_col = bottom_cols[1];

    // Split left column into Sessions (top) and Apps (bottom)
    let left_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(50), // Sessions
            Constraint::Percentage(50), // Apps
        ].as_ref())
        .split(left_col);

    LayoutRects {
        input: input_area,
        sessions: left_rows[0],
        apps: left_rows[1],
        settings: right_col,
    }
}

fn ui(f: &mut Frame, state: &mut TuiState) {
    let rects = calculate_layout(f.area());

    // Input Area
    let input_title = if state.confirm_clear {
        " Input (Press Ctrl+C again to clear) "
    } else {
        " Input (Enter to Send, Shift+Enter for newline) "
    };
    
    let input_block = Block::default()
        .borders(Borders::ALL)
        .title(input_title)
        .border_style(if state.focused == FocusedWindow::Input { 
            if state.confirm_clear { Style::default().fg(Color::Red).add_modifier(Modifier::BOLD) }
            else { Style::default().fg(Color::Green).add_modifier(Modifier::BOLD) }
        } else { Style::default() });
    
    // Handle cursor position logic for multiple lines (not implemented in Paragraph directly)
    // For simplicity, we'll stick with basic rendering but we could add a block cursor character
    // at the cursor position if we wanted to be fancy, but terminal usually handles it if we set cursor position.
    
    let input_text = Paragraph::new(state.input.as_str())
        .block(input_block)
        .wrap(Wrap { trim: false });
    f.render_widget(input_text, rects.input);
    
    // Set cursor position
    if state.focused == FocusedWindow::Input {
        // We need to calculate the screen coordinates of the cursor.
        // This is tricky with wrapping.
        // For now, let's assume no wrapping or handle simple cases.
        // A better way is to let the user rely on the blinking block cursor if we can position it correctly.
        // But ratatui doesn't easily give us the layout of the text inside the paragraph.
        
        // Let's try a simple approach: Count newlines up to cursor_position.
        let (cursor_x, cursor_y) = calculate_cursor_pos(&state.input, state.cursor_position, rects.input.width - 2); // -2 for borders
        
        f.set_cursor_position(Position::new(
            rects.input.x + 1 + cursor_x,
            rects.input.y + 1 + cursor_y,
        ));
    }

    // Sessions List
    let sessions_items: Vec<ListItem> = state.sessions
        .iter()
        .map(|s| ListItem::new(Line::from(s.as_str())))
        .collect();
    
    let sessions_list = List::new(sessions_items)
        .block(Block::default().borders(Borders::ALL).title(" Sessions ")
        .border_style(if state.focused == FocusedWindow::SessionList { Style::default().fg(Color::Green).add_modifier(Modifier::BOLD) } else { Style::default() }))
        .highlight_style(Style::default().add_modifier(Modifier::BOLD).fg(Color::Cyan))
        .highlight_symbol(">> ");
    f.render_stateful_widget(sessions_list, rects.sessions, &mut state.session_list_state);

    // Apps List (Target)
    let mut apps_items = vec![
        ListItem::new(Line::from("All Tools")).style(Style::default().add_modifier(Modifier::BOLD))
    ];
    apps_items.extend(state.apps
        .iter()
        .map(|a| ListItem::new(Line::from(a.name.as_str()))));

    let apps_list = List::new(apps_items)
        .block(Block::default().borders(Borders::ALL).title(" Target App (Column) ")
        .border_style(if state.focused == FocusedWindow::AppList { Style::default().fg(Color::Green).add_modifier(Modifier::BOLD) } else { Style::default() }))
        .highlight_style(Style::default().add_modifier(Modifier::BOLD).fg(Color::Cyan))
        .highlight_symbol(">> ");
    f.render_stateful_widget(apps_list, rects.apps, &mut state.app_list_state);

    // Settings
    let settings_items = vec![
        ListItem::new(Line::from(vec![
            Span::styled(if state.target_type == TargetType::Prompt { " (•) " } else { " ( ) " }, Style::default().fg(Color::Cyan)),
            Span::raw("Target: Prompt (Top Pane)"),
        ])),
        ListItem::new(Line::from(vec![
            Span::styled(if state.target_type == TargetType::Command { " (•) " } else { " ( ) " }, Style::default().fg(Color::Cyan)),
            Span::raw("Target: Command (Bottom Pane)"),
        ])),
        ListItem::new(Line::from(vec![
            Span::styled(if state.ultrathink { " [x] " } else { " [ ] " }, Style::default().fg(Color::Cyan)),
            Span::raw("Ultrathink"),
        ])),
    ];

    let settings_list = List::new(settings_items)
        .block(Block::default().borders(Borders::ALL).title(" Settings (Space to toggle) ")
        .border_style(if state.focused == FocusedWindow::Settings { Style::default().fg(Color::Green).add_modifier(Modifier::BOLD) } else { Style::default() }))
        .highlight_style(Style::default().add_modifier(Modifier::BOLD).bg(Color::DarkGray))
        .highlight_symbol("> ");
    f.render_stateful_widget(settings_list, rects.settings, &mut state.settings_list_state);
}

fn find_active_sessions(project_name: &str) -> Result<Vec<String>> {
    let output = Command::new("tmux")
        .args(["list-sessions", "-F", "#{session_name}"])
        .output()
        .map_err(|e| MultiAiError::CommandFailed(format!("Failed to list sessions: {}", e)))?;

    if !output.status.success() {
        // It's possible no sessions exist
        return Ok(vec![]);
    }

    let output_str = String::from_utf8_lossy(&output.stdout);
    let all_sessions: Vec<String> = output_str
        .lines()
        .map(|s| s.to_string())
        .collect();

    // First try to find sessions starting with project_name
    let matched_sessions: Vec<String> = all_sessions
        .iter()
        .filter(|s| s.starts_with(project_name))
        .cloned()
        .collect();

    if !matched_sessions.is_empty() {
        Ok(matched_sessions)
    } else {
        // If no matches found, return all sessions to let user choose
        Ok(all_sessions)
    }
}

fn calculate_cursor_pos(input: &str, cursor_idx: usize, max_width: u16) -> (u16, u16) {
    if input.is_empty() {
        return (0, 0);
    }

    let mut x = 0;
    let mut y = 0;
    
    for (i, c) in input.char_indices() {
        if i == cursor_idx {
            break;
        }
        if c == '\n' {
            x = 0;
            y += 1;
        } else {
            x += 1;
            if x >= max_width {
                x = 0;
                y += 1;
            }
        }
    }
    
    (x, y)
}

fn execute_send_action(action: SendAction) -> Result<()> {
    let window = "apps"; // Assuming standard single window layout
    
    let panes = get_panes(&action.session_name, window)?;
    
    if panes.is_empty() {
         return Err(MultiAiError::Tmux("No panes found in session".to_string()));
    }
    
    // Re-sort purely by x first to identify columns.
    let mut x_sorted = panes.clone();
    x_sorted.sort_by_key(|p| p.x);
    
    // Determine column starts
    let mut unique_xs = Vec::new();
    if !x_sorted.is_empty() {
        let mut last_x = x_sorted[0].x;
        unique_xs.push(last_x);
        for p in &x_sorted {
            if (p.x as i32 - last_x as i32).abs() > 5 {
                last_x = p.x;
                unique_xs.push(last_x);
            }
        }
    }
    
    // For each column (unique X), get panes and sort by Y.
    let mut column_panes_map: Vec<Vec<TmuxPane>> = Vec::new();
    
    for &x in &unique_xs {
        let mut col_panes: Vec<TmuxPane> = panes.iter()
            .filter(|p| (p.x as i32 - x as i32).abs() <= 5)
            .cloned()
            .collect();
        col_panes.sort_by_key(|p| p.y);
        column_panes_map.push(col_panes);
    }
    
    // Determine which columns to target
    let target_indices: Vec<usize> = match action.app_index {
        Some(idx) => vec![idx],
        None => (0..column_panes_map.len()).collect(),
    };

    for &app_idx in &target_indices {
        if app_idx >= column_panes_map.len() {
            continue; // Should we warn?
        }
        
        let target_column = &column_panes_map[app_idx];
        
        let target_pane_index = match action.target_type {
            TargetType::Prompt => 0,
            TargetType::Command => 1,
        };
        
        if target_pane_index >= target_column.len() {
            continue; // Warn?
        }
        
        let target_pane = &target_column[target_pane_index];
        
        let mut final_text = action.text.clone();
        
        // Apply ultrathink if needed
        if action.ultrathink && action.target_type == TargetType::Prompt {
             // We need to get the app corresponding to this column.
             // Assuming apps order matches column order.
             if app_idx < action.apps.len() {
                 if let Some(ultra) = action.apps[app_idx].ultrathink() {
                     final_text.push_str("\n\n");
                     final_text.push_str(ultra);
                 }
             }
        }
        
        // Send Keys
        let output = Command::new("tmux")
            .args([
                "send-keys",
                "-t",
                &target_pane.id,
                &final_text,
                "Enter",
            ])
            .output()
            .map_err(|e| MultiAiError::CommandFailed(format!("Failed to send keys: {}", e)))?;
            
        if !output.status.success() {
             eprintln!("Failed to send keys to pane {}", target_pane.id);
        }
    }

    Ok(())
}

#[derive(Clone, Debug)]
struct TmuxPane {
    id: String,
    x: usize,
    y: usize,
}

fn get_panes(session: &str, window: &str) -> Result<Vec<TmuxPane>> {
    let output = Command::new("tmux")
        .args([
            "list-panes",
            "-t",
            &format!("{}:{}", session, window),
            "-F",
            "#{pane_id} #{pane_left} #{pane_top}",
        ])
        .output()
        .map_err(|e| MultiAiError::CommandFailed(format!("Failed to list panes: {}", e)))?;

    if !output.status.success() {
        return Err(MultiAiError::Tmux(format!("Failed to list panes for {}:{}", session, window)));
    }
    
    let output_str = String::from_utf8_lossy(&output.stdout);
    let mut panes = Vec::new();
    
    for line in output_str.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() == 3 {
            panes.push(TmuxPane {
                id: parts[0].to_string(),
                x: parts[1].parse().unwrap_or(0),
                y: parts[2].parse().unwrap_or(0),
            });
        }
    }
    
    Ok(panes)
}
