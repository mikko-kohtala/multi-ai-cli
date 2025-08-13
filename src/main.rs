mod config;
mod error;
mod tmux;
mod worktree;

use clap::Parser;
use config::{ProjectConfig, UserConfig};
use error::{MultiAiError, Result};
use std::fs;
use std::path::Path;
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
    
    #[arg(help = "Project directory name (e.g., 'kuntoon')")]
    project: Option<String>,

    #[arg(help = "Branch prefix (e.g., 'vercel-theme')")]
    branch_prefix: Option<String>,
}

#[derive(Parser, Debug)]
enum Command {
    #[command(about = "Create worktrees and tmux session for multiple AI tools")]
    Create {
        #[arg(help = "Project directory name")]
        project: String,
        
        #[arg(help = "Branch prefix for the worktrees")]
        branch_prefix: String,
    },
    
    #[command(about = "Remove worktrees and tmux session for a branch prefix")]
    Remove {
        #[arg(help = "Project directory name")]
        project: String,
        
        #[arg(help = "Branch prefix to remove")]
        branch_prefix: String,
    },
}

fn main() -> Result<()> {
    let args = Args::parse();

    match args.command {
        Some(Command::Create { project, branch_prefix }) => {
            create_command(project, branch_prefix)
        }
        Some(Command::Remove { project, branch_prefix }) => {
            remove_command(project, branch_prefix)
        }
        None => {
            // Default create command for backwards compatibility
            let project = args.project.ok_or_else(|| {
                MultiAiError::Config("Project name is required. Use 'multi-ai --help' for usage information.".to_string())
            })?;
            let branch_prefix = args.branch_prefix.ok_or_else(|| {
                MultiAiError::Config("Branch prefix is required. Use 'multi-ai --help' for usage information.".to_string())
            })?;
            create_command(project, branch_prefix)
        }
    }
}

fn create_command(project: String, branch_prefix: String) -> Result<()> {
    let user_config = load_user_config()?;
    
    let project_path = user_config.expand_path().join(&project);
    
    if !project_path.exists() {
        return Err(MultiAiError::ProjectNotFound(format!(
            "Project '{}' not found at '{}'",
            project,
            project_path.display()
        )));
    }

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
            project
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

    let tmux_manager = TmuxManager::new(&project, &branch_prefix);
    
    println!("\nCreating tmux session '{}-{}'...", project, branch_prefix);
    tmux_manager.create_session(&project_config.ai_apps, &worktree_paths)?;
    
    println!("✓ Tmux session created successfully!");
    println!("\nAttaching to session...");
    tmux_manager.attach_session()?;

    Ok(())
}

fn remove_command(project: String, branch_prefix: String) -> Result<()> {
    let user_config = load_user_config()?;
    
    let project_path = user_config.expand_path().join(&project);
    
    if !project_path.exists() {
        return Err(MultiAiError::ProjectNotFound(format!(
            "Project '{}' not found at '{}'",
            project,
            project_path.display()
        )));
    }

    let project_config = load_project_config(&project_path)?;
    let worktree_manager = WorktreeManager::new(project_path.clone());
    
    if !worktree_manager.has_gwt_cli() {
        return Err(MultiAiError::Worktree(
            "gwt CLI is not installed. Please install from https://github.com/mikko-kohtala/git-worktree-cli".to_string()
        ));
    }

    // Kill tmux session first
    let tmux_manager = TmuxManager::new(&project, &branch_prefix);
    println!("Removing tmux session '{}-{}'...", project, branch_prefix);
    match tmux_manager.kill_session() {
        Ok(_) => println!("  ✓ Tmux session removed"),
        Err(e) => eprintln!("  ⚠ Failed to remove tmux session: {}", e),
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

fn load_user_config() -> Result<UserConfig> {
    let config_path = UserConfig::config_path();
    
    if !config_path.exists() {
        return Err(MultiAiError::Config(format!(
            "User configuration not found at '{}'. Please create it with your code_root path.",
            config_path.display()
        )));
    }

    let content = fs::read_to_string(&config_path)
        .map_err(|e| MultiAiError::Config(format!("Failed to read user config: {}", e)))?;
    
    UserConfig::from_json(&content)
        .map_err(|e| MultiAiError::Config(format!("Failed to parse user config: {}", e)))
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
