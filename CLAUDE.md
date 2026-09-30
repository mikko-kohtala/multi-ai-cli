# Multi-AI CLI

`mai`: a Rust CLI that sets up one git worktree per AI tool and opens them side by side in iTerm2 or tmux. Usage and config: README.md.

## Project workflow

Before making changes, read and follow `.agents/skills/project-workflow/SKILL.md` from the repository root.

## Adding AI tools

When adding a new AI tool, always update:
1. `apps.jsonc` - Add the tool and its command variants
2. `README.md` - Update the AI tools list
3. `Cargo.toml` - Increment the version number

## Version Management

**IMPORTANT**: Whenever making code changes, always increment the version in Cargo.toml:
- Patch version (x.x.N) for bug fixes and minor improvements
- Minor version (x.N.x) for new features
- Major version (N.x.x) for breaking changes

## Validation
Validate all work with `make check` (fmt, clippy, tests) before calling it done. `make fmt` applies formatting.

## Gotchas

- Configs live outside the repo in `~/.config/multi-ai-cli/`, one `.jsonc` per project named from the git remote URL (e.g. `github_com_owner_repo.jsonc`), each with a required `project_path`. Fallback: a scan for a matching `project_path`/`worktrees_path`.
- The repo's `apps.jsonc` is compiled in as the default. At runtime `~/.config/multi-ai-cli/apps.jsonc` wins; `make install` symlinks it to the repo copy unless a real file is already there.
- Tmux: capture `#{pane_id}` of the original pane before splitting and target panes by ID, never by index, so it works regardless of `base-index`/`pane-base-index`.
- Tmux: new panes need a 500ms delay before commands are sent, or the shell isn't ready.
