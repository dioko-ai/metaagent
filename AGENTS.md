# AGENTS.md

## Purpose
This repository requires test coverage for all new code and behavior changes.

## Required Rules
1. Any new functionality must include tests in the same change.
2. Any bug fix must include a regression test that fails before the fix and passes after.
3. Any behavior change must update existing tests or add new ones to reflect the new behavior.
4. Refactors must preserve or improve test coverage for touched code.
5. Do not merge code that adds logic without corresponding tests unless explicitly approved by the user.

## Test Types
- Prefer unit tests for pure logic and state transitions.
- Add integration tests when behavior spans modules.
- Add UI/render tests for TUI-visible behavior when practical.

## Verification Checklist
Before considering work complete:
1. Run `cargo test`.
2. Ensure all tests pass.
3. Ensure new/updated behavior is covered by tests.

## Notes for Agents
- If a requested change cannot be tested directly, explain why and provide the closest practical automated test.
- Keep tests readable and focused on behavior, not implementation details.
