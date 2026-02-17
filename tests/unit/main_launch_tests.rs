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
fn parse_launch_options_rejects_unknown_arg() {
    let err = parse_launch_options(vec!["--weird".to_string()]).expect_err("should fail");
    assert!(err.to_string().contains("Unknown argument"));
}

#[test]
fn slash_commands_do_not_route_to_master() {
    assert!(!should_send_to_master("/start"));
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
    assert!(should_send_to_master("start execution"));
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
    assert_eq!(
        submit_block_reason(true, false, true, "/quit"),
        Some(SubmitBlockReason::ProjectInfoGathering)
    );
    assert_eq!(
        submit_block_reason(false, false, true, "hello"),
        Some(SubmitBlockReason::TaskCheck)
    );
    assert_eq!(
        submit_block_reason(false, true, false, "hello"),
        Some(SubmitBlockReason::MasterBusy)
    );
    assert_eq!(submit_block_reason(false, false, true, "/quit"), None);
    assert_eq!(submit_block_reason(false, false, false, "hello"), None);
}

#[test]
fn master_report_prompt_queue_serializes_dispatch() {
    let mut in_flight = false;
    let mut queue = std::collections::VecDeque::new();

    let first = enqueue_or_dispatch_master_report_prompt(
        "first".to_string(),
        &mut in_flight,
        &mut queue,
    );
    assert_eq!(first.as_deref(), Some("first"));
    assert!(in_flight);
    assert!(queue.is_empty());

    let second = enqueue_or_dispatch_master_report_prompt(
        "second".to_string(),
        &mut in_flight,
        &mut queue,
    );
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
fn master_completion_skips_task_file_processing_while_execution_is_enabled() {
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
    );
    assert_eq!(options.len(), 1);
    assert_eq!(options[0].session_dir, "/tmp/only");
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
