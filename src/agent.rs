use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentEvent {
    Output(String),
    System(String),
    Completed { success: bool, code: i32 },
}

#[derive(Debug, Clone)]
pub struct CodexCommandConfig {
    pub program: String,
    pub args_prefix: Vec<String>,
    pub output_mode: AdapterOutputMode,
    pub persistent_session: bool,
    pub model: Option<String>,
    pub model_reasoning_effort: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    Codex,
    Claude,
}

impl BackendKind {
    fn from_program(program: &str) -> Self {
        let binary = Path::new(program)
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or(program)
            .to_ascii_lowercase();
        if binary.contains("claude") {
            Self::Claude
        } else {
            Self::Codex
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterOutputMode {
    PlainText,
    JsonAssistantOnly,
}

impl CodexCommandConfig {
    pub fn default_for_backend(backend: BackendKind) -> Self {
        match backend {
            BackendKind::Codex => Self {
                program: "codex".to_string(),
                // This version intentionally runs Codex fully unsandboxed and with approvals disabled.
                // Future versions should replace this with a safer, explicit permissions model.
                args_prefix: vec![
                    "exec".to_string(),
                    "--dangerously-bypass-approvals-and-sandbox".to_string(),
                    "--color".to_string(),
                    "never".to_string(),
                ],
                output_mode: AdapterOutputMode::PlainText,
                persistent_session: false,
                model: None,
                model_reasoning_effort: None,
            },
            BackendKind::Claude => Self {
                program: "claude".to_string(),
                args_prefix: vec!["--dangerously-skip-permissions".to_string()],
                output_mode: AdapterOutputMode::PlainText,
                persistent_session: false,
                model: None,
                model_reasoning_effort: None,
            },
        }
    }

    pub fn backend_kind(&self) -> BackendKind {
        BackendKind::from_program(&self.program)
    }
}

impl Default for CodexCommandConfig {
    fn default() -> Self {
        Self::default_for_backend(BackendKind::Codex)
    }
}

pub struct CodexAdapter {
    config: CodexCommandConfig,
    event_tx: Sender<AgentEvent>,
    event_rx: Receiver<AgentEvent>,
    session_id: Arc<Mutex<Option<String>>>,
}

impl CodexAdapter {
    #[cfg(test)]
    pub fn new() -> Self {
        Self::with_config(CodexCommandConfig::default())
    }

    #[cfg(test)]
    pub fn new_persistent() -> Self {
        let mut config = CodexCommandConfig::default();
        config.persistent_session = true;
        Self::with_config(config)
    }

    #[cfg(test)]
    pub fn new_json_assistant_persistent() -> Self {
        let mut config = CodexCommandConfig::default();
        config.args_prefix.push("--json".to_string());
        config.output_mode = AdapterOutputMode::JsonAssistantOnly;
        config.persistent_session = true;
        Self::with_config(config)
    }

    #[cfg(test)]
    pub fn new_master() -> Self {
        let mut config = CodexCommandConfig::default();
        config.args_prefix.push("--json".to_string());
        config.output_mode = AdapterOutputMode::JsonAssistantOnly;
        config.persistent_session = true;
        Self::with_config(config)
    }

    pub fn with_config(config: CodexCommandConfig) -> Self {
        let (event_tx, event_rx) = mpsc::channel();
        Self {
            config,
            event_tx,
            event_rx,
            session_id: Arc::new(Mutex::new(None)),
        }
    }

    pub fn send_prompt(&self, prompt: String) {
        let config = self.config.clone();
        let tx = self.event_tx.clone();
        let session_id = self.session_id.clone();
        thread::spawn(move || {
            let mut command = Command::new(&config.program);
            if config.persistent_session {
                let known_session = session_id.lock().ok().and_then(|g| g.clone());
                if let Some(existing_session) = known_session {
                    command
                        .args(build_resume_prompt_args(&config, &existing_session))
                        .arg(prompt);
                } else {
                    command.args(build_new_session_args(&config)).arg(prompt);
                }
            } else {
                command.args(build_new_session_args(&config)).arg(prompt);
            }
            command.stdout(Stdio::piped()).stderr(Stdio::piped());

            let mut child = match command.spawn() {
                Ok(child) => child,
                Err(err) => {
                    let _ = tx.send(AgentEvent::System(format!(
                        "Codex adapter failed to start: {err}"
                    )));
                    let _ = tx.send(AgentEvent::Completed {
                        success: false,
                        code: -1,
                    });
                    return;
                }
            };

            let mut readers = Vec::new();

            if let Some(stdout) = child.stdout.take() {
                readers.push(spawn_reader(
                    stdout,
                    tx.clone(),
                    config.output_mode,
                    Some(session_id.clone()),
                ));
            }
            if let Some(stderr) = child.stderr.take() {
                readers.push(spawn_reader(
                    stderr,
                    tx.clone(),
                    config.output_mode,
                    Some(session_id.clone()),
                ));
            }

            let wait_result = child.wait();
            let skip_reader_join_after_wait = config.persistent_session
                && matches!(config.output_mode, AdapterOutputMode::PlainText);
            if skip_reader_join_after_wait {
                // Worker-style adapters can run shell commands that spawn background descendants.
                // Those descendants may inherit stdout/stderr and keep pipes open, causing reader
                // joins to block forever after the main process exits. Emit completion immediately
                // after wait so scheduling can continue.
                emit_completion_event(&tx, wait_result);
                return;
            }
            for reader in readers {
                let _ = reader.join();
            }
            emit_completion_event(&tx, wait_result);
        });
    }

    #[cfg(test)]
    pub fn drain_events(&self) -> Vec<AgentEvent> {
        self.drain_events_limited(usize::MAX)
    }

    pub fn drain_events_limited(&self, max_events: usize) -> Vec<AgentEvent> {
        let mut events = Vec::new();
        if max_events == 0 {
            return events;
        }
        while events.len() < max_events {
            let Ok(event) = self.event_rx.try_recv() else {
                break;
            };
            events.push(event);
        }
        events
    }

    pub fn reset_session(&self) {
        if let Ok(mut lock) = self.session_id.lock() {
            *lock = None;
        }
    }

    pub fn saved_session_id(&self) -> Option<String> {
        self.session_id.lock().ok().and_then(|lock| lock.clone())
    }

    pub fn set_saved_session_id(&self, session_id: Option<String>) {
        if let Ok(mut lock) = self.session_id.lock() {
            *lock = session_id;
        }
    }
}

fn spawn_reader<R: std::io::Read + Send + 'static>(
    reader: R,
    tx: Sender<AgentEvent>,
    output_mode: AdapterOutputMode,
    session_id: Option<Arc<Mutex<Option<String>>>>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        for line in BufReader::new(reader).lines().map_while(Result::ok) {
            if let Some(state) = &session_id
                && let Some(found) = parse_session_id_from_jsonl_line(&line)
                && let Ok(mut lock) = state.lock()
                && lock.is_none()
            {
                *lock = Some(found);
            }
            match output_mode {
                AdapterOutputMode::PlainText => {
                    let _ = tx.send(AgentEvent::Output(line));
                }
                AdapterOutputMode::JsonAssistantOnly => {
                    if let Some(text) = parse_agent_message_from_jsonl_line(&line) {
                        let _ = tx.send(AgentEvent::Output(text));
                    }
                }
            }
        }
    })
}

