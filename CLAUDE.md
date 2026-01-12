# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Development Commands

Use `devbox run` for all cargo commands:

```bash
devbox run cargo build                  # Build debug
devbox run cargo build --release        # Build release
devbox run cargo run                    # Run TUI (no subcommand)
devbox run cargo run -- new my-task     # Run with CLI args
devbox run cargo test                   # Run all tests
devbox run cargo test --test integration # Run integration tests only
devbox run cargo test test_name         # Run specific test
RUST_LOG=debug devbox run cargo run     # Enable debug logging
WAGNER_LOG=/tmp/wagner.log devbox run cargo run  # Log to file
```

## Architecture

Wagner is a multi-repo task manager that orchestrates AI agent sessions across git worktrees with tmux. It uses a layered architecture with dependency injection for testing.

### Core Components

**`Wagner<T: Terminal, A: Agent>`** (`src/wagner.rs`) - Main orchestrator generic over terminal and agent implementations. Manages task lifecycle: creating worktrees, spawning tmux sessions, launching agents.

**Terminal trait** (`src/terminal/mod.rs`) - Abstraction over tmux operations. Two implementations:
- `Tmux` - Real tmux interaction
- `MockTerminal` - In-memory for testing

**Agent trait** (`src/agent/mod.rs`) - AI agent abstraction with detection capabilities:
- `ClaudeCode` - Claude Code agent with status detection
- `TestAgent` - For testing

**StatusMonitor** (`src/monitor/mod.rs`) - Polls tmux panes, detects agent types, and determines activity status (working/idle/waiting). Uses content hashing to detect output changes.

### Data Flow

1. CLI (`src/cli/`) parses commands via clap
2. Commands instantiate `Wagner` with real `Tmux` and `ClaudeCode`
3. `Wagner` uses `Store` (`src/store.rs`) for task persistence
4. Tasks stored as JSON in `~/.config/wagner/` and task directories

### TUI

`src/tui/app.rs` is the main TUI application using ratatui. Event handling in `src/tui/event.rs`, rendering in `src/tui/ui.rs`.

### Key Types

- `Task` - Represents a task with repos, worktrees, branches
- `TaskRepo` - Single repo within a task (name, source, worktree path, branch)
- `RepoSource` - Either `Local(PathBuf)` or `Remote(String)` for git URLs
- `RepoSpec` - Parsed from CLI as `name:source:branch`
- `Workspace` - Named collection of repos with base branch

## Testing Strategy

Integration tests use `MockTerminal` and `TestAgent` to verify task/worktree management without real tmux. Tests create temporary git repos with `tempfile`.

## Configuration

Config at `~/.config/wagner/config.json`. Key settings: `tasks_root`, `default_agent`, `workspaces`, `keybindings`.
