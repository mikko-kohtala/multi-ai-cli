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
#[command(about = "Multi-AI workspace manager with git worktrees", long_about = None)]
struct Args {
    #[arg(help = "Project directory name (e.g., 'kuntoon')")]
    project: String,

    #[arg(help = "Branch prefix (e.g., 'vercel-theme')")]
    branch_prefix: String,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let user_config = load_user_config()?;
    
    let project_path = user_config.expand_path().join(&args.project);
    
    if !project_path.exists() {
        return Err(MultiAiError::ProjectNotFound(format!(
            "Project '{}' not found at '{}'",
            args.project,
            project_path.display()
        )));
    }

    let project_config = load_project_config(&project_path)?;

    let worktree_manager = WorktreeManager::new(project_path.clone());
    
    if !worktree_manager.is_git_repo() {
        return Err(MultiAiError::Worktree(format!(
            "Project '{}' is not a git repository",
            args.project
        )));
    }

    if !worktree_manager.has_gwt_cli() {
        return Err(MultiAiError::Worktree(
            "gwt CLI is not installed. Please install from https://github.com/mikko-kohtala/git-worktree-cli".to_string()
        ));
    }

    let mut worktree_paths = Vec::new();
    
    for ai_app in &project_config.ai_apps {
        let branch_name = format!("{}-{}", ai_app.as_str(), args.branch_prefix);
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

    let tmux_manager = TmuxManager::new(&args.project, &args.branch_prefix);
    
    println!("\nCreating tmux session '{}-{}'...", args.project, args.branch_prefix);
    tmux_manager.create_session(&project_config.ai_apps, &worktree_paths)?;
    
    println!("✓ Tmux session created successfully!");
    println!("\nAttaching to session...");
    tmux_manager.attach_session()?;

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
