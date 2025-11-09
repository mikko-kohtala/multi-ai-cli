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
  "terminals_per_column": 2,  // Number of terminal panes per column (first is AI command, rest are shells)
  "mode": "iterm2",           // Required: iterm2 | tmux-single-window | tmux-multi-window
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

- `terminals_per_column` (optional): Number of terminal panes per column (default: 2). The first pane runs the AI command, additional panes are shell terminals
- `mode` (required): One of `"iterm2"`, `"tmux-single-window"`, `"tmux-multi-window"`. On Linux, `iterm2` is not supported. The CLI flag `--tmux` overrides to `tmux-multi-window`
- `ai_apps`: Array of AI applications to configure
  - `name`: The name of the AI tool (used for branch naming)
  - `command`: The full command to launch the AI tool with any flags

## Usage

**Important**: As of v0.9.0, `mai add` and `mai remove` commands must be run from a directory containing both `multi-ai-config.jsonc` and `git-worktree-config.jsonc` files.

### Create worktrees and terminal sessions

**Default (uses config):**
```bash
# From your project directory:
cd ~/code/my-project
mai add feature-branch   # Uses mode from config
```

**With tmux:**
```bash
# From your project directory:
cd ~/code/my-project
mai add feature-branch --tmux  # Overrides to tmux-multi-window
```

This will:
1. Create git worktrees for each AI app (e.g., `feature-branch-claude`, `feature-branch-gemini`)
2. Create iTerm2 tabs (or tmux windows) for each AI application
3. Each tab/window has two panes:
   - Top pane: Runs the AI tool with specified command
   - Bottom pane: Shell in the worktree directory for manual commands

### Remove worktrees and cleanup

```bash
# From your project directory:
cd ~/code/my-project
mai remove feature-branch

# With tmux:
mai remove feature-branch --tmux
```

## Terminal Layout

### iTerm2 Mode (Default)
- Creates a single tab with all AI applications
- Column-based layout: each AI app gets a vertical column with 2 panes
  - 1 app: 1x2 layout (1 column, 2 rows)
  - 2 apps: 2x2 layout (2 columns, each with 2 rows)  
  - 3 apps: 3x2 layout (3 columns, each with 2 rows)
  - 4 apps: 4x2 layout (4 columns, each with 2 rows)
- Top pane in each column: runs the AI tool
- Bottom pane in each column: shell for manual commands

### Tmux Mode
- Creates a single tmux session named `<project>-<branch-prefix>`
- Two layouts are supported (selected via `mode`):
  - `tmux-multi-window`: One window per AI application (two panes: left runs AI, right is a shell)
  - `tmux-single-window`: Single window named `apps` with N columns (one per app); each column splits into two panes (top runs AI, bottom is a shell)

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

When using tmux:
- Switch windows: `Ctrl+b` then window number (0, 1, 2...)
- Switch panes: `Ctrl+b` then arrow keys
- Detach from session: `Ctrl+b` then `d`
- Reattach to session: `tmux attach -t <session-name>`

Pane targeting details:
- The tool targets panes by stable pane IDs (e.g., `%3`) captured before splits, not by indices, so it works regardless of `base-index`/`pane-base-index` settings.

## Tmux Windows and Panes

This tool uses tmux programmatically to set up sessions:
- Sessions: `tmux new-session -d -s <session> -n <window> -c <dir>`
- Windows: `tmux new-window -t <session>: -n <name> -c <dir>` (one per AI app)
- Panes: `tmux split-window -h -t <session>:<window> -c <dir> -p 50` (two panes per window)
- Send keys: `tmux send-keys -t <pane_id> "<cmd>" Enter`

Pane targeting details:
- We capture the original pane ID before splitting and use it to run the AI command. This avoids assumptions about `base-index`/`pane-base-index` and works across tmux configs
- Inspect panes with `tmux list-panes -t <session>:<window> -F "#{pane_index} #{pane_id} #{pane_active}"`

## License

MIT
