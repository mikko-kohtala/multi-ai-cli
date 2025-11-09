# Multi-AI CLI

A Rust CLI tool that manages multiple AI development environments using git worktrees and iTerm2/tmux sessions. It automates the setup of separate worktrees for different AI tools and creates organized terminal sessions for each.

## Supported AI Tools

The following AI development tools are supported:
- **claude**: Anthropic's AI assistant (with `--dangerously-skip-permissions` flag for YOLO mode)
- **gemini**: Google's AI assistant (with `--yolo` flag for YOLO mode)
- **codex**: GitHub Copilot's AI assistant (with `--ask-for-approval never` flag for YOLO mode)
- **amp**: AI assistant (with `--dangerously-allow-all` flag for YOLO mode)
- **opencode**: AI coding assistant (no special flags for YOLO mode)
- **cursor-agent**: Cursor AI assistant (with `--force` flag for YOLO mode)

## Features

- 🌳 **Git Worktree Management**: Automatically creates and manages git worktrees for each AI tool
- 🖥️ **iTerm2 Integration** (default): Creates tabs with split panes for each AI application
- 🎛️ **Tmux Support** (optional): Creates tmux sessions with organized windows and panes
  - **Multi-Window Layout** (default): One window per AI app - ideal for focused work
  - **Single-Window Layout**: All apps in columns - ideal for side-by-side comparison
- 🎨 **Flexible Configuration**: Define custom commands for each AI tool
- 🚀 **Quick Setup**: Single command to set up multiple AI environments

## Prerequisites

- [gwt CLI](https://github.com/mikko-kohtala/git-worktree-cli) - Git worktree management tool
- iTerm2 (for default mode on macOS)
- tmux (optional, for `--tmux` flag)

## Installation

```bash
cargo install --path .
```

Or build from source:

```bash
cargo build --release
# Binary will be at ./target/release/mai
```

## Configuration

### Required Files

As of v0.9.0, the following files must exist in your project directory:
1. `multi-ai-config.jsonc` - Defines AI applications and their commands
2. `git-worktree-config.jsonc` - Git worktree configuration (created by `gwt init`)

### Setting up multi-ai-config.jsonc

You can create the config file interactively:
```bash
mai init
```

Or create it manually:

```jsonc
{
  "terminal_mode": "tmux-single-window", // Optional: "iterm2", "tmux-multi-window", or "tmux-single-window"
  "terminals_per_column": 2,  // Number of terminal panes per column (first is AI command, rest are shells)
  "ai_apps": [
    {
      "name": "claude",
      "command": "claude --dangerously-skip-permissions"
    },
    {
      "name": "gemini",
      "command": "gemini --yolo"
    },
    {
      "name": "codex",
      "command": "codex --ask-for-approval never"
    },
    {
      "name": "amp",
      "command": "amp --dangerously-allow-all"
    },
    {
      "name": "opencode",
      "command": "opencode"
    },
    {
      "name": "cursor-agent",
      "command": "cursor-agent --force"
    }
  ]
}
```

### Configuration Fields

- `terminal_mode` (optional): Terminal mode to use. Valid values:
  - `"iterm2"` - iTerm2 mode (macOS only, default on macOS)
  - `"tmux-multi-window"` - Tmux with one window per AI app
  - `"tmux-single-window"` - Tmux with all apps in one window (default on Linux)
  - If not specified, uses system default. Can be overridden with `--mode` CLI flag.
- `terminals_per_column` (optional): Number of terminal panes per column (default: 2). The first pane runs the AI command, additional panes are shell terminals
- `ai_apps`: Array of AI applications to configure
  - `name`: The name of the AI tool (used for branch naming)
  - `command`: The full command to launch the AI tool with any flags

## Usage

**Important**: As of v0.9.0, `mai add` and `mai remove` commands must be run from a directory containing both `multi-ai-config.jsonc` and `git-worktree-config.jsonc` files.

### Create worktrees and terminal sessions

**Use system default (or config file setting):**
```bash
# From your project directory:
cd ~/code/my-project
mai add feature-branch
# Uses: iTerm2 on macOS, tmux-single-window on Linux, or terminal_mode from config
```

**Explicit mode selection:**
```bash
# iTerm2 mode (macOS only):
mai add feature-branch --mode iterm2

# Tmux multi-window mode (one window per AI app):
mai add feature-branch --mode tmux-multi-window

# Tmux single-window mode (all apps in one window):
mai add feature-branch --mode tmux-single-window
```

This will:
1. Create git worktrees for each AI app (e.g., `feature-branch-claude`, `feature-branch-gemini`)
2. Create terminal sessions based on the selected mode
3. Each session has panes for:
   - Running the AI tool with specified command
   - Shell in the worktree directory for manual commands

### Remove worktrees and cleanup

```bash
# From your project directory (uses system default or config):
cd ~/code/my-project
mai remove feature-branch

# With explicit mode:
mai remove feature-branch --mode tmux-single-window
```

## Terminal Layout

### iTerm2 Mode (Default on macOS)
- Creates a single tab with all AI applications
- Column-based layout: each AI app gets a vertical column with 2 panes
  - 1 app: 1x2 layout (1 column, 2 rows)
  - 2 apps: 2x2 layout (2 columns, each with 2 rows)  
  - 3 apps: 3x2 layout (3 columns, each with 2 rows)
  - 4 apps: 4x2 layout (4 columns, each with 2 rows)
- Top pane in each column: runs the AI tool
- Bottom pane in each column: shell for manual commands

### Tmux Mode (v0.11.0+)

**Multi-Window Layout (default - `--tmux`):**
- Creates a single tmux session named `<project>-<branch-prefix>`
- One window per AI application
- Each window split into two panes:
  - Left pane (50%): Runs the AI tool
  - Right pane (50%): Shell for manual commands
- Switch between windows with `Ctrl+b` followed by window number
- Best for: Focused work on one AI tool at a time

**Single-Window Layout (`--tmux --tmux-layout single-window`):**
- Creates a single tmux session with ONE window containing all AI apps
- Vertical columns layout (similar to iTerm2 mode):
  - Each AI app gets its own column
  - Each column split horizontally into two panes
  - Top pane: Runs the AI tool
  - Bottom pane: Shell for manual commands
- All AI tools visible at once
- Best for: Side-by-side comparison of AI responses

## Example Workflow

1. Initialize your project with gwt:
```bash
cd ~/code/my-project
gwt init
```

2. Create the configuration file:
```bash
mai init  # Interactive setup
# OR manually create multi-ai-config.jsonc:
cat > multi-ai-config.jsonc << 'EOF'
{
  "ai_apps": [
    {
      "name": "claude",
      "command": "claude --dangerously-skip-permissions"
    },
    {
      "name": "gemini",
      "command": "gemini"
    }
  ]
}
EOF
```

3. Create AI development environments:
```bash
cd ~/code/my-project
mai add new-feature
```

4. Work on your feature across multiple AI tools

5. Clean up when done:
```bash
cd ~/code/my-project
mai remove new-feature
```

## Tmux Navigation

When using tmux modes (default on Linux):
- **Multi-window mode**:
  - Switch windows: `Ctrl+b` followed by window number (0, 1, 2...)
  - Switch panes: `Ctrl+b` followed by arrow keys
- **Single-window mode**:
  - Switch panes: `Ctrl+b` followed by arrow keys
- **General**:
  - Detach from session: `Ctrl+b` followed by `d`
  - Reattach to session: `tmux attach -t <session-name>`

## License

MIT