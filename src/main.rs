use std::collections::{HashMap, VecDeque};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crossterm::cursor::SetCursorStyle;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::prelude::*;

mod agent;
mod agent_models;
mod app;
mod default_config;
mod deterministic;
mod events;
mod session_store;
mod subagents;
mod text_layout;
mod theme;
mod ui;
mod workflow;

use agent::{AdapterOutputMode, AgentEvent, CodexAdapter, CodexCommandConfig};
use agent_models::{CodexAgentKind, CodexAgentModelRouting, CodexModelProfile};
use app::{App, Pane, ResumeSessionOption, RightPaneMode};
use deterministic::TestRunnerAdapter;
use events::AppEvent;
use session_store::{
    PlannerTaskFileEntry, PlannerTaskKindFile, PlannerTaskStatusFile, SessionListEntry,
    SessionStore, TaskFailFileEntry,
};
use theme::Theme;
use workflow::{JobRun, StartedJob, WorkerRole, WorkflowFailure, WorkflowFailureKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProjectInfoStage {
    GatheringInfo,
    WritingSessionMeta,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SilentMasterCommand {
    SplitAudits,
    MergeAudits,
    SplitTests,
    MergeTests,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubmitBlockReason {
    ProjectInfoGathering,
    MasterBusy,
    TaskCheck,
    ExecutionBusy,
}

#[derive(Debug, Clone)]
struct PendingTaskWriteBaseline {
    tasks_json: String,
}

const GLOBAL_RIGHT_SCROLL_LINES: u16 = 5;
const MAX_ADAPTER_EVENTS_PER_LOOP: usize = 128;

fn apply_codex_profile(config: &mut CodexCommandConfig, profile: &CodexModelProfile) {
    config.model = Some(profile.model.clone());
    config.model_reasoning_effort = profile.thinking_effort.clone();
}

fn build_codex_adapter(
    output_mode: AdapterOutputMode,
    persistent_session: bool,
    profile: &CodexModelProfile,
) -> CodexAdapter {
    let mut config = CodexCommandConfig::default();
    if matches!(output_mode, AdapterOutputMode::JsonAssistantOnly) {
        config.args_prefix.push("--json".to_string());
    }
    config.output_mode = output_mode;
    config.persistent_session = persistent_session;
    apply_codex_profile(&mut config, profile);
    CodexAdapter::with_config(config)
}

fn build_json_persistent_adapter(
    model_routing: &CodexAgentModelRouting,
    kind: CodexAgentKind,
) -> CodexAdapter {
    let profile = model_routing.profile_for(kind);
    build_codex_adapter(AdapterOutputMode::JsonAssistantOnly, true, &profile)
}

fn build_plain_adapter(
    model_routing: &CodexAgentModelRouting,
    kind: CodexAgentKind,
    persistent_session: bool,
) -> CodexAdapter {
    let profile = model_routing.profile_for(kind);
    build_codex_adapter(AdapterOutputMode::PlainText, persistent_session, &profile)
}

fn worker_role_agent_kind(role: WorkerRole) -> CodexAgentKind {
    match role {
        WorkerRole::Implementor => CodexAgentKind::WorkerImplementor,
        WorkerRole::Auditor => CodexAgentKind::WorkerAuditor,
        WorkerRole::TestWriter => CodexAgentKind::WorkerTestWriter,
        WorkerRole::FinalAudit => CodexAgentKind::WorkerFinalAudit,
        WorkerRole::TestRunner => CodexAgentKind::WorkerTestWriter,
    }
}

fn build_worker_adapter(model_routing: &CodexAgentModelRouting, role: WorkerRole) -> CodexAdapter {
    build_plain_adapter(model_routing, worker_role_agent_kind(role), true)
}

fn main() -> io::Result<()> {
    let launch_options = parse_launch_options(std::env::args().skip(1))?;
    let startup_message = if let Some(path) = launch_options.send_file {
        Some(std::fs::read_to_string(path)?)
    } else {
        None
    };

    enable_raw_mode()?;

    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableMouseCapture,
        SetCursorStyle::SteadyBar
    )?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;
    let theme = Theme::load_or_default("theme.toml");
    let cwd = std::env::current_dir()?;
    let result = run_app(
        &mut terminal,
        App::default(),
        &theme,
        cwd,
        startup_message.as_deref(),
    );

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        SetCursorStyle::DefaultUserShape,
        DisableMouseCapture,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;

    result
}

fn dispatch_worker_job(
    job: &workflow::StartedJob,
    worker_agent_adapters: &mut HashMap<String, CodexAdapter>,
    active_worker_context_key: &mut Option<String>,
    test_runner_adapter: &TestRunnerAdapter,
    session_store: &SessionStore,
    model_routing: &CodexAgentModelRouting,
) {
    match &job.run {
        JobRun::AgentPrompt(prompt) => {
            let key = job
                .parent_context_key
                .clone()
                .unwrap_or_else(|| format!("top:{}", job.top_task_id));
            // Replace per-branch adapters at each dispatch so late/stale output from any prior run
            // (for example, from descendant processes that keep pipes open) cannot bleed into the
            // next job's event stream. Preserve the branch's saved Codex session id so contextual
            // continuity is retained across retries/audits within the same branch.
            let prior_session_id = worker_agent_adapters
                .remove(&key)
                .and_then(|adapter| adapter.saved_session_id());
            let adapter = build_worker_adapter(model_routing, job.role);
            adapter.set_saved_session_id(prior_session_id);
            worker_agent_adapters.insert(key.clone(), adapter);
            let adapter = worker_agent_adapters
                .get(&key)
                .expect("worker adapter should be present after insertion");
            adapter.send_prompt(prompt.clone());
            *active_worker_context_key = Some(key);
        }
        JobRun::DeterministicTestRun => {
            *active_worker_context_key = None;
            let test_command = session_test_command(session_store);
            test_runner_adapter.run_tests_with_command(test_command.as_deref());
        }
    }
}

fn start_next_worker_job_if_any(
    app: &mut App,
    worker_agent_adapters: &mut HashMap<String, CodexAdapter>,
    active_worker_context_key: &mut Option<String>,
    test_runner_adapter: &TestRunnerAdapter,
    session_store: &SessionStore,
    model_routing: &CodexAgentModelRouting,
) {
    if let Some(job) = claim_next_worker_job_and_persist_snapshot(app, session_store) {
        dispatch_worker_job(
            &job,
            worker_agent_adapters,
            active_worker_context_key,
            test_runner_adapter,
            session_store,
            model_routing,
        );
        app.push_agent_message(format!(
            "System: Starting {:?} for task #{}.",
            job.role, job.top_task_id
        ));
    }
}

fn claim_next_worker_job_and_persist_snapshot(
    app: &mut App,
    session_store: &SessionStore,
) -> Option<StartedJob> {
    let job = app.start_next_worker_job();
    if let Err(err) = persist_runtime_tasks_snapshot(app, session_store) {
        app.push_agent_message(format!(
            "System: Failed to persist runtime task status to tasks.json: {err}"
        ));
    }
    job
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    mut app: App,
    theme: &Theme,
    cwd: PathBuf,
    startup_message: Option<&str>,
) -> io::Result<()> {
    let mut session_store: Option<SessionStore> = None;
    let model_routing = match CodexAgentModelRouting::load_from_metaagent_config() {
        Ok(config) => config,
        Err(err) => {
            app.push_agent_message(format!(
                "System: Failed to load model profile config from ~/.metaagent/config.toml: {err}. Using defaults."
            ));
            CodexAgentModelRouting::default()
        }
    };
    let master_adapter = build_json_persistent_adapter(&model_routing, CodexAgentKind::Master);
    let master_report_adapter =
        build_json_persistent_adapter(&model_routing, CodexAgentKind::MasterReport);
    let project_info_adapter =
        build_json_persistent_adapter(&model_routing, CodexAgentKind::ProjectInfo);
    let mut worker_agent_adapters: HashMap<String, CodexAdapter> = HashMap::new();
    let mut active_worker_context_key: Option<String> = None;
    let docs_attach_adapter =
        build_plain_adapter(&model_routing, CodexAgentKind::DocsAttach, false);
    let task_check_adapter = build_plain_adapter(&model_routing, CodexAgentKind::TaskCheck, false);
    let test_runner_adapter = TestRunnerAdapter::new();
    let mut master_transcript: Vec<String> = Vec::new();
    let mut master_report_transcript: Vec<String> = Vec::new();
    let mut master_report_in_flight = false;
    let mut pending_master_report_prompts: VecDeque<String> = VecDeque::new();
    let mut project_info_transcript: Vec<String> = Vec::new();
    let mut pending_task_write_baseline: Option<PendingTaskWriteBaseline> = None;
    let mut task_file_fix_retry_count: u8 = 0;
    let mut docs_attach_in_flight = false;
    let mut task_check_in_flight = false;
    let mut task_check_baseline: Option<String> = None;
    let mut master_session_intro_needed = true;
    let mut master_report_session_intro_needed = true;
    let mut pending_master_message_after_project_info: Option<String> = None;
    let mut project_info_in_flight = false;
    let mut project_info_stage: Option<ProjectInfoStage> = None;
    let mut project_info_text: Option<String> = None;
    app.push_agent_message("Agent: What can I help you build?".to_string());

    if let Some(message) = startup_message
        && let Some(message) = app.submit_direct_message(message)
    {
        submit_user_message(
            &mut app,
            message,
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
            terminal,
            &mut pending_task_write_baseline,
            &mut docs_attach_in_flight,
            &mut master_session_intro_needed,
            &mut master_report_session_intro_needed,
            &mut pending_master_message_after_project_info,
            &mut project_info_in_flight,
            &mut project_info_stage,
            &mut project_info_text,
            &model_routing,
        )?;
    }

    while app.running {
        let mut chat_updated = false;
        for event in master_adapter.drain_events_limited(MAX_ADAPTER_EVENTS_PER_LOOP) {
            match event {
                AgentEvent::Output(line) => {
                    master_transcript.push(line.clone());
                    app.push_agent_message(format!("Agent: {line}"));
                    chat_updated = true;
                }
                AgentEvent::System(line) => {
                    app.push_agent_message(format!("System: {line}"));
                    chat_updated = true;
                }
                AgentEvent::Completed { .. } => {
                    let Some(active_session) = session_store.as_ref() else {
                        app.set_master_in_progress(false);
                        master_transcript.clear();
                        chat_updated = true;
                        continue;
                    };
                    app.set_master_in_progress(false);
                    let _transcript = master_transcript.join("\n");
                    master_transcript.clear();
                    let baseline_tasks_text = pending_task_write_baseline
                        .as_ref()
                        .map(|b| b.tasks_json.clone());
                    let mut tasks_refresh_ok = false;
                    let mut requested_task_file_retry = false;
                    if should_process_master_task_file_updates(app.is_execution_busy()) {
                        match active_session.read_tasks() {
                            Ok(mut tasks) => {
                                let docs_sanitized = sanitize_master_docs_fields(
                                    &mut tasks,
                                    pending_task_write_baseline
                                        .as_ref()
                                        .map(|b| b.tasks_json.as_str()),
                                );
                                if docs_sanitized {
                                    match serde_json::to_string_pretty(&tasks) {
                                        Ok(text) => {
                                            if let Err(err) =
                                                std::fs::write(active_session.tasks_file(), text)
                                            {
                                                app.push_agent_message(format!(
                                                    "System: Failed to enforce docs policy in tasks.json: {err}"
                                                ));
                                            } else {
                                                app.push_agent_message(
                                                    "System: Removed master-written docs entries; use /attach-docs to populate docs."
                                                        .to_string(),
                                                );
                                            }
                                        }
                                        Err(err) => app.push_agent_message(format!(
                                            "System: Failed to serialize tasks.json after docs sanitization: {err}"
                                        )),
                                    }
                                }
                                match app.sync_planner_tasks_from_file(tasks) {
                                    Ok(()) => {
                                        tasks_refresh_ok = true;
                                        task_file_fix_retry_count = 0;
                                    }
                                    Err(err) => {
                                        app.push_agent_message(format!(
                                            "System: Failed to refresh task tree from tasks.json: {err}"
                                        ));
                                    }
                                }
                            }
                            Err(err) => {
                                app.push_agent_message(format!(
                                    "System: Failed to read tasks.json after master update: {err}"
                                ));
                            }
                        }
                        if let Ok(markdown) = active_session.read_planner_markdown() {
                            app.set_planner_markdown(markdown);
                        }

                        if !tasks_refresh_ok {
                            if task_file_fix_retry_count < 2 {
                                task_file_fix_retry_count =
                                    task_file_fix_retry_count.saturating_add(1);
                                requested_task_file_retry = true;
                                master_adapter.send_prompt(
                                    subagents::build_session_intro_if_needed(
                                        "tasks.json failed to parse/validate. Fix tasks.json immediately and retry. \
                                     Ensure id and parent_id are valid values and hierarchy is valid. \
                                     Do not ask the user to start execution yet.",
                                        active_session
                                            .session_dir()
                                            .display()
                                            .to_string()
                                            .as_str(),
                                        &active_session.session_meta_file().display().to_string(),
                                        project_info_text.as_deref(),
                                        &mut master_session_intro_needed,
                                    ),
                                );
                                app.set_master_in_progress(true);
                                app.push_agent_message(format!(
                                    "System: Requested tasks.json correction from master (attempt {}).",
                                    task_file_fix_retry_count
                                ));
                            } else {
                                app.push_agent_message(
                                    "System: tasks.json correction retries exceeded. Waiting for next user input."
                                        .to_string(),
                                );
                                task_file_fix_retry_count = 0;
                            }
                        }
                    }
                    let changed_tasks = if tasks_refresh_ok {
                        tasks_changed_since_baseline(
                            baseline_tasks_text.as_deref(),
                            std::fs::read_to_string(active_session.tasks_file())
                                .ok()
                                .as_deref(),
                        )
                    } else {
                        false
                    };
                    if should_clear_task_write_baseline(tasks_refresh_ok, requested_task_file_retry)
                    {
                        pending_task_write_baseline = None;
                    }

                    if tasks_refresh_ok {
                        if should_start_task_check(
                            changed_tasks,
                            task_check_in_flight,
                            docs_attach_in_flight,
                        ) {
                            task_check_in_flight = true;
                            task_check_baseline =
                                std::fs::read_to_string(active_session.tasks_file()).ok();
                            app.set_task_check_in_progress(true);
                            app.push_subagent_output(
                                "TaskCheckSystem: Checking updated tasks.json".to_string(),
                            );
                            task_check_adapter.send_prompt(subagents::build_task_check_prompt(
                                &active_session.tasks_file().display().to_string(),
                                &active_session.project_info_file().display().to_string(),
                                &active_session.session_meta_file().display().to_string(),
                            ));
                        }
                        start_next_worker_job_if_any(
                            &mut app,
                            &mut worker_agent_adapters,
                            &mut active_worker_context_key,
                            &test_runner_adapter,
                            active_session,
                            &model_routing,
                        );
                    }
                    chat_updated = true;
                }
            }
        }

        let worker_events = active_worker_context_key
            .as_ref()
            .and_then(|key| worker_agent_adapters.get(key))
            .map(|adapter| adapter.drain_events_limited(MAX_ADAPTER_EVENTS_PER_LOOP))
            .unwrap_or_default();
        for event in worker_events {
            match event {
                AgentEvent::Output(line) => {
                    app.on_worker_output(line);
                    chat_updated = true;
                }
                AgentEvent::System(line) => {
                    app.on_worker_system_output(line);
                    chat_updated = true;
                }
                AgentEvent::Completed { success, code } => {
                    let Some(active_session) = session_store.as_ref() else {
                        active_worker_context_key = None;
                        let _ = app.on_worker_completed(success, code);
                        chat_updated = true;
                        continue;
                    };
                    if let Some(key) = active_worker_context_key.as_ref()
                        && let Some(adapter) = worker_agent_adapters.get(key)
                    {
                        for tail_event in drain_post_completion_worker_events(adapter) {
                            match tail_event {
                                AgentEvent::Output(line) => {
                                    app.on_worker_output(line);
                                }
                                AgentEvent::System(line) => {
                                    app.on_worker_system_output(line);
                                }
                                AgentEvent::Completed { .. } => {}
                            }
                        }
                    }
                    active_worker_context_key = None;
                    let new_context_entries = app.on_worker_completed(success, code);
                    let exhausted_failures = app.drain_worker_failures();
                    if !exhausted_failures.is_empty() {
                        if let Some(prompt) = handle_exhausted_loop_failures(
                            &mut app,
                            active_session,
                            &mut master_report_session_intro_needed,
                            project_info_text.as_deref(),
                            exhausted_failures,
                        ) && let Some(prompt_to_send) = enqueue_or_dispatch_master_report_prompt(
                            prompt,
                            &mut master_report_in_flight,
                            &mut pending_master_report_prompts,
                        ) {
                            master_report_transcript.clear();
                            master_report_adapter.send_prompt(prompt_to_send);
                        }
                    }
                    if !new_context_entries.is_empty() {
                        if let Err(err) =
                            active_session.write_rolling_context(&app.rolling_context_entries())
                        {
                            app.push_agent_message(format!(
                                "System: Failed to persist rolling_context.json: {err}"
                            ));
                        }
                        let prompt = app.prepare_context_report_prompt(&new_context_entries);
                        let prompt = subagents::build_session_intro_if_needed(
                            &prompt,
                            active_session.session_dir().display().to_string().as_str(),
                            &active_session.session_meta_file().display().to_string(),
                            project_info_text.as_deref(),
                            &mut master_report_session_intro_needed,
                        );
                        if let Some(prompt_to_send) = enqueue_or_dispatch_master_report_prompt(
                            prompt,
                            &mut master_report_in_flight,
                            &mut pending_master_report_prompts,
                        ) {
                            master_report_transcript.clear();
                            master_report_adapter.send_prompt(prompt_to_send);
                        }
                    }
                    start_next_worker_job_if_any(
                        &mut app,
                        &mut worker_agent_adapters,
                        &mut active_worker_context_key,
                        &test_runner_adapter,
                        active_session,
                        &model_routing,
                    );
                    chat_updated = true;
                }
            }
        }

        for event in test_runner_adapter.drain_events_limited(MAX_ADAPTER_EVENTS_PER_LOOP) {
            match event {
                AgentEvent::Output(line) => {
                    app.on_worker_output(line);
                    chat_updated = true;
                }
                AgentEvent::System(line) => {
                    app.on_worker_system_output(line);
                    chat_updated = true;
                }
                AgentEvent::Completed { success, code } => {
                    let Some(active_session) = session_store.as_ref() else {
                        let _ = app.on_worker_completed(success, code);
                        chat_updated = true;
                        continue;
                    };
                    let new_context_entries = app.on_worker_completed(success, code);
                    let exhausted_failures = app.drain_worker_failures();
                    if !exhausted_failures.is_empty() {
                        if let Some(prompt) = handle_exhausted_loop_failures(
                            &mut app,
                            active_session,
                            &mut master_report_session_intro_needed,
                            project_info_text.as_deref(),
                            exhausted_failures,
                        ) && let Some(prompt_to_send) = enqueue_or_dispatch_master_report_prompt(
                            prompt,
                            &mut master_report_in_flight,
                            &mut pending_master_report_prompts,
                        ) {
                            master_report_transcript.clear();
                            master_report_adapter.send_prompt(prompt_to_send);
                        }
                    }
                    if !new_context_entries.is_empty() {
                        if let Err(err) =
                            active_session.write_rolling_context(&app.rolling_context_entries())
                        {
                            app.push_agent_message(format!(
                                "System: Failed to persist rolling_context.json: {err}"
                            ));
                        }
                        let prompt = app.prepare_context_report_prompt(&new_context_entries);
                        let prompt = subagents::build_session_intro_if_needed(
                            &prompt,
                            active_session.session_dir().display().to_string().as_str(),
                            &active_session.session_meta_file().display().to_string(),
                            project_info_text.as_deref(),
                            &mut master_report_session_intro_needed,
                        );
                        if let Some(prompt_to_send) = enqueue_or_dispatch_master_report_prompt(
                            prompt,
                            &mut master_report_in_flight,
                            &mut pending_master_report_prompts,
                        ) {
                            master_report_transcript.clear();
                            master_report_adapter.send_prompt(prompt_to_send);
                        }
                    }
                    start_next_worker_job_if_any(
                        &mut app,
                        &mut worker_agent_adapters,
                        &mut active_worker_context_key,
                        &test_runner_adapter,
                        active_session,
                        &model_routing,
                    );
                    chat_updated = true;
                }
            }
        }

        for event in master_report_adapter.drain_events_limited(MAX_ADAPTER_EVENTS_PER_LOOP) {
            match event {
                AgentEvent::Output(line) => {
                    master_report_transcript.push(line);
                }
                AgentEvent::System(_line) => {}
                AgentEvent::Completed { .. } => {
                    let summary = master_report_transcript
                        .iter()
                        .rev()
                        .find(|line| !line.trim().is_empty())
                        .map(|line| format_internal_master_update(line))
                        .unwrap_or_else(|| {
                            "Here's what just happened: a sub-agent completed work.".to_string()
                        });
                    app.push_agent_message(format!("Agent: {summary}"));
                    master_report_transcript.clear();
                    if let Some(prompt_to_send) = complete_and_next_master_report_prompt(
                        &mut master_report_in_flight,
                        &mut pending_master_report_prompts,
                    ) {
                        master_report_transcript.clear();
                        master_report_adapter.send_prompt(prompt_to_send);
                    }
                    chat_updated = true;
                }
            }
        }

        for event in project_info_adapter.drain_events_limited(MAX_ADAPTER_EVENTS_PER_LOOP) {
            match event {
                AgentEvent::Output(line) => {
                    project_info_transcript.push(line.clone());
                    app.push_subagent_output(format!("ProjectInfo: {line}"));
                    chat_updated = true;
                }
                AgentEvent::System(line) => {
                    app.push_subagent_output(format!("ProjectInfoSystem: {line}"));
                    chat_updated = true;
                }
                AgentEvent::Completed { success, code } => {
                    let Some(active_session) = session_store.as_ref() else {
                        project_info_stage = None;
                        project_info_in_flight = false;
                        project_info_transcript.clear();
                        chat_updated = true;
                        continue;
                    };
                    match project_info_stage {
                        Some(ProjectInfoStage::GatheringInfo) => {
                            if success {
                                let gathered = match active_session.read_project_info() {
                                    Ok(file_text) if !file_text.trim().is_empty() => {
                                        Some(file_text)
                                    }
                                    Ok(_) => None,
                                    Err(err) => {
                                        app.push_agent_message(format!(
                                            "System: Project info run succeeded but reading project-info.md failed: {err}"
                                        ));
                                        None
                                    }
                                };
                                if let Some(markdown) = gathered {
                                    project_info_text = Some(markdown);
                                    app.push_agent_message(
                                        "System: Project context gathered and attached for this session."
                                            .to_string(),
                                    );
                                } else if !project_info_transcript.is_empty() {
                                    let markdown = project_info_transcript.join("\n");
                                    if let Err(err) = active_session.write_project_info(&markdown) {
                                        app.push_agent_message(format!(
                                            "System: Failed to persist project-info.md fallback output: {err}"
                                        ));
                                    } else {
                                        project_info_text = Some(markdown);
                                        app.push_agent_message(
                                            "System: Project context gathered and attached for this session."
                                                .to_string(),
                                        );
                                    }
                                } else {
                                    app.push_agent_message(
                                        "System: Project context run returned no content; proceeding without attachment."
                                            .to_string(),
                                    );
                                }

                                if let Some(original_prompt) =
                                    pending_master_message_after_project_info.as_deref()
                                {
                                    let meta_prompt = subagents::build_session_meta_prompt(
                                        original_prompt,
                                        &active_session.session_meta_file().display().to_string(),
                                    );
                                    project_info_adapter.send_prompt(meta_prompt);
                                    project_info_stage = Some(ProjectInfoStage::WritingSessionMeta);
                                    app.push_subagent_output(
                                        "ProjectInfoSystem: Writing session meta.json".to_string(),
                                    );
                                    project_info_transcript.clear();
                                    chat_updated = true;
                                    continue;
                                }
                            } else {
                                app.push_agent_message(format!(
                                    "System: Project context gather exited with code {code}; proceeding without attachment."
                                ));
                            }
                        }
                        Some(ProjectInfoStage::WritingSessionMeta) => {
                            if success {
                                if let Ok(meta) = active_session.read_session_meta() {
                                    app.push_agent_message(format!(
                                        "System: Session metadata saved: \"{}\" ({})",
                                        meta.title, meta.created_at
                                    ));
                                } else {
                                    app.push_agent_message(
                                        "System: Session metadata write completed.".to_string(),
                                    );
                                }
                            } else {
                                app.push_agent_message(format!(
                                    "System: Session metadata write exited with code {code}; continuing."
                                ));
                            }
                        }
                        None => {}
                    }

                    project_info_stage = None;
                    project_info_in_flight = false;

                    if let Some(pending_message) = pending_master_message_after_project_info.take()
                    {
                        let master_prompt = if app.is_planner_mode() {
                            app.prepare_planner_prompt(
                                &pending_message,
                                &active_session.planner_file().display().to_string(),
                                &active_session.project_info_file().display().to_string(),
                            )
                        } else {
                            app.prepare_master_prompt(
                                &pending_message,
                                &active_session.tasks_file().display().to_string(),
                            )
                        };
                        let with_intro = subagents::build_session_intro_if_needed(
                            &master_prompt,
                            active_session.session_dir().display().to_string().as_str(),
                            &active_session.session_meta_file().display().to_string(),
                            project_info_text.as_deref(),
                            &mut master_session_intro_needed,
                        );
                        master_adapter.send_prompt(with_intro);
                        app.set_master_in_progress(true);
                        pending_task_write_baseline = capture_tasks_baseline(active_session);
                    }

                    project_info_transcript.clear();
                    chat_updated = true;
                }
            }
        }

        for event in docs_attach_adapter.drain_events_limited(MAX_ADAPTER_EVENTS_PER_LOOP) {
            match event {
                AgentEvent::Output(line) => {
                    app.push_subagent_output(format!("Docs: {line}"));
                    chat_updated = true;
                }
                AgentEvent::System(line) => {
                    app.push_subagent_output(format!("DocsSystem: {line}"));
                    chat_updated = true;
                }
                AgentEvent::Completed { success, code } => {
                    docs_attach_in_flight = false;
                    app.set_docs_attach_in_progress(false);
                    let Some(active_session) = session_store.as_ref() else {
                        chat_updated = true;
                        continue;
                    };
                    match active_session.read_tasks() {
                        Ok(tasks) => match app.sync_planner_tasks_from_file(tasks) {
                            Ok(_) => {
                                if success {
                                    app.push_agent_message(
                                        "System: Documentation has been attached to planner tasks."
                                            .to_string(),
                                    );
                                } else {
                                    app.push_agent_message(format!(
                                        "System: Documentation attach run exited with code {code}."
                                    ));
                                }
                            }
                            Err(err) => app.push_agent_message(format!(
                                "System: Docs attach completed but task refresh failed: {err}"
                            )),
                        },
                        Err(err) => app.push_agent_message(format!(
                            "System: Docs attach completed but reading tasks.json failed: {err}"
                        )),
                    }
                    chat_updated = true;
                }
            }
        }
        for event in task_check_adapter.drain_events_limited(MAX_ADAPTER_EVENTS_PER_LOOP) {
            match event {
                AgentEvent::Output(line) => {
                    app.push_subagent_output(format!("TaskCheck: {line}"));
                    chat_updated = true;
                }
                AgentEvent::System(line) => {
                    app.push_subagent_output(format!("TaskCheckSystem: {line}"));
                    chat_updated = true;
                }
                AgentEvent::Completed { success, code } => {
                    task_check_in_flight = false;
                    app.set_task_check_in_progress(false);
                    let Some(active_session) = session_store.as_ref() else {
                        task_check_baseline = None;
                        chat_updated = true;
                        continue;
                    };
                    let after_text = std::fs::read_to_string(active_session.tasks_file()).ok();
                    let changed = match (task_check_baseline.as_deref(), after_text.as_deref()) {
                        (Some(before), Some(after)) => before != after,
                        _ => false,
                    };
                    task_check_baseline = None;
                    if let Ok(tasks) = active_session.read_tasks() {
                        match app.sync_planner_tasks_from_file(tasks) {
                            Ok(()) => {
                                if changed {
                                    app.push_agent_message(
                                        "System: Task checker applied fixes to tasks.json."
                                            .to_string(),
                                    );
                                }
                            }
                            Err(err) => app.push_agent_message(format!(
                                "System: Task checker completed but task refresh failed: {err}"
                            )),
                        }
                    } else {
                        app.push_agent_message(
                            "System: Task checker completed but tasks.json could not be read."
                                .to_string(),
                        );
                    }
                    if success {
                        app.push_subagent_output(
                            "TaskCheckSystem: Task check complete.".to_string(),
                        );
                    } else {
                        app.push_subagent_output(format!(
                            "TaskCheckSystem: Task check exited with code {code}."
                        ));
                    }
                    chat_updated = true;
                }
            }
        }
        if chat_updated {
            let size = terminal.size()?;
            let screen = Rect::new(0, 0, size.width, size.height);
            let max_scroll = ui::chat_max_scroll(screen, &app);
            app.set_chat_scroll(max_scroll);
        }

        terminal.draw(|frame| ui::render(frame, &app, theme))?;

        match events::next_event()? {
            AppEvent::Tick => app.on_tick(),
            AppEvent::Quit => app.quit(),
            AppEvent::NextPane => {
                if app.is_resume_picker_open() {
                    // ignore pane focus changes while resume picker is open
                } else if app.active_pane == Pane::LeftBottom && app.autocomplete_top_command() {
                    // keep focus in input when command autocomplete is applied
                } else {
                    app.next_pane();
                }
            }
            AppEvent::PrevPane => {
                if !app.is_resume_picker_open() {
                    app.prev_pane();
                }
            }
            AppEvent::MoveUp => {
                if app.is_resume_picker_open() {
                    app.resume_picker_move_up();
                } else if app.active_pane == Pane::LeftBottom {
                    let size = terminal.size()?;
                    let width = ui::chat_input_text_width(Rect::new(0, 0, size.width, size.height));
                    app.move_cursor_up(width);
                } else if app.active_pane == Pane::Right {
                    app.scroll_right_up();
                } else {
                    app.scroll_up();
                }
            }
            AppEvent::MoveDown => {
                if app.is_resume_picker_open() {
                    app.resume_picker_move_down();
                } else if app.active_pane == Pane::LeftBottom {
                    let size = terminal.size()?;
                    let width = ui::chat_input_text_width(Rect::new(0, 0, size.width, size.height));
                    app.move_cursor_down(width);
                } else if app.active_pane == Pane::Right {
                    let size = terminal.size()?;
                    let screen = Rect::new(0, 0, size.width, size.height);
                    let max_scroll = ui::right_max_scroll(screen, &app);
                    app.scroll_right_down(max_scroll);
                } else {
                    app.scroll_down();
                }
            }
            AppEvent::CursorLeft => {
                if app.active_pane == Pane::LeftBottom && !app.is_resume_picker_open() {
                    app.move_cursor_left();
                }
            }
            AppEvent::CursorRight => {
                if app.active_pane == Pane::LeftBottom && !app.is_resume_picker_open() {
                    app.move_cursor_right();
                }
            }
            AppEvent::ScrollChatUp => {
                if app.is_resume_picker_open() {
                    app.resume_picker_move_up();
                } else if app.active_pane == Pane::LeftBottom {
                    app.scroll_chat_up();
                } else if app.active_pane == Pane::Right {
                    app.scroll_right_up();
                } else {
                    app.scroll_up();
                }
            }
            AppEvent::ScrollChatDown => {
                if app.is_resume_picker_open() {
                    app.resume_picker_move_down();
                } else if app.active_pane == Pane::LeftBottom {
                    let size = terminal.size()?;
                    let screen = Rect::new(0, 0, size.width, size.height);
                    let max_scroll = ui::chat_max_scroll(screen, &app);
                    app.scroll_chat_down(max_scroll);
                } else if app.active_pane == Pane::Right {
                    let size = terminal.size()?;
                    let screen = Rect::new(0, 0, size.width, size.height);
                    let max_scroll = ui::right_max_scroll(screen, &app);
                    app.scroll_right_down(max_scroll);
                } else {
                    app.scroll_down();
                }
            }
            AppEvent::ScrollRightUpGlobal => {
                scroll_right_up_global(&mut app);
            }
            AppEvent::ScrollRightDownGlobal => {
                let size = terminal.size()?;
                let screen = Rect::new(0, 0, size.width, size.height);
                let max_scroll = ui::right_max_scroll(screen, &app);
                scroll_right_down_global(&mut app, max_scroll);
            }
            AppEvent::InputChar(c) => {
                if app.is_resume_picker_open() {
                    if c == ' '
                        && let Some(selection) = app.select_resume_session()
                    {
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
                            terminal,
                        )?;
                    }
                } else if app.active_pane == Pane::LeftBottom {
                    app.input_char(c);
                } else if c == 'j' {
                    app.scroll_down();
                } else if c == 'k' {
                    app.scroll_up();
                }
            }
            AppEvent::Backspace => {
                if app.active_pane == Pane::LeftBottom && !app.is_resume_picker_open() {
                    app.backspace_input();
                }
            }
            AppEvent::Submit => {
                if app.is_resume_picker_open() {
                    if let Some(selection) = app.select_resume_session() {
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
                            terminal,
                        )?;
                    }
                } else if app.active_pane == Pane::LeftBottom {
                    let pending = app.chat_input().trim().to_string();
                    match submit_block_reason(
                        project_info_in_flight,
                        app.is_master_in_progress(),
                        app.is_task_check_in_progress(),
                        app.is_execution_busy(),
                        &pending,
                    ) {
                        Some(SubmitBlockReason::ProjectInfoGathering) => {
                            app.push_agent_message(
                                "System: Project context gathering is in progress. Enter/Return submissions are temporarily disabled until it completes.".to_string(),
                            );
                            continue;
                        }
                        Some(SubmitBlockReason::MasterBusy) => {
                            app.push_agent_message(
                                "System: Master is still processing your previous request. Enter/Return submissions are temporarily disabled until it completes.".to_string(),
                            );
                            continue;
                        }
                        Some(SubmitBlockReason::TaskCheck) => {
                            app.push_agent_message(
                                "System: Task checking is in progress. Message and slash commands are temporarily blocked (except /quit and /exit).".to_string(),
                            );
                            continue;
                        }
                        Some(SubmitBlockReason::ExecutionBusy) => {
                            app.push_agent_message(
                                "System: Execution is currently running. Master/task editing commands are blocked until active worker jobs finish."
                                    .to_string(),
                            );
                            continue;
                        }
                        None => {}
                    }
                    if parse_silent_master_command(&pending).is_some()
                        || App::is_add_final_audit_command(&pending)
                        || App::is_remove_final_audit_command(&pending)
                    {
                        if let Some(message) = app.consume_chat_input_trimmed() {
                            submit_user_message(
                                &mut app,
                                message,
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
                                terminal,
                                &mut pending_task_write_baseline,
                                &mut docs_attach_in_flight,
                                &mut master_session_intro_needed,
                                &mut master_report_session_intro_needed,
                                &mut pending_master_message_after_project_info,
                                &mut project_info_in_flight,
                                &mut project_info_stage,
                                &mut project_info_text,
                                &model_routing,
                            )?;
                        }
                    } else if let Some(message) = app.submit_chat_message() {
                        submit_user_message(
                            &mut app,
                            message,
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
                            terminal,
                            &mut pending_task_write_baseline,
                            &mut docs_attach_in_flight,
                            &mut master_session_intro_needed,
                            &mut master_report_session_intro_needed,
                            &mut pending_master_message_after_project_info,
                            &mut project_info_in_flight,
                            &mut project_info_stage,
                            &mut project_info_text,
                            &model_routing,
                        )?;
                    }
                }
            }
            AppEvent::MouseScrollUp => {
                if app.is_resume_picker_open() {
                    app.resume_picker_move_up();
                } else if app.active_pane == Pane::LeftBottom {
                    app.scroll_chat_up();
                } else if app.active_pane == Pane::Right {
                    app.scroll_right_up();
                } else {
                    app.scroll_up();
                }
            }
            AppEvent::MouseScrollDown => {
                if app.is_resume_picker_open() {
                    app.resume_picker_move_down();
                } else if app.active_pane == Pane::LeftBottom {
                    let size = terminal.size()?;
                    let screen = Rect::new(0, 0, size.width, size.height);
                    let max_scroll = ui::chat_max_scroll(screen, &app);
                    app.scroll_chat_down(max_scroll);
                } else if app.active_pane == Pane::Right {
                    let size = terminal.size()?;
                    let screen = Rect::new(0, 0, size.width, size.height);
                    let max_scroll = ui::right_max_scroll(screen, &app);
                    app.scroll_right_down(max_scroll);
                } else {
                    app.scroll_down();
                }
            }
            AppEvent::MouseLeftClick(column, row) => {
                if !app.is_resume_picker_open() {
                    let size = terminal.size()?;
                    let screen = Rect::new(0, 0, size.width, size.height);
                    if let Some(pane) = ui::pane_hit_test(screen, column, row) {
                        app.active_pane = pane;
                    }
                    if let Some(task_key) =
                        ui::right_pane_toggle_hit_test(screen, &app, column, row)
                    {
                        app.toggle_task_details(&task_key);
                    }
                }
            }
        }
    }

    Ok(())
}

