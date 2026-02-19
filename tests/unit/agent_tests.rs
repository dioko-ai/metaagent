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
    assert!(config.model.is_none());
    assert!(config.model_reasoning_effort.is_none());
}

#[test]
fn default_claude_config_uses_claude_backend_defaults() {
    let config = CodexCommandConfig::default_for_backend(BackendKind::Claude);
    assert_eq!(config.program, "claude");
    assert_eq!(
        config.args_prefix,
        vec!["--dangerously-skip-permissions".to_string()]
    );
    assert_eq!(config.output_mode, AdapterOutputMode::PlainText);
    assert!(!config.persistent_session);
    assert!(config.model.is_none());
    assert!(config.model_reasoning_effort.is_none());
}

#[test]
fn backend_kind_detects_claude_from_program_name() {
    let mut config = CodexCommandConfig::default();
    config.program = "/usr/local/bin/claude-code".to_string();
    assert_eq!(config.backend_kind(), BackendKind::Claude);

    config.program = "/usr/local/bin/codex".to_string();
    assert_eq!(config.backend_kind(), BackendKind::Codex);
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
fn saved_session_id_accessors_round_trip() {
    let adapter = CodexAdapter::new_persistent();
    assert!(adapter.saved_session_id().is_none());
    adapter.set_saved_session_id(Some("session-abc".to_string()));
    assert_eq!(adapter.saved_session_id().as_deref(), Some("session-abc"));
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
fn new_session_args_include_model_and_reasoning_effort() {
    let mut config = CodexCommandConfig::default();
    config.model = Some("gpt-5.3-codex".to_string());
    config.model_reasoning_effort = Some("high".to_string());
    let args = build_new_session_args(&config);
    assert!(
        args.windows(2)
            .any(|pair| pair[0] == "--model" && pair[1] == "gpt-5.3-codex")
    );
    assert!(
        args.windows(2)
            .any(|pair| pair[0] == "-c" && pair[1] == "model_reasoning_effort=\"high\"")
    );
}

#[test]
fn resume_args_keep_model_and_reasoning_effort() {
    let mut config = CodexCommandConfig::default();
    config.model = Some("gpt-5.3-codex".to_string());
    config.model_reasoning_effort = Some("xhigh".to_string());
    config.args_prefix.push("--json".to_string());
    let args = build_resume_args(&config);
    assert!(!args.iter().any(|arg| arg == "exec"));
    assert!(!args.iter().any(|arg| arg == "--color"));
    assert!(
        args.windows(2)
            .any(|pair| pair[0] == "--model" && pair[1] == "gpt-5.3-codex")
    );
    assert!(
        args.windows(2)
            .any(|pair| pair[0] == "-c" && pair[1] == "model_reasoning_effort=\"xhigh\"")
    );
}

#[test]
fn codex_resume_prompt_args_keep_exec_resume_parity() {
    let config = CodexCommandConfig::default();
    let args = build_resume_prompt_args(&config, "session-123");
    assert_eq!(
        args,
        vec![
            "exec".to_string(),
            "resume".to_string(),
            "--dangerously-bypass-approvals-and-sandbox".to_string(),
            "session-123".to_string()
        ]
    );
}

#[test]
fn claude_new_session_args_include_prompt_and_json_flags() {
    let mut config = CodexCommandConfig::default_for_backend(BackendKind::Claude);
    config.output_mode = AdapterOutputMode::JsonAssistantOnly;
    config.model = Some("claude-sonnet-4.5".to_string());
    let args = build_new_session_args(&config);
    assert_eq!(
        args,
        vec![
            "--dangerously-skip-permissions".to_string(),
            "--output-format".to_string(),
            "stream-json".to_string(),
            "--model".to_string(),
            "claude-sonnet-4.5".to_string(),
            "-p".to_string(),
        ]
    );
}

#[test]
fn claude_resume_prompt_args_use_resume_flag() {
    let mut config = CodexCommandConfig::default_for_backend(BackendKind::Claude);
    config.output_mode = AdapterOutputMode::JsonAssistantOnly;
    config.model = Some("claude-sonnet-4.5".to_string());
    let args = build_resume_prompt_args(&config, "thread-abc");
    assert_eq!(
        args,
        vec![
            "--dangerously-skip-permissions".to_string(),
            "--output-format".to_string(),
            "stream-json".to_string(),
            "--model".to_string(),
            "claude-sonnet-4.5".to_string(),
            "--resume".to_string(),
            "thread-abc".to_string(),
            "-p".to_string(),
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
        model: None,
        model_reasoning_effort: None,
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
        if outputs.iter().any(|l| l == "out:hello") && outputs.iter().any(|l| l == "err:hello") {
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
        model: None,
        model_reasoning_effort: None,
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
        model: None,
        model_reasoning_effort: None,
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
        model: None,
        model_reasoning_effort: None,
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
fn adapter_spawn_error_still_emits_completed_event() {
    let adapter = CodexAdapter::with_config(CodexCommandConfig {
        program: "__no_such_program__".to_string(),
        args_prefix: Vec::new(),
        output_mode: AdapterOutputMode::PlainText,
        persistent_session: false,
        model: None,
        model_reasoning_effort: None,
    });
    adapter.send_prompt("hello".to_string());

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut saw_completed = false;
    while Instant::now() < deadline {
        for event in adapter.drain_events() {
            if let AgentEvent::Completed { success, code } = event {
                assert!(!success);
                assert_eq!(code, -1);
                saw_completed = true;
            }
        }
        if saw_completed {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }

    assert!(
        saw_completed,
        "expected completed event even when process spawn fails"
    );
}

#[test]
fn claude_backend_spawn_error_still_emits_completed_event() {
    let adapter = CodexAdapter::with_config(CodexCommandConfig {
        program: "__no_such_claude_binary__".to_string(),
        args_prefix: Vec::new(),
        output_mode: AdapterOutputMode::PlainText,
        persistent_session: false,
        model: Some("claude-sonnet-4.5".to_string()),
        model_reasoning_effort: Some("high".to_string()),
    });
    adapter.send_prompt("hello".to_string());

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut saw_completed = false;
    while Instant::now() < deadline {
        for event in adapter.drain_events() {
            if let AgentEvent::Completed { success, code } = event {
                assert!(!success);
                assert_eq!(code, -1);
                saw_completed = true;
            }
        }
        if saw_completed {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }

    assert!(
        saw_completed,
        "expected completed event even when Claude process spawn fails"
    );
}

#[test]
fn persistent_plain_text_completes_without_waiting_for_background_descendants() {
    let adapter = CodexAdapter::with_config(CodexCommandConfig {
        program: "bash".to_string(),
        args_prefix: vec![
            "-lc".to_string(),
            "printf 'early\\n'; (sleep 2) &".to_string(),
        ],
        output_mode: AdapterOutputMode::PlainText,
        persistent_session: true,
        model: None,
        model_reasoning_effort: None,
    });
    let started = Instant::now();
    adapter.send_prompt("ignored".to_string());

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut completed_elapsed: Option<Duration> = None;
    while Instant::now() < deadline {
        for event in adapter.drain_events() {
            if let AgentEvent::Completed { .. } = event {
                completed_elapsed = Some(started.elapsed());
                break;
            }
        }
        if completed_elapsed.is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }

    let elapsed = completed_elapsed.expect("expected completed event");
    assert!(
        elapsed < Duration::from_millis(1200),
        "completion was delayed by descendant-held pipes: elapsed={elapsed:?}"
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
