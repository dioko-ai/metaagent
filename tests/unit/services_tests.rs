use super::*;

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::agent::{AdapterOutputMode, AgentEvent, BackendKind};
use crate::session_store::{
    PlannerTaskFileEntry, PlannerTaskKindFile, PlannerTaskStatusFile, SessionStore,
};
use crate::workflow::{JobRun, StartedJob, WorkerRole, WorkflowFailure, WorkflowFailureKind};

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

fn seed_simple_plan(app: &mut App) {
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
}

fn wait_for_runner_events(test_runner: &TestRunnerAdapter) -> Vec<AgentEvent> {
    let mut events = Vec::new();
    for _ in 0..80 {
        events.extend(test_runner.drain_events_limited(64));
        if events
            .iter()
            .any(|event| matches!(event, AgentEvent::Completed { .. }))
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    events
}

#[test]
fn claim_next_worker_job_persists_claimed_status_snapshot() {
    let service = DefaultCoreOrchestrationService;
    let mut app = App::default();
    seed_simple_plan(&mut app);
    app.start_execution();

    let (store, session_dir) = open_temp_store("metaagent-services-claim");
    let job = service
        .claim_next_worker_job_and_persist_snapshot(&mut app, &store)
        .expect("claim should succeed")
        .expect("first job should exist");

    assert_eq!(job.role, WorkerRole::Implementor);
    let persisted = store.read_tasks().expect("read persisted tasks");
    let impl_status = persisted
        .iter()
        .find(|entry| entry.id == "impl")
        .map(|entry| entry.status);
    assert_eq!(impl_status, Some(PlannerTaskStatusFile::InProgress));

    let _ = std::fs::remove_dir_all(&session_dir);
}

#[test]
fn start_next_worker_job_if_any_none_still_persists_snapshot() {
    let service = DefaultCoreOrchestrationService;
    let mut app = App::default();
    let (store, session_dir) = open_temp_store("metaagent-services-none");

    let started = service
        .start_next_worker_job_if_any(
            &mut app,
            &mut std::collections::HashMap::new(),
            &mut None,
            &TestRunnerAdapter::new(),
            &store,
            &CodexAgentModelRouting::default(),
        )
        .expect("start should succeed");

    assert!(started.is_none());
    assert_eq!(
        std::fs::read_to_string(store.tasks_file()).expect("tasks file should be writable"),
        "[]"
    );

    let _ = std::fs::remove_dir_all(&session_dir);
}

#[test]
fn dispatch_agent_prompt_preserves_prior_worker_session_id() {
    let service = DefaultCoreOrchestrationService;
    let (store, session_dir) = open_temp_store("metaagent-services-dispatch-agent");
    let mut adapters = std::collections::HashMap::new();
    let old_adapter = crate::agent::CodexAdapter::new_persistent();
    old_adapter.set_saved_session_id(Some("session-123".to_string()));
    adapters.insert("implementor:1".to_string(), old_adapter);

    let mut active_key = None;
    let test_runner = TestRunnerAdapter::new();
    let routing = CodexAgentModelRouting::default();
    let job = StartedJob {
        run: JobRun::AgentPrompt("implement this".to_string()),
        role: WorkerRole::Implementor,
        top_task_id: 1,
        parent_context_key: Some("implementor:1".to_string()),
    };

    service.dispatch_worker_job(
        &job,
        &mut adapters,
        &mut active_key,
        &test_runner,
        &store,
        &routing,
    );

    assert_eq!(active_key.as_deref(), Some("implementor:1"));
    let saved = adapters
        .get("implementor:1")
        .expect("adapter should exist")
        .saved_session_id();
    assert_eq!(saved.as_deref(), Some("session-123"));

    let _ = std::fs::remove_dir_all(&session_dir);
}

#[test]
fn dispatch_agent_prompt_keeps_existing_context_and_creates_new_context_for_future_dispatches() {
    let service = DefaultCoreOrchestrationService;
    let (store, session_dir) = open_temp_store("metaagent-services-dispatch-future");
    let mut adapters = std::collections::HashMap::new();
    let existing_adapter = crate::agent::CodexAdapter::new_persistent();
    existing_adapter.set_saved_session_id(Some("session-123".to_string()));
    adapters.insert("implementor:1".to_string(), existing_adapter);

    let mut active_key = None;
    let test_runner = TestRunnerAdapter::new();
    let mut routing = CodexAgentModelRouting::default();

    let first_job = StartedJob {
        run: JobRun::AgentPrompt("first".to_string()),
        role: WorkerRole::Implementor,
        top_task_id: 1,
        parent_context_key: Some("implementor:1".to_string()),
    };
    service.dispatch_worker_job(
        &first_job,
        &mut adapters,
        &mut active_key,
        &test_runner,
        &store,
        &routing,
    );
    assert_eq!(active_key.as_deref(), Some("implementor:1"));

    routing =
        CodexAgentModelRouting::from_toml_str("[backend]\nselected = \"claude\"\n").unwrap_or_default();
    let second_job = StartedJob {
        run: JobRun::AgentPrompt("second".to_string()),
        role: WorkerRole::Implementor,
        top_task_id: 2,
        parent_context_key: Some("implementor:2".to_string()),
    };
    service.dispatch_worker_job(
        &second_job,
        &mut adapters,
        &mut active_key,
        &test_runner,
        &store,
        &routing,
    );

    assert_eq!(active_key.as_deref(), Some("implementor:2"));
    assert_eq!(adapters.len(), 2);
    assert_eq!(
        adapters
            .get("implementor:1")
            .expect("original adapter should remain")
            .saved_session_id()
            .as_deref(),
        Some("session-123")
    );
    assert_eq!(
        adapters
            .get("implementor:2")
            .expect("new adapter should be created for new context")
            .saved_session_id(),
        None
    );

    let _ = std::fs::remove_dir_all(&session_dir);
}

#[test]
fn dispatch_agent_prompt_does_not_replace_in_flight_context_adapter_after_backend_change() {
    let service = DefaultCoreOrchestrationService;
    let (store, session_dir) = open_temp_store("metaagent-services-dispatch-in-flight");
    let mut adapters = std::collections::HashMap::new();
    let existing_adapter = crate::agent::CodexAdapter::new_persistent();
    existing_adapter.set_saved_session_id(Some("session-123".to_string()));
    adapters.insert("implementor:1".to_string(), existing_adapter);

    let mut active_key = None;
    let test_runner = TestRunnerAdapter::new();
    let routing =
        CodexAgentModelRouting::from_toml_str("[backend]\nselected = \"claude\"\n").unwrap_or_default();

    let job = StartedJob {
        run: JobRun::AgentPrompt("continue same context".to_string()),
        role: WorkerRole::Implementor,
        top_task_id: 1,
        parent_context_key: Some("implementor:1".to_string()),
    };
    service.dispatch_worker_job(
        &job,
        &mut adapters,
        &mut active_key,
        &test_runner,
        &store,
        &routing,
    );

    assert_eq!(active_key.as_deref(), Some("implementor:1"));
    assert_eq!(adapters.len(), 1);
    assert_eq!(
        adapters
            .get("implementor:1")
            .expect("existing adapter should still be used")
            .saved_session_id()
            .as_deref(),
        Some("session-123")
    );

    let _ = std::fs::remove_dir_all(&session_dir);
}

#[test]
fn build_worker_adapter_for_codex_keeps_plain_text_persistent_behavior() {
    let routing = CodexAgentModelRouting::default();
    let adapter = build_worker_adapter(&routing, WorkerRole::Implementor);
    let config = adapter.config_snapshot();

    assert_eq!(config.backend_kind(), BackendKind::Codex);
    assert_eq!(config.output_mode, AdapterOutputMode::PlainText);
    assert!(config.persistent_session);
    assert!(config.skip_reader_join_after_wait);
}

#[test]
fn build_worker_adapter_for_claude_uses_json_persistent_mode_for_resumption() {
    let routing =
        CodexAgentModelRouting::from_toml_str("[backend]\nselected = \"claude\"\n").unwrap_or_default();
    let adapter = build_worker_adapter(&routing, WorkerRole::Implementor);
    let config = adapter.config_snapshot();

    assert_eq!(config.backend_kind(), BackendKind::Claude);
    assert_eq!(config.output_mode, AdapterOutputMode::JsonAssistantOnly);
    assert!(config.persistent_session);
    assert!(config.skip_reader_join_after_wait);
}

#[test]
fn dispatch_deterministic_test_run_uses_trimmed_meta_test_command() {
    let service = DefaultCoreOrchestrationService;
    let (store, session_dir) = open_temp_store("metaagent-services-dispatch-test");
    std::fs::write(
        store.session_meta_file(),
        r#"{"title":"Session","created_at":"2026-02-18T00:00:00Z","stack_description":"Rust","test_command":"  printf SERVICE_LAYER_OK  "}"#,
    )
    .expect("write meta");

    let mut active_key = Some("placeholder".to_string());
    let test_runner = TestRunnerAdapter::new();
    let routing = CodexAgentModelRouting::default();
    let mut adapters = std::collections::HashMap::new();
    let job = StartedJob {
        run: JobRun::DeterministicTestRun,
        role: WorkerRole::TestRunner,
        top_task_id: 1,
        parent_context_key: Some("test_writer:1".to_string()),
    };

    service.dispatch_worker_job(
        &job,
        &mut adapters,
        &mut active_key,
        &test_runner,
        &store,
        &routing,
    );

    let events = wait_for_runner_events(&test_runner);
    assert!(events.iter().any(|event| {
        matches!(
            event,
            AgentEvent::Output(line) if line.contains("SERVICE_LAYER_OK")
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            event,
            AgentEvent::Completed {
                success: true,
                code: 0
            }
        )
    }));
    assert!(active_key.is_none());

    let _ = std::fs::remove_dir_all(&session_dir);
}

#[test]
fn build_exhausted_loop_failures_prompt_empty_is_noop() {
    let service = DefaultCoreOrchestrationService;
    let (store, session_dir) = open_temp_store("metaagent-services-failures-empty");
    let mut intro_needed = true;

    let prompt = service
        .build_exhausted_loop_failures_prompt(&store, &mut intro_needed, None, Vec::new())
        .expect("prompt generation should succeed");

    assert!(prompt.is_none());
    assert!(intro_needed);
    assert!(store.read_task_fails().expect("read task fails").is_empty());

    let _ = std::fs::remove_dir_all(&session_dir);
}

#[test]
fn build_exhausted_loop_failures_prompt_appends_entries_and_intro_once() {
    let service = DefaultCoreOrchestrationService;
    let (store, session_dir) = open_temp_store("metaagent-services-failures");
    let mut intro_needed = true;

    let first_prompt = service
        .build_exhausted_loop_failures_prompt(
            &store,
            &mut intro_needed,
            Some("Project context details"),
            vec![
                WorkflowFailure {
                    kind: WorkflowFailureKind::Audit,
                    top_task_id: 10,
                    top_task_title: "Audit branch".to_string(),
                    attempts: 4,
                    reason: "audit failed".to_string(),
                    action_taken: "stopped".to_string(),
                },
                WorkflowFailure {
                    kind: WorkflowFailureKind::Test,
                    top_task_id: 11,
                    top_task_title: "Tests branch".to_string(),
                    attempts: 5,
                    reason: "tests failed".to_string(),
                    action_taken: "stopped".to_string(),
                },
            ],
        )
        .expect("first prompt generation should succeed")
        .expect("first prompt should be present");

    assert!(first_prompt.contains("Meta-agent session working directory"));
    assert!(first_prompt.contains("Project context (project-info.md):"));
    assert!(first_prompt.contains("task-fails.json"));
    assert!(first_prompt.contains("Would they like these unresolved items written to TODO.md"));
    assert!(!intro_needed);

    let second_prompt = service
        .build_exhausted_loop_failures_prompt(
            &store,
            &mut intro_needed,
            Some("ignored once intro already sent"),
            vec![WorkflowFailure {
                kind: WorkflowFailureKind::Audit,
                top_task_id: 12,
                top_task_title: "Another audit branch".to_string(),
                attempts: 4,
                reason: "still failing".to_string(),
                action_taken: "stopped".to_string(),
            }],
        )
        .expect("second prompt generation should succeed")
        .expect("second prompt should be present");

    assert!(!second_prompt.contains("Meta-agent session working directory"));

    let fails = store.read_task_fails().expect("read task fails");
    assert_eq!(fails.len(), 3);
    assert_eq!(fails[0].kind, "audit");
    assert_eq!(fails[1].kind, "test");
    assert_eq!(fails[2].kind, "audit");
    assert!(fails.iter().all(|entry| entry.created_at_epoch_secs > 0));

    let _ = std::fs::remove_dir_all(&session_dir);
}

#[test]
fn capture_tasks_baseline_reads_tasks_json() {
    let service = DefaultCoreOrchestrationService;
    let (store, session_dir) = open_temp_store("metaagent-services-baseline");
    std::fs::write(store.tasks_file(), "[{\"id\":\"x\"}]\n").expect("write tasks");

    let baseline = service
        .capture_tasks_baseline(&store)
        .expect("baseline should exist");
    assert_eq!(baseline.tasks_json, "[{\"id\":\"x\"}]\n");

    let _ = std::fs::remove_dir_all(&session_dir);
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
