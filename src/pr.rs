//! Best-effort discovery of open pull requests (and their linked issues/tickets)
//! via the GitHub CLI (`gh`). Used to pre-fill review context links so AI
//! reviewers can read the PR description and associated ticket before reviewing.

use serde::Deserialize;
use std::path::Path;
use std::process::Command;

/// An open PR discovered for the repository.
#[derive(Debug, Clone)]
pub struct PrInfo {
    /// Head branch name of the PR (matches local/remote branch names).
    pub branch: String,
    pub url: String,
    pub title: String,
    /// Issue/ticket URLs linked to the PR: GitHub closing issues plus any
    /// issue-tracker links (GitHub issues, Jira, Linear) found in the PR body.
    pub issue_urls: Vec<String>,
}

#[derive(Deserialize)]
struct GhPr {
    #[serde(rename = "headRefName")]
    head_ref_name: String,
    url: String,
    title: String,
    #[serde(default)]
    body: String,
    #[serde(default, rename = "closingIssuesReferences")]
    closing_issues_references: Vec<GhIssueRef>,
}

#[derive(Deserialize)]
struct GhIssueRef {
    url: String,
}

/// List open PRs for the repository at `project_path`.
/// Best-effort: returns an empty list if `gh` is missing, the repo is not on
/// GitHub, the user is not authenticated, or the output can't be parsed.
pub fn list_open_prs(project_path: &Path) -> Vec<PrInfo> {
    let output = Command::new("gh")
        .args([
            "pr",
            "list",
            "--state",
            "open",
            "--limit",
            "200",
            "--json",
            "headRefName,url,title,body,closingIssuesReferences",
        ])
        .current_dir(project_path)
        .output();

    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    let Ok(prs) = serde_json::from_slice::<Vec<GhPr>>(&output.stdout) else {
        return Vec::new();
    };

    prs.into_iter()
        .map(|pr| {
            let mut issue_urls: Vec<String> = pr
                .closing_issues_references
                .into_iter()
                .map(|i| i.url)
                .collect();
            for url in extract_issue_urls(&pr.body) {
                if !issue_urls.contains(&url) {
                    issue_urls.push(url);
                }
            }
            PrInfo {
                branch: pr.head_ref_name,
                url: pr.url,
                title: pr.title,
                issue_urls,
            }
        })
        .collect()
}

/// Find the open PR whose head branch matches `branch`.
pub fn find_pr_for_branch<'a>(prs: &'a [PrInfo], branch: &str) -> Option<&'a PrInfo> {
    prs.iter().find(|pr| pr.branch == branch)
}

/// Extract issue-tracker URLs (GitHub issues, Jira, Linear) from free text
/// such as a PR description.
fn extract_issue_urls(text: &str) -> Vec<String> {
    let mut urls = Vec::new();
    for token in text.split(|c: char| c.is_whitespace() || matches!(c, '(' | '<' | '[')) {
        let url = token.trim_end_matches(|c| {
            matches!(
                c,
                ')' | '>' | ']' | '.' | ',' | ';' | ':' | '"' | '\'' | '*' | '`'
            )
        });
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            continue;
        }
        let is_issue_link = url.contains("linear.app/")
            || url.contains("atlassian.net/browse/")
            || (url.contains("github.com/") && url.contains("/issues/"));
        if is_issue_link && !urls.iter().any(|u| u == url) {
            urls.push(url.to_string());
        }
    }
    urls
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_issue_urls_from_markdown_body() {
        let body = "Fixes [SAMP-117](https://acme.atlassian.net/browse/SAMP-117).\n\
                    See also <https://github.com/acme/app/issues/42> and\n\
                    https://linear.app/acme/issue/ENG-123/fix-the-thing.";
        let urls = extract_issue_urls(body);
        assert_eq!(
            urls,
            vec![
                "https://acme.atlassian.net/browse/SAMP-117",
                "https://github.com/acme/app/issues/42",
                "https://linear.app/acme/issue/ENG-123/fix-the-thing",
            ]
        );
    }

    #[test]
    fn ignores_non_issue_links_and_dedupes() {
        let body = "Docs: https://example.com/docs\n\
                    https://github.com/acme/app/issues/7\n\
                    again https://github.com/acme/app/issues/7\n\
                    PR link https://github.com/acme/app/pull/8";
        let urls = extract_issue_urls(body);
        assert_eq!(urls, vec!["https://github.com/acme/app/issues/7"]);
    }
}
