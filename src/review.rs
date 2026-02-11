use crate::config::AiApp;
use crate::error::{MultiAiError, Result};
use crate::git::{self, BranchInfo};
use crate::init;
use crate::worktree::WorktreeManager;
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
    widgets::{
        Block, Borders, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Wrap,
    },
    Frame, Terminal,
};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Review tag & selected tool (used by prompt-sending logic)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum ReviewTag {
    Ai,
    Meta,
}

#[derive(Clone)]
struct SelectedTool {
    service_index: usize,
    tag: ReviewTag,
}

const DEFAULT_REVIEW_PROMPT: &str =
    "Review changes in this branch against the base branch. Once done with the review, write findings to REVIEW.md";

// ---------------------------------------------------------------------------
// Wizard state
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum ConfigSection {
    Prompt,
    SendPrompts,
    AiReviewers,
    MetaReviewer,
}

impl ConfigSection {
    fn next(self) -> Self {
        match self {
            Self::Prompt => Self::SendPrompts,
            Self::SendPrompts => Self::AiReviewers,
            Self::AiReviewers => Self::MetaReviewer,
            Self::MetaReviewer => Self::Prompt,
        }
    }

    fn prev(self) -> Self {
        match self {
            Self::Prompt => Self::MetaReviewer,
            Self::SendPrompts => Self::Prompt,
            Self::AiReviewers => Self::SendPrompts,
            Self::MetaReviewer => Self::AiReviewers,
        }
    }
}

#[derive(Clone)]
enum ReviewStep {
    SelectBranch {
        branches: Vec<BranchInfo>,
        focused: usize,
        filter: String,
    },
    Configure {
        focus: ConfigSection,
        // Prompt
        prompt_text: String,
        prompt_cursor: usize,
        // Send prompts toggle
        send_prompts: bool,
        // AI reviewers (multi-select checkboxes)
        ai_selected: Vec<bool>,
        ai_focused: usize,
        // Meta reviewers (multi-select checkboxes)
        meta_selected: Vec<bool>,
        meta_focused: usize,
    },
}

#[derive(PartialEq)]
enum AppState {
    Running,
    Completed,
    Cancelled,
}

struct ReviewWizardState {
    current_step: ReviewStep,
    history: Vec<ReviewStep>,
    app_state: AppState,

    // Available tools loaded from apps.jsonc
    review_services: Vec<AiApp>,

    // Populated when user confirms
    source_branch: String,
    /// Full git ref for reset (e.g. "origin/branch" for remote-only branches).
    source_branch_ref: String,
    review_prompt: String,
    send_prompts: bool,
    selected_tools: Vec<SelectedTool>,
}

impl ReviewWizardState {
    fn new(branches: Vec<BranchInfo>, branch: Option<&str>) -> Self {
        let review_services = init::load_apps().unwrap_or_default();

        // If a branch argument was given and matches exactly, skip to Configure
        if let Some(b) = branch {
            if let Some(matched) = branches.iter().find(|bi| bi.name == b) {
                let source_branch = matched.name.clone();
                let source_branch_ref = if matched.remote_only {
                    format!("origin/{}", matched.name)
                } else {
                    matched.name.clone()
                };

                let mut ai_selected: Vec<bool> =
                    review_services.iter().map(|app| app.default).collect();
                if !ai_selected.iter().any(|&s| s) && !ai_selected.is_empty() {
                    ai_selected[0] = true;
                }
                let meta_selected: Vec<bool> =
                    review_services.iter().map(|a| a.meta_review).collect();

                let prompt = DEFAULT_REVIEW_PROMPT.to_string();
                let len = prompt.len();
                return Self {
                    current_step: ReviewStep::Configure {
                        focus: ConfigSection::AiReviewers,
                        prompt_text: prompt,
                        prompt_cursor: len,
                        send_prompts: true,
                        ai_selected,
                        ai_focused: 0,
                        meta_selected,
                        meta_focused: 0,
                    },
                    history: Vec::new(),
                    app_state: AppState::Running,
                    review_services,
                    source_branch,
                    source_branch_ref,
                    review_prompt: DEFAULT_REVIEW_PROMPT.to_string(),
                    send_prompts: true,
                    selected_tools: Vec::new(),
                };
            }
        }

        Self {
            current_step: ReviewStep::SelectBranch {
                branches,
                focused: 0,
                filter: String::new(),
            },
            history: Vec::new(),
            app_state: AppState::Running,
            review_services,
            source_branch: String::new(),
            source_branch_ref: String::new(),
            review_prompt: DEFAULT_REVIEW_PROMPT.to_string(),
            send_prompts: true,
            selected_tools: Vec::new(),
        }
    }

