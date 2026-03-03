# CLAUDE.md

Contributor/agent guide for this repository.

## Fast Commands

Use `devbox run` for local development commands:

```bash
devbox run cargo build
devbox run cargo run -- --help
devbox run cargo run -- claude
RUST_LOG=debug devbox run cargo run -- daemon start
```

## Architecture Map

High-level architecture is documented in `docs/architecture.md`.

Primary implementation files:

- Orchestration: `src/wagner.rs`
- Task model: `src/model/task.rs`
- Persistence/config: `src/store.rs`, `src/config.rs`
- Terminal abstraction + tmux: `src/terminal/mod.rs`, `src/terminal/tmux.rs`
- Core runtime: `src/core/mod.rs`
- Command semantics: `src/core/command_executor.rs`
- Status derivation: `src/core/status_engine.rs`, `src/monitor/*`
- Transport contracts: `src/transport/mod.rs`
- Daemon loop + IPC integration: `src/transport/daemon.rs`, `src/transport/ipc.rs`
- Telegram adapter: `src/transport/telegram/*`
- CLI surface: `src/cli/mod.rs`, `src/cli/commands/mod.rs`
- TUI app/event/rendering: `src/tui/app.rs`, `src/tui/event.rs`, `src/tui/ui.rs`

## Runtime Paths

- Local path: CLI/TUI -> `Wagner<Terminal, Agent>` -> tmux/store
- Remote path: CLI IPC -> daemon -> `WagnerCore` -> adapter(s)
- Monitoring path: agent JSONL -> watcher/status engine -> `CoreEvent`

## Editing Rules for This Repo

- Keep `CoreCommand`, `CoreResponse`, `CoreEvent` transport-agnostic.
- Keep adapter-specific state inside adapter modules (e.g. Telegram state registry).
- Put command behavior in `command_executor`; avoid duplicating logic in adapters.
- Keep README user-focused (install + quick start), and keep architecture detail in docs.

## When You Need X, Start Here

- Add/modify CLI command: `src/cli/mod.rs` + `src/cli/commands/mod.rs`
- Add remote action: `src/transport/mod.rs` + `src/core/command_executor.rs`
- Change status behavior: `src/core/status_engine.rs` + `src/monitor/*`
- Change daemon lifecycle: `src/transport/daemon.rs`
- Change Telegram UX/callbacks: `src/transport/telegram/{commands,mod,render,state}.rs`
