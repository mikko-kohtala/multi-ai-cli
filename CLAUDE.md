# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Multi-AI CLI is a Rust tool that manages multiple AI development environments using git worktrees and tmux sessions. It automates the setup of separate worktrees for different AI tools (Claude, Codex, Amp, Gemini) and creates organized tmux sessions for each.

## Version Management

**IMPORTANT**: Whenever making code changes, always increment the patch version in Cargo.toml. This ensures proper version tracking for all changes.

Example: Change `version = "0.2.0"` to `version = "0.2.1"` for any code modifications.

## Common Commands

### Build
```bash
cargo build           # Debug build
cargo build --release # Release build for production use
```

### Run
```bash
cargo run -- <project> <branch-prefix>                    # Run from source
cargo run -- create <project> <branch-prefix>             # Create worktrees and tmux session
cargo run -- remove <project> <branch-prefix>             # Remove worktrees and tmux session
./target/debug/multi-ai <project> <branch-prefix>         # Run debug binary
./target/release/multi-ai <project> <branch-prefix>       # Run release binary
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
1. **main.rs**: Entry point, handles CLI argument parsing via clap, orchestrates the create/remove commands
2. **config.rs**: Manages two configuration types:
   - `UserConfig`: Reads from `~/.config/multi-ai/settings.jsonc` for global code_root path
   - `ProjectConfig`: Reads from project's `multi-ai-config.json[c]` for AI apps list
   - `AiApp` enum: Defines available AI tools and their launch commands

3. **worktree.rs**: `WorktreeManager` interfaces with gwt CLI to:
   - Create git worktrees for each AI app with naming pattern: `<branch-prefix>-<ai-app>`
   - Validate gwt CLI availability and project initialization
   - Remove worktrees during cleanup

4. **tmux.rs**: `TmuxManager` handles tmux automation:
   - Creates session named `<project>-<branch-prefix>`
   - Creates one window per AI app, each split into two panes
   - Left pane (index 1): Launches the AI tool after shell initialization (500ms delay)
   - Right pane (index 2): Shell for manual commands
   - Important: Pane indices start from 1, not 0

5. **error.rs**: Custom error types using thiserror for structured error handling

### Key Implementation Details

- **Tmux Pane Targeting**: After splitting, panes are indexed 1 (left) and 2 (right). The code sends commands to `.1` for the left pane.
- **Shell Initialization**: A 500ms delay ensures the shell is ready before sending commands
- **Path Expansion**: User config supports `~` expansion via shellexpand crate
- **JSONC Support**: Both JSON and JSONC (with comments) formats are supported for configurations

### Dependencies
- External tools: gwt CLI, tmux
- Key crates: clap (CLI), serde (serialization), jsonc-parser (JSONC support), thiserror (errors)

## Known Issues & Fixes

### Tmux Pane Index Issue
Fixed in commit: Tmux panes start indexing from 1, not 0. Commands should target `.1` for the first pane.

### Shell Initialization Timing
Fixed by adding 500ms delay after pane creation to ensure shell is ready before sending commands.