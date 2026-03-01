# Wagner Architecture & UX Reference

Wagner is a multi-repo AI agent orchestrator. You define tasks (collections of git repos), Wagner creates worktrees + tmux sessions, launches AI agents (Claude Code or Codex) in each pane, monitors their status via JSONL file tailing, and lets you interact locally (TUI) or remotely (Telegram).

---

## System Overview

```
                         ┌──────────────┐
                         │  Telegram Bot │ (remote)
                         └──────┬───────┘
                                │ polls / sends
                         ┌──────┴───────┐
                         │    Daemon     │ (background process)
                         │  daemon_tick  │
                         └──────┬───────┘
                                │
              ┌─────────────────┼─────────────────┐
              │                 │                  │
       ┌──────┴──────┐  ┌──────┴──────┐   ┌──────┴──────┐
       │ SessionWatch│  │    Store     │   │    Tmux     │
       │ (JSONL tail)│  │ (task.json)  │   │ (terminal)  │
       └──────┬──────┘  └─────────────┘   └──────┬──────┘
              │                                    │
              │         ┌──────────────┐          │
              └────────►│  Agent Panes │◄─────────┘
                        │ (claude/codex)│
                        └──────────────┘

       ┌──────────────┐
       │     TUI      │ (local, same SessionWatcher)
       └──────────────┘
```

Both the TUI and the Daemon use the same `SessionWatcher` -> JSONL tailing pipeline for status detection. The TUI reads status directly and renders in ratatui. The Daemon translates status transitions into Telegram messages.

---

## Core Concepts

### Task
A named unit of work spanning 1+ git repos. Created via `wagner new` (creates worktrees) or `wagner start` (attaches to existing repos). Stored as JSON at `{task_dir}/.wagner/task.json`.

### TrackedPane
Each pane in a task's tmux session is tracked with:
- `pane_id` — tmux pane ID (`%N`)
- `engine` — ClaudeCode or Codex
- `session_id` — agent session UUID (for resume)
- `jsonl_path` — path to the agent's JSONL output file
- `launched_at` — timestamp

### Engine
Distinguishes agent types. Each has different:
- Launch command: `claude --session-id {id}` vs `codex`
- Resume command: `claude --resume {id}` vs `codex`
- Process name: `claude` vs `codex` (for dead detection via `get_pane_command`)
- JSONL format: different event schemas

---

## CLI Commands

### Task Lifecycle

| Command | What it does |
|---------|-------------|
| `wagner new <name> [-r repos] [-w workspace] [-b branch]` | Create task with git worktrees, tmux session, and prepared agent commands |
| `wagner start [paths] [-n name]` | Attach to existing repos (no worktrees created) |
| `wagner list` | List all tasks |
| `wagner delete [-f] <name>` | Kill session, remove worktrees. `-f` also deletes branches |
| `wagner attach [task]` | Attach to tmux session. Auto-resumes dead agents first |
| `wagner cd <task> [repo]` | Open shell in task's worktree directory |
| `wagner add [task] [repo]` | Add another agent pane to a task |
| `wagner add-repo <task> <spec>` | Add a repo to an existing task |
| `wagner rm-repo <task> <repo>` | Remove a repo from a task |
| `wagner detach [task]` | Stop tracking an attached task (leaves repos untouched) |

### Infrastructure

| Command | What it does |
|---------|-------------|
| `wagner` (no subcommand) | Launch TUI |
| `wagner daemon start` | Run daemon in foreground (connects to Telegram) |
| `wagner daemon status` | Check if daemon is running |
| `wagner repair [--execute]` | Clean up orphaned worktrees (dry-run by default) |
| `wagner completions <shell>` | Generate shell completions |
| `wagner update [--check]` | Update wagner binary |

### Workspaces

| Command | What it does |
|---------|-------------|
| `wagner workspace add <name> <repos...> [-b base]` | Create named repo collection |
| `wagner workspace add-repo <ws> <repo>` | Add repo to workspace |
| `wagner workspace rm-repo <ws> <repo>` | Remove repo from workspace |
| `wagner workspace list` | List workspaces |
| `wagner workspace remove <name>` | Delete workspace |

### Plugins & Chains

| Command | What it does |
|---------|-------------|
| `wagner plugin list` | List available plugins |
| `wagner plugin enable <id>` | Enable a plugin |
| `wagner plugin disable <id>` | Disable a plugin |
| `wagner plugin install-skills` | Install agent skills for enabled plugins |
| `wagner chains list` | List all chains |
| `wagner chains show <chain> [-l link]` | Show chain content |
| `wagner chains promote <chain> [-t task]` | Promote task-local chain to repo level |

