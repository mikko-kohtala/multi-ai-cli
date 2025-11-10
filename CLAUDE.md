# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Multi-AI CLI is a Rust tool that manages multiple AI development environments using git worktrees and tmux sessions. It automates the setup of separate worktrees for different AI tools and creates organized tmux or iTerm2 sessions for each.

### Supported AI Tools

The following AI development tools are supported:
- **claude**: Anthropic's AI assistant (with `--dangerously-skip-permissions` flag for YOLO mode)
- **gemini**: Google's AI assistant (with `--yolo` flag for YOLO mode)
- **codex**: GitHub Copilot's AI assistant (with `--ask-for-approval never` flag for YOLO mode)
- **amp**: AI assistant (with `--dangerously-allow-all` flag for YOLO mode)
- **opencode**: AI coding assistant (no special flags for YOLO mode)
- **cursor-agent**: Cursor AI assistant (with `--force` flag for YOLO mode)

**IMPORTANT**: When adding new AI tools, always update:
1. `src/init.rs` - Add to AiService::SERVICES array
2. `CLAUDE.md` - Add to this supported tools list
3. `README.md` - Update the AI tools list
4. `Cargo.toml` - Increment the version number

## Version Management

**IMPORTANT**: Whenever making code changes, always increment the version in Cargo.toml:
- Patch version (x.x.N) for bug fixes and minor improvements
- Minor version (x.N.x) for new features
- Major version (N.x.x) for breaking changes

## Common Commands

### Build
```bash
cargo build           # Debug build
cargo build --release # Release build for production use
```

### Run

**Config File Locations**: Config files can be placed in either:
- Current directory (checked first)
- `./main/` subdirectory (checked second if not found in current directory)

This allows you to keep config files version-controlled in a `./main/` subdirectory while maintaining worktrees at the repo root level.

```bash
# From a directory with the required config files:
cargo run -- add <branch-prefix>             # Create worktrees and session
cargo run -- add <branch-prefix> --tmux      # Use tmux instead of iTerm2
cargo run -- remove <branch-prefix>          # Remove worktrees and session
cargo run -- remove <branch-prefix> --tmux   # Remove tmux session
cargo run -- continue <branch-prefix>        # Create new session/tab for existing worktrees
cargo run -- resume <branch-prefix>          # Alias for continue

# Or using the binary:
mai add <branch-prefix>                      # Create worktrees and session
mai add <branch-prefix> --tmux               # Use tmux instead of iTerm2
mai remove <branch-prefix>                   # Remove worktrees and session
mai remove <branch-prefix> --tmux            # Remove tmux session
mai continue <branch-prefix>                 # Create new session/tab for existing worktrees
mai resume <branch-prefix>                   # Alias for continue

# Initialize a new config file:
mai init                                      # Interactive setup of multi-ai-config.jsonc
```

### Test
```bash
cargo test              # Run all tests
cargo test <test_name>  # Run specific test
cargo test -- --nocapture # Show test output
```

### Check & Lint
```bash
cargo check    # Quick compilation check without producing binary
cargo clippy   # Rust linter with additional checks
cargo fmt      # Format code according to Rust standards
```

## Architecture

### Core Flow
1. **main.rs**: Entry point, handles CLI argument parsing via clap, orchestrates the add/remove commands
   - Commands work from current directory, no project path argument needed
   - Validates presence of both `multi-ai-config.jsonc` and `git-worktree-config.jsonc` in current directory or `./main/` subdirectory
2. **config.rs**: Manages project configuration:
   - `ProjectConfig`: Reads `multi-ai-config.jsonc` for AI apps list and `mode`
   - `Mode`: enum for `iterm2`, `tmux-single-window`, `tmux-multi-window` (optional; defaults: macOS → iterm2, others → tmux-single-window)
   - `TmuxLayout`: internal enum used by tmux adapter (`SingleWindow`, `MultiWindow`)
   - `AiApp` struct: Defines AI tool name and full command to execute

3. **worktree.rs**: `WorktreeManager` interfaces with gwt CLI to:
   - Create git worktrees for each AI app with naming pattern: `<branch-prefix>-<ai-app>`
   - Validate gwt CLI availability and project initialization
   - Check for `git-worktree-config.jsonc` (or `.yaml` for backward compatibility)
   - Remove worktrees during cleanup

4. **iterm2.rs**: `ITerm2Manager` handles iTerm2 automation (default):
   - Creates a single tab with all AI apps
   - Each AI app gets horizontal split (top/bottom panes)
   - Commands use `cd <path> && <command>` chaining for proper directory navigation
   - Top pane launches the AI tool with custom command
   - Bottom pane provides shell in worktree directory

5. **tmux.rs**: `TmuxManager` handles tmux automation (with --tmux flag or config):
   - Creates session named `<project>-<branch-prefix>`
   - Supports two layouts:
     - `tmux-multi-window`: one window per AI app, each split into two panes (left: AI, right: shell)
     - `tmux-single-window`: single window `apps` with equal-width columns per app, each column split into two panes (top: AI, bottom: shell)
   - Launch pane: original pane per app (left for multi_window, top for single_window) runs the AI tool (500ms delay before sending)
   - Pane targeting uses `#{pane_id}` captured pre-split to avoid index assumptions

6. **error.rs**: Custom error types using thiserror for structured error handling

### Key Implementation Details

- **Current Directory Usage**: Commands must be run from directory containing config files
- **Required Files**: Both `multi-ai-config.jsonc` and `git-worktree-config.jsonc` must exist (current directory or `./main/` subdirectory)
- **Config File Search**: Config files are searched in two locations:
  1. Current directory (checked first)
  2. `./main/` subdirectory (checked if not found in current directory)
  - This allows keeping configs in git while maintaining worktrees at repo root
  - `project_path` always remains the current directory (repo root), regardless of config location
- **Tmux Pane Targeting**: Capture `#{pane_id}` of the original pane before splitting and target by ID. This works regardless of `base-index`/`pane-base-index`.
- **Mode Defaults by OS**: If not specified via CLI or config, defaults to iTerm2 on macOS and tmux single-window elsewhere.
- **Shell Initialization**: A 500ms delay ensures the shell is ready before sending commands
- **JSONC Support**: Configuration files use JSONC format (JSON with comments)

### Dependencies
- External tools: gwt CLI, tmux
- Key crates: clap (CLI), serde (serialization), jsonc-parser (JSONC support), thiserror (errors)

## Known Issues & Fixes

### Shell Initialization Timing
Fixed by adding 500ms delay after pane creation to ensure shell is ready before sending commands.
