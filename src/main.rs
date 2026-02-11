mod config;
mod error;
mod git;
mod init;
#[cfg(target_os = "macos")]
mod iterm2;
mod picker;
mod review;
mod send;
mod tmux;
mod worktree;

use clap::{Parser, ValueEnum};
use config::{Mode, ProjectConfig, TmuxLayout};
use error::{MultiAiError, Result};
#[cfg(target_os = "macos")]
use iterm2::ITerm2Manager;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::SystemTime;
use tmux::TmuxManager;
use worktree::WorktreeManager;

#[derive(Parser, Debug)]
#[command(name = "mai")]
#[command(version, about = "Multi-AI workspace manager with git worktrees", long_about = None)]
#[command(author = "Mikko Kohtala")]
#[command(disable_version_flag = true)]
struct Args {
    #[arg(short = 'v', long = "version", action = clap::ArgAction::Version)]
    _version: (),

    #[command(subcommand)]
    command: Option<Command>,

    #[arg(help = "Path to project directory")]
    project_path: Option<String>,

    #[arg(help = "Branch prefix (e.g., 'vercel-theme')")]
    branch_prefix: Option<String>,
}

#[derive(Parser, Debug)]
enum Command {
    #[command(about = "Initialize multi-ai-config.jsonc file interactively")]
    Init,

    #[command(about = "Add worktrees and session for multiple AI tools")]
    Add {
        #[arg(help = "Branch prefix for the worktrees (interactive picker if omitted)")]
        branch_prefix: Option<String>,

        #[arg(
            long,
            help = "Use tmux multi-window layout (legacy flag)",
            conflicts_with = "mode"
        )]
        tmux: bool,

        #[arg(
            long,
            value_enum,
            help = "Override configured mode (iterm2, tmux-single-window, tmux-multi-window)"
        )]
        mode: Option<ModeOverride>,
    },

    #[command(about = "Remove worktrees and session for a branch prefix")]
    Remove {
        #[arg(help = "Branch prefix to remove (interactive picker if omitted)")]
        branch_prefix: Option<String>,

        #[arg(
            long,
            help = "Assume tmux multi-window session when cleaning up (legacy flag)",
            conflicts_with = "mode"
        )]
        tmux: bool,

        #[arg(long, value_enum, help = "Override configured mode to control cleanup")]
        mode: Option<ModeOverride>,

        #[arg(
            short = 'f',
            long = "force",
            help = "Skip confirmation prompt and remove immediately"
        )]
        force: bool,
    },

    #[command(about = "Continue working on existing worktrees (creates new session/tab)")]
    Continue {
        #[arg(help = "Branch prefix for the existing worktrees")]
        branch_prefix: String,

        #[arg(
            long,
            help = "Use tmux multi-window layout (legacy flag)",
            conflicts_with = "mode"
        )]
        tmux: bool,

        #[arg(
            long,
            value_enum,
            help = "Override configured mode (iterm2, tmux-single-window, tmux-multi-window)"
        )]
        mode: Option<ModeOverride>,
    },

    #[command(about = "Resume working on existing worktrees (alias for continue)")]
    Resume {
        #[arg(help = "Branch prefix for the existing worktrees")]
        branch_prefix: String,

        #[arg(
            long,
            help = "Use tmux multi-window layout (legacy flag)",
            conflicts_with = "mode"
        )]
        tmux: bool,

        #[arg(
            long,
            value_enum,
            help = "Override configured mode (iterm2, tmux-single-window, tmux-multi-window)"
        )]
        mode: Option<ModeOverride>,
    },

    #[command(about = "Send text to a running session via TUI")]
    Send,

    #[command(about = "Launch interactive multi-AI code review")]
    Review {
        /// Branch to review (skips branch selection if exact match found)
        #[arg(index = 1)]
        branch: Option<String>,
    },

    #[command(about = "Open the project config file in the default application")]
    Config,

    #[command(about = "List worktree environments and their worktrees")]
    List,

    #[command(about = "Open the global AI tools configuration file")]
    Apps,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum ModeOverride {
    Iterm2,
    #[value(name = "tmux-single-window")]
    TmuxSingleWindow,
    #[value(name = "tmux-multi-window")]
    TmuxMultiWindow,
}

impl From<ModeOverride> for Mode {
    fn from(value: ModeOverride) -> Self {
        match value {
            ModeOverride::Iterm2 => Mode::Iterm2,
            ModeOverride::TmuxSingleWindow => Mode::TmuxSingleWindow,
            ModeOverride::TmuxMultiWindow => Mode::TmuxMultiWindow,
        }
    }
}

