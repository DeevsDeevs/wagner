# Wagner

Multi-repo task manager for AI agent sessions. Orchestrates agent instances across git worktrees with tmux.

## Features

- **Lightweight mode** - Start sessions on existing repos without worktrees
- **Workspace support** - Define repo groups, create tasks with `-w <workspace>`
- **Multi-pane sessions** - Tmux pane per repo with agent launched automatically
- **Git worktree isolation** - Each task gets isolated worktrees
- **Base branch tracking** - Configure diff base per workspace
- **TUI dashboard** - Monitor all active sessions

## Installation

```bash
# Quick install (recommended)
curl -fsSL https://raw.githubusercontent.com/DeevsDeevs/wagner/main/install.sh | sh

# From crates.io
cargo install wagner

# With Nix
nix profile install github:DeevsDeevs/wagner

# From source
git clone https://github.com/DeevsDeevs/wagner.git
cd wagner && cargo build --release
```

### Updating

```bash
wagner update           # Update to latest version
wagner update --check   # Check for updates without installing
```

## Quick Start

### Lightweight mode (no worktrees)

Start a session on your current repo without creating worktrees or branches:

```bash
cd ~/projects/myrepo
wagner start
# Uses current directory and branch as-is
# Launches tmux session with agent
# Task name derived from repo-branch (e.g., "myrepo-main")

wagner start --name my-session  # Custom name
```

For multiple existing repos:

```bash
# Auto-detect repos in current directory
cd ~/my-project  # Contains frontend/, backend/ subdirs
wagner start
# Detects all git repos, creates pane per repo

# Or specify paths explicitly
wagner start ~/frontend ~/backend --name fullstack
```

To stop tracking (leaves repos untouched):

```bash
wagner detach my-session
```

### Full mode with worktrees

Create isolated worktrees with new branches:

```bash
cd ~/projects/myrepo
wagner new my-feature
# Creates worktree with branch feature/my-feature
# Launches tmux session with agent
```

### Multi-repo with workspace

```bash
# Configure workspace (one-time)
wagner ws add myproject \
  frontend:~/repos/frontend \
  backend:~/repos/backend \
  --base-branch main

# Create task from workspace
wagner new my-feature -w myproject
# Creates worktrees in all repos
# Opens tmux with pane per repo + central pane
```

### Remote repos

Wagner can clone remote repos automatically. Remote repos are cloned once (as bare repos) to `repos_root` and reused across tasks:

```bash
# Add remote repo to task
wagner add-repo my-task api:git@github.com:org/api.git:feature/my-task

# Or in workspace config
wagner ws add myproject \
  frontend:~/local/frontend \
  backend:git@github.com:org/backend.git
```

### Other common commands

```bash
wagner ls                    # List tasks
wagner a my-feature          # Attach to task
wagner cd my-feature         # Open shell in task worktree
wagner rm my-feature         # Delete task
wagner rm my-feature -f      # Delete task + branches
```

## Commands

| Command | Alias | Description |
|---------|-------|-------------|
| `wagner` | | Launch TUI |
| `wagner start [paths]` | `s` | Start session on existing repos (no worktrees) |
| `wagner new <name>` | | Create task with worktrees |
| `wagner new <name> -w <ws>` | | Create from workspace |
| `wagner list` | `ls` | List tasks (`[A]` = attached) |
| `wagner attach [task]` | `a` | Attach to session |
| `wagner detach [task]` | | Stop tracking attached task |
| `wagner add [task] [repo]` | | Add agent pane |
| `wagner add-repo <task> <spec>` | | Add repo to task |
| `wagner rm-repo <task> <repo>` | | Remove repo from task |
| `wagner delete <task>` | `rm` | Delete task (managed only) |
| `wagner cd <task> [repo]` | | Open shell in task worktree |
| `wagner workspace` | `ws` | Manage workspaces |
| `wagner repair` | | Clean up orphaned worktrees |
| `wagner update` | | Update to latest version |

### Task modes

Wagner has two modes for managing tasks:

| | Attached (`start`) | Managed (`new`) |
|---|---|---|
| **Use case** | Quick work on existing branch | Isolated feature work |
| **Worktrees** | No - uses repo directly | Yes - creates git worktree |
| **Branches** | Uses current branch | Creates new branch |
| **Cleanup** | `detach` - just stops tracking | `delete` - removes worktrees |
| **List indicator** | `[A]` prefix | (none) |

### Workspace commands

```bash
wagner ws add <name> <repos...> [-b <base>]  # Create workspace
wagner ws add-repo <ws> <name:path>          # Add repo
wagner ws rm-repo <ws> <name>                # Remove repo
wagner ws ls                                  # List workspaces
wagner ws rm <name>                          # Delete workspace
```

## Configuration

`~/.config/wagner/config.json`:

```json
{
  "tasks_root": "/home/user/tasks",
  "repos_root": "~/repos",
  "default_agent": "claude",
  "diff_base": "main",
  "workspaces": {
    "myproject": {
      "base_branch": "main",
      "frontend": "~/repos/frontend",
      "backend": "~/repos/backend"
    }
  }
}
```

| Setting | Default | Description |
|---------|---------|-------------|
| `tasks_root` | `~/tasks` | Where task directories are created |
| `repos_root` | `~/repos` | Where remote repos are cloned (bare) |
| `default_agent` | `claude` | Agent to launch in panes |
| `diff_base` | `main` | Default branch for diffs |

### Repair & Cleanup

Wagner automatically cleans up on task creation failure. For manual cleanup of orphaned resources:

```bash
wagner repair            # Scan for orphans (dry run)
wagner repair --execute  # Actually clean up
```

This finds and removes:
- Task directories without valid `task.json`
- Worktrees pointing to deleted tasks

## Shell Completions

```bash
eval "$(wagner completions zsh)"   # Zsh
eval "$(wagner completions bash)"  # Bash
wagner completions fish | source   # Fish
```

## TUI Keybindings

| Key | Action |
|-----|--------|
| `j/k` | Navigate |
| `a` | Attach |
| `n` | New task |
| `d` | Delete |
| `c` | View diff |
| `?` | Help |
| `S` | Settings |
| `q` | Quit |

## License

MIT
