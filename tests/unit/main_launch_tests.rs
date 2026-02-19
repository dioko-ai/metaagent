use super::*;

fn open_temp_store(prefix: &str) -> (SessionStore, std::path::PathBuf) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let cwd = std::env::current_dir().expect("cwd");
    let session_dir = std::env::temp_dir().join(format!("{prefix}-{now}"));
    let store = SessionStore::open_existing(&cwd, &session_dir).expect("open existing store");
    (store, session_dir)
}

fn with_temp_home<T>(prefix: &str, f: impl FnOnce(&std::path::Path) -> T) -> T {
    let _guard = crate::artifact_io::home_env_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let temp_home = std::env::temp_dir().join(format!("{prefix}-{now}"));
    std::fs::create_dir_all(&temp_home).expect("create temp home");

    let prior_home = std::env::var_os("HOME");
    // SAFETY: tests serialize HOME mutation with HOME_LOCK and restore afterward.
    unsafe {
        std::env::set_var("HOME", &temp_home);
    }

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&temp_home)));

    // SAFETY: restoration mirrors the guarded mutation above.
    unsafe {
        if let Some(value) = prior_home {
            std::env::set_var("HOME", value);
        } else {
            std::env::remove_var("HOME");
        }
    }
    std::fs::remove_dir_all(&temp_home).ok();

    match result {
        Ok(value) => value,
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

fn integration_plan_with_final() -> Vec<PlannerTaskFileEntry> {
    vec![
        PlannerTaskFileEntry {
            id: "top".to_string(),
            title: "Task".to_string(),
            details: "top details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Task,
            status: PlannerTaskStatusFile::Pending,
            parent_id: None,
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl".to_string(),
            title: "Implementation".to_string(),
            details: "impl details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Implementor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("top".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl-audit".to_string(),
            title: "Implementation audit".to_string(),
            details: "audit details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Auditor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("impl".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "tw".to_string(),
            title: "Test writing".to_string(),
            details: "tw details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::TestWriter,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("top".to_string()),
            order: Some(1),
        },
        PlannerTaskFileEntry {
            id: "tw-audit".to_string(),
            title: "Test audit".to_string(),
            details: "tw audit details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Auditor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("tw".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "tw-runner".to_string(),
            title: "Run tests".to_string(),
            details: "runner details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::TestRunner,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("tw".to_string()),
            order: Some(1),
        },
        PlannerTaskFileEntry {
            id: "final".to_string(),
            title: "Final audit".to_string(),
            details: "final audit details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::FinalAudit,
            status: PlannerTaskStatusFile::Pending,
            parent_id: None,
            order: Some(1),
        },
    ]
}

fn simple_execution_plan() -> Vec<PlannerTaskFileEntry> {
    vec![
        PlannerTaskFileEntry {
            id: "top".to_string(),
            title: "Task".to_string(),
            details: "top details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Task,
            status: PlannerTaskStatusFile::Pending,
            parent_id: None,
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl".to_string(),
            title: "Implementation".to_string(),
            details: "impl details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Implementor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("top".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl-audit".to_string(),
            title: "Implementation audit".to_string(),
            details: "audit details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Auditor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("impl".to_string()),
            order: Some(0),
        },
    ]
}

#[test]
fn parse_launch_options_accepts_send_file() {
    let options = parse_launch_options(vec![
        "--send-file".to_string(),
        "/tmp/prompt.txt".to_string(),
    ])
    .expect("options should parse");
    assert_eq!(
        options.send_file.as_deref(),
        Some(std::path::Path::new("/tmp/prompt.txt"))
    );
}

#[test]
fn parse_launch_options_accepts_verbose_flag() {
    let options =
        parse_launch_options(vec!["--verbose".to_string()]).expect("options should parse");
    assert!(options.command.is_none());
    assert!(options.verbose);
}

#[test]
fn parse_launch_options_rejects_unknown_arg() {
    let err = parse_launch_options(vec!["--weird".to_string()]).expect_err("should fail");
    assert!(err.to_string().contains("Unknown argument"));
}

#[test]
fn slash_commands_do_not_route_to_master() {
    assert!(!should_send_to_master("/start"));
    assert!(!should_send_to_master("/backend"));
    assert!(!should_send_to_master("/convert"));
    assert!(!should_send_to_master("/skip-plan"));
    assert!(!should_send_to_master("/definitely-not-a-command"));
    assert!(!should_send_to_master("/quit"));
    assert!(!should_send_to_master("/exit"));
    assert!(!should_send_to_master("/attach-docs"));
    assert!(!should_send_to_master("/newmaster"));
    assert!(!should_send_to_master("/resume"));
    assert!(!should_send_to_master("/split-audits"));
    assert!(!should_send_to_master("/merge-audits"));
    assert!(!should_send_to_master("/split-tests"));
    assert!(!should_send_to_master("/merge-tests"));
    assert!(!should_send_to_master("/add-final-audit"));
    assert!(!should_send_to_master("/remove-final-audit"));
    assert!(!should_send_to_master("start execution"));
    assert!(!should_send_to_master("run tasks"));
    assert!(!should_send_to_master("start"));
    assert!(should_send_to_master("hello"));
}

#[test]
fn session_initialization_gate_only_triggers_for_non_slash_messages() {
    assert!(!should_initialize_session_for_message("/resume"));
    assert!(!should_initialize_session_for_message("   /start   "));
    assert!(should_initialize_session_for_message("Build this feature"));
    assert!(should_initialize_session_for_message("start execution"));
}

#[test]
fn active_session_gate_applies_only_to_session_bound_slash_commands() {
    assert!(command_requires_active_session("/start"));
    assert!(command_requires_active_session("/convert"));
    assert!(!command_requires_active_session("/skip-plan"));
    assert!(command_requires_active_session("/attach-docs"));
    assert!(command_requires_active_session("/split-audits"));
    assert!(command_requires_active_session("/add-final-audit"));
    assert!(!command_requires_active_session("/resume"));
    assert!(!command_requires_active_session("/newmaster"));
    assert!(!command_requires_active_session("hello"));
}

#[test]
fn known_slash_command_detection_is_strict() {
    assert!(is_known_slash_command("/start"));
    assert!(is_known_slash_command("/backend"));
    assert!(is_known_slash_command("/convert"));
    assert!(is_known_slash_command("/skip-plan"));
    assert!(is_known_slash_command("/run"));
    assert!(is_known_slash_command("/quit"));
    assert!(is_known_slash_command("/attach-docs"));
    assert!(!is_known_slash_command("/unknown-cmd"));
    assert!(!is_known_slash_command("hello"));
}

#[test]
fn resumed_right_pane_mode_uses_task_list_when_tasks_exist() {
    let tasks = vec![PlannerTaskFileEntry {
        id: "1".to_string(),
        title: "Task".to_string(),
        details: "details".to_string(),
        docs: Vec::new(),
        kind: PlannerTaskKindFile::Task,
        status: PlannerTaskStatusFile::Pending,
        parent_id: None,
        order: Some(0),
    }];
    assert_eq!(resumed_right_pane_mode(&tasks), RightPaneMode::TaskList);
}

#[test]
fn resumed_right_pane_mode_uses_planner_when_no_tasks_exist() {
    let tasks: Vec<PlannerTaskFileEntry> = Vec::new();
    assert_eq!(
        resumed_right_pane_mode(&tasks),
        RightPaneMode::PlannerMarkdown
    );
}

#[test]
fn task_check_allows_only_quit_commands() {
    assert!(is_allowed_during_task_check("/quit"));
    assert!(is_allowed_during_task_check("/exit"));
    assert!(!is_allowed_during_task_check("/start"));
    assert!(!is_allowed_during_task_check("/attach-docs"));
    assert!(!is_allowed_during_task_check("hello"));
}

#[test]
fn submit_block_reason_prioritizes_project_info_and_respects_task_check_quit_escape() {
    assert_eq!(submit_block_reason(true, false, true, false, "/quit"), None);
    assert_eq!(
        submit_block_reason(false, false, true, false, "hello"),
        Some(SubmitBlockReason::TaskCheck)
    );
    assert_eq!(
        submit_block_reason(false, true, false, false, "hello"),
        Some(SubmitBlockReason::MasterBusy)
    );
    assert_eq!(
        submit_block_reason(false, true, false, false, "/quit"),
        None
    );
    assert_eq!(
        submit_block_reason(false, false, true, false, "/quit"),
        None
    );
    assert_eq!(
        submit_block_reason(false, false, false, false, "hello"),
        None
    );
}

#[test]
fn backend_command_is_not_blocked_while_other_flows_are_in_flight() {
    assert_eq!(
        submit_block_reason(true, true, true, true, "/backend"),
        None
    );
}

#[test]
fn backend_command_recognition_is_trimmed_and_case_insensitive() {
    assert!(is_backend_command("/backend"));
    assert!(is_backend_command("  /BACKEND  "));
    assert!(!is_backend_command("/backend now"));
}

#[test]
fn backend_picker_options_prioritize_current_backend() {
    let codex_first = backend_picker_options(BackendKind::Codex);
    assert_eq!(codex_first.len(), 2);
    assert_eq!(codex_first[0].kind, BackendKind::Codex);
    assert_eq!(codex_first[1].kind, BackendKind::Claude);

    let claude_first = backend_picker_options(BackendKind::Claude);
    assert_eq!(claude_first.len(), 2);
    assert_eq!(claude_first[0].kind, BackendKind::Claude);
    assert_eq!(claude_first[1].kind, BackendKind::Codex);
}

#[test]
fn update_backend_selected_in_toml_preserves_existing_sections() {
    let updated = update_backend_selected_in_toml(
        r#"
        [backend]
        selected = "codex"

        [backend.codex]
        program = "codex-custom"

        [custom]
        keep = true
        "#,
        BackendKind::Claude,
    )
    .expect("backend update should succeed");

    let parsed: toml::Value = toml::from_str(&updated).expect("updated config should parse");
    let selected = parsed
        .get("backend")
        .and_then(|backend| backend.get("selected"))
        .and_then(toml::Value::as_str);
    assert_eq!(selected, Some("claude"));

    let codex_program = parsed
        .get("backend")
        .and_then(|backend| backend.get("codex"))
        .and_then(|codex| codex.get("program"))
        .and_then(toml::Value::as_str);
    assert_eq!(codex_program, Some("codex-custom"));

    let keep_custom = parsed
        .get("custom")
        .and_then(|custom| custom.get("keep"))
        .and_then(toml::Value::as_bool);
    assert_eq!(keep_custom, Some(true));
}

#[test]
fn update_backend_selected_in_toml_rejects_non_table_backend_section() {
    let err = update_backend_selected_in_toml("backend = 1", BackendKind::Claude)
        .expect_err("non-table backend section should fail");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert!(err
        .to_string()
        .contains("backend section is not a TOML table"));
}

#[test]
fn execution_busy_blocks_master_and_task_editing_commands() {
    assert_eq!(
        submit_block_reason(false, false, false, true, "Please update tasks"),
        Some(SubmitBlockReason::ExecutionBusy)
    );
    assert_eq!(
        submit_block_reason(false, false, false, true, "/newmaster"),
        Some(SubmitBlockReason::ExecutionBusy)
    );
    assert_eq!(
        submit_block_reason(false, false, false, true, "/resume"),
        Some(SubmitBlockReason::ExecutionBusy)
    );
    assert_eq!(
        submit_block_reason(false, false, false, true, "/split-audits"),
        Some(SubmitBlockReason::ExecutionBusy)
    );
    assert_eq!(
        submit_block_reason(false, false, false, true, "/add-final-audit"),
        Some(SubmitBlockReason::ExecutionBusy)
    );
    assert_eq!(
        submit_block_reason(false, false, false, true, "/quit"),
        None
    );
    assert_eq!(
        submit_block_reason(false, false, false, true, "/start"),
        None
    );
}

#[test]
fn post_completion_tail_drain_collects_late_worker_output() {
    let adapter = CodexAdapter::with_config(CodexCommandConfig {
        program: "bash".to_string(),
        args_prefix: vec![
            "-lc".to_string(),
            "(sleep 0.03; printf 'late\\n') & printf 'early\\n'".to_string(),
        ],
        output_mode: AdapterOutputMode::PlainText,
        persistent_session: true,
        skip_reader_join_after_wait: false,
        model: None,
        model_reasoning_effort: None,
    });
    adapter.send_prompt("ignored".to_string());

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let mut saw_completed = false;
    let mut saw_late_before_completion = false;
    while std::time::Instant::now() < deadline {
        for event in adapter.drain_events() {
            match event {
                AgentEvent::Output(line) => {
                    if line.trim() == "late" {
                        saw_late_before_completion = true;
                    }
                }
                AgentEvent::Completed { .. } => {
                    saw_completed = true;
                }
                AgentEvent::System(_) => {}
            }
        }
        if saw_completed {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    assert!(saw_completed, "expected completed event");
    let tail = drain_post_completion_worker_events(&adapter);
    let saw_late_in_tail = tail
        .iter()
        .any(|event| matches!(event, AgentEvent::Output(line) if line.trim() == "late"));
    assert!(
        saw_late_before_completion || saw_late_in_tail,
        "expected delayed output to be observable either before completion or in post-completion tail"
    );
}

#[test]
fn master_report_prompt_queue_serializes_dispatch() {
    let mut in_flight = false;
    let mut queue = std::collections::VecDeque::new();

    let first =
        enqueue_or_dispatch_master_report_prompt("first".to_string(), &mut in_flight, &mut queue);
    assert_eq!(first.as_deref(), Some("first"));
    assert!(in_flight);
    assert!(queue.is_empty());

    let second =
        enqueue_or_dispatch_master_report_prompt("second".to_string(), &mut in_flight, &mut queue);
    assert!(second.is_none());
    assert!(in_flight);
    assert_eq!(queue.len(), 1);

    let next = complete_and_next_master_report_prompt(&mut in_flight, &mut queue);
    assert_eq!(next.as_deref(), Some("second"));
    assert!(in_flight);
    assert!(queue.is_empty());

    let done = complete_and_next_master_report_prompt(&mut in_flight, &mut queue);
    assert!(done.is_none());
    assert!(!in_flight);
}

#[test]
fn task_check_start_gate_blocks_while_docs_attach_running() {
    assert!(should_start_task_check(true, false, false));
    assert!(!should_start_task_check(true, true, false));
    assert!(!should_start_task_check(true, false, true));
    assert!(!should_start_task_check(false, false, false));
}

#[test]
fn master_completion_skips_task_file_processing_only_while_execution_is_busy() {
    assert!(!should_process_master_task_file_updates(true));
    assert!(should_process_master_task_file_updates(false));
}

#[test]
fn tasks_change_detection_handles_missing_baseline_for_first_write() {
    assert!(tasks_changed_since_baseline(None, Some("[{\"id\":\"1\"}]")));
    assert!(!tasks_changed_since_baseline(None, Some("   ")));
    assert!(tasks_changed_since_baseline(
        Some("[]"),
        Some("[{\"id\":\"1\"}]")
    ));
    assert!(!tasks_changed_since_baseline(Some("[]"), Some("[]")));
}

#[test]
fn task_write_baseline_is_preserved_only_while_retry_is_requested() {
    assert!(!should_clear_task_write_baseline(false, true));
    assert!(should_clear_task_write_baseline(false, false));
    assert!(should_clear_task_write_baseline(true, true));
}

#[test]
fn reset_master_report_runtime_clears_inflight_queue_and_transcript() {
    let mut in_flight = true;
    let mut queue = std::collections::VecDeque::from([String::from("queued")]);
    let mut transcript = vec![String::from("line")];

    reset_master_report_runtime(&mut in_flight, &mut queue, &mut transcript);

    assert!(!in_flight);
    assert!(queue.is_empty());
    assert!(transcript.is_empty());
}

#[test]
fn reset_task_check_runtime_clears_state() {
    let mut in_flight = true;
    let mut baseline = Some("before".to_string());

    reset_task_check_runtime(&mut in_flight, &mut baseline);

    assert!(!in_flight);
    assert!(baseline.is_none());
}

#[test]
fn prepare_resumed_session_rejects_malformed_tasks_json() {
    let (store, session_dir) = open_temp_store("resume-prepare-malformed");
    std::fs::write(store.tasks_file(), "{ invalid json").expect("write malformed tasks.json");

    let selection = ResumeSessionOption {
        session_dir: session_dir.display().to_string(),
        workspace: "workspace".to_string(),
        title: None,
        created_at_label: None,
        last_used_epoch_secs: 0,
    };
    let cwd = std::env::current_dir().expect("cwd");
    let err = prepare_resumed_session(&cwd, &selection).expect_err("prepare should fail");
    assert!(
        err.to_string()
            .contains("failed to read tasks.json for resumed session")
    );

    std::fs::remove_dir_all(session_dir).ok();
}

#[test]
fn prepare_resumed_session_rejects_invalid_task_hierarchy() {
    let (store, session_dir) = open_temp_store("resume-prepare-invalid-shape");
    let invalid_tasks = vec![
        PlannerTaskFileEntry {
            id: "top".to_string(),
            title: "Top".to_string(),
            details: "d".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Task,
            status: PlannerTaskStatusFile::Pending,
            parent_id: None,
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl".to_string(),
            title: "Impl".to_string(),
            details: "d".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Implementor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("top".to_string()),
            order: Some(0),
        },
    ];
    std::fs::write(
        store.tasks_file(),
        serde_json::to_string_pretty(&invalid_tasks).expect("serialize invalid tasks"),
    )
    .expect("write invalid tasks");

    let selection = ResumeSessionOption {
        session_dir: session_dir.display().to_string(),
        workspace: "workspace".to_string(),
        title: None,
        created_at_label: None,
        last_used_epoch_secs: 0,
    };
    let cwd = std::env::current_dir().expect("cwd");
    let err = prepare_resumed_session(&cwd, &selection).expect_err("prepare should fail");
    assert!(
        err.to_string()
            .contains("failed to validate resumed tasks.json")
    );

    std::fs::remove_dir_all(session_dir).ok();
}

#[test]
fn resume_session_does_not_switch_when_target_tasks_are_invalid() {
    let (current_store, current_dir) = open_temp_store("resume-current-valid");
    let current_tasks = vec![PlannerTaskFileEntry {
        id: "current-task".to_string(),
        title: "Current Task".to_string(),
        details: "details".to_string(),
        docs: Vec::new(),
        kind: PlannerTaskKindFile::Task,
        status: PlannerTaskStatusFile::Pending,
        parent_id: None,
        order: Some(0),
    }];
    std::fs::write(
        current_store.tasks_file(),
        serde_json::to_string_pretty(&current_tasks).expect("serialize current tasks"),
    )
    .expect("write current tasks");

    let (target_store, target_dir) = open_temp_store("resume-target-invalid");
    std::fs::write(target_store.tasks_file(), "{ invalid json")
        .expect("write malformed target tasks");

    let mut app = App::default();
    app.sync_planner_tasks_from_file(current_tasks)
        .expect("seed current task view");
    app.set_right_pane_mode(RightPaneMode::TaskList);
    let mut session_store = Some(current_store);

    let master_adapter = CodexAdapter::new();
    let master_report_adapter = CodexAdapter::new();
    let project_info_adapter = CodexAdapter::new();
    let mut worker_agent_adapters: HashMap<String, CodexAdapter> = HashMap::new();
    let mut active_worker_context_key = None;
    let mut pending_task_write_baseline = None;
    let mut docs_attach_in_flight = false;
    let mut master_session_intro_needed = false;
    let mut master_report_session_intro_needed = false;
    let mut pending_master_message_after_project_info = None;
    let mut project_info_in_flight = false;
    let mut project_info_stage = None;
    let mut project_info_text = None;
    let mut master_report_in_flight = true;
    let mut pending_master_report_prompts = std::collections::VecDeque::from([String::from("x")]);
    let mut master_report_transcript = vec![String::from("line")];
    let mut task_check_in_flight = true;
    let mut task_check_baseline = Some("baseline".to_string());

    let backend = ratatui::backend::TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).expect("test terminal");

    let selection = ResumeSessionOption {
        session_dir: target_dir.display().to_string(),
        workspace: "workspace".to_string(),
        title: None,
        created_at_label: None,
        last_used_epoch_secs: 0,
    };
    resume_session(
        &mut app,
        &mut session_store,
        selection,
        &master_adapter,
        &master_report_adapter,
        &project_info_adapter,
        &mut worker_agent_adapters,
        &mut active_worker_context_key,
        &mut pending_task_write_baseline,
        &mut docs_attach_in_flight,
        &mut master_session_intro_needed,
        &mut master_report_session_intro_needed,
        &mut pending_master_message_after_project_info,
        &mut project_info_in_flight,
        &mut project_info_stage,
        &mut project_info_text,
        &mut master_report_in_flight,
        &mut pending_master_report_prompts,
        &mut master_report_transcript,
        &mut task_check_in_flight,
        &mut task_check_baseline,
        &mut terminal,
    )
    .expect("resume should not hard-fail");

    let active_session = session_store.as_ref().expect("session remains set");
    assert_eq!(
        active_session.session_dir(),
        current_dir.as_path(),
        "resume should not switch active session when target tasks are invalid"
    );
    assert!(
        app.right_block_lines(80)
            .iter()
            .any(|line| line.contains("Current Task"))
    );
    let last = app
        .left_bottom_lines()
        .last()
        .expect("resume failure message should be present");
    assert!(last.contains("Failed to resume session"));

    std::fs::remove_dir_all(current_dir).ok();
    std::fs::remove_dir_all(target_dir).ok();
}

#[test]
fn resume_session_resets_master_report_and_task_check_runtime_state_on_success() {
    let (current_store, current_dir) = open_temp_store("resume-current-state-reset");
    let (target_store, target_dir) = open_temp_store("resume-target-state-reset");
    let target_tasks = vec![PlannerTaskFileEntry {
        id: "target-task".to_string(),
        title: "Target Task".to_string(),
        details: "details".to_string(),
        docs: Vec::new(),
        kind: PlannerTaskKindFile::Task,
        status: PlannerTaskStatusFile::Pending,
        parent_id: None,
        order: Some(0),
    }];
    std::fs::write(
        target_store.tasks_file(),
        serde_json::to_string_pretty(&target_tasks).expect("serialize target tasks"),
    )
    .expect("write target tasks");

    let mut app = App::default();
    let mut session_store = Some(current_store);

    let master_adapter = CodexAdapter::new();
    let master_report_adapter = CodexAdapter::new();
    let project_info_adapter = CodexAdapter::new();
    let mut worker_agent_adapters: HashMap<String, CodexAdapter> = HashMap::new();
    let mut active_worker_context_key = None;
    let mut pending_task_write_baseline = Some(PendingTaskWriteBaseline {
        tasks_json: "[]".to_string(),
    });
    let mut docs_attach_in_flight = true;
    let mut master_session_intro_needed = false;
    let mut master_report_session_intro_needed = false;
    let mut pending_master_message_after_project_info = Some("queued".to_string());
    let mut project_info_in_flight = true;
    let mut project_info_stage = Some(ProjectInfoStage::GatheringInfo);
    let mut project_info_text = Some("context".to_string());
    let mut master_report_in_flight = true;
    let mut pending_master_report_prompts = std::collections::VecDeque::from([String::from("x")]);
    let mut master_report_transcript = vec![String::from("line")];
    let mut task_check_in_flight = true;
    let mut task_check_baseline = Some("baseline".to_string());

    let backend = ratatui::backend::TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).expect("test terminal");

    let selection = ResumeSessionOption {
        session_dir: target_dir.display().to_string(),
        workspace: "workspace".to_string(),
        title: None,
        created_at_label: None,
        last_used_epoch_secs: 0,
    };
    resume_session(
        &mut app,
        &mut session_store,
        selection,
        &master_adapter,
        &master_report_adapter,
        &project_info_adapter,
        &mut worker_agent_adapters,
        &mut active_worker_context_key,
        &mut pending_task_write_baseline,
        &mut docs_attach_in_flight,
        &mut master_session_intro_needed,
        &mut master_report_session_intro_needed,
        &mut pending_master_message_after_project_info,
        &mut project_info_in_flight,
        &mut project_info_stage,
        &mut project_info_text,
        &mut master_report_in_flight,
        &mut pending_master_report_prompts,
        &mut master_report_transcript,
        &mut task_check_in_flight,
        &mut task_check_baseline,
        &mut terminal,
    )
    .expect("resume should succeed");

    let active_session = session_store.as_ref().expect("session should be present");
    assert_eq!(active_session.session_dir(), target_dir.as_path());
    assert!(!master_report_in_flight);
    assert!(pending_master_report_prompts.is_empty());
    assert!(master_report_transcript.is_empty());
    assert!(!task_check_in_flight);
    assert!(task_check_baseline.is_none());

    std::fs::remove_dir_all(current_dir).ok();
    std::fs::remove_dir_all(target_dir).ok();
}

#[test]
fn newmaster_resets_master_report_and_task_check_runtime_state() {
    let mut app = App::default();
    let master_adapter = CodexAdapter::new();
    let master_report_adapter = CodexAdapter::new();
    let project_info_adapter = CodexAdapter::new();
    let docs_attach_adapter = CodexAdapter::new();
    let test_runner_adapter = TestRunnerAdapter::new();
    let model_routing = CodexAgentModelRouting::default();

    let mut worker_agent_adapters: HashMap<String, CodexAdapter> = HashMap::new();
    let mut active_worker_context_key = None;
    let mut session_store: Option<SessionStore> = None;
    let cwd = std::env::current_dir().expect("cwd");
    let mut pending_task_write_baseline = Some(PendingTaskWriteBaseline {
        tasks_json: "[]".to_string(),
    });
    let mut docs_attach_in_flight = false;
    let mut master_session_intro_needed = false;
    let mut master_report_session_intro_needed = false;
    let mut pending_master_message_after_project_info = Some("queued".to_string());
    let mut project_info_in_flight = true;
    let mut project_info_stage = Some(ProjectInfoStage::GatheringInfo);
    let mut project_info_text = Some("context".to_string());
    let mut master_report_in_flight = true;
    let mut pending_master_report_prompts = std::collections::VecDeque::from([String::from("x")]);
    let mut master_report_transcript = vec![String::from("line")];
    let mut task_check_in_flight = true;
    let mut task_check_baseline = Some("before".to_string());

    app.set_task_check_in_progress(true);
    app.set_docs_attach_in_progress(true);
    app.set_master_in_progress(true);

    let backend = ratatui::backend::TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).expect("test terminal");

    submit_user_message(
        &mut app,
        "/newmaster".to_string(),
        &master_adapter,
        &master_report_adapter,
        &project_info_adapter,
        &mut worker_agent_adapters,
        &mut active_worker_context_key,
        &docs_attach_adapter,
        &test_runner_adapter,
        &mut master_report_in_flight,
        &mut pending_master_report_prompts,
        &mut master_report_transcript,
        &mut task_check_in_flight,
        &mut task_check_baseline,
        &mut session_store,
        &cwd,
        &mut terminal,
        &mut pending_task_write_baseline,
        &mut docs_attach_in_flight,
        &mut master_session_intro_needed,
        &mut master_report_session_intro_needed,
        &mut pending_master_message_after_project_info,
        &mut project_info_in_flight,
        &mut project_info_stage,
        &mut project_info_text,
        &model_routing,
    )
    .expect("newmaster command should succeed");

    assert!(!master_report_in_flight);
    assert!(pending_master_report_prompts.is_empty());
    assert!(master_report_transcript.is_empty());
    assert!(!task_check_in_flight);
    assert!(task_check_baseline.is_none());
}