    fn next(&mut self, next_step: ReviewStep) {
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
            ReviewStep::SelectBranch { .. } => (1, 2),
            ReviewStep::Configure { .. } => (2, 2),
        }
    }
}

// ---------------------------------------------------------------------------
// Filtered branch helpers
// ---------------------------------------------------------------------------

fn filtered_branches<'a>(branches: &'a [BranchInfo], filter: &str) -> Vec<(usize, &'a BranchInfo)> {
    if filter.is_empty() {
        branches.iter().enumerate().collect()
    } else {
        let lower = filter.to_lowercase();
        branches
            .iter()
            .enumerate()
            .filter(|(_, b)| b.name.to_lowercase().contains(&lower))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn run_review(
    _project_config: crate::config::ProjectConfig,
    project_name: String,
    project_path: PathBuf,
    worktree_manager: WorktreeManager,
    branch: Option<String>,
) -> Result<()> {
    // 1. Fetch branches (may involve network I/O) before entering TUI
    print!("Fetching branches...");
    io::stdout().flush().ok();
    let branches = git::list_all_branches(&project_path);
    println!(" {} branches found.", branches.len());

    // 2. Run TUI wizard
    let mut terminal = setup_terminal()?;
    let mut wizard = ReviewWizardState::new(branches, branch.as_deref());
    let result = run_wizard(&mut terminal, &mut wizard);
    cleanup_terminal(&mut terminal)?;

    result?;

    if wizard.app_state != AppState::Completed {
        println!("Review cancelled.");
        return Ok(());
    }

    // 2. Generate branch prefix
    let branch_prefix =
        generate_review_prefix(worktree_manager.worktrees_path(), &wizard.source_branch);
    println!("Review prefix: {}", branch_prefix);

    // 3. Build AiApp list
    let review_apps: Vec<AiApp> = wizard
        .selected_tools
        .iter()
        .map(|t| {
            let app = &wizard.review_services[t.service_index];
            if t.tag == ReviewTag::Meta {
                AiApp {
                    name: format!("meta-{}", app.name),
                    command: app.command.clone(),
                    slug: app.slug.as_ref().map(|s| format!("meta-{}", s)),
                    ultrathink: app.ultrathink.clone(),
                    default: false,
                    meta_review: false,
                    description: app.description.clone(),
                }
            } else {
                app.clone()
            }
        })
        .collect();

    // 4. Create worktrees in parallel
    println!("Creating review worktrees...");
    let worktree_paths = create_review_worktrees(
        &worktree_manager,
        &branch_prefix,
        &review_apps,
        &wizard.source_branch_ref,
    )?;
    println!("All review worktrees created.");

    // 5. Build review & meta prompts
    let review_prompt = &wizard.review_prompt;

    // Build review locations for meta prompt
    let mut review_locations = Vec::new();
    for (i, tool) in wizard.selected_tools.iter().enumerate() {
        if tool.tag == ReviewTag::Ai {
            if let Some((_app, path)) = worktree_paths.get(i) {
                let app = &wizard.review_services[tool.service_index];
                review_locations.push(format!("- {}: {}/REVIEW.md", app.name, path));
            }
        }
    }
    let meta_prompt = if !review_locations.is_empty() {
        Some(format!(
            "Your task is to review the code reviews made by other AI tools. \
             You will find the review markdown files from these locations:\n\
             {}\n\n\
             Please wait for the REVIEW.md files to appear, then read all of the reviews, \
             and create a comprehensive summary of the review results. You can investigate the repo and review results if needed. Use sub-agents if needed.\n\n\
             Once you are done, create REVIEW_SUMMARY.md with your consolidated findings.",
            review_locations.join("\n")
        ))
    } else {
        None
    };

    if !wizard.send_prompts {
        println!("Note: AI review prompts will NOT be sent automatically.");
    }

    // 6. Create iTerm2 layout, launch tools, and send prompts via AppleScript
    println!("Creating iTerm2 layout and launching tools...");
    create_iterm2_layout_applescript(
        &wizard,
        &review_apps,
        &worktree_paths,
        review_prompt,
        meta_prompt.as_deref(),
        &branch_prefix,
    )?;

    println!(
        "\nReview session '{}-{}' started in iTerm2.",
        project_name, branch_prefix
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Terminal setup
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Wizard event loop
// ---------------------------------------------------------------------------

fn run_wizard(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    wizard: &mut ReviewWizardState,
) -> Result<()> {
    while wizard.app_state == AppState::Running {
        terminal.draw(|f| render(f, wizard))?;
        handle_input(wizard)?;
    }
    Ok(())
}

fn handle_input(wizard: &mut ReviewWizardState) -> Result<()> {
    if event::poll(Duration::from_millis(16))?
        && let Event::Key(key) = event::read()?
    {
        // Global quit
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            wizard.app_state = AppState::Cancelled;
            return Ok(());
        }

        match &mut wizard.current_step {
            ReviewStep::SelectBranch { .. } => handle_branch_input(wizard, key.code),
            ReviewStep::Configure { .. } => {
                handle_configure_input(wizard, key.code, key.modifiers)
            }
        }
    }
    Ok(())
}

// -- Branch step input --

fn handle_branch_input(wizard: &mut ReviewWizardState, key: KeyCode) {
    match key {
        KeyCode::Esc | KeyCode::Left => {
            wizard.back();
        }
        KeyCode::Enter | KeyCode::Right => {
            if let ReviewStep::SelectBranch {
                branches,
                focused,
                filter,
            } = &wizard.current_step
            {
                let filtered = filtered_branches(branches, filter);
                if let Some((_orig_idx, branch)) = filtered.get(*focused) {
                    wizard.source_branch = branch.name.clone();
                    wizard.source_branch_ref = if branch.remote_only {
                        format!("origin/{}", branch.name)
                    } else {
                        branch.name.clone()
                    };

                    let mut ai_selected: Vec<bool> = wizard
                        .review_services
                        .iter()
                        .map(|app| app.default)
                        .collect();
                    // If no defaults configured, select the first entry
                    if !ai_selected.iter().any(|&s| s) && !ai_selected.is_empty() {
                        ai_selected[0] = true;
                    }

                    // Pre-select entries marked with meta_review in apps.jsonc
                    let meta_selected: Vec<bool> = wizard
                        .review_services
                        .iter()
                        .map(|a| a.meta_review)
                        .collect();

                    let prompt = wizard.review_prompt.clone();
                    let len = prompt.len();
                    wizard.next(ReviewStep::Configure {
                        focus: ConfigSection::AiReviewers,
                        prompt_text: prompt,
                        prompt_cursor: len,
                        send_prompts: wizard.send_prompts,
                        ai_selected,
                        ai_focused: 0,
                        meta_selected,
                        meta_focused: 0,
                    });
                }
            }
        }
        KeyCode::Up => {
            if let ReviewStep::SelectBranch { focused, .. } = &mut wizard.current_step {
                *focused = focused.saturating_sub(1);
            }
        }
        KeyCode::Down => {
            if let ReviewStep::SelectBranch {
                branches,
                focused,
                filter,
            } = &mut wizard.current_step
            {
                let count = filtered_branches(branches, filter).len();
                if count > 0 && *focused < count - 1 {
                    *focused += 1;
                }
            }
        }
        KeyCode::Backspace => {
            if let ReviewStep::SelectBranch {
                filter, focused, ..
            } = &mut wizard.current_step
            {
                filter.pop();
                *focused = 0;
            }
        }
        KeyCode::Char(c) => {
            if let ReviewStep::SelectBranch {
                filter, focused, ..
            } = &mut wizard.current_step
            {
                filter.push(c);
                *focused = 0;
            }
        }
        _ => {}
    }
}

// -- Configure step input --

fn handle_configure_input(wizard: &mut ReviewWizardState, key: KeyCode, modifiers: KeyModifiers) {
    let ReviewStep::Configure {
        focus,
        prompt_text,
        prompt_cursor,
        send_prompts,
        ai_selected,
        ai_focused,
        meta_selected,
        meta_focused,
    } = &mut wizard.current_step
    else {
        return;
    };

    // Tab / Shift+Tab: cycle sections
    if key == KeyCode::Tab {
        if modifiers.contains(KeyModifiers::SHIFT) {
            *focus = focus.prev();
        } else {
            *focus = focus.next();
        }
        return;
    }

    // Esc: back to branch selection
    if key == KeyCode::Esc {
        // Save prompt and toggle state before going back
        wizard.review_prompt = prompt_text.clone();
        wizard.send_prompts = *send_prompts;
        wizard.back();
        return;
    }

    // Enter: start review (from any section except Prompt)
    if key == KeyCode::Enter && *focus != ConfigSection::Prompt {
        let any_ai = ai_selected.iter().any(|&s| s);
        if !any_ai {
            return; // Must have at least one AI tool
        }

        // Populate wizard state for downstream functions
        wizard.review_prompt = prompt_text.clone();
        wizard.send_prompts = *send_prompts;
        wizard.selected_tools = Vec::new();
        for (i, &sel) in ai_selected.iter().enumerate() {
            if sel {
                wizard.selected_tools.push(SelectedTool {
                    service_index: i,
                    tag: ReviewTag::Ai,
                });
            }
        }
        for (i, &sel) in meta_selected.iter().enumerate() {
            if sel {
                wizard.selected_tools.push(SelectedTool {
                    service_index: i,
                    tag: ReviewTag::Meta,
                });
            }
        }

        wizard.app_state = AppState::Completed;
        return;
    }

    // q: quit (only when not in prompt)
    if key == KeyCode::Char('q') && *focus != ConfigSection::Prompt {
        wizard.app_state = AppState::Cancelled;
        return;
    }

    match focus {
        ConfigSection::Prompt => {
            handle_prompt_keys(prompt_text, prompt_cursor, key, modifiers);
        }
        ConfigSection::SendPrompts => {
            if key == KeyCode::Char(' ') {
                *send_prompts = !*send_prompts;
            }
        }
        ConfigSection::AiReviewers => match key {
            KeyCode::Up => *ai_focused = ai_focused.saturating_sub(1),
            KeyCode::Down => {
                if *ai_focused < ai_selected.len().saturating_sub(1) {
                    *ai_focused += 1;
                }
            }
            KeyCode::Char(' ') => {
                if *ai_focused < ai_selected.len() {
                    ai_selected[*ai_focused] = !ai_selected[*ai_focused];
                }
            }
            _ => {}
        },
        ConfigSection::MetaReviewer => match key {
            KeyCode::Up => *meta_focused = meta_focused.saturating_sub(1),
            KeyCode::Down => {
                if *meta_focused < meta_selected.len().saturating_sub(1) {
                    *meta_focused += 1;
                }
            }
            KeyCode::Char(' ') => {
                if *meta_focused < meta_selected.len() {
                    meta_selected[*meta_focused] = !meta_selected[*meta_focused];
                }
            }
            _ => {}
        },
    }
}

fn handle_prompt_keys(
    text: &mut String,
    cursor: &mut usize,
    key: KeyCode,
    modifiers: KeyModifiers,
) {
    match key {
        KeyCode::Enter if modifiers.contains(KeyModifiers::SHIFT) => {
            text.insert(*cursor, '\n');
            *cursor += 1;
        }
        KeyCode::Backspace => {
            if *cursor > 0 {
                let prev = text[..*cursor]
                    .char_indices()
                    .next_back()
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                text.drain(prev..*cursor);
                *cursor = prev;
            }
        }
        KeyCode::Delete => {
            if *cursor < text.len() {
                let next = text[*cursor..]
                    .char_indices()
                    .nth(1)
                    .map(|(i, _)| *cursor + i)
                    .unwrap_or(text.len());
                text.drain(*cursor..next);
            }
        }
        KeyCode::Left => {
            if *cursor > 0 {
                *cursor = text[..*cursor]
                    .char_indices()
                    .next_back()
                    .map(|(i, _)| i)
                    .unwrap_or(0);
            }
        }
        KeyCode::Right => {
            if *cursor < text.len() {
                *cursor = text[*cursor..]
                    .char_indices()
                    .nth(1)
                    .map(|(i, _)| *cursor + i)
                    .unwrap_or(text.len());
            }
        }
        KeyCode::Home => *cursor = 0,
        KeyCode::End => *cursor = text.len(),
        KeyCode::Char(c) => {
            text.insert(*cursor, c);
            *cursor += c.len_utf8();
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn render(f: &mut Frame, wizard: &ReviewWizardState) {
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

fn render_header(f: &mut Frame, area: Rect, wizard: &ReviewWizardState) {
    let (current, total) = wizard.step_number();
    let branch_suffix = if !wizard.source_branch.is_empty() {
        format!(" — {} ", wizard.source_branch)
    } else {
        String::new()
    };
    let title = format!(
        " Multi-AI Code Review (Step {}/{}){} ",
        current, total, branch_suffix
    );
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

fn render_content(f: &mut Frame, area: Rect, wizard: &ReviewWizardState) {
    match &wizard.current_step {
        ReviewStep::SelectBranch {
            branches,
            focused,
            filter,
        } => render_branch_select(f, area, branches, *focused, filter),
        ReviewStep::Configure { .. } => render_configure(f, area, wizard),
    }
}

fn render_branch_select(
    f: &mut Frame,
    area: Rect,
    branches: &[BranchInfo],
    focused: usize,
    filter: &str,
) {
    let filtered = filtered_branches(branches, filter);

    // Find the longest branch name for alignment
    let max_name_len = filtered
        .iter()
        .map(|(_, b)| b.name.len())
        .max()
        .unwrap_or(0)
        .min(50);

    let items: Vec<ListItem> = filtered
        .iter()
        .enumerate()
        .map(|(i, (_orig_idx, branch))| {
            let is_focused = i == focused;
            let style = if is_focused {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Gray)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let origin_tag = if branch.remote_only { " (origin)" } else { "" };
            let origin_style = if is_focused {
                Style::default().fg(Color::Black).bg(Color::Gray)
            } else {
                Style::default().fg(Color::Yellow)
            };
            let date_style = if is_focused {
                Style::default().fg(Color::Black).bg(Color::Gray)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let line = Line::from(vec![
                Span::raw(format!("  {:<width$}", branch.name, width = max_name_len)),
                Span::styled(origin_tag, origin_style),
                Span::raw("  "),
                Span::styled(&branch.date, date_style),
            ]);
            ListItem::new(line).style(style)
        })
        .collect();

    let filter_display = if filter.is_empty() {
        String::new()
    } else {
        format!(" (filter: {}) ", filter)
    };

    let title = format!(
        " Select Branch to Review{} [{} branches] ",
        filter_display,
        filtered.len()
    );

    let item_count = filtered.len();
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .title_bottom(" Type to filter | ↑/↓: navigate | Enter: select "),
        )
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Gray)
                .add_modifier(Modifier::BOLD),
        );

    let mut list_state = ListState::default().with_selected(Some(focused));
    f.render_stateful_widget(list, area, &mut list_state);

    // Scrollbar
    let scrollbar_area = area.inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 0,
    });
    let mut scrollbar_state = ScrollbarState::new(item_count.saturating_sub(1)).position(focused);
    f.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight),
        scrollbar_area,
        &mut scrollbar_state,
    );
}

fn render_configure(f: &mut Frame, area: Rect, wizard: &ReviewWizardState) {
    let ReviewStep::Configure {
        focus,
        prompt_text,
        prompt_cursor,
        send_prompts,
        ai_selected,
        ai_focused,
        meta_selected,
        meta_focused,
    } = &wizard.current_step
    else {
        return;
    };

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5), // Prompt
            Constraint::Length(3), // Send prompts toggle
            Constraint::Min(0),   // AI Reviewers + Meta Reviewer side by side
        ])
        .split(area);

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50), // AI Reviewers
            Constraint::Percentage(50), // Meta Reviewer
        ])
        .split(rows[2]);

    // -- Prompt section --
    let prompt_border = if *focus == ConfigSection::Prompt {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let paragraph = Paragraph::new(prompt_text.as_str())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(prompt_border)
                .title(" Review Prompt ")
                .title_bottom(" Shift+Enter: newline "),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(paragraph, rows[0]);

    // Show cursor when prompt is focused
    if *focus == ConfigSection::Prompt {
        let inner = Block::default().borders(Borders::ALL).inner(rows[0]);
        let lines_before: Vec<&str> = prompt_text[..*prompt_cursor].split('\n').collect();
        let cursor_line = lines_before.len().saturating_sub(1);
        let cursor_col = lines_before.last().map_or(0, |l| l.chars().count());

        let inner_width = inner.width as usize;
        let mut screen_row = 0u16;
        let mut screen_col = 0u16;

        for (line_idx, line) in prompt_text.split('\n').enumerate() {
            let line_len = line.chars().count();
            let wrapped_lines = if inner_width > 0 {
                ((line_len.max(1)) as f64 / inner_width as f64).ceil() as u16
            } else {
                1
            };

            if line_idx == cursor_line {
                if inner_width > 0 {
                    screen_row += (cursor_col / inner_width) as u16;
                    screen_col = (cursor_col % inner_width) as u16;
                }
                break;
            }
            screen_row += wrapped_lines;
        }

        let final_row = inner.y + screen_row;
        let final_col = inner.x + screen_col;

        if final_row < inner.y + inner.height && final_col < inner.x + inner.width {
            f.set_cursor_position((final_col, final_row));
        }
    }

    // -- Send prompts toggle --
    let toggle_border = if *focus == ConfigSection::SendPrompts {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let checkbox = if *send_prompts { "[x]" } else { "[ ]" };
    let toggle_text = format!(" {} Send AI review prompts automatically", checkbox);
    let toggle_widget = Paragraph::new(toggle_text).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(toggle_border)
            .title(" Options "),
    );
    f.render_widget(toggle_widget, rows[1]);

    // -- AI Reviewers section --
    let ai_border = if *focus == ConfigSection::AiReviewers {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let ai_items: Vec<ListItem> = wizard
        .review_services
        .iter()
        .enumerate()
        .map(|(i, app)| {
            let checkbox = if ai_selected[i] { "[x]" } else { "[ ]" };
            ListItem::new(format!(" {} {}", checkbox, app.command))
        })
        .collect();

    let ai_highlight = if *focus == ConfigSection::AiReviewers {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Gray)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };

    let ai_list = List::new(ai_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(ai_border)
                .title(" AI Reviewers ")
                .title_bottom(" Space: toggle "),
        )
        .highlight_style(ai_highlight);

    let ai_count = wizard.review_services.len();
    let mut ai_state = ListState::default().with_selected(Some(*ai_focused));
    f.render_stateful_widget(ai_list, columns[0], &mut ai_state);

    let ai_scrollbar_area = columns[0].inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 0,
    });
    let mut ai_scrollbar_state =
        ScrollbarState::new(ai_count.saturating_sub(1)).position(*ai_focused);
    f.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight),
        ai_scrollbar_area,
        &mut ai_scrollbar_state,
    );

    // -- Meta Reviewer section --
    let meta_border = if *focus == ConfigSection::MetaReviewer {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let meta_items: Vec<ListItem> = wizard
        .review_services
        .iter()
        .enumerate()
        .map(|(i, app)| {
            let checkbox = if meta_selected[i] { "[x]" } else { "[ ]" };
            ListItem::new(format!(" {} {}", checkbox, app.command))
        })
        .collect();

    let meta_highlight = if *focus == ConfigSection::MetaReviewer {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Gray)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };

    let meta_count = wizard.review_services.len();
    let meta_list = List::new(meta_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(meta_border)
                .title(" Meta Reviewers ")
                .title_bottom(" Space: toggle "),
        )
        .highlight_style(meta_highlight);

    let mut meta_state = ListState::default().with_selected(Some(*meta_focused));
    f.render_stateful_widget(meta_list, columns[1], &mut meta_state);

    let meta_scrollbar_area = columns[1].inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 0,
    });
    let mut meta_scrollbar_state =
        ScrollbarState::new(meta_count.saturating_sub(1)).position(*meta_focused);
    f.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight),
        meta_scrollbar_area,
        &mut meta_scrollbar_state,
    );
}

