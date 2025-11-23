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
- **copilot**: GitHub Copilot CLI (interactive mode)
- **kilo**: Kilo Code CLI (interactive mode)
- **cline**: Cline CLI (interactive mode)
- **droid**: Factory CLI (interactive mode)

## Features

- 🌳 **Git Worktree Management**: Automatically creates and manages git worktrees for each AI tool
- 🖥️ **iTerm2 Integration** (`mode: "iterm2"` on macOS): Creates tabs with split panes for each AI application
- 🎛️ **Tmux Support** (`mode: "tmux-single-window"` or `"tmux-multi-window"`): Creates tmux sessions with organized windows and panes
- 🎨 **Flexible Configuration**: Define custom commands for each AI tool
- 🚀 **Quick Setup**: Single command to set up multiple AI environments

## Prerequisites

- [gwt CLI](https://github.com/mikko-kohtala/git-worktree-cli) - Git worktree management tool
- iTerm2 (only if you plan to use `mode: "iterm2"` on macOS)
- tmux (required when `mode` is a tmux layout or when overriding via `--mode`/`--tmux`)

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

The following files must exist in your project directory:
1. `multi-ai-config.jsonc` - Defines AI applications and their commands
2. `git-worktree-config.jsonc` - Git worktree configuration (created by `gwt init`)

**Config File Locations**: Config files can be placed in either:
- Current directory (checked first)
- `./main/` subdirectory (checked second if not found in current directory)

This allows you to keep config files version-controlled in a `./main/` subdirectory while maintaining worktrees at the repo root level.

### Setting up multi-ai-config.jsonc

You can create the config file interactively:
```bash
mai init
```

Or create it manually:

```jsonc
{
  "terminals_per_column": 2,  // Number of terminal panes per column (first is AI command, rest are shells)
  "mode": "iterm2",           // Optional: iterm2 | tmux-single-window | tmux-multi-window (defaults: macOS→iterm2, others→tmux-single-window)
  "ai_apps": [
    {
      "name": "claude",
      "command": "claude --dangerously-skip-permissions",
      "ultrathink": "ultrathink"
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
      "command": "amp --dangerously-allow-all",
      "ultrathink": "Use oracle and think heavily"
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
- `mode` (optional): One of `"iterm2"`, `"tmux-single-window"`, `"tmux-multi-window"`. Defaults by OS: macOS → `iterm2`; others → `tmux-single-window`. Use CLI `--mode` (or legacy `--tmux`) to override per run.
- `ai_apps`: Array of AI applications to configure
  - `name`: The name of the AI tool (used for branch naming)
  - `command`: The full command to launch the AI tool with any flags
  - `ultrathink` (optional): Hint appended to prompts when using `mai send` (e.g., `"ultrathink"` for Claude Code, `"Use oracle and think heavily"` for amp)

## Usage

**Important**: `mai add`, `mai continue`, `mai resume`, and `mai remove` commands must be run from a directory containing both `multi-ai-config.jsonc` and `git-worktree-config.jsonc` files (either in the current directory or in a `./main/` subdirectory).

### Create worktrees and terminal sessions

```bash
# From your project directory:
cd ~/code/my-project
mai add feature-branch   # Respects the mode defined in multi-ai-config.jsonc
```

Need a different layout for a single run? Use the new `--mode` flag (or `--tmux` as a shorthand for `tmux-multi-window`):

```bash
mai add feature-branch --mode tmux-single-window
# legacy alias:
mai add feature-branch --tmux
```

This will:
1. Create git worktrees for each AI app (e.g., `feature-branch-claude`, `feature-branch-gemini`)
2. Create iTerm2 tabs (or tmux windows) for each AI application
3. Each tab/window has two panes:
   - Top pane: Runs the AI tool with specified command
   - Bottom pane: Shell in the worktree directory for manual commands

### Continue working on existing worktrees

If you've closed your terminal session but the worktrees still exist, you can create a new session/tab:

```bash
# From your project directory:
cd ~/code/my-project
mai continue feature-branch   # Creates new session/tab for existing worktrees
# Or use the alias:
mai resume feature-branch
```

This will:
1. Check that worktrees for the branch prefix already exist
2. Create a new iTerm2 tab (or tmux session) pointing to the existing worktrees
3. Each tab/window will have the same layout as `add` command

**Note**: If worktrees don't exist, you'll get an error asking you to run `mai add` first.

### Send prompts/commands into a running session

Launch an interactive TUI to paste text into the tmux single-window layout:

```bash
mai send
```

- Left side: compose the prompt/command text (Ctrl+S to send)
- Top-right: pick the tmux session and AI column, and choose whether to send as a prompt (top pane) or a command (second terminal in the column)
- Bottom-right: toggle the per-tool `ultrathink` hint and other send options
- Mouse and keyboard are both supported

> Designed for the `tmux-single-window` layout where each AI app owns a column with a running pane on top and a shell underneath.

### Remove worktrees and cleanup

```bash
# From your project directory:
cd ~/code/my-project
mai remove feature-branch

# Override cleanup behavior or skip confirmation:
mai remove feature-branch --mode tmux-multi-window
mai remove feature-branch --force   # removes without prompting
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

### Tmux Mode
- Creates a single tmux session named `<project>-<branch-prefix>`
- Two layouts are supported (selected via `mode`):
  - `tmux-multi-window`: One window per AI application (two panes: left runs AI, right is a shell)
  - `tmux-single-window`: Single window named `apps` with N equal-width columns (one per app); each column splits into two panes (top runs AI, bottom is a shell)

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
  "mode": "iterm2",
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

5. If you close your terminal but want to continue later:
```bash
cd ~/code/my-project
mai continue new-feature  # or: mai resume new-feature
```

6. Clean up when done:
```bash
cd ~/code/my-project
mai remove new-feature
```

## Tmux Navigation

When `mode` selects a tmux layout (or you override with `--mode`/`--tmux`):
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