fn submit_user_message<B: Backend>(
    app: &mut App,
    message: String,
    master_adapter: &CodexAdapter,
    master_report_adapter: &CodexAdapter,
    project_info_adapter: &CodexAdapter,
    worker_agent_adapters: &mut HashMap<String, CodexAdapter>,
    active_worker_context_key: &mut Option<String>,
    docs_attach_adapter: &CodexAdapter,
    test_runner_adapter: &TestRunnerAdapter,
    master_report_in_flight: &mut bool,
    pending_master_report_prompts: &mut VecDeque<String>,
    master_report_transcript: &mut Vec<String>,
    task_check_in_flight: &mut bool,
    task_check_baseline: &mut Option<String>,
    session_store: &mut Option<SessionStore>,
    cwd: &Path,
    terminal: &mut Terminal<B>,
    pending_task_write_baseline: &mut Option<PendingTaskWriteBaseline>,
    docs_attach_in_flight: &mut bool,
    master_session_intro_needed: &mut bool,
    master_report_session_intro_needed: &mut bool,
    pending_master_message_after_project_info: &mut Option<String>,
    project_info_in_flight: &mut bool,
    project_info_stage: &mut Option<ProjectInfoStage>,
    project_info_text: &mut Option<String>,
    model_routing: &CodexAgentModelRouting,
) -> io::Result<()> {
    if should_send_to_master(&message) && app.is_master_in_progress() {
        app.push_agent_message(
            "System: Master is still processing your previous request. Please wait for completion before sending another message."
                .to_string(),
        );
        let size = terminal.size()?;
        let screen = Rect::new(0, 0, size.width, size.height);
        let max_scroll = ui::chat_max_scroll(screen, app);
        app.set_chat_scroll(max_scroll);
        return Ok(());
    }

    initialize_session_for_message_if_needed(app, &message, cwd, session_store, project_info_text)?;

    if command_requires_active_session(&message) && session_store.is_none() {
        app.push_agent_message(
            "System: No active session yet. Enter a normal message to start one, or use /resume to select an existing session."
                .to_string(),
        );
        let size = terminal.size()?;
        let screen = Rect::new(0, 0, size.width, size.height);
        let max_scroll = ui::chat_max_scroll(screen, app);
        app.set_chat_scroll(max_scroll);
        return Ok(());
    }

    if app.is_execution_busy() && conflicts_with_running_execution(&message) {
        app.push_agent_message(
            "System: Execution is currently running. Master/task editing commands are blocked until active worker jobs finish."
                .to_string(),
        );
        let size = terminal.size()?;
        let screen = Rect::new(0, 0, size.width, size.height);
        let max_scroll = ui::chat_max_scroll(screen, app);
        app.set_chat_scroll(max_scroll);
        return Ok(());
    }

    if App::is_add_final_audit_command(&message) || App::is_remove_final_audit_command(&message) {
        let active_session = session_store
            .as_ref()
            .expect("final-audit commands require an active session");
        if handle_final_audit_command(app, &message, active_session, terminal)? {
            return Ok(());
        }
    }

    if parse_silent_master_command(&message).is_some() {
        let active_session = session_store
            .as_ref()
            .expect("silent master commands require an active session");
        if handle_silent_master_command(
            app,
            &message,
            master_adapter,
            active_session,
            terminal,
            pending_task_write_baseline,
            master_session_intro_needed,
            project_info_text.as_deref(),
        )? {
            return Ok(());
        }
    }

    if App::is_new_master_command(&message) {
        if app.is_execution_busy() {
            app.push_agent_message(
                "System: Cannot start a new master session while worker execution is running. Wait for active jobs to finish first."
                    .to_string(),
            );
            let size = terminal.size()?;
            let screen = Rect::new(0, 0, size.width, size.height);
            let max_scroll = ui::chat_max_scroll(screen, app);
            app.set_chat_scroll(max_scroll);
            return Ok(());
        }
        master_adapter.reset_session();
        master_report_adapter.reset_session();
        project_info_adapter.reset_session();
        worker_agent_adapters.clear();
        *active_worker_context_key = None;
        *pending_task_write_baseline = None;
        *master_session_intro_needed = true;
        *master_report_session_intro_needed = true;
        *project_info_stage = None;
        *project_info_in_flight = false;
        *pending_master_message_after_project_info = None;
        *docs_attach_in_flight = false;
        reset_master_report_runtime(
            master_report_in_flight,
            pending_master_report_prompts,
            master_report_transcript,
        );
        reset_task_check_runtime(task_check_in_flight, task_check_baseline);
        app.set_docs_attach_in_progress(false);
        app.set_task_check_in_progress(false);
        app.set_master_in_progress(false);
        app.reset_execution_for_session_switch();
        app.push_agent_message("System: Started new master session".to_string());
        let size = terminal.size()?;
        let screen = Rect::new(0, 0, size.width, size.height);
        let max_scroll = ui::chat_max_scroll(screen, app);
        app.set_chat_scroll(max_scroll);
        return Ok(());
    }

    if App::is_attach_docs_command(&message) {
        let active_session = session_store
            .as_ref()
            .expect("/attach-docs requires an active session");
        if *docs_attach_in_flight {
            app.push_agent_message(
                "System: Docs attach is already running. Please wait for completion.".to_string(),
            );
        } else {
            let prompt =
                app.prepare_attach_docs_prompt(&active_session.tasks_file().display().to_string());
            docs_attach_adapter.send_prompt(prompt);
            *docs_attach_in_flight = true;
            app.set_docs_attach_in_progress(true);
            app.push_agent_message("System: Started documentation attach sub-agent.".to_string());
        }
        let size = terminal.size()?;
        let screen = Rect::new(0, 0, size.width, size.height);
        let max_scroll = ui::chat_max_scroll(screen, app);
        app.set_chat_scroll(max_scroll);
        return Ok(());
    }

    if App::is_quit_command(&message) {
        app.quit();
        return Ok(());
    }

    if App::is_resume_command(&message) {
        match SessionStore::list_sessions() {
            Ok(sessions) if sessions.is_empty() => {
                app.push_agent_message("System: No saved sessions found.".to_string());
            }
            Ok(sessions) => {
                let current_session_dir = session_store.as_ref().map(SessionStore::session_dir);
                let options = build_resume_options(sessions, current_session_dir);
                if options.is_empty() {
                    app.push_agent_message(
                        "System: No other saved sessions found to resume.".to_string(),
                    );
                } else {
                    app.open_resume_picker(options);
                    app.push_agent_message(
                        "System: Select a session in the resume picker and press Enter or Space."
                            .to_string(),
                    );
                }
            }
            Err(err) => {
                app.push_agent_message(format!("System: Failed to list sessions: {err}"));
            }
        }
        let size = terminal.size()?;
        let screen = Rect::new(0, 0, size.width, size.height);
        let max_scroll = ui::chat_max_scroll(screen, app);
        app.set_chat_scroll(max_scroll);
        return Ok(());
    }

    if App::is_planner_mode_command(&message) {
        let active_session = session_store
            .as_ref()
            .expect("/planner requires an active session");
        if let Ok(markdown) = active_session.read_planner_markdown() {
            app.set_planner_markdown(markdown);
        }
        app.set_right_pane_mode(RightPaneMode::PlannerMarkdown);
        app.push_agent_message(
            "System: Planner mode enabled. The right pane now shows planner.md for collaborative markdown planning."
                .to_string(),
        );
        let size = terminal.size()?;
        let screen = Rect::new(0, 0, size.width, size.height);
        let max_scroll = ui::chat_max_scroll(screen, app);
        app.set_chat_scroll(max_scroll);
        return Ok(());
    }

    if App::is_skip_plan_command(&message) {
        app.set_right_pane_mode(RightPaneMode::TaskList);
        app.push_agent_message(
            "System: Skip-plan mode enabled. The right pane now shows planner tasks for execution."
                .to_string(),
        );
        let size = terminal.size()?;
        let screen = Rect::new(0, 0, size.width, size.height);
        let max_scroll = ui::chat_max_scroll(screen, app);
        app.set_chat_scroll(max_scroll);
        return Ok(());
    }

    if App::is_convert_command(&message) {
        let active_session = session_store
            .as_ref()
            .expect("/convert requires an active session");
        if !app.is_planner_mode() {
            app.push_agent_message(
                "System: /convert is only available in planner mode. Use /planner first."
                    .to_string(),
            );
            let size = terminal.size()?;
            let screen = Rect::new(0, 0, size.width, size.height);
            let max_scroll = ui::chat_max_scroll(screen, app);
            app.set_chat_scroll(max_scroll);
            return Ok(());
        }

        app.set_right_pane_mode(RightPaneMode::TaskList);
        app.push_agent_message(
            "System: Converting planner.md into tasks.json in task mode...".to_string(),
        );

        let command_prompt = subagents::build_convert_plan_prompt(
            &active_session.planner_file().display().to_string(),
            &active_session.tasks_file().display().to_string(),
        );
        let master_prompt = app.prepare_master_prompt(
            &command_prompt,
            &active_session.tasks_file().display().to_string(),
        );
        let with_intro = subagents::build_session_intro_if_needed(
            &master_prompt,
            active_session.session_dir().display().to_string().as_str(),
            &active_session.session_meta_file().display().to_string(),
            project_info_text.as_deref(),
            master_session_intro_needed,
        );
        master_adapter.send_prompt(with_intro);
        app.set_master_in_progress(true);
        *pending_task_write_baseline = capture_tasks_baseline(active_session);

        let size = terminal.size()?;
        let screen = Rect::new(0, 0, size.width, size.height);
        let max_scroll = ui::chat_max_scroll(screen, app);
        app.set_chat_scroll(max_scroll);
        return Ok(());
    }

    if message.trim().starts_with('/') && !is_known_slash_command(&message) {
        app.push_agent_message(format!(
            "System: Unknown command `{}`. Type `/` to see available commands.",
            message.trim()
        ));
        let size = terminal.size()?;
        let screen = Rect::new(0, 0, size.width, size.height);
        let max_scroll = ui::chat_max_scroll(screen, app);
        app.set_chat_scroll(max_scroll);
        return Ok(());
    }

    if should_send_to_master(&message) {
        let active_session = session_store
            .as_ref()
            .expect("non-slash messages require an active session");
        if project_info_text.is_none() {
            if !*project_info_in_flight {
                let prompt = subagents::build_project_info_prompt(
                    &cwd.display().to_string(),
                    &message,
                    &active_session.project_info_file().display().to_string(),
                );
                project_info_adapter.send_prompt(prompt);
                *project_info_in_flight = true;
                *project_info_stage = Some(ProjectInfoStage::GatheringInfo);
                *pending_master_message_after_project_info = Some(message.clone());
                app.push_agent_message(
                    "System: Gathering project context before contacting master.".to_string(),
                );
            } else {
                *pending_master_message_after_project_info = Some(message.clone());
                app.push_agent_message(
                    "System: Project context is still gathering; your message will be sent when ready."
                        .to_string(),
                );
            }
        } else {
            let master_prompt = if app.is_planner_mode() {
                app.prepare_planner_prompt(
                    &message,
                    &active_session.planner_file().display().to_string(),
                    &active_session.project_info_file().display().to_string(),
                )
            } else {
                app.prepare_master_prompt(
                    &message,
                    &active_session.tasks_file().display().to_string(),
                )
            };
            let with_intro = subagents::build_session_intro_if_needed(
                &master_prompt,
                active_session.session_dir().display().to_string().as_str(),
                &active_session.session_meta_file().display().to_string(),
                project_info_text.as_deref(),
                master_session_intro_needed,
            );
            master_adapter.send_prompt(with_intro);
            app.set_master_in_progress(true);
        }
    }
    if App::is_start_execution_command(&message) {
        let active_session = session_store
            .as_ref()
            .expect("start execution requires an active session");
        if is_slash_start_command(&message) {
            app.push_agent_message("System: Started execution".to_string());
        }
        *pending_task_write_baseline = None;
        for system_message in app.start_execution() {
            app.push_agent_message(system_message);
        }
        if let Some(job) = claim_next_worker_job_and_persist_snapshot(app, active_session) {
            dispatch_worker_job(
                &job,
                worker_agent_adapters,
                active_worker_context_key,
                test_runner_adapter,
                active_session,
                model_routing,
            );
            app.push_agent_message(format!(
                "System: Starting {:?} for task #{}.",
                job.role, job.top_task_id
            ));
        }
    } else {
        *pending_task_write_baseline = session_store.as_ref().and_then(capture_tasks_baseline);
    }
    let size = terminal.size()?;
    let screen = Rect::new(0, 0, size.width, size.height);
    let max_scroll = ui::chat_max_scroll(screen, app);
    app.set_chat_scroll(max_scroll);
    Ok(())
}

