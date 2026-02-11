use std::path::{Path, PathBuf};
use std::process::Command;

/// Get the remote origin URL for a git repository
pub fn get_remote_origin_url(path: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(path)
        .output()
        .ok()?;

    if output.status.success() {
        let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if url.is_empty() {
            None
        } else {
            Some(url)
        }
    } else {
        None
    }
}

/// Generate a safe filename from a git remote URL
///
/// Examples:
/// - `git@github.com:owner/repo.git` -> `github_owner_repo`
/// - `https://github.com/owner/repo.git` -> `github_owner_repo`
/// - `https://github.com/owner/repo` -> `github_owner_repo`
pub fn generate_config_filename(repo_url: &str) -> String {
    let url = repo_url.trim();

    // Remove common prefixes
    let url = url
        .strip_prefix("git@")
        .or_else(|| url.strip_prefix("https://"))
        .or_else(|| url.strip_prefix("http://"))
        .or_else(|| url.strip_prefix("ssh://git@"))
        .unwrap_or(url);

    // Remove .git suffix
    let url = url.strip_suffix(".git").unwrap_or(url);

    // Replace special characters with underscores
    let safe_name: String = url
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();

    // Collapse multiple underscores and trim
    let mut result = String::new();
    let mut prev_underscore = false;
    for c in safe_name.chars() {
        if c == '_' {
            if !prev_underscore && !result.is_empty() {
                result.push(c);
            }
            prev_underscore = true;
        } else {
            result.push(c);
            prev_underscore = false;
        }
    }

    // Remove trailing underscore
    while result.ends_with('_') {
        result.pop();
    }

    result
}

/// Get the top-level directory of the git repository.
/// Works from within worktrees as well.
pub fn get_repo_root(path: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(path)
        .output()
        .ok()?;

    if output.status.success() {
        let toplevel = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if toplevel.is_empty() {
            None
        } else {
            Some(PathBuf::from(toplevel))
        }
    } else {
        None
    }
}

/// A git branch with its name and last commit date.
#[derive(Clone)]
pub struct BranchInfo {
    pub name: String,
    pub date: String,
    /// True when this branch only exists on a remote (not checked out locally).
    pub remote_only: bool,
}

/// List local git branches sorted by most recent commit date (descending).
/// Returns branch names and relative commit dates.
pub fn list_local_branches(path: &Path) -> Vec<BranchInfo> {
    let output = Command::new("git")
        .args([
            "branch",
            "--sort=-committerdate",
            "--format=%(refname:short)\t%(committerdate:relative)",
        ])
        .current_dir(path)
        .output();

    match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|line| {
                let (name, date) = line.split_once('\t')?;
                Some(BranchInfo {
                    name: name.to_string(),
                    date: date.to_string(),
                    remote_only: false,
                })
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// List all branches (local + remote) sorted by most recent commit date.
/// Remote branches that have a local counterpart are excluded (local wins).
/// Fetches from origin first to ensure the list is up-to-date.
pub fn list_all_branches(path: &Path) -> Vec<BranchInfo> {
    // Fetch latest refs from origin (best-effort, don't fail if offline)
    let _ = Command::new("git")
        .args(["fetch", "--prune"])
        .current_dir(path)
        .output();

    let local = list_local_branches(path);
    let local_names: std::collections::HashSet<&str> =
        local.iter().map(|b| b.name.as_str()).collect();

    // List remote branches (origin only)
    let output = Command::new("git")
        .args([
            "branch",
            "-r",
            "--sort=-committerdate",
            "--format=%(refname:short)\t%(committerdate:relative)",
        ])
        .current_dir(path)
        .output();

    let mut remote: Vec<BranchInfo> = match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|line| {
                let (full_name, date) = line.split_once('\t')?;
                // Strip "origin/" prefix; skip HEAD pointer
                let short = full_name.strip_prefix("origin/")?;
                if short == "HEAD" {
                    return None;
                }
                // Skip if a local branch with the same name exists
                if local_names.contains(short) {
                    return None;
                }
                Some(BranchInfo {
                    name: short.to_string(),
                    date: date.to_string(),
                    remote_only: true,
                })
            })
            .collect(),
        _ => Vec::new(),
    };

    // Build a properly sorted unified list using git for-each-ref
    let combined_output = Command::new("git")
        .args([
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname:short)\t%(committerdate:relative)",
            "refs/heads/",
            "refs/remotes/origin/",
        ])
        .current_dir(path)
        .output();

    if let Ok(out) = combined_output {
        if out.status.success() {
            let mut seen = std::collections::HashSet::new();
            let mut sorted = Vec::new();
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                if let Some((full_name, date)) = line.split_once('\t') {
                    let short = full_name.strip_prefix("origin/").unwrap_or(full_name);
                    if short == "HEAD" {
                        continue;
                    }
                    if seen.contains(short) {
                        continue;
                    }
                    seen.insert(short.to_string());
                    let is_remote = full_name.starts_with("origin/") && !local_names.contains(short);
                    sorted.push(BranchInfo {
                        name: short.to_string(),
                        date: date.to_string(),
                        remote_only: is_remote,
                    });
                }
            }
            return sorted;
        }
    }

    // Fallback: concatenate local + remote without re-sorting
    let mut all = local;
    all.append(&mut remote);
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_config_filename_ssh() {
        assert_eq!(
            generate_config_filename("git@github.com:owner/repo.git"),
            "github_com_owner_repo"
        );
    }

    #[test]
    fn test_generate_config_filename_https() {
        assert_eq!(
            generate_config_filename("https://github.com/owner/repo.git"),
            "github_com_owner_repo"
        );
    }

    #[test]
    fn test_generate_config_filename_no_git_suffix() {
        assert_eq!(
            generate_config_filename("https://github.com/owner/repo"),
            "github_com_owner_repo"
        );
    }

    #[test]
    fn test_generate_config_filename_gitlab() {
        assert_eq!(
            generate_config_filename("git@gitlab.com:group/subgroup/project.git"),
            "gitlab_com_group_subgroup_project"
        );
    }
}
