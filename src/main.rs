mod config;
mod error;
mod init;
mod iterm2;
mod tmux;
mod worktree;

use clap::Parser;
use config::{ProjectConfig, TerminalMode};
use error::{MultiAiError, Result};
use iterm2::ITerm2Manager;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
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

        #[arg(long, help = "Terminal mode: iterm2, tmux-multi-window, or tmux-single-window. Defaults to system default (iTerm2 on macOS, tmux-single-window on Linux) or config file setting")]
        mode: Option<String>,
    },

    #[command(about = "Remove worktrees and session for a branch prefix")]
    Remove {
        #[arg(help = "Branch prefix to remove")]
        branch_prefix: String,

        #[arg(long, help = "Terminal mode: iterm2, tmux-multi-window, or tmux-single-window. Defaults to system default or config file setting")]
        mode: Option<String>,
    },
}

fn main() -> Result<()> {
    let args = Args::parse();

    match args.command {
        Some(Command::Init) => {
            init::run_init()
        }
        Some(Command::Add { branch_prefix, mode }) => {
            create_command(branch_prefix, mode)
        }
        Some(Command::Remove { branch_prefix, mode }) => {
            remove_command(branch_prefix, mode)
        }
        None => {
            eprintln!("Error: Command required. Use 'mai add <branch-prefix>' or 'mai remove <branch-prefix>'");
            eprintln!("Run 'mai --help' for more information.");
            std::process::exit(1);
        }
    }
}

fn create_command(branch_prefix: String, mode_arg: Option<String>) -> Result<()> {
    let project_path = std::env::current_dir()
        .map_err(|e| MultiAiError::Config(format!("Failed to get current directory: {}", e)))?;

    // Check for multi-ai-config.jsonc in current directory
    let config_path = project_path.join("multi-ai-config.jsonc");
    if !config_path.exists() {
        return Err(MultiAiError::Config(
            "multi-ai-config.jsonc not found in current directory. Please run 'mai add' from a directory containing this file.".to_string()
        ));
    }

    // Check for git-worktree-config.jsonc in current directory
    let gwt_config_path = project_path.join("git-worktree-config.jsonc");
    if !gwt_config_path.exists() {
        return Err(MultiAiError::Config(
            "git-worktree-config.jsonc not found in current directory. Please ensure this file exists.".to_string()
        ));
    }

    let project_name = project_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| MultiAiError::Config("Invalid project path".to_string()))?
        .to_string();

    let project_config = load_project_config(&project_path)?;

    // Determine terminal mode: CLI > config > system default
    let terminal_mode = if let Some(mode_str) = mode_arg {
        TerminalMode::from_str(&mode_str)
            .ok_or_else(|| MultiAiError::Config(format!(
                "Invalid terminal mode: '{}'. Valid options: iterm2, tmux-multi-window, tmux-single-window",
                mode_str
            )))?
    } else if let Some(mode) = project_config.terminal_mode {
        mode
    } else {
        TerminalMode::system_default()
    };

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
            println!("  Creating worktree for {} with branch '{}'...", ai_app_clone.as_str(), branch_name);
            
            let worktree_manager = WorktreeManager::new(project_path_clone);
            match worktree_manager.add_worktree(&branch_name) {
                Ok(worktree_path) => {
                    println!("  ✓ Created worktree for {}: {}", ai_app_clone.as_str(), worktree_path.display());
                    let mut paths = worktree_paths_clone.lock().unwrap();
                    paths.push((ai_app_clone, worktree_path.to_string_lossy().to_string()));
                }
                Err(e) => {
                    eprintln!("  ✗ Failed to create worktree for {}: {}", ai_app_clone.as_str(), e);
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
        project_config.ai_apps.iter().position(|app| app.name == a.0.name).unwrap_or(0)
    });
    
    println!("✓ All worktrees created successfully!");

    // Validate terminal mode for platform
    #[cfg(not(target_os = "macos"))]
    if terminal_mode == TerminalMode::Iterm2 {
        return Err(MultiAiError::Config(
            "iTerm2 mode is only available on macOS. Please use --mode tmux-multi-window or --mode tmux-single-window on Linux.".to_string()
        ));
    }

    match terminal_mode {
        TerminalMode::Iterm2 => {
            let iterm2_manager = ITerm2Manager::new(&project_name, &branch_prefix, project_config.terminals_per_column);

            println!("\nCreating iTerm2 tabs for AI applications...");
            println!("  Apps to create tabs for: {:?}", worktree_paths.iter().map(|(app, _)| app.as_str()).collect::<Vec<_>>());
            println!("  Terminals per column: {}", project_config.terminals_per_column);
            match iterm2_manager.create_tabs_per_app(&project_config.ai_apps, &worktree_paths) {
                Ok(_) => println!("✓ iTerm2 tabs created successfully!"),
                Err(e) => {
                    eprintln!("✗ Failed to create iTerm2 tabs: {}", e);
                    return Err(e);
                }
            }
        }
        TerminalMode::TmuxMultiWindow | TerminalMode::TmuxSingleWindow => {
            let layout = match terminal_mode {
                TerminalMode::TmuxMultiWindow => tmux::TmuxLayout::MultiWindow,
                TerminalMode::TmuxSingleWindow => tmux::TmuxLayout::SingleWindow,
                _ => unreachable!(),
            };
            let tmux_manager = TmuxManager::new(&project_name, &branch_prefix, layout);

            println!("\nCreating tmux session '{}-{}' with {:?} mode...", project_name, branch_prefix, terminal_mode);
            tmux_manager.create_session(&project_config.ai_apps, &worktree_paths)?;

            println!("✓ Tmux session created successfully!");
            println!("\nAttaching to session...");
            tmux_manager.attach_session()?;
        }
    }

    Ok(())
}

