use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crossterm::cursor::SetCursorStyle;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::prelude::*;

mod agent;
mod app;
mod deterministic;
mod events;
mod subagents;
mod session_store;
mod text_layout;
mod theme;
mod ui;
mod workflow;

use agent::{AgentEvent, CodexAdapter};
use app::{App, Pane, ResumeSessionOption, RightPaneMode};
use deterministic::TestRunnerAdapter;
use events::AppEvent;
use session_store::{
    PlannerTaskFileEntry, PlannerTaskKindFile, PlannerTaskStatusFile, SessionListEntry,
    SessionStore, TaskFailFileEntry,
};
use theme::Theme;
use workflow::{JobRun, WorkflowFailure, WorkflowFailureKind};

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
    TaskCheck,
}

#[derive(Debug, Clone)]
struct PendingTaskWriteBaseline {
    tasks_json: String,
}

const GLOBAL_RIGHT_SCROLL_LINES: u16 = 5;
const MAX_ADAPTER_EVENTS_PER_LOOP: usize = 128;

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
            let adapter = CodexAdapter::new_persistent();
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
) {
    if let Some(job) = app.start_next_worker_job() {
        dispatch_worker_job(
            &job,
            worker_agent_adapters,
            active_worker_context_key,
            test_runner_adapter,
            session_store,
        );
        app.push_agent_message(format!(
            "System: Starting {:?} for task #{}.",
            job.role, job.top_task_id
        ));
    }
    if let Err(err) = persist_runtime_tasks_snapshot(app, session_store) {
        app.push_agent_message(format!(
            "System: Failed to persist runtime task status to tasks.json: {err}"
        ));
    }
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    mut app: App,
    theme: &Theme,
    cwd: PathBuf,
    startup_message: Option<&str>,
) -> io::Result<()> {
    let mut session_store: Option<SessionStore> = None;
    let master_adapter = CodexAdapter::new_master();
    let master_report_adapter = CodexAdapter::new_master();
    let project_info_adapter = CodexAdapter::new_json_assistant_persistent();
    let mut worker_agent_adapters: HashMap<String, CodexAdapter> = HashMap::new();
    let mut active_worker_context_key: Option<String> = None;
    let docs_attach_adapter = CodexAdapter::new();
    let task_check_adapter = CodexAdapter::new();
    let test_runner_adapter = TestRunnerAdapter::new();
    let mut master_transcript: Vec<String> = Vec::new();
    let mut master_report_transcript: Vec<String> = Vec::new();
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
                    if should_process_master_task_file_updates(app.is_execution_enabled()) {
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
                    active_worker_context_key = None;
                    let new_context_entries = app.on_worker_completed(success, code);
                    let exhausted_failures = app.drain_worker_failures();
                    if !exhausted_failures.is_empty() {
                        handle_exhausted_loop_failures(
                            &mut app,
                            active_session,
                            &master_report_adapter,
                            &mut master_report_session_intro_needed,
                            project_info_text.as_deref(),
                            exhausted_failures,
                        );
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
                        master_report_adapter.send_prompt(subagents::build_session_intro_if_needed(
                            &prompt,
                            active_session.session_dir().display().to_string().as_str(),
                            &active_session.session_meta_file().display().to_string(),
                            project_info_text.as_deref(),
                            &mut master_report_session_intro_needed,
                        ));
                    }
                    start_next_worker_job_if_any(
                        &mut app,
                        &mut worker_agent_adapters,
                        &mut active_worker_context_key,
                        &test_runner_adapter,
                        active_session,
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
                        handle_exhausted_loop_failures(
                            &mut app,
                            active_session,
                            &master_report_adapter,
                            &mut master_report_session_intro_needed,
                            project_info_text.as_deref(),
                            exhausted_failures,
                        );
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
                        master_report_adapter.send_prompt(subagents::build_session_intro_if_needed(
                            &prompt,
                            active_session.session_dir().display().to_string().as_str(),
                            &active_session.session_meta_file().display().to_string(),
                            project_info_text.as_deref(),
                            &mut master_report_session_intro_needed,
                        ));
                    }
                    start_next_worker_job_if_any(
                        &mut app,
                        &mut worker_agent_adapters,
                        &mut active_worker_context_key,
                        &test_runner_adapter,
                        active_session,
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
                            terminal,
                        )?;
                        task_check_in_flight = false;
                        task_check_baseline = None;
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
                            terminal,
                        )?;
                        task_check_in_flight = false;
                        task_check_baseline = None;
                    }
                } else if app.active_pane == Pane::LeftBottom {
                    let pending = app.chat_input().trim().to_string();
                    match submit_block_reason(
                        project_info_in_flight,
                        app.is_task_check_in_progress(),
                        &pending,
                    ) {
                        Some(SubmitBlockReason::ProjectInfoGathering) => {
                            app.push_agent_message(
                                "System: Project context gathering is in progress. Enter/Return submissions are temporarily disabled until it completes.".to_string(),
                            );
                            continue;
                        }
                        Some(SubmitBlockReason::TaskCheck) => {
                            app.push_agent_message(
                                "System: Task checking is in progress. Message and slash commands are temporarily blocked (except /quit and /exit).".to_string(),
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

fn submit_user_message(
    app: &mut App,
    message: String,
    master_adapter: &CodexAdapter,
    master_report_adapter: &CodexAdapter,
    project_info_adapter: &CodexAdapter,
    worker_agent_adapters: &mut HashMap<String, CodexAdapter>,
    active_worker_context_key: &mut Option<String>,
    docs_attach_adapter: &CodexAdapter,
    test_runner_adapter: &TestRunnerAdapter,
    session_store: &mut Option<SessionStore>,
    cwd: &Path,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    pending_task_write_baseline: &mut Option<PendingTaskWriteBaseline>,
    docs_attach_in_flight: &mut bool,
    master_session_intro_needed: &mut bool,
    master_report_session_intro_needed: &mut bool,
    pending_master_message_after_project_info: &mut Option<String>,
    project_info_in_flight: &mut bool,
    project_info_stage: &mut Option<ProjectInfoStage>,
    project_info_text: &mut Option<String>,
) -> io::Result<()> {
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
        master_adapter.reset_session();
        master_report_adapter.reset_session();
        project_info_adapter.reset_session();
        worker_agent_adapters.clear();
        *active_worker_context_key = None;
        *master_session_intro_needed = true;
        *master_report_session_intro_needed = true;
        *project_info_stage = None;
        *project_info_in_flight = false;
        app.set_master_in_progress(false);
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
        if let Some(job) = app.start_next_worker_job() {
            dispatch_worker_job(
                &job,
                worker_agent_adapters,
                active_worker_context_key,
                test_runner_adapter,
                active_session,
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

#[allow(clippy::too_many_arguments)]
fn resume_session(
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
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> io::Result<()> {
    let cwd = std::env::current_dir()?;
    let selected_path = PathBuf::from(&selection.session_dir);
    let resumed = SessionStore::open_existing(&cwd, &selected_path)?;
    *session_store = Some(resumed);
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

    app.reset_execution_for_session_switch();
    app.set_task_check_in_progress(false);
    app.set_docs_attach_in_progress(false);
    app.set_master_in_progress(false);

    let rolling = active_session.read_rolling_context().unwrap_or_default();
    app.replace_rolling_context_entries(rolling);

    match active_session.read_tasks() {
        Ok(tasks) => {
            let pane_mode = resumed_right_pane_mode(&tasks);
            match app.sync_planner_tasks_from_file(tasks) {
                Ok(()) => {
                    app.set_right_pane_mode(pane_mode);
                }
                Err(err) => app.push_agent_message(format!(
                    "System: Failed to refresh task tree from resumed tasks.json: {err}"
                )),
            }
        }
        Err(err) => app.push_agent_message(format!("System: Failed to read tasks.json: {err}")),
    }
    if let Ok(markdown) = active_session.read_planner_markdown() {
        app.set_planner_markdown(markdown);
    }

    *project_info_text = active_session
        .read_project_info()
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

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

fn handle_final_audit_command(
    app: &mut App,
    message: &str,
    session_store: &SessionStore,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> io::Result<bool> {
    if App::is_add_final_audit_command(message) {
        let mut tasks = session_store.read_tasks().unwrap_or_default();
        ensure_final_audit_task(&mut tasks);
        normalize_root_orders_with_final_last(&mut tasks);
        let text = serde_json::to_string_pretty(&tasks).map_err(io::Error::other)?;
        std::fs::write(session_store.tasks_file(), text)?;
        match app.sync_planner_tasks_from_file(tasks) {
            Ok(_) => app.push_agent_message("System: Added final audit task.".to_string()),
            Err(err) => app.push_agent_message(format!(
                "System: Final audit task was written, but refresh failed: {err}"
            )),
        }
        let size = terminal.size()?;
        let screen = Rect::new(0, 0, size.width, size.height);
        let max_scroll = ui::chat_max_scroll(screen, app);
        app.set_chat_scroll(max_scroll);
        return Ok(true);
    }

    if App::is_remove_final_audit_command(message) {
        let mut tasks = session_store.read_tasks().unwrap_or_default();
        let before = tasks.len();
        tasks.retain(|task| task.kind != PlannerTaskKindFile::FinalAudit);
        normalize_root_orders_with_final_last(&mut tasks);
        let text = serde_json::to_string_pretty(&tasks).map_err(io::Error::other)?;
        std::fs::write(session_store.tasks_file(), text)?;
        match app.sync_planner_tasks_from_file(tasks) {
            Ok(_) => {
                if before == 0 {
                    app.push_agent_message("System: No final audit task was present.".to_string());
                } else {
                    app.push_agent_message("System: Removed final audit task.".to_string());
                }
            }
            Err(err) => app.push_agent_message(format!(
                "System: Final audit removal was written, but refresh failed: {err}"
            )),
        }
        let size = terminal.size()?;
        let screen = Rect::new(0, 0, size.width, size.height);
        let max_scroll = ui::chat_max_scroll(screen, app);
        app.set_chat_scroll(max_scroll);
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
    master_report_adapter: &CodexAdapter,
    master_report_session_intro_needed: &mut bool,
    project_info_text: Option<&str>,
    failures: Vec<WorkflowFailure>,
) {
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
        return;
    }

    let has_test_failure = fail_entries.iter().any(|entry| entry.kind == "test");
    let prompt = subagents::build_failure_report_prompt(
        &session_store.task_fails_file().display().to_string(),
        &fail_entries,
        has_test_failure,
    );
    master_report_adapter.send_prompt(subagents::build_session_intro_if_needed(
        &prompt,
        session_store.session_dir().display().to_string().as_str(),
        &session_store.session_meta_file().display().to_string(),
        project_info_text,
        master_report_session_intro_needed,
    ));
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
    !trimmed.starts_with('/')
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

fn submit_block_reason(
    project_info_in_flight: bool,
    task_check_in_progress: bool,
    message: &str,
) -> Option<SubmitBlockReason> {
    if project_info_in_flight {
        return Some(SubmitBlockReason::ProjectInfoGathering);
    }
    if task_check_in_progress && !is_allowed_during_task_check(message) {
        return Some(SubmitBlockReason::TaskCheck);
    }
    None
}

fn should_start_task_check(
    changed_tasks: bool,
    task_check_in_flight: bool,
    docs_attach_in_flight: bool,
) -> bool {
    changed_tasks && !task_check_in_flight && !docs_attach_in_flight
}

fn should_process_master_task_file_updates(execution_enabled: bool) -> bool {
    !execution_enabled
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
fn handle_silent_master_command(
    app: &mut App,
    message: &str,
    master_adapter: &CodexAdapter,
    session_store: &SessionStore,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
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
mod launch_tests {
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
            submit_block_reason(true, true, "/quit"),
            Some(SubmitBlockReason::ProjectInfoGathering)
        );
        assert_eq!(
            submit_block_reason(false, true, "hello"),
            Some(SubmitBlockReason::TaskCheck)
        );
        assert_eq!(submit_block_reason(false, true, "/quit"), None);
        assert_eq!(submit_block_reason(false, false, "hello"), None);
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
                            test_audit_failures_left =
                                test_audit_failures_left.saturating_sub(1);
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
        let job = reloaded.start_next_worker_job().expect("final audit should run");
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
        let prompt = subagents::build_convert_plan_prompt(
            "/tmp/session/planner.md",
            "/tmp/session/tasks.json",
        );
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
        assert!(
            prompt.contains("Ensure every test_writer has at least one direct test_runner child")
        );
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
}
