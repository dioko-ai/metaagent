# Architecture Cutover Guide

This document defines the post-cutover module boundaries, CLI usage contracts, and transport-extension guidance.

Primary architecture reference:
- GitHub Well-Architected architecture checklist: https://wellarchitected.github.com/library/architecture/checklist/

## Module Boundaries

- `src/app.rs`
  - Owns user-facing state transitions and workflow projection into panes/messages.
  - Does not own persistence or transport-specific I/O.
- `src/workflow.rs`
  - Owns task-graph validation, execution ordering, retry state, and failure progression.
  - Transport-agnostic orchestration core.
- `src/session_store.rs`
  - Owns session lifecycle and durable artifacts (`tasks.json`, `planner.md`, `rolling_context.json`, `task-fails.json`, project/session metadata).
- `src/services.rs`
  - Owns orchestration and prompt-preparation service seams used by runtime (`CoreOrchestrationService`, `UiPromptService`).
  - `main.rs` should call these services instead of duplicating orchestration helpers.
- `src/agent.rs` and `src/agent_models.rs`
  - Own backend process command defaults and routing/config merge behavior.
  - Keep backend-selection resolution and per-agent command composition here, not in UI state types.
- `src/api/`
  - Owns transport-facing contracts (`contracts.rs`), envelopes (`envelope.rs`), and capability matrix (`capabilities.rs`).
- `src/main.rs`
  - Composition root: wires adapters, event loop, and CLI command dispatch.
  - Keep business rules in `app`, `workflow`, `session_store`, or `services`.

## CLI Usage Contract

Use the API command tree for scriptable behavior:

- `metaagent-rust api capability list|get`
- `metaagent-rust api app ...`
- `metaagent-rust api workflow ...`
- `metaagent-rust api session ...`

Automation expectations:

- Prefer `--output json`.
- JSON success envelope: `{ "status": "ok", "summary": "...", "data": ... }`
- JSON error envelope: `{ "status": "err", "error": { "code": ..., "message": ..., "retryable": ..., "details": ... } }`
- Exit-code mapping is defined in `src/main.rs` (`exit_code_for_error`).
- Parse-time argument errors are emitted on `stderr`; domain errors in JSON mode are emitted on `stdout`.

See also `docs/cli-parity-checklist.md` and `scripts/cli_scriptability_examples.sh`.

## Future Transport Extension

When adding a new transport adapter:

1. Add a new `TransportAdapter` implementation in `src/main.rs` (or extract `transport/` module once adapter count grows).
2. Map transport inputs into `api::RequestEnvelope<api::ApiRequestContract>`.
3. Route through `execute_core_api_contract` to reuse the same core behavior and error contracts.
4. Populate `RequestMetadata.transport` and `RequestMetadata.actor` with stable values.
5. Keep adapter-specific behavior limited to parsing/formatting; do not fork orchestration logic.
6. If a new capability is introduced, update `src/api/capabilities.rs` with request/response contracts and code paths.

## Hardening Checklist for Changes

Before merging architecture-affecting changes:

- Confirm each change maps to a single owning module boundary above.
- Confirm failure modes remain explicit and serialized through API envelopes.
- Confirm transport adapters stay thin and contract-driven.
- Confirm operator-facing docs stay aligned (`docs/cli-parity-checklist.md`, this guide).

## Session Guardrails

- Runtime backend selection uses `~/.metaagent/config.toml` (`[backend].selected`), including `/backend` picker updates.
- Session artifacts under `src/session_store.rs` remain scoped to per-session state only (`tasks.json`, planner/context/failure metadata).
- Backend changes apply to newly created adapters in the current run; in-flight adapters are not swapped mid-request.
