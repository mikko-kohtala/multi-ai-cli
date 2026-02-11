use crate::config::{AiApp, Mode, ProjectConfig};
use crate::error::{MultiAiError, Result};
use std::path::PathBuf;
use ratatui::crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame, Terminal,
};
use std::fs;
use std::io;
use std::time::Duration;

/// The bundled default apps.jsonc content, embedded at compile time.
const EMBEDDED_APPS_JSONC: &str = include_str!("../apps.jsonc");

#[derive(Clone)]
enum WizardStep {
    SelectMode {
        selected: usize,
    },
    Review,
}

struct WizardState {
    current_step: WizardStep,
    history: Vec<WizardStep>,
    terminal_mode: Mode,
    app_state: AppState,
    project_path: PathBuf,
    worktrees_path: Option<PathBuf>,
}

#[derive(PartialEq)]
enum AppState {
    Running,
    Completed,
    Cancelled,
}

impl WizardState {
    fn new(project_path: PathBuf) -> Result<Self> {
        // Auto-detect worktrees_path from gwt config
        let worktrees_path =
            crate::worktree::WorktreeManager::read_worktrees_path_public(&project_path);

        Ok(Self {
            current_step: WizardStep::SelectMode {
                selected: get_default_mode_index(),
            },
            history: Vec::new(),
            terminal_mode: Mode::default_for_platform(),
            app_state: AppState::Running,
            project_path,
            worktrees_path,
        })
    }

    fn next(&mut self, next_step: WizardStep) {
        self.history.push(self.current_step.clone());
        self.current_step = next_step;
    }

    fn back(&mut self) -> bool {
        if let Some(prev) = self.history.pop() {
            self.current_step = prev;
            true
        } else {
            self.app_state = AppState::Cancelled;
            false
        }
    }

    fn step_number(&self) -> (usize, usize) {
        match &self.current_step {
            WizardStep::SelectMode { .. } => (1, 2),
            WizardStep::Review => (2, 2),
        }
    }

    fn get_config(&self) -> ProjectConfig {
        ProjectConfig {
            ai_apps: Vec::new(),
            terminals_per_column: 2,
            mode: Some(self.terminal_mode.clone()),
            project_path: Some(self.project_path.clone()),
            worktrees_path: self.worktrees_path.clone(),
        }
    }
}

pub fn run_init() -> Result<()> {
    let current_dir = std::env::current_dir()
        .map_err(|e| MultiAiError::Config(format!("Failed to get current directory: {}", e)))?;

    let project_path = crate::git::get_repo_root(&current_dir).ok_or_else(|| {
        MultiAiError::Config(
            "Not inside a git repository. Run 'mai init' from within a git repo.".to_string(),
        )
    })?;

    let mut terminal = setup_terminal()?;
    let mut wizard = WizardState::new(project_path)?;

    let result = run_wizard(&mut terminal, &mut wizard);

    cleanup_terminal(&mut terminal)?;

    match result {
        Ok(()) if wizard.app_state == AppState::Completed => {
            save_config(&wizard)?;
            Ok(())
        }
        Ok(()) => {
            println!("Configuration cancelled.");
            Ok(())
        }
        Err(e) => Err(e),
    }
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    Ok(Terminal::new(backend)?)
}

fn cleanup_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}

fn run_wizard(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    wizard: &mut WizardState,
) -> Result<()> {
    while wizard.app_state == AppState::Running {
        terminal.draw(|f| render(f, wizard))?;
        handle_input(wizard)?;
    }
    Ok(())
}

fn handle_input(wizard: &mut WizardState) -> Result<()> {
    if event::poll(Duration::from_millis(16))?
        && let Event::Key(key) = event::read()?
    {
        match key.code {
            KeyCode::Esc | KeyCode::Left => {
                wizard.back();
            }
            KeyCode::Enter | KeyCode::Right => validate_and_next(wizard),
            KeyCode::Up => handle_up(wizard),
            KeyCode::Down => handle_down(wizard),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                wizard.app_state = AppState::Cancelled;
            }
            KeyCode::Char('q') => {
                wizard.app_state = AppState::Cancelled;
            }
            _ => {}
        }
    }
    Ok(())
}

