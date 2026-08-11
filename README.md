# Multi-AI CLI

A Rust CLI tool that manages multiple AI development environments using git worktrees and iTerm2/tmux sessions. It automates the setup of separate worktrees for different AI tools and creates organized terminal sessions for each.

## Supported AI Tools

The following AI development tools are supported:

- **claude**: Anthropic's AI assistant (with `--dangerously-skip-permissions` flag for YOLO mode)
- **gemini**: Google's AI assistant (with `--yolo` flag for YOLO mode)
- **codex**: OpenAI Codex CLI (with `--yolo` flag for YOLO mode)
- **amp**: AI assistant (with `--dangerously-allow-all` flag for YOLO mode)
- **opencode**: AI coding assistant (no special flags for YOLO mode)
- **cursor-agent**: Cursor AI assistant (with `--force` flag for YOLO mode)
- **copilot**: GitHub Copilot CLI (with `--allow-all-tools` flag for YOLO mode)
- **kilo**: Kilo Code CLI (interactive mode)
- **cline**: Cline CLI (interactive mode)
- **droid**: Factory CLI (interactive mode)

## Features

- 🌳 **Git Worktree Management**: Automatically creates and manages git worktrees for each AI tool
- 🖥️ **iTerm2 Integration** (`mode: "iterm2"` on macOS): Creates tabs with split panes for each AI application
- 🎛️ **Tmux Support** (`mode: "tmux-single-window"` or `"tmux-multi-window"`): Creates tmux sessions with organized windows and panes
- 🔍 **Multi-AI Code Review** (`mai review`): Launch parallel code reviews across multiple AI tools with unified summaries
- 📋 **Multi-AI Collaborative Planning** (`mai plan`): Generate implementation plans from multiple AI perspectives
- 📨 **Send Commands** (`mai send`): Interactive TUI to send prompts to running AI sessions
- 🎨 **Flexible Configuration**: Define custom commands for each AI tool with global or per-project config
- 🚀 **Quick Setup**: Single command to set up multiple AI environments

## Prerequisites

