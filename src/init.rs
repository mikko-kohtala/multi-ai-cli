use crate::config::{AiApp, Mode, ProjectConfig};
use crate::error::Result;
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

#[derive(Debug, Clone)]
struct CommandVariant {
    command: &'static str,
    description: &'static str,
    is_default: bool,
}

#[derive(Debug, Clone)]
struct AiService {
    name: &'static str,
    display_name: &'static str,
    variants: &'static [CommandVariant],
}

impl AiService {
    const SERVICES: &'static [AiService] = &[
        AiService {
            name: "claude",
            display_name: "Claude Code",
            variants: &[
                CommandVariant {
                    command: "claude",
                    description: "Standard mode - asks for permission on all actions",
                    is_default: true,
                },
                CommandVariant {
                    command: "claude --dangerously-skip-permissions",
                    description: "Skip all permissions - use with care, no safety checks",
                    is_default: false,
                },
                CommandVariant {
                    command: "claude --permission-mode plan --allow-dangerously-skip-permissions",
                    description: "Plan mode with skip option - review first, skip if needed",
                    is_default: false,
                },
            ],
        },
        AiService {
            name: "gemini",
            display_name: "Gemini CLI",
            variants: &[
                CommandVariant {
                    command: "gemini",
                    description: "Standard mode - asks for confirmation on actions",
                    is_default: true,
                },
                CommandVariant {
                    command: "gemini --yolo",
                    description: "Auto-execute all actions - no confirmation prompts",
                    is_default: false,
                },
            ],
        },
        AiService {
            name: "codex",
            display_name: "Codex CLI",
            variants: &[
                CommandVariant {
                    command: "codex",
                    description: "Standard mode - asks for approval on each action",
                    is_default: true,
                },
                CommandVariant {
                    command: "codex --yolo",
                    description: "Auto-approve all actions - never asks for approval",
                    is_default: false,
                },
                CommandVariant {
                    command: "codex --yolo --model gpt-5.1-codex-max --config model_reasoning_effort='high'",
                    description: "YOLO + max model - highest reasoning with auto-approval",
                    is_default: false,
                },
                CommandVariant {
                    command: "codex --yolo --model gpt-5.1 --config model_reasoning_effort='high'",
                    description: "YOLO + base model - high reasoning with auto-approval",
                    is_default: false,
                },
                CommandVariant {
                    command: "codex --yolo --model gpt-5.1-codex --config model_reasoning_effort='high'",
                    description: "YOLO + codex model - high reasoning with auto-approval",
                    is_default: false,
                },
            ],
        },
        AiService {
            name: "amp",
            display_name: "Amp CLI",
            variants: &[
                CommandVariant {
                    command: "amp",
                    description: "Standard mode - requests permission before changes",
                    is_default: true,
                },
                CommandVariant {
                    command: "amp --dangerously-allow-all",
                    description: "Allow all operations - dangerous, bypasses safety",
                    is_default: false,
                },
            ],
        },
        AiService {
            name: "opencode",
            display_name: "OpenCode CLI",
            variants: &[
                CommandVariant {
                    command: "opencode",
                    description: "Standard mode - interactive approval workflow",
                    is_default: true,
                },
                CommandVariant {
                    command: "opencode --auto-approve",
                    description: "Auto-approve mode - automatic execution (if supported)",
                    is_default: false,
                },
            ],
        },
        AiService {
            name: "cursor-agent",
            display_name: "Cursor CLI",
            variants: &[
                CommandVariant {
                    command: "cursor-agent",
                    description: "Standard mode - asks before executing actions",
                    is_default: true,
                },
                CommandVariant {
                    command: "cursor-agent --force",
                    description: "Force mode - executes without confirmation",
                    is_default: false,
                },
            ],
        },
        AiService {
            name: "copilot",
            display_name: "GitHub Copilot CLI",
            variants: &[
                CommandVariant {
                    command: "copilot",
                    description: "Standard mode - interactive GitHub Copilot CLI",
                    is_default: true,
                },
                CommandVariant {
                    command: "copilot --allow-all-tools",
                    description: "YOLO mode - auto-approve all tool calls",
                    is_default: false,
                },
            ],
        },
        AiService {
            name: "kilo",
            display_name: "Kilo Code CLI",
            variants: &[
                CommandVariant {
                    command: "kilo",
                    description: "Standard mode - Kilo Code CLI interface",
                    is_default: true,
                },
            ],
        },
        AiService {
            name: "cline",
            display_name: "Cline CLI",
            variants: &[
                CommandVariant {
                    command: "cline",
                    description: "Standard mode - Cline CLI interface",
                    is_default: true,
                },
            ],
        },
        AiService {
            name: "droid",
            display_name: "Factory CLI",
            variants: &[
                CommandVariant {
                    command: "droid",
                    description: "Standard mode - Factory CLI (droid) interface",
                    is_default: true,
                },
            ],
        },
    ];
}

