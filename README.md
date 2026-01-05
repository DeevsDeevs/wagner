# Wagner

Multi-repo task manager for agents sessions. Orchestrates multiple Claude instances across git worktrees with tmux.

## Overview

Wagner solves the problem of managing multiple Claude Code sessions when working on tasks that span multiple repositories. It:

- Creates isolated git worktrees for each task
- Manages tmux sessions with multiple panes
- Sets up Claude Code hooks for status tracking
- Provides a unified view of all active sessions

## Installation

### With devbox (recommended)

```bash
git clone https://github.com/youruser/wagner.git
cd wagner
devbox shell
cargo build --release
```

### With Cargo

```bash
cargo install --path .
```

### From source

```bash
git clone https://github.com/youruser/wagner.git
cd wagner
cargo build --release
# Binary at ./target/release/wagner
```

## Quick Start

### 1. Create a task

```bash
# From inside a git repo - just provide a name
cd ~/projects/myrepo
wagner new my-feature

# Creates worktree with branch task/my-feature
# Specify custom branch with -b
wagner new my-feature -b fix/auth-bug

# Multi-repo tasks use --repos
wagner new my-feature --repos \
  frontend:~/projects/frontend:feature-x,\
  backend:~/projects/backend:feature-x
```

This will:
1. Create a task folder at `~/tasks/my-feature/`
2. Create git worktrees for each repo
3. Set up Claude Code hooks in each worktree
4. Create a tmux session `wagner_my-feature`

### 2. List tasks

```bash
wagner list
```

Output:
```
my-feature           3 repos  (2h ago)   /home/user/tasks/my-feature
another-task         1 repo   (3d ago)   /home/user/tasks/another-task
```

### 3. Attach to a task

```bash
# Explicit
wagner attach my-feature

# Auto-detect from cwd (when inside a task directory)
cd ~/tasks/my-feature
wagner attach
```

### 4. Add more Claude panes

```bash
# Auto-detect task from cwd
cd ~/tasks/my-feature
wagner add

# Explicit task and repo
wagner add my-feature backend
```

### 5. Delete a task

```bash
# Removes worktrees, keeps branches
wagner delete my-feature

# Also deletes the branches
wagner delete my-feature --force
```

## Repo Specification Format

Repos are specified as `name:source:branch` where:

- **name**: Display name for the repo within the task
- **source**: Either a local path or git URL
  - Local: `/path/to/repo` or `~/repos/myrepo`
  - Remote: `https://github.com/user/repo.git` or `git@github.com:user/repo.git`
- **branch**: Branch name for the worktree (default: `main`)

Examples:
```bash
# Local repo, custom branch
frontend:/home/user/repos/frontend:feature-auth

# Local repo, default branch (main)
frontend:/home/user/repos/frontend

# Remote repo (will be cloned)
backend:https://github.com/org/backend.git:develop
```

## Configuration

Config is stored at `$XDG_CONFIG_HOME/wagner/config.json` (or `~/.config/wagner/config.json`).

```json
{
  "tasks_root": "/home/user/tasks",
  "default_agent": "claude"
}
```

- **tasks_root**: Where task folders are created (default: `~/tasks`)
- **default_agent**: Agent to use (currently only `claude` supported)

## Directory Structure

When you create a task, Wagner creates:

```
~/tasks/my-feature/
├── .wagner/
│   └── task.json          # Task metadata
├── frontend/              # Git worktree
│   └── .claude/
│       └── settings.json  # Claude hooks
├── backend/               # Git worktree
│   └── .claude/
│       └── settings.json
└── shared/                # Git worktree
    └── .claude/
        └── settings.json
```

## Commands

| Command | Description |
|---------|-------------|
| `wagner` | Launch TUI |
| `wagner new <name>` | Create task from current repo (auto branch: `task/<name>`) |
| `wagner new <name> -b <branch>` | Create task with specific branch |
| `wagner new <name> --repos <specs>` | Create multi-repo task |
| `wagner list` | List all tasks with age |
| `wagner attach [task]` | Attach to tmux session (auto-detects from cwd) |
| `wagner add [task] [repo]` | Add Claude pane (auto-detects from cwd) |
| `wagner delete <task> [--force]` | Delete task (--force removes branches) |

## Workflow Example

```bash
# Single repo workflow
cd ~/projects/myapi
wagner new user-auth            # Creates task/user-auth branch
wagner attach                   # Opens tmux
wagner add                      # Add another Claude pane
wagner delete user-auth --force # Cleanup when done

# Multi-repo workflow
wagner new user-auth --repos \
  api:~/work/api:feature/user-auth,\
  web:~/work/web:feature/user-auth

cd ~/tasks/user-auth
wagner attach                   # Auto-detects task
wagner add api                  # Add pane in api repo
```

## Architecture

Wagner uses a hexagonal architecture with swappable backends:

- **Terminal trait**: Currently `Tmux`, future `Ghostty`
- **Agent trait**: Currently `ClaudeCode`, extensible to other agents

## Development

```bash
# Enter dev environment
devbox shell

# Build
cargo build

# Run tests
cargo test

# Check
cargo check

# Format
cargo fmt

# Lint
cargo clippy
```

## License

MIT