#[test]
fn cli_task_contract_round_trip_preserves_all_fields() {
    let file_task = PlannerTaskFileEntry {
        id: "task-1".to_string(),
        title: "Implement adapter seam".to_string(),
        details: "details".to_string(),
        docs: vec![session_store::PlannerTaskDocFileEntry {
            title: "Rust tests".to_string(),
            url: "https://doc.rust-lang.org/stable/book/ch11-00-testing.html".to_string(),
            summary: "Testing chapter".to_string(),
        }],
        kind: PlannerTaskKindFile::TestWriter,
        status: PlannerTaskStatusFile::NeedsChanges,
        parent_id: Some("top".to_string()),
        order: Some(2),
    };

    let contract = file_task_to_contract_task(file_task.clone());
    let round_tripped = contract_task_to_file_task(contract);
    assert_eq!(round_tripped.id, file_task.id);
    assert_eq!(round_tripped.title, file_task.title);
    assert_eq!(round_tripped.details, file_task.details);
    assert_eq!(round_tripped.parent_id, file_task.parent_id);
    assert_eq!(round_tripped.order, file_task.order);
    assert_eq!(round_tripped.kind, file_task.kind);
    assert_eq!(round_tripped.status, file_task.status);
    assert_eq!(round_tripped.docs.len(), file_task.docs.len());
    assert_eq!(round_tripped.docs[0].title, file_task.docs[0].title);
    assert_eq!(round_tripped.docs[0].url, file_task.docs[0].url);
    assert_eq!(round_tripped.docs[0].summary, file_task.docs[0].summary);
}

