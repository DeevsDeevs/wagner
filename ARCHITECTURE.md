# Wagner Architecture

Multi-repo task manager for agents sessions with chain integration.

## Problem Statement

Working on features that span multiple repositories requires:
- Switching between repos manually
- Losing context when jumping between Claude sessions
- No unified view of multi-repo task progress
- Chain context scattered across repos

Claude Squad handles single-repo sessions well. Wagner adds the **multi-repo task** layer on top.

## Core Concept

```
Task = Collection of repos working toward one goal
     = N worktrees + M Claude sessions + chain context per repo
```

**Example**: "Add OAuth to platform"
- `api-service` - backend auth endpoints
- `web-app` - frontend auth flow
- `shared-types` - auth DTOs

Each repo can have its own Claude sessions and chains. Wagner shows them unified.

## What Wagner Is NOT

- Not a replacement for claude-squad (different scope)
- Not a Claude Code wrapper (Claude runs in tmux, wagner observes)
- Not a git client (just creates worktrees, user manages branches)

## Design Decisions

### Why Not Extend Claude Squad?

Claude Squad is session-centric (1 repo = 1 session = 1 branch). Wagner is task-centric (1 task = N repos). Different mental models, different tools.

### Status Detection: Polling

After studying claude-squad's implementation, we chose **polling** for status detection:

- Captures tmux pane output periodically
- Hashes content to detect changes (output changed = running)
- Pattern-matches for agent-specific prompts (waiting for input)
- Works with any CLI agent (Claude, Aider, OpenCode, Gemini, Amp)
- No agent-side setup required

Key improvements over claude-squad:
- **Pane-level status** (not just session-level)
- **Agent detection** (distinguish agent panes from plain terminals)
- **Adapter pattern** (easy to add new agents)
- **ANSI stripping** (consistent hashing)

### Session Ownership

Who spawns Claude instances?

Option A: Wagner spawns Claude automatically
- Pros: Full control, auto-restart
- Cons: Complex, prescriptive, error-prone

Option B: User spawns Claude manually
- Pros: Simple, flexible, user controls prompts
- Cons: Extra step

Decision: **Option B**. Wagner creates the tmux session/panes. User runs `claude` (or any agent) when ready. Wagner observes via polling.

### Worktree Location

Option A: Hidden (e.g., `~/.wagner/worktrees/`)
Option B: Visible inside task folder

Decision: **Option B**. User should see and access worktrees directly.

```
~/tasks/oauth-feature/
├── .wagner/        # wagner metadata
├── api-service/    # worktree
├── web-app/        # worktree
└── shared-types/   # worktree
```

## Data Model

### Minimal Viable Model

```rust
struct Task {
    name: String,           // "oauth-feature"
    path: PathBuf,          // ~/tasks/oauth-feature
    repos: Vec<TaskRepo>,
    created_at: DateTime,
}

struct TaskRepo {
    name: String,           // "api-service"
    source: PathBuf,        // ~/code/api-service (original repo)
    worktree: PathBuf,      // ~/tasks/oauth-feature/api-service
    branch: String,         // "feature/oauth"
}

struct TrackedPane {
    handle: PaneHandle,             // tmux pane id
    agent_type: Option<AgentType>,  // None = plain terminal
    status: PaneStatus,
    output_hash: [u8; 32],
    last_change: Instant,
}

enum PaneStatus {
    // Agent statuses
    AgentStarting,
    AgentRunning,
    AgentWaiting,
    AgentIdle,
    AgentError(String),
    // Terminal statuses
    TerminalActive,
    TerminalIdle,
    Unknown,
}
```

### What We Dropped

- **Chain as entity**: Chains live in `.claude/chains/`. Wagner just reads them. No separate Chain struct.
- **Session name**: Auto-generated from task/repo/n. User doesn't name sessions.
- **Default layout config**: Hardcode 1+2 layout. Configure later if needed.

## Code Architecture

### Layered Design (Hexagonal / Ports-Adapters)

```
         ┌─────────┐     ┌─────────┐
         │   CLI   │     │   TUI   │      ← Interfaces (thin)
         └────┬────┘     └────┬────┘
              │               │
              └───────┬───────┘
                      ▼
              ┌──────────────┐
              │    Wagner    │             ← Application (orchestrator)
              │  <T, A>      │
              └──────┬───────┘
                     │
       ┌─────────────┼─────────────┐
       ▼             ▼             ▼
  ┌─────────┐  ┌──────────┐  ┌─────────┐
  │Terminal │  │  Agent   │  │  Store  │  ← Ports (traits)
  │ (trait) │  │ (trait)  │  │         │
  └────┬────┘  └────┬─────┘  └─────────┘
       │            │
  ┌────┴────┐  ┌────┴─────┐
  │  Tmux   │  │ClaudeCode│                ← Adapters (implementations)
  │(Ghostty)│  │(OpenCode)│
  └─────────┘  └──────────┘
```

