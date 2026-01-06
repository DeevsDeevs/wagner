# Wagner

Multi-repo task manager for AI agent sessions. Orchestrates agent instances across git worktrees with tmux.

## Features

- **Workspace support** - Define repo groups, create tasks with `-w <workspace>`
- **Multi-pane sessions** - Tmux pane per repo with agent launched automatically
- **Git worktree isolation** - Each task gets isolated worktrees
- **Base branch tracking** - Configure diff base per workspace
- **TUI dashboard** - Monitor all active sessions

## Installation

```bash
# From crates.io
cargo install wagner

# With Nix
nix profile install github:DeevsDeevs/wagner

# From source
git clone https://github.com/DeevsDeevs/wagner.git
cd wagner && cargo build --release
```

## Quick Start

### Single repo

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

### Other common commands

```bash
wagner ls                    # List tasks
wagner a my-feature          # Attach to task
wagner rm my-feature         # Delete task
wagner rm my-feature -f      # Delete task + branches
```

## Commands

| Command | Alias | Description |
|---------|-------|-------------|
| `wagner` | | Launch TUI |
| `wagner new <name>` | | Create task |
| `wagner new <name> -w <ws>` | | Create from workspace |
| `wagner list` | `ls` | List tasks |
| `wagner attach [task]` | `a` | Attach to session |
| `wagner add [task] [repo]` | | Add agent pane |
| `wagner add-repo <task> <spec>` | | Add repo to task |
| `wagner rm-repo <task> <repo>` | | Remove repo from task |
| `wagner delete <task>` | `rm` | Delete task |
| `wagner workspace` | `ws` | Manage workspaces |

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
