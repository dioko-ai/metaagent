# CLI/TUI Parity Checklist

This checklist maps current TUI-visible capabilities to non-interactive CLI commands for parity validation and automation.

References:
- `docs/architecture-cutover-guide.md`
- `src/main.rs`
- `src/app.rs`
- `src/api/capabilities.rs`
- GitHub Well-Architected architecture checklist: https://wellarchitected.github.com/library/architecture/checklist/

## Parity Matrix

| TUI capability | CLI command path | Parity status | Notes |
|---|---|---|---|
| Discover capability surface | `api capability list` | Full | Returns capability IDs, domains, operation types, contracts, and code paths. |
| Inspect one capability | `api capability get --id <capability_id>` | Full | Stable machine-readable lookup for introspection. |
| Build master prompt payload | `api app prepare-master-prompt --message ... --tasks-file ...` | Full | CLI covers prompt preparation, not TUI-side async master dispatch lifecycle. |
| Build planner prompt payload | `api app prepare-planner-prompt --message ... --planner-file ... --project-info-file ...` | Full | Transport-agnostic prompt generation parity. |
| Build attach-docs prompt payload | `api app prepare-attach-docs-prompt --tasks-file ...` | Full | Parity for prompt text generation. |
| Validate/normalize task graph (`tasks.json`) | `api workflow validate-tasks --tasks-file <path>` | Full | Mirrors workflow task sync/validation semantics used by UI state sync. |
| Render right-pane task block projection | `api workflow right-pane-view --tasks-file <path> --width <n>` | Full | Returns lines/toggles for automation snapshots. |
| Initialize session storage | `api session init [--cwd <path>]` | Full | Returns initialized session directory in JSON mode. |
| Open existing session | `api session open --session-dir <path> [--cwd <path>]` | Full | Matches resume/open storage behavior. |
| List resumable sessions | `api session list` | Full | Equivalent data source for TUI resume picker population. |
| Read session tasks | `api session read-tasks --session-dir <path> [--cwd <path>]` | Full | Non-interactive access to persisted planner tasks. |
| Read planner markdown | `api session read-planner --session-dir <path> [--cwd <path>]` | Full | Non-interactive access to persisted planner markdown. |
| Read/write rolling context artifact | `api session read-rolling-context --session-dir <path> [--cwd <path>]`, `api session write-rolling-context --session-dir <path> --entries-file <json> [--cwd <path>]` | Full | CLI parity for persisted rolling task context used by status reporting. |
| Read/append task failure ledger | `api session read-task-fails --session-dir <path> [--cwd <path>]`, `api session append-task-fails --session-dir <path> --entries-file <json> [--cwd <path>]` | Full | CLI parity for durable workflow failure records. |
| Read/write project info context | `api session read-project-info --session-dir <path> [--cwd <path>]`, `api session write-project-info --session-dir <path> --markdown-file <path> [--cwd <path>]` | Full | CLI parity for project context consumed by subagent prompts. |
| Read session metadata | `api session read-session-meta --session-dir <path> [--cwd <path>]` | Full | CLI access to session title/created/test-command metadata. |
| Choose backend (`/backend`) | _No direct CLI command yet_ | Gap | TUI picker updates `~/.metaagent/config.toml` (`[backend].selected`); selection affects newly created adapters only. |
| Start execution (`/start`, `/run`) | _No CLI command yet_ | Gap | TUI-only orchestration trigger in this transport pass. |
| Live terminal event loop (chat input, pane nav, scrolling) | _No CLI command_ | Intentional gap | Interactive TUI behavior is not exposed as one-shot CLI commands. |
| Slash task-edit controls (`/split-audits`, `/merge-audits`, `/split-tests`, `/merge-tests`, `/add-final-audit`, `/remove-final-audit`) | _No direct CLI command yet_ | Gap | Only accessible through interactive message command flow currently. |

## Scriptability Expectations

- Prefer `--output json` for automation. JSON is emitted on `stdout`.
- Success envelope shape:
  - `status = "ok"`
  - `summary` is a concise human-readable result
  - `data` carries structured payload
- Error envelope shape (when a command runs and returns domain error in JSON mode):
  - `status = "err"`
  - `error.code` uses stable values (`invalid_request`, `validation_failed`, `not_found`, `io_failure`, `unsupported`, `internal`, etc.)
  - `error.message` is a stable diagnostic string
  - `error.retryable` is currently `false`
  - `error.details` may be omitted
- Exit codes are mapped from `ApiErrorCode`:
  - `10 invalid_request`
  - `11 validation_failed`
  - `12 not_found`
  - `13 conflict`
  - `14 io_failure`
  - `15 external_failure`
  - `16 unsupported`
  - `17 internal`
- Argument/usage parsing errors are emitted to `stderr` before command dispatch.

## Automation Asset

Use `scripts/cli_scriptability_examples.sh` as a non-interactive smoke script for:
- success JSON envelope checks
- JSON error envelope + exit-code checks
- stderr behavior checks for parse-time argument failures
