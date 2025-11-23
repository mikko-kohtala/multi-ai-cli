mod config;
mod error;
mod init;
#[cfg(target_os = "macos")]
mod iterm2;
mod send;
mod tmux;
mod worktree;

use clap::{Parser, ValueEnum};
use config::{Mode, ProjectConfig, TmuxLayout};
use error::{MultiAiError, Result};
#[cfg(target_os = "macos")]
use iterm2::ITerm2Manager;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
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
        #[arg(help = "Branch prefix for the worktrees")]
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

    #[command(about = "Remove worktrees and session for a branch prefix")]
    Remove {
        #[arg(help = "Branch prefix to remove")]
        branch_prefix: String,

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
        }) => create_command(branch_prefix, tmux, mode),
        Some(Command::Remove {
            branch_prefix,
            tmux,
            mode,
            force,
        }) => remove_command(branch_prefix, tmux, mode, force),
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

/// Find a config file by checking current directory first, then ./main/ subdirectory
fn find_config_file(base_path: &Path, filename: &str) -> Option<PathBuf> {
    // First check current directory
    let current_path = base_path.join(filename);
    if current_path.exists() {
        return Some(current_path);
    }

    // Then check ./main/ subdirectory
    let main_path = base_path.join("main").join(filename);
    if main_path.exists() {
        return Some(main_path);
    }

    None
}