fn render_footer(f: &mut Frame, area: Rect, wizard: &ReviewWizardState) {
    let hints = match &wizard.current_step {
        ReviewStep::SelectBranch { .. } => {
            "Type to filter | ↑/↓: navigate | Enter: select | Esc: cancel | Ctrl+C: quit"
        }
        ReviewStep::Configure { focus, .. } => match focus {
            ConfigSection::Prompt => {
                "Tab: next section | Shift+Enter: newline | Esc: back | Ctrl+C: quit"
            }
            ConfigSection::SendPrompts => {
                "Space: toggle | Tab: next section | Enter: start review | Esc: back"
            }
            ConfigSection::AiReviewers => {
                "↑/↓: navigate | Space: toggle | Tab: next section | Enter: start review | Esc: back"
            }
            ConfigSection::MetaReviewer => {
                "↑/↓: navigate | Space: toggle | Tab: next section | Enter: start review | Esc: back"
            }
        },
    };

    let footer = Paragraph::new(hints)
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL));

    f.render_widget(footer, area);
}

// ---------------------------------------------------------------------------
// Branch prefix generation
// ---------------------------------------------------------------------------

fn generate_review_prefix(worktrees_path: &Path, source_branch: &str) -> String {
    let prefix_base = format!("{}-review", source_branch);

    // Scan existing directories to find the next number
    let mut max_num = 0u32;
    if let Ok(entries) = std::fs::read_dir(worktrees_path) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            // Match pattern: {source_branch}-review-NN-*
            if let Some(rest) = name.strip_prefix(&format!("{}-", prefix_base)) {
                // rest should start with NN-
                if let Some(num_str) = rest.split('-').next()
                    && let Ok(num) = num_str.parse::<u32>()
                {
                    max_num = max_num.max(num);
                }
            }
        }
    }

    format!("{}-{:02}", prefix_base, max_num + 1)
}