fn handle_up(wizard: &mut WizardState) {
    if let WizardStep::SelectMode { selected } = &mut wizard.current_step {
        let max = get_mode_options().len() - 1;
        *selected = if *selected == 0 { max } else { *selected - 1 };
    }
}

fn handle_down(wizard: &mut WizardState) {
    if let WizardStep::SelectMode { selected } = &mut wizard.current_step {
        let max = get_mode_options().len() - 1;
        *selected = (*selected + 1) % (max + 1);
    }
}

fn validate_and_next(wizard: &mut WizardState) {
    match &wizard.current_step {
        WizardStep::SelectMode { selected } => {
            let modes = get_mode_options();
            wizard.terminal_mode = modes[*selected].clone();
            wizard.next(WizardStep::Review);
        }
        WizardStep::Review => {
            wizard.app_state = AppState::Completed;
        }
    }
}

fn get_mode_options() -> Vec<Mode> {
    #[cfg(target_os = "macos")]
    {
        vec![Mode::Iterm2, Mode::TmuxMultiWindow, Mode::TmuxSingleWindow]
    }
    #[cfg(not(target_os = "macos"))]
    {
        vec![Mode::TmuxMultiWindow, Mode::TmuxSingleWindow]
    }
}

fn get_default_mode_index() -> usize {
    let default_mode = Mode::default_for_platform();
    let modes = get_mode_options();
    modes
        .iter()
        .position(|m| std::mem::discriminant(m) == std::mem::discriminant(&default_mode))
        .unwrap_or(0)
}

fn render(f: &mut Frame, wizard: &WizardState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(0),    // Content
            Constraint::Length(3), // Footer
        ])
        .split(f.area());

    render_header(f, chunks[0], wizard);
    render_content(f, chunks[1], wizard);
    render_footer(f, chunks[2], wizard);
}

fn render_header(f: &mut Frame, area: Rect, wizard: &WizardState) {
    let (current, total) = wizard.step_number();
    let title = format!(" Multi-AI CLI Configuration (Step {}/{}) ", current, total);
    let header = Paragraph::new(title)
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(header, area);
}

fn render_content(f: &mut Frame, area: Rect, wizard: &WizardState) {
    match &wizard.current_step {
        WizardStep::SelectMode { selected } => {
            render_mode_select(f, area, *selected);
        }
        WizardStep::Review => {
            render_review(f, area, wizard);
        }
    }
}

fn render_mode_select(f: &mut Frame, area: Rect, selected: usize) {
    let modes = get_mode_options();
    let mode_labels = modes
        .iter()
        .map(|m| match m {
            Mode::Iterm2 => "iTerm2 (macOS only)",
            Mode::TmuxMultiWindow => "tmux multi-window",
            Mode::TmuxSingleWindow => "tmux single-window",
        })
        .collect::<Vec<_>>();

    let items: Vec<ListItem> = mode_labels
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let checkbox = if i == selected { "[✓]" } else { "[ ]" };
            let content = format!(" {} {}", checkbox, label);
            let style = if i == selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Gray)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(content).style(style)
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Select Terminal Mode ")
            .title_bottom(" ↑/↓: select, Enter: confirm "),
    );

    f.render_widget(list, area);
}

fn render_review(f: &mut Frame, area: Rect, wizard: &WizardState) {
    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "Configuration Summary:",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Project Path:",
            Style::default().fg(Color::Yellow),
        )),
        Line::from(format!("  {}", wizard.project_path.display())),
    ];

    if let Some(ref wt_path) = wizard.worktrees_path {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Worktrees Path:",
            Style::default().fg(Color::Yellow),
        )));
        lines.push(Line::from(format!("  {}", wt_path.display())));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Terminal Mode:",
        Style::default().fg(Color::Yellow),
    )));
    let mode_str = match wizard.terminal_mode {
        Mode::Iterm2 => "iTerm2",
        Mode::TmuxMultiWindow => "tmux multi-window",
        Mode::TmuxSingleWindow => "tmux single-window",
    };
    lines.push(Line::from(format!("  {}", mode_str)));

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "AI tools are configured globally in apps.jsonc (run 'mai apps' to edit)",
        Style::default().fg(Color::DarkGray),
    )));

    // Add save confirmation prompt
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Save configuration?",
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
    )));

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Review & Save Configuration ")
                .title_bottom(" Enter: save, ESC: back "),
        )
        .alignment(Alignment::Left);

    f.render_widget(paragraph, area);
}

