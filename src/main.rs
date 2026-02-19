use std::collections::{HashMap, HashSet, VecDeque};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use clap::{Args, Parser, Subcommand, ValueEnum};
use crossterm::cursor::SetCursorStyle;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::prelude::*;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

mod agent;
mod agent_models;
mod api;
mod app;
mod artifact_io;
mod default_config;
mod deterministic;
mod events;
mod services;
mod session_store;
mod subagents;
mod text_layout;
mod theme;
mod ui;
mod workflow;

use agent::{AdapterOutputMode, AgentEvent, BackendKind, CodexAdapter, CodexCommandConfig};
use agent_models::{CodexAgentKind, CodexAgentModelRouting, CodexModelProfile};
use app::{App, BackendOption, Pane, ResumeSessionOption, RightPaneMode};
use artifact_io::{ensure_default_metaagent_config, load_merged_metaagent_config_text};
use deterministic::TestRunnerAdapter;
use events::AppEvent;
use services::{
    CoreOrchestrationService, DefaultCoreOrchestrationService, DefaultUiPromptService,
    TaskWriteBaseline, UiPromptService,
};
use session_store::{
    PlannerTaskFileEntry, PlannerTaskKindFile, PlannerTaskStatusFile, SessionListEntry,
    SessionStore, TaskFailFileEntry,
};
use theme::Theme;
#[cfg(test)]
use workflow::JobRun;

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

const GLOBAL_RIGHT_SCROLL_LINES: u16 = 5;
const MAX_ADAPTER_EVENTS_PER_LOOP: usize = 32;
const UI_TICK_INTERVAL: Duration = Duration::from_millis(120);
#[cfg(test)]
type PendingTaskWriteBaseline = TaskWriteBaseline;

fn apply_codex_profile(config: &mut CodexCommandConfig, profile: &CodexModelProfile) {
    if !matches!(config.backend_kind(), BackendKind::Codex) {
        return;
    }
    config.model = Some(profile.model.clone());
    config.model_reasoning_effort = profile.thinking_effort.clone();
}

fn base_command_config_for_backend(
    model_routing: &CodexAgentModelRouting,
    selected_backend: BackendKind,
) -> CodexCommandConfig {
    let config = model_routing.base_command_config();
    if config.backend_kind() == selected_backend {
        config
    } else {
        CodexCommandConfig::default_for_backend(selected_backend)
    }
}

fn build_codex_adapter(
    model_routing: &CodexAgentModelRouting,
    selected_backend: BackendKind,
    output_mode: AdapterOutputMode,
    persistent_session: bool,
    profile: &CodexModelProfile,
) -> CodexAdapter {
    let mut config = base_command_config_for_backend(model_routing, selected_backend);
    if matches!(output_mode, AdapterOutputMode::JsonAssistantOnly)
        && matches!(config.backend_kind(), BackendKind::Codex)
    {
        config.args_prefix.push("--json".to_string());
    }
    config.output_mode = output_mode;
    config.persistent_session = persistent_session;
    apply_codex_profile(&mut config, profile);
    CodexAdapter::with_config(config)
}

fn build_json_persistent_adapter(
    model_routing: &CodexAgentModelRouting,
    selected_backend: BackendKind,
    kind: CodexAgentKind,
) -> CodexAdapter {
    let profile = model_routing.profile_for(kind);
    build_codex_adapter(
        model_routing,
        selected_backend,
        AdapterOutputMode::JsonAssistantOnly,
        true,
        &profile,
    )
}

fn build_plain_adapter(
    model_routing: &CodexAgentModelRouting,
    selected_backend: BackendKind,
    kind: CodexAgentKind,
    persistent_session: bool,
) -> CodexAdapter {
    let profile = model_routing.profile_for(kind);
    build_codex_adapter(
        model_routing,
        selected_backend,
        AdapterOutputMode::PlainText,
        persistent_session,
        &profile,
    )
}