fn parse_agent_message_from_jsonl_line(line: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    if value.get("type")?.as_str()? != "item.completed" {
        return None;
    }
    let item = value.get("item")?;
    if item.get("type")?.as_str()? != "agent_message" {
        return None;
    }
    item.get("text")?.as_str().map(ToString::to_string)
}

fn sanitize_resume_args(args: Vec<String>) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len());
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        if arg == "--color" {
            let _ = iter.next();
            continue;
        }
        out.push(arg);
    }
    out
}

fn build_new_session_args(config: &CodexCommandConfig) -> Vec<String> {
    match config.backend_kind() {
        BackendKind::Codex => {
            let mut args = config.args_prefix.clone();
            append_codex_model_selection_args(
                &mut args,
                config.model.as_deref(),
                config.model_reasoning_effort.as_deref(),
            );
            args
        }
        BackendKind::Claude => {
            let mut args = config.args_prefix.clone();
            if matches!(config.output_mode, AdapterOutputMode::JsonAssistantOnly) {
                args.push("--output-format".to_string());
                args.push("stream-json".to_string());
            }
            append_claude_model_selection_args(&mut args, config.model.as_deref());
            args.push("-p".to_string());
            args
        }
    }
}

fn build_resume_args(config: &CodexCommandConfig) -> Vec<String> {
    match config.backend_kind() {
        BackendKind::Codex => {
            let mut args = config.args_prefix.clone();
            if args.first().is_some_and(|arg| arg == "exec") {
                args.remove(0);
            }
            let mut args = sanitize_resume_args(args);
            append_codex_model_selection_args(
                &mut args,
                config.model.as_deref(),
                config.model_reasoning_effort.as_deref(),
            );
            args
        }
        BackendKind::Claude => {
            let mut args = config.args_prefix.clone();
            if matches!(config.output_mode, AdapterOutputMode::JsonAssistantOnly) {
                args.push("--output-format".to_string());
                args.push("stream-json".to_string());
            }
            append_claude_model_selection_args(&mut args, config.model.as_deref());
            args
        }
    }
}

