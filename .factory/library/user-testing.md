# User Testing

## Validation Surface

Wagner is a CLI/TUI tool with no web surface. All validation is through:
1. **Compilation**: `devbox run cargo check --all-features` (zero errors)
2. **Tests**: `devbox run cargo test` (all tests pass)
3. **Lint**: `devbox run cargo clippy --all-features` (zero warnings)
4. **Code review**: scrutiny validator reviews implementation quality

## Validation Concurrency

Since validation is purely through `cargo test` / `cargo check` / `cargo clippy` (no running services, no browser), concurrency is limited by compilation:
- Max concurrent validators: 1 (compilation is already parallel internally)
- Rationale: cargo test uses multiple threads internally; running multiple cargo processes would compete for the same build cache and cause lock contention

## Testing Patterns

Existing tests use:
- `tempfile::TempDir` for isolated filesystem fixtures
- Mock `Terminal` trait impl (`src/agent/test.rs`) for tmux abstraction
- Direct struct construction for unit tests
- Integration tests in `tests/` directory (e.g., `tests/sync.rs`)

## No Interactive Testing

There is no running application to interact with. All behavioral assertions are verified through the test suite. The "user testing" validator should run `devbox run cargo test` and `devbox run cargo clippy` and verify they pass.
