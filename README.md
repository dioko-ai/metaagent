# Bob The Agent

<img width="300" alt="bob-the-agent" src="https://github.com/user-attachments/assets/1d2563a3-316d-40ae-92a6-d9bd232fb517" />


**A lightning-fast AI orchestrator that decomposes complex coding tasks into agent-driven workflows — built in Rust.**

## What is Bob?

Bob is a TUI and CLI tool that acts as a master planner for software engineering tasks. You describe what you want built, and Bob decomposes your request into a structured task graph, then dispatches a team of specialized AI agents to execute each step — with built-in quality gates and retry loops to ensure correctness.

The multi-agent workflow follows a rigorous pipeline: **Implementor** writes the code, an **Auditor** reviews the implementation, a **Test Writer** generates tests, a **Test Runner** executes them, and a **Final Audit** verifies the end result. When an audit or test fails, Bob automatically retries the failing stage with feedback from the previous attempt, creating a self-correcting development loop.

The interactive TUI provides a three-pane layout: sub-agent output streams in the top-left, a chat and input area occupies the bottom-left, and a task list with planner visualization sits on the right. Bob supports dual backends — **OpenAI Codex CLI** and **Anthropic Claude CLI** — and is written entirely in Rust (~13k lines), delivering a native binary with zero runtime overhead and instant startup.

## Key Benefits

- **Built in Rust** — lightning-fast native binary, no interpreter overhead
- **Multi-agent orchestration** — automated plan, implement, audit, test, and verify pipeline
- **Built-in quality gates** — code audits and test execution with retry loops (up to 4 audit passes, 5 test passes, 4 final-audit passes)
- **Dual backend support** — OpenAI Codex CLI and Anthropic Claude CLI
- **Persistent sessions** — resume work across sessions with full context
- **Configurable model routing** — different models and thinking effort per agent role
- **Interactive TUI** — three-pane layout with real-time output, chat, and task visualization
- **Scriptable CLI API** — JSON API for automation
- **Customizable themes** — TOML-based theme configuration

## How It Works

```
 You describe a task
        │
        ▼
 ┌───────────────┐
 │ Master Planner│  ← Decomposes into task graph
 └──────┬────────┘
        │
        ▼
 ┌──────────────┐     ┌───────────┐
 │ Implementor  │────▶│  Auditor  │──┐
 └──────────────┘     └───────────┘  │ Fails? Retry (up to 4 passes)
        ▲                            │
        └────────────────────────────┘
                     │ Passes
                     ▼
              ┌─────────────┐     ┌─────────────┐
              │ Test Writer │────▶│ Test Runner │──┐
              └─────────────┘     └─────────────┘  │ Fails? Retry (up to 5 passes)
                     ▲                             │
                     └─────────────────────────────┘
                                  │ Passes
                                  ▼
                          ┌──────────────┐
                          │ Final Audit  │──┐
                          └──────────────┘  │ Fails? Retry (up to 4 passes)
                                 ▲          │
                                 └──────────┘
                                  │ Passes
                                  ▼
                                Done ✓
```

Bob includes a **collaborative planner mode** where you can interactively refine the task plan before execution. Use `/convert` to transform the planner markdown into a structured task list, then `/start` to kick off the agent pipeline.

## Quick Start

### Prerequisites