fn resumed_right_pane_mode(tasks: &[PlannerTaskFileEntry]) -> RightPaneMode {
    if tasks.is_empty() {
        RightPaneMode::PlannerMarkdown
    } else {
        RightPaneMode::TaskList
    }
}

#[derive(Debug)]
struct PreparedResumeSession {
    store: SessionStore,
    tasks: Vec<PlannerTaskFileEntry>,
    pane_mode: RightPaneMode,
    planner_markdown: String,
    rolling_context: Vec<String>,
    project_info_text: Option<String>,
}

fn prepare_resumed_session(
    cwd: &Path,
    selection: &ResumeSessionOption,
) -> io::Result<PreparedResumeSession> {
    let selected_path = PathBuf::from(&selection.session_dir);
    let store = SessionStore::open_existing(cwd, &selected_path)?;
    let tasks = store.read_tasks().map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to read tasks.json for resumed session: {err}"),
        )
    })?;
    let mut validator = workflow::Workflow::default();
    validator
        .sync_planner_tasks_from_file(tasks.clone())
        .map_err(|err| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("failed to validate resumed tasks.json: {err}"),
            )
        })?;

    Ok(PreparedResumeSession {
        pane_mode: resumed_right_pane_mode(&tasks),
        planner_markdown: store.read_planner_markdown().unwrap_or_default(),
        rolling_context: store.read_rolling_context().unwrap_or_default(),
        project_info_text: store
            .read_project_info()
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        store,
        tasks,
    })
}