#[test]
fn execute_core_api_contract_keeps_request_identity_and_capability() {
    let request = api::RequestEnvelope {
        request_id: Some("req-42".to_string()),
        capability: api::CapabilityId::AppPromptPreparation,
        metadata: api::RequestMetadata {
            transport: Some("mock-transport".to_string()),
            actor: Some("mock-actor".to_string()),
        },
        payload: api::ApiRequestContract::App(api::AppRequest::PrepareAttachDocsPrompt {
            tasks_file: "/tmp/tasks.json".to_string(),
        }),
    };

    let response = execute_core_api_contract(request).expect("contract execution should succeed");
    assert_eq!(response.request_id.as_deref(), Some("req-42"));
    assert_eq!(response.capability, api::CapabilityId::AppPromptPreparation);
    assert!(matches!(
        response.result,
        api::ApiResultEnvelope::Ok {
            data: api::ApiResponseContract::App(api::AppResponse::Prompt { .. })
        }
    ));
}

#[test]
fn execute_core_workflow_validation_is_transport_agnostic() {
    let tasks = vec![api::PlannerTaskEntryContract {
        id: "top".to_string(),
        title: "Top".to_string(),
        details: "d".to_string(),
        docs: Vec::new(),
        kind: api::PlannerTaskKindContract::Task,
        status: api::PlannerTaskStatusContract::Pending,
        parent_id: None,
        order: Some(0),
    }];

    let request_with_cli_transport = api::RequestEnvelope {
        request_id: Some("a".to_string()),
        capability: api::CapabilityId::WorkflowTaskGraphSync,
        metadata: api::RequestMetadata {
            transport: Some("cli".to_string()),
            actor: None,
        },
        payload: api::ApiRequestContract::Workflow(api::WorkflowRequest::SyncPlannerTasks {
            tasks: tasks.clone(),
        }),
    };

    let request_with_mock_transport = api::RequestEnvelope {
        request_id: Some("b".to_string()),
        capability: api::CapabilityId::WorkflowTaskGraphSync,
        metadata: api::RequestMetadata {
            transport: Some("mock-http".to_string()),
            actor: None,
        },
        payload: api::ApiRequestContract::Workflow(api::WorkflowRequest::SyncPlannerTasks {
            tasks,
        }),
    };

    let response_a = execute_core_api_contract(request_with_cli_transport)
        .expect("cli transport contract request should succeed");
    let response_b = execute_core_api_contract(request_with_mock_transport)
        .expect("mock transport contract request should succeed");

    let data_a = match response_a.result {
        api::ApiResultEnvelope::Ok { data } => data,
        api::ApiResultEnvelope::Err { error } => panic!("unexpected error: {error:?}"),
    };
    let data_b = match response_b.result {
        api::ApiResultEnvelope::Ok { data } => data,
        api::ApiResultEnvelope::Err { error } => panic!("unexpected error: {error:?}"),
    };

    assert_eq!(data_a, data_b);
}

#[test]
fn execute_core_unsupported_domain_is_transport_agnostic() {
    let request_cli = api::RequestEnvelope {
        request_id: None,
        capability: api::CapabilityId::EventPolling,
        metadata: api::RequestMetadata {
            transport: Some("cli".to_string()),
            actor: None,
        },
        payload: api::ApiRequestContract::Events(api::EventsRequest::NextEvent),
    };
    let request_mock = api::RequestEnvelope {
        request_id: None,
        capability: api::CapabilityId::EventPolling,
        metadata: api::RequestMetadata {
            transport: Some("mock-transport".to_string()),
            actor: Some("{\"source\":\"mock\"}".to_string()),
        },
        payload: api::ApiRequestContract::Events(api::EventsRequest::NextEvent),
    };

    let err_cli = execute_core_api_contract(request_cli).expect_err("events domain should fail");
    let err_mock = execute_core_api_contract(request_mock).expect_err("events domain should fail");

    assert_eq!(err_cli.code, api::ApiErrorCode::Unsupported);
    assert_eq!(err_mock.code, api::ApiErrorCode::Unsupported);
    assert_eq!(err_cli.message, err_mock.message);
}

#[test]
fn execute_core_session_project_info_write_then_read_round_trips() {
    let (store, session_dir) = open_temp_store("metaagent-session-project-info-contract");
    let cwd = std::env::current_dir().expect("cwd");
    let actor = format!(
        "{{\"cwd\":\"{}\",\"session_dir\":\"{}\"}}",
        cwd.display(),
        session_dir.display()
    );

    let write_request = api::RequestEnvelope {
        request_id: Some("write-project-info".to_string()),
        capability: api::CapabilityId::SessionProjectContextStorage,
        metadata: api::RequestMetadata {
            transport: Some("cli".to_string()),
            actor: Some(actor.clone()),
        },
        payload: api::ApiRequestContract::Session(api::SessionRequest::WriteProjectInfo {
            markdown: "# Context\n\n- Added via API contract\n".to_string(),
        }),
    };
    let write_response =
        execute_core_api_contract(write_request).expect("write project info should succeed");
    assert!(matches!(
        write_response.result,
        api::ApiResultEnvelope::Ok {
            data: api::ApiResponseContract::Session(api::SessionResponse::Ack)
        }
    ));

    let read_request = api::RequestEnvelope {
        request_id: Some("read-project-info".to_string()),
        capability: api::CapabilityId::SessionProjectContextStorage,
        metadata: api::RequestMetadata {
            transport: Some("cli".to_string()),
            actor: Some(actor),
        },
        payload: api::ApiRequestContract::Session(api::SessionRequest::ReadProjectInfo),
    };
    let read_response =
        execute_core_api_contract(read_request).expect("read project info should succeed");
    match read_response.result {
        api::ApiResultEnvelope::Ok {
            data: api::ApiResponseContract::Session(api::SessionResponse::ProjectInfo { markdown }),
        } => assert_eq!(markdown, "# Context\n\n- Added via API contract\n"),
        other => panic!("unexpected response: {other:?}"),
    }

    drop(store);
    std::fs::remove_dir_all(session_dir).ok();
}

