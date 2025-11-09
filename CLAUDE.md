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
**IMPORTANT**: As of v0.9.0, `mai add` and `mai remove` must be run from a directory containing both `multi-ai-config.jsonc` and `git-worktree-config.jsonc` files.

```bash
# From a directory with the required config files:
cargo run -- add <branch-prefix>                               # Use system default (iTerm2 on macOS, tmux-single-window on Linux)
cargo run -- add <branch-prefix> --mode iterm2                 # Use iTerm2 (macOS only)
cargo run -- add <branch-prefix> --mode tmux-multi-window      # Use tmux with separate windows per AI app
cargo run -- add <branch-prefix> --mode tmux-single-window     # Use tmux with all apps in one window
cargo run -- remove <branch-prefix>                            # Remove worktrees and session
cargo run -- remove <branch-prefix> --mode tmux-single-window  # Specify mode for removal

# Or using the binary:
mai add <branch-prefix>                          # Use system default or config file setting
mai add <branch-prefix> --mode iterm2            # iTerm2 mode (macOS only)
mai add <branch-prefix> --mode tmux-multi-window # Tmux: one window per AI app
mai add <branch-prefix> --mode tmux-single-window# Tmux: all apps in one window (columns)
mai remove <branch-prefix>                       # Remove worktrees and session
mai remove <branch-prefix> --mode tmux-multi-window # Specify mode for removal

# Initialize a new config file:
mai init                                         # Interactive setup of multi-ai-config.jsonc
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
   - As of v0.9.0: Commands work from current directory, no project path argument needed
   - Validates presence of both `multi-ai-config.jsonc` and `git-worktree-config.jsonc` in current directory
2. **config.rs**: Manages project configuration:
   - `ProjectConfig`: Reads from current directory's `multi-ai-config.jsonc` for AI apps list
   - `AiApp` struct: Defines AI tool name and full command to execute
   - `TerminalMode` enum (v0.11.0+): Defines terminal mode (Iterm2, TmuxMultiWindow, TmuxSingleWindow)
     - System defaults: macOS → Iterm2, Linux → TmuxSingleWindow
     - Priority: CLI flag > config file > system default

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

5. **tmux.rs**: `TmuxManager` handles tmux automation (with --tmux flag):
   - Creates session named `<project>-<branch-prefix>`
   - Two layout modes (v0.11.0+):
     - **Multi-Window** (default): One window per AI app, each split into two panes
       - Left pane (index 1): Launches the AI tool after shell initialization (500ms delay)
       - Right pane (index 2): Shell for manual commands
       - Important: Pane indices start from 1, not 0
     - **Single-Window**: All AI apps in one window with vertical columns (like iTerm2)
       - Creates vertical splits for each AI app (columns)
       - Each column split horizontally into two panes
       - Top pane: Launches the AI tool
       - Bottom pane: Shell for manual commands

6. **error.rs**: Custom error types using thiserror for structured error handling

### Key Implementation Details

- **Current Directory Usage** (v0.9.0+): Commands must be run from directory containing config files
- **Required Files**: Both `multi-ai-config.jsonc` and `git-worktree-config.jsonc` must exist in current directory
- **Terminal Mode Selection** (v0.11.0+):
  - Configurable via `terminal_mode` field in `multi-ai-config.jsonc` (optional)
  - Valid values: `"iterm2"`, `"tmux-multi-window"`, `"tmux-single-window"`
  - Priority: `--mode` CLI flag > config file > system default
  - System defaults: macOS → iterm2, Linux → tmux-single-window
  - iTerm2 mode validates platform (macOS only)
- **Tmux Pane Targeting**: After splitting, panes are indexed 1 (left) and 2 (right). The code sends commands to `.1` for the left pane
- **Shell Initialization**: A 500ms delay ensures the shell is ready before sending commands
- **JSONC Support**: Configuration files use JSONC format (JSON with comments)

### Dependencies
- External tools: gwt CLI, tmux
- Key crates: clap (CLI), serde (serialization), jsonc-parser (JSONC support), thiserror (errors)

## Known Issues & Fixes

### Tmux Pane Index Issue
Fixed in commit: Tmux panes start indexing from 1, not 0. Commands should target `.1` for the first pane.

### Shell Initialization Timing
Fixed by adding 500ms delay after pane creation to ensure shell is ready before sending commands.