#[allow(clippy::too_many_arguments)]
fn resume_session<B: Backend>(
    app: &mut App,
    session_store: &mut Option<SessionStore>,
    selection: ResumeSessionOption,
    master_adapter: &CodexAdapter,
    master_report_adapter: &CodexAdapter,
    project_info_adapter: &CodexAdapter,
    worker_agent_adapters: &mut HashMap<String, CodexAdapter>,
    active_worker_context_key: &mut Option<String>,
    pending_task_write_baseline: &mut Option<PendingTaskWriteBaseline>,
    docs_attach_in_flight: &mut bool,
    master_session_intro_needed: &mut bool,
    master_report_session_intro_needed: &mut bool,
    pending_master_message_after_project_info: &mut Option<String>,
    project_info_in_flight: &mut bool,
    project_info_stage: &mut Option<ProjectInfoStage>,
    project_info_text: &mut Option<String>,
    master_report_in_flight: &mut bool,
    pending_master_report_prompts: &mut VecDeque<String>,
    master_report_transcript: &mut Vec<String>,
    task_check_in_flight: &mut bool,
    task_check_baseline: &mut Option<String>,
    terminal: &mut Terminal<B>,
) -> io::Result<()> {
    let cwd = std::env::current_dir()?;
    let prepared = match prepare_resumed_session(&cwd, &selection) {
        Ok(prepared) => prepared,
        Err(err) => {
            app.push_agent_message(format!(
                "System: Failed to resume session {}: {err}",
                selection.session_dir
            ));
            let size = terminal.size()?;
            let screen = Rect::new(0, 0, size.width, size.height);
            let max_scroll = ui::chat_max_scroll(screen, app);
            app.set_chat_scroll(max_scroll);
            return Ok(());
        }
    };

    *session_store = Some(prepared.store);
    let active_session = session_store
        .as_ref()
        .expect("resumed session should be available");

    master_adapter.reset_session();
    master_report_adapter.reset_session();
    project_info_adapter.reset_session();
    worker_agent_adapters.clear();
    *active_worker_context_key = None;
    *master_session_intro_needed = true;
    *master_report_session_intro_needed = true;
    *pending_task_write_baseline = None;
    *pending_master_message_after_project_info = None;
    *docs_attach_in_flight = false;
    *project_info_in_flight = false;
    *project_info_stage = None;
    reset_master_report_runtime(
        master_report_in_flight,
        pending_master_report_prompts,
        master_report_transcript,
    );
    reset_task_check_runtime(task_check_in_flight, task_check_baseline);

    app.reset_execution_for_session_switch();
    app.set_task_check_in_progress(false);
    app.set_docs_attach_in_progress(false);
    app.set_master_in_progress(false);

    app.replace_rolling_context_entries(prepared.rolling_context);

    match app.sync_planner_tasks_from_file(prepared.tasks) {
        Ok(()) => {
            app.set_right_pane_mode(prepared.pane_mode);
        }
        Err(err) => app.push_agent_message(format!(
            "System: Failed to refresh task tree from resumed tasks.json: {err}"
        )),
    }
    app.set_planner_markdown(prepared.planner_markdown);

    *project_info_text = prepared.project_info_text;

    app.push_agent_message(format!(
        "System: Resumed session {}",
        active_session.session_dir().display()
    ));

    let size = terminal.size()?;
    let screen = Rect::new(0, 0, size.width, size.height);
    let max_scroll = ui::chat_max_scroll(screen, app);
    app.set_chat_scroll(max_scroll);
    Ok(())
}

