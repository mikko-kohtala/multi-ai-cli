# Multi-AI CLI

A Rust CLI tool that manages multiple AI development environments using git worktrees and iTerm2/tmux sessions. It automates the setup of separate worktrees for different AI tools (Claude, Codex, Amp, Gemini) and creates organized terminal sessions for each.

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
# Binary will be at ./target/release/multi-ai
```

## Configuration

Create a `multi-ai-config.jsonc` file in your project root with your AI applications and their commands:

```jsonc
{
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
    }
  ]
}
```

### Configuration Fields

- `name`: The name of the AI tool (used for branch naming)
- `command`: The full command to launch the AI tool with any flags

## Usage

### Create worktrees and terminal sessions

**Default (iTerm2):**
```bash
multi-ai create ~/code/my-project feature-branch

# Or using the shorthand:
multi-ai ~/code/my-project feature-branch
```

**With tmux:**
```bash
multi-ai create ~/code/my-project feature-branch --tmux
```

This will:
1. Create git worktrees for each AI app (e.g., `feature-branch-claude`, `feature-branch-gemini`)
2. Create iTerm2 tabs (or tmux windows) for each AI application
3. Each tab/window has two panes:
   - Top pane: Runs the AI tool with specified command
   - Bottom pane: Shell in the worktree directory for manual commands

### Remove worktrees and cleanup

```bash
multi-ai remove ~/code/my-project feature-branch

# With tmux:
multi-ai remove ~/code/my-project feature-branch --tmux
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
- One window per AI application  
- Each window split into two panes

## Example Workflow

1. Initialize your project with gwt:
```bash
cd ~/code/my-project
gwt init
```

2. Create the configuration file:
```bash
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
multi-ai create ~/code/my-project new-feature
```

4. Work on your feature across multiple AI tools

5. Clean up when done:
```bash
multi-ai remove ~/code/my-project new-feature
```

## Tmux Navigation

When using `--tmux` flag:
- Switch windows: `Ctrl+b` followed by window number (0, 1, 2...)
- Switch panes: `Ctrl+b` followed by arrow keys
- Detach from session: `Ctrl+b` followed by `d`
- Reattach to session: `tmux attach -t <session-name>`

## License

MIT