#[test]
fn execute_core_session_task_fails_append_then_read_round_trips() {
    let (store, session_dir) = open_temp_store("metaagent-session-task-fails-contract");
    let cwd = std::env::current_dir().expect("cwd");
    let actor = format!(
        "{{\"cwd\":\"{}\",\"session_dir\":\"{}\"}}",
        cwd.display(),
        session_dir.display()
    );

    let append_request = api::RequestEnvelope {
        request_id: Some("append-task-fails".to_string()),
        capability: api::CapabilityId::SessionFailureStorage,
        metadata: api::RequestMetadata {
            transport: Some("cli".to_string()),
            actor: Some(actor.clone()),
        },
        payload: api::ApiRequestContract::Session(api::SessionRequest::AppendTaskFails {
            entries: vec![api::TaskFailureContract {
                kind: api::WorkflowFailureKindContract::Test,
                top_task_id: 44,
                top_task_title: "Regression coverage".to_string(),
                attempts: 2,
                reason: "missing test path".to_string(),
                action_taken: "requeued".to_string(),
            }],
        }),
    };
    let append_response =
        execute_core_api_contract(append_request).expect("append task fails should succeed");
    assert!(matches!(
        append_response.result,
        api::ApiResultEnvelope::Ok {
            data: api::ApiResponseContract::Session(api::SessionResponse::Ack)
        }
    ));

    let read_request = api::RequestEnvelope {
        request_id: Some("read-task-fails".to_string()),
        capability: api::CapabilityId::SessionFailureStorage,
        metadata: api::RequestMetadata {
            transport: Some("cli".to_string()),
            actor: Some(actor),
        },
        payload: api::ApiRequestContract::Session(api::SessionRequest::ReadTaskFails),
    };
    let read_response =
        execute_core_api_contract(read_request).expect("read task fails should succeed");
    match read_response.result {
        api::ApiResultEnvelope::Ok {
            data: api::ApiResponseContract::Session(api::SessionResponse::TaskFails { entries }),
        } => {
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].kind, api::WorkflowFailureKindContract::Test);
            assert_eq!(entries[0].top_task_id, 44);
            assert_eq!(entries[0].top_task_title, "Regression coverage");
            assert_eq!(entries[0].attempts, 2);
        }
        other => panic!("unexpected response: {other:?}"),
    }

    drop(store);
    std::fs::remove_dir_all(session_dir).ok();
}

#[test]
fn execute_core_session_read_session_meta_maps_file_fields() {
    let (store, session_dir) = open_temp_store("metaagent-session-meta-contract");
    let cwd = std::env::current_dir().expect("cwd");
    let actor = format!(
        "{{\"cwd\":\"{}\",\"session_dir\":\"{}\"}}",
        cwd.display(),
        session_dir.display()
    );

    std::fs::write(
        store.session_meta_file(),
        r#"{"title":"Hardening pass","created_at":"2026-02-18T12:00:00Z","stack_description":"Rust","test_command":"cargo test"}"#,
    )
    .expect("write session meta");

    let request = api::RequestEnvelope {
        request_id: Some("read-session-meta".to_string()),
        capability: api::CapabilityId::SessionProjectContextStorage,
        metadata: api::RequestMetadata {
            transport: Some("cli".to_string()),
            actor: Some(actor),
        },
        payload: api::ApiRequestContract::Session(api::SessionRequest::ReadSessionMeta),
    };
    let response = execute_core_api_contract(request).expect("read session meta should succeed");
    match response.result {
        api::ApiResultEnvelope::Ok {
            data: api::ApiResponseContract::Session(api::SessionResponse::SessionMeta { meta }),
        } => {
            assert_eq!(meta.title, "Hardening pass");
            assert_eq!(meta.created_at, "2026-02-18T12:00:00Z");
            assert_eq!(meta.stack_description, "Rust");
            assert_eq!(meta.test_command.as_deref(), Some("cargo test"));
        }
        other => panic!("unexpected response: {other:?}"),
    }

    drop(store);
    std::fs::remove_dir_all(session_dir).ok();
}

#[test]
fn convert_submit_uses_prompt_service_and_captures_tasks_baseline() {
    let mut app = App::default();
    let master_adapter = CodexAdapter::new();
    let master_report_adapter = CodexAdapter::new();
    let project_info_adapter = CodexAdapter::new();
    let docs_attach_adapter = CodexAdapter::new();
    let test_runner_adapter = TestRunnerAdapter::new();
    let model_routing = CodexAgentModelRouting::default();

    let mut worker_agent_adapters: HashMap<String, CodexAdapter> = HashMap::new();
    let mut active_worker_context_key = None;
    let cwd = std::env::current_dir().expect("cwd");
    let (store, session_dir) = open_temp_store("metaagent-convert-submit-service");
    let mut session_store = Some(store);
    let active_session = session_store.as_ref().expect("active session");
    let baseline_text = r#"[{"id":"top"}]"#;
    std::fs::write(active_session.tasks_file(), baseline_text).expect("write baseline tasks");

    let mut pending_task_write_baseline = None;
    let mut docs_attach_in_flight = false;
    let mut master_session_intro_needed = true;
    let mut master_report_session_intro_needed = true;
    let mut pending_master_message_after_project_info = None;
    let mut project_info_in_flight = false;
    let mut project_info_stage = None;
    let mut project_info_text = Some("existing project context".to_string());
    let mut master_report_in_flight = false;
    let mut pending_master_report_prompts = std::collections::VecDeque::new();
    let mut master_report_transcript = Vec::new();
    let mut task_check_in_flight = false;
    let mut task_check_baseline = None;

    let backend = ratatui::backend::TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).expect("test terminal");

    submit_user_message(
        &mut app,
        "/convert".to_string(),
        &master_adapter,
        &master_report_adapter,
        &project_info_adapter,
        &mut worker_agent_adapters,
        &mut active_worker_context_key,
        &docs_attach_adapter,
        &test_runner_adapter,
        &mut master_report_in_flight,
        &mut pending_master_report_prompts,
        &mut master_report_transcript,
        &mut task_check_in_flight,
        &mut task_check_baseline,
        &mut session_store,
        &cwd,
        &mut terminal,
        &mut pending_task_write_baseline,
        &mut docs_attach_in_flight,
        &mut master_session_intro_needed,
        &mut master_report_session_intro_needed,
        &mut pending_master_message_after_project_info,
        &mut project_info_in_flight,
        &mut project_info_stage,
        &mut project_info_text,
        &model_routing,
    )
    .expect("convert should succeed");

    assert!(app.is_master_in_progress());
    assert!(!app.is_planner_mode());
    assert!(!master_session_intro_needed);
    assert_eq!(
        pending_task_write_baseline
            .as_ref()
            .map(|baseline| baseline.tasks_json.as_str()),
        Some(baseline_text)
    );

    std::fs::remove_dir_all(session_dir).ok();
}

#[test]
fn normal_submit_with_project_context_uses_prompt_service_path() {
    let mut app = App::default();
    app.set_right_pane_mode(RightPaneMode::TaskList);
    let master_adapter = CodexAdapter::new();
    let master_report_adapter = CodexAdapter::new();
    let project_info_adapter = CodexAdapter::new();
    let docs_attach_adapter = CodexAdapter::new();
    let test_runner_adapter = TestRunnerAdapter::new();
    let model_routing = CodexAgentModelRouting::default();

    let mut worker_agent_adapters: HashMap<String, CodexAdapter> = HashMap::new();
    let mut active_worker_context_key = None;
    let cwd = std::env::current_dir().expect("cwd");
    let (store, session_dir) = open_temp_store("metaagent-message-submit-service");
    let mut session_store = Some(store);
    let active_session = session_store.as_ref().expect("active session");
    let baseline_text = r#"[{"id":"existing"}]"#;
    std::fs::write(active_session.tasks_file(), baseline_text).expect("write baseline tasks");

    let mut pending_task_write_baseline = None;
    let mut docs_attach_in_flight = false;
    let mut master_session_intro_needed = true;
    let mut master_report_session_intro_needed = true;
    let mut pending_master_message_after_project_info = None;
    let mut project_info_in_flight = false;
    let mut project_info_stage = None;
    let mut project_info_text = Some("existing project context".to_string());
    let mut master_report_in_flight = false;
    let mut pending_master_report_prompts = std::collections::VecDeque::new();
    let mut master_report_transcript = Vec::new();
    let mut task_check_in_flight = false;
    let mut task_check_baseline = None;

    let backend = ratatui::backend::TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).expect("test terminal");

    submit_user_message(
        &mut app,
        "Please plan this change".to_string(),
        &master_adapter,
        &master_report_adapter,
        &project_info_adapter,
        &mut worker_agent_adapters,
        &mut active_worker_context_key,
        &docs_attach_adapter,
        &test_runner_adapter,
        &mut master_report_in_flight,
        &mut pending_master_report_prompts,
        &mut master_report_transcript,
        &mut task_check_in_flight,
        &mut task_check_baseline,
        &mut session_store,
        &cwd,
        &mut terminal,
        &mut pending_task_write_baseline,
        &mut docs_attach_in_flight,
        &mut master_session_intro_needed,
        &mut master_report_session_intro_needed,
        &mut pending_master_message_after_project_info,
        &mut project_info_in_flight,
        &mut project_info_stage,
        &mut project_info_text,
        &model_routing,
    )
    .expect("message submit should succeed");

    assert!(app.is_master_in_progress());
    assert!(!master_session_intro_needed);
    assert!(!project_info_in_flight);
    assert!(pending_master_message_after_project_info.is_none());
    assert_eq!(
        pending_task_write_baseline
            .as_ref()
            .map(|baseline| baseline.tasks_json.as_str()),
        Some(baseline_text)
    );

    std::fs::remove_dir_all(session_dir).ok();
}