// ---------------------------------------------------------------------------
// Worktree creation
// ---------------------------------------------------------------------------

fn create_review_worktrees(
    worktree_manager: &WorktreeManager,
    branch_prefix: &str,
    review_apps: &[AiApp],
    source_branch: &str,
) -> Result<Vec<(AiApp, String)>> {
    let worktree_paths = Arc::new(Mutex::new(Vec::new()));
    let errors = Arc::new(Mutex::new(Vec::new()));
    let mut handles = vec![];

    for ai_app in review_apps {
        let branch_name = format!("{}-{}-01", branch_prefix, ai_app.slug());
        let ai_app_clone = ai_app.clone();
        let worktree_paths_clone = Arc::clone(&worktree_paths);
        let errors_clone = Arc::clone(&errors);
        let project_path = worktree_manager.project_path().to_path_buf();
        let wt_path = worktree_manager.worktrees_path().to_path_buf();
        let source_branch = source_branch.to_string();

        let handle = thread::spawn(move || {
            println!(
                "  Creating worktree for {} with branch '{}'...",
                ai_app_clone.as_str(),
                branch_name
            );

            let wm = WorktreeManager::with_worktrees_path(project_path, wt_path);
            match wm.add_worktree(&branch_name) {
                Ok(worktree_path) => {
                    // Reset worktree to source branch content
                    let reset_result = Command::new("git")
                        .args(["reset", "--hard", &source_branch])
                        .current_dir(&worktree_path)
                        .output();

                    match reset_result {
                        Ok(output) if output.status.success() => {
                            println!(
                                "  Created worktree for {}: {}",
                                ai_app_clone.as_str(),
                                worktree_path.display()
                            );
                            let mut paths = worktree_paths_clone.lock().unwrap();
                            paths.push((
                                ai_app_clone,
                                worktree_path.to_string_lossy().to_string(),
                            ));
                        }
                        Ok(output) => {
                            let stderr = String::from_utf8_lossy(&output.stderr);
                            eprintln!(
                                "  Failed to reset worktree for {}: {}",
                                ai_app_clone.as_str(),
                                stderr
                            );
                            let mut errs = errors_clone.lock().unwrap();
                            errs.push(format!(
                                "{}: git reset failed: {}",
                                ai_app_clone.as_str(),
                                stderr
                            ));
                        }
                        Err(e) => {
                            eprintln!(
                                "  Failed to reset worktree for {}: {}",
                                ai_app_clone.as_str(),
                                e
                            );
                            let mut errs = errors_clone.lock().unwrap();
                            errs.push(format!(
                                "{}: git reset failed: {}",
                                ai_app_clone.as_str(),
                                e
                            ));
                        }
                    }
                }
                Err(e) => {
                    eprintln!(
                        "  Failed to create worktree for {}: {}",
                        ai_app_clone.as_str(),
                        e
                    );
                    let mut errs = errors_clone.lock().unwrap();
                    errs.push(format!("{}: {}", ai_app_clone.as_str(), e));
                }
            }
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.join().expect("Thread panicked");
    }

    let errors = errors.lock().unwrap();
    if !errors.is_empty() {
        return Err(MultiAiError::Worktree(format!(
            "Failed to create some review worktrees:\n{}",
            errors.join("\n")
        )));
    }

    // Sort by app order
    let mut paths = worktree_paths.lock().unwrap().clone();
    paths.sort_by_key(|a| {
        review_apps
            .iter()
            .position(|app| app.name == a.0.name)
            .unwrap_or(0)
    });

    Ok(paths)
}

// ---------------------------------------------------------------------------
// iTerm2 layout creation via AppleScript (single invocation)
// ---------------------------------------------------------------------------

/// Escape a string for embedding in an AppleScript double-quoted string.
/// Newlines are replaced with `" & return & "` so multi-line text is
/// concatenated properly instead of breaking the string literal.
fn applescript_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\" & return & \"")
}