fn main() -> io::Result<()> {
    let launch_options = parse_launch_options(std::env::args().skip(1))?;
    if let Some(command) = launch_options.command {
        let exit_code =
            run_cli_command(command, launch_options.output_mode, launch_options.verbose);
        std::process::exit(exit_code);
    }
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

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    mut app: App,
    theme: &Theme,
    cwd: PathBuf,
    startup_message: Option<&str>,
) -> io::Result<()> {
    let orchestration_service = DefaultCoreOrchestrationService;
    let prompt_service = DefaultUiPromptService;

    let mut session_store: Option<SessionStore> = None;
    let mut model_routing = match CodexAgentModelRouting::load_from_metaagent_config() {
        Ok(config) => config,
        Err(err) => {
            app.push_agent_message(format!(
                "System: Failed to load model profile config from ~/.metaagent/config.toml: {err}. Using defaults."
            ));
            CodexAgentModelRouting::default()
        }
    };
    let mut selected_backend = model_routing.base_command_config().backend_kind();
    let mut master_adapter = build_json_persistent_adapter(
        &model_routing,
        selected_backend,
        CodexAgentKind::Master,
    );
    let mut master_report_adapter =
        build_json_persistent_adapter(&model_routing, selected_backend, CodexAgentKind::MasterReport);
    let mut project_info_adapter =
        build_json_persistent_adapter(&model_routing, selected_backend, CodexAgentKind::ProjectInfo);
    let mut worker_agent_adapters: HashMap<String, CodexAdapter> = HashMap::new();
    let mut active_worker_context_key: Option<String> = None;
    let mut docs_attach_adapter =
        build_plain_adapter(&model_routing, selected_backend, CodexAgentKind::DocsAttach, false);
    let mut task_check_adapter =
        build_plain_adapter(&model_routing, selected_backend, CodexAgentKind::TaskCheck, false);
    let test_runner_adapter = TestRunnerAdapter::new();
    let mut master_transcript: Vec<String> = Vec::new();
    let mut master_report_transcript: Vec<String> = Vec::new();
    let mut master_report_in_flight = false;
    let mut pending_master_report_prompts: VecDeque<String> = VecDeque::new();
    let mut project_info_transcript: Vec<String> = Vec::new();
    let mut pending_task_write_baseline: Option<TaskWriteBaseline> = None;
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
        submit_user_message_with_runtime(
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
            &mut model_routing,
            &mut selected_backend,
        )?;
    }

    let mut needs_draw = true;
    let mut last_ui_tick = Instant::now();
    while app.running {
        let input_pending = events::has_pending_input()?;
        let mut chat_updated = false;

        if !input_pending {
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
                        if should_clear_task_write_baseline(
                            tasks_refresh_ok,
                            requested_task_file_retry,
                        ) {
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
                            match orchestration_service.start_next_worker_job_if_any(
                            &mut app,
                            &mut worker_agent_adapters,
                            &mut active_worker_context_key,
                            &test_runner_adapter,
                            active_session,
                            &model_routing,
                        ) {
                            Ok(Some(job)) => app.push_agent_message(format!(
                                "System: Starting {:?} for task #{}.",
                                job.role, job.top_task_id
                            )),
                            Ok(None) => {}
                            Err(err) => app.push_agent_message(format!(
                                "System: Failed to persist runtime task status to tasks.json: {err}"
                            )),
                        }
                        }
                        chat_updated = true;
                    }
                }
            }
        }

        if !input_pending {
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
                        let outcome = orchestration_service.complete_worker_cycle_and_start_next(
                            &mut app,
                            success,
                            code,
                            &mut worker_agent_adapters,
                            &mut active_worker_context_key,
                            &test_runner_adapter,
                            active_session,
                            &model_routing,
                            &mut master_report_session_intro_needed,
                            project_info_text.as_deref(),
                        );
                        for warning in outcome.warnings {
                            app.push_agent_message(format!("System: {warning}"));
                        }
                        for prompt in [outcome.failure_report_prompt, outcome.context_report_prompt]
                            .into_iter()
                            .flatten()
                        {
                            if let Some(prompt_to_send) = enqueue_or_dispatch_master_report_prompt(
                                prompt,
                                &mut master_report_in_flight,
                                &mut pending_master_report_prompts,
                            ) {
                                master_report_transcript.clear();
                                master_report_adapter.send_prompt(prompt_to_send);
                            }
                        }
                        if let Some(job) = outcome.started_job {
                            app.push_agent_message(format!(
                                "System: Starting {:?} for task #{}.",
                                job.role, job.top_task_id
                            ));
                        }
                        chat_updated = true;
                    }
                }
            }
        }

        if !input_pending {
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
                        let outcome = orchestration_service.complete_worker_cycle_and_start_next(
                            &mut app,
                            success,
                            code,
                            &mut worker_agent_adapters,
                            &mut active_worker_context_key,
                            &test_runner_adapter,
                            active_session,
                            &model_routing,
                            &mut master_report_session_intro_needed,
                            project_info_text.as_deref(),
                        );
                        for warning in outcome.warnings {
                            app.push_agent_message(format!("System: {warning}"));
                        }
                        for prompt in [outcome.failure_report_prompt, outcome.context_report_prompt]
                            .into_iter()
                            .flatten()
                        {
                            if let Some(prompt_to_send) = enqueue_or_dispatch_master_report_prompt(
                                prompt,
                                &mut master_report_in_flight,
                                &mut pending_master_report_prompts,
                            ) {
                                master_report_transcript.clear();
                                master_report_adapter.send_prompt(prompt_to_send);
                            }
                        }
                        if let Some(job) = outcome.started_job {
                            app.push_agent_message(format!(
                                "System: Starting {:?} for task #{}.",
                                job.role, job.top_task_id
                            ));
                        }
                        chat_updated = true;
                    }
                }
            }
        }

        if !input_pending {
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
        }

        if !input_pending {
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
                                        if let Err(err) =
                                            active_session.write_project_info(&markdown)
                                        {
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
                                            &active_session
                                                .session_meta_file()
                                                .display()
                                                .to_string(),
                                        );
                                        project_info_adapter.send_prompt(meta_prompt);
                                        project_info_stage =
                                            Some(ProjectInfoStage::WritingSessionMeta);
                                        app.push_subagent_output(
                                            "ProjectInfoSystem: Writing session meta.json"
                                                .to_string(),
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

                        if let Some(pending_message) =
                            pending_master_message_after_project_info.take()
                        {
                            let with_intro = prompt_service.build_master_prompt_for_message(
                                &app,
                                &pending_message,
                                active_session,
                                project_info_text.as_deref(),
                                &mut master_session_intro_needed,
                            );
                            master_adapter.send_prompt(with_intro);
                            app.set_master_in_progress(true);
                            pending_task_write_baseline =
                                orchestration_service.capture_tasks_baseline(active_session);
                        }

                        project_info_transcript.clear();
                        chat_updated = true;
                    }
                }
            }
        }

        if !input_pending {
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
        }
        if !input_pending {
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
                        let changed = match (task_check_baseline.as_deref(), after_text.as_deref())
                        {
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
        }
        if chat_updated {
            let size = terminal.size()?;
            let screen = Rect::new(0, 0, size.width, size.height);
            let max_scroll = ui::chat_max_scroll(screen, &app);
            app.set_chat_scroll(max_scroll);
            needs_draw = true;
        }

        let app_event = events::next_event()?;
        if !matches!(app_event, AppEvent::Tick) {
            needs_draw = true;
        }
        match app_event {
            AppEvent::Tick => {
                if last_ui_tick.elapsed() >= UI_TICK_INTERVAL {
                    app.on_tick();
                    last_ui_tick = Instant::now();
                    needs_draw = true;
                }
            }
            AppEvent::Quit => app.quit(),
            AppEvent::NextPane => {
                if is_picker_open(&app) {
                    // ignore pane focus changes while a picker is open
                } else if app.active_pane == Pane::LeftBottom && app.autocomplete_top_command() {
                    // keep focus in input when command autocomplete is applied
                } else {
                    app.next_pane();
                }
            }
            AppEvent::PrevPane => {
                if !is_picker_open(&app) {
                    app.prev_pane();
                }
            }
            AppEvent::MoveUp => {
                if app.is_resume_picker_open() {
                    app.resume_picker_move_up();
                } else if app.is_backend_picker_open() {
                    app.backend_picker_move_up();
                } else if app.active_pane == Pane::LeftBottom {
                    let size = terminal.size()?;
                    let width = ui::chat_input_text_width(Rect::new(0, 0, size.width, size.height));
                    app.move_cursor_up(width);
                } else if app.active_pane == Pane::Right {
                    if app.is_planner_mode() {
                        let size = terminal.size()?;
                        let screen = Rect::new(0, 0, size.width, size.height);
                        let (width, visible_lines) = ui::planner_editor_metrics(screen);
                        let max_scroll = ui::right_max_scroll(screen, &app);
                        app.planner_move_cursor_up(width);
                        app.ensure_planner_cursor_visible(width, visible_lines, max_scroll);
                    } else {
                        app.scroll_right_up();
                    }
                } else {
                    app.scroll_up();
                }
            }
            AppEvent::MoveDown => {
                if app.is_resume_picker_open() {
                    app.resume_picker_move_down();
                } else if app.is_backend_picker_open() {
                    app.backend_picker_move_down();
                } else if app.active_pane == Pane::LeftBottom {
                    let size = terminal.size()?;
                    let width = ui::chat_input_text_width(Rect::new(0, 0, size.width, size.height));
                    app.move_cursor_down(width);
                } else if app.active_pane == Pane::Right {
                    let size = terminal.size()?;
                    let screen = Rect::new(0, 0, size.width, size.height);
                    if app.is_planner_mode() {
                        let (width, visible_lines) = ui::planner_editor_metrics(screen);
                        let max_scroll = ui::right_max_scroll(screen, &app);
                        app.planner_move_cursor_down(width);
                        app.ensure_planner_cursor_visible(width, visible_lines, max_scroll);
                    } else {
                        let max_scroll = ui::right_max_scroll(screen, &app);
                        app.scroll_right_down(max_scroll);
                    }
                } else {
                    let size = terminal.size()?;
                    let screen = Rect::new(0, 0, size.width, size.height);
                    let max_scroll = ui::left_top_max_scroll(screen, &app);
                    app.scroll_left_top_down(max_scroll);
                }
            }
            AppEvent::CursorLeft => {
                if is_picker_open(&app) {
                    // ignore cursor movement while a picker is open
                } else if app.active_pane == Pane::LeftBottom {
                    app.move_cursor_left();
                } else if app.active_pane == Pane::Right && app.is_planner_mode() {
                    let size = terminal.size()?;
                    let screen = Rect::new(0, 0, size.width, size.height);
                    let (width, visible_lines) = ui::planner_editor_metrics(screen);
                    let max_scroll = ui::right_max_scroll(screen, &app);
                    app.planner_move_cursor_left();
                    app.ensure_planner_cursor_visible(width, visible_lines, max_scroll);
                }
            }
            AppEvent::CursorRight => {
                if is_picker_open(&app) {
                    // ignore cursor movement while a picker is open
                } else if app.active_pane == Pane::LeftBottom {
                    app.move_cursor_right();
                } else if app.active_pane == Pane::Right && app.is_planner_mode() {
                    let size = terminal.size()?;
                    let screen = Rect::new(0, 0, size.width, size.height);
                    let (width, visible_lines) = ui::planner_editor_metrics(screen);
                    let max_scroll = ui::right_max_scroll(screen, &app);
                    app.planner_move_cursor_right();
                    app.ensure_planner_cursor_visible(width, visible_lines, max_scroll);
                }
            }
            AppEvent::ScrollChatUp => {
                if app.is_resume_picker_open() {
                    app.resume_picker_move_up();
                } else if app.is_backend_picker_open() {
                    app.backend_picker_move_up();
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
                } else if app.is_backend_picker_open() {
                    app.backend_picker_move_down();
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
                    let size = terminal.size()?;
                    let screen = Rect::new(0, 0, size.width, size.height);
                    let max_scroll = ui::left_top_max_scroll(screen, &app);
                    app.scroll_left_top_down(max_scroll);
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
                } else if app.is_backend_picker_open() {
                    if c == ' '
                        && let Some(selection) = app.select_backend_option()
                    {
                        apply_backend_selection(
                            &mut app,
                            selection,
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
                    }
                } else if app.active_pane == Pane::LeftBottom {
                    app.input_char(c);
                } else if app.active_pane == Pane::Right && app.is_planner_mode() {
                    let size = terminal.size()?;
                    let screen = Rect::new(0, 0, size.width, size.height);
                    let (width, visible_lines) = ui::planner_editor_metrics(screen);
                    app.planner_input_char(c);
                    let max_scroll = ui::right_max_scroll(screen, &app);
                    app.ensure_planner_cursor_visible(width, visible_lines, max_scroll);
                    persist_planner_markdown_if_possible(&mut app, session_store.as_ref());
                } else if c == 'j' {
                    if app.active_pane == Pane::Right {
                        let size = terminal.size()?;
                        let screen = Rect::new(0, 0, size.width, size.height);
                        let max_scroll = ui::right_max_scroll(screen, &app);
                        app.scroll_right_down(max_scroll);
                    } else {
                        let size = terminal.size()?;
                        let screen = Rect::new(0, 0, size.width, size.height);
                        let max_scroll = ui::left_top_max_scroll(screen, &app);
                        app.scroll_left_top_down(max_scroll);
                    }
                } else if c == 'k' {
                    if app.active_pane == Pane::Right {
                        app.scroll_right_up();
                    } else {
                        app.scroll_up();
                    }
                }
            }
            AppEvent::Backspace => {
                if app.is_resume_picker_open() {
                    app.open_resume_picker(Vec::new());
                    app.push_agent_message("System: Resume picker cancelled.".to_string());
                } else if app.is_backend_picker_open() {
                    app.open_backend_picker(Vec::new());
                    app.push_agent_message("System: Backend picker cancelled.".to_string());
                } else if app.active_pane == Pane::LeftBottom {
                    app.backspace_input();
                } else if app.active_pane == Pane::Right
                    && app.is_planner_mode()
                {
                    let size = terminal.size()?;
                    let screen = Rect::new(0, 0, size.width, size.height);
                    let (width, visible_lines) = ui::planner_editor_metrics(screen);
                    app.planner_backspace();
                    let max_scroll = ui::right_max_scroll(screen, &app);
                    app.ensure_planner_cursor_visible(width, visible_lines, max_scroll);
                    persist_planner_markdown_if_possible(&mut app, session_store.as_ref());
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
                } else if app.is_backend_picker_open() {
                    if let Some(selection) = app.select_backend_option() {
                        apply_backend_selection(
                            &mut app,
                            selection,
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
                    }
                } else if app.active_pane == Pane::Right && app.is_planner_mode() {
                    let size = terminal.size()?;
                    let screen = Rect::new(0, 0, size.width, size.height);
                    let (width, visible_lines) = ui::planner_editor_metrics(screen);
                    app.planner_insert_newline();
                    let max_scroll = ui::right_max_scroll(screen, &app);
                    app.ensure_planner_cursor_visible(width, visible_lines, max_scroll);
                    persist_planner_markdown_if_possible(&mut app, session_store.as_ref());
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
                            submit_user_message_with_runtime(
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
                                &mut model_routing,
                                &mut selected_backend,
                            )?;
                        }
                    } else if let Some(message) = app.submit_chat_message() {
                        submit_user_message_with_runtime(
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
                            &mut model_routing,
                            &mut selected_backend,
                        )?;
                    }
                }
            }
            AppEvent::MouseScrollUp => {
                if app.is_resume_picker_open() {
                    app.resume_picker_move_up();
                } else if app.is_backend_picker_open() {
                    app.backend_picker_move_up();
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
                } else if app.is_backend_picker_open() {
                    app.backend_picker_move_down();
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
                    let size = terminal.size()?;
                    let screen = Rect::new(0, 0, size.width, size.height);
                    let max_scroll = ui::left_top_max_scroll(screen, &app);
                    app.scroll_left_top_down(max_scroll);
                }
            }
            AppEvent::MouseLeftClick(column, row) => {
                if !is_picker_open(&app) {
                    let size = terminal.size()?;
                    let screen = Rect::new(0, 0, size.width, size.height);
                    if let Some(pane) = ui::pane_hit_test(screen, column, row) {
                        app.active_pane = pane;
                    }
                    if app.is_planner_mode() {
                        if let Some(cursor) = ui::planner_cursor_hit_test(screen, &app, column, row)
                        {
                            app.set_planner_cursor(cursor);
                        }
                    } else if let Some(task_key) =
                        ui::right_pane_toggle_hit_test(screen, &app, column, row)
                    {
                        app.toggle_task_details(&task_key);
                    }
                }
            }
        }

        if needs_draw && !events::has_pending_input()? {
            terminal.draw(|frame| ui::render(frame, &app, theme))?;
            needs_draw = false;
        }
    }

    Ok(())
}

