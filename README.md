# Multi-AI CLI

A Rust CLI tool that manages multiple AI development environments using git worktrees and tmux sessions.

## Prerequisites

- [gwt CLI](https://github.com/mikko-kohtala/git-worktree-cli) - Git worktree management tool
- tmux - Terminal multiplexer
- Rust/Cargo - For building from source

## Installation

```bash
cargo build --release
cargo install --path .
```

## Configuration

### 1. User Configuration
Create `~/.config/multi-ai/settings.json`:

```json
{
  "code_root": "~/code/mikko"
}
```

This defines the root directory where your projects are located.

### 2. Project Configuration
In each project directory, create `multi-ai-config.yaml`:

```yaml
ai_apps:
  - claude
  - codex
  - amp
  - gemini
```

This defines which AI tools should be set up for the project.

## Usage

```bash
multi-ai <project> <branch-prefix>
```

Example:
```bash
multi-ai kuntoon vercel-theme
```

This will:
1. Navigate to `/Users/mikkoh/code/mikko/kuntoon`
2. Create worktrees for each configured AI app:
   - `claude-vercel-theme`
   - `codex-vercel-theme`
3. Create a tmux session `kuntoon-vercel-theme` with:
   - One window per AI app
   - Each window split into two panes:
     - Left pane: AI tool launched
     - Right pane: Shell in the worktree directory
4. Attach to the tmux session

## Tmux Navigation

- Switch windows: `Ctrl+b` followed by window number (0, 1, 2...)
- Switch panes: `Ctrl+b` followed by arrow keys
- Detach from session: `Ctrl+b` followed by `d`
- Reattach to session: `tmux attach -t <session-name>`

## Error Handling

The CLI will validate:
- User configuration exists at `~/.config/multi-ai/settings.json`
- Project exists in the configured code root
- Project has `multi-ai-config.yaml`
- Project is a git repository
- gwt CLI is installed
- tmux is installed