#[derive(Clone)]
enum WizardStep {
    SelectServices {
        selected: Vec<bool>,
        focused: usize,
    },
    ConfigureCommand {
        service_idx: usize,
        selected_variant: usize,
    },
    SelectMode {
        selected: usize,
    },
    Review,
}

struct WizardState {
    current_step: WizardStep,
    history: Vec<WizardStep>,
    selected_services: Vec<usize>,
    service_commands: Vec<String>,
    terminal_mode: Mode,
    app_state: AppState,
}

#[derive(PartialEq)]
enum AppState {
    Running,
    Completed,
    Cancelled,
}

impl WizardState {
    fn new() -> Self {
        Self {
            current_step: WizardStep::SelectServices {
                selected: vec![false; AiService::SERVICES.len()],
                focused: 0,
            },
            history: Vec::new(),
            selected_services: Vec::new(),
            service_commands: Vec::new(),
            terminal_mode: Mode::default_for_platform(),
            app_state: AppState::Running,
        }
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
            WizardStep::SelectServices { .. } => (1, 4),
            WizardStep::ConfigureCommand { .. } => (2, 4),
            WizardStep::SelectMode { .. } => (3, 4),
            WizardStep::Review => (4, 4),
        }
    }

    fn get_config(&self) -> ProjectConfig {
        let ai_apps: Vec<AiApp> = self
            .selected_services
            .iter()
            .zip(self.service_commands.iter())
            .map(|(&idx, cmd)| AiApp {
                name: AiService::SERVICES[idx].name.to_string(),
                command: cmd.clone(),
                ultrathink: None,
            })
            .collect();

        ProjectConfig {
            ai_apps,
            terminals_per_column: 2,
            mode: Some(self.terminal_mode.clone()),
        }
    }
}

fn get_default_variant_index(service: &AiService) -> usize {
    service
        .variants
        .iter()
        .position(|v| v.is_default)
        .unwrap_or(0)
}

