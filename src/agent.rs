use std::io::{BufRead, BufReader};
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterOutputMode {
    PlainText,
    JsonAssistantOnly,
}

impl Default for CodexCommandConfig {
    fn default() -> Self {
        Self {
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
        }
    }
}

pub struct CodexAdapter {
    config: CodexCommandConfig,
    event_tx: Sender<AgentEvent>,
    event_rx: Receiver<AgentEvent>,
    session_id: Arc<Mutex<Option<String>>>,
}

impl CodexAdapter {
    pub fn new() -> Self {
        Self::with_config(CodexCommandConfig::default())
    }

    pub fn new_persistent() -> Self {
        let mut config = CodexCommandConfig::default();
        config.persistent_session = true;
        Self::with_config(config)
    }

    pub fn new_json_assistant_persistent() -> Self {
        let mut config = CodexCommandConfig::default();
        config.args_prefix.push("--json".to_string());
        config.output_mode = AdapterOutputMode::JsonAssistantOnly;
        config.persistent_session = true;
        Self::with_config(config)
    }

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
                    let mut resume_args = config.args_prefix.clone();
                    if resume_args.first().is_some_and(|arg| arg == "exec") {
                        resume_args.remove(0);
                    }
                    resume_args = sanitize_resume_args(resume_args);
                    command
                        .arg("exec")
                        .arg("resume")
                        .args(resume_args)
                        .arg(existing_session)
                        .arg(prompt);
                } else {
                    command.args(&config.args_prefix).arg(prompt);
                }
            } else {
                command.args(&config.args_prefix).arg(prompt);
            }
            command.stdout(Stdio::piped()).stderr(Stdio::piped());

            let mut child = match command.spawn() {
                Ok(child) => child,
                Err(err) => {
                    let _ = tx.send(AgentEvent::System(format!(
                        "Codex adapter failed to start: {err}"
                    )));
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
            for reader in readers {
                let _ = reader.join();
            }
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
                }
            }
        });
    }

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
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn default_codex_config_is_unsandboxed_for_this_version() {
        let config = CodexCommandConfig::default();
        assert_eq!(config.program, "codex");
        assert_eq!(
            config.args_prefix,
            vec![
                "exec",
                "--dangerously-bypass-approvals-and-sandbox",
                "--color",
                "never"
            ]
        );
        assert_eq!(config.output_mode, AdapterOutputMode::PlainText);
        assert!(!config.persistent_session);
    }

    #[test]
    fn master_config_enables_persistent_session() {
        let adapter = CodexAdapter::new_master();
        assert!(adapter.config.persistent_session);
    }

    #[test]
    fn json_assistant_persistent_config_enables_persistent_session() {
        let adapter = CodexAdapter::new_json_assistant_persistent();
        assert!(adapter.config.persistent_session);
    }

    #[test]
    fn plain_text_persistent_config_enables_persistent_session() {
        let adapter = CodexAdapter::new_persistent();
        assert!(adapter.config.persistent_session);
        assert_eq!(adapter.config.output_mode, AdapterOutputMode::PlainText);
    }

    #[test]
    fn parses_session_id_from_jsonl_event() {
        let line =
            r#"{"type":"session.started","session":{"id":"123e4567-e89b-12d3-a456-426614174000"}}"#;
        let parsed = parse_session_id_from_jsonl_line(line);
        assert_eq!(
            parsed.as_deref(),
            Some("123e4567-e89b-12d3-a456-426614174000")
        );
    }

    #[test]
    fn reset_session_clears_saved_session_id() {
        let adapter = CodexAdapter::new_master();
        {
            let mut lock = adapter.session_id.lock().expect("lock should succeed");
            *lock = Some("session-1".to_string());
        }
        adapter.reset_session();
        let lock = adapter.session_id.lock().expect("lock should succeed");
        assert!(lock.is_none());
    }

    #[test]
    fn resume_args_strip_color_flag() {
        let args = vec![
            "--dangerously-bypass-approvals-and-sandbox".to_string(),
            "--color".to_string(),
            "never".to_string(),
            "--json".to_string(),
        ];
        let sanitized = sanitize_resume_args(args);
        assert_eq!(
            sanitized,
            vec![
                "--dangerously-bypass-approvals-and-sandbox".to_string(),
                "--json".to_string(),
            ]
        );
    }

    #[test]
    fn adapter_streams_stdout_and_stderr() {
        let adapter = CodexAdapter::with_config(CodexCommandConfig {
            program: "bash".to_string(),
            args_prefix: vec![
                "-lc".to_string(),
                "printf 'out:%s\\n' \"$1\"; printf 'err:%s\\n' \"$1\" 1>&2".to_string(),
                "codex-test".to_string(),
            ],
            output_mode: AdapterOutputMode::PlainText,
            persistent_session: false,
        });
        adapter.send_prompt("hello".to_string());

        let deadline = Instant::now() + Duration::from_secs(2);
        let mut outputs = Vec::new();
        while Instant::now() < deadline {
            for event in adapter.drain_events() {
                match event {
                    AgentEvent::Output(line) => outputs.push(line),
                    AgentEvent::Completed { .. } => {}
                    AgentEvent::System(_) => {}
                }
            }
            if outputs.iter().any(|l| l == "out:hello") && outputs.iter().any(|l| l == "err:hello")
            {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }

        assert!(outputs.iter().any(|l| l == "out:hello"));
        assert!(outputs.iter().any(|l| l == "err:hello"));
    }

    #[test]
    fn adapter_emits_completed_event() {
        let adapter = CodexAdapter::with_config(CodexCommandConfig {
            program: "bash".to_string(),
            args_prefix: vec!["-lc".to_string(), "printf 'done\\n'".to_string()],
            output_mode: AdapterOutputMode::PlainText,
            persistent_session: false,
        });
        adapter.send_prompt("ignored".to_string());

        let deadline = Instant::now() + Duration::from_secs(2);
        let mut completed = false;
        while Instant::now() < deadline {
            for event in adapter.drain_events() {
                if let AgentEvent::Completed { success, code } = event {
                    assert!(success);
                    assert_eq!(code, 0);
                    completed = true;
                }
            }
            if completed {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }

        assert!(completed, "expected completed event");
    }

    #[test]
    fn adapter_emits_completed_after_output_is_drained() {
        let adapter = CodexAdapter::with_config(CodexCommandConfig {
            program: "bash".to_string(),
            args_prefix: vec![
                "-lc".to_string(),
                "(sleep 0.05; printf 'late\\n') & printf 'early\\n'".to_string(),
            ],
            output_mode: AdapterOutputMode::PlainText,
            persistent_session: false,
        });
        adapter.send_prompt("ignored".to_string());

        let deadline = Instant::now() + Duration::from_secs(2);
        let mut saw_completed = false;
        let mut saw_late = false;
        let mut output_after_completed = false;
        while Instant::now() < deadline {
            for event in adapter.drain_events() {
                match event {
                    AgentEvent::Output(line) => {
                        if saw_completed {
                            output_after_completed = true;
                        }
                        if line.trim() == "late" {
                            saw_late = true;
                        }
                    }
                    AgentEvent::Completed { .. } => {
                        saw_completed = true;
                    }
                    AgentEvent::System(_) => {}
                }
            }
            if saw_completed && saw_late {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }

        let extra_poll_deadline = Instant::now() + Duration::from_millis(150);
        while Instant::now() < extra_poll_deadline {
            for event in adapter.drain_events() {
                if let AgentEvent::Output(_) = event
                    && saw_completed
                {
                    output_after_completed = true;
                }
            }
            thread::sleep(Duration::from_millis(10));
        }

        assert!(saw_completed, "expected completed event");
        assert!(saw_late, "expected delayed output line");
        assert!(
            !output_after_completed,
            "saw output after completed event, which can corrupt next-job context"
        );
    }

    #[test]
    fn adapter_reports_spawn_errors() {
        let adapter = CodexAdapter::with_config(CodexCommandConfig {
            program: "__no_such_program__".to_string(),
            args_prefix: Vec::new(),
            output_mode: AdapterOutputMode::PlainText,
            persistent_session: false,
        });
        adapter.send_prompt("hello".to_string());

        let deadline = Instant::now() + Duration::from_secs(2);
        let mut system_messages = Vec::new();
        while Instant::now() < deadline {
            for event in adapter.drain_events() {
                if let AgentEvent::System(line) = event {
                    system_messages.push(line);
                }
            }
            if !system_messages.is_empty() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }

        assert!(
            system_messages
                .iter()
                .any(|msg| msg.contains("failed to start")),
            "expected startup error, got: {system_messages:?}"
        );
    }

    #[test]
    fn parses_only_json_agent_message_lines() {
        let line = r#"{"type":"item.completed","item":{"type":"agent_message","text":"hello"}}"#;
        assert_eq!(
            parse_agent_message_from_jsonl_line(line),
            Some("hello".to_string())
        );
        let non_agent = r#"{"type":"item.completed","item":{"type":"reasoning","text":"x"}}"#;
        assert_eq!(parse_agent_message_from_jsonl_line(non_agent), None);
        assert_eq!(parse_agent_message_from_jsonl_line("not-json"), None);
    }

    #[test]
    fn drain_events_limited_respects_max_and_preserves_queue() {
        let adapter = CodexAdapter::new();
        for idx in 0..5 {
            adapter
                .event_tx
                .send(AgentEvent::Output(format!("line-{idx}")))
                .expect("send should succeed");
        }

        let first = adapter.drain_events_limited(2);
        assert_eq!(first.len(), 2);
        assert!(matches!(first[0], AgentEvent::Output(_)));
        assert!(matches!(first[1], AgentEvent::Output(_)));

        let second = adapter.drain_events_limited(10);
        assert_eq!(second.len(), 3);
        assert!(second.iter().all(|e| matches!(e, AgentEvent::Output(_))));
    }
}