**Key principles:**
- **Core is I/O-free**: Domain models have no dependencies
- **Traits define boundaries**: Terminal, Agent are swappable
- **Interfaces are thin**: CLI/TUI just parse input and call Wagner
- **Wagner is generic**: `Wagner<T: Terminal, A: Agent>` - testable, flexible

### Project Layout

```
wagner/
├── Cargo.toml
├── flake.nix                    # Nix/devbox support
├── src/
│   ├── lib.rs                   # Library crate (re-exports public API)
│   ├── main.rs                  # Binary entry point
│   │
│   ├── error.rs                 # WagnerError, WagnerResult
│   ├── config.rs                # Paths, global config
│   │
│   ├── model/                   # Domain models (pure, no I/O)
│   │   ├── mod.rs
│   │   ├── task.rs              # Task, TaskRepo
│   │   └── session.rs           # Session, SessionStatus
│   │
│   ├── terminal/                # Terminal abstraction
│   │   ├── mod.rs               # pub trait Terminal
│   │   └── tmux.rs              # impl Terminal for Tmux
│   │   └── (ghostty.rs)         # future
│   │
│   ├── agent/                   # Agent detection (polling-based)
│   │   ├── mod.rs               # pub trait AgentDetector
│   │   ├── claude.rs            # ClaudeCodeDetector
│   │   ├── aider.rs             # AiderDetector
│   │   ├── terminal.rs          # TerminalDetector (fallback)
│   │   └── (opencode.rs)        # future
│   │
│   ├── monitor/                 # Status monitoring
│   │   ├── mod.rs               # StatusMonitor
│   │   └── ansi.rs              # ANSI stripping utility
│   │
│   ├── store.rs                 # JSON persistence
│   │
│   ├── wagner.rs                # Orchestrator: Wagner<T, A>
│   │
│   ├── cli/                     # CLI interface
│   │   ├── mod.rs               # Clap definition + dispatch
│   │   └── commands/
│   │       ├── mod.rs
│   │       ├── new.rs
│   │       ├── list.rs
│   │       ├── delete.rs
│   │       ├── add.rs
│   │       ├── attach.rs
│   │       └── send.rs
│   │
│   └── tui/                     # TUI interface (Phase 3)
│       ├── mod.rs
│       ├── app.rs               # TUI state machine
│       ├── input.rs             # Key handling
│       └── widgets/
│           ├── mod.rs
│           ├── tasks.rs
│           ├── sessions.rs
│           ├── preview.rs       # Renders tmux capture
│           └── chains.rs
```

### Runtime Data

```
~/.config/wagner/
├── config.json          # global settings
└── sessions.json        # active sessions (ephemeral)

~/tasks/                  # default task root (configurable)
└── oauth-feature/
    ├── .wagner/
    │   └── task.json    # task metadata
    ├── api-service/     # worktree
    ├── web-app/         # worktree
    └── shared-types/    # worktree
```

## Core Traits

### Terminal Trait

Abstracts terminal multiplexer (tmux now, ghostty later):

```rust
/// Handle to a terminal session (e.g., tmux session)
#[derive(Clone, Debug)]
pub struct SessionHandle(pub String);

/// Handle to a terminal pane (e.g., tmux pane %5)
#[derive(Clone, Debug)]
pub struct PaneHandle(pub String);

/// Terminal multiplexer abstraction
pub trait Terminal: Send + Sync {
    /// Create a new session with given name, starting in cwd
    fn create_session(&self, name: &str, cwd: &Path) -> Result<SessionHandle>;

    /// Create a new pane in session, starting in cwd
    fn create_pane(&self, session: &SessionHandle, cwd: &Path) -> Result<PaneHandle>;

    /// Capture current pane content (for TUI preview)
    fn capture(&self, pane: &PaneHandle, lines: usize) -> Result<String>;

    /// Send keystrokes to pane
    fn send_keys(&self, pane: &PaneHandle, keys: &str) -> Result<()>;

    /// Attach terminal to session (takes over terminal)
    fn attach(&self, session: &SessionHandle) -> Result<()>;

    /// List all panes in session
    fn list_panes(&self, session: &SessionHandle) -> Result<Vec<PaneHandle>>;

    /// Kill a pane
    fn kill(&self, pane: &PaneHandle) -> Result<()>;

    /// Check if session exists
    fn session_exists(&self, name: &str) -> Result<bool>;
}
```