- [Rust toolchain](https://rustup.rs/) (install via `rustup`)
- At least one AI backend CLI:
  - [OpenAI Codex CLI](https://github.com/openai/codex) — `codex`
  - [Anthropic Claude CLI](https://github.com/anthropics/claude-code) — `claude`

### Clone & Build

```bash
git clone https://github.com/anthropics/metaagent.git
cd metaagent
cargo build --release
```

The binary will be at `target/release/bob`.

### Configure

Bob stores its configuration in `~/.bob/config.toml` (with legacy fallback to `~/.metaagent/config.toml`). A default config is created on first run. Key sections:

**Model profiles** control which model and thinking effort are used:

```toml
[codex.model_profiles.small-dumb]
model = "gpt-5.1-codex-mini"
thinking_effort = "low"

[codex.model_profiles.large-smart]
model = "gpt-5.3-codex"
thinking_effort = "medium"

[codex.model_profiles.large-supergenius]
model = "gpt-5.3-codex"
thinking_effort = "xhigh"
```

**Agent profiles** map each worker role to a model profile:

```toml
[codex.agent_profiles]
master = "large-smart"
master_report = "large-smart"
project_info = "large-smart"
docs_attach = "large-smart"
task_check = "large-smart"
worker_implementor = "large-smart"
worker_auditor = "large-smart"
worker_test_writer = "large-smart"
worker_final_audit = "large-smart"
```

### Usage

1. **Launch** — Run `bob` in your project directory
2. **Describe your project** — The master planner creates a task graph
3. **Review the plan** — Inspect and refine tasks in the planner view
4. **Convert** — Type `/convert` to transform the plan into executable tasks
5. **Start** — Type `/start` to begin the agent pipeline
6. **Watch** — Agents execute sequentially: implement, audit, write tests, run tests, final audit

**Switch backends** at any time with the `/backend` command.

## Installation

Pre-built binaries are available on the [GitHub Releases](https://github.com/anthropics/metaagent/releases) page (details TBD). Alternatively, build from source — see [Compiling](#compiling) below.

## Compiling

### macOS

**Prerequisites:**

- Xcode Command Line Tools: `xcode-select --install`
- Rust toolchain: install via [rustup](https://rustup.rs/)

**Build:**

```bash
cargo build --release
```

The binary will be at `./target/release/bob`.

### Linux

**Prerequisites:**

- `build-essential` (Debian/Ubuntu) or equivalent C toolchain for your distro
- Rust toolchain: install via [rustup](https://rustup.rs/)

**Build:**

```bash
cargo build --release
```

All dependencies are pure Rust (`crossterm`, `ratatui`, `clap`, `serde`, `toml`, `serde_json`), so no extra system libraries are required.

### Reproducible builds

Use the `--locked` flag to ensure builds use the exact dependency versions from `Cargo.lock`:

```bash
cargo build --release --locked
```

## Configuration

Bob merges an embedded default configuration (`src/default_config.toml`) with the user config at `~/.bob/config.toml` (with legacy fallback to `~/.metaagent/config.toml`). Missing keys are filled from defaults, so you only need to override what you want to change. The merged config is written back on every launch.

### Backend selection

The `[backend]` table controls which AI backend is used:

```toml
[backend]
selected = "codex"   # or "claude"

[backend.codex]
program = "codex"
args_prefix = ["exec", "--dangerously-bypass-approvals-and-sandbox", "--color", "never"]

[backend.claude]
program = "claude"
args_prefix = ["--dangerously-skip-permissions"]
```

Switch backends at runtime with the `/backend` TUI command — the selection is persisted to your config file.

### Model profiles

Model profiles define a model and thinking effort level. The default config ships with these profiles (for the Codex backend):

| Profile | Model | Thinking Effort |
|---|---|---|
| `small-dumb` | `gpt-5.1-codex-mini` | `low` |
| `small-smart` | `gpt-5.1-codex-mini` | `medium` |
| `small-genius` | `gpt-5.1-codex-mini` | `high` |
| `small-supergenius` | `gpt-5.1-codex-mini` | `xhigh` |
| `large-dumb` | `gpt-5.3-codex` | `low` |
| `large-smart` | `gpt-5.3-codex` | `medium` |
| `large-genius` | `gpt-5.3-codex` | `high` |
| `large-supergenius` | `gpt-5.3-codex` | `xhigh` |

Override or add profiles in your config:

```toml
[codex.model_profiles.my-custom-profile]
model = "gpt-5.3-codex"
thinking_effort = "high"
```

### Agent routing

The `[codex.agent_profiles]` table maps each agent role to a model profile:

```toml
[codex.agent_profiles]
master = "large-smart"
master_report = "large-smart"
project_info = "large-smart"
docs_attach = "large-smart"
task_check = "large-smart"
worker_implementor = "large-smart"
worker_auditor = "large-smart"
worker_test_writer = "large-smart"
worker_final_audit = "large-smart"
```

Route heavier roles to more capable profiles and lighter roles to cheaper ones:

```toml
[codex.agent_profiles]
master = "large-supergenius"
worker_implementor = "large-genius"
worker_auditor = "small-smart"
task_check = "small-dumb"
```

### Theme

TUI colors are customizable via a `theme.toml` file. See `src/theme.rs` for the full list of themeable elements.

## Commands Reference

Bob's TUI provides 16 slash commands, organized by category:

### Planning

| Command | Description |
|---|---|
| `/planner` | Show collaborative planner markdown |
| `/convert` | Convert planner markdown to tasks |
| `/skip-plan` | Show task list view (skip planner) |

### Execution

| Command | Description |
|---|---|
| `/start` | Start execution of the task pipeline |
| `/backend` | Choose backend (Codex or Claude) |
| `/attach-docs` | Attach docs to tasks |

### Session Management

| Command | Description |
|---|---|
| `/newmaster` | Start a new master session |
| `/resume` | Resume a prior session |
| `/quit` | Quit app |
| `/exit` | Quit app |

### Workflow Customization

| Command | Description |
|---|---|
| `/split-audits` | Split audits per concern |
| `/merge-audits` | Merge audits |
| `/split-tests` | Split tests per concern |
| `/merge-tests` | Merge tests |
| `/add-final-audit` | Add final audit task |
| `/remove-final-audit` | Remove final audit task |

## CLI API

Bob exposes a JSON API via the CLI for scripting and automation.

### Output mode

Pass `--output json` to get machine-readable JSON output:

```bash
bob --output json api <resource> <action>
```

### Resources

The API is organized into four resource namespaces:

| Namespace | Description |
|---|---|
| `api capability` | List and inspect available API capabilities |
| `api app` | Prepare master, planner, and attach-docs prompts |
| `api workflow` | Validate tasks and render right-pane views |
| `api session` | Init, open, list, and read sessions |

### JSON envelope

All API responses follow a typed envelope structure:

```json
{
  "request_id": "optional-correlation-id",
  "capability": "capability.list",
  "result": {
    "status": "ok",
    "data": { }
  }
}
```

On error:

```json
{
  "result": {
    "status": "err",
    "error": {
      "code": "not_found",
      "message": "Session does not exist",
      "retryable": false
    }
  }
}
```

Error codes: `invalid_request`, `validation_failed`, `not_found`, `conflict`, `io_failure`, `external_failure`, `unsupported`, `internal`.

For full API details, see [`docs/architecture-cutover-guide.md`](docs/architecture-cutover-guide.md).

## License

TBD
