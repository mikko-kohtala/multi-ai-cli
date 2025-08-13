mod config;
mod error;
mod iterm2;
mod tmux;
mod worktree;

use clap::Parser;
use config::ProjectConfig;
use error::{MultiAiError, Result};
use iterm2::ITerm2Manager;
use std::fs;
use std::path::{Path, PathBuf};
use tmux::TmuxManager;
use worktree::WorktreeManager;

#[derive(Parser, Debug)]
#[command(name = "multi-ai")]
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
    #[command(about = "Create worktrees and session for multiple AI tools")]
    Create {
        #[arg(help = "Path to project directory")]
        project_path: String,
        
        #[arg(help = "Branch prefix for the worktrees")]
        branch_prefix: String,
        
        #[arg(long, help = "Use tmux instead of iTerm2")]
        tmux: bool,
    },
    
    #[command(about = "Remove worktrees and session for a branch prefix")]
    Remove {
        #[arg(help = "Path to project directory")]
        project_path: String,
        
        #[arg(help = "Branch prefix to remove")]
        branch_prefix: String,
        
        #[arg(long, help = "Remove tmux session instead of iTerm2 tabs")]
        tmux: bool,
    },
}

fn main() -> Result<()> {
    let args = Args::parse();

    match args.command {
        Some(Command::Create { project_path, branch_prefix, tmux }) => {
            create_command(project_path, branch_prefix, tmux)
        }
        Some(Command::Remove { project_path, branch_prefix, tmux }) => {
            remove_command(project_path, branch_prefix, tmux)
        }
        None => {
            // Default create command for backwards compatibility
            let project_path = args.project_path.ok_or_else(|| {
                MultiAiError::Config("Project path is required. Use 'multi-ai --help' for usage information.".to_string())
            })?;
            let branch_prefix = args.branch_prefix.ok_or_else(|| {
                MultiAiError::Config("Branch prefix is required. Use 'multi-ai --help' for usage information.".to_string())
            })?;
            create_command(project_path, branch_prefix, false) // Default to iTerm2
        }
    }
}

fn create_command(project_path: String, branch_prefix: String, use_tmux: bool) -> Result<()> {
    let project_path = expand_path(&project_path);
    
    if !project_path.exists() {
        return Err(MultiAiError::ProjectNotFound(format!(
            "Project not found at '{}'",
            project_path.display()
        )));
    }

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
        return Err(MultiAiError::Worktree(format!(
            "Project '{}' is not initialized with gwt. Please run 'gwt init' in the project directory first.",
            project_name
        )));
    }

    let mut worktree_paths = Vec::new();
    
    for ai_app in &project_config.ai_apps {
        let branch_name = format!("{}-{}", branch_prefix, ai_app.as_str());
        println!("Creating worktree for {} with branch '{}'...", ai_app.as_str(), branch_name);
        
        match worktree_manager.add_worktree(&branch_name) {
            Ok(worktree_path) => {
                println!("  ✓ Created worktree at: {}", worktree_path.display());
                worktree_paths.push((ai_app.clone(), worktree_path.to_string_lossy().to_string()));
            }
            Err(e) => {
                eprintln!("  ✗ Failed to create worktree: {}", e);
                return Err(e);
            }
        }
    }

    if use_tmux {
        let tmux_manager = TmuxManager::new(&project_name, &branch_prefix);
        
        println!("\nCreating tmux session '{}-{}'...", project_name, branch_prefix);
        tmux_manager.create_session(&project_config.ai_apps, &worktree_paths)?;
        
        println!("✓ Tmux session created successfully!");
        println!("\nAttaching to session...");
        tmux_manager.attach_session()?;
    } else {
        let iterm2_manager = ITerm2Manager::new(&project_name, &branch_prefix);
        
        println!("\nCreating iTerm2 tabs for AI applications...");
        iterm2_manager.create_tabs_per_app(&project_config.ai_apps, &worktree_paths)?;
        
        println!("✓ iTerm2 tabs created successfully!");
    }

    Ok(())
}

fn remove_command(project_path: String, branch_prefix: String, use_tmux: bool) -> Result<()> {
    let project_path = expand_path(&project_path);
    
    if !project_path.exists() {
        return Err(MultiAiError::ProjectNotFound(format!(
            "Project not found at '{}'",
            project_path.display()
        )));
    }

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

    if use_tmux {
        // Kill tmux session
        let tmux_manager = TmuxManager::new(&project_name, &branch_prefix);
        println!("Removing tmux session '{}-{}'...", project_name, branch_prefix);
        match tmux_manager.kill_session() {
            Ok(_) => println!("  ✓ Tmux session removed"),
            Err(e) => eprintln!("  ⚠ Failed to remove tmux session: {}", e),
        }
    } else {
        // For iTerm2, we can't programmatically close tabs, just notify the user
        println!("Please manually close the iTerm2 tabs for '{}-{}'", project_name, branch_prefix);
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
    // Try .json first, then .jsonc
    let json_path = project_path.join("multi-ai-config.json");
    let jsonc_path = project_path.join("multi-ai-config.jsonc");
    
    let config_path = if json_path.exists() {
        json_path
    } else if jsonc_path.exists() {
        jsonc_path
    } else {
        return Err(MultiAiError::Config(format!(
            "Project configuration not found. Please create multi-ai-config.json or multi-ai-config.jsonc in the project root: {}",
            project_path.display()
        )));
    };

    let content = fs::read_to_string(&config_path)
        .map_err(|e| MultiAiError::Config(format!("Failed to read project config: {}", e)))?;
    
    ProjectConfig::from_json(&content)
        .map_err(|e| MultiAiError::Config(format!("Failed to parse project config: {}", e)))
}

fn expand_path(path: &str) -> PathBuf {
    PathBuf::from(shellexpand::tilde(path).to_string())
}