fn build_resume_options(
    sessions: Vec<SessionListEntry>,
    current_session_dir: Option<&std::path::Path>,
) -> Vec<ResumeSessionOption> {
    sessions
        .into_iter()
        .filter(|entry| {
            current_session_dir
                .map(|dir| entry.session_dir != dir)
                .unwrap_or(true)
        })
        .map(|entry| ResumeSessionOption {
            session_dir: entry.session_dir.display().to_string(),
            workspace: entry.workspace,
            title: entry.title,
            created_at_label: entry.created_at_label,
            last_used_epoch_secs: entry.last_used_epoch_secs,
        })
        .collect()
}

fn handle_final_audit_command<B: Backend>(
    app: &mut App,
    message: &str,
    session_store: &SessionStore,
    terminal: &mut Terminal<B>,
) -> io::Result<bool> {
    if handle_final_audit_tasks_command(app, message, session_store)? {
        let size = terminal.size()?;
        let screen = Rect::new(0, 0, size.width, size.height);
        let max_scroll = ui::chat_max_scroll(screen, app);
        app.set_chat_scroll(max_scroll);
        return Ok(true);
    }
    Ok(false)
}

fn handle_final_audit_tasks_command(
    app: &mut App,
    message: &str,
    session_store: &SessionStore,
) -> io::Result<bool> {
    if (App::is_add_final_audit_command(message) || App::is_remove_final_audit_command(message))
        && app.is_execution_busy()
    {
        app.push_agent_message(
            "System: Cannot modify final-audit tasks while worker execution is running. Wait for active jobs to finish first."
                .to_string(),
        );
        return Ok(true);
    }

    if App::is_add_final_audit_command(message) {
        let mut tasks = match session_store.read_tasks() {
            Ok(tasks) => tasks,
            Err(err) => {
                app.push_agent_message(format!(
                    "System: Could not read tasks file; final audit command aborted: {err}"
                ));
                return Ok(true);
            }
        };
        ensure_final_audit_task(&mut tasks);
        normalize_root_orders_with_final_last(&mut tasks);
        match app.sync_planner_tasks_from_file(tasks.clone()) {
            Ok(_) => match serde_json::to_string_pretty(&tasks)
                .map_err(io::Error::other)
                .and_then(|text| std::fs::write(session_store.tasks_file(), text))
            {
                Ok(()) => app.push_agent_message("System: Added final audit task.".to_string()),
                Err(err) => app.push_agent_message(format!(
                    "System: Failed to write tasks file while adding final audit task: {err}"
                )),
            },
            Err(err) => app.push_agent_message(format!(
                "System: Final audit command aborted; task tree refresh failed before write: {err}"
            )),
        }
        return Ok(true);
    }

    if App::is_remove_final_audit_command(message) {
        let mut tasks = match session_store.read_tasks() {
            Ok(tasks) => tasks,
            Err(err) => {
                app.push_agent_message(format!(
                    "System: Could not read tasks file; final audit command aborted: {err}"
                ));
                return Ok(true);
            }
        };
        let final_audit_count = tasks
            .iter()
            .filter(|task| task.kind == PlannerTaskKindFile::FinalAudit)
            .count();
        tasks.retain(|task| task.kind != PlannerTaskKindFile::FinalAudit);
        normalize_root_orders_with_final_last(&mut tasks);
        match app.sync_planner_tasks_from_file(tasks.clone()) {
            Ok(_) => match serde_json::to_string_pretty(&tasks)
                .map_err(io::Error::other)
                .and_then(|text| std::fs::write(session_store.tasks_file(), text))
            {
                Ok(()) => {
                    if final_audit_count == 0 {
                        app.push_agent_message(
                            "System: No final audit task was present.".to_string(),
                        );
                    } else {
                        app.push_agent_message("System: Removed final audit task.".to_string());
                    }
                }
                Err(err) => app.push_agent_message(format!(
                    "System: Failed to write tasks file while removing final audit task: {err}"
                )),
            },
            Err(err) => app.push_agent_message(format!(
                "System: Final audit command aborted; task tree refresh failed before write: {err}"
            )),
        }
        return Ok(true);
    }

    Ok(false)
}