fn main() -> Result<()> {
    let args = Args::parse();

    match args.command {
        Some(Command::Init) => init::run_init(),
        Some(Command::Add {
            branch_prefix,
            tmux,
            mode,
        }) => {
            if let Some(prefix) = branch_prefix {
                create_command(prefix, tmux, mode, None)
            } else {
                interactive_add_command(tmux, mode)
            }
        }
        Some(Command::Remove {
            branch_prefix,
            tmux,
            mode,
            force,
        }) => {
            if let Some(prefix) = branch_prefix {
                remove_command(prefix, tmux, mode, force)
            } else {
                interactive_remove_command(tmux, mode, force)
            }
        }
        Some(Command::Continue {
            branch_prefix,
            tmux,
            mode,
        }) => continue_command(branch_prefix, tmux, mode),
        Some(Command::Resume {
            branch_prefix,
            tmux,
            mode,
        }) => continue_command(branch_prefix, tmux, mode),
        Some(Command::Send) => send_command(),
        Some(Command::Review { branch }) => review_command(branch),
        Some(Command::List) => list_command(),
        Some(Command::Config) => config_command(),
        Some(Command::Apps) => apps_command(),
        None => {
            eprintln!("Error: Command required. Use 'mai add <branch-prefix>' or 'mai remove <branch-prefix>'");
            eprintln!("Run 'mai --help' for more information.");
            std::process::exit(1);
        }
    }
}

#[inline]
fn system_default_mode() -> Mode {
    #[cfg(target_os = "macos")]
    {
        Mode::Iterm2
    }
    #[cfg(not(target_os = "macos"))]
    {
        Mode::TmuxSingleWindow
    }
}

/// Find git-worktree-config.jsonc by checking:
/// 1. Current directory
/// 2. ./main/ subdirectory
/// 3. Global gwt config by repo URL: ~/.config/git-worktree-cli/projects/{repo-name}.jsonc
/// 4. Global gwt config by path match (worktreesPath or projectPath in config)
fn find_gwt_config_file(base_path: &Path) -> Option<PathBuf> {
    // First check current directory
    let current_path = base_path.join("git-worktree-config.jsonc");
    if current_path.exists() {
        return Some(current_path);
    }

    // Then check ./main/ subdirectory
    let main_path = base_path.join("main").join("git-worktree-config.jsonc");
    if main_path.exists() {
        return Some(main_path);
    }

    // Check global gwt configs - gwt uses ~/.config/ not the platform config dir
    let home_dir = dirs::home_dir()?;
    let gwt_projects_dir = home_dir.join(".config").join("git-worktree-cli").join("projects");
    if !gwt_projects_dir.exists() {
        return None;
    }

    // Try to find by repo URL first
    if let Some(repo_url) = git::get_remote_origin_url(base_path) {
        let config_filename = format!("{}.jsonc", git::generate_config_filename(&repo_url));
        let global_path = gwt_projects_dir.join(&config_filename);
        if global_path.exists() {
            return Some(global_path);
        }
    }

    // Search all global gwt configs for matching worktreesPath or projectPath
    let base_path_canonical = base_path.canonicalize().ok();

    // Helper to check if a config path matches the base path
    let check_path_match =
        |base_canonical: &Option<PathBuf>, base: &Path, config_path: &PathBuf| -> bool {
            if let Some(base_can) = base_canonical {
                if let Ok(config_canonical) = config_path.canonicalize() {
                    base_can == &config_canonical || base_can.starts_with(&config_canonical)
                } else {
                    base == config_path || base.starts_with(config_path)
                }
            } else {
                base == config_path || base.starts_with(config_path)
            }
        };

    for entry in std::fs::read_dir(&gwt_projects_dir).ok()?.flatten() {
        let path = entry.path();

        if path.extension().map(|e| e == "jsonc").unwrap_or(false) {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(Some(serde_json::Value::Object(map))) =
                    jsonc_parser::parse_to_serde_value(&content, &Default::default())
                {
                    // Check both worktreesPath and projectPath
                    // gwt uses camelCase: worktreesPath, projectPath
                    let matches = {
                        let mut found = false;

                        // Check worktreesPath
                        if let Some(serde_json::Value::String(worktrees_path)) =
                            map.get("worktreesPath")
                        {
                            let wt_path = PathBuf::from(worktrees_path);
                            found = check_path_match(&base_path_canonical, base_path, &wt_path);
                        }

                        // Check projectPath
                        if !found {
                            if let Some(serde_json::Value::String(project_path)) =
                                map.get("projectPath")
                            {
                                let proj_path = PathBuf::from(project_path);
                                found =
                                    check_path_match(&base_path_canonical, base_path, &proj_path);
                            }
                        }

                        found
                    };

                    if matches {
                        return Some(path);
                    }
                }
            }
        }
    }

    None
}

/// Create a WorktreeManager, using the mai config's worktrees_path if set.
fn make_worktree_manager(
    project_config: &ProjectConfig,
    project_path: PathBuf,
) -> WorktreeManager {
    if let Some(ref wt_path) = project_config.worktrees_path {
        WorktreeManager::with_worktrees_path(project_path, wt_path.clone())
    } else {
        WorktreeManager::new(project_path)
    }
}