**Implementations:**
- `Tmux` - shells out to `tmux` CLI
- `Ghostty` (future) - uses Ghostty's scripting API

### AgentDetector Trait

Abstracts agent-specific status detection via polling:

```rust
/// Type of AI coding agent
#[derive(Clone, Debug, PartialEq)]
pub enum AgentType {
    ClaudeCode,
    Aider,
    OpenCode,
    Gemini,
    Amp,
    Unknown(String),
}

/// Status of a single pane
#[derive(Clone, Debug, PartialEq)]
pub enum PaneStatus {
    // Agent statuses
    AgentStarting,      // Agent detected, initializing
    AgentRunning,       // Output changing (working)
    AgentWaiting,       // Prompt detected (needs input)
    AgentIdle,          // Stable output, ready for prompt
    AgentError(String),

    // Terminal (no agent detected)
    TerminalActive,     // Recent output changes
    TerminalIdle,       // No recent changes

    Unknown,
}

/// Agent detection and status parsing
pub trait AgentDetector: Send + Sync {
    /// Agent type identifier
    fn agent_type(&self) -> AgentType;

    /// Command that launches this agent
    fn launch_command(&self) -> &str;

    /// Check if this agent is running in pane
    fn detect_agent(&self, pane_command: &str, output: &str) -> bool;

    /// Detect status from pane output
    fn detect_status(&self, output: &str, output_changed: bool, since_change: Duration) -> PaneStatus;

    /// Patterns indicating "waiting for input"
    fn waiting_patterns(&self) -> &[&str];

    /// Patterns indicating "running/thinking"
    fn running_patterns(&self) -> &[&str];
}
```

**Implementations:**
- `ClaudeCodeDetector` - detects Claude Code prompts and spinner
- `AiderDetector` - detects Aider confirmation prompts
- `OpenCodeDetector` - detects OpenCode patterns
- `GeminiDetector` - detects Gemini CLI patterns
- `TerminalDetector` - fallback for plain shells

## Status Monitoring

Wagner monitors pane status via polling. This works for:
- **AI agents** (Claude, Aider, OpenCode, etc.) - detects waiting/running/idle
- **Plain terminals** - tracks activity for any process (build servers, logs, etc.)

Wagner can be used purely as a terminal/pane manager without any AI agents.

### Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      StatusMonitor                          │
│  - polls panes every N ms (configurable: status_poll_ms)    │
│  - maintains pane state cache                               │
│  - strips ANSI before hashing                               │
└─────────────────────────────────────────────────────────────┘
                              │
              ┌───────────────┼───────────────┐
              ▼               ▼               ▼
       ┌──────────┐    ┌──────────┐    ┌──────────┐
       │  Claude  │    │  Aider   │    │ Terminal │
       │ Detector │    │ Detector │    │ Detector │
       └──────────┘    └──────────┘    └──────────┘
```

### Configuration

```rust
// In config.rs
pub struct Config {
    // ...
    #[serde(default = "default_status_poll_ms")]
    pub status_poll_ms: u64,  // Default: 500ms
}
```

Configurable via settings popup or `~/.config/wagner/config.json`.

### Pane Tracking

```rust
/// Extended pane info with status
pub struct TrackedPane {
    pub handle: PaneHandle,
    pub agent_type: Option<AgentType>,  // None = plain terminal
    pub status: PaneStatus,
    pub output_hash: [u8; 32],
    pub last_change: Instant,
}

/// Session aggregate status (derived from panes)
pub enum SessionAggregateStatus {
    NeedsAttention,  // At least one pane waiting for input
    Working,         // At least one pane running, none waiting
    Idle,            // All panes idle
    Empty,           // No panes
}
```

### Detection Flow

1. **Capture**: `tmux capture-pane` gets pane content
2. **Strip ANSI**: Remove color codes for consistent hashing
3. **Hash**: SHA256 of cleaned output
4. **Compare**: Changed hash = activity detected
5. **Detect Agent**: Check pane command + output patterns
6. **Detect Status**: Agent-specific pattern matching
7. **Update Cache**: Store new status and hash

### Agent Detection Order

Detectors are checked in priority order until one matches:

1. ClaudeCode (looks for `claude` command, `╭─` TUI border)
2. Aider (looks for `aider` command)
3. OpenCode (looks for `opencode` command)
4. Gemini (looks for `gemini` command)
5. Amp (looks for `amp` command)
6. **Fallback**: Plain terminal (no agent)

### Detection Patterns

| Agent | Waiting Patterns | Running Patterns |
|-------|------------------|------------------|
| Claude | `[Y/n]`, `[y/N]`, `No, and tell Claude what to do differently` | `⠋⠙⠹⠸`, `...`, `Thinking` |
| Aider | `(Y)es/(N)o/(D)on't ask again` | Output changing |
| Gemini | `Yes, allow once` | Output changing |
| Terminal | N/A | Output changing |