#[test]
fn start_submit_claims_first_job_and_persists_snapshot_via_service() {
    let mut app = App::default();
    app.sync_planner_tasks_from_file(simple_execution_plan())
        .expect("sync plan");
    let master_adapter = CodexAdapter::new();
    let master_report_adapter = CodexAdapter::new();
    let project_info_adapter = CodexAdapter::new();
    let docs_attach_adapter = CodexAdapter::new();
    let test_runner_adapter = TestRunnerAdapter::new();
    let model_routing = CodexAgentModelRouting::default();

    let mut worker_agent_adapters: HashMap<String, CodexAdapter> = HashMap::new();
    let mut active_worker_context_key = None;
    let cwd = std::env::current_dir().expect("cwd");
    let (store, session_dir) = open_temp_store("metaagent-start-submit-service");
    let mut session_store = Some(store);

    let mut pending_task_write_baseline = None;
    let mut docs_attach_in_flight = false;
    let mut master_session_intro_needed = true;
    let mut master_report_session_intro_needed = true;
    let mut pending_master_message_after_project_info = None;
    let mut project_info_in_flight = false;
    let mut project_info_stage = None;
    let mut project_info_text = Some("existing project context".to_string());
    let mut master_report_in_flight = false;
    let mut pending_master_report_prompts = std::collections::VecDeque::new();
    let mut master_report_transcript = Vec::new();
    let mut task_check_in_flight = false;
    let mut task_check_baseline = None;

    let backend = ratatui::backend::TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).expect("test terminal");

    submit_user_message(
        &mut app,
        "/start".to_string(),
        &master_adapter,
        &master_report_adapter,
        &project_info_adapter,
        &mut worker_agent_adapters,
        &mut active_worker_context_key,
        &docs_attach_adapter,
        &test_runner_adapter,
        &mut master_report_in_flight,
        &mut pending_master_report_prompts,
        &mut master_report_transcript,
        &mut task_check_in_flight,
        &mut task_check_baseline,
        &mut session_store,
        &cwd,
        &mut terminal,
        &mut pending_task_write_baseline,
        &mut docs_attach_in_flight,
        &mut master_session_intro_needed,
        &mut master_report_session_intro_needed,
        &mut pending_master_message_after_project_info,
        &mut project_info_in_flight,
        &mut project_info_stage,
        &mut project_info_text,
        &model_routing,
    )
    .expect("start submit should succeed");

    let active_session = session_store.as_ref().expect("active session");
    let persisted = active_session.read_tasks().expect("read tasks");
    let impl_status = persisted
        .iter()
        .find(|entry| entry.id == "impl")
        .map(|entry| entry.status);
    assert_eq!(impl_status, Some(PlannerTaskStatusFile::InProgress));
    assert!(
        app.left_bottom_lines()
            .iter()
            .any(|line| line.contains("Starting Implementor for task #1")),
        "expected worker dispatch status message after /start"
    );

    std::fs::remove_dir_all(session_dir).ok();
}

#[test]
fn persist_runtime_tasks_snapshot_writes_updated_subtask_statuses() {
    let mut app = App::default();
    app.sync_planner_tasks_from_file(vec![
        PlannerTaskFileEntry {
            id: "top".to_string(),
            title: "Task".to_string(),
            details: "top details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Task,
            status: PlannerTaskStatusFile::Pending,
            parent_id: None,
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl".to_string(),
            title: "Implementation".to_string(),
            details: "impl details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Implementor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("top".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl-audit".to_string(),
            title: "Audit".to_string(),
            details: "audit details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Auditor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("impl".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "tw".to_string(),
            title: "Write tests".to_string(),
            details: "tw details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::TestWriter,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("top".to_string()),
            order: Some(1),
        },
        PlannerTaskFileEntry {
            id: "tw-runner".to_string(),
            title: "Run tests".to_string(),
            details: "runner details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::TestRunner,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("tw".to_string()),
            order: Some(0),
        },
    ])
    .expect("sync should succeed");

    app.start_execution();
    let _ = app.start_next_worker_job().expect("implementor");
    app.on_worker_output("implemented".to_string());
    app.on_worker_completed(true, 0);

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let cwd = std::env::current_dir().expect("cwd");
    let session_dir = std::env::temp_dir().join(format!("metaagent-runtime-status-{now}"));
    let store = SessionStore::open_existing(&cwd, &session_dir).expect("open existing store");

    persist_runtime_tasks_snapshot(&app, &store).expect("persist runtime snapshot");
    let persisted = store.read_tasks().expect("read persisted tasks");

    let impl_status = persisted
        .iter()
        .find(|entry| entry.id == "impl")
        .map(|entry| entry.status);
    assert_eq!(impl_status, Some(PlannerTaskStatusFile::Done));

    let audit_status = persisted
        .iter()
        .find(|entry| entry.id == "impl-audit")
        .map(|entry| entry.status);
    assert_eq!(audit_status, Some(PlannerTaskStatusFile::Pending));

    let _ = std::fs::remove_dir_all(&session_dir);
}

#[test]
fn start_execution_claim_persists_first_job_status_immediately() {
    let mut app = App::default();
    app.sync_planner_tasks_from_file(vec![
        PlannerTaskFileEntry {
            id: "top".to_string(),
            title: "Task".to_string(),
            details: "top details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Task,
            status: PlannerTaskStatusFile::Pending,
            parent_id: None,
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl".to_string(),
            title: "Implementation".to_string(),
            details: "impl details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Implementor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("top".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl-audit".to_string(),
            title: "Audit".to_string(),
            details: "audit details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Auditor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("impl".to_string()),
            order: Some(0),
        },
    ])
    .expect("sync should succeed");
    let (store, session_dir) = open_temp_store("metaagent-start-persist-first-dispatch");

    app.start_execution();
    let first_job =
        claim_next_worker_job_and_persist_snapshot(&mut app, &store).expect("first worker job");
    assert!(matches!(first_job.run, JobRun::AgentPrompt(_)));

    let persisted = store.read_tasks().expect("read persisted tasks");
    let impl_status = persisted
        .iter()
        .find(|entry| entry.id == "impl")
        .map(|entry| entry.status);
    assert_eq!(impl_status, Some(PlannerTaskStatusFile::InProgress));

    let _ = std::fs::remove_dir_all(&session_dir);
}

#[test]
fn integration_happy_path_persists_and_reloads_cleanly() {
    let mut app = App::default();
    app.sync_planner_tasks_from_file(integration_plan_with_final())
        .expect("sync should succeed");
    let (store, session_dir) = open_temp_store("metaagent-integration-happy");

    app.start_execution();
    for _ in 0..40 {
        let Some(job) = app.start_next_worker_job() else {
            break;
        };
        match job.run {
            JobRun::AgentPrompt(prompt) => {
                if prompt.contains("reviewing implementation output")
                    || prompt.contains("reviewing test-writing output")
                    || prompt.contains("final audit sub-agent")
                {
                    app.on_worker_output("AUDIT_RESULT: PASS".to_string());
                    app.on_worker_output("No issues found".to_string());
                } else {
                    app.on_worker_output("completed".to_string());
                }
                let _ = app.on_worker_completed(true, 0);
            }
            JobRun::DeterministicTestRun => {
                app.on_worker_output("all passed".to_string());
                let _ = app.on_worker_completed(true, 0);
            }
        }
        persist_runtime_tasks_snapshot(&app, &store).expect("persist runtime snapshot");
    }

    let persisted = store.read_tasks().expect("read persisted tasks");
    let top_status = persisted
        .iter()
        .find(|entry| entry.id == "top")
        .map(|entry| entry.status);
    let final_status = persisted
        .iter()
        .find(|entry| entry.id == "final")
        .map(|entry| entry.status);
    assert_eq!(top_status, Some(PlannerTaskStatusFile::Done));
    assert_eq!(final_status, Some(PlannerTaskStatusFile::Done));

    let mut reloaded = App::default();
    reloaded
        .sync_planner_tasks_from_file(persisted)
        .expect("reload from persisted tasks should sync");
    reloaded.start_execution();
    assert!(reloaded.start_next_worker_job().is_none());

    let _ = std::fs::remove_dir_all(&session_dir);
}

#[test]
fn integration_retry_paths_recover_without_exhausted_failures() {
    let mut app = App::default();
    app.sync_planner_tasks_from_file(integration_plan_with_final())
        .expect("sync should succeed");
    let (store, session_dir) = open_temp_store("metaagent-integration-retries");

    let mut impl_audit_failures_left = 1u8;
    let mut test_audit_failures_left = 1u8;
    let mut runner_failures_left = 1u8;

    app.start_execution();
    for _ in 0..80 {
        let Some(job) = app.start_next_worker_job() else {
            break;
        };
        match job.run {
            JobRun::AgentPrompt(prompt) => {
                if prompt.contains("reviewing implementation output") {
                    if impl_audit_failures_left > 0 {
                        impl_audit_failures_left = impl_audit_failures_left.saturating_sub(1);
                        app.on_worker_output("AUDIT_RESULT: FAIL".to_string());
                        app.on_worker_output("Critical blocker still present".to_string());
                    } else {
                        app.on_worker_output("AUDIT_RESULT: PASS".to_string());
                        app.on_worker_output("No issues found".to_string());
                    }
                } else if prompt.contains("reviewing test-writing output") {
                    if test_audit_failures_left > 0 {
                        test_audit_failures_left = test_audit_failures_left.saturating_sub(1);
                        app.on_worker_output("AUDIT_RESULT: FAIL".to_string());
                        app.on_worker_output("Coverage gap remains".to_string());
                    } else {
                        app.on_worker_output("AUDIT_RESULT: PASS".to_string());
                        app.on_worker_output("No issues found".to_string());
                    }
                } else if prompt.contains("final audit sub-agent") {
                    app.on_worker_output("AUDIT_RESULT: PASS".to_string());
                    app.on_worker_output("No issues found".to_string());
                } else {
                    app.on_worker_output("completed".to_string());
                }
                let _ = app.on_worker_completed(true, 0);
            }
            JobRun::DeterministicTestRun => {
                if runner_failures_left > 0 {
                    runner_failures_left = runner_failures_left.saturating_sub(1);
                    app.on_worker_output("tests failing".to_string());
                    let _ = app.on_worker_completed(false, 1);
                } else {
                    app.on_worker_output("all passed".to_string());
                    let _ = app.on_worker_completed(true, 0);
                }
            }
        }
        persist_runtime_tasks_snapshot(&app, &store).expect("persist runtime snapshot");
    }

    let persisted = store.read_tasks().expect("read persisted tasks");
    let top_status = persisted
        .iter()
        .find(|entry| entry.id == "top")
        .map(|entry| entry.status);
    let final_status = persisted
        .iter()
        .find(|entry| entry.id == "final")
        .map(|entry| entry.status);
    assert_eq!(top_status, Some(PlannerTaskStatusFile::Done));
    assert_eq!(final_status, Some(PlannerTaskStatusFile::Done));
    assert!(app.drain_worker_failures().is_empty());

    let _ = std::fs::remove_dir_all(&session_dir);
}

#[test]
fn integration_snapshot_does_not_write_illegal_children_for_done_or_final_roots() {
    let mut app = App::default();
    app.sync_planner_tasks_from_file(vec![
        PlannerTaskFileEntry {
            id: "done-top".to_string(),
            title: "Done top".to_string(),
            details: "done details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Task,
            status: PlannerTaskStatusFile::Done,
            parent_id: None,
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "final".to_string(),
            title: "Final".to_string(),
            details: "final details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::FinalAudit,
            status: PlannerTaskStatusFile::Pending,
            parent_id: None,
            order: Some(1),
        },
    ])
    .expect("sync should succeed");

    let (store, session_dir) = open_temp_store("metaagent-integration-root-safety");
    app.start_execution();
    persist_runtime_tasks_snapshot(&app, &store).expect("persist runtime snapshot");
    let persisted = store.read_tasks().expect("read persisted tasks");

    assert!(
        !persisted
            .iter()
            .any(|entry| entry.parent_id.as_deref() == Some("done-top"))
    );
    assert!(
        !persisted
            .iter()
            .any(|entry| entry.parent_id.as_deref() == Some("final"))
    );

    let mut reloaded = App::default();
    reloaded
        .sync_planner_tasks_from_file(persisted)
        .expect("reload from persisted tasks should sync");
    reloaded.start_execution();
    let job = reloaded
        .start_next_worker_job()
        .expect("final audit should run");
    assert!(matches!(job.run, JobRun::AgentPrompt(_)));

    let _ = std::fs::remove_dir_all(&session_dir);
}