fn submit_user_message_with_runtime<B: Backend>(
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
    pending_task_write_baseline: &mut Option<TaskWriteBaseline>,
    docs_attach_in_flight: &mut bool,
    master_session_intro_needed: &mut bool,
    master_report_session_intro_needed: &mut bool,
    pending_master_message_after_project_info: &mut Option<String>,
    project_info_in_flight: &mut bool,
    project_info_stage: &mut Option<ProjectInfoStage>,
    project_info_text: &mut Option<String>,
    model_routing: &mut CodexAgentModelRouting,
    selected_backend: &mut BackendKind,
) -> io::Result<()> {
    let orchestration_service = DefaultCoreOrchestrationService;
    let prompt_service = DefaultUiPromptService;

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

    if *docs_attach_in_flight
        && (App::is_new_master_command(&message) || App::is_resume_command(&message))
    {
        app.push_agent_message(
            "System: Documentation attach is still running. Wait for it to finish before switching sessions."
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
                let options = build_resume_options(sessions, current_session_dir, Some(cwd));
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

    if is_backend_command(&message) {
        let options = backend_picker_options(*selected_backend);
        app.open_backend_picker(options);
        app.push_agent_message(
            "System: Select a backend in the picker and press Enter or Space (Backspace cancels)."
                .to_string(),
        );
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

        let with_intro = prompt_service.build_convert_master_prompt(
            app,
            active_session,
            project_info_text.as_deref(),
            master_session_intro_needed,
        );
        master_adapter.send_prompt(with_intro);
        app.set_master_in_progress(true);
        *pending_task_write_baseline = orchestration_service.capture_tasks_baseline(active_session);

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
            let with_intro = prompt_service.build_master_prompt_for_message(
                app,
                &message,
                active_session,
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
        match orchestration_service.start_next_worker_job_if_any(
            app,
            worker_agent_adapters,
            active_worker_context_key,
            test_runner_adapter,
            active_session,
            model_routing,
        ) {
            Ok(Some(job)) => app.push_agent_message(format!(
                "System: Starting {:?} for task #{}.",
                job.role, job.top_task_id
            )),
            Ok(None) => {}
            Err(err) => app.push_agent_message(format!(
                "System: Failed to persist runtime task status to tasks.json: {err}"
            )),
        }
    } else {
        *pending_task_write_baseline = session_store
            .as_ref()
            .and_then(|store| orchestration_service.capture_tasks_baseline(store));
    }
    let size = terminal.size()?;
    let screen = Rect::new(0, 0, size.width, size.height);
    let max_scroll = ui::chat_max_scroll(screen, app);
    app.set_chat_scroll(max_scroll);
    Ok(())
}

#[allow(dead_code)]
#[allow(clippy::too_many_arguments)]
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
    pending_task_write_baseline: &mut Option<TaskWriteBaseline>,
    docs_attach_in_flight: &mut bool,
    master_session_intro_needed: &mut bool,
    master_report_session_intro_needed: &mut bool,
    pending_master_message_after_project_info: &mut Option<String>,
    project_info_in_flight: &mut bool,
    project_info_stage: &mut Option<ProjectInfoStage>,
    project_info_text: &mut Option<String>,
    model_routing: &CodexAgentModelRouting,
) -> io::Result<()> {
    let mut routing = model_routing.clone();
    let mut selected_backend = routing.base_command_config().backend_kind();
    submit_user_message_with_runtime(
        app,
        message,
        master_adapter,
        master_report_adapter,
        project_info_adapter,
        worker_agent_adapters,
        active_worker_context_key,
        docs_attach_adapter,
        test_runner_adapter,
        master_report_in_flight,
        pending_master_report_prompts,
        master_report_transcript,
        task_check_in_flight,
        task_check_baseline,
        session_store,
        cwd,
        terminal,
        pending_task_write_baseline,
        docs_attach_in_flight,
        master_session_intro_needed,
        master_report_session_intro_needed,
        pending_master_message_after_project_info,
        project_info_in_flight,
        project_info_stage,
        project_info_text,
        &mut routing,
        &mut selected_backend,
    )
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
    pending_task_write_baseline: &mut Option<TaskWriteBaseline>,
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
    current_workspace: Option<&Path>,
) -> Vec<ResumeSessionOption> {
    sessions
        .into_iter()
        .filter(|entry| {
            current_session_dir
                .map(|dir| entry.session_dir != dir)
                .unwrap_or(true)
        })
        .filter(|entry| {
            current_workspace
                .map(|workspace| Path::new(entry.workspace.trim()) == workspace)
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

    let Some(baseline_text) = baseline_tasks_json else {
        return false;
    };
    let Ok(baseline_entries) = serde_json::from_str::<Vec<PlannerTaskFileEntry>>(baseline_text)
    else {
        return false;
    };
    let baseline_docs = baseline_entries
        .into_iter()
        .map(|entry| (entry.id, entry.docs))
        .collect::<HashMap<_, _>>();

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

#[cfg(test)]
fn persist_runtime_tasks_snapshot(app: &App, session_store: &SessionStore) -> io::Result<()> {
    let tasks = app.planner_tasks_for_file();
    let text = serde_json::to_string_pretty(&tasks).map_err(io::Error::other)?;
    std::fs::write(session_store.tasks_file(), text)
}

#[cfg(test)]
fn claim_next_worker_job_and_persist_snapshot(
    app: &mut App,
    session_store: &SessionStore,
) -> Option<workflow::StartedJob> {
    let job = app.start_next_worker_job();
    if let Err(err) = persist_runtime_tasks_snapshot(app, session_store) {
        app.push_agent_message(format!(
            "System: Failed to persist runtime task status to tasks.json: {err}"
        ));
    }
    job
}

fn persist_planner_markdown_if_possible(app: &mut App, session_store: Option<&SessionStore>) {
    let Some(session_store) = session_store else {
        return;
    };
    if let Err(err) = session_store.write_planner_markdown(app.planner_markdown()) {
        app.push_agent_message(format!(
            "System: Failed to write planner markdown to planner.md: {err}"
        ));
    }
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

fn is_picker_open(app: &App) -> bool {
    app.is_resume_picker_open() || app.is_backend_picker_open()
}

fn backend_label(kind: BackendKind) -> &'static str {
    match kind {
        BackendKind::Codex => "codex",
        BackendKind::Claude => "claude",
    }
}

fn backend_picker_options(selected_backend: BackendKind) -> Vec<BackendOption> {
    let mut options = vec![
        BackendOption {
            kind: BackendKind::Codex,
            label: "Codex",
            description: "OpenAI Codex backend",
        },
        BackendOption {
            kind: BackendKind::Claude,
            label: "Claude",
            description: "Anthropic Claude backend",
        },
    ];
    if selected_backend == BackendKind::Claude {
        options.swap(0, 1);
    }
    options
}

fn update_backend_selected_in_toml(
    text: &str,
    selected_backend: BackendKind,
) -> io::Result<String> {
    let mut value = if text.trim().is_empty() {
        toml::Value::Table(toml::map::Map::new())
    } else {
        toml::from_str::<toml::Value>(text)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?
    };
    let Some(table) = value.as_table_mut() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "config root is not a TOML table",
        ));
    };
    let backend = table
        .entry("backend")
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    let Some(backend_table) = backend.as_table_mut() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "backend section is not a TOML table",
        ));
    };
    backend_table.insert(
        "selected".to_string(),
        toml::Value::String(backend_label(selected_backend).to_string()),
    );
    toml::to_string_pretty(&value).map_err(io::Error::other)
}

fn persist_backend_selection(selected_backend: BackendKind) -> io::Result<std::path::PathBuf> {
    let config_file = ensure_default_metaagent_config()?;
    let existing = std::fs::read_to_string(&config_file).unwrap_or_default();
    let updated = update_backend_selected_in_toml(&existing, selected_backend)?;
    std::fs::write(&config_file, updated)?;
    Ok(config_file)
}

fn rebuild_runtime_adapters(
    model_routing: &CodexAgentModelRouting,
    selected_backend: BackendKind,
    master_adapter: &mut CodexAdapter,
    master_report_adapter: &mut CodexAdapter,
    project_info_adapter: &mut CodexAdapter,
    docs_attach_adapter: &mut CodexAdapter,
    task_check_adapter: &mut CodexAdapter,
    active_worker_context_key: &mut Option<String>,
    worker_agent_adapters: &mut HashMap<String, CodexAdapter>,
) {
    *master_adapter = build_json_persistent_adapter(model_routing, selected_backend, CodexAgentKind::Master);
    *master_report_adapter = build_json_persistent_adapter(
        model_routing,
        selected_backend,
        CodexAgentKind::MasterReport,
    );
    *project_info_adapter = build_json_persistent_adapter(
        model_routing,
        selected_backend,
        CodexAgentKind::ProjectInfo,
    );
    *docs_attach_adapter = build_plain_adapter(
        model_routing,
        selected_backend,
        CodexAgentKind::DocsAttach,
        false,
    );
    *task_check_adapter = build_plain_adapter(
        model_routing,
        selected_backend,
        CodexAgentKind::TaskCheck,
        false,
    );
    worker_agent_adapters.clear();
    *active_worker_context_key = None;
}

fn rebuild_model_routing_with_backend_selection(
    model_routing: &mut CodexAgentModelRouting,
    selected_backend: BackendKind,
) -> io::Result<()> {
    let merged = load_merged_metaagent_config_text().unwrap_or_default();
    let updated = update_backend_selected_in_toml(&merged, selected_backend)?;
    *model_routing = CodexAgentModelRouting::from_toml_str(&updated)?;
    Ok(())
}