pub fn run_init() -> Result<()> {
    let mut terminal = setup_terminal()?;
    let mut wizard = WizardState::new();

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
    if event::poll(Duration::from_millis(16))? {
        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Esc | KeyCode::Left => {
                    wizard.back();
                }
                KeyCode::Enter | KeyCode::Right => validate_and_next(wizard),
                KeyCode::Up => handle_up(wizard),
                KeyCode::Down => handle_down(wizard),
                KeyCode::Char(' ') => handle_space(wizard),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    wizard.app_state = AppState::Cancelled;
                }
                KeyCode::Char('q') => {
                    wizard.app_state = AppState::Cancelled;
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn handle_up(wizard: &mut WizardState) {
    match &mut wizard.current_step {
        WizardStep::SelectServices { focused, .. } => {
            *focused = focused.saturating_sub(1);
        }
        WizardStep::ConfigureCommand {
            service_idx,
            selected_variant,
        } => {
            let service_idx = wizard.selected_services[*service_idx];
            let service = &AiService::SERVICES[service_idx];
            let max = service.variants.len() - 1;
            *selected_variant = if *selected_variant == 0 {
                max
            } else {
                *selected_variant - 1
            };
        }
        WizardStep::SelectMode { selected } => {
            let max = get_mode_options().len() - 1;
            *selected = if *selected == 0 { max } else { *selected - 1 };
        }
        _ => {}
    }
}

fn handle_down(wizard: &mut WizardState) {
    match &mut wizard.current_step {
        WizardStep::SelectServices { focused, selected } => {
            if *focused < selected.len() - 1 {
                *focused += 1;
            }
        }
        WizardStep::ConfigureCommand {
            service_idx,
            selected_variant,
        } => {
            let service_idx = wizard.selected_services[*service_idx];
            let service = &AiService::SERVICES[service_idx];
            let max = service.variants.len() - 1;
            *selected_variant = (*selected_variant + 1) % (max + 1);
        }
        WizardStep::SelectMode { selected } => {
            let max = get_mode_options().len() - 1;
            *selected = (*selected + 1) % (max + 1);
        }
        _ => {}
    }
}

fn handle_space(wizard: &mut WizardState) {
    if let WizardStep::SelectServices { selected, focused } = &mut wizard.current_step {
        selected[*focused] = !selected[*focused];
    }
}

fn validate_and_next(wizard: &mut WizardState) {
    match &wizard.current_step {
        WizardStep::SelectServices { selected, .. } => {
            let selected_indices: Vec<usize> = selected
                .iter()
                .enumerate()
                .filter_map(|(i, &sel)| if sel { Some(i) } else { None })
                .collect();

            if selected_indices.is_empty() {
                // Show error - for now just do nothing
                return;
            }

            wizard.selected_services = selected_indices;
            wizard.service_commands = Vec::new();

            // Move to first service command configuration
            let service_idx = wizard.selected_services[0];
            let service = &AiService::SERVICES[service_idx];
            wizard.next(WizardStep::ConfigureCommand {
                service_idx: 0,
                selected_variant: get_default_variant_index(service),
            });
        }
        WizardStep::ConfigureCommand {
            service_idx,
            selected_variant,
        } => {
            let service_idx = wizard.selected_services[*service_idx];
            let service = &AiService::SERVICES[service_idx];
            let command = service.variants[*selected_variant].command;
            wizard.service_commands.push(command.to_string());

            let current_config_idx = wizard.service_commands.len();
            if current_config_idx < wizard.selected_services.len() {
                // More services to configure
                let next_service_idx = wizard.selected_services[current_config_idx];
                let next_service = &AiService::SERVICES[next_service_idx];
                wizard.next(WizardStep::ConfigureCommand {
                    service_idx: current_config_idx,
                    selected_variant: get_default_variant_index(next_service),
                });
            } else {
                // All services configured, move to mode selection
                wizard.next(WizardStep::SelectMode {
                    selected: get_default_mode_index(),
                });
            }
        }
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
        WizardStep::SelectServices { selected, focused } => {
            render_multiselect(f, area, selected, *focused);
        }
        WizardStep::ConfigureCommand {
            service_idx,
            selected_variant,
        } => {
            let service_idx = wizard.selected_services[*service_idx];
            render_command_variant(f, area, service_idx, *selected_variant);
        }
        WizardStep::SelectMode { selected } => {
            render_mode_select(f, area, *selected);
        }
        WizardStep::Review => {
            render_review(f, area, wizard);
        }
    }
}

fn render_multiselect(f: &mut Frame, area: Rect, selected: &[bool], focused: usize) {
    let items: Vec<ListItem> = AiService::SERVICES
        .iter()
        .enumerate()
        .map(|(i, service)| {
            let checkbox = if selected[i] { "[✓]" } else { "[ ]" };
            let content = format!(
                " {}  {:<20}  {}",
                checkbox, service.display_name, service.name
            );
            let style = if i == focused {
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
            .title(" Select AI Services (Space: toggle, Enter: continue) "),
    );

    f.render_widget(list, area);
}

fn render_command_variant(f: &mut Frame, area: Rect, service_idx: usize, selected: usize) {
    let service = &AiService::SERVICES[service_idx];
    let title = format!(" Configure {} - Select Command ", service.display_name);

    let items: Vec<ListItem> = service
        .variants
        .iter()
        .enumerate()
        .map(|(i, variant)| {
            let radio = if i == selected { "(•)" } else { "( )" };
            let default_marker = if variant.is_default { " [default]" } else { "" };

            // Two-line format: command + description
            let content = vec![
                Line::from(vec![
                    Span::styled(format!("{} ", radio), Style::default().fg(Color::Cyan)),
                    Span::styled(
                        variant.command,
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(default_marker, Style::default().fg(Color::Yellow)),
                ]),
                Line::from(format!("    {}", variant.description)),
            ];

            let style = if i == selected {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };

            ListItem::new(content).style(style)
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .title_bottom(" ↑/↓: select variant, Enter: confirm "),
    );

    f.render_widget(list, area);
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
            "AI Services:",
            Style::default().fg(Color::Yellow),
        )),
    ];

    for (service_idx, command) in wizard
        .selected_services
        .iter()
        .zip(wizard.service_commands.iter())
    {
        let service = &AiService::SERVICES[*service_idx];
        lines.push(Line::from(format!(
            "  • {}: {}",
            service.display_name, command
        )));
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

    // Add save confirmation prompt
    lines.push(Line::from(""));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Save configuration to multi-ai-config.jsonc?",
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
        WizardStep::SelectServices { .. } => {
            "↑/↓: navigate | Space: toggle | Enter/→: next | ESC/←: cancel | Ctrl+C/q: quit"
        }
        WizardStep::ConfigureCommand { .. } => {
            "↑/↓: select variant | Enter/→: next | ESC/←: back | Ctrl+C/q: quit"
        }
        WizardStep::SelectMode { .. } => {
            "↑/↓: select | Enter/→: next | ESC/←: back | Ctrl+C/q: quit"
        }
        WizardStep::Review => "Enter/→: save | ESC/←: back | Ctrl+C/q: quit",
    };

    let footer = Paragraph::new(hints)
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL));

    f.render_widget(footer, area);
}

fn save_config(wizard: &WizardState) -> Result<()> {
    let config = wizard.get_config();
    let config_path = "multi-ai-config.jsonc";

    // Check if file exists
    if fs::metadata(config_path).is_ok() {
        // File exists, ask for confirmation in normal terminal mode
        print!("\n{} already exists. Overwrite? [y/n]: ", config_path);
        io::Write::flush(&mut io::stdout())?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        if !matches!(input.trim().to_lowercase().as_str(), "y" | "yes") {
            println!("Configuration not saved.");
            return Ok(());
        }
    }

    let json_content = format!(
        r#"{{
  // Multi-AI CLI configuration
  // Generated by: mai init
  "terminals_per_column": {},  // Number of terminal panes per column (first is AI command, rest are shells)
  "mode": "{}",               // Required: iterm2 | tmux-single-window | tmux-multi-window
  "ai_apps": [{}
  ]
}}"#,
        config.terminals_per_column,
        match wizard.terminal_mode {
            Mode::Iterm2 => "iterm2",
            Mode::TmuxMultiWindow => "tmux-multi-window",
            Mode::TmuxSingleWindow => "tmux-single-window",
        },
        config
            .ai_apps
            .iter()
            .map(|app| format!(
                r#"
    {{
      "name": "{}",
      "command": "{}"
    }}"#,
                app.name, app.command
            ))
            .collect::<Vec<_>>()
            .join(",")
    );

    fs::write(config_path, json_content)?;
    println!("\n✓ Configuration saved to {}", config_path);
    println!("\nYou can now run:");
    println!("  mai add <branch-prefix>              # Uses mode from config");
    println!("  mai add <branch-prefix> --mode tmux-single-window  # Override for a single run");
    println!("  mai add <branch-prefix> --tmux       # Legacy alias for tmux-multi-window");

    Ok(())
}
