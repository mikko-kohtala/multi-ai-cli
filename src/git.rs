use std::path::Path;
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
