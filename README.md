# Wagner

Wagner is a tmux-based orchestrator for running AI agents across one or many repos.

It supports three primary workflows:
- quick launch in the current repo (`wagner claude`, `wagner codex`)
- attached multi-repo sessions without worktrees (`wagner start`)
- managed tasks with isolated git worktrees (`wagner new`)

## Install

```bash
# Recommended
curl -fsSL https://raw.githubusercontent.com/DeevsDeevs/wagner/main/install.sh | sh

# Cargo
cargo install wagner

# Nix
nix profile install github:DeevsDeevs/wagner
```

## Requirements

- `tmux`
- `git`
- at least one agent CLI (`claude` and/or `codex`)

## Quick Start (Simplest)

From inside any git repo:

```bash
wagner claude      # start/attach a Claude task for current directory
wagner codex       # start/attach a Codex task for current directory
wagner terminal    # start/attach a plain terminal task
```

Optional custom task name:

```bash
wagner claude -n my-task
```

Useful follow-ups:

```bash
wagner ls
wagner a my-task
wagner detach my-task
```

## Multi-Repo Without Worktrees

```bash
# auto-detect repo(s) from current directory
wagner start

# explicit paths
wagner start ~/repos/frontend ~/repos/backend -n fullstack

# stop tracking (repos stay untouched)
wagner detach fullstack
```

## Managed Worktree Tasks

```bash
# single repo (auto-detected from cwd)
wagner new my-feature

# workspace-based multi-repo task
wagner workspace add app frontend:~/repos/frontend backend:~/repos/backend
wagner new my-feature -w app
```

Cleanup:

```bash
wagner rm my-feature        # remove task/worktrees
wagner rm my-feature -f     # also remove task branches
```

## Daemon + Remote Control

Start daemon:

```bash
wagner daemon start
wagner daemon status
wagner daemon restart
wagner daemon stop
```

Once daemon is running, these commands use IPC:

```bash
wagner status [task]
wagner send <task> "message"
wagner approve [task] [pane]
wagner reject <task> [pane]
wagner output <task> [pane] -l 120
wagner resume <task> [pane]
wagner add [task] [repo] [--name pane-name] [--agent claude|codex|terminal]
```

## Telegram Adapter (Optional)

Configure `~/.config/wagner/config.json`:

```json
{
  "daemon": {
    "telegram": {
      "bot_token": "<telegram_bot_token>",
      "chat_id": 123456789,
      "allowed_users": [123456789],
      "notify_waiting": true,
      "rate_limit_ms": 50
    }
  }
}
```

Then run `wagner daemon start`.

## Minimal Config

Default config path: `~/.config/wagner/config.json`

```json
{
  "tasks_root": "~/tasks",
  "repos_root": "~/repos",
  "default_agent": "claude",
  "diff_base": "main"
}
```

## More Docs

- Architecture: `docs/architecture.md`
- Contributor map: `CLAUDE.md`

## License

MIT