---

## Task Creation Flow (step by step)

### `wagner new my-task -w my-workspace`

1. Resolve repos from workspace config (or `--repos`, or auto-detect current git repo)
2. Create task directory: `~/tasks/my-task/`
3. For each repo: create git worktree at `~/tasks/my-task/{repo-name}/`
4. Create tmux session `wagner_my-task`
5. For each repo/pane:
   - Generate UUID session_id
   - Pre-type agent launch command in pane (does NOT execute — user presses Enter)
   - Predict JSONL path from session_id + cwd
   - Store `TrackedPane` in task
6. Save task metadata to `.wagner/task.json`
7. Set up plugin symlinks if enabled

### `wagner start ~/code/my-repo`

1. Detect repos (explicit paths, or auto-detect from cwd)
2. Derive task name from repo/branch (e.g. `my-repo-feature-foo`)
3. Create task pointing at existing repo paths (no worktrees)
4. Create tmux session + panes same as above
5. Register in attached registry (`~/.config/wagner/.attached_registry.json`)

### `wagner attach my-task`

1. Resolve task name (from arg or auto-detect from cwd)
2. Call `resume_dead_agents()` — for each tracked pane, check if agent process is running via `get_pane_command()`. If dead, send resume command
3. Attach terminal to tmux session

---

## JSONL Monitoring Pipeline

This is the core detection system used by both TUI and Daemon.

### Architecture

```
JSONL File (written by agent)
    │
    ▼
PaneWatcher (tails file from last offset)
    │ parse lines
    ▼
AgentEvent (Thinking, ToolProposed, TurnComplete, etc.)
    │
    ▼
StatusDeriver (state machine: Active/Idle/Waiting)
    │ tick() for timeouts
    ▼
PaneStatus (Agent { status, activity } | Terminal | Unknown)
    │
    ▼
SessionWatcher (aggregates per-session)
    │
    ▼
SessionAggregateStatus (NeedsAttention | Working | Idle | Empty)
```

### Agent Events (parsed from JSONL)

| Event | Trigger |
|-------|---------|
| `SessionStarted` | Agent session begins |
| `UserMessage` | User sends input |
| `Thinking` | Agent reasoning |
| `TextOutput` | Agent generating response |
| `ToolProposed { tool_id, tool_name }` | Agent wants to use a tool |
| `ToolCompleted { tool_id }` | Tool finished successfully |
| `ToolRejected { tool_id }` | Tool was denied |
| `TurnComplete` | Agent finished its turn |
| `Progress` | Progress update (no state change) |

### Status Derivation State Machine

```
                    UserMessage / SessionStarted / Thinking / TextOutput
                    ┌──────────────────────────────────────────────────┐
                    │                                                  │
                    ▼                                                  │
              ┌──────────┐                                             │
              │  Active   │──── idle_threshold (2s) no events ───►┌────┴───┐
              │          │                                        │  Idle  │
              │          │◄──── any event ────────────────────────┤        │
              └────┬─────┘                                        └────────┘
                   │                                                  ▲
                   │ ToolProposed + approval_timeout (1s)             │
                   ▼                                                  │
              ┌──────────┐                                            │
              │ Waiting   │──── ToolCompleted / TurnComplete ─────────┘
              │(Approval) │
              │(Question) │  (AskUserQuestion tool → Waiting(Question))
              │(Permission│
              │(Input)    │
              └──────────┘
```

### Activity Types (while Active)

When a tool is proposed, the activity shows what the agent is doing:

| Tool | Activity (Claude) |
|------|-------------------|
| `Bash` | ToolBash |
| `Edit`, `NotebookEdit` | ToolEdit |
| `Write` | ToolWrite |
| `Read`, `Glob`, `Grep` | ToolRead |
| `Agent` | Subagent |
| `WebSearch` | WebSearch |
| `WebFetch` | WebFetch |
| `TodoWrite`, `TaskCreate`, `TaskUpdate` | TodoUpdate |
| (other) | Exploring |
| (no pending tool) | Thinking |

### Pane Status Hierarchy

```
PaneStatus
├── Agent { agent_type, status: AgentStatus }
│   ├── Active(Activity)    — agent is working
│   ├── Waiting(WaitReason) — agent needs input
│   │   ├── Approval        — tool permission (y/n)
│   │   ├── Permission      — similar to approval
│   │   ├── Question        — AskUserQuestion
│   │   └── Input           — free-form input needed
│   └── Idle                — turn complete, waiting for user
├── Terminal(TerminalStatus) — non-agent pane fallback
│   ├── Active
│   └── Idle
└── Unknown
```