fn render_footer(f: &mut Frame, area: Rect, wizard: &WizardState) {
    let hints = match &wizard.current_step {
        WizardStep::SelectMode { .. } => {
            "↑/↓: select | Enter/→: next | ESC/←: cancel | Ctrl+C/q: quit"
        }
        WizardStep::Review => "Enter/→: save | ESC/←: back | Ctrl+C/q: quit",
    };

    let footer = Paragraph::new(hints)
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL));

    f.render_widget(footer, area);
}

pub fn default_apps_content() -> String {
    EMBEDDED_APPS_JSONC.to_string()
}

/// Load AI apps from `~/.config/multi-ai-cli/apps.jsonc`.
/// If the file doesn't exist, falls back to the embedded default.
pub fn load_apps() -> Result<Vec<AiApp>> {
    let config_dir = ProjectConfig::config_dir()
        .map_err(|e| MultiAiError::Config(format!("Could not determine config directory: {}", e)))?;
    let apps_path = config_dir.join("apps.jsonc");

    let content = if apps_path.exists() {
        fs::read_to_string(&apps_path)
            .map_err(|e| MultiAiError::Config(format!("Failed to read apps.jsonc: {}", e)))?
    } else {
        EMBEDDED_APPS_JSONC.to_string()
    };

    let parsed = jsonc_parser::parse_to_serde_value(&content, &Default::default())
        .map_err(|e| MultiAiError::Config(format!("Failed to parse apps.jsonc: {}", e)))?;

    let value = parsed.ok_or_else(|| MultiAiError::Config("apps.jsonc is empty".to_string()))?;

    let apps: Vec<AiApp> = serde_json::from_value(value)
        .map_err(|e| MultiAiError::Config(format!("Invalid apps.jsonc format: {}", e)))?;

    Ok(apps)
}

fn save_config(wizard: &WizardState) -> Result<()> {
    let config = wizard.get_config();

    let config_dir = ProjectConfig::config_dir()
        .map_err(|e| MultiAiError::Config(format!("Could not determine config directory: {}", e)))?;

    fs::create_dir_all(&config_dir)?;

    let repo_url = crate::git::get_remote_origin_url(&wizard.project_path).ok_or_else(|| {
        MultiAiError::Config(
            "Could not determine git remote URL. Make sure you have a remote named 'origin'."
                .to_string(),
        )
    })?;

    let config_filename = format!(
        "{}.jsonc",
        crate::git::generate_config_filename(&repo_url)
    );
    let config_path = config_dir.join(&config_filename);

    // Check if file exists
    if fs::metadata(&config_path).is_ok() {
        print!(
            "\n{} already exists. Overwrite? [y/n]: ",
            config_path.display()
        );
        io::Write::flush(&mut io::stdout())?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        if !matches!(input.trim().to_lowercase().as_str(), "y" | "yes") {
            println!("Configuration not saved.");
            return Ok(());
        }
    }

    let worktrees_line = if let Some(ref wt_path) = wizard.worktrees_path {
        format!(
            "\n  \"worktrees_path\": \"{}\",",
            wt_path.display()
        )
    } else {
        String::new()
    };

    let json_content = format!(
        r#"{{
  // Multi-AI CLI configuration
  // Generated by: mai init
  // AI tools are configured globally — run 'mai apps' to edit
  "project_path": "{}",{}
  "terminals_per_column": {},  // Number of terminal panes per column (first is AI command, rest are shells)
  "mode": "{}"                 // iterm2 | tmux-single-window | tmux-multi-window
}}"#,
        wizard.project_path.display(),
        worktrees_line,
        config.terminals_per_column,
        match wizard.terminal_mode {
            Mode::Iterm2 => "iterm2",
            Mode::TmuxMultiWindow => "tmux-multi-window",
            Mode::TmuxSingleWindow => "tmux-single-window",
        },
    );

    fs::write(&config_path, json_content)?;
    println!("\n✓ Configuration saved to {}", config_path.display());
    println!("  Project path: {}", wizard.project_path.display());
    println!("\nYou can now run:");
    println!("  mai add <branch-prefix>              # Uses mode from config");
    println!("  mai add <branch-prefix> --mode tmux-single-window  # Override for a single run");
    println!("  mai config                           # Open config in editor");

    Ok(())
}