fn apply_backend_selection(
    app: &mut App,
    selected: BackendOption,
    selected_backend: &mut BackendKind,
    model_routing: &mut CodexAgentModelRouting,
    active_worker_context_key: &mut Option<String>,
    worker_agent_adapters: &mut HashMap<String, CodexAdapter>,
    master_adapter: &mut CodexAdapter,
    master_report_adapter: &mut CodexAdapter,
    project_info_adapter: &mut CodexAdapter,
    docs_attach_adapter: &mut CodexAdapter,
    task_check_adapter: &mut CodexAdapter,
) {
    let target = selected.kind;
    let was = *selected_backend;
    if was == target {
        app.push_agent_message(format!("System: Backend remains {}.", selected.label));
        return;
    }
    *selected_backend = target;
    if let Err(err) = rebuild_model_routing_with_backend_selection(model_routing, target) {
        app.push_agent_message(format!(
            "System: Backend switched to {} in memory, but model config reload failed: {err}. Using fallback defaults for future adapters.",
            selected.label
        ));
        *model_routing = CodexAgentModelRouting::from_toml_str(&format!(
            "[backend]\nselected = \"{}\"\n",
            backend_label(target)
        ))
        .unwrap_or_default();
    }
    rebuild_runtime_adapters(
        model_routing,
        target,
        master_adapter,
        master_report_adapter,
        project_info_adapter,
        docs_attach_adapter,
        task_check_adapter,
        active_worker_context_key,
        worker_agent_adapters,
    );

    match persist_backend_selection(target) {
        Ok(config_file) => app.push_agent_message(format!(
            "System: Backend set to {}. Saved to {}. New adapters will use this backend.",
            selected.label,
            config_file.display()
        )),
        Err(err) => app.push_agent_message(format!(
            "System: Backend set to {} for this run, but persistence to ~/.metaagent/config.toml failed: {err}. New adapters in this run will still use this backend.",
            selected.label
        )),
    }
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
    if App::is_quit_command(message) {
        return None;
    }
    if is_backend_command(message) {
        return None;
    }
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
    pending_task_write_baseline: &mut Option<TaskWriteBaseline>,
    master_session_intro_needed: &mut bool,
    project_info_text: Option<&str>,
) -> io::Result<bool> {
    let orchestration_service = DefaultCoreOrchestrationService;
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
    *pending_task_write_baseline = orchestration_service.capture_tasks_baseline(session_store);

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

fn is_backend_command(message: &str) -> bool {
    message.trim().eq_ignore_ascii_case("/backend")
}

fn is_known_slash_command(message: &str) -> bool {
    let trimmed = message.trim();
    if !trimmed.starts_with('/') {
        return false;
    }
    App::is_start_execution_command(trimmed)
        || is_backend_command(trimmed)
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

#[allow(dead_code)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "snake_case")]
enum CliOutputMode {
    Human,
    Json,
}

impl Default for CliOutputMode {
    fn default() -> Self {
        Self::Human
    }
}

#[derive(Debug, Parser)]
#[command(name = "metaagent-rust")]
struct LaunchCli {
    #[arg(long = "send-file", value_name = "PATH")]
    send_file: Option<PathBuf>,
    #[arg(long = "output", value_enum, default_value_t = CliOutputMode::Human)]
    output_mode: CliOutputMode,
    #[arg(long = "verbose", default_value_t = false)]
    verbose: bool,
    #[command(subcommand)]
    command: Option<RootCommand>,
}

#[derive(Debug, Clone, Subcommand)]
enum RootCommand {
    Api(ApiRootCommand),
}

#[derive(Debug, Clone, Args)]
struct ApiRootCommand {
    #[command(subcommand)]
    resource: ApiResourceCommand,
}

#[derive(Debug, Clone, Subcommand)]
enum ApiResourceCommand {
    Capability {
        #[command(subcommand)]
        action: CapabilityCommand,
    },
    App {
        #[command(subcommand)]
        action: AppCommand,
    },
    Workflow {
        #[command(subcommand)]
        action: WorkflowCommand,
    },
    Session {
        #[command(subcommand)]
        action: SessionCommand,
    },
}

#[derive(Debug, Clone, Subcommand)]
enum CapabilityCommand {
    List,
    Get {
        #[arg(long)]
        id: String,
    },
}

#[derive(Debug, Clone, Subcommand)]
enum AppCommand {
    PrepareMasterPrompt {
        #[arg(long)]
        message: String,
        #[arg(long)]
        tasks_file: String,
    },
    PreparePlannerPrompt {
        #[arg(long)]
        message: String,
        #[arg(long)]
        planner_file: String,
        #[arg(long)]
        project_info_file: String,
    },
    PrepareAttachDocsPrompt {
        #[arg(long)]
        tasks_file: String,
    },
}

#[derive(Debug, Clone, Subcommand)]
enum WorkflowCommand {
    ValidateTasks {
        #[arg(long)]
        tasks_file: PathBuf,
    },
    RightPaneView {
        #[arg(long)]
        tasks_file: PathBuf,
        #[arg(long, default_value_t = 100)]
        width: u16,
    },
}

#[derive(Debug, Clone, Subcommand)]
enum SessionCommand {
    Init {
        #[arg(long)]
        cwd: Option<PathBuf>,
    },
    Open {
        #[arg(long)]
        cwd: Option<PathBuf>,
        #[arg(long)]
        session_dir: PathBuf,
    },
    List,
    ReadTasks {
        #[arg(long)]
        cwd: Option<PathBuf>,
        #[arg(long)]
        session_dir: PathBuf,
    },
    ReadPlanner {
        #[arg(long)]
        cwd: Option<PathBuf>,
        #[arg(long)]
        session_dir: PathBuf,
    },
    ReadRollingContext {
        #[arg(long)]
        cwd: Option<PathBuf>,
        #[arg(long)]
        session_dir: PathBuf,
    },
    WriteRollingContext {
        #[arg(long)]
        cwd: Option<PathBuf>,
        #[arg(long)]
        session_dir: PathBuf,
        #[arg(long)]
        entries_file: PathBuf,
    },
    ReadTaskFails {
        #[arg(long)]
        cwd: Option<PathBuf>,
        #[arg(long)]
        session_dir: PathBuf,
    },
    AppendTaskFails {
        #[arg(long)]
        cwd: Option<PathBuf>,
        #[arg(long)]
        session_dir: PathBuf,
        #[arg(long)]
        entries_file: PathBuf,
    },
    ReadProjectInfo {
        #[arg(long)]
        cwd: Option<PathBuf>,
        #[arg(long)]
        session_dir: PathBuf,
    },
    WriteProjectInfo {
        #[arg(long)]
        cwd: Option<PathBuf>,
        #[arg(long)]
        session_dir: PathBuf,
        #[arg(long)]
        markdown_file: PathBuf,
    },
    ReadSessionMeta {
        #[arg(long)]
        cwd: Option<PathBuf>,
        #[arg(long)]
        session_dir: PathBuf,
    },
}

#[derive(Debug, Serialize)]
struct CliCommandOutput {
    summary: String,
    data: Value,
}

#[derive(Debug)]
struct CliCommandError {
    code: api::ApiErrorCode,
    message: String,
    details: Option<Value>,
}

impl CliCommandError {
    fn new(code: api::ApiErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
        }
    }

    fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum CliEnvelope {
    Ok { summary: String, data: Value },
    Err { error: api::ApiErrorEnvelope },
}

fn run_cli_command(command: RootCommand, output_mode: CliOutputMode, verbose: bool) -> i32 {
    let registry = TransportAdapterRegistry::register_defaults();
    let result = registry.dispatch(command);
    emit_cli_result(result, output_mode, verbose)
}

trait TransportAdapter {
    fn id(&self) -> &'static str;
    fn execute(&self, command: RootCommand) -> Result<CliCommandOutput, CliCommandError>;
}

#[derive(Debug, Default)]
struct TransportAdapterRegistry {
    cli: CliTransportAdapter,
}

impl TransportAdapterRegistry {
    fn register_defaults() -> Self {
        Self {
            cli: CliTransportAdapter,
        }
    }

    fn dispatch(&self, command: RootCommand) -> Result<CliCommandOutput, CliCommandError> {
        self.cli.execute(command)
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct CliTransportAdapter;

#[derive(Debug)]
struct CliContractInvocation {
    request: api::RequestEnvelope<api::ApiRequestContract>,
}

impl TransportAdapter for CliTransportAdapter {
    fn id(&self) -> &'static str {
        "cli"
    }

    fn execute(&self, command: RootCommand) -> Result<CliCommandOutput, CliCommandError> {
        match command {
            RootCommand::Api(api_command) => self.execute_api_command(api_command),
        }
    }
}

impl CliTransportAdapter {
    fn execute_api_command(
        &self,
        command: ApiRootCommand,
    ) -> Result<CliCommandOutput, CliCommandError> {
        match command.resource {
            ApiResourceCommand::Capability { action } => self.execute_capability_action(action),
            resource => {
                let invocation = self.map_resource_to_contract(resource)?;
                let response = execute_core_api_contract(invocation.request)?;
                self.map_contract_response(response)
            }
        }
    }

    fn execute_capability_action(
        &self,
        action: CapabilityCommand,
    ) -> Result<CliCommandOutput, CliCommandError> {
        match action {
            CapabilityCommand::List => {
                let capabilities = api::CAPABILITY_MATRIX
                    .iter()
                    .map(CapabilityDefinitionView::from)
                    .collect::<Vec<_>>();
                Ok(CliCommandOutput {
                    summary: format!("Listed {} capabilities", capabilities.len()),
                    data: serde_json::to_value(capabilities).map_err(|err| {
                        CliCommandError::new(
                            api::ApiErrorCode::Internal,
                            format!("Failed to serialize capability matrix: {err}"),
                        )
                    })?,
                })
            }
            CapabilityCommand::Get { id } => {
                let parsed = parse_capability_id(&id)?;
                let definition = api::capability_definition(parsed).ok_or_else(|| {
                    CliCommandError::new(api::ApiErrorCode::NotFound, "Capability id not found")
                        .with_details(json!({ "id": id }))
                })?;
                Ok(CliCommandOutput {
                    summary: format!("Loaded capability `{}`", id),
                    data: serde_json::to_value(CapabilityDefinitionView::from(definition))
                        .map_err(|err| {
                            CliCommandError::new(
                                api::ApiErrorCode::Internal,
                                format!("Failed to serialize capability definition: {err}"),
                            )
                        })?,
                })
            }
        }
    }

    fn map_resource_to_contract(
        &self,
        resource: ApiResourceCommand,
    ) -> Result<CliContractInvocation, CliCommandError> {
        let request = match resource {
            ApiResourceCommand::App { action } => {
                let payload = match action {
                    AppCommand::PrepareMasterPrompt {
                        message,
                        tasks_file,
                    } => api::AppRequest::PrepareMasterPrompt {
                        message,
                        tasks_file,
                    },
                    AppCommand::PreparePlannerPrompt {
                        message,
                        planner_file,
                        project_info_file,
                    } => api::AppRequest::PreparePlannerPrompt {
                        message,
                        planner_file,
                        project_info_file,
                    },
                    AppCommand::PrepareAttachDocsPrompt { tasks_file } => {
                        api::AppRequest::PrepareAttachDocsPrompt { tasks_file }
                    }
                };
                CliContractInvocation {
                    request: build_cli_envelope(
                        api::CapabilityId::AppPromptPreparation,
                        api::ApiRequestContract::App(payload),
                        self.id(),
                    ),
                }
            }
            ApiResourceCommand::Workflow { action } => match action {
                WorkflowCommand::ValidateTasks { tasks_file } => {
                    let tasks = read_cli_tasks_contract(&tasks_file)?;
                    CliContractInvocation {
                        request: build_cli_envelope(
                            api::CapabilityId::WorkflowTaskGraphSync,
                            api::ApiRequestContract::Workflow(
                                api::WorkflowRequest::SyncPlannerTasks { tasks },
                            ),
                            self.id(),
                        ),
                    }
                }
                WorkflowCommand::RightPaneView { tasks_file, width } => {
                    let tasks = read_cli_tasks_contract(&tasks_file)?;
                    CliContractInvocation {
                        request: build_cli_envelope_with_actor(
                            api::CapabilityId::WorkflowContextProjection,
                            api::ApiRequestContract::Workflow(
                                api::WorkflowRequest::RightPaneBlockView,
                            ),
                            self.id(),
                            json!({
                                "width": width,
                                "tasks": tasks,
                            }),
                        )?,
                    }
                }
            },
            ApiResourceCommand::Session { action } => {
                let payload = match action {
                    SessionCommand::Init { cwd } => {
                        let cwd = resolve_cli_cwd(cwd)?;
                        api::SessionRequest::Initialize {
                            cwd: cwd.to_string_lossy().to_string(),
                        }
                    }
                    SessionCommand::Open { cwd, session_dir } => {
                        let cwd = resolve_cli_cwd(cwd)?;
                        api::SessionRequest::OpenExisting {
                            cwd: cwd.to_string_lossy().to_string(),
                            session_dir: session_dir.to_string_lossy().to_string(),
                        }
                    }
                    SessionCommand::List => api::SessionRequest::ListSessions,
                    SessionCommand::ReadTasks { cwd, session_dir } => {
                        return Ok(CliContractInvocation {
                            request: build_cli_envelope_with_actor(
                                api::CapabilityId::SessionPlannerStorage,
                                api::ApiRequestContract::Session(api::SessionRequest::ReadTasks),
                                self.id(),
                                json!(resolve_session_lookup_context(cwd, session_dir)?),
                            )?,
                        });
                    }
                    SessionCommand::ReadPlanner { cwd, session_dir } => {
                        return Ok(CliContractInvocation {
                            request: build_cli_envelope_with_actor(
                                api::CapabilityId::SessionPlannerStorage,
                                api::ApiRequestContract::Session(
                                    api::SessionRequest::ReadPlannerMarkdown,
                                ),
                                self.id(),
                                json!(resolve_session_lookup_context(cwd, session_dir)?),
                            )?,
                        });
                    }
                    SessionCommand::ReadRollingContext { cwd, session_dir } => {
                        return Ok(CliContractInvocation {
                            request: build_cli_envelope_with_actor(
                                api::CapabilityId::SessionPlannerStorage,
                                api::ApiRequestContract::Session(
                                    api::SessionRequest::ReadRollingContext,
                                ),
                                self.id(),
                                json!(resolve_session_lookup_context(cwd, session_dir)?),
                            )?,
                        });
                    }
                    SessionCommand::WriteRollingContext {
                        cwd,
                        session_dir,
                        entries_file,
                    } => {
                        let entries = read_json_from_file::<Vec<String>>(
                            &entries_file,
                            "rolling-context entries",
                        )?;
                        return Ok(CliContractInvocation {
                            request: build_cli_envelope_with_actor(
                                api::CapabilityId::SessionPlannerStorage,
                                api::ApiRequestContract::Session(
                                    api::SessionRequest::WriteRollingContext { entries },
                                ),
                                self.id(),
                                json!(resolve_session_lookup_context(cwd, session_dir)?),
                            )?,
                        });
                    }
                    SessionCommand::ReadTaskFails { cwd, session_dir } => {
                        return Ok(CliContractInvocation {
                            request: build_cli_envelope_with_actor(
                                api::CapabilityId::SessionFailureStorage,
                                api::ApiRequestContract::Session(
                                    api::SessionRequest::ReadTaskFails,
                                ),
                                self.id(),
                                json!(resolve_session_lookup_context(cwd, session_dir)?),
                            )?,
                        });
                    }
                    SessionCommand::AppendTaskFails {
                        cwd,
                        session_dir,
                        entries_file,
                    } => {
                        let entries = read_json_from_file::<Vec<api::TaskFailureContract>>(
                            &entries_file,
                            "task-fails entries",
                        )?;
                        return Ok(CliContractInvocation {
                            request: build_cli_envelope_with_actor(
                                api::CapabilityId::SessionFailureStorage,
                                api::ApiRequestContract::Session(
                                    api::SessionRequest::AppendTaskFails { entries },
                                ),
                                self.id(),
                                json!(resolve_session_lookup_context(cwd, session_dir)?),
                            )?,
                        });
                    }
                    SessionCommand::ReadProjectInfo { cwd, session_dir } => {
                        return Ok(CliContractInvocation {
                            request: build_cli_envelope_with_actor(
                                api::CapabilityId::SessionProjectContextStorage,
                                api::ApiRequestContract::Session(
                                    api::SessionRequest::ReadProjectInfo,
                                ),
                                self.id(),
                                json!(resolve_session_lookup_context(cwd, session_dir)?),
                            )?,
                        });
                    }
                    SessionCommand::WriteProjectInfo {
                        cwd,
                        session_dir,
                        markdown_file,
                    } => {
                        let markdown = std::fs::read_to_string(&markdown_file).map_err(|err| {
                            CliCommandError::new(
                                api::ApiErrorCode::IoFailure,
                                format!("Failed to read markdown file: {err}"),
                            )
                            .with_details(json!({ "markdown_file": markdown_file }))
                        })?;
                        return Ok(CliContractInvocation {
                            request: build_cli_envelope_with_actor(
                                api::CapabilityId::SessionProjectContextStorage,
                                api::ApiRequestContract::Session(
                                    api::SessionRequest::WriteProjectInfo { markdown },
                                ),
                                self.id(),
                                json!(resolve_session_lookup_context(cwd, session_dir)?),
                            )?,
                        });
                    }
                    SessionCommand::ReadSessionMeta { cwd, session_dir } => {
                        return Ok(CliContractInvocation {
                            request: build_cli_envelope_with_actor(
                                api::CapabilityId::SessionProjectContextStorage,
                                api::ApiRequestContract::Session(
                                    api::SessionRequest::ReadSessionMeta,
                                ),
                                self.id(),
                                json!(resolve_session_lookup_context(cwd, session_dir)?),
                            )?,
                        });
                    }
                };
                CliContractInvocation {
                    request: build_cli_envelope(
                        match payload {
                            api::SessionRequest::Initialize { .. }
                            | api::SessionRequest::OpenExisting { .. }
                            | api::SessionRequest::ListSessions => {
                                api::CapabilityId::SessionLifecycle
                            }
                            _ => api::CapabilityId::SessionPlannerStorage,
                        },
                        api::ApiRequestContract::Session(payload),
                        self.id(),
                    ),
                }
            }
            ApiResourceCommand::Capability { .. } => {
                return Err(CliCommandError::new(
                    api::ApiErrorCode::Unsupported,
                    "Capability commands are handled directly by the CLI adapter",
                ));
            }
        };
        Ok(request)
    }

    fn map_contract_response(
        &self,
        response: api::ResponseEnvelope<api::ApiResponseContract>,
    ) -> Result<CliCommandOutput, CliCommandError> {
        match response.result {
            api::ApiResultEnvelope::Ok { data } => self.map_ok_contract_data(data),
            api::ApiResultEnvelope::Err { error } => Err(CliCommandError {
                code: error.code,
                message: error.message,
                details: error.details,
            }),
        }
    }

    fn map_ok_contract_data(
        &self,
        data: api::ApiResponseContract,
    ) -> Result<CliCommandOutput, CliCommandError> {
        match data {
            api::ApiResponseContract::App(api::AppResponse::Prompt { text }) => {
                Ok(CliCommandOutput {
                    summary: "Prepared prompt".to_string(),
                    data: json!({ "prompt": text }),
                })
            }
            api::ApiResponseContract::Workflow(api::WorkflowResponse::PlannerTasks { tasks }) => {
                Ok(CliCommandOutput {
                    summary: format!("Validated {} planner tasks", tasks.len()),
                    data: json!({
                        "count": tasks.len(),
                        "tasks": tasks,
                    }),
                })
            }
            api::ApiResponseContract::Workflow(api::WorkflowResponse::RightPaneBlock {
                lines,
                toggles,
            }) => Ok(CliCommandOutput {
                summary: "Rendered right-pane workflow block".to_string(),
                data: json!({ "lines": lines, "toggles": toggles }),
            }),
            api::ApiResponseContract::Session(api::SessionResponse::Initialized { session }) => {
                Ok(CliCommandOutput {
                    summary: "Initialized session".to_string(),
                    data: json!({
                        "session_dir": session.session_dir,
                    }),
                })
            }
            api::ApiResponseContract::Session(api::SessionResponse::Sessions { sessions }) => {
                Ok(CliCommandOutput {
                    summary: format!("Found {} session(s)", sessions.len()),
                    data: serde_json::to_value(sessions).map_err(|err| {
                        CliCommandError::new(
                            api::ApiErrorCode::Internal,
                            format!("Failed to serialize session list: {err}"),
                        )
                    })?,
                })
            }
            api::ApiResponseContract::Session(api::SessionResponse::Tasks { tasks }) => {
                Ok(CliCommandOutput {
                    summary: format!("Read {} task(s)", tasks.len()),
                    data: json!({ "tasks": tasks.into_iter().map(contract_task_to_file_task).collect::<Vec<_>>() }),
                })
            }
            api::ApiResponseContract::Session(api::SessionResponse::PlannerMarkdown {
                markdown,
            }) => Ok(CliCommandOutput {
                summary: "Read planner markdown".to_string(),
                data: json!({ "markdown": markdown }),
            }),
            api::ApiResponseContract::Session(api::SessionResponse::RollingContext { entries }) => {
                Ok(CliCommandOutput {
                    summary: format!("Read {} rolling-context entrie(s)", entries.len()),
                    data: json!({ "entries": entries }),
                })
            }
            api::ApiResponseContract::Session(api::SessionResponse::TaskFails { entries }) => {
                Ok(CliCommandOutput {
                    summary: format!("Read {} task-fail entrie(s)", entries.len()),
                    data: json!({ "entries": entries }),
                })
            }
            api::ApiResponseContract::Session(api::SessionResponse::ProjectInfo { markdown }) => {
                Ok(CliCommandOutput {
                    summary: "Read project info markdown".to_string(),
                    data: json!({ "markdown": markdown }),
                })
            }
            api::ApiResponseContract::Session(api::SessionResponse::SessionMeta { meta }) => {
                Ok(CliCommandOutput {
                    summary: "Read session metadata".to_string(),
                    data: json!({ "meta": meta }),
                })
            }
            api::ApiResponseContract::Session(api::SessionResponse::Ack) => Ok(CliCommandOutput {
                summary: "Completed session operation".to_string(),
                data: json!({}),
            }),
            other => Err(CliCommandError::new(
                api::ApiErrorCode::Unsupported,
                format!("CLI transport does not support response contract: {other:?}"),
            )),
        }
    }
}

fn build_cli_envelope(
    capability: api::CapabilityId,
    payload: api::ApiRequestContract,
    transport_id: &str,
) -> api::RequestEnvelope<api::ApiRequestContract> {
    api::RequestEnvelope {
        request_id: None,
        capability,
        metadata: api::RequestMetadata {
            transport: Some(transport_id.to_string()),
            actor: Some("operator".to_string()),
        },
        payload,
    }
}

fn build_cli_envelope_with_actor(
    capability: api::CapabilityId,
    payload: api::ApiRequestContract,
    transport_id: &str,
    actor_value: Value,
) -> Result<api::RequestEnvelope<api::ApiRequestContract>, CliCommandError> {
    Ok(api::RequestEnvelope {
        request_id: None,
        capability,
        metadata: api::RequestMetadata {
            transport: Some(transport_id.to_string()),
            actor: Some(serde_json::to_string(&actor_value).map_err(|err| {
                CliCommandError::new(
                    api::ApiErrorCode::Internal,
                    format!("Failed to serialize transport metadata: {err}"),
                )
            })?),
        },
        payload,
    })
}

fn read_cli_tasks_contract(
    tasks_file: &Path,
) -> Result<Vec<api::PlannerTaskEntryContract>, CliCommandError> {
    let text = std::fs::read_to_string(tasks_file).map_err(|err| {
        CliCommandError::new(
            api::ApiErrorCode::IoFailure,
            format!("Failed to read tasks file: {err}"),
        )
        .with_details(json!({ "tasks_file": tasks_file }))
    })?;
    let tasks = serde_json::from_str::<Vec<PlannerTaskFileEntry>>(&text).map_err(|err| {
        CliCommandError::new(
            api::ApiErrorCode::InvalidRequest,
            format!("Failed to parse tasks JSON: {err}"),
        )
        .with_details(json!({ "tasks_file": tasks_file }))
    })?;
    Ok(tasks.into_iter().map(file_task_to_contract_task).collect())
}

fn read_json_from_file<T>(path: &Path, label: &str) -> Result<T, CliCommandError>
where
    T: DeserializeOwned,
{
    let text = std::fs::read_to_string(path).map_err(|err| {
        CliCommandError::new(
            api::ApiErrorCode::IoFailure,
            format!("Failed to read {label} file: {err}"),
        )
        .with_details(json!({ "path": path }))
    })?;
    serde_json::from_str::<T>(&text).map_err(|err| {
        CliCommandError::new(
            api::ApiErrorCode::InvalidRequest,
            format!("Failed to parse {label} JSON: {err}"),
        )
        .with_details(json!({ "path": path }))
    })
}

fn resolve_cli_cwd(cwd: Option<PathBuf>) -> Result<PathBuf, CliCommandError> {
    Ok(cwd.unwrap_or(std::env::current_dir().map_err(|err| {
        CliCommandError::new(
            api::ApiErrorCode::IoFailure,
            format!("Failed to resolve current directory: {err}"),
        )
    })?))
}

fn resolve_session_lookup_context(
    cwd: Option<PathBuf>,
    session_dir: PathBuf,
) -> Result<Value, CliCommandError> {
    let cwd = resolve_cli_cwd(cwd)?;
    Ok(json!({
        "cwd": cwd,
        "session_dir": session_dir,
    }))
}

fn decode_actor_json(actor: Option<String>) -> Result<Value, CliCommandError> {
    let raw = actor.ok_or_else(|| {
        CliCommandError::new(
            api::ApiErrorCode::InvalidRequest,
            "Missing transport actor metadata",
        )
    })?;
    serde_json::from_str(&raw).map_err(|err| {
        CliCommandError::new(
            api::ApiErrorCode::InvalidRequest,
            format!("Invalid transport actor metadata: {err}"),
        )
    })
}

fn actor_pathbuf(actor: &Value, key: &str) -> Result<PathBuf, CliCommandError> {
    let value = actor.get(key).and_then(Value::as_str).ok_or_else(|| {
        CliCommandError::new(
            api::ApiErrorCode::InvalidRequest,
            format!("Missing `{key}` in transport metadata"),
        )
    })?;
    Ok(PathBuf::from(value))
}

fn execute_core_api_contract(
    request: api::RequestEnvelope<api::ApiRequestContract>,
) -> Result<api::ResponseEnvelope<api::ApiResponseContract>, CliCommandError> {
    let request_id = request.request_id.clone();
    let capability = request.capability;
    let metadata = request.metadata;
    let data = match request.payload {
        api::ApiRequestContract::App(app_request) => {
            api::ApiResponseContract::App(execute_core_app_request(app_request)?)
        }
        api::ApiRequestContract::Workflow(workflow_request) => api::ApiResponseContract::Workflow(
            execute_core_workflow_request(workflow_request, metadata)?,
        ),
        api::ApiRequestContract::Session(session_request) => api::ApiResponseContract::Session(
            execute_core_session_request(session_request, metadata)?,
        ),
        api::ApiRequestContract::Events(_) | api::ApiRequestContract::Subagent(_) => {
            return Err(CliCommandError::new(
                api::ApiErrorCode::Unsupported,
                "CLI transport does not expose this API contract domain",
            ));
        }
    };
    Ok(api::ResponseEnvelope {
        request_id,
        capability,
        result: api::ApiResultEnvelope::Ok { data },
    })
}

fn execute_core_app_request(request: api::AppRequest) -> Result<api::AppResponse, CliCommandError> {
    match request {
        api::AppRequest::PrepareMasterPrompt {
            message,
            tasks_file,
        } => Ok(api::AppResponse::Prompt {
            text: App::default().prepare_master_prompt(&message, &tasks_file),
        }),
        api::AppRequest::PreparePlannerPrompt {
            message,
            planner_file,
            project_info_file,
        } => Ok(api::AppResponse::Prompt {
            text: App::default().prepare_planner_prompt(
                &message,
                &planner_file,
                &project_info_file,
            ),
        }),
        api::AppRequest::PrepareAttachDocsPrompt { tasks_file } => Ok(api::AppResponse::Prompt {
            text: App::default().prepare_attach_docs_prompt(&tasks_file),
        }),
        _ => Err(CliCommandError::new(
            api::ApiErrorCode::Unsupported,
            "App request is not available in CLI transport mode",
        )),
    }
}

fn execute_core_workflow_request(
    request: api::WorkflowRequest,
    metadata: api::RequestMetadata,
) -> Result<api::WorkflowResponse, CliCommandError> {
    match request {
        api::WorkflowRequest::SyncPlannerTasks { tasks } => {
            let file_tasks = tasks.into_iter().map(contract_task_to_file_task).collect();
            let mut workflow = workflow::Workflow::default();
            workflow
                .sync_planner_tasks_from_file(file_tasks)
                .map_err(|err| CliCommandError::new(api::ApiErrorCode::ValidationFailed, err))?;
            let normalized = workflow
                .planner_tasks_for_file()
                .into_iter()
                .map(file_task_to_contract_task)
                .collect::<Vec<_>>();
            Ok(api::WorkflowResponse::PlannerTasks { tasks: normalized })
        }
        api::WorkflowRequest::RightPaneBlockView => {
            let actor = decode_actor_json(metadata.actor)?;
            let width = actor
                .get("width")
                .and_then(Value::as_u64)
                .and_then(|value| u16::try_from(value).ok())
                .ok_or_else(|| {
                    CliCommandError::new(
                        api::ApiErrorCode::InvalidRequest,
                        "Missing or invalid `width` in transport metadata",
                    )
                })?;
            let tasks: Vec<api::PlannerTaskEntryContract> =
                serde_json::from_value(actor.get("tasks").cloned().ok_or_else(|| {
                    CliCommandError::new(
                        api::ApiErrorCode::InvalidRequest,
                        "Missing `tasks` in transport metadata",
                    )
                })?)
                .map_err(|err| {
                    CliCommandError::new(
                        api::ApiErrorCode::InvalidRequest,
                        format!("Invalid `tasks` in transport metadata: {err}"),
                    )
                })?;
            let mut workflow = workflow::Workflow::default();
            workflow
                .sync_planner_tasks_from_file(
                    tasks.into_iter().map(contract_task_to_file_task).collect(),
                )
                .map_err(|err| CliCommandError::new(api::ApiErrorCode::ValidationFailed, err))?;
            let pane = workflow.right_pane_block_view(width, &HashSet::new());
            Ok(api::WorkflowResponse::RightPaneBlock {
                lines: pane.lines,
                toggles: pane
                    .toggles
                    .into_iter()
                    .map(|toggle| api::RightPaneToggleContract {
                        line_index: toggle.line_index,
                        task_key: toggle.task_key,
                    })
                    .collect(),
            })
        }
        _ => Err(CliCommandError::new(
            api::ApiErrorCode::Unsupported,
            "Workflow request is not available in CLI transport mode",
        )),
    }
}

fn execute_core_session_request(
    request: api::SessionRequest,
    metadata: api::RequestMetadata,
) -> Result<api::SessionResponse, CliCommandError> {
    let open_actor_session = || -> Result<SessionStore, CliCommandError> {
        let actor = decode_actor_json(metadata.actor.clone())?;
        let cwd = actor_pathbuf(&actor, "cwd")?;
        let session_dir = actor_pathbuf(&actor, "session_dir")?;
        SessionStore::open_existing(&cwd, &session_dir).map_err(|err| {
            CliCommandError::new(
                api::ApiErrorCode::NotFound,
                format!("Failed to open session: {err}"),
            )
            .with_details(json!({ "cwd": cwd, "session_dir": session_dir }))
        })
    };

    match request {
        api::SessionRequest::Initialize { cwd } => {
            let cwd = PathBuf::from(cwd);
            let session = SessionStore::initialize(&cwd).map_err(|err| {
                CliCommandError::new(
                    api::ApiErrorCode::IoFailure,
                    format!("Failed to initialize session store: {err}"),
                )
                .with_details(json!({ "cwd": cwd }))
            })?;
            Ok(api::SessionResponse::Initialized {
                session: api::SessionStoreSnapshotContract {
                    session_dir: session.session_dir().display().to_string(),
                    tasks_file: session.tasks_file().display().to_string(),
                    planner_file: session.planner_file().display().to_string(),
                    task_fails_file: session.task_fails_file().display().to_string(),
                    project_info_file: session.project_info_file().display().to_string(),
                    session_meta_file: session.session_meta_file().display().to_string(),
                },
            })
        }
        api::SessionRequest::OpenExisting { cwd, session_dir } => {
            let cwd = PathBuf::from(cwd);
            let session_dir = PathBuf::from(session_dir);
            let session = SessionStore::open_existing(&cwd, &session_dir).map_err(|err| {
                CliCommandError::new(
                    api::ApiErrorCode::NotFound,
                    format!("Failed to open session: {err}"),
                )
                .with_details(json!({ "cwd": cwd, "session_dir": session_dir }))
            })?;
            Ok(api::SessionResponse::Initialized {
                session: api::SessionStoreSnapshotContract {
                    session_dir: session.session_dir().display().to_string(),
                    tasks_file: session.tasks_file().display().to_string(),
                    planner_file: session.planner_file().display().to_string(),
                    task_fails_file: session.task_fails_file().display().to_string(),
                    project_info_file: session.project_info_file().display().to_string(),
                    session_meta_file: session.session_meta_file().display().to_string(),
                },
            })
        }
        api::SessionRequest::ListSessions => {
            let sessions = SessionStore::list_sessions().map_err(|err| {
                CliCommandError::new(
                    api::ApiErrorCode::IoFailure,
                    format!("Failed to list sessions: {err}"),
                )
            })?;
            Ok(api::SessionResponse::Sessions {
                sessions: sessions
                    .into_iter()
                    .map(|entry| api::SessionListEntryContract {
                        session_dir: entry.session_dir.display().to_string(),
                        workspace: entry.workspace,
                        title: entry.title,
                        created_at_label: entry.created_at_label,
                        created_at_epoch_secs: entry.created_at_epoch_secs,
                        last_used_epoch_secs: entry.last_used_epoch_secs,
                    })
                    .collect(),
            })
        }
        api::SessionRequest::ReadTasks => {
            let session = open_actor_session()?;
            let tasks = session.read_tasks().map_err(|err| {
                CliCommandError::new(
                    api::ApiErrorCode::IoFailure,
                    format!("Failed to read tasks file: {err}"),
                )
            })?;
            Ok(api::SessionResponse::Tasks {
                tasks: tasks.into_iter().map(file_task_to_contract_task).collect(),
            })
        }
        api::SessionRequest::ReadPlannerMarkdown => {
            let session = open_actor_session()?;
            let markdown = session.read_planner_markdown().map_err(|err| {
                CliCommandError::new(
                    api::ApiErrorCode::IoFailure,
                    format!("Failed to read planner markdown: {err}"),
                )
            })?;
            Ok(api::SessionResponse::PlannerMarkdown { markdown })
        }
        api::SessionRequest::WriteRollingContext { entries } => {
            let session = open_actor_session()?;
            session.write_rolling_context(&entries).map_err(|err| {
                CliCommandError::new(
                    api::ApiErrorCode::IoFailure,
                    format!("Failed to write rolling context: {err}"),
                )
            })?;
            Ok(api::SessionResponse::Ack)
        }
        api::SessionRequest::ReadRollingContext => {
            let session = open_actor_session()?;
            let entries = session.read_rolling_context().map_err(|err| {
                CliCommandError::new(
                    api::ApiErrorCode::IoFailure,
                    format!("Failed to read rolling context: {err}"),
                )
            })?;
            Ok(api::SessionResponse::RollingContext { entries })
        }
        api::SessionRequest::ReadTaskFails => {
            let session = open_actor_session()?;
            let entries = session.read_task_fails().map_err(|err| {
                CliCommandError::new(
                    api::ApiErrorCode::IoFailure,
                    format!("Failed to read task fails: {err}"),
                )
            })?;
            let entries = entries
                .into_iter()
                .map(file_task_fail_to_contract)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(api::SessionResponse::TaskFails { entries })
        }
        api::SessionRequest::AppendTaskFails { entries } => {
            let session = open_actor_session()?;
            let now_secs = current_epoch_secs();
            let entries = entries
                .into_iter()
                .map(|entry| contract_task_fail_to_file(entry, now_secs))
                .collect::<Vec<_>>();
            session.append_task_fails(&entries).map_err(|err| {
                CliCommandError::new(
                    api::ApiErrorCode::IoFailure,
                    format!("Failed to append task fails: {err}"),
                )
            })?;
            Ok(api::SessionResponse::Ack)
        }
        api::SessionRequest::ReadProjectInfo => {
            let session = open_actor_session()?;
            let markdown = session.read_project_info().map_err(|err| {
                CliCommandError::new(
                    api::ApiErrorCode::IoFailure,
                    format!("Failed to read project info: {err}"),
                )
            })?;
            Ok(api::SessionResponse::ProjectInfo { markdown })
        }
        api::SessionRequest::WriteProjectInfo { markdown } => {
            let session = open_actor_session()?;
            session.write_project_info(&markdown).map_err(|err| {
                CliCommandError::new(
                    api::ApiErrorCode::IoFailure,
                    format!("Failed to write project info: {err}"),
                )
            })?;
            Ok(api::SessionResponse::Ack)
        }
        api::SessionRequest::ReadSessionMeta => {
            let session = open_actor_session()?;
            let meta = session.read_session_meta().map_err(|err| {
                CliCommandError::new(
                    api::ApiErrorCode::IoFailure,
                    format!("Failed to read session meta: {err}"),
                )
            })?;
            Ok(api::SessionResponse::SessionMeta {
                meta: file_session_meta_to_contract(meta),
            })
        }
    }
}

fn file_task_to_contract_task(task: PlannerTaskFileEntry) -> api::PlannerTaskEntryContract {
    api::PlannerTaskEntryContract {
        id: task.id,
        title: task.title,
        details: task.details,
        docs: task
            .docs
            .into_iter()
            .map(file_doc_to_contract_doc)
            .collect(),
        kind: file_kind_to_contract_kind(task.kind),
        status: file_status_to_contract_status(task.status),
        parent_id: task.parent_id,
        order: task.order,
    }
}

fn contract_task_to_file_task(task: api::PlannerTaskEntryContract) -> PlannerTaskFileEntry {
    PlannerTaskFileEntry {
        id: task.id,
        title: task.title,
        details: task.details,
        docs: task
            .docs
            .into_iter()
            .map(contract_doc_to_file_doc)
            .collect(),
        kind: contract_kind_to_file_kind(task.kind),
        status: contract_status_to_file_status(task.status),
        parent_id: task.parent_id,
        order: task.order,
    }
}

fn file_doc_to_contract_doc(
    doc: session_store::PlannerTaskDocFileEntry,
) -> api::PlannerTaskDocContract {
    api::PlannerTaskDocContract {
        title: doc.title,
        url: doc.url,
        summary: doc.summary,
    }
}

fn contract_doc_to_file_doc(
    doc: api::PlannerTaskDocContract,
) -> session_store::PlannerTaskDocFileEntry {
    session_store::PlannerTaskDocFileEntry {
        title: doc.title,
        url: doc.url,
        summary: doc.summary,
    }
}

fn file_task_fail_to_contract(
    entry: TaskFailFileEntry,
) -> Result<api::TaskFailureContract, CliCommandError> {
    let kind = match entry.kind.as_str() {
        "audit" => api::WorkflowFailureKindContract::Audit,
        "test" => api::WorkflowFailureKindContract::Test,
        other => {
            return Err(CliCommandError::new(
                api::ApiErrorCode::ValidationFailed,
                format!("Unknown task fail kind in artifact: {other}"),
            ));
        }
    };
    Ok(api::TaskFailureContract {
        kind,
        top_task_id: entry.top_task_id,
        top_task_title: entry.top_task_title,
        attempts: entry.attempts,
        reason: entry.reason,
        action_taken: entry.action_taken,
    })
}

fn contract_task_fail_to_file(
    entry: api::TaskFailureContract,
    created_at_epoch_secs: u64,
) -> TaskFailFileEntry {
    TaskFailFileEntry {
        kind: match entry.kind {
            api::WorkflowFailureKindContract::Audit => "audit".to_string(),
            api::WorkflowFailureKindContract::Test => "test".to_string(),
        },
        top_task_id: entry.top_task_id,
        top_task_title: entry.top_task_title,
        attempts: entry.attempts,
        reason: entry.reason,
        action_taken: entry.action_taken,
        created_at_epoch_secs,
    }
}

fn file_session_meta_to_contract(meta: session_store::SessionMetaFile) -> api::SessionMetaContract {
    api::SessionMetaContract {
        title: meta.title,
        created_at: meta.created_at,
        stack_description: meta.stack_description,
        test_command: meta.test_command,
    }
}

fn current_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn file_kind_to_contract_kind(kind: PlannerTaskKindFile) -> api::PlannerTaskKindContract {
    match kind {
        PlannerTaskKindFile::Task => api::PlannerTaskKindContract::Task,
        PlannerTaskKindFile::Implementor => api::PlannerTaskKindContract::Implementor,
        PlannerTaskKindFile::Auditor => api::PlannerTaskKindContract::Auditor,
        PlannerTaskKindFile::TestWriter => api::PlannerTaskKindContract::TestWriter,
        PlannerTaskKindFile::TestRunner => api::PlannerTaskKindContract::TestRunner,
        PlannerTaskKindFile::FinalAudit => api::PlannerTaskKindContract::FinalAudit,
    }
}

fn contract_kind_to_file_kind(kind: api::PlannerTaskKindContract) -> PlannerTaskKindFile {
    match kind {
        api::PlannerTaskKindContract::Task => PlannerTaskKindFile::Task,
        api::PlannerTaskKindContract::Implementor => PlannerTaskKindFile::Implementor,
        api::PlannerTaskKindContract::Auditor => PlannerTaskKindFile::Auditor,
        api::PlannerTaskKindContract::TestWriter => PlannerTaskKindFile::TestWriter,
        api::PlannerTaskKindContract::TestRunner => PlannerTaskKindFile::TestRunner,
        api::PlannerTaskKindContract::FinalAudit => PlannerTaskKindFile::FinalAudit,
    }
}

fn file_status_to_contract_status(status: PlannerTaskStatusFile) -> api::PlannerTaskStatusContract {
    match status {
        PlannerTaskStatusFile::Pending => api::PlannerTaskStatusContract::Pending,
        PlannerTaskStatusFile::InProgress => api::PlannerTaskStatusContract::InProgress,
        PlannerTaskStatusFile::NeedsChanges => api::PlannerTaskStatusContract::NeedsChanges,
        PlannerTaskStatusFile::Done => api::PlannerTaskStatusContract::Done,
    }
}

fn contract_status_to_file_status(status: api::PlannerTaskStatusContract) -> PlannerTaskStatusFile {
    match status {
        api::PlannerTaskStatusContract::Pending => PlannerTaskStatusFile::Pending,
        api::PlannerTaskStatusContract::InProgress => PlannerTaskStatusFile::InProgress,
        api::PlannerTaskStatusContract::NeedsChanges => PlannerTaskStatusFile::NeedsChanges,
        api::PlannerTaskStatusContract::Done => PlannerTaskStatusFile::Done,
    }
}

fn parse_capability_id(raw: &str) -> Result<api::CapabilityId, CliCommandError> {
    serde_json::from_str::<api::CapabilityId>(&format!("\"{raw}\"")).map_err(|_| {
        CliCommandError::new(
            api::ApiErrorCode::InvalidRequest,
            format!("Unknown capability id `{raw}`"),
        )
    })
}

fn emit_cli_result(
    result: Result<CliCommandOutput, CliCommandError>,
    output_mode: CliOutputMode,
    verbose: bool,
) -> i32 {
    match result {
        Ok(output) => {
            match output_mode {
                CliOutputMode::Human => {
                    println!("{}", output.summary);
                    render_cli_payload_human(&output.data, verbose);
                }
                CliOutputMode::Json => {
                    let envelope = CliEnvelope::Ok {
                        summary: output.summary,
                        data: output.data,
                    };
                    match serde_json::to_string_pretty(&envelope) {
                        Ok(text) => println!("{text}"),
                        Err(err) => {
                            eprintln!("Failed to serialize CLI success output: {err}");
                            return exit_code_for_error(api::ApiErrorCode::Internal);
                        }
                    }
                }
            }
            0
        }
        Err(err) => {
            match output_mode {
                CliOutputMode::Human => {
                    eprintln!("{}: {}", api_error_code_label(err.code), err.message);
                    if let Some(details) = err.details {
                        eprintln!("details: {details}");
                    }
                }
                CliOutputMode::Json => {
                    let envelope = CliEnvelope::Err {
                        error: api::ApiErrorEnvelope {
                            code: err.code,
                            message: err.message,
                            retryable: false,
                            details: err.details,
                        },
                    };
                    match serde_json::to_string_pretty(&envelope) {
                        Ok(text) => println!("{text}"),
                        Err(encode_err) => {
                            eprintln!("Failed to serialize CLI error output: {encode_err}");
                        }
                    }
                }
            }
            exit_code_for_error(err.code)
        }
    }
}

fn render_cli_payload_human(payload: &Value, verbose: bool) {
    if payload.is_null() {
        return;
    }
    if is_capability_definition_list(payload) {
        if let Some(capabilities) = payload.as_array() {
            render_capability_list_human(capabilities);
        }
        return;
    }

    if verbose {
        let text = match serde_json::to_string_pretty(payload) {
            Ok(text) => text,
            Err(_) => return,
        };
        println!("Data:");
        for line in text.lines() {
            println!("  {line}");
        }
        return;
    }

    match payload {
        Value::Array(items) => {
            if items.is_empty() {
                return;
            }
            render_array_brief(items);
        }
        Value::Object(map) => {
            if map.is_empty() {
                return;
            }
            println!("Data:");
            for (key, value) in map {
                print_field_summary(key, value);
            }
        }
        _ => {
            println!("Data: {}", render_value_short(payload));
        }
    }
}

fn print_field_summary(key: &str, value: &Value) {
    match value {
        Value::Array(items) => {
            println!("  {key}: {} entries", items.len());
            for item in items.iter().take(6) {
                print_array_item(item);
            }
        }
        _ => {
            println!("  {key}: {}", render_value_short(value));
        }
    }
}

fn render_array_brief(items: &[Value]) {
    println!("Data: {} entries", items.len());
    for item in items.iter().take(8) {
        print_array_item(item);
    }
}

fn print_array_item(item: &Value) {
    match item {
        Value::Object(obj) => {
            if let Some(id) = obj.get("id").and_then(Value::as_str) {
                if let Some(session_dir) = obj.get("session_dir").and_then(Value::as_str) {
                    println!("  - {id}: {session_dir}");
                } else if let Some(title) = obj.get("title").and_then(Value::as_str) {
                    println!("  - {id}: {title}");
                } else {
                    println!("  - {id}");
                }
            } else if let Some(title) = obj.get("title").and_then(Value::as_str) {
                println!("  - {title}");
            } else if let Some(session_dir) = obj.get("session_dir").and_then(Value::as_str) {
                println!("  - {session_dir}");
            } else if let Some(markdown) = obj.get("markdown").and_then(Value::as_str) {
                let snippet = markdown
                    .lines()
                    .next()
                    .unwrap_or("")
                    .chars()
                    .take(80)
                    .collect::<String>();
                println!("  - {snippet}");
            } else if obj.is_empty() {
                println!("  - {{}}");
            } else {
                println!("  - {}", render_value_short(item));
            }
        }
        _ => {
            println!("  - {}", render_value_short(item));
        }
    }
}

fn render_value_short(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => {
            if value.len() > 80 {
                format!("{}...", &value[..80])
            } else {
                value.to_string()
            }
        }
        Value::Array(items) => format!("{entries} entries", entries = items.len()),
        Value::Object(map) => format!("{entries} fields", entries = map.len()),
    }
}

fn is_capability_definition_list(payload: &Value) -> bool {
    let Some(values) = payload.as_array() else {
        return false;
    };
    !values.is_empty()
        && values.iter().all(|value| {
            value.get("id").is_some()
                && value.get("domain").is_some()
                && value.get("operation").is_some()
                && value.get("request_contract").is_some()
                && value.get("response_contract").is_some()
                && value.get("code_paths").is_some()
                && value.get("notes").is_some()
        })
}

fn render_capability_list_human(capabilities: &[Value]) {
    for capability in capabilities {
        let id = capability
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("<missing-id>");
        let domain = capability
            .get("domain")
            .and_then(Value::as_str)
            .unwrap_or("<missing-domain>");
        let operation = capability
            .get("operation")
            .and_then(Value::as_str)
            .unwrap_or("<missing-operation>");
        let request_contract = capability
            .get("request_contract")
            .and_then(Value::as_str)
            .unwrap_or("<missing-request-contract>");
        let response_contract = capability
            .get("response_contract")
            .and_then(Value::as_str)
            .unwrap_or("<missing-response-contract>");
        let code_paths = capability
            .get("code_paths")
            .and_then(|value| serde_json::to_string_pretty(value).ok())
            .unwrap_or_else(|| "<missing-code-paths>".to_string());
        let notes = capability
            .get("notes")
            .and_then(Value::as_str)
            .unwrap_or("<missing-notes>");

        println!("  - {id} ({domain} / {operation})");
        println!("    request_contract: {request_contract}");
        println!("    response_contract: {response_contract}");
        println!("    code_paths: {code_paths}");
        println!("    notes: {notes}");
    }
}

fn api_error_code_label(code: api::ApiErrorCode) -> &'static str {
    match code {
        api::ApiErrorCode::InvalidRequest => "invalid_request",
        api::ApiErrorCode::ValidationFailed => "validation_failed",
        api::ApiErrorCode::NotFound => "not_found",
        api::ApiErrorCode::Conflict => "conflict",
        api::ApiErrorCode::IoFailure => "io_failure",
        api::ApiErrorCode::ExternalFailure => "external_failure",
        api::ApiErrorCode::Unsupported => "unsupported",
        api::ApiErrorCode::Internal => "internal",
    }
}

fn exit_code_for_error(code: api::ApiErrorCode) -> i32 {
    match code {
        api::ApiErrorCode::InvalidRequest => 10,
        api::ApiErrorCode::ValidationFailed => 11,
        api::ApiErrorCode::NotFound => 12,
        api::ApiErrorCode::Conflict => 13,
        api::ApiErrorCode::IoFailure => 14,
        api::ApiErrorCode::ExternalFailure => 15,
        api::ApiErrorCode::Unsupported => 16,
        api::ApiErrorCode::Internal => 17,
    }
}

#[derive(Debug, Serialize)]
struct CapabilityDefinitionView<'a> {
    id: api::CapabilityId,
    domain: api::CapabilityDomain,
    operation: api::capabilities::CapabilityOperation,
    request_contract: &'a str,
    response_contract: &'a str,
    code_paths: &'a [&'a str],
    notes: &'a str,
}

