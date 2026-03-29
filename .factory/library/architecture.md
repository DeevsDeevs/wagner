# Architecture

## Overview

Wagner is a tmux-based orchestrator for AI coding agents. It manages agent sessions across repositories using tmux panes, monitors agent activity via JSONL output parsing, and supports remote control through adapters (Telegram).

## Core Components

### Wagner<T, A> (src/wagner.rs)
The main orchestrator for local flows. Generic over `Terminal` trait and `Agent` trait. Handles:
- Task lifecycle: create, start, detach, delete
- Session/pane management: creating tmux sessions, adding panes, launching agents
- Agent resume: detecting dead agents and relaunching them
- Quick-launch: single-command workflow for starting agents in current repo

### WagnerCore (src/core/mod.rs)
Wraps StatusEngine + CommandExecutor for the daemon path. Processes CoreCommand/CoreResponse/CoreEvent.

### Engine Enum (src/model/task.rs)
Defines supported agent types: ClaudeCode, Codex, Terminal, and Droid. Each variant provides:
- `launch_command(session_id)` - command to start the agent
- `resume_command(session_id)` - command to resume a session
- `process_name()` - for dead-agent detection
- `enter_delay_ms()` - delay between text send and Enter key in tmux

### Agent Trait (src/agent/mod.rs)
Higher-level agent abstraction with `predict_jsonl_path()` for JSONL file discovery. AgentChoice dispatches between implementations (Claude, Codex, and Droid).

### Terminal Trait (src/terminal/mod.rs)
Abstracts tmux operations. Key methods: create_session, create_pane, send_text_enter, send_keys, capture, kill_pane. Real impl in tmux.rs.

### Monitoring Pipeline (src/monitor/)
- `SessionWatcher` (watcher.rs): tails JSONL files per pane, dispatches to engine-specific parsers
- `claude_events.rs` / `codex_events.rs` / `droid_events.rs`: parse JSONL events into `AgentEvent`
- `StatusEngine` (src/core/status_engine.rs): aggregates pane events into session-level status with debouncing
- `StatusMonitor` (src/monitor/mod.rs): TUI's own status monitoring via terminal capture + agent detection

### Transport Layer (src/transport/)
- `CoreCommand/CoreResponse/CoreEvent` (mod.rs): transport-agnostic message types
- `daemon.rs`: polling loop, health checks, IPC serving, adapter integration
- `ipc.rs`: Unix socket IPC for CLI-to-daemon communication
- `telegram/`: Telegram bot adapter for remote control

## Data Flow

### Local Path
CLI/TUI -> Wagner<Terminal, Agent> -> tmux -> agent process
                                    -> store (task.json persistence)

### Remote Path
CLI -> IPC socket -> Daemon -> WagnerCore -> CommandExecutor -> tmux
                            -> StatusEngine -> Adapter (Telegram)

### Monitoring Path
Agent writes JSONL -> SessionWatcher tails file -> engine parser -> AgentEvent
                   -> StatusEngine debounces -> CoreEvent -> Adapter notification

## Dual Dispatch Pattern
There are TWO paths to launch agents:
1. `prepare_agent_in_pane()` - uses Agent trait (for managed task creation)
2. `prepare_agent_in_pane_with_engine()` - uses Engine enum directly (for quick_launch, add_pane)

The command_executor `AddPane` path delegates to shared Wagner pane-creation logic to keep daemon/local behavior aligned.

## Persistence
- Config: `~/.config/wagner/config.json`
- Task metadata: `<task_dir>/.wagner/task.json`
- Attached registry: `<tasks_root>/.attached_registry.json`
- Daemon: `~/.config/wagner/daemon.sock`, `daemon.pid`
- Telegram state: `~/.config/wagner/telegram_state.json`

## Key Invariants
1. CoreCommand/CoreResponse/CoreEvent are transport-agnostic
2. Status is derived from JSONL + watcher state, not UI state
3. tmux is the execution boundary; all sends/captures route through Terminal trait
4. Adapters transform core events/commands, never re-implement business logic
5. Task metadata in .wagner/task.json is source of truth for tracked panes