/// Discover worktree branch names matching a prefix by scanning the worktrees directory.
/// Returns directory names like ["test01-claude", "test01-gemini-yolo"].
/// Also includes a standalone worktree whose name equals the prefix exactly.
fn discover_worktree_branches(worktree_manager: &WorktreeManager, branch_prefix: &str) -> Vec<String> {
    let wt_dir = worktree_manager.worktrees_path();
    let prefix_dash = format!("{}-", branch_prefix);
    let mut branches: Vec<String> = collect_worktree_entries(wt_dir)
        .into_iter()
        .filter(|name| name.starts_with(&prefix_dash) || name == branch_prefix)
        .collect();
    branches.sort();
    branches
}

fn interactive_add_command(
    cli_tmux: bool,
    mode_override: Option<ModeOverride>,
) -> Result<()> {
    let result = picker::run_app_picker(None)?;
    let Some(result) = result else {
        println!("Cancelled.");
        return Ok(());
    };

    if result.selected_apps.is_empty() {
        println!("No tools selected.");
        return Ok(());
    }

    create_command(result.env_name, cli_tmux, mode_override, Some(result.selected_apps))
}

fn interactive_remove_command(
    cli_tmux: bool,
    mode_override: Option<ModeOverride>,
    force: bool,
) -> Result<()> {
    let current_dir = std::env::current_dir()
        .map_err(|e| MultiAiError::Config(format!("Failed to get current directory: {}", e)))?;

    let (_config_path, project_config, project_path) = ProjectConfig::find_config(&current_dir)
        .map_err(|e| MultiAiError::Config(format!("Failed to find config: {}", e)))?
        .ok_or_else(|| MultiAiError::Config(
            "Config not found in ~/.config/multi-ai-cli/. Run 'mai init' from your project directory to create one.".to_string()
        ))?;

    let worktree_manager = make_worktree_manager(&project_config, project_path);

    let prefix_groups = discover_all_prefixes(&worktree_manager, &project_config);

    if prefix_groups.is_empty() {
        println!("No worktree prefixes found.");
        return Ok(());
    }

    let selected = picker::run_prefix_picker(prefix_groups)?;
    let Some(selected) = selected else {
        println!("Cancelled.");
        return Ok(());
    };

    if selected.is_empty() {
        println!("No prefixes selected.");
        return Ok(());
    }

    for prefix in &selected {
        println!("\n--- Removing '{}' ---", prefix);
        remove_command(prefix.clone(), cli_tmux, mode_override, force)?;
    }

    Ok(())
}

/// Recursively collect worktree directory names relative to `base`.
/// A directory is considered a worktree if it contains a `.git` entry.
/// Intermediate directories (e.g. `feat/` for branch `feat/branch-name`)
/// are traversed without being collected themselves.
fn collect_worktree_entries(base: &Path) -> Vec<String> {
    fn walk(dir: &Path, prefix: &str, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            if !entry.path().is_dir() {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(String::from) else {
                continue;
            };
            let relative = if prefix.is_empty() {
                name
            } else {
                format!("{}/{}", prefix, name)
            };
            if entry.path().join(".git").exists() {
                out.push(relative);
            } else {
                walk(&entry.path(), &relative, out);
            }
        }
    }
    let mut entries = Vec::new();
    walk(base, "", &mut entries);
    entries
}

/// Discover all unique branch prefixes by scanning the worktrees directory
/// and stripping known app slug suffixes.
/// Returns (prefix, worktree_dir_names) pairs sorted by prefix.
fn discover_all_prefixes(
    worktree_manager: &WorktreeManager,
    project_config: &ProjectConfig,
) -> Vec<(String, Vec<String>)> {
    let wt_dir = worktree_manager.worktrees_path();

    // Collect known slugs from global apps.jsonc and project config
    let mut known_slugs: Vec<String> = Vec::new();

    if let Ok(all_apps) = init::load_apps() {
        for app in &all_apps {
            known_slugs.push(app.slug());
        }
    }

    for app in &project_config.ai_apps {
        known_slugs.push(app.slug());
    }

    known_slugs.sort();
    known_slugs.dedup();

    // Sort by length descending so longer slugs match first
    // (prevents "claude" from matching before "claude-plan-yolo")
    known_slugs.sort_by_key(|b| std::cmp::Reverse(b.len()));

    let mut prefix_map: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();

    for name in collect_worktree_entries(wt_dir) {
        if name == "main" {
            continue;
        }
        let mut matched = false;
        for slug in &known_slugs {
            let suffix = format!("-{}", slug);
            if let Some(prefix) = name.strip_suffix(&suffix)
                && !prefix.is_empty()
            {
                prefix_map
                    .entry(prefix.to_string())
                    .or_default()
                    .push(name.clone());
                matched = true;
                break;
            }
        }
        if !matched {
            // Standalone worktree not matching any known slug
            prefix_map
                .entry(name.clone())
                .or_default()
                .push(name);
        }
    }

    for worktrees in prefix_map.values_mut() {
        worktrees.sort();
    }

    prefix_map.into_iter().collect()
}