- **iTerm2** + [it2 CLI](https://github.com/mkusaka/it2) — only needed for iTerm2 mode and `mai review`/`mai plan` on macOS. Install it2 with: `uvx install it2`
- **tmux** — needed for tmux modes, `mai send`, and as a fallback on non-macOS platforms

## Installation

```bash
cargo install --path .
```

Or use the Makefile, which also symlinks global config files (`apps.jsonc`, `settings.jsonc`) to `~/.config/multi-ai-cli/`:

```bash
make install
```

Or build from source without installing:

```bash
cargo build --release
# Binary will be at ./target/release/mai
```

## Configuration

### Config Discovery

All configs live in `~/.config/multi-ai-cli/`, one file per project, named by git remote URL (e.g., `github_com_owner_repo.jsonc`). Discovery:

1. Git remote URL → generate filename → look up `~/.config/multi-ai-cli/{filename}.jsonc`
2. Fallback: scan all `.jsonc` files for matching `project_path` or `worktrees_path`

Each config requires a `project_path` field pointing to the main git repository. Run `mai init` from your project to create one.

### Setting up a project config

You can create the config file interactively:

```bash
mai init
```

Or create it manually at `~/.config/multi-ai-cli/{project}.jsonc`:

```jsonc
{
  "project_path": "/Users/you/code/my-project",
  "terminals_per_column": 2, // Number of terminal panes per column (first is AI command, rest are shells)
  "mode": "iterm2", // Optional: iterm2 | tmux-single-window | tmux-multi-window (defaults: macOS→iterm2, others→tmux-single-window)
  "hooks": {
    "postAdd": ["npm install"]  // Commands to run in each new worktree after creation
  }
}
```

AI tools are configured globally in `apps.jsonc` (see [Global Configuration](#global-configuration) below). You can override them per-project by adding an `ai_apps` array to the project config.

### Project Configuration Fields

- `project_path` (required): Absolute path to the main git repository. Auto-detected by `mai init`.
- `worktrees_path` (optional): The worktrees root path, used for config matching when running inside a worktree. Defaults to project directory.
- `terminals_per_column` (optional): Number of terminal panes per column (default: 2). The first pane runs the AI command, additional panes are shell terminals.
- `mode` (optional): One of `"iterm2"`, `"tmux-single-window"`, `"tmux-multi-window"`. Defaults by OS: macOS → `iterm2`; others → `tmux-single-window`. Use CLI `--mode` (or legacy `--tmux`) to override per run.
- `ai_apps` (optional): Array of AI applications. If omitted, tools from global `apps.jsonc` are used via the interactive picker.
- `hooks` (optional): Lifecycle hooks:
  - `postAdd` (array of strings): Commands to run in each new worktree directory after creation (e.g., `"npm install"`, `"make setup"`)

### Global Configuration

Global config files live in `~/.config/multi-ai-cli/`:

#### `apps.jsonc` — AI Tools

Defines all available AI tools. Run `mai apps` to open this file. Each entry has:

```jsonc
{
  "name": "claude",                                    // Tool name
  "slug": "claude-plan-yolo",                          // Git-safe slug for branch names (auto-generated if omitted)
  "command": "claude --permission-mode plan ...",       // Full command to launch
  "description": "Plan mode with skip option",         // Shown in interactive pickers
  "default": true,                                     // Pre-selected in interactive pickers
  "meta_review": false,                                // Eligible for meta-reviewer/meta-planner role
  "ultrathink": "ultrathink"                           // Extra text appended when Ultrathink is enabled in mai send
}
```

#### `settings.jsonc` — Prompts and Templates

Contains review and plan prompt templates. Run `mai apps` or edit directly. Supports template variables:

- `{{base_branch}}` — substituted with the selected base branch name
- `{{review_locations}}` — replaced with paths to individual `REVIEW.md` files
- `{{plan_locations}}` — replaced with paths to individual `PLAN.md` files
- `{{context}}` — (optional, review prompts) replaced with the PR/issue context block; if absent, the block is appended to the end of the prompt

## Usage

**Important**: Most commands should be run from within your project (or a worktree). Config is discovered from `~/.config/multi-ai-cli/` using the git remote URL.

### Create worktrees and terminal sessions

```bash
cd ~/code/my-project
mai add feature-branch   # Respects the mode defined in config

# Interactive mode — pick environment name and AI tools:
mai add

# Override layout for a single run:
mai add feature-branch --mode tmux-single-window
# Legacy alias:
mai add feature-branch --tmux
```

This will:

1. Create git worktrees for each AI app (e.g., `feature-branch-claude`, `feature-branch-gemini`)
2. Run any `postAdd` hooks in each new worktree
3. Create iTerm2 tabs (or tmux windows) for each AI application
4. Each tab/window has panes:
   - Top pane: Runs the AI tool with specified command
   - Bottom pane: Shell in the worktree directory for manual commands

### Continue working on existing worktrees

If you've closed your terminal session but the worktrees still exist, you can create a new session/tab:

```bash
mai continue feature-branch   # Creates new session/tab for existing worktrees
# Or use the alias:
mai resume feature-branch
```

This will:

1. Check that worktrees for the branch prefix already exist
2. Create a new iTerm2 tab (or tmux session) pointing to the existing worktrees
3. Each tab/window will have the same layout as `add` command

**Note**: If worktrees don't exist, you'll get an error asking you to run `mai add` first.

### List worktree environments

```bash
mai list
```

Shows all worktree environments grouped by branch prefix, with relative timestamps and app slugs.

### Remove worktrees and cleanup

```bash
mai remove feature-branch

# Interactive mode — pick one or more environments to remove:
mai remove

# Override cleanup behavior or skip confirmation:
mai remove feature-branch --mode tmux-multi-window
mai remove feature-branch --force   # removes without prompting
```

### Send commands to AI sessions

The `mai send` command opens an interactive TUI that allows you to send prompts or commands to running AI sessions:

```bash
mai send
```

This will:

1. Detect active tmux sessions for your project
2. Open an interactive TUI where you can:
   - Type multi-line input
   - Select which session and AI tool to send to
   - Choose to send to the AI prompt pane or command shell pane
   - Toggle "ultrathink" mode for supported AI tools

**Note**: `mai send` currently targets the `tmux-single-window` layout (window name `apps`). It does not work with iTerm2 or `tmux-multi-window` sessions yet. When Ultrathink is enabled, the configured `ai_apps[].ultrathink` text is appended to the prompt pane.

#### Keyboard Controls

- **Enter**: Send the message
- **Shift+Enter**: Insert a newline (requires terminal configuration, see below)
- **Ctrl+C**: Clear input (press twice to confirm)
- **Tab**: Cycle focus between windows (Input → Sessions → Apps → Settings)
- **Arrow keys**: Navigate lists
- **Space/Enter** (in Settings): Toggle options
- **q** (when not in Input): Quit

#### Terminal Setup for Shift+Enter

Most terminal emulators don't transmit the Shift modifier with Enter by default. To enable Shift+Enter for multi-line input, configure your terminal:

**iTerm2 (macOS):**

1. Open iTerm2 → Preferences (⌘,)
2. Navigate to: **Profiles** → Select your profile → **Keys** tab
3. Click **Key Mappings** → **+** (to add a new key mapping)
4. Press **Shift+Enter** when prompted
5. Set **Action** to: **Send Text**
6. Set **Text** to: `\n` (literal backslash followed by n)
7. Click **OK**

**Alternative methods for multi-line input:**

- Configure **Option+Enter** or **Ctrl+J** as alternatives
- Use external editors and copy-paste for longer inputs

### Multi-AI code review

The `mai review` command launches an interactive TUI for code review across multiple AI tools:

```bash
cd ~/code/my-project
mai review                        # Interactive branch selection
mai review feat/my-branch         # Skip branch selection
mai review input                  # Print last saved review prompt
mai review meta                   # Print last saved meta reviewer prompt
mai review copy-unified-path      # Copy unified review file path to clipboard
```

This will:

1. Select a branch and base branch to review against
2. Detect the branch's open PR via `gh` (best-effort) and pre-fill an editable **Context Links** field with the PR URL and any linked issues/tickets (GitHub issues, Jira, Linear) — paste additional URLs as needed
3. Create review worktrees for each selected AI reviewer
4. Generate a `CHANGES.diff` file in each worktree (merge-base diff)
5. Send the review prompt to each AI tool, including a "Related context" block with the PR/issue links so reviewers read the PR description and ticket to understand intent
6. Each reviewer writes findings to `REVIEW.md`
7. If a meta reviewer is selected, it synthesizes all reviews into:
   - `REVIEW_SUMMARY.md` — consolidated findings with per-tool attribution
   - `REVIEW_SUMMARY_UNIFIED.md` — unified review without attribution

PR detection requires the [GitHub CLI](https://cli.github.com/) (`gh`) to be installed and authenticated; without it the Context Links field starts empty and links can be pasted manually.

### Multi-AI collaborative planning

The `mai plan` command launches an interactive TUI for collaborative planning across multiple AI tools:

```bash
cd ~/code/my-project
mai plan                # Interactive branch selection and planning
mai plan feat/my-branch # Skip branch selection
mai plan input          # Print last saved plan prompt
mai plan meta           # Print last saved meta planner prompt
```

Each selected AI planner creates an independent implementation plan (saved to `PLAN.md`), then the meta planner synthesizes all plans into a unified `PLAN_UNIFIED.md`. The planning prompt references [superpowers skills](https://github.com/anthropics/superpowers) for structured brainstorming and plan writing.

### Open configuration files

```bash
mai config   # Open the project config file in default application
mai apps     # Open the global AI tools config (apps.jsonc)
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
  - `tmux-single-window`: Single window named `apps` with N equal-width columns (one per app); each column splits into two panes (top runs AI, bottom is a shell). This layout is required for `mai send`.

## Example Workflow

1. Create the configuration file:

```bash
cd ~/code/my-project
mai init  # Interactive setup, saves to ~/.config/multi-ai-cli/
```

2. Create AI development environments:

```bash
mai add new-feature
```

3. Work on your feature across multiple AI tools

4. If you close your terminal but want to continue later:

```bash
mai continue new-feature  # or: mai resume new-feature
```

5. Clean up when done:

```bash
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
