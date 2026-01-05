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

### 1. Create a task with repos

```bash
# Format: name:source:branch
# source can be a local path or git URL

# Single repo from local path
wagner new my-feature --repos frontend:/path/to/frontend:feature-branch

# Multiple repos
wagner new my-feature --repos \
  frontend:/path/to/frontend:feature-x,\
  backend:/path/to/backend:feature-x,\
  shared:/path/to/shared:feature-x
```

This will:
1. Create a task folder at `~/tasks/my-feature/`
2. Create git worktrees for each repo in the task folder
3. Set up Claude Code hooks in each worktree
4. Create a tmux session `wagner_my-feature`

### 2. List tasks

```bash
wagner list
```

Output:
```
my-feature           3 repos  /home/user/tasks/my-feature
another-task         1 repo   /home/user/tasks/another-task
```

### 3. Attach to a task

```bash
wagner attach my-feature
```

This attaches to the tmux session for that task.

### 4. Add more Claude panes

```bash
# Add a pane in the first repo
wagner add my-feature

# Add a pane in a specific repo
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
| `wagner new <name> --repos <specs>` | Create a new task with worktrees |
| `wagner list` | List all tasks |
| `wagner attach <task>` | Attach to task's tmux session |
| `wagner add [task] [repo]` | Add a new Claude pane |
| `wagner delete <task> [--force]` | Delete a task |
| `wagner send <session> <message>` | Send message to session (not implemented) |
| `wagner chains [task] [repo]` | List chains (not implemented) |

## Workflow Example

```bash
# 1. Create a task for a new feature spanning 3 repos
wagner new user-auth --repos \
  api:~/work/api:feature/user-auth,\
  web:~/work/web:feature/user-auth,\
  shared:~/work/shared:feature/user-auth

# 2. Attach and start working
wagner attach user-auth

# 3. Inside tmux, you'll have panes for each repo
# Each pane is cd'd to its worktree with Claude hooks set up

# 4. Add another Claude instance for parallel work
wagner add user-auth api

# 5. When done, clean up
wagner delete user-auth
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
