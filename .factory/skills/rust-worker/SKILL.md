---
name: rust-worker
description: Implements Rust features with TDD, testing, and verification for the Wagner codebase
---

# Rust Worker

NOTE: Startup and cleanup are handled by `worker-base`. This skill defines the WORK PROCEDURE.

## When to Use This Skill

All implementation features in the Wagner codebase: bug fixes, new agent support, refactoring, UX improvements. Any feature that modifies Rust source code.

## Required Skills

None

## Work Procedure

1. **Read the feature description thoroughly.** Understand preconditions, expectedBehavior, and verificationSteps. Read AGENTS.md for mission boundaries and coding conventions.

2. **Investigate affected code.** Read all files mentioned in the feature description. Understand the existing patterns, data structures, and control flow. Use Grep to find all call sites and references to functions/types you'll modify.

3. **Write failing tests first (RED).** Before any implementation:
   - Create or extend test files with test cases that exercise the expected behavior
   - For bug fixes: write a test that reproduces the bug (currently fails or would fail without the fix)
   - For new features: write tests for the core behavior
   - Run `devbox run cargo test` to confirm tests fail as expected
   - Cover edge cases identified in the feature description

4. **Implement the fix/feature (GREEN).** Make the tests pass:
   - Follow existing code patterns and style
   - For Engine/AgentType enum changes: add match arms in ALL places (use Grep to find every `Engine::` and `AgentType::` match)
   - For refactoring: extract shared logic into helper functions/methods
   - Keep changes minimal and focused

5. **Verify compilation and tests.**
   - Run `devbox run cargo check --all-features` — must produce zero errors
   - Run `devbox run cargo test` — all tests must pass (existing + new)
   - Run `devbox run cargo clippy --all-features -- -D warnings` — zero warnings

6. **Manual verification.** For each behavioral change:
   - Trace the code path mentally to confirm correctness
   - Check that all match arms are exhaustive (no `_ =>` catchalls for Engine/AgentType unless intentional)
   - Verify serde compatibility if model types changed
   - Check that the Terminal trait mock in tests captures the right calls

7. **Commit with descriptive message.** One commit per feature, referencing what was fixed/added.

## Example Handoff

```json
{
  "salientSummary": "Added Engine::Droid variant with launch/resume/process_name/enter_delay methods. Wrote 6 unit tests covering all Engine methods for Droid. cargo test passes (51 tests), cargo clippy clean.",
  "whatWasImplemented": "Added Droid variant to Engine enum in src/model/task.rs with launch_command returning 'droid', resume_command returning 'droid --resume {session_id}', process_name returning 'droid', enter_delay_ms returning 5. Updated all match arms across the codebase (model, wagner.rs, command_executor.rs, daemon.rs, tui/app.rs). Added serde roundtrip test for TrackedPane with Engine::Droid.",
  "whatWasLeftUndone": "",
  "verification": {
    "commandsRun": [
      { "command": "devbox run cargo check --all-features", "exitCode": 0, "observation": "Clean compilation with no errors" },
      { "command": "devbox run cargo test", "exitCode": 0, "observation": "51 tests passed, 0 failed" },
      { "command": "devbox run cargo clippy --all-features -- -D warnings", "exitCode": 0, "observation": "No warnings" }
    ],
    "interactiveChecks": [
      { "action": "Traced Engine::Droid match arms across all files using grep", "observed": "Found and updated 14 match expressions. No missing arms." },
      { "action": "Verified serde serialization of Engine::Droid", "observed": "Serializes as 'droid', deserializes correctly. Existing task.json without droid field still parses." }
    ]
  },
  "tests": {
    "added": [
      {
        "file": "src/model/task.rs",
        "cases": [
          { "name": "engine_launch_command_droid", "verifies": "Droid launch returns 'droid'" },
          { "name": "engine_resume_command_droid", "verifies": "Droid resume returns 'droid --resume {id}'" },
          { "name": "engine_process_name_droid", "verifies": "Droid process name is 'droid'" },
          { "name": "engine_enter_delay_droid", "verifies": "Droid enter delay is 5ms" },
          { "name": "tracked_pane_serde_roundtrip_droid", "verifies": "Engine::Droid serializes/deserializes" },
          { "name": "engine_deser_without_droid", "verifies": "Old task.json still works" }
        ]
      }
    ]
  },
  "discoveredIssues": []
}
```

## When to Return to Orchestrator

- A required type or function doesn't exist yet (dependency on another feature)
- Compilation fails due to changes in a file you don't own
- A test failure reveals a deeper architectural issue
- The feature requires adding a new crate dependency (check with orchestrator first)
- You cannot determine the correct behavior from the feature description