fn create_command(
    mut branch_prefix: String,
    cli_tmux: bool,
    mode_override: Option<ModeOverride>,
    override_apps: Option<Vec<config::AiApp>>,
) -> Result<()> {
    let current_dir = std::env::current_dir()
        .map_err(|e| MultiAiError::Config(format!("Failed to get current directory: {}", e)))?;

    // Find config using the new search order
    let (config_path, project_config, project_path) = ProjectConfig::find_config(&current_dir)
        .map_err(|e| MultiAiError::Config(format!("Failed to find config: {}", e)))?
        .ok_or_else(|| MultiAiError::Config(
            "Config not found in ~/.config/multi-ai-cli/. Run 'mai init' from your project directory to create one.".to_string()
        ))?;

    println!("Using config: {}", config_path.display());

    // Check for git-worktree-config.jsonc (at the project path)
    let _gwt_config_path = find_gwt_config_file(&project_path)
        .ok_or_else(|| MultiAiError::Config(
            format!("git-worktree-config.jsonc not found in {} or its ./main/ subdirectory. Please ensure this file exists.", project_path.display())
        ))?;

    let project_name = project_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| MultiAiError::Config("Invalid project path".to_string()))?
        .to_string();

    let worktree_manager = make_worktree_manager(&project_config, project_path.clone());

    if !worktree_manager.has_gwt_cli() {
        return Err(MultiAiError::Worktree(
            "gwt CLI is not installed. Please install from https://github.com/mikko-kohtala/git-worktree-cli".to_string()
        ));
    }

    if !worktree_manager.is_gwt_project() {
        return Err(MultiAiError::Worktree(
            "Current directory is not initialized with gwt. Please ensure git-worktree-config.jsonc exists or run 'gwt init' first.".to_string()
        ));
    }

    let ai_apps = if let Some(apps) = override_apps {
        apps
    } else if !project_config.ai_apps.is_empty() {
        project_config.ai_apps.clone()
    } else {
        // No apps in config — launch interactive picker with prefilled env name
        let result = picker::run_app_picker(Some(&branch_prefix))?;
        let Some(result) = result else {
            println!("Cancelled.");
            return Ok(());
        };
        if result.selected_apps.is_empty() {
            println!("No tools selected.");
            return Ok(());
        }
        branch_prefix = result.env_name;
        result.selected_apps
    };

    // Create worktrees in parallel
    println!("Creating worktrees in parallel...");
    let worktree_paths = Arc::new(Mutex::new(Vec::new()));
    let errors = Arc::new(Mutex::new(Vec::new()));

    let mut handles = vec![];

    let config_wt_path = project_config.worktrees_path.clone();
    for ai_app in &ai_apps {
        let branch_name = format!("{}-{}", branch_prefix, ai_app.slug());
        let ai_app_clone = ai_app.clone();
        let project_path_clone = project_path.clone();
        let config_wt_path_clone = config_wt_path.clone();
        let worktree_paths_clone = Arc::clone(&worktree_paths);
        let errors_clone = Arc::clone(&errors);

        let handle = thread::spawn(move || {
            println!(
                "  Creating worktree for {} with branch '{}'...",
                ai_app_clone.command(),
                branch_name
            );

            let worktree_manager = if let Some(wt_path) = config_wt_path_clone {
                WorktreeManager::with_worktrees_path(project_path_clone, wt_path)
            } else {
                WorktreeManager::new(project_path_clone)
            };
            match worktree_manager.add_worktree(&branch_name) {
                Ok(worktree_path) => {
                    println!(
                        "  ✓ Created worktree for {}: {}",
                        ai_app_clone.command(),
                        worktree_path.display()
                    );
                    let mut paths = worktree_paths_clone.lock().unwrap();
                    paths.push((ai_app_clone, worktree_path.to_string_lossy().to_string()));
                }
                Err(e) => {
                    eprintln!(
                        "  ✗ Failed to create worktree for {}: {}",
                        ai_app_clone.command(),
                        e
                    );
                    let mut errs = errors_clone.lock().unwrap();
                    errs.push(format!("{}: {}", ai_app_clone.command(), e));
                }
            }
        });

        handles.push(handle);
    }

    // Wait for all threads to complete
    for handle in handles {
        handle.join().expect("Thread panicked");
    }

    // Check if there were any errors
    let errors = errors.lock().unwrap();
    if !errors.is_empty() {
        return Err(MultiAiError::Worktree(format!(
            "Failed to create some worktrees:\n{}",
            errors.join("\n")
        )));
    }

    // Get the final worktree paths, sorted by app order
    let mut worktree_paths = worktree_paths.lock().unwrap().clone();
    worktree_paths.sort_by_key(|a| {
        ai_apps
            .iter()
            .position(|app| app.name == a.0.name)
            .unwrap_or(0)
    });

    println!("✓ All worktrees created successfully!");

    // Determine mode: CLI override > legacy --tmux > config file > system default
    let mut mode = mode_override.map(Into::into);
    if mode.is_none() && cli_tmux {
        mode = Some(Mode::TmuxMultiWindow);
    }
    if mode.is_none() {
        mode = project_config.mode.clone();
    }
    let mode = mode.unwrap_or_else(system_default_mode);

    match mode {
        Mode::Iterm2 => {
            #[cfg(not(target_os = "macos"))]
            {
                return Err(MultiAiError::Config(
                    "iTerm2 mode is only supported on macOS".to_string(),
                ));
            }
            #[cfg(target_os = "macos")]
            {
                let iterm2_manager = ITerm2Manager::new(
                    &project_name,
                    &branch_prefix,
                    project_config.terminals_per_column,
                );
                println!("\nCreating iTerm2 tabs for AI applications...");
                println!(
                    "  Apps to create tabs for: {:?}",
                    worktree_paths
                        .iter()
                        .map(|(app, _)| app.as_str())
                        .collect::<Vec<_>>()
                );
                println!(
                    "  Terminals per column: {}",
                    project_config.terminals_per_column
                );
                match iterm2_manager.create_tabs_per_app(&ai_apps, &worktree_paths) {
                    Ok(_) => println!("✓ iTerm2 tabs created successfully!"),
                    Err(e) => {
                        eprintln!("✗ Failed to create iTerm2 tabs: {}", e);
                        return Err(e);
                    }
                }
            }
        }
        Mode::TmuxMultiWindow | Mode::TmuxSingleWindow => {
            let layout = match mode {
                Mode::TmuxSingleWindow => TmuxLayout::SingleWindow,
                _ => TmuxLayout::MultiWindow,
            };
            let tmux_manager = TmuxManager::new(&project_name, &branch_prefix);
            println!(
                "\nCreating tmux session '{}-{}' (layout: {:?})...",
                project_name, branch_prefix, layout
            );
            tmux_manager.create_session(&ai_apps, &worktree_paths, layout)?;
            println!("✓ Tmux session created successfully!");
            println!("\nAttaching to session...");
            tmux_manager.attach_session()?;
        }
    }

    Ok(())
}

