use crate::config::AiApp;
use crate::error::Result;
use crate::init;
use ratatui::crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame, Terminal,
};
use std::io;
use std::time::Duration;

pub struct PickerResult {
    pub env_name: String,
    pub selected_apps: Vec<AiApp>,
}

#[derive(Clone, Copy, PartialEq)]
enum Section {
    EnvName,
    AppList,
}

struct PickerState {
    focus: Section,
    env_name: String,
    apps: Vec<AiApp>,
    selected: Vec<bool>,
    focused: usize,
    cancelled: bool,
    confirmed: bool,
}

pub fn run_app_picker(prefill_env_name: Option<&str>) -> Result<Option<PickerResult>> {
    let apps = init::load_apps().unwrap_or_default();
    let selected: Vec<bool> = apps.iter().map(|a| a.default).collect();

    let has_prefill = prefill_env_name.is_some();
    let mut state = PickerState {
        focus: if has_prefill {
            Section::AppList
        } else {
            Section::EnvName
        },
        env_name: prefill_env_name.unwrap_or_default().to_string(),
        apps,
        selected,
        focused: 0,
        cancelled: false,
        confirmed: false,
    };

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    while !state.cancelled && !state.confirmed {
        terminal.draw(|f| render(f, &state))?;
        handle_input(&mut state)?;
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    if state.cancelled {
        return Ok(None);
    }

    let selected_apps: Vec<AiApp> = state
        .apps
        .iter()
        .enumerate()
        .filter_map(|(i, app)| {
            if state.selected[i] {
                Some(app.clone())
            } else {
                None
            }
        })
        .collect();

    Ok(Some(PickerResult {
        env_name: state.env_name,
        selected_apps,
    }))
}

fn handle_input(state: &mut PickerState) -> Result<()> {
    if !event::poll(Duration::from_millis(16))? {
        return Ok(());
    }
    let Event::Key(key) = event::read()? else {
        return Ok(());
    };

    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        state.cancelled = true;
        return Ok(());
    }

    // Tab: switch sections
    if key.code == KeyCode::Tab {
        state.focus = match state.focus {
            Section::EnvName => Section::AppList,
            Section::AppList => Section::EnvName,
        };
        return Ok(());
    }

    // Esc: cancel
    if key.code == KeyCode::Esc {
        state.cancelled = true;
        return Ok(());
    }

    // Enter: confirm (from app list, if env name is non-empty and at least one app selected)
    if key.code == KeyCode::Enter && state.focus == Section::AppList {
        let name_ok = !state.env_name.trim().is_empty();
        let any_selected = state.selected.iter().any(|&s| s);
        if name_ok && any_selected {
            state.confirmed = true;
        }
        return Ok(());
    }

    // Enter in env name: move to app list
    if key.code == KeyCode::Enter && state.focus == Section::EnvName {
        state.focus = Section::AppList;
        return Ok(());
    }

    match state.focus {
        Section::EnvName => match key.code {
            KeyCode::Char(c) => state.env_name.push(c),
            KeyCode::Backspace => {
                state.env_name.pop();
            }
            _ => {}
        },
        Section::AppList => match key.code {
            KeyCode::Up => state.focused = state.focused.saturating_sub(1),
            KeyCode::Down => {
                if state.focused < state.apps.len().saturating_sub(1) {
                    state.focused += 1;
                }
            }
            KeyCode::Char(' ') => {
                if state.focused < state.selected.len() {
                    state.selected[state.focused] = !state.selected[state.focused];
                }
            }
            KeyCode::Char('q') => state.cancelled = true,
            _ => {}
        },
    }

    Ok(())
}

fn render(f: &mut Frame, state: &PickerState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Length(3), // Env name input
            Constraint::Min(0),   // App list
            Constraint::Length(3), // Footer
        ])
        .split(f.area());

    // Header
    let header = Paragraph::new(" New Environment ")
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(header, chunks[0]);

    // Env name input
    let name_border = if state.focus == Section::EnvName {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let name_input = Paragraph::new(state.env_name.as_str()).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(name_border)
            .title(" Environment Name "),
    );
    f.render_widget(name_input, chunks[1]);

    // Show cursor when name input is focused
    if state.focus == Section::EnvName {
        let inner = Block::default().borders(Borders::ALL).inner(chunks[1]);
        let cursor_x = inner.x + state.env_name.chars().count() as u16;
        let cursor_y = inner.y;
        if cursor_x < inner.x + inner.width {
            f.set_cursor_position((cursor_x, cursor_y));
        }
    }

    // App list
    let app_border = if state.focus == Section::AppList {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let items: Vec<ListItem> = state
        .apps
        .iter()
        .enumerate()
        .map(|(i, app)| {
            let checkbox = if state.selected[i] { "[x]" } else { "[ ]" };
            ListItem::new(format!(" {} {}", checkbox, app.command))
        })
        .collect();

    let highlight = if state.focus == Section::AppList {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Gray)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(app_border)
                .title(" Select AI Tools ")
                .title_bottom(" Space: toggle "),
        )
        .highlight_style(highlight);

    let mut list_state = ListState::default().with_selected(Some(state.focused));
    f.render_stateful_widget(list, chunks[2], &mut list_state);

    // Footer
    let hints = match state.focus {
        Section::EnvName => "Type env name | Enter/Tab: next | Esc: cancel | Ctrl+C: quit",
        Section::AppList => {
            "↑/↓: navigate | Space: toggle | Tab: edit name | Enter: create | Esc: cancel"
        }
    };

    let footer = Paragraph::new(hints)
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(footer, chunks[3]);
}