fn ensure_final_audit_task(tasks: &mut Vec<PlannerTaskFileEntry>) {
    if let Some(existing) = tasks
        .iter_mut()
        .find(|task| task.kind == PlannerTaskKindFile::FinalAudit)
    {
        existing.parent_id = None;
        existing.status = PlannerTaskStatusFile::Pending;
        if existing.title.trim().is_empty() {
            existing.title = "Final Audit".to_string();
        }
        if existing.details.trim().is_empty() {
            existing.details =
                "Perform a final holistic audit after all implementation and test tasks complete."
                    .to_string();
        }
        return;
    }

    let mut suffix = 1usize;
    let mut id = "final-audit".to_string();
    while tasks.iter().any(|task| task.id == id) {
        suffix = suffix.saturating_add(1);
        id = format!("final-audit-{suffix}");
    }
    tasks.push(PlannerTaskFileEntry {
        id,
        title: "Final Audit".to_string(),
        details: "Perform a final holistic audit after all implementation and test tasks complete."
            .to_string(),
        docs: Vec::new(),
        kind: PlannerTaskKindFile::FinalAudit,
        status: PlannerTaskStatusFile::Pending,
        parent_id: None,
        order: Some(u32::MAX),
    });
}

fn normalize_root_orders_with_final_last(tasks: &mut [PlannerTaskFileEntry]) {
    let mut non_final = tasks
        .iter()
        .enumerate()
        .filter(|(_, task)| {
            task.parent_id.is_none() && task.kind != PlannerTaskKindFile::FinalAudit
        })
        .map(|(idx, task)| (idx, task.order.unwrap_or(u32::MAX)))
        .collect::<Vec<_>>();
    non_final.sort_by_key(|(_, order)| *order);
    for (pos, (idx, _)) in non_final.into_iter().enumerate() {
        tasks[idx].order = Some(pos as u32);
    }

    let mut finals = tasks
        .iter()
        .enumerate()
        .filter(|(_, task)| {
            task.parent_id.is_none() && task.kind == PlannerTaskKindFile::FinalAudit
        })
        .map(|(idx, task)| (idx, task.order.unwrap_or(u32::MAX)))
        .collect::<Vec<_>>();
    finals.sort_by_key(|(_, order)| *order);
    let base = tasks
        .iter()
        .filter(|task| task.parent_id.is_none() && task.kind != PlannerTaskKindFile::FinalAudit)
        .count() as u32;
    for (offset, (idx, _)) in finals.into_iter().enumerate() {
        tasks[idx].order = Some(base.saturating_add(offset as u32));
    }
}