fn create_iterm2_layout_applescript(
    wizard: &ReviewWizardState,
    review_apps: &[AiApp],
    worktree_paths: &[(AiApp, String)],
    review_prompt: &str,
    _meta_prompt: Option<&str>,
    branch_prefix: &str,
) -> Result<()> {
    if worktree_paths.is_empty() {
        return Ok(());
    }

    let num_apps = review_apps.len();

    let mut script = String::from(
        r#"
tell application "iTerm"
    tell current window
        create tab with default profile
        tell current session"#,
    );

    // --- Create vertical splits for columns ---
    for i in 2..=num_apps {
        if i == 2 {
            script.push_str(
                "\n            set col2 to (split vertically with default profile)",
            );
        } else {
            script.push_str(&format!(
                "\n            tell col{}\n                set col{} to (split vertically with default profile)\n            end tell",
                i - 1, i
            ));
        }
    }

    // --- Create horizontal splits for shell panes ---
    // Column 1 (current session)
    script.push_str(
        "\n            set col1Shell to (split horizontally with default profile)",
    );
    // Other columns
    for i in 2..=num_apps {
        script.push_str(&format!(
            "\n            tell col{}\n                set col{}Shell to (split horizontally with default profile)\n            end tell",
            i, i
        ));
    }

    // --- Launch AI tools and shells in each column ---
    for (i, (app, path)) in worktree_paths.iter().enumerate() {
        let col_num = i + 1;
        let escaped_path = applescript_escape(path);
        let escaped_cmd = applescript_escape(&app.command);

        if i == 0 {
            // First column: current session is the AI pane
            script.push_str(&format!(
                r#"
            delay 2
            write text "cd {} && {}""#,
                escaped_path, escaped_cmd
            ));
            // Shell pane
            script.push_str(&format!(
                r#"
            tell col1Shell
                delay 1
                write text "cd {}"
            end tell"#,
                escaped_path
            ));
        } else {
            // Other columns: colN is the AI pane
            script.push_str(&format!(
                r#"
            tell col{}
                delay 1
                write text "cd {} && {}"
            end tell"#,
                col_num, escaped_path, escaped_cmd
            ));
            // Shell pane
            script.push_str(&format!(
                r#"
            tell col{}Shell
                delay 1
                write text "cd {}"
            end tell"#,
                col_num, escaped_path
            ));
        }
    }

    // --- Send review prompts to AI-tagged tools after a delay ---
    // Each tool gets its own delay before prompt send so slower tools
    // (codex, copilot) have time to initialise their input.
    if wizard.send_prompts {
        script.push_str("\n            delay 5");
        for (i, tool) in wizard.selected_tools.iter().enumerate() {
            if tool.tag != ReviewTag::Ai {
                continue;
            }
            let col_num = i + 1;
            let escaped_prompt = applescript_escape(review_prompt);
            if i == 0 {
                script.push_str(&format!(
                    r#"
            write text "{}"
            delay 0.5
            write text """#,
                    escaped_prompt
                ));
            } else {
                script.push_str(&format!(
                    r#"
            tell col{}
                delay 1
                write text "{}"
                delay 0.5
                write text ""
            end tell"#,
                    col_num, escaped_prompt
                ));
            }
        }

    }

    // --- Set tab title ---
    script.push_str(&format!(
        r#"
            set name to "{}""#,
        applescript_escape(branch_prefix)
    ));

    script.push_str(
        r#"
        end tell
    end tell
end tell"#,
    );

    // Execute
    let output = Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
        .map_err(|e| MultiAiError::Review(format!("Failed to execute AppleScript: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(MultiAiError::Review(format!(
            "AppleScript failed: {}",
            stderr
        )));
    }

    Ok(())
}
