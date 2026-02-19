use std::collections::HashMap;
use std::io;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::agent::{BackendKind, CodexAdapter};
use crate::agent_models::{CodexAgentKind, CodexAgentModelRouting};
use crate::app::App;
use crate::artifact_io::{read_text_file, write_text_file};
use crate::deterministic::TestRunnerAdapter;
use crate::session_store::{SessionStore, TaskFailFileEntry};
use crate::subagents;
use crate::workflow::{JobRun, StartedJob, WorkerRole, WorkflowFailure, WorkflowFailureKind};

#[derive(Debug, Clone)]
pub struct TaskWriteBaseline {
    pub tasks_json: String,
}

#[derive(Debug)]
pub struct WorkerCompletionOutcome {
    pub failure_report_prompt: Option<String>,
    pub context_report_prompt: Option<String>,
    pub started_job: Option<StartedJob>,
    pub warnings: Vec<String>,
}

pub trait CoreOrchestrationService {
    fn claim_next_worker_job_and_persist_snapshot(
        &self,
        app: &mut App,
        session_store: &SessionStore,
    ) -> io::Result<Option<StartedJob>>;

    fn dispatch_worker_job(
        &self,
        job: &StartedJob,
        worker_agent_adapters: &mut HashMap<String, CodexAdapter>,
        active_worker_context_key: &mut Option<String>,
        test_runner_adapter: &TestRunnerAdapter,
        session_store: &SessionStore,
        model_routing: &CodexAgentModelRouting,
    );

    fn start_next_worker_job_if_any(
        &self,
        app: &mut App,
        worker_agent_adapters: &mut HashMap<String, CodexAdapter>,
        active_worker_context_key: &mut Option<String>,
        test_runner_adapter: &TestRunnerAdapter,
        session_store: &SessionStore,
        model_routing: &CodexAgentModelRouting,
    ) -> io::Result<Option<StartedJob>>;

    fn capture_tasks_baseline(&self, session_store: &SessionStore) -> Option<TaskWriteBaseline>;

    fn build_exhausted_loop_failures_prompt(
        &self,
        session_store: &SessionStore,
        master_report_session_intro_needed: &mut bool,
        project_info_text: Option<&str>,
        failures: Vec<WorkflowFailure>,
    ) -> io::Result<Option<String>>;

    #[allow(clippy::too_many_arguments)]
    fn complete_worker_cycle_and_start_next(
        &self,
        app: &mut App,
        success: bool,
        code: i32,
        worker_agent_adapters: &mut HashMap<String, CodexAdapter>,
        active_worker_context_key: &mut Option<String>,
        test_runner_adapter: &TestRunnerAdapter,
        session_store: &SessionStore,
        model_routing: &CodexAgentModelRouting,
        master_report_session_intro_needed: &mut bool,
        project_info_text: Option<&str>,
    ) -> WorkerCompletionOutcome;
}

pub trait UiPromptService {
    fn build_master_prompt_for_message(
        &self,
        app: &App,
        message: &str,
        session_store: &SessionStore,
        project_info_text: Option<&str>,
        master_session_intro_needed: &mut bool,
    ) -> String;

    fn build_convert_master_prompt(
        &self,
        app: &App,
        session_store: &SessionStore,
        project_info_text: Option<&str>,
        master_session_intro_needed: &mut bool,
    ) -> String;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultCoreOrchestrationService;

#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultUiPromptService;

impl CoreOrchestrationService for DefaultCoreOrchestrationService {
    fn claim_next_worker_job_and_persist_snapshot(
        &self,
        app: &mut App,
        session_store: &SessionStore,
    ) -> io::Result<Option<StartedJob>> {
        let job = app.start_next_worker_job();
        self.persist_runtime_tasks_snapshot(app, session_store)?;
        Ok(job)
    }

    fn dispatch_worker_job(
        &self,
        job: &StartedJob,
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
                worker_agent_adapters
                    .entry(key.clone())
                    .or_insert_with(|| build_worker_adapter(model_routing, job.role));
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
        &self,
        app: &mut App,
        worker_agent_adapters: &mut HashMap<String, CodexAdapter>,
        active_worker_context_key: &mut Option<String>,
        test_runner_adapter: &TestRunnerAdapter,
        session_store: &SessionStore,
        model_routing: &CodexAgentModelRouting,
    ) -> io::Result<Option<StartedJob>> {
        let Some(job) = self.claim_next_worker_job_and_persist_snapshot(app, session_store)? else {
            return Ok(None);
        };
        self.dispatch_worker_job(
            &job,
            worker_agent_adapters,
            active_worker_context_key,
            test_runner_adapter,
            session_store,
            model_routing,
        );
        Ok(Some(job))
    }

    fn capture_tasks_baseline(&self, session_store: &SessionStore) -> Option<TaskWriteBaseline> {
        let tasks_json = read_text_file(session_store.tasks_file()).ok()?;
        Some(TaskWriteBaseline { tasks_json })
    }

    fn build_exhausted_loop_failures_prompt(
        &self,
        session_store: &SessionStore,
        master_report_session_intro_needed: &mut bool,
        project_info_text: Option<&str>,
        failures: Vec<WorkflowFailure>,
    ) -> io::Result<Option<String>> {
        if failures.is_empty() {
            return Ok(None);
        }
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
        session_store.append_task_fails(&fail_entries)?;
        let has_test_failure = fail_entries.iter().any(|entry| entry.kind == "test");
        let prompt = subagents::build_failure_report_prompt(
            &session_store.task_fails_file().display().to_string(),
            &fail_entries,
            has_test_failure,
        );
        Ok(Some(subagents::build_session_intro_if_needed(
            &prompt,
            session_store.session_dir().display().to_string().as_str(),
            &session_store.session_meta_file().display().to_string(),
            project_info_text,
            master_report_session_intro_needed,
        )))
    }