#[test]
fn integration_persisted_legacy_in_progress_impl_resumes_at_auditor() {
    let mut app = App::default();
    app.sync_planner_tasks_from_file(vec![
        PlannerTaskFileEntry {
            id: "top".to_string(),
            title: "Task".to_string(),
            details: "top details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Task,
            status: PlannerTaskStatusFile::Pending,
            parent_id: None,
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl".to_string(),
            title: "Implementation".to_string(),
            details: "impl details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Implementor,
            status: PlannerTaskStatusFile::InProgress,
            parent_id: Some("top".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl-audit".to_string(),
            title: "Audit".to_string(),
            details: "audit details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Auditor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("impl".to_string()),
            order: Some(0),
        },
    ])
    .expect("sync should succeed");

    let (store, session_dir) = open_temp_store("metaagent-integration-legacy-resume");
    persist_runtime_tasks_snapshot(&app, &store).expect("persist runtime snapshot");
    let persisted = store.read_tasks().expect("read persisted tasks");

    let mut reloaded = App::default();
    reloaded
        .sync_planner_tasks_from_file(persisted)
        .expect("reload from persisted tasks should sync");
    reloaded.start_execution();
    let job = reloaded
        .start_next_worker_job()
        .expect("auditor should be resumed");
    match job.run {
        JobRun::AgentPrompt(prompt) => {
            assert!(prompt.contains("reviewing implementation output"));
        }
        JobRun::DeterministicTestRun => panic!("expected auditor prompt"),
    }

    let _ = std::fs::remove_dir_all(&session_dir);
}

#[test]
fn global_right_scroll_moves_five_lines_per_event() {
    let mut app = App::default();
    scroll_right_down_global(&mut app, 100);
    assert_eq!(app.right_scroll(), 5);

    scroll_right_up_global(&mut app);
    assert_eq!(app.right_scroll(), 0);
}

#[test]
fn global_right_scroll_respects_bounds() {
    let mut app = App::default();
    for _ in 0..3 {
        app.scroll_right_down(100);
    }
    scroll_right_up_global(&mut app);
    assert_eq!(app.right_scroll(), 0);

    for _ in 0..98 {
        app.scroll_right_down(100);
    }
    scroll_right_down_global(&mut app, 100);
    assert_eq!(app.right_scroll(), 100);
}

#[test]
fn parses_silent_master_commands() {
    assert_eq!(
        parse_silent_master_command("/split-audits"),
        Some(SilentMasterCommand::SplitAudits)
    );
    assert_eq!(
        parse_silent_master_command("/merge-audits"),
        Some(SilentMasterCommand::MergeAudits)
    );
    assert_eq!(
        parse_silent_master_command("/split-tests"),
        Some(SilentMasterCommand::SplitTests)
    );
    assert_eq!(
        parse_silent_master_command("/merge-tests"),
        Some(SilentMasterCommand::MergeTests)
    );
    assert_eq!(parse_silent_master_command("/start"), None);
}

#[test]
fn build_resume_options_excludes_current_session_dir() {
    let current = std::path::Path::new("/tmp/current");
    let options = build_resume_options(
        vec![
            SessionListEntry {
                session_dir: std::path::PathBuf::from("/tmp/current"),
                workspace: "/work/current".to_string(),
                title: Some("Current".to_string()),
                created_at_label: Some("2026-02-16T10:00:00Z".to_string()),
                created_at_epoch_secs: 10,
                last_used_epoch_secs: 10,
            },
            SessionListEntry {
                session_dir: std::path::PathBuf::from("/tmp/other"),
                workspace: "/work/other".to_string(),
                title: Some("Other".to_string()),
                created_at_label: Some("2026-02-16T11:00:00Z".to_string()),
                created_at_epoch_secs: 9,
                last_used_epoch_secs: 9,
            },
        ],
        Some(current),
        None,
    );
    assert_eq!(options.len(), 1);
    assert_eq!(options[0].session_dir, "/tmp/other");
    assert_eq!(options[0].title.as_deref(), Some("Other"));
}

#[test]
fn build_resume_options_includes_all_sessions_without_active_session() {
    let options = build_resume_options(
        vec![SessionListEntry {
            session_dir: std::path::PathBuf::from("/tmp/only"),
            workspace: "/work/only".to_string(),
            title: Some("Only".to_string()),
            created_at_label: Some("2026-02-16T11:00:00Z".to_string()),
            created_at_epoch_secs: 1,
            last_used_epoch_secs: 1,
        }],
        None,
        None,
    );
    assert_eq!(options.len(), 1);
    assert_eq!(options[0].session_dir, "/tmp/only");
}

#[test]
fn build_resume_options_filters_to_current_workspace() {
    let workspace = std::path::Path::new("/work/current");
    let options = build_resume_options(
        vec![
            SessionListEntry {
                session_dir: std::path::PathBuf::from("/tmp/current-1"),
                workspace: "/work/current".to_string(),
                title: Some("Current".to_string()),
                created_at_label: Some("2026-02-16T10:00:00Z".to_string()),
                created_at_epoch_secs: 10,
                last_used_epoch_secs: 10,
            },
            SessionListEntry {
                session_dir: std::path::PathBuf::from("/tmp/other"),
                workspace: "/work/other".to_string(),
                title: Some("Other".to_string()),
                created_at_label: Some("2026-02-16T11:00:00Z".to_string()),
                created_at_epoch_secs: 9,
                last_used_epoch_secs: 9,
            },
        ],
        None,
        Some(workspace),
    );
    assert_eq!(options.len(), 1);
    assert_eq!(options[0].session_dir, "/tmp/current-1");
}

#[test]
fn sanitize_master_docs_fields_clears_docs_for_new_tasks() {
    let mut tasks = vec![PlannerTaskFileEntry {
        id: "new-1".to_string(),
        title: "Task".to_string(),
        details: "details".to_string(),
        docs: vec![session_store::PlannerTaskDocFileEntry {
            title: "Doc".to_string(),
            url: "https://example.com".to_string(),
            summary: "sum".to_string(),
        }],
        kind: session_store::PlannerTaskKindFile::Task,
        status: session_store::PlannerTaskStatusFile::Pending,
        parent_id: None,
        order: Some(0),
    }];

    let changed = sanitize_master_docs_fields(&mut tasks, Some("[]"));
    assert!(changed);
    assert!(tasks[0].docs.is_empty());
}

#[test]
fn sanitize_master_docs_fields_preserves_baseline_docs_for_existing_tasks() {
    let baseline = r#"[{
            "id":"t1",
            "title":"Task",
            "details":"d",
            "docs":[{"title":"Keep","url":"https://keep","summary":"s"}],
            "kind":"task",
            "status":"pending",
            "parent_id":null,
            "order":0
        }]"#;
    let mut tasks = vec![PlannerTaskFileEntry {
        id: "t1".to_string(),
        title: "Task".to_string(),
        details: "d".to_string(),
        docs: vec![session_store::PlannerTaskDocFileEntry {
            title: "Wrong".to_string(),
            url: "https://wrong".to_string(),
            summary: String::new(),
        }],
        kind: session_store::PlannerTaskKindFile::Task,
        status: session_store::PlannerTaskStatusFile::Pending,
        parent_id: None,
        order: Some(0),
    }];

    let changed = sanitize_master_docs_fields(&mut tasks, Some(baseline));
    assert!(changed);
    assert_eq!(tasks[0].docs.len(), 1);
    assert_eq!(tasks[0].docs[0].title, "Keep");
}

#[test]
fn sanitize_master_docs_fields_ignores_missing_baseline() {
    let mut tasks = vec![PlannerTaskFileEntry {
        id: "t1".to_string(),
        title: "Task".to_string(),
        details: "details".to_string(),
        docs: vec![session_store::PlannerTaskDocFileEntry {
            title: "Keep".to_string(),
            url: "https://keep".to_string(),
            summary: "sum".to_string(),
        }],
        kind: session_store::PlannerTaskKindFile::Task,
        status: session_store::PlannerTaskStatusFile::Pending,
        parent_id: None,
        order: Some(0),
    }];

    let changed = sanitize_master_docs_fields(&mut tasks, None);
    assert!(!changed);
    assert_eq!(tasks[0].docs.len(), 1);
    assert_eq!(tasks[0].docs[0].title, "Keep");
}

#[test]
fn docs_attach_blocks_session_switch_commands() {
    let mut app = App::default();
    let master_adapter = CodexAdapter::new();
    let master_report_adapter = CodexAdapter::new();
    let project_info_adapter = CodexAdapter::new();
    let docs_attach_adapter = CodexAdapter::new();
    let test_runner_adapter = TestRunnerAdapter::new();
    let model_routing = CodexAgentModelRouting::default();

    let mut worker_agent_adapters: HashMap<String, CodexAdapter> = HashMap::new();
    let mut active_worker_context_key = None;
    let mut session_store: Option<SessionStore> = None;
    let cwd = std::env::current_dir().expect("cwd");
    let mut pending_task_write_baseline = None;
    let mut docs_attach_in_flight = true;
    let mut master_session_intro_needed = true;
    let mut master_report_session_intro_needed = true;
    let mut pending_master_message_after_project_info = None;
    let mut project_info_in_flight = false;
    let mut project_info_stage = None;
    let mut project_info_text = None;
    let mut master_report_in_flight = false;
    let mut pending_master_report_prompts = std::collections::VecDeque::new();
    let mut master_report_transcript = Vec::new();
    let mut task_check_in_flight = false;
    let mut task_check_baseline = None;

    let backend = ratatui::backend::TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).expect("test terminal");

    submit_user_message(
        &mut app,
        "/newmaster".to_string(),
        &master_adapter,
        &master_report_adapter,
        &project_info_adapter,
        &mut worker_agent_adapters,
        &mut active_worker_context_key,
        &docs_attach_adapter,
        &test_runner_adapter,
        &mut master_report_in_flight,
        &mut pending_master_report_prompts,
        &mut master_report_transcript,
        &mut task_check_in_flight,
        &mut task_check_baseline,
        &mut session_store,
        &cwd,
        &mut terminal,
        &mut pending_task_write_baseline,
        &mut docs_attach_in_flight,
        &mut master_session_intro_needed,
        &mut master_report_session_intro_needed,
        &mut pending_master_message_after_project_info,
        &mut project_info_in_flight,
        &mut project_info_stage,
        &mut project_info_text,
        &model_routing,
    )
    .expect("newmaster command should not hard fail");
    assert!(
        app.left_bottom_lines()
            .last()
            .expect("status message")
            .contains("Documentation attach is still running")
    );

    submit_user_message(
        &mut app,
        "/resume".to_string(),
        &master_adapter,
        &master_report_adapter,
        &project_info_adapter,
        &mut worker_agent_adapters,
        &mut active_worker_context_key,
        &docs_attach_adapter,
        &test_runner_adapter,
        &mut master_report_in_flight,
        &mut pending_master_report_prompts,
        &mut master_report_transcript,
        &mut task_check_in_flight,
        &mut task_check_baseline,
        &mut session_store,
        &cwd,
        &mut terminal,
        &mut pending_task_write_baseline,
        &mut docs_attach_in_flight,
        &mut master_session_intro_needed,
        &mut master_report_session_intro_needed,
        &mut pending_master_message_after_project_info,
        &mut project_info_in_flight,
        &mut project_info_stage,
        &mut project_info_text,
        &model_routing,
    )
    .expect("resume command should not hard fail");
    assert!(
        app.left_bottom_lines()
            .last()
            .expect("status message")
            .contains("Documentation attach is still running")
    );
    assert!(!app.is_resume_picker_open());
}