fn remove_command(
    branch_prefix: String,
    cli_tmux: bool,
    mode_override: Option<ModeOverride>,
    force: bool,
) -> Result<()> {
    let current_dir = std::env::current_dir()
        .map_err(|e| MultiAiError::Config(format!("Failed to get current directory: {}", e)))?;

    // Find config using the new search order
    let (config_path, project_config, project_path) = ProjectConfig::find_config(&current_dir)
        .map_err(|e| MultiAiError::Config(format!("Failed to find config: {}", e)))?
        .ok_or_else(|| MultiAiError::Config(
            "Config not found in ~/.config/multi-ai-cli/. Run 'mai init' from your project directory to create one.".to_string()
        ))?;

    println!("Using config: {}", config_path.display());

    // Check for git-worktree-config.jsonc (at the project path)
    let _gwt_config_path = find_gwt_config_file(&project_path)
        .ok_or_else(|| MultiAiError::Config(
            format!("git-worktree-config.jsonc not found in {} or its ./main/ subdirectory. Please ensure this file exists.", project_path.display())
        ))?;

    let project_name = project_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| MultiAiError::Config("Invalid project path".to_string()))?
        .to_string();
    let worktree_manager = make_worktree_manager(&project_config, project_path.clone());

    if !worktree_manager.has_gwt_cli() {
        return Err(MultiAiError::Worktree(
            "gwt CLI is not installed. Please install from https://github.com/mikko-kohtala/git-worktree-cli".to_string()
        ));
    }

    // Determine which worktree branches to remove
    let branch_names: Vec<String> = if !project_config.ai_apps.is_empty() {
        project_config
            .ai_apps
            .iter()
            .map(|app| format!("{}-{}", branch_prefix, app.slug()))
            .collect()
    } else {
        discover_worktree_branches(&worktree_manager, &branch_prefix)
    };

    if branch_names.is_empty() {
        println!("No worktrees found for prefix '{}'.", branch_prefix);
        return Ok(());
    }

    // Ask for confirmation
    println!("⚠️  You are about to remove:");
    println!("  - Worktrees for branches:");
    for branch_name in &branch_names {
        println!("    • {}", branch_name);
    }
    // Determine mode for cleanup (optional)
    let mut mode = mode_override.map(Into::into);
    if mode.is_none() && cli_tmux {
        mode = Some(Mode::TmuxMultiWindow);
    }
    if mode.is_none() {
        mode = project_config.mode.clone();
    }

    match mode {
        Some(Mode::TmuxMultiWindow) | Some(Mode::TmuxSingleWindow) => {
            println!("  - Tmux session: {}-{}", project_name, branch_prefix);
        }
        Some(Mode::Iterm2) => {
            println!("  - Note: iTerm2 tabs must be closed manually");
        }
        None => {
            println!(
                "  - Will attempt to remove tmux session '{}-{}' if present; iTerm2 tabs must be closed manually",
                project_name, branch_prefix
            );
        }
    }
    println!();

    if !force {
        if !ask_confirmation("Are you sure you want to remove these worktrees and session?")? {
            println!("Removal cancelled.");
            return Ok(());
        }
    } else {
        println!("Forcing removal without confirmation (--force).");
    }

    // Best-effort: try to kill tmux session regardless of configured mode.
    // If tmux isn't installed or the session doesn't exist, this will no-op or warn.
    let tmux_manager = TmuxManager::new(&project_name, &branch_prefix);
    println!(
        "Removing tmux session '{}-{}' (if present)...",
        project_name, branch_prefix
    );
    match tmux_manager.kill_session() {
        Ok(_) => println!("  ✓ Tmux session removed or not present"),
        Err(e) => eprintln!("  ⚠ Tmux cleanup skipped: {}", e),
    }

    // For iTerm2, we can't programmatically close tabs, just notify the user
    println!(
        "Note: If you previously used iTerm2 mode, please close the iTerm2 tabs for '{}-{}' manually.",
        project_name, branch_prefix
    );

    // Remove worktrees
    for branch_name in &branch_names {
        println!("Removing worktree for branch '{}'...", branch_name);

        match worktree_manager.remove_worktree(branch_name) {
            Ok(_) => println!("  ✓ Removed worktree: {}", branch_name),
            Err(e) => eprintln!("  ✗ Failed to remove worktree: {}", e),
        }
    }

    println!("\n✓ Cleanup completed!");
    Ok(())
}