// --- Prefix picker for interactive remove ---

struct PrefixPickerState {
    /// (prefix, worktree_dir_names)
    groups: Vec<(String, Vec<String>)>,
    selected: Vec<bool>,
    focused: usize,
    cancelled: bool,
    confirmed: bool,
}

/// Shows an interactive multi-select list of worktree prefix groups.
/// Each entry shows the prefix and its worktree directories.
/// Returns the selected prefix names.
pub fn run_prefix_picker(groups: Vec<(String, Vec<String>)>) -> Result<Option<Vec<String>>> {
    let count = groups.len();
    let mut state = PrefixPickerState {
        groups,
        selected: vec![false; count],
        focused: 0,
        cancelled: false,
        confirmed: false,
    };

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    while !state.cancelled && !state.confirmed {
        terminal.draw(|f| render_prefix_picker(f, &state))?;
        handle_prefix_input(&mut state)?;
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    if state.cancelled {
        return Ok(None);
    }

    let selected: Vec<String> = state
        .groups
        .iter()
        .enumerate()
        .filter_map(|(i, (prefix, _))| {
            if state.selected[i] {
                Some(prefix.clone())
            } else {
                None
            }
        })
        .collect();

    Ok(Some(selected))
}

fn handle_prefix_input(state: &mut PrefixPickerState) -> Result<()> {
    if !event::poll(Duration::from_millis(16))? {
        return Ok(());
    }
    let Event::Key(key) = event::read()? else {
        return Ok(());
    };

    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        state.cancelled = true;
        return Ok(());
    }

    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => state.cancelled = true,
        KeyCode::Up => state.focused = state.focused.saturating_sub(1),
        KeyCode::Down => {
            if state.focused < state.groups.len().saturating_sub(1) {
                state.focused += 1;
            }
        }
        KeyCode::Char(' ') => {
            if state.focused < state.selected.len() {
                state.selected[state.focused] = !state.selected[state.focused];
            }
        }
        KeyCode::Char('a') => {
            let all_selected = state.selected.iter().all(|&s| s);
            for s in &mut state.selected {
                *s = !all_selected;
            }
        }
        KeyCode::Enter => {
            if state.selected.iter().any(|&s| s) {
                state.confirmed = true;
            }
        }
        _ => {}
    }

    Ok(())
}

fn render_prefix_picker(f: &mut Frame, state: &PrefixPickerState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(0),   // List
            Constraint::Length(3), // Footer
        ])
        .split(f.area());

    // Header
    let header = Paragraph::new(" Remove Worktrees ")
        .style(
            Style::default()
                .fg(Color::Red)
                .add_modifier(Modifier::BOLD),
        )
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(header, chunks[0]);

    // List — each item shows prefix + worktree names
    use ratatui::text::{Line, Span};

    let items: Vec<ListItem> = state
        .groups
        .iter()
        .enumerate()
        .map(|(i, (prefix, worktrees))| {
            let checkbox = if state.selected[i] { "[x]" } else { "[ ]" };
            let header_line = Line::from(vec![
                Span::raw(format!(" {} ", checkbox)),
                Span::styled(
                    prefix.clone(),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("  ({} worktrees)", worktrees.len()),
                    Style::default().fg(Color::DarkGray),
                ),
            ]);
            let detail_line = Line::from(Span::styled(
                format!("     {}", worktrees.join(", ")),
                Style::default().fg(Color::DarkGray),
            ));
            ListItem::new(vec![header_line, detail_line])
        })
        .collect();

    let highlight = Style::default()
        .fg(Color::Black)
        .bg(Color::Gray)
        .add_modifier(Modifier::BOLD);

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(" Select environments to remove ")
                .title_bottom(" Space: toggle | a: all "),
        )
        .highlight_style(highlight);

    let mut list_state = ListState::default().with_selected(Some(state.focused));
    f.render_stateful_widget(list, chunks[1], &mut list_state);

    // Footer
    let footer = Paragraph::new(
        "↑/↓: navigate | Space: toggle | a: select all | Enter: remove | Esc: cancel",
    )
    .style(Style::default().fg(Color::DarkGray))
    .alignment(Alignment::Center)
    .block(Block::default().borders(Borders::ALL));
    f.render_widget(footer, chunks[2]);
}