#[test]
fn backend_command_opens_picker_and_closes_resume_picker() {
    let mut app = App::default();
    app.open_resume_picker(vec![ResumeSessionOption {
        session_dir: "/tmp/example".to_string(),
        workspace: "workspace".to_string(),
        title: Some("Session".to_string()),
        created_at_label: Some("now".to_string()),
        last_used_epoch_secs: 1,
    }]);
    assert!(app.is_resume_picker_open());

    let master_adapter = CodexAdapter::new();
    let master_report_adapter = CodexAdapter::new();
    let project_info_adapter = CodexAdapter::new();
    let docs_attach_adapter = CodexAdapter::new();
    let test_runner_adapter = TestRunnerAdapter::new();
    let model_routing = CodexAgentModelRouting::default();

    let mut worker_agent_adapters: HashMap<String, CodexAdapter> = HashMap::new();
    let mut active_worker_context_key = None;
    let mut session_store: Option<SessionStore> = None;
    let cwd = std::env::current_dir().expect("cwd");
    let mut pending_task_write_baseline = None;
    let mut docs_attach_in_flight = false;
    let mut master_session_intro_needed = true;
    let mut master_report_session_intro_needed = true;
    let mut pending_master_message_after_project_info = None;
    let mut project_info_in_flight = false;
    let mut project_info_stage = None;
    let mut project_info_text = None;
    let mut master_report_in_flight = false;
    let mut pending_master_report_prompts = std::collections::VecDeque::new();
    let mut master_report_transcript = Vec::new();
    let mut task_check_in_flight = false;
    let mut task_check_baseline = None;

    let backend = ratatui::backend::TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).expect("test terminal");

    submit_user_message(
        &mut app,
        "/backend".to_string(),
        &master_adapter,
        &master_report_adapter,
        &project_info_adapter,
        &mut worker_agent_adapters,
        &mut active_worker_context_key,
        &docs_attach_adapter,
        &test_runner_adapter,
        &mut master_report_in_flight,
        &mut pending_master_report_prompts,
        &mut master_report_transcript,
        &mut task_check_in_flight,
        &mut task_check_baseline,
        &mut session_store,
        &cwd,
        &mut terminal,
        &mut pending_task_write_baseline,
        &mut docs_attach_in_flight,
        &mut master_session_intro_needed,
        &mut master_report_session_intro_needed,
        &mut pending_master_message_after_project_info,
        &mut project_info_in_flight,
        &mut project_info_stage,
        &mut project_info_text,
        &model_routing,
    )
    .expect("backend command should not hard fail");

    assert!(!app.is_resume_picker_open());
    assert!(app.is_backend_picker_open());
    assert_eq!(app.backend_picker_options().len(), 2);
    assert!(
        app.left_bottom_lines()
            .last()
            .expect("status message")
            .contains("Select a backend in the picker")
    );
}

#[test]
fn apply_backend_selection_persists_and_reports_success() {
    with_temp_home("metaagent-backend-select-success", |_| {
        let mut app = App::default();
        let mut selected_backend = BackendKind::Codex;
        let mut model_routing = CodexAgentModelRouting::default();
        let mut master_adapter = build_json_persistent_adapter(
            &model_routing,
            selected_backend,
            CodexAgentKind::Master,
        );
        let mut master_report_adapter = build_json_persistent_adapter(
            &model_routing,
            selected_backend,
            CodexAgentKind::MasterReport,
        );
        let mut project_info_adapter = build_json_persistent_adapter(
            &model_routing,
            selected_backend,
            CodexAgentKind::ProjectInfo,
        );
        let mut docs_attach_adapter = build_plain_adapter(
            &model_routing,
            selected_backend,
            CodexAgentKind::DocsAttach,
            false,
        );
        let mut task_check_adapter = build_plain_adapter(
            &model_routing,
            selected_backend,
            CodexAgentKind::TaskCheck,
            false,
        );
        let mut active_worker_context_key = Some("top:42".to_string());
        let mut worker_agent_adapters: HashMap<String, CodexAdapter> =
            [("top:42".to_string(), CodexAdapter::new())]
                .into_iter()
                .collect();

        apply_backend_selection(
            &mut app,
            BackendOption {
                kind: BackendKind::Claude,
                label: "Claude",
                description: "Anthropic Claude backend",
            },
            &mut selected_backend,
            &mut model_routing,
            &mut active_worker_context_key,
            &mut worker_agent_adapters,
            &mut master_adapter,
            &mut master_report_adapter,
            &mut project_info_adapter,
            &mut docs_attach_adapter,
            &mut task_check_adapter,
        );

        assert_eq!(selected_backend, BackendKind::Claude);
        assert_eq!(
            model_routing.base_command_config().backend_kind(),
            BackendKind::Claude
        );
        let expected_program = model_routing.base_command_config().program;
        assert_eq!(master_adapter.program(), expected_program);
        assert_eq!(master_report_adapter.program(), expected_program);
        assert_eq!(project_info_adapter.program(), expected_program);
        assert_eq!(docs_attach_adapter.program(), expected_program);
        assert_eq!(task_check_adapter.program(), expected_program);
        assert!(active_worker_context_key.is_none());
        assert!(worker_agent_adapters.is_empty());
        let last = app.left_bottom_lines().last().expect("status message");
        assert!(last.contains("Backend set to Claude. Saved to"));
        assert!(last.contains("New adapters will use this backend."));
    });
}

#[test]
fn apply_backend_selection_persist_failure_keeps_in_memory_backend_and_reports_failure() {
    with_temp_home("metaagent-backend-select-failure", |home| {
        let config_dir = home.join(".metaagent");
        std::fs::create_dir_all(&config_dir).expect("create config dir");
        std::fs::create_dir(config_dir.join("config.toml")).expect("create invalid config path");

        let mut app = App::default();
        let mut selected_backend = BackendKind::Codex;
        let mut model_routing = CodexAgentModelRouting::default();
        let mut master_adapter = build_json_persistent_adapter(
            &model_routing,
            selected_backend,
            CodexAgentKind::Master,
        );
        let mut master_report_adapter = build_json_persistent_adapter(
            &model_routing,
            selected_backend,
            CodexAgentKind::MasterReport,
        );
        let mut project_info_adapter = build_json_persistent_adapter(
            &model_routing,
            selected_backend,
            CodexAgentKind::ProjectInfo,
        );
        let mut docs_attach_adapter = build_plain_adapter(
            &model_routing,
            selected_backend,
            CodexAgentKind::DocsAttach,
            false,
        );
        let mut task_check_adapter = build_plain_adapter(
            &model_routing,
            selected_backend,
            CodexAgentKind::TaskCheck,
            false,
        );
        let mut active_worker_context_key = Some("top:42".to_string());
        let mut worker_agent_adapters: HashMap<String, CodexAdapter> =
            [("top:42".to_string(), CodexAdapter::new())]
                .into_iter()
                .collect();

        apply_backend_selection(
            &mut app,
            BackendOption {
                kind: BackendKind::Claude,
                label: "Claude",
                description: "Anthropic Claude backend",
            },
            &mut selected_backend,
            &mut model_routing,
            &mut active_worker_context_key,
            &mut worker_agent_adapters,
            &mut master_adapter,
            &mut master_report_adapter,
            &mut project_info_adapter,
            &mut docs_attach_adapter,
            &mut task_check_adapter,
        );

        assert_eq!(selected_backend, BackendKind::Claude);
        assert_eq!(
            model_routing.base_command_config().backend_kind(),
            BackendKind::Claude
        );
        let expected_program = model_routing.base_command_config().program;
        assert_eq!(master_adapter.program(), expected_program);
        assert_eq!(master_report_adapter.program(), expected_program);
        assert_eq!(project_info_adapter.program(), expected_program);
        assert_eq!(docs_attach_adapter.program(), expected_program);
        assert_eq!(task_check_adapter.program(), expected_program);
        assert!(active_worker_context_key.is_none());
        assert!(worker_agent_adapters.is_empty());
        let last = app.left_bottom_lines().last().expect("status message");
        assert!(last.contains("Backend set to Claude for this run"));
        assert!(last.contains("persistence to ~/.metaagent/config.toml failed"));
        assert!(last.contains("New adapters in this run will still use this backend."));
    });
}

#[test]
fn failure_report_prompt_includes_entries_and_todo_question_for_tests() {
    let prompt = subagents::build_failure_report_prompt(
        "/tmp/session/task-fails.json",
        &[TaskFailFileEntry {
            kind: "test".to_string(),
            top_task_id: 7,
            top_task_title: "Add tests".to_string(),
            attempts: 5,
            reason: "tests kept failing".to_string(),
            action_taken: "removed failing tests".to_string(),
            created_at_epoch_secs: 123,
        }],
        true,
    );
    assert!(prompt.contains("task-fails.json"));
    assert!(prompt.contains("kind=test"));
    assert!(prompt.contains("Would they like these unresolved items written to TODO.md?"));
}

#[test]
fn slash_start_command_detection_only_matches_slash_form() {
    assert!(is_slash_start_command("/start"));
    assert!(is_slash_start_command("/run"));
    assert!(!is_slash_start_command("start execution"));
}

#[test]
fn formats_internal_master_update_with_standard_prefix() {
    assert_eq!(
        format_internal_master_update("completed implementor pass"),
        "Here's what just happened: completed implementor pass"
    );
    assert_eq!(
        format_internal_master_update("Here's what just happened: done"),
        "Here's what just happened: done"
    );
}

#[test]
fn prepends_master_session_intro_once() {
    let mut intro_needed = true;
    let first = subagents::build_session_intro_if_needed(
        "Do work",
        "/tmp/session-1",
        "/tmp/session-1/meta.json",
        Some("Project info"),
        &mut intro_needed,
    );
    assert!(first.contains("Meta-agent session working directory: /tmp/session-1"));
    assert!(first.contains("Session metadata file path: /tmp/session-1/meta.json"));
    assert!(first.contains("Hard guardrail:"));
    assert!(first.contains("Never modify project workspace files directly."));
    assert!(first.contains("Project context (project-info.md):"));
    assert!(first.contains("Project info"));
    assert!(first.contains("Do work"));
    let second = subagents::build_session_intro_if_needed(
        "Do more",
        "/tmp/session-1",
        "/tmp/session-1/meta.json",
        Some("Project info"),
        &mut intro_needed,
    );
    assert_eq!(second, "Do more");
}

#[test]
fn project_info_prompt_includes_question_and_output_path() {
    let prompt = subagents::build_project_info_prompt(
        "/tmp/workspace",
        "How should we implement task batching?",
        "/tmp/session/project-info.md",
    );
    assert!(prompt.contains("Current working directory: /tmp/workspace"));
    assert!(prompt.contains("How should we implement task batching?"));
    assert!(prompt.contains("path: /tmp/session/project-info.md"));
    assert!(prompt.contains("Inspect only local files in the repository"));
    assert!(prompt.contains("Do not browse the web"));
    assert!(prompt.contains("Do not propose implementation ideas"));
    assert!(prompt.contains("\"Language & Tech Stack\""));
    assert!(prompt.contains("\"File Structure\""));
    assert!(prompt.contains("\"Testing Setup\""));
    assert!(prompt.contains("best command to run the project's tests end-to-end"));
    assert!(prompt.contains("single verbatim shell command runnable in bash as-is"));
}

#[test]
fn convert_plan_prompt_references_planner_and_tasks_files() {
    let prompt =
        subagents::build_convert_plan_prompt("/tmp/session/planner.md", "/tmp/session/tasks.json");
    assert!(prompt.contains("now in task mode"));
    assert!(prompt.contains("/tmp/session/planner.md"));
    assert!(prompt.contains("/tmp/session/tasks.json"));
    assert!(prompt.contains("Convert the current plan into concrete task entries"));
}