fn continue_command(
    branch_prefix: String,
    cli_tmux: bool,
    mode_override: Option<ModeOverride>,
) -> Result<()> {
    let current_dir = std::env::current_dir()
        .map_err(|e| MultiAiError::Config(format!("Failed to get current directory: {}", e)))?;

    // Find config using the new search order
    let (config_path, project_config, project_path) = ProjectConfig::find_config(&current_dir)
        .map_err(|e| MultiAiError::Config(format!("Failed to find config: {}", e)))?
        .ok_or_else(|| MultiAiError::Config(
            "Config not found in ~/.config/multi-ai-cli/. Run 'mai init' from your project directory to create one.".to_string()
        ))?;

    println!("Using config: {}", config_path.display());

    // Check for git-worktree-config.jsonc (at the project path)
    let _gwt_config_path = find_gwt_config_file(&project_path)
        .ok_or_else(|| MultiAiError::Config(
            format!("git-worktree-config.jsonc not found in {} or its ./main/ subdirectory. Please ensure this file exists.", project_path.display())
        ))?;

    let project_name = project_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| MultiAiError::Config("Invalid project path".to_string()))?
        .to_string();
    let worktree_manager = make_worktree_manager(&project_config, project_path.clone());

    // Discover worktree paths — use config ai_apps if set, otherwise scan the directory
    let worktree_paths: Vec<(config::AiApp, String)> = if !project_config.ai_apps.is_empty() {
        let ai_app_slugs: Vec<String> = project_config
            .ai_apps
            .iter()
            .map(|app| app.slug())
            .collect();

        if !worktree_manager.worktrees_exist(&branch_prefix, &ai_app_slugs) {
            return Err(MultiAiError::Worktree(format!(
                "Worktrees for '{}' do not exist. Run 'mai add {}' first.",
                branch_prefix, branch_prefix
            )));
        }

        project_config
            .ai_apps
            .iter()
            .map(|ai_app| {
                let branch_name = format!("{}-{}", branch_prefix, ai_app.slug());
                let worktree_path = worktree_manager.worktrees_path().join(&branch_name);
                (ai_app.clone(), worktree_path.to_string_lossy().to_string())
            })
            .collect()
    } else {
        // No ai_apps in config — discover from directory and match against apps.jsonc
        let all_apps = init::load_apps().unwrap_or_default();
        let branch_names = discover_worktree_branches(&worktree_manager, &branch_prefix);
        if branch_names.is_empty() {
            return Err(MultiAiError::Worktree(format!(
                "No worktrees found for prefix '{}'. Run 'mai add {}' first.",
                branch_prefix, branch_prefix
            )));
        }
        let prefix_dash = format!("{}-", branch_prefix);
        branch_names
            .iter()
            .map(|branch_name| {
                let slug = branch_name.strip_prefix(&prefix_dash).unwrap_or(branch_name);
                let app = all_apps
                    .iter()
                    .find(|a| a.slug() == slug)
                    .cloned()
                    .unwrap_or_else(|| config::AiApp {
                        name: slug.to_string(),
                        command: slug.to_string(),
                        slug: Some(slug.to_string()),
                        ultrathink: None,
                        default: false,
                        meta_review: false,
                        description: None,
                    });
                let worktree_path = worktree_manager.worktrees_path().join(branch_name);
                (app, worktree_path.to_string_lossy().to_string())
            })
            .collect()
    };

    println!("✓ Found existing worktrees for '{}'", branch_prefix);

    // Determine mode: CLI override > legacy --tmux > config file > system default
    let mut mode = mode_override.map(Into::into);
    if mode.is_none() && cli_tmux {
        mode = Some(Mode::TmuxMultiWindow);
    }
    if mode.is_none() {
        mode = project_config.mode.clone();
    }
    let mode = mode.unwrap_or_else(system_default_mode);

    match mode {
        Mode::Iterm2 => {
            #[cfg(not(target_os = "macos"))]
            {
                return Err(MultiAiError::Config(
                    "iTerm2 mode is only supported on macOS".to_string(),
                ));
            }
            #[cfg(target_os = "macos")]
            {
                let iterm2_manager = ITerm2Manager::new(
                    &project_name,
                    &branch_prefix,
                    project_config.terminals_per_column,
                );
                println!("\nCreating new iTerm2 tab for existing worktrees...");
                println!(
                    "  Apps to create tabs for: {:?}",
                    worktree_paths
                        .iter()
                        .map(|(app, _)| app.as_str())
                        .collect::<Vec<_>>()
                );
                println!(
                    "  Terminals per column: {}",
                    project_config.terminals_per_column
                );
                let ai_apps: Vec<config::AiApp> = worktree_paths.iter().map(|(app, _)| app.clone()).collect();
                match iterm2_manager.create_tabs_per_app(&ai_apps, &worktree_paths) {
                    Ok(_) => println!("✓ iTerm2 tab created successfully!"),
                    Err(e) => {
                        eprintln!("✗ Failed to create iTerm2 tab: {}", e);
                        return Err(e);
                    }
                }
            }
        }
        Mode::TmuxMultiWindow | Mode::TmuxSingleWindow => {
            let layout = match mode {
                Mode::TmuxSingleWindow => TmuxLayout::SingleWindow,
                _ => TmuxLayout::MultiWindow,
            };
            let tmux_manager = TmuxManager::new(&project_name, &branch_prefix);
            println!(
                "\nCreating new tmux session '{}-{}' (layout: {:?})...",
                project_name, branch_prefix, layout
            );
            let ai_apps: Vec<config::AiApp> = worktree_paths.iter().map(|(app, _)| app.clone()).collect();
            tmux_manager.create_session(&ai_apps, &worktree_paths, layout)?;
            println!("✓ Tmux session created successfully!");
            println!("\nAttaching to session...");
            tmux_manager.attach_session()?;
        }
    }

    Ok(())
}