impl<'a> From<&'a api::capabilities::CapabilityDefinition> for CapabilityDefinitionView<'a> {
    fn from(value: &'a api::capabilities::CapabilityDefinition) -> Self {
        Self {
            id: value.id,
            domain: value.domain,
            operation: value.operation,
            request_contract: value.request_contract,
            response_contract: value.response_contract,
            code_paths: value.code_paths,
            notes: value.notes,
        }
    }
}

#[derive(Debug, Default)]
struct LaunchOptions {
    send_file: Option<PathBuf>,
    output_mode: CliOutputMode,
    verbose: bool,
    command: Option<RootCommand>,
}

fn parse_launch_options<I>(args: I) -> io::Result<LaunchOptions>
where
    I: IntoIterator<Item = String>,
{
    let mut argv = vec!["metaagent-rust".to_string()];
    argv.extend(args);
    let parsed = LaunchCli::try_parse_from(argv).map_err(|err| {
        if matches!(
            err.kind(),
            clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
        ) {
            print!("{err}");
            std::process::exit(0);
        }
        let message = err.to_string();
        if matches!(err.kind(), clap::error::ErrorKind::UnknownArgument) {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("Unknown argument: {message}"),
            )
        } else {
            io::Error::new(io::ErrorKind::InvalidInput, message)
        }
    })?;
    Ok(LaunchOptions {
        send_file: parsed.send_file,
        output_mode: parsed.output_mode,
        verbose: parsed.verbose,
        command: parsed.command,
    })
}

#[cfg(test)]
#[path = "../tests/unit/main_launch_tests.rs"]
mod launch_tests;