fn create_command(
    branch_prefix: String,
    cli_tmux: bool,
    mode_override: Option<ModeOverride>,
) -> Result<()> {
    let project_path = std::env::current_dir()
        .map_err(|e| MultiAiError::Config(format!("Failed to get current directory: {}", e)))?;

    // Check for multi-ai-config.jsonc (current directory or ./main/ subdirectory)
    let _config_path = find_config_file(&project_path, "multi-ai-config.jsonc")
        .ok_or_else(|| MultiAiError::Config(
            "multi-ai-config.jsonc not found in current directory or ./main/ subdirectory. Please run 'mai add' from a directory containing this file.".to_string()
        ))?;

    // Check for git-worktree-config.jsonc (current directory or ./main/ subdirectory)
    let _gwt_config_path = find_config_file(&project_path, "git-worktree-config.jsonc")
        .ok_or_else(|| MultiAiError::Config(
            "git-worktree-config.jsonc not found in current directory or ./main/ subdirectory. Please ensure this file exists.".to_string()
        ))?;

    let project_name = project_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| MultiAiError::Config("Invalid project path".to_string()))?
        .to_string();

    let project_config = load_project_config(&project_path)?;

    let worktree_manager = WorktreeManager::new(project_path.clone());

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

    // Create worktrees in parallel
    println!("Creating worktrees in parallel...");
    let worktree_paths = Arc::new(Mutex::new(Vec::new()));
    let errors = Arc::new(Mutex::new(Vec::new()));

    let mut handles = vec![];

    for ai_app in &project_config.ai_apps {
        let branch_name = format!("{}-{}", branch_prefix, ai_app.as_str());
        let ai_app_clone = ai_app.clone();
        let project_path_clone = project_path.clone();
        let worktree_paths_clone = Arc::clone(&worktree_paths);
        let errors_clone = Arc::clone(&errors);

        let handle = thread::spawn(move || {
            println!(
                "  Creating worktree for {} with branch '{}'...",
                ai_app_clone.as_str(),
                branch_name
            );

            let worktree_manager = WorktreeManager::new(project_path_clone);
            match worktree_manager.add_worktree(&branch_name) {
                Ok(worktree_path) => {
                    println!(
                        "  ✓ Created worktree for {}: {}",
                        ai_app_clone.as_str(),
                        worktree_path.display()
                    );
                    let mut paths = worktree_paths_clone.lock().unwrap();
                    paths.push((ai_app_clone, worktree_path.to_string_lossy().to_string()));
                }
                Err(e) => {
                    eprintln!(
                        "  ✗ Failed to create worktree for {}: {}",
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
        project_config
            .ai_apps
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
                match iterm2_manager.create_tabs_per_app(&project_config.ai_apps, &worktree_paths) {
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
            tmux_manager.create_session(&project_config.ai_apps, &worktree_paths, layout)?;
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
    let project_path = std::env::current_dir()
        .map_err(|e| MultiAiError::Config(format!("Failed to get current directory: {}", e)))?;

    // Check for multi-ai-config.jsonc (current directory or ./main/ subdirectory)
    let _config_path = find_config_file(&project_path, "multi-ai-config.jsonc")
        .ok_or_else(|| MultiAiError::Config(
            "multi-ai-config.jsonc not found in current directory or ./main/ subdirectory. Please run 'mai remove' from a directory containing this file.".to_string()
        ))?;

    // Check for git-worktree-config.jsonc (current directory or ./main/ subdirectory)
    let _gwt_config_path = find_config_file(&project_path, "git-worktree-config.jsonc")
        .ok_or_else(|| MultiAiError::Config(
            "git-worktree-config.jsonc not found in current directory or ./main/ subdirectory. Please ensure this file exists.".to_string()
        ))?;

    let project_name = project_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| MultiAiError::Config("Invalid project path".to_string()))?
        .to_string();

    let project_config = load_project_config(&project_path)?;
    let worktree_manager = WorktreeManager::new(project_path.clone());

    if !worktree_manager.has_gwt_cli() {
        return Err(MultiAiError::Worktree(
            "gwt CLI is not installed. Please install from https://github.com/mikko-kohtala/git-worktree-cli".to_string()
        ));
    }

    // Ask for confirmation
    println!("⚠️  You are about to remove:");
    println!("  - Worktrees for branches:");
    for ai_app in &project_config.ai_apps {
        let branch_name = format!("{}-{}", branch_prefix, ai_app.as_str());
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

    // Remove worktrees for each AI app
    for ai_app in &project_config.ai_apps {
        let branch_name = format!("{}-{}", branch_prefix, ai_app.as_str());
        println!("Removing worktree for branch '{}'...", branch_name);

        match worktree_manager.remove_worktree(&branch_name) {
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
    let project_path = std::env::current_dir()
        .map_err(|e| MultiAiError::Config(format!("Failed to get current directory: {}", e)))?;

    // Check for multi-ai-config.jsonc (current directory or ./main/ subdirectory)
    let _config_path = find_config_file(&project_path, "multi-ai-config.jsonc")
        .ok_or_else(|| MultiAiError::Config(
            "multi-ai-config.jsonc not found in current directory or ./main/ subdirectory. Please run 'mai continue' from a directory containing this file.".to_string()
        ))?;

    // Check for git-worktree-config.jsonc (current directory or ./main/ subdirectory)
    let _gwt_config_path = find_config_file(&project_path, "git-worktree-config.jsonc")
        .ok_or_else(|| MultiAiError::Config(
            "git-worktree-config.jsonc not found in current directory or ./main/ subdirectory. Please ensure this file exists.".to_string()
        ))?;

    let project_name = project_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| MultiAiError::Config("Invalid project path".to_string()))?
        .to_string();

    let project_config = load_project_config(&project_path)?;
    let worktree_manager = WorktreeManager::new(project_path.clone());

    // Check if worktrees exist
    let ai_app_names: Vec<String> = project_config
        .ai_apps
        .iter()
        .map(|app| app.name.clone())
        .collect();

    if !worktree_manager.worktrees_exist(&branch_prefix, &ai_app_names) {
        return Err(MultiAiError::Worktree(format!(
            "Worktrees for '{}' do not exist. Run 'mai add {}' first.",
            branch_prefix, branch_prefix
        )));
    }

    println!("✓ Found existing worktrees for '{}'", branch_prefix);

    // Build worktree paths list (without creating them)
    let worktree_paths: Vec<(config::AiApp, String)> = project_config
        .ai_apps
        .iter()
        .map(|ai_app| {
            let branch_name = format!("{}-{}", branch_prefix, ai_app.as_str());
            let worktree_path = project_path.join(&branch_name);
            (ai_app.clone(), worktree_path.to_string_lossy().to_string())
        })
        .collect();

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
                match iterm2_manager.create_tabs_per_app(&project_config.ai_apps, &worktree_paths) {
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
            tmux_manager.create_session(&project_config.ai_apps, &worktree_paths, layout)?;
            println!("✓ Tmux session created successfully!");
            println!("\nAttaching to session...");
            tmux_manager.attach_session()?;
        }
    }

    Ok(())
}

fn send_command() -> Result<()> {
    let project_path = std::env::current_dir()
        .map_err(|e| MultiAiError::Config(format!("Failed to get current directory: {}", e)))?;

    // Check for multi-ai-config.jsonc (current directory or ./main/ subdirectory)
    let _config_path = find_config_file(&project_path, "multi-ai-config.jsonc")
        .ok_or_else(|| MultiAiError::Config(
            "multi-ai-config.jsonc not found in current directory or ./main/ subdirectory. Please run 'mai send' from a directory containing this file.".to_string()
        ))?;

    let project_name = project_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| MultiAiError::Config("Invalid project path".to_string()))?
        .to_string();

    let project_config = load_project_config(&project_path)?;

    send::run_send_command(project_config, project_name)
}

fn load_project_config(project_path: &Path) -> Result<ProjectConfig> {
    // Look for .jsonc in current directory or ./main/ subdirectory
    let config_path = find_config_file(project_path, "multi-ai-config.jsonc")
        .ok_or_else(|| MultiAiError::Config(
            "multi-ai-config.jsonc not found in current directory or ./main/ subdirectory. Please create this file first."
                .to_string(),
        ))?;

    let content = fs::read_to_string(&config_path)
        .map_err(|e| MultiAiError::Config(format!("Failed to read project config: {}", e)))?;

    ProjectConfig::from_json(&content)
        .map_err(|e| MultiAiError::Config(format!("Failed to parse project config: {}", e)))
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