### Status State Machine

```
[pane created] ──────────────────► Unknown
                                      │
              ┌───────────────────────┼───────────────────────┐
              │ agent detected        │ no agent              │
              ▼                       ▼                       │
         AgentStarting          TerminalIdle                  │
              │                       │                       │
              │ output changed        │ output changed        │
              ▼                       ▼                       │
         AgentRunning           TerminalActive                │
              │                       │                       │
              ├─► waiting pattern ──► AgentWaiting            │
              │                            │                  │
              │ no change for N sec        │ output changed   │
              ▼                            ▼                  │
         AgentIdle ◄───────────────── AgentRunning            │
              │                                               │
              │ output changed                                │
              └───────────────────────────────────────────────┘
```

## TUI Design

### Preview Pane (Viewing tmux in TUI)

The TUI shows live tmux content without requiring attachment:

1. **Capture**: `Terminal::capture(pane, 50)` gets last 50 lines from tmux pane
2. **Render**: ratatui `Paragraph` widget displays the captured text
3. **Refresh**: Tick every 500ms to update preview
4. **ANSI**: ratatui handles ANSI color codes from tmux output

```rust
// Simplified preview update loop
fn update_preview(&mut self) {
    if let Some(session) = self.selected_session() {
        let output = self.terminal.capture(&session.pane, 50)?;
        self.preview_content = output;
    }
}
```

The user sees Claude's output in real-time without leaving the TUI. Press `Enter` to fully attach.

### Layout

```
┌─────────────────────────────────────────────────────────────────┐
│ WAGNER                                            [oauth-feature]│
├──────────────────┬──────────────────────────────────────────────┤
│ TASKS            │ SESSIONS                                     │
│                  │                                              │
│ > oauth-feature  │ api-service/1    [running]   2m ago          │
│   refactor-auth  │ api-service/2    [waiting]   30s ago         │
│                  │ web-app/1        [stopped]   10m ago         │
│                  │                                              │
├──────────────────┼──────────────────────────────────────────────┤
│ REPOS            │ PREVIEW                                      │
│                  │                                              │
│ > api-service    │ $ claude                                     │
│   web-app        │ > Implementing OAuth flow for /auth/login... │
│   shared-types   │                                              │
│                  │ Changes:                                     │
│ CHAINS           │   src/auth/handler.rs (+45, -12)             │
│                  │   src/auth/mod.rs (+3, -0)                   │
│ > auth-flow (3)  │                                              │
│   error-handling │ Would you like me to continue?               │
│                  │ [Y/n]                                        │
├──────────────────┴──────────────────────────────────────────────┤
│ a:add  d:detach  e:enter  s:send  c:chains  q:quit              │
└─────────────────────────────────────────────────────────────────┘
```

### Keybindings

| Key | Action |
|-----|--------|
| `j/k` | Navigate lists |
| `Tab` | Switch focus (tasks -> repos -> sessions) |
| `Enter` | Attach to selected session |
| `a` | Add new Claude pane to selected repo |
| `d` | Detach (return to TUI from attached session) |
| `s` | Send message to selected session |
| `c` | Browse chains for selected repo |
| `q` | Quit |

## CLI Commands

### `wagner new <name> --repos <spec>`

Create new task with worktrees.

```bash
# From existing local repos
wagner new oauth-feature --repos api-service:~/code/api-service:feature/oauth

# From git URLs (clones first)
wagner new oauth-feature --repos api-service:git@github.com:org/api.git:feature/oauth

# Mixed
wagner new oauth-feature --repos api:~/code/api:feature/oauth,web:git@github.com:org/web.git:feature/oauth
```

Repo spec: `<name>:<source>:<branch>` where source is path or git URL (auto-detected).

### `wagner delete <task>`

Remove task and worktrees.

```bash
wagner delete oauth-feature          # Interactive prompt: keep branches?
wagner delete oauth-feature --force  # Remove everything (worktrees + branches)
```

### `wagner list`

List all tasks with status summary.