#[test]
fn task_check_prompt_includes_tasks_path_and_guardrails() {
    let prompt = subagents::build_task_check_prompt(
        "/tmp/session/tasks.json",
        "/tmp/session/project-info.md",
        "/tmp/session/meta.json",
    );
    assert!(prompt.contains("/tmp/session/tasks.json"));
    assert!(prompt.contains("/tmp/session/project-info.md"));
    assert!(prompt.contains("/tmp/session/meta.json"));
    assert!(prompt.contains("edit this tasks.json directly to fix them"));
    assert!(prompt.contains("each test_writer must be a direct child of a top-level task"));
    assert!(prompt.contains("Enforce special-case sequencing for test bootstrapping"));
    assert!(prompt.contains("meta.json test_command is null/empty"));
    assert!(prompt.contains("dedicated testing-setup top-level task"));
    assert!(prompt.contains("PASS"));
    assert!(prompt.contains("FIXED"));
}

#[test]
fn split_tests_prompt_requires_flat_test_writer_structure() {
    let prompt = subagents::split_tests_command_prompt();
    assert!(prompt.contains("each test_writer must be a direct child of the top-level task"));
    assert!(prompt.contains("Do not create umbrella/nested test_writer parent groups"));
    assert!(prompt.contains("Ensure every test_writer has at least one direct test_runner child"));
}

#[test]
fn session_meta_prompt_includes_output_path_and_user_prompt() {
    let prompt = subagents::build_session_meta_prompt(
        "Build planner mode and task conversion.",
        "/tmp/session/meta.json",
    );
    assert!(prompt.contains("path: /tmp/session/meta.json"));
    assert!(prompt.contains("\"title\""));
    assert!(prompt.contains("\"created_at\""));
    assert!(prompt.contains("\"stack_description\""));
    assert!(prompt.contains("\"test_command\""));
    assert!(prompt.contains("language/technology stack"));
    assert!(prompt.contains("exact command string runnable in bash as-is"));
    assert!(prompt.contains("provide only the raw command string value"));
    assert!(prompt.contains("set test_command to JSON null"));
    assert!(prompt.contains("Build planner mode and task conversion."));
}

#[test]
fn normalize_test_command_trims_or_drops_empty_values() {
    assert_eq!(
        normalize_test_command(Some(" cargo test --all ".to_string())).as_deref(),
        Some("cargo test --all")
    );
    assert!(normalize_test_command(Some("   ".to_string())).is_none());
    assert!(normalize_test_command(None).is_none());
}

#[test]
fn ensure_final_audit_task_adds_or_resets_final_task() {
    let mut tasks = vec![PlannerTaskFileEntry {
        id: "1".to_string(),
        title: "Task".to_string(),
        details: "d".to_string(),
        docs: Vec::new(),
        kind: PlannerTaskKindFile::Task,
        status: PlannerTaskStatusFile::Pending,
        parent_id: None,
        order: Some(0),
    }];
    ensure_final_audit_task(&mut tasks);
    assert!(
        tasks
            .iter()
            .any(|t| t.kind == PlannerTaskKindFile::FinalAudit && t.parent_id.is_none())
    );

    let existing = tasks
        .iter_mut()
        .find(|t| t.kind == PlannerTaskKindFile::FinalAudit)
        .expect("final audit task");
    existing.status = PlannerTaskStatusFile::Done;
    ensure_final_audit_task(&mut tasks);
    let existing = tasks
        .iter()
        .find(|t| t.kind == PlannerTaskKindFile::FinalAudit)
        .expect("final audit task");
    assert!(matches!(existing.status, PlannerTaskStatusFile::Pending));
}

#[test]
fn normalize_root_orders_with_final_last_places_final_at_end() {
    let mut tasks = vec![
        PlannerTaskFileEntry {
            id: "f".to_string(),
            title: "Final".to_string(),
            details: "d".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::FinalAudit,
            status: PlannerTaskStatusFile::Pending,
            parent_id: None,
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "a".to_string(),
            title: "A".to_string(),
            details: "d".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Task,
            status: PlannerTaskStatusFile::Pending,
            parent_id: None,
            order: Some(1),
        },
    ];
    normalize_root_orders_with_final_last(&mut tasks);
    let final_order = tasks
        .iter()
        .find(|t| t.kind == PlannerTaskKindFile::FinalAudit)
        .and_then(|t| t.order)
        .expect("final order");
    let task_order = tasks
        .iter()
        .find(|t| t.kind == PlannerTaskKindFile::Task)
        .and_then(|t| t.order)
        .expect("task order");
    assert!(final_order > task_order);
}

#[test]
fn add_final_audit_aborts_without_writing_when_tasks_read_fails() {
    let (store, session_dir) = open_temp_store("final-audit-read-fail-add");
    std::fs::remove_file(store.tasks_file()).expect("remove tasks file");

    let mut app = App::default();
    let handled = handle_final_audit_tasks_command(&mut app, "/add-final-audit", &store)
        .expect("command should not error");
    assert!(handled);
    assert!(
        !store.tasks_file().exists(),
        "command should not recreate tasks file on read failure"
    );
    let last = app
        .left_bottom_lines()
        .last()
        .expect("expected user-visible error");
    assert!(last.contains("Could not read tasks file"));
    assert!(last.contains("final audit command aborted"));

    std::fs::remove_dir_all(session_dir).ok();
}

#[test]
fn remove_final_audit_does_not_overwrite_malformed_tasks_json() {
    let (store, session_dir) = open_temp_store("final-audit-read-fail-remove");
    std::fs::write(store.tasks_file(), "{ invalid json").expect("write malformed tasks.json");
    let before = std::fs::read_to_string(store.tasks_file()).expect("read malformed tasks.json");

    let mut app = App::default();
    let handled = handle_final_audit_tasks_command(&mut app, "/remove-final-audit", &store)
        .expect("command should not error");
    assert!(handled);

    let after = std::fs::read_to_string(store.tasks_file()).expect("read tasks after command");
    assert_eq!(
        after, before,
        "command should not rewrite tasks.json when parsing fails"
    );
    let last = app
        .left_bottom_lines()
        .last()
        .expect("expected user-visible error");
    assert!(last.contains("Could not read tasks file"));
    assert!(last.contains("final audit command aborted"));

    std::fs::remove_dir_all(session_dir).ok();
}

#[test]
fn remove_final_audit_reports_not_present_when_only_non_final_tasks_exist() {
    let (store, session_dir) = open_temp_store("final-audit-not-present-remove");
    let tasks = vec![PlannerTaskFileEntry {
        id: "task-1".to_string(),
        title: "Task".to_string(),
        details: "details".to_string(),
        docs: Vec::new(),
        kind: PlannerTaskKindFile::Task,
        status: PlannerTaskStatusFile::Pending,
        parent_id: None,
        order: Some(0),
    }];
    let tasks_json = serde_json::to_string_pretty(&tasks).expect("serialize tasks");
    std::fs::write(store.tasks_file(), tasks_json).expect("write tasks");

    let mut app = App::default();
    let handled = handle_final_audit_tasks_command(&mut app, "/remove-final-audit", &store)
        .expect("command should not error");
    assert!(handled);

    let last = app
        .left_bottom_lines()
        .last()
        .expect("expected status message");
    assert_eq!(last, "System: No final audit task was present.");

    std::fs::remove_dir_all(session_dir).ok();
}

#[test]
fn final_audit_commands_do_not_write_while_execution_busy() {
    let (store, session_dir) = open_temp_store("final-audit-block-while-running");
    let tasks = integration_plan_with_final();
    let tasks_json = serde_json::to_string_pretty(&tasks).expect("serialize tasks");
    std::fs::write(store.tasks_file(), tasks_json).expect("write tasks");

    let mut app = App::default();
    app.sync_planner_tasks_from_file(tasks.clone())
        .expect("sync tasks");
    app.start_execution();
    let _ = app
        .start_next_worker_job()
        .expect("active worker should exist");
    assert!(app.is_execution_busy());

    let before = std::fs::read_to_string(store.tasks_file()).expect("read tasks before");
    let handled = handle_final_audit_tasks_command(&mut app, "/add-final-audit", &store)
        .expect("command should not error");
    assert!(handled);
    let after = std::fs::read_to_string(store.tasks_file()).expect("read tasks after");
    assert_eq!(after, before, "tasks.json must not be mutated while busy");

    let last = app
        .left_bottom_lines()
        .last()
        .expect("expected status message");
    assert!(last.contains("Cannot modify final-audit tasks while worker execution is running"));

    std::fs::remove_dir_all(session_dir).ok();
}

#[cfg(unix)]
#[test]
fn add_final_audit_reports_write_failure_without_returning_error() {
    use std::os::unix::fs::PermissionsExt;

    let (store, session_dir) = open_temp_store("final-audit-add-write-fail");
    let tasks = vec![PlannerTaskFileEntry {
        id: "task-1".to_string(),
        title: "Task".to_string(),
        details: "details".to_string(),
        docs: Vec::new(),
        kind: PlannerTaskKindFile::Task,
        status: PlannerTaskStatusFile::Pending,
        parent_id: None,
        order: Some(0),
    }];
    let tasks_json = serde_json::to_string_pretty(&tasks).expect("serialize tasks");
    std::fs::write(store.tasks_file(), tasks_json).expect("write tasks");
    let before = std::fs::read_to_string(store.tasks_file()).expect("read tasks before");

    let mut perms = std::fs::metadata(store.tasks_file())
        .expect("metadata")
        .permissions();
    perms.set_mode(0o444);
    std::fs::set_permissions(store.tasks_file(), perms).expect("set readonly");

    let mut app = App::default();
    let handled = handle_final_audit_tasks_command(&mut app, "/add-final-audit", &store)
        .expect("command should not error");
    assert!(handled);

    let after = std::fs::read_to_string(store.tasks_file()).expect("read tasks after");
    assert_eq!(
        after, before,
        "tasks.json should stay unchanged when write fails"
    );
    let last = app
        .left_bottom_lines()
        .last()
        .expect("expected status message");
    assert!(last.contains("Failed to write tasks file while adding final audit task"));

    let mut reset_perms = std::fs::metadata(store.tasks_file())
        .expect("metadata")
        .permissions();
    reset_perms.set_mode(0o644);
    let _ = std::fs::set_permissions(store.tasks_file(), reset_perms);
    std::fs::remove_dir_all(session_dir).ok();
}

#[cfg(unix)]
#[test]
fn remove_final_audit_reports_write_failure_without_returning_error() {
    use std::os::unix::fs::PermissionsExt;

    let (store, session_dir) = open_temp_store("final-audit-remove-write-fail");
    let tasks = integration_plan_with_final();
    let tasks_json = serde_json::to_string_pretty(&tasks).expect("serialize tasks");
    std::fs::write(store.tasks_file(), tasks_json).expect("write tasks");
    let before = std::fs::read_to_string(store.tasks_file()).expect("read tasks before");

    let mut perms = std::fs::metadata(store.tasks_file())
        .expect("metadata")
        .permissions();
    perms.set_mode(0o444);
    std::fs::set_permissions(store.tasks_file(), perms).expect("set readonly");

    let mut app = App::default();
    let handled = handle_final_audit_tasks_command(&mut app, "/remove-final-audit", &store)
        .expect("command should not error");
    assert!(handled);

    let after = std::fs::read_to_string(store.tasks_file()).expect("read tasks after");
    assert_eq!(
        after, before,
        "tasks.json should stay unchanged when write fails"
    );
    let last = app
        .left_bottom_lines()
        .last()
        .expect("expected status message");
    assert!(last.contains("Failed to write tasks file while removing final audit task"));

    let mut reset_perms = std::fs::metadata(store.tasks_file())
        .expect("metadata")
        .permissions();
    reset_perms.set_mode(0o644);
    let _ = std::fs::set_permissions(store.tasks_file(), reset_perms);
    std::fs::remove_dir_all(session_dir).ok();
}
