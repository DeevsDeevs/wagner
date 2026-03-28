# Environment

## Toolchain
- Rust 1.93.0 (edition 2024) via devbox
- All commands must use `devbox run cargo ...`

## Required System Tools
- tmux (execution boundary for all agent panes)
- git (worktree management, repo operations)

## Agent CLIs
- `claude` - Claude Code CLI
- `codex` - OpenAI Codex CLI
- `droid` - Factory Droid CLI (being added)

## JSONL Paths
- Claude: `~/.claude/projects/{project_id}/{session_id}.jsonl` (project_id = cwd with `/` and `.` replaced by `-`)
- Droid: `~/.factory/sessions/{project_id}/{session_id}.jsonl` (project_id = cwd with `/` replaced by `-`)
- Codex: No JSONL output

## Feature Flags
- `telegram` feature (default): enables Telegram adapter via teloxide crate
- Build without: `cargo build --no-default-features`

## Dev Dependencies
- `tempfile` crate for test fixtures