fn remove_command(branch_prefix: String, mode_arg: Option<String>) -> Result<()> {
    let project_path = std::env::current_dir()
        .map_err(|e| MultiAiError::Config(format!("Failed to get current directory: {}", e)))?;

    // Check for multi-ai-config.jsonc in current directory
    let config_path = project_path.join("multi-ai-config.jsonc");
    if !config_path.exists() {
        return Err(MultiAiError::Config(
            "multi-ai-config.jsonc not found in current directory. Please run 'mai remove' from a directory containing this file.".to_string()
        ));
    }

    // Check for git-worktree-config.jsonc in current directory
    let gwt_config_path = project_path.join("git-worktree-config.jsonc");
    if !gwt_config_path.exists() {
        return Err(MultiAiError::Config(
            "git-worktree-config.jsonc not found in current directory. Please ensure this file exists.".to_string()
        ));
    }

    let project_name = project_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| MultiAiError::Config("Invalid project path".to_string()))?
        .to_string();

    let project_config = load_project_config(&project_path)?;

    // Determine terminal mode: CLI > config > system default
    let terminal_mode = if let Some(mode_str) = mode_arg {
        TerminalMode::from_str(&mode_str)
            .ok_or_else(|| MultiAiError::Config(format!(
                "Invalid terminal mode: '{}'. Valid options: iterm2, tmux-multi-window, tmux-single-window",
                mode_str
            )))?
    } else if let Some(mode) = project_config.terminal_mode {
        mode
    } else {
        TerminalMode::system_default()
    };
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
    match terminal_mode {
        TerminalMode::TmuxMultiWindow | TerminalMode::TmuxSingleWindow => {
            println!("  - Tmux session: {}-{}", project_name, branch_prefix);
        }
        TerminalMode::Iterm2 => {
            println!("  - Note: iTerm2 tabs must be closed manually");
        }
    }
    println!();

    if !ask_confirmation("Are you sure you want to remove these worktrees and session?")? {
        println!("Removal cancelled.");
        return Ok(());
    }

    match terminal_mode {
        TerminalMode::TmuxMultiWindow | TerminalMode::TmuxSingleWindow => {
            // Kill tmux session - layout doesn't matter for removal
            let layout = match terminal_mode {
                TerminalMode::TmuxMultiWindow => tmux::TmuxLayout::MultiWindow,
                TerminalMode::TmuxSingleWindow => tmux::TmuxLayout::SingleWindow,
                _ => unreachable!(),
            };
            let tmux_manager = TmuxManager::new(&project_name, &branch_prefix, layout);
            println!("Removing tmux session '{}-{}'...", project_name, branch_prefix);
            match tmux_manager.kill_session() {
                Ok(_) => println!("  ✓ Tmux session removed"),
                Err(e) => eprintln!("  ⚠ Failed to remove tmux session: {}", e),
            }
        }
        TerminalMode::Iterm2 => {
            // For iTerm2, we can't programmatically close tabs, just notify the user
            println!("Please manually close the iTerm2 tabs for '{}-{}'", project_name, branch_prefix);
        }
    }

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

fn load_project_config(project_path: &Path) -> Result<ProjectConfig> {
    // Only look for .jsonc
    let config_path = project_path.join("multi-ai-config.jsonc");
    
    if !config_path.exists() {
        return Err(MultiAiError::Config(
            "multi-ai-config.jsonc not found in current directory. Please create this file first.".to_string()
        ));
    }

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