fn send_command() -> Result<()> {
    let current_dir = std::env::current_dir()
        .map_err(|e| MultiAiError::Config(format!("Failed to get current directory: {}", e)))?;

    // Find config using the new search order
    let (config_path, project_config, project_path) = ProjectConfig::find_config(&current_dir)
        .map_err(|e| MultiAiError::Config(format!("Failed to find config: {}", e)))?
        .ok_or_else(|| MultiAiError::Config(
            "Config not found in ~/.config/multi-ai-cli/. Run 'mai init' from your project directory to create one.".to_string()
        ))?;

    println!("Using config: {}", config_path.display());

    let project_name = project_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| MultiAiError::Config("Invalid project path".to_string()))?
        .to_string();

    send::run_send_command(project_config, project_name)
}

fn review_command(branch: Option<String>) -> Result<()> {
    let current_dir = std::env::current_dir()
        .map_err(|e| MultiAiError::Config(format!("Failed to get current directory: {}", e)))?;

    // Find config
    let (config_path, project_config, project_path) = ProjectConfig::find_config(&current_dir)
        .map_err(|e| MultiAiError::Config(format!("Failed to find config: {}", e)))?
        .ok_or_else(|| MultiAiError::Config(
            "Config not found in ~/.config/multi-ai-cli/. Run 'mai init' from your project directory to create one.".to_string()
        ))?;

    println!("Using config: {}", config_path.display());

    // Check for gwt config
    let _gwt_config_path = find_gwt_config_file(&project_path)
        .ok_or_else(|| MultiAiError::Config(
            format!("git-worktree-config.jsonc not found in {} or its ./main/ subdirectory. Please ensure this file exists.", project_path.display())
        ))?;

    let project_name = project_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| MultiAiError::Config("Invalid project path".to_string()))?
        .to_string();

    let worktree_manager = make_worktree_manager(&project_config, project_path.clone());

    if !worktree_manager.has_gwt_cli() {
        return Err(MultiAiError::Worktree(
            "gwt CLI is not installed. Please install from https://github.com/mikko-kohtala/git-worktree-cli".to_string()
        ));
    }

    if !worktree_manager.is_gwt_project() {
        return Err(MultiAiError::Worktree(
            "Current directory is not initialized with gwt. Please ensure git-worktree-config.jsonc exists or run 'gwt init' first.".to_string()
        ));
    }

    review::run_review(project_config, project_name, project_path, worktree_manager, branch)
}