    fn complete_worker_cycle_and_start_next(
        &self,
        app: &mut App,
        success: bool,
        code: i32,
        worker_agent_adapters: &mut HashMap<String, CodexAdapter>,
        active_worker_context_key: &mut Option<String>,
        test_runner_adapter: &TestRunnerAdapter,
        session_store: &SessionStore,
        model_routing: &CodexAgentModelRouting,
        master_report_session_intro_needed: &mut bool,
        project_info_text: Option<&str>,
    ) -> WorkerCompletionOutcome {
        let mut warnings = Vec::new();
        let mut failure_report_prompt = None;
        let mut context_report_prompt = None;

        let new_context_entries = app.on_worker_completed(success, code);
        let exhausted_failures = app.drain_worker_failures();
        if !exhausted_failures.is_empty() {
            match self.build_exhausted_loop_failures_prompt(
                session_store,
                master_report_session_intro_needed,
                project_info_text,
                exhausted_failures,
            ) {
                Ok(prompt) => {
                    failure_report_prompt = prompt;
                }
                Err(err) => warnings.push(format!("Failed to append task-fails.json: {err}")),
            }
        }

        if !new_context_entries.is_empty() {
            if let Err(err) = session_store.write_rolling_context(&app.rolling_context_entries()) {
                warnings.push(format!("Failed to persist rolling_context.json: {err}"));
            }
            let prompt = app.prepare_context_report_prompt(&new_context_entries);
            context_report_prompt = Some(subagents::build_session_intro_if_needed(
                &prompt,
                session_store.session_dir().display().to_string().as_str(),
                &session_store.session_meta_file().display().to_string(),
                project_info_text,
                master_report_session_intro_needed,
            ));
        }

        let started_job = match self.start_next_worker_job_if_any(
            app,
            worker_agent_adapters,
            active_worker_context_key,
            test_runner_adapter,
            session_store,
            model_routing,
        ) {
            Ok(job) => job,
            Err(err) => {
                warnings.push(format!(
                    "Failed to persist runtime task status to tasks.json: {err}"
                ));
                None
            }
        };

        WorkerCompletionOutcome {
            failure_report_prompt,
            context_report_prompt,
            started_job,
            warnings,
        }
    }
}

impl UiPromptService for DefaultUiPromptService {
    fn build_master_prompt_for_message(
        &self,
        app: &App,
        message: &str,
        session_store: &SessionStore,
        project_info_text: Option<&str>,
        master_session_intro_needed: &mut bool,
    ) -> String {
        let session_dir = session_store.session_dir().display().to_string();
        let session_meta_file = session_store.session_meta_file().display().to_string();
        let tasks_file = session_store.tasks_file().display().to_string();
        let planner_file = session_store.planner_file().display().to_string();
        let project_info_file = session_store.project_info_file().display().to_string();
        let prompt = if app.is_planner_mode() {
            app.prepare_planner_prompt(message, &planner_file, &project_info_file)
        } else {
            app.prepare_master_prompt(message, &tasks_file)
        };
        subagents::build_session_intro_if_needed(
            &prompt,
            session_dir.as_str(),
            &session_meta_file,
            project_info_text,
            master_session_intro_needed,
        )
    }

    fn build_convert_master_prompt(
        &self,
        app: &App,
        session_store: &SessionStore,
        project_info_text: Option<&str>,
        master_session_intro_needed: &mut bool,
    ) -> String {
        let session_dir = session_store.session_dir().display().to_string();
        let session_meta_file = session_store.session_meta_file().display().to_string();
        let tasks_file = session_store.tasks_file().display().to_string();
        let planner_file = session_store.planner_file().display().to_string();
        let command_prompt = subagents::build_convert_plan_prompt(&planner_file, &tasks_file);
        let master_prompt = app.prepare_master_prompt(&command_prompt, &tasks_file);
        subagents::build_session_intro_if_needed(
            &master_prompt,
            session_dir.as_str(),
            &session_meta_file,
            project_info_text,
            master_session_intro_needed,
        )
    }
}

impl DefaultCoreOrchestrationService {
    fn persist_runtime_tasks_snapshot(
        &self,
        app: &App,
        session_store: &SessionStore,
    ) -> io::Result<()> {
        let tasks = app.planner_tasks_for_file();
        let text = serde_json::to_string_pretty(&tasks).map_err(io::Error::other)?;
        write_text_file(session_store.tasks_file(), &text)
    }
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
    let mut config = model_routing.base_command_config();
    config.output_mode = if matches!(config.backend_kind(), BackendKind::Claude) {
        crate::agent::AdapterOutputMode::JsonAssistantOnly
    } else {
        crate::agent::AdapterOutputMode::PlainText
    };
    config.persistent_session = true;
    config.skip_reader_join_after_wait = true;
    let profile = model_routing.profile_for(worker_role_agent_kind(role));
    if matches!(config.backend_kind(), BackendKind::Codex) {
        config.model = Some(profile.model.clone());
        config.model_reasoning_effort = profile.thinking_effort;
    } else {
        config.model = None;
        config.model_reasoning_effort = None;
    }
    CodexAdapter::with_config(config)
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

#[cfg(test)]
#[path = "../tests/unit/services_tests.rs"]
mod tests;