### Session Aggregate Status

Computed from all panes in a task's session:

| Status | Condition |
|--------|-----------|
| NeedsAttention | Any pane is Waiting |
| Working | Any pane is Active, none Waiting |
| Idle | All panes Idle |
| Empty | No panes found |

---

## TUI

Launched with `wagner` (no subcommand).

### Layout

```
┌─ Sidebar (28 cols) ─┬─────── Main Area ──────────┐
│ ┌─ Tasks ──────────┐│                             │
│ │ ▼ my-task        ││  [my-task] pane1  ?=help    │
│ │    repo1         ││                             │
│ │    repo2         ││  $ claude --session-id ...  │
│ │   other-task     ││  > Working on feature...    │
│ ├─ Panes ──────────┤│  > Reading src/main.rs      │
│ │ ● %1 repo1       ││  > ...                      │
│ │ ○ %2 repo2       ││                             │
│ └──────────────────┘│                             │
└─────────────────────┴─────────────────────────────┘
```

### Status Icons in Pane List

- `●` Working (green)
- `◉` Waiting / NeedsAttention (red)
- `○` Idle (dim)

### Key Bindings (all configurable)

**Navigation**: `j/k` (up/down), `h/l` (sidebar/terminal), `1-9` (select pane), `Enter` (expand/select), `o` (switch Tasks/Panes section)

**Actions**: `n` (new task), `p` (add pane), `d` (delete), `s` (send message), `a` (attach to tmux), `r` (refresh), `c` (view diffs), `Ctrl+v` (copy mode), `Tab` (Chains tab), `S` (settings), `q` (quit)

**Terminal focus** (after pressing `l`/right): Keys forwarded to tmux pane. `Esc`/`h` returns to sidebar. `Ctrl+e` sends Escape, `Ctrl+t` sends Tab.

### Monitoring

The TUI owns a `SessionWatcher` and polls on every refresh cycle (~100ms):
1. `track_task()` registers all TrackedPanes and their JSONL paths
2. `poll_active()` tails JSONL files, parses events, derives statuses
3. Pane statuses update in the sidebar in real time

---

## Daemon + Telegram

### Starting

```bash
wagner daemon start
```

Requires `daemon.telegram` config with `token` and `chat_id`.

### Daemon Loop

Every tick (~100ms):
1. Reload tasks from store (picks up new tasks)
2. Poll all tmux sessions via SessionWatcher (same JSONL pipeline as TUI)
3. Detect per-pane status transitions
4. Debounce session-level transitions (1s stability, 3s cold-start grace)
5. Emit events to Telegram on state changes
6. Poll Telegram for incoming commands/callbacks
7. Route commands to handlers

### Telegram Notifications

| Event | Telegram Message |
|-------|-----------------|
| Daemon starts | `Wagner Daemon Started` + task list |
| Pane waiting | `🔴 task \| pane — Waiting: Approval` + output tail + `[Approve][Reject][Output]` buttons |
| Pane working (after waiting) | Edits previous message in-place to `🟢 task \| pane — Working` |
| Pane idle (if configured) | `⚪ task \| pane — Idle` + output tail |
| Session status change | `🟢 task — Working` + `[Details]` button |
| Daemon stopping | `Wagner Daemon Stopping` |

### Telegram Commands

| Command | Aliases | Behavior |
|---------|---------|----------|
| `/status` | `/s` | Full status: all tasks + panes + `[Details][Refresh]` buttons |
| `/status <task>` | `/s <task>` | Task drill-down: per-pane status + `[Approve][Output]` per pane + `[Approve All][Back]` |
| `/tasks` | `/list` | List tasks with aggregate status |
| `/approve` | `/y` | Smart: 0 waiting = error, 1 = auto-approve, N = picker buttons |
| `/approve <task> [pane]` | `/y <task>` | Approve specific pane (sends "y" + Enter) |
| `/reject <task> [pane]` | `/n <task>` | Reject (sends "n" + Enter) |
| `/send <task> <msg>` | | Send text to pane + Enter |
| `/output <task> [lines]` | `/o` | Capture last N lines of pane output |
| `/resume <task> [pane]` | | Resume dead agent session |
| `/focus <task> [pane] [--sticky]` | | Suppress notifications from non-matching panes |
| `/unfocus` | | Exit focus mode, shows suppressed count |
| `/help` | `/start` | Command reference |

### Reply-to-Message