fn sanitize_master_docs_fields(
    tasks: &mut [PlannerTaskFileEntry],
    baseline_tasks_json: Option<&str>,
) -> bool {
    use std::collections::HashMap;

    let baseline_docs = baseline_tasks_json
        .and_then(|text| serde_json::from_str::<Vec<PlannerTaskFileEntry>>(text).ok())
        .map(|entries| {
            entries
                .into_iter()
                .map(|entry| (entry.id, entry.docs))
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default();

    let mut changed = false;
    for task in tasks.iter_mut() {
        if let Some(existing_docs) = baseline_docs.get(&task.id) {
            if task.docs != *existing_docs {
                task.docs = existing_docs.clone();
                changed = true;
            }
        } else if !task.docs.is_empty() {
            task.docs.clear();
            changed = true;
        }
    }
    changed
}

fn handle_exhausted_loop_failures(
    app: &mut App,
    session_store: &SessionStore,
    master_report_session_intro_needed: &mut bool,
    project_info_text: Option<&str>,
    failures: Vec<WorkflowFailure>,
) -> Option<String> {
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let fail_entries: Vec<TaskFailFileEntry> = failures
        .iter()
        .map(|failure| TaskFailFileEntry {
            kind: match failure.kind {
                WorkflowFailureKind::Audit => "audit".to_string(),
                WorkflowFailureKind::Test => "test".to_string(),
            },
            top_task_id: failure.top_task_id,
            top_task_title: failure.top_task_title.clone(),
            attempts: failure.attempts,
            reason: failure.reason.clone(),
            action_taken: failure.action_taken.clone(),
            created_at_epoch_secs: now_secs,
        })
        .collect();

    if let Err(err) = session_store.append_task_fails(&fail_entries) {
        app.push_agent_message(format!("System: Failed to append task-fails.json: {err}"));
        return None;
    }

    let has_test_failure = fail_entries.iter().any(|entry| entry.kind == "test");
    let prompt = subagents::build_failure_report_prompt(
        &session_store.task_fails_file().display().to_string(),
        &fail_entries,
        has_test_failure,
    );
    Some(subagents::build_session_intro_if_needed(
        &prompt,
        session_store.session_dir().display().to_string().as_str(),
        &session_store.session_meta_file().display().to_string(),
        project_info_text,
        master_report_session_intro_needed,
    ))
}

fn persist_runtime_tasks_snapshot(app: &App, session_store: &SessionStore) -> io::Result<()> {
    let tasks = app.planner_tasks_for_file();
    let text = serde_json::to_string_pretty(&tasks).map_err(io::Error::other)?;
    std::fs::write(session_store.tasks_file(), text)
}

fn capture_tasks_baseline(session_store: &SessionStore) -> Option<PendingTaskWriteBaseline> {
    let tasks_json = std::fs::read_to_string(session_store.tasks_file()).ok()?;
    Some(PendingTaskWriteBaseline { tasks_json })
}

fn tasks_changed_since_baseline(before: Option<&str>, after: Option<&str>) -> bool {
    match (before, after) {
        (Some(before), Some(after)) => before != after,
        (None, Some(after)) => !after.trim().is_empty(),
        _ => false,
    }
}

fn should_clear_task_write_baseline(
    tasks_refresh_ok: bool,
    requested_task_file_retry: bool,
) -> bool {
    tasks_refresh_ok || !requested_task_file_retry
}

fn reset_master_report_runtime(
    master_report_in_flight: &mut bool,
    pending_master_report_prompts: &mut VecDeque<String>,
    master_report_transcript: &mut Vec<String>,
) {
    *master_report_in_flight = false;
    pending_master_report_prompts.clear();
    master_report_transcript.clear();
}

fn reset_task_check_runtime(
    task_check_in_flight: &mut bool,
    task_check_baseline: &mut Option<String>,
) {
    *task_check_in_flight = false;
    *task_check_baseline = None;
}

fn scroll_right_up_global(app: &mut App) {
    for _ in 0..GLOBAL_RIGHT_SCROLL_LINES {
        app.scroll_right_up();
    }
}

fn scroll_right_down_global(app: &mut App, max_scroll: u16) {
    for _ in 0..GLOBAL_RIGHT_SCROLL_LINES {
        app.scroll_right_down(max_scroll);
    }
}

fn should_send_to_master(message: &str) -> bool {
    let trimmed = message.trim();
    !trimmed.starts_with('/') && !App::is_start_execution_command(trimmed)
}

fn should_initialize_session_for_message(message: &str) -> bool {
    let trimmed = message.trim();
    !trimmed.starts_with('/')
}

fn command_requires_active_session(message: &str) -> bool {
    let trimmed = message.trim();
    if !trimmed.starts_with('/') {
        return false;
    }
    App::is_start_execution_command(trimmed)
        || App::is_planner_mode_command(trimmed)
        || App::is_convert_command(trimmed)
        || App::is_attach_docs_command(trimmed)
        || parse_silent_master_command(trimmed).is_some()
        || App::is_add_final_audit_command(trimmed)
        || App::is_remove_final_audit_command(trimmed)
}

fn initialize_session_for_message_if_needed(
    app: &mut App,
    message: &str,
    cwd: &Path,
    session_store: &mut Option<SessionStore>,
    project_info_text: &mut Option<String>,
) -> io::Result<()> {
    if !should_initialize_session_for_message(message) || session_store.is_some() {
        return Ok(());
    }

    let store = SessionStore::initialize(cwd)?;
    app.push_agent_message(format!(
        "System: Session dir initialized at {}",
        store.session_dir().display()
    ));
    match store.read_tasks() {
        Ok(tasks) => match app.sync_planner_tasks_from_file(tasks) {
            Ok(()) => {}
            Err(err) => app.push_agent_message(format!(
                "System: Failed to refresh task tree from tasks.json: {err}"
            )),
        },
        Err(err) => app.push_agent_message(format!("System: Failed to read tasks.json: {err}")),
    }
    if let Ok(markdown) = store.read_planner_markdown() {
        app.set_planner_markdown(markdown);
    }
    *project_info_text = store
        .read_project_info()
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    *session_store = Some(store);
    Ok(())
}

fn is_allowed_during_task_check(message: &str) -> bool {
    App::is_quit_command(message)
}

fn conflicts_with_running_execution(message: &str) -> bool {
    should_send_to_master(message)
        || App::is_new_master_command(message)
        || App::is_resume_command(message)
        || App::is_convert_command(message)
        || App::is_attach_docs_command(message)
        || parse_silent_master_command(message).is_some()
        || App::is_add_final_audit_command(message)
        || App::is_remove_final_audit_command(message)
}

fn submit_block_reason(
    project_info_in_flight: bool,
    master_in_progress: bool,
    task_check_in_progress: bool,
    execution_busy: bool,
    message: &str,
) -> Option<SubmitBlockReason> {
    if project_info_in_flight {
        return Some(SubmitBlockReason::ProjectInfoGathering);
    }
    if master_in_progress {
        return Some(SubmitBlockReason::MasterBusy);
    }
    if task_check_in_progress && !is_allowed_during_task_check(message) {
        return Some(SubmitBlockReason::TaskCheck);
    }
    if execution_busy && conflicts_with_running_execution(message) {
        return Some(SubmitBlockReason::ExecutionBusy);
    }
    None
}

fn enqueue_or_dispatch_master_report_prompt(
    prompt: String,
    master_report_in_flight: &mut bool,
    pending_master_report_prompts: &mut VecDeque<String>,
) -> Option<String> {
    if *master_report_in_flight {
        pending_master_report_prompts.push_back(prompt);
        None
    } else {
        *master_report_in_flight = true;
        Some(prompt)
    }
}

fn complete_and_next_master_report_prompt(
    master_report_in_flight: &mut bool,
    pending_master_report_prompts: &mut VecDeque<String>,
) -> Option<String> {
    if let Some(next_prompt) = pending_master_report_prompts.pop_front() {
        *master_report_in_flight = true;
        Some(next_prompt)
    } else {
        *master_report_in_flight = false;
        None
    }
}

fn should_start_task_check(
    changed_tasks: bool,
    task_check_in_flight: bool,
    docs_attach_in_flight: bool,
) -> bool {
    changed_tasks && !task_check_in_flight && !docs_attach_in_flight
}

fn drain_post_completion_worker_events(adapter: &CodexAdapter) -> Vec<AgentEvent> {
    const MAX_TAIL_POLLS: usize = 24;
    const MAX_IDLE_POLLS: usize = 8;
    let mut out = Vec::new();
    let mut idle_polls = 0usize;
    for _ in 0..MAX_TAIL_POLLS {
        let batch = adapter.drain_events_limited(MAX_ADAPTER_EVENTS_PER_LOOP);
        if batch.is_empty() {
            idle_polls = idle_polls.saturating_add(1);
            if idle_polls >= MAX_IDLE_POLLS {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
            continue;
        }
        idle_polls = 0;
        out.extend(batch);
    }
    out
}

fn should_process_master_task_file_updates(execution_busy: bool) -> bool {
    !execution_busy
}

fn parse_silent_master_command(message: &str) -> Option<SilentMasterCommand> {
    if App::is_split_audits_command(message) {
        return Some(SilentMasterCommand::SplitAudits);
    }
    if App::is_merge_audits_command(message) {
        return Some(SilentMasterCommand::MergeAudits);
    }
    if App::is_split_tests_command(message) {
        return Some(SilentMasterCommand::SplitTests);
    }
    if App::is_merge_tests_command(message) {
        return Some(SilentMasterCommand::MergeTests);
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn handle_silent_master_command<B: Backend>(
    app: &mut App,
    message: &str,
    master_adapter: &CodexAdapter,
    session_store: &SessionStore,
    terminal: &mut Terminal<B>,
    pending_task_write_baseline: &mut Option<PendingTaskWriteBaseline>,
    master_session_intro_needed: &mut bool,
    project_info_text: Option<&str>,
) -> io::Result<bool> {
    let Some(command) = parse_silent_master_command(message) else {
        return Ok(false);
    };

    let (status_message, command_prompt) = match command {
        SilentMasterCommand::SplitAudits => (
            "System: Splitting audits...".to_string(),
            subagents::split_audits_command_prompt(),
        ),
        SilentMasterCommand::MergeAudits => (
            "System: Merging audits...".to_string(),
            subagents::merge_audits_command_prompt(),
        ),
        SilentMasterCommand::SplitTests => (
            "System: Splitting tests...".to_string(),
            subagents::split_tests_command_prompt(),
        ),
        SilentMasterCommand::MergeTests => (
            "System: Merging tests...".to_string(),
            subagents::merge_tests_command_prompt(),
        ),
    };

    app.push_agent_message(status_message);
    let master_prompt = app.prepare_master_prompt(
        &command_prompt,
        &session_store.tasks_file().display().to_string(),
    );
    let with_intro = subagents::build_session_intro_if_needed(
        &master_prompt,
        session_store.session_dir().display().to_string().as_str(),
        &session_store.session_meta_file().display().to_string(),
        project_info_text,
        master_session_intro_needed,
    );
    master_adapter.send_prompt(with_intro);
    app.set_master_in_progress(true);
    *pending_task_write_baseline = capture_tasks_baseline(session_store);

    let size = terminal.size()?;
    let screen = Rect::new(0, 0, size.width, size.height);
    let max_scroll = ui::chat_max_scroll(screen, app);
    app.set_chat_scroll(max_scroll);
    Ok(true)
}

fn is_slash_start_command(message: &str) -> bool {
    let trimmed = message.trim();
    trimmed.starts_with('/') && App::is_start_execution_command(trimmed)
}

fn is_known_slash_command(message: &str) -> bool {
    let trimmed = message.trim();
    if !trimmed.starts_with('/') {
        return false;
    }
    App::is_start_execution_command(trimmed)
        || App::is_planner_mode_command(trimmed)
        || App::is_skip_plan_command(trimmed)
        || App::is_convert_command(trimmed)
        || App::is_quit_command(trimmed)
        || App::is_attach_docs_command(trimmed)
        || App::is_new_master_command(trimmed)
        || App::is_resume_command(trimmed)
        || App::is_split_audits_command(trimmed)
        || App::is_merge_audits_command(trimmed)
        || App::is_split_tests_command(trimmed)
        || App::is_merge_tests_command(trimmed)
        || App::is_add_final_audit_command(trimmed)
        || App::is_remove_final_audit_command(trimmed)
}

fn session_test_command(session_store: &SessionStore) -> Option<String> {
    session_store
        .read_session_meta()
        .ok()
        .and_then(|meta| normalize_test_command(meta.test_command))
}

fn normalize_test_command(value: Option<String>) -> Option<String> {
    value
        .map(|command| command.trim().to_string())
        .filter(|command| !command.is_empty())
}

fn format_internal_master_update(line: &str) -> String {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return "Here's what just happened: a sub-agent completed work.".to_string();
    }
    if trimmed
        .to_lowercase()
        .starts_with("here's what just happened:")
    {
        return trimmed.to_string();
    }
    format!("Here's what just happened: {trimmed}")
}

#[derive(Debug, Default)]
struct LaunchOptions {
    send_file: Option<PathBuf>,
}

fn parse_launch_options<I>(args: I) -> io::Result<LaunchOptions>
where
    I: IntoIterator<Item = String>,
{
    let mut options = LaunchOptions::default();
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--send-file" => {
                let Some(path) = iter.next() else {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "--send-file requires a path argument",
                    ));
                };
                options.send_file = Some(PathBuf::from(path));
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("Unknown argument: {arg}"),
                ));
            }
        }
    }
    Ok(options)
}

#[cfg(test)]
#[path = "../tests/unit/main_launch_tests.rs"]
mod launch_tests;