```
oauth-feature    3 repos   2 active   ~/tasks/oauth-feature
refactor-auth    1 repo    0 active   ~/tasks/refactor-auth
```

### `wagner add [task] [repo]`

Add Claude pane to repo. Task defaults to current (from cwd). Repo defaults to first.

### `wagner attach <task>/<session>`

Attach to specific session.

```bash
wagner attach oauth-feature/api-service/1
```

### `wagner send <task>/<session> "message"`

Send message without attaching.

### `wagner chains [task] [repo]`

List chains for repo.

### `wagner` (no args)

Launch TUI.

## State Persistence

### Task State (`task.json`)

```json
{
  "name": "oauth-feature",
  "repos": [
    {
      "name": "api-service",
      "source": "/home/user/code/api-service",
      "worktree": "/home/user/tasks/oauth-feature/api-service",
      "branch": "feature/oauth"
    }
  ],
  "created_at": "2025-01-03T12:00:00Z"
}
```

### Session State (`sessions.json`)

```json
{
  "sessions": [
    {
      "id": "a1b2c3",
      "task": "oauth-feature",
      "repo": "api-service",
      "tmux_pane": "%5",
      "status": "running",
      "last_activity": "2025-01-03T12:05:00Z"
    }
  ]
}
```

Sessions are ephemeral. If tmux pane dies, session is removed on next sync.

## Chain Integration

Chains are read-only from Wagner's perspective. Chain files live in each repo's `.claude/chains/`.

### Display

When browsing a repo, show its chains:
- Chain name
- Number of links
- Most recent link summary

### View

Allow viewing chain link content in preview pane.

### No Write

Wagner does not create chains. That's Claude's job via `/chain-link`.

## Implementation Phases

### Phase 1: Core CLI (MVP)

**Goal**: Create tasks, manage worktrees, basic session tracking.

1. `wagner new` - create task with worktrees
2. `wagner list` - list tasks
3. Task state persistence
4. Basic tmux session/pane creation

**Validate**: Can create multi-repo task, worktrees work.

### Phase 2: Status Monitoring

**Goal**: Real-time pane status via polling.

1. `AgentDetector` trait and implementations
2. `StatusMonitor` with polling loop
3. ANSI stripping utility
4. Pane-level status tracking
5. Session aggregate status

**Validate**: Status reflects actual state within configurable poll interval.

### Phase 3: TUI (Basic)

**Goal**: Visual task/session management.

1. Task list widget
2. Session list widget
3. Preview pane (tmux capture)
4. Basic navigation
5. Attach/detach

**Validate**: Can see all tasks/sessions, attach to any.

### Phase 4: TUI (Complete)

**Goal**: Full TUI experience.

1. Send message without attach
2. Chain browser
3. Keybindings polish
4. Status colors/indicators

**Validate**: Full workflow without leaving TUI.

### Phase 5: Polish

**Goal**: Production ready.

1. Error handling
2. Config file
3. CLI help/docs
4. Edge cases (orphan sessions, dead panes)

## Distribution

### Install Methods

```bash
# Nix/devbox
nix run github:deevs/wagner
devbox add github:deevs/wagner

# Cargo (compiles from source)
cargo install wagner

# Direct binary (GitHub releases)
curl -L https://github.com/deevs/wagner/releases/latest/download/wagner-linux-x86_64 -o wagner
```

### Supported Platforms

- Linux x86_64
- Linux aarch64
- macOS x86_64
- macOS aarch64 (Apple Silicon)

## Risks and Mitigations

### Risk: Detection pattern fragility

Agent prompt patterns may change between versions.

**Mitigation**: Patterns are in adapters, easy to update. Fall back to hash-based change detection.

### Risk: Tmux complexity

Tmux pane/session management has edge cases.

**Mitigation**: Study claude-squad's tmux wrapper. Keep abstraction thin.

### Risk: Scope creep

Easy to add features (auto-prompts, AI orchestration, etc).

**Mitigation**: Strict MVP. Wagner is a viewer/organizer, not an orchestrator.

## Success Criteria

1. Can create multi-repo task in < 30 seconds
2. Pane status reflects reality within poll interval (configurable, default 500ms)
3. Can switch between any pane in 2 keystrokes
4. Chain context visible without leaving TUI
5. No data loss on crash (state persisted)
6. Works with any CLI agent or plain terminals

## Out of Scope (V1)

- Auto-starting agent sessions
- Session orchestration (sequencing, dependencies)
- Multi-machine sync
- Web UI
- Ghostty backend (until API exists)