Reply to any NeedsAttention notification:
- `y` / `yes` → sends "y" to that pane (approve shortcut)
- `n` / `no` → sends "n" to that pane (reject shortcut)
- Any other text → sent literally (for answering questions/providing input)

Routing: daemon stores `message_id → (task, pane)` mapping when sending NeedsAttention.

### Inline Buttons & Callbacks

Buttons use short callback data (`a:5`, `td:2`) mapped via ID registries to avoid Telegram's 64-byte limit.

| Button | Callback | Action |
|--------|----------|--------|
| Approve | `a:<eid>` | Send "y" + Enter to pane |
| Reject | `r:<eid>` | Send "n" + Enter to pane |
| Output | `o:<eid>` | Capture pane tail, send as message |
| Focus Pane | `fp:<eid>` | Enter focus mode for pane |
| Details | `td:<tid>` | Edit message to show task drill-down |
| Approve All | `aa:<tid>` | Approve all waiting panes in task |
| Focus Task | `ft:<tid>` | Enter focus mode for task |
| Refresh | `sr` | Refresh full status (edit in place) |
| Back | `bk` | Return to full status view |
| Unfocus | `uf` | Clear focus mode |

### Focus Mode

Suppress notifications from panes that don't match the focus target:
- `/focus my-task` — only get notifications from my-task
- `/focus my-task %5` — only from specific pane
- Suppressed notifications increment a counter shown on unfocus
- `--sticky` flag preserved for future use

---

## Resume Flow

### Auto-resume on attach

`wagner attach` calls `resume_dead_agents()` before attaching:
1. Load task's TrackedPanes
2. For each: check `get_pane_command()` (tmux's `#{pane_current_command}`)
3. If process name doesn't match engine's `process_name()` → agent is dead
4. Send `engine.resume_command(session_id)` + Enter to pane

### Manual resume via Telegram

`/resume <task>` does the same check for a single task. Returns error if agent is already running.

### Known gap

The daemon does NOT proactively detect dead agents. There's no notification when an agent crashes — the user must notice on their own or use `wagner attach` (which auto-resumes). Dead-agent detection in `daemon_tick` is a backlog item.

---

## Configuration

Located at `~/.config/wagner/config.json`.

### Key Settings

```jsonc
{
  "tasks_root": "~/tasks",          // where task worktrees go
  "repos_root": "~/repos",          // bare clone cache for remote repos
  "default_agent": "claude",        // "claude" or "codex"
  "diff_base": "main",              // git diff base branch

  // TUI
  "refresh_interval_ms": 100,
  "sidebar_width": 28,
  "page_scroll_lines": 20,
  "capture_lines": 500,
  "show_hints": false,
  "keybindings": { /* fully customizable */ },

  // Terminal
  "terminal": {
    "use_control_mode": true,
    "control_mode_timeout_ms": 5000
  },

  // Monitor (JSONL polling)
  "monitor": {
    "active_poll_ms": 100,
    "background_poll_ms": 2000,
    "idle_threshold_ms": 2000,
    "approval_timeout_ms": 1000,
    "max_lines_per_poll": 1000,
    "daemon_seed_lines": 50
  },

  // Daemon
  "daemon": {
    "poll_interval_ms": 100,
    "telegram": {
      "token": "BOT_TOKEN",
      "chat_id": 12345,
      "notify_idle": false,
      "default_output_lines": 30
    }
  },

  // Workspaces
  "workspaces": {
    "my-ws": {
      "repos": { "repo1": "~/code/r1", "repo2": "~/code/r2" },
      "base_branch": "main"
    }
  },

  // Plugins
  "plugins": {
    "chains": { "enabled": true }
  }
}
```

### Data Storage

- Task metadata: `{task_dir}/.wagner/task.json`
- Attached registry: `{tasks_root}/.attached_registry.json`
- Config: `~/.config/wagner/config.json`
- Chains: `.claude/chains/{chain-name}/` (per-repo, symlinked into tasks)

---

## Known Gaps & Backlog

| # | Item | Priority | Notes |
|---|------|----------|-------|
| 1 | Dead-agent detection in daemon_tick | high | Auto-resume + notify when agent crashes. Currently no signal to Telegram user. |
| 2 | Remove `resume_command()` from Agent trait | low | Duplicates `Engine::resume_command()`. Only Engine version used in production. |
| 3 | Auto-unfocus on idle (30s) | low | Focus mode stays on until manual `/unfocus`. |
| 4 | Better pane names (repo_name) | low | Pane titles show tmux IDs, not repo names. |