fn build_resume_prompt_args(config: &CodexCommandConfig, existing_session: &str) -> Vec<String> {
    match config.backend_kind() {
        BackendKind::Codex => {
            let mut args = vec!["exec".to_string(), "resume".to_string()];
            args.extend(build_resume_args(config));
            args.push(existing_session.to_string());
            args
        }
        BackendKind::Claude => {
            let mut args = build_resume_args(config);
            args.push("--resume".to_string());
            args.push(existing_session.to_string());
            args.push("-p".to_string());
            args
        }
    }
}

fn append_codex_model_selection_args(
    args: &mut Vec<String>,
    model: Option<&str>,
    model_reasoning_effort: Option<&str>,
) {
    if let Some(model) = model.map(str::trim).filter(|value| !value.is_empty()) {
        args.push("--model".to_string());
        args.push(model.to_string());
    }
    if let Some(effort) = model_reasoning_effort
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        args.push("-c".to_string());
        args.push(format!("model_reasoning_effort={effort:?}"));
    }
}

fn append_claude_model_selection_args(args: &mut Vec<String>, model: Option<&str>) {
    if let Some(model) = model.map(str::trim).filter(|value| !value.is_empty()) {
        args.push("--model".to_string());
        args.push(model.to_string());
    }
}

fn emit_completion_event(
    tx: &Sender<AgentEvent>,
    wait_result: std::io::Result<std::process::ExitStatus>,
) {
    match wait_result {
        Ok(status) => {
            let code = status.code().unwrap_or(-1);
            let _ = tx.send(AgentEvent::Completed {
                success: status.success(),
                code,
            });
            if !status.success() {
                let _ = tx.send(AgentEvent::System(format!(
                    "Codex adapter exited with status code {code}"
                )));
            }
        }
        Err(err) => {
            let _ = tx.send(AgentEvent::System(format!(
                "Codex adapter failed while waiting for process: {err}"
            )));
            let _ = tx.send(AgentEvent::Completed {
                success: false,
                code: -1,
            });
        }
    }
}

fn parse_session_id_from_jsonl_line(line: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let direct = value
        .get("session_id")
        .and_then(|v| v.as_str())
        .or_else(|| value.get("conversation_id").and_then(|v| v.as_str()))
        .or_else(|| value.get("thread_id").and_then(|v| v.as_str()));
    if let Some(id) = direct
        && looks_like_session_id(id)
    {
        return Some(id.to_string());
    }
    let session_obj = value.get("session")?;
    let nested = session_obj
        .get("id")
        .and_then(|v| v.as_str())
        .or_else(|| session_obj.get("session_id").and_then(|v| v.as_str()));
    nested
        .filter(|id| looks_like_session_id(id))
        .map(ToString::to_string)
}

fn looks_like_session_id(id: &str) -> bool {
    let trimmed = id.trim();
    if trimmed.len() < 8 || trimmed.contains(char::is_whitespace) {
        return false;
    }
    trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}

#[cfg(test)]
#[path = "../tests/unit/agent_tests.rs"]
mod tests;