fn format_relative_time(time: SystemTime) -> String {
    let elapsed = time.elapsed().unwrap_or_default();
    let secs = elapsed.as_secs();
    if secs < 60 {
        "just now".to_string()
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86400 {
        format!("{}h ago", secs / 3600)
    } else if secs < 604800 {
        format!("{}d ago", secs / 86400)
    } else {
        format!("{}w ago", secs / 604800)
    }
}

fn list_command() -> Result<()> {
    let current_dir = std::env::current_dir()
        .map_err(|e| MultiAiError::Config(format!("Failed to get current directory: {}", e)))?;

    let (_config_path, project_config, project_path) = ProjectConfig::find_config(&current_dir)
        .map_err(|e| MultiAiError::Config(format!("Failed to find config: {}", e)))?
        .ok_or_else(|| MultiAiError::Config(
            "Config not found in ~/.config/multi-ai-cli/. Run 'mai init' from your project directory to create one.".to_string()
        ))?;

    let worktree_manager = make_worktree_manager(&project_config, project_path);
    let wt_dir = worktree_manager.worktrees_path();
    let groups = discover_all_prefixes(&worktree_manager, &project_config);

    if groups.is_empty() {
        println!("No worktrees found.");
        return Ok(());
    }

    // Collect groups with their most recent mtime
    let mut timed_groups: Vec<(String, Vec<String>, SystemTime)> = groups
        .into_iter()
        .map(|(prefix, worktrees)| {
            let most_recent = worktrees
                .iter()
                .filter_map(|wt| {
                    std::fs::metadata(wt_dir.join(wt))
                        .and_then(|m| m.modified())
                        .ok()
                })
                .max()
                .unwrap_or(SystemTime::UNIX_EPOCH);
            (prefix, worktrees, most_recent)
        })
        .collect();

    // Sort newest first
    timed_groups.sort_by(|a, b| b.2.cmp(&a.2));

    // Find max prefix length for alignment
    let max_prefix_len = timed_groups.iter().map(|(p, _, _)| p.len()).max().unwrap_or(0);

    for (prefix, worktrees, mtime) in &timed_groups {
        let time_str = format_relative_time(*mtime);
        let is_standalone = worktrees.len() == 1 && worktrees[0] == *prefix;
        if is_standalone {
            println!("{:<width$}  {}", prefix, time_str, width = max_prefix_len);
        } else {
            let slugs: Vec<&str> = worktrees
                .iter()
                .map(|wt| {
                    let suffix = format!("{}-", prefix);
                    wt.strip_prefix(&suffix).unwrap_or(wt.as_str())
                })
                .collect();
            println!(
                "{:<width$}  {}  {}",
                prefix,
                time_str,
                slugs.join(", "),
                width = max_prefix_len
            );
        }
    }

    Ok(())
}

fn config_command() -> Result<()> {
    let current_dir = std::env::current_dir()
        .map_err(|e| MultiAiError::Config(format!("Failed to get current directory: {}", e)))?;

    let (config_path, _, _) = ProjectConfig::find_config(&current_dir)
        .map_err(|e| MultiAiError::Config(format!("Failed to find config: {}", e)))?
        .ok_or_else(|| MultiAiError::Config(
            "Config not found in ~/.config/multi-ai-cli/. Run 'mai init' from your project directory to create one.".to_string()
        ))?;

    println!("Opening config: {}", config_path.display());

    #[cfg(target_os = "macos")]
    let opener = "open";
    #[cfg(not(target_os = "macos"))]
    let opener = "xdg-open";

    std::process::Command::new(opener)
        .arg(&config_path)
        .spawn()
        .map_err(|e| MultiAiError::Config(format!("Failed to open config file: {}", e)))?;

    Ok(())
}

fn apps_command() -> Result<()> {
    let config_dir = ProjectConfig::config_dir()
        .map_err(|e| MultiAiError::Config(format!("Could not determine config directory: {}", e)))?;

    let apps_path = config_dir.join("apps.jsonc");

    if !apps_path.exists() {
        std::fs::create_dir_all(&config_dir)?;
        std::fs::write(&apps_path, init::default_apps_content())?;
        println!("Created default apps config: {}", apps_path.display());
        println!(
            "Tip: Run 'make install' from the repo to symlink the repo's apps.jsonc instead."
        );
    } else {
        let metadata = std::fs::symlink_metadata(&apps_path);
        if let Ok(m) = metadata {
            if m.file_type().is_symlink() {
                if let Ok(target) = std::fs::read_link(&apps_path) {
                    println!("Opening apps config: {} -> {}", apps_path.display(), target.display());
                } else {
                    println!("Opening apps config: {} (symlink)", apps_path.display());
                }
            } else {
                println!("Opening apps config: {}", apps_path.display());
            }
        } else {
            println!("Opening apps config: {}", apps_path.display());
        }
    }

    #[cfg(target_os = "macos")]
    let opener = "open";
    #[cfg(not(target_os = "macos"))]
    let opener = "xdg-open";

    std::process::Command::new(opener)
        .arg(&apps_path)
        .spawn()
        .map_err(|e| MultiAiError::Config(format!("Failed to open apps config file: {}", e)))?;

    Ok(())
}

fn ask_confirmation(question: &str) -> Result<bool> {
    loop {
        print!("{} [y/n]: ", question);
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        match input.trim().to_lowercase().as_str() {
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => println!("Please enter 'y' or 'n'"),
        }
    }
}
