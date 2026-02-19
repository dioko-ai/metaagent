use std::collections::{HashSet, VecDeque};

mod implementation_auditor;
mod implementor;
mod test_auditor;
mod test_runner;
mod test_writer;

use crate::session_store::{
    PlannerTaskDocFileEntry, PlannerTaskFileEntry, PlannerTaskKindFile, PlannerTaskStatusFile,
};

const FILES_CHANGED_BEGIN: &str = "FILES_CHANGED_BEGIN";
const FILES_CHANGED_END: &str = "FILES_CHANGED_END";
const MAX_AUDIT_RETRIES: u8 = 4;
const MAX_TEST_RETRIES: u8 = 5;
const MAX_FINAL_AUDIT_RETRIES: u8 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Pending,
    InProgress,
    NeedsChanges,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerRole {
    Implementor,
    Auditor,
    TestWriter,
    TestRunner,
    FinalAudit,
}

#[derive(Debug, Clone)]
pub enum JobRun {
    AgentPrompt(String),
    DeterministicTestRun,
}

#[derive(Debug, Clone)]
pub struct StartedJob {
    pub run: JobRun,
    pub role: WorkerRole,
    pub top_task_id: u64,
    pub parent_context_key: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ActiveJobMeta {
    pub role: WorkerRole,
    pub top_task_id: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowFailureKind {
    Audit,
    Test,
}

#[derive(Debug, Clone)]
pub struct WorkflowFailure {
    pub kind: WorkflowFailureKind,
    pub top_task_id: u64,
    pub top_task_title: String,
    pub attempts: u8,
    pub reason: String,
    pub action_taken: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TaskKind {
    Top,
    FinalAudit,
    Implementor,
    Auditor,
    TestWriter,
    TestRunner,
}

#[derive(Debug, Clone)]
struct TaskNode {
    id: u64,
    external_id: Option<String>,
    title: String,
    details: String,
    docs: Vec<PlannerTaskDocFileEntry>,
    status: TaskStatus,
    kind: TaskKind,
    children: Vec<TaskNode>,
}

#[derive(Debug, Clone)]
pub struct RightPaneToggleLine {
    pub line_index: usize,
    pub task_key: String,
}

#[derive(Debug, Clone)]
pub struct RightPaneBlockView {
    pub lines: Vec<String>,
    pub toggles: Vec<RightPaneToggleLine>,
}

#[derive(Debug, Clone)]
enum WorkerJobKind {
    Implementor {
        implementor_id: u64,
        pass: u8,
        feedback: Option<String>,
        resume_auditor_id: Option<u64>,
        resume_audit_pass: Option<u8>,
    },
    Auditor {
        implementor_id: u64,
        auditor_id: u64,
        pass: u8,
        implementation_report: Option<String>,
        changed_files_summary: Option<String>,
    },
    TestWriterAuditor {
        test_writer_id: u64,
        auditor_id: u64,
        pass: u8,
        test_report: Option<String>,
    },
    TestWriter {
        test_writer_id: u64,
        pass: u8,
        feedback: Option<String>,
        skip_test_runner_on_success: bool,
        resume_auditor_id: Option<u64>,
        resume_audit_pass: Option<u8>,
    },
    TestRunner {
        test_writer_id: u64,
        test_runner_id: u64,
        pass: u8,
    },
    ImplementorTestRunner {
        implementor_id: u64,
        test_runner_id: u64,
        pass: u8,
    },
    FinalAudit {
        final_audit_id: u64,
        pass: u8,
        feedback: Option<String>,
    },
}

impl WorkerJobKind {
    fn role(&self) -> WorkerRole {
        match self {
            WorkerJobKind::Implementor { .. } => WorkerRole::Implementor,
            WorkerJobKind::Auditor { .. } => WorkerRole::Auditor,
            WorkerJobKind::TestWriterAuditor { .. } => WorkerRole::Auditor,
            WorkerJobKind::TestWriter { .. } => WorkerRole::TestWriter,
            WorkerJobKind::TestRunner { .. } => WorkerRole::TestRunner,
            WorkerJobKind::ImplementorTestRunner { .. } => WorkerRole::TestRunner,
            WorkerJobKind::FinalAudit { .. } => WorkerRole::FinalAudit,
        }
    }

    fn parent_context_key(&self) -> Option<String> {
        match self {
            WorkerJobKind::Implementor { implementor_id, .. } => {
                Some(format!("implementor:{implementor_id}"))
            }
            WorkerJobKind::Auditor { auditor_id, .. } => Some(format!("auditor:{auditor_id}")),
            WorkerJobKind::ImplementorTestRunner { implementor_id, .. } => {
                Some(format!("implementor:{implementor_id}"))
            }
            WorkerJobKind::TestWriter { test_writer_id, .. } => {
                Some(format!("test_writer:{test_writer_id}"))
            }
            WorkerJobKind::TestWriterAuditor { auditor_id, .. } => {
                Some(format!("test_auditor:{auditor_id}"))
            }
            WorkerJobKind::TestRunner { test_writer_id, .. } => {
                Some(format!("test_writer:{test_writer_id}"))
            }
            WorkerJobKind::FinalAudit { final_audit_id, .. } => {
                Some(format!("final_audit:{final_audit_id}"))
            }
        }
    }
}

#[derive(Debug, Clone)]
struct WorkerJob {
    top_task_id: u64,
    kind: WorkerJobKind,
}

#[derive(Debug, Clone)]
struct ActiveJob {
    job: WorkerJob,
    transcript: Vec<String>,
}

#[derive(Debug)]
pub struct Workflow {
    tasks: Vec<TaskNode>,
    queue: VecDeque<WorkerJob>,
    active: Option<ActiveJob>,
    rolling_context: VecDeque<String>,
    max_context_entries: usize,
    next_id: u64,
    execution_enabled: bool,
    recent_failures: Vec<WorkflowFailure>,
    exhausted_final_audits: HashSet<u64>,
}

impl Default for Workflow {
    fn default() -> Self {
        Self {
            tasks: Vec::new(),
            queue: VecDeque::new(),
            active: None,
            rolling_context: VecDeque::new(),
            max_context_entries: 16,
            next_id: 1,
            execution_enabled: false,
            recent_failures: Vec::new(),
            exhausted_final_audits: HashSet::new(),
        }
    }
}

impl Workflow {
    pub fn prepare_master_prompt(&self, user_message: &str) -> String {
        format!(
            "You are the master Codex agent in a TUI.\n\
             Primary responsibilities:\n\
             1) Answer user questions clearly and directly.\n\
             2) Collaboratively maintain and update the task tree state.\n\
             3) If the user asks for additional changes after prior tasks are done, append new tasks instead of replacing completed history.\n\
             4) After task-list updates are ready, tell the user `/start` is ready to run.\n\
             Execution is currently {}. Only start execution when user explicitly asks to start.\n\
             `/start` always resumes from the last unfinished task.\n\
             Rolling task context:\n{}\n\
             Current task tree:\n{}\n\
             User message:\n{}\n\
             Keep your conversational response concise.",
            if self.execution_enabled {
                "enabled"
            } else {
                "disabled"
            },
            self.context_block(),
            self.task_tree_compact(),
            user_message
        )
    }

    pub fn rolling_context_entries(&self) -> Vec<String> {
        self.rolling_context.iter().cloned().collect()
    }

    pub fn replace_rolling_context_entries(&mut self, entries: Vec<String>) {
        self.rolling_context.clear();
        let keep = entries.len().saturating_sub(self.max_context_entries);
        for entry in entries.into_iter().skip(keep) {
            self.rolling_context.push_back(entry);
        }
    }

    pub fn planner_tasks_for_file(&self) -> Vec<PlannerTaskFileEntry> {
        fn file_id_for_node(node: &TaskNode) -> String {
            node.external_id
                .clone()
                .unwrap_or_else(|| format!("internal-{}", node.id))
        }

        fn collect(
            node: &TaskNode,
            parent_id: Option<&str>,
            order: u32,
            out: &mut Vec<PlannerTaskFileEntry>,
        ) {
            let node_id = file_id_for_node(node);
            out.push(PlannerTaskFileEntry {
                id: node_id.clone(),
                title: node.title.clone(),
                details: node.details.clone(),
                docs: node.docs.clone(),
                kind: task_kind_to_file(node.kind),
                status: task_status_to_file(node.status),
                parent_id: parent_id.map(ToString::to_string),
                order: Some(order),
            });
            for (idx, child) in node.children.iter().enumerate() {
                collect(child, Some(&node_id), idx as u32, out);
            }
        }

        let mut out = Vec::new();
        for (idx, task) in self.ordered_root_nodes().iter().enumerate() {
            collect(task, None, idx as u32, &mut out);
        }
        out
    }

    pub fn reset_execution_runtime(&mut self) {
        self.execution_enabled = false;
        self.queue.clear();
        self.active = None;
        self.recent_failures.clear();
        self.exhausted_final_audits.clear();
    }

    pub fn sync_planner_tasks_from_file(
        &mut self,
        entries: Vec<PlannerTaskFileEntry>,
    ) -> Result<usize, String> {
        let execution_busy = self.active.is_some() || !self.queue.is_empty();
        if self.execution_enabled && execution_busy {
            return Err("Cannot reload planner tasks while execution is enabled".to_string());
        }
        if self.execution_enabled {
            // If execution was enabled but no worker is active/queued, switch back to planning
            // mode so task reloads can proceed.
            self.reset_execution_runtime();
        }

        let mut id_to_num = std::collections::HashMap::<String, u64>::new();
        for entry in &entries {
            if entry.id.trim().is_empty() {
                return Err("Planner task id cannot be empty".to_string());
            }
            if entry.details.trim().is_empty() {
                return Err(format!(
                    "Planner task {} must include non-empty details",
                    entry.id
                ));
            }
            if id_to_num.contains_key(&entry.id) {
                return Err(format!("Duplicate planner task id {}", entry.id));
            }
            let num = self.alloc_id();
            id_to_num.insert(entry.id.clone(), num);
        }

        for entry in &entries {
            if let Some(parent) = &entry.parent_id
                && !id_to_num.contains_key(parent)
            {
                return Err(format!(
                    "Planner task {} references missing parent_id {}",
                    entry.id, parent
                ));
            }
        }

        let mut children_map =
            std::collections::HashMap::<Option<String>, Vec<&PlannerTaskFileEntry>>::new();
        for entry in &entries {
            children_map
                .entry(entry.parent_id.clone())
                .or_default()
                .push(entry);
        }
        for children in children_map.values_mut() {
            children.sort_by_key(|entry| entry.order.unwrap_or(u32::MAX));
        }

        fn build_nodes(
            parent: Option<&str>,
            children_map: &std::collections::HashMap<Option<String>, Vec<&PlannerTaskFileEntry>>,
            id_to_num: &std::collections::HashMap<String, u64>,
            visited: &mut std::collections::HashSet<String>,
            stack: &mut std::collections::HashSet<String>,
            is_root_level: bool,
        ) -> Result<Vec<TaskNode>, String> {
            let key = parent.map(ToString::to_string);
            let mut out = Vec::new();
            if let Some(children) = children_map.get(&key) {
                for entry in children {
                    if stack.contains(&entry.id) {
                        return Err(format!("Cycle detected in planner tasks at {}", entry.id));
                    }
                    stack.insert(entry.id.clone());
                    visited.insert(entry.id.clone());
                    let child_nodes = build_nodes(
                        Some(&entry.id),
                        children_map,
                        id_to_num,
                        visited,
                        stack,
                        false,
                    )?;
                    stack.remove(&entry.id);
                    let kind = task_kind_from_file(entry.kind);
                    if is_root_level && !matches!(kind, TaskKind::Top | TaskKind::FinalAudit) {
                        return Err(format!(
                            "Root planner task {} must have kind \"task\" or \"final_audit\"",
                            entry.id
                        ));
                    }
                    out.push(TaskNode {
                        id: *id_to_num
                            .get(&entry.id)
                            .ok_or_else(|| format!("Missing numeric id for {}", entry.id))?,
                        external_id: Some(entry.id.clone()),
                        title: entry.title.trim().to_string(),
                        details: entry.details.trim().to_string(),
                        docs: entry.docs.clone(),
                        status: match entry.status {
                            PlannerTaskStatusFile::Pending => TaskStatus::Pending,
                            PlannerTaskStatusFile::InProgress => TaskStatus::InProgress,
                            PlannerTaskStatusFile::NeedsChanges => TaskStatus::NeedsChanges,
                            PlannerTaskStatusFile::Done => TaskStatus::Done,
                        },
                        kind,
                        children: child_nodes,
                    });
                }
            }
            Ok(out)
        }

        let mut visited = std::collections::HashSet::new();
        let mut stack = std::collections::HashSet::new();
        let root_nodes = build_nodes(
            None,
            &children_map,
            &id_to_num,
            &mut visited,
            &mut stack,
            true,
        )?;
        if visited.len() != entries.len() {
            return Err("Planner task graph has disconnected/cyclic nodes".to_string());
        }
        validate_required_subtask_structure(&root_nodes)?;

        self.tasks = root_nodes;
        self.queue.clear();
        self.active = None;
        self.recent_failures.clear();
        self.exhausted_final_audits.clear();
        Ok(entries.len())
    }

    pub fn start_execution(&mut self) -> Vec<String> {
        if self.execution_enabled {
            if self.active.is_some() {
                return vec![
                    "System: Execution is already running; continuing current task.".to_string(),
                ];
            }
            let queued = self.enqueue_ready_top_tasks();
            if queued > 0 {
                return vec![format!(
                    "System: Resumed from last unfinished task(s). Queued {} task job(s).",
                    queued
                )];
            }
            return vec![
                "System: Execution is already enabled. No unfinished tasks to resume.".to_string(),
            ];
        }

        self.execution_enabled = true;
        let queued = self.enqueue_ready_top_tasks();
        vec![format!(
            "System: Execution enabled. Queued {} task job(s).",
            queued
        )]
    }

    #[cfg(test)]
    pub fn execution_enabled(&self) -> bool {
        self.execution_enabled
    }

    pub fn execution_busy(&self) -> bool {
        self.execution_enabled && (self.active.is_some() || !self.queue.is_empty())
    }

    pub fn start_next_job(&mut self) -> Option<StartedJob> {
        if !self.execution_enabled || self.active.is_some() {
            return None;
        }
        let job = self.queue.pop_front()?;
        self.mark_job_started(&job);
        let role = job.kind.role();
        let run = self.run_for_job(&job);
        let started = StartedJob {
            run,
            role,
            top_task_id: job.top_task_id,
            parent_context_key: job.kind.parent_context_key(),
        };
        self.active = Some(ActiveJob {
            job,
            transcript: Vec::new(),
        });
        Some(started)
    }

    pub fn active_job_meta(&self) -> Option<ActiveJobMeta> {
        self.active.as_ref().map(|active| ActiveJobMeta {
            role: active.job.kind.role(),
            top_task_id: active.job.top_task_id,
        })
    }

    pub fn append_active_output(&mut self, line: String) {
        if let Some(active) = self.active.as_mut() {
            active.transcript.push(line);
        }
    }

    pub fn finish_active_job(&mut self, success: bool, code: i32) -> Vec<String> {
        let Some(active) = self.active.take() else {
            return Vec::new();
        };

        let job = active.job;
        let transcript = active.transcript;
        let mut messages = Vec::new();

        match job.kind {
            WorkerJobKind::Implementor {
                implementor_id,
                pass,
                resume_auditor_id,
                resume_audit_pass,
                ..
            } => {
                implementor::on_completion(
                    self,
                    job.top_task_id,
                    implementor_id,
                    pass,
                    resume_auditor_id,
                    resume_audit_pass,
                    &transcript,
                    success,
                    code,
                    &mut messages,
                );
            }
            WorkerJobKind::Auditor {
                implementor_id,
                auditor_id,
                pass,
                implementation_report,
                changed_files_summary,
                ..
            } => {
                implementation_auditor::on_completion(
                    self,
                    job.top_task_id,
                    implementor_id,
                    auditor_id,
                    pass,
                    implementation_report,
                    changed_files_summary,
                    &transcript,
                    success,
                    code,
                    &mut messages,
                );
            }
            WorkerJobKind::TestWriter {
                test_writer_id,
                pass,
                skip_test_runner_on_success,
                resume_auditor_id,
                resume_audit_pass,
                ..
            } => {
                test_writer::on_completion(
                    self,
                    job.top_task_id,
                    test_writer_id,
                    pass,
                    skip_test_runner_on_success,
                    resume_auditor_id,
                    resume_audit_pass,
                    &transcript,
                    success,
                    code,
                    &mut messages,
                );
            }
            WorkerJobKind::TestWriterAuditor {
                test_writer_id,
                auditor_id,
                pass,
                test_report,
            } => {
                test_auditor::on_completion(
                    self,
                    job.top_task_id,
                    test_writer_id,
                    auditor_id,
                    pass,
                    test_report,
                    &transcript,
                    success,
                    code,
                    &mut messages,
                );
            }
            WorkerJobKind::TestRunner {
                test_writer_id,
                test_runner_id,
                pass,
            } => {
                test_runner::on_writer_completion(
                    self,
                    job.top_task_id,
                    test_writer_id,
                    test_runner_id,
                    pass,
                    &transcript,
                    success,
                    code,
                    &mut messages,
                );
            }
            WorkerJobKind::ImplementorTestRunner {
                implementor_id,
                test_runner_id,
                pass,
            } => {
                test_runner::on_implementor_completion(
                    self,
                    job.top_task_id,
                    implementor_id,
                    test_runner_id,
                    pass,
                    &transcript,
                    success,
                    code,
                    &mut messages,
                );
            }
            WorkerJobKind::FinalAudit {
                final_audit_id,
                pass,
                ..
            } => {
                self.push_context(make_context_summary(
                    "FinalAudit",
                    &self.task_title(job.top_task_id),
                    &transcript,
                    success,
                ));
                let explicit_pass = success && parse_audit_result_token(&transcript) == Some(true);
                if explicit_pass {
                    self.exhausted_final_audits.remove(&final_audit_id);
                    self.set_status(final_audit_id, TaskStatus::Done);
                    messages.push(format!(
                        "System: Final audit task #{} completed on pass {}.",
                        job.top_task_id, pass
                    ));
                } else {
                    self.set_status(final_audit_id, TaskStatus::NeedsChanges);
                    let feedback = audit_feedback(&transcript, code, success);
                    if pass >= MAX_FINAL_AUDIT_RETRIES {
                        self.exhausted_final_audits.insert(final_audit_id);
                        self.recent_failures.push(WorkflowFailure {
                            kind: WorkflowFailureKind::Audit,
                            top_task_id: job.top_task_id,
                            top_task_title: self.task_title(job.top_task_id),
                            attempts: pass,
                            reason: feedback.clone(),
                            action_taken: format!(
                                "Final audit retries exhausted at pass {}; stopped requeueing and awaiting user action.",
                                pass
                            ),
                        });
                        messages.push(format!(
                            "System: Final audit task #{} still failed at pass {}. Max retries ({}) reached; no further final-audit retries queued.",
                            job.top_task_id, pass, MAX_FINAL_AUDIT_RETRIES
                        ));
                    } else {
                        self.queue.push_back(WorkerJob {
                            top_task_id: job.top_task_id,
                            kind: WorkerJobKind::FinalAudit {
                                final_audit_id,
                                pass: pass.saturating_add(1),
                                feedback: Some(feedback.clone()),
                            },
                        });
                        messages.push(if success {
                            format!(
                                "System: Final audit task #{} did not explicitly pass; retry queued.",
                                job.top_task_id
                            )
                        } else {
                            format!(
                                "System: Final audit task #{} failed (code {}); retry queued.",
                                job.top_task_id, code
                            )
                        });
                    }
                }
            }
        }

        if self.execution_enabled {
            let _ = self.enqueue_ready_top_tasks();
        }
        messages
    }

    pub fn drain_recent_failures(&mut self) -> Vec<WorkflowFailure> {
        std::mem::take(&mut self.recent_failures)
    }

    pub fn right_pane_lines(&self) -> Vec<String> {
        let mut lines = vec!["Task Tree".to_string()];
        if self.tasks.is_empty() {
            lines.push("  (no tasks queued)".to_string());
        } else {
            for task in self.ordered_root_nodes() {
                render_tree(task, 0, &mut lines);
            }
        }

        lines.push(String::new());
        lines.push("Execution".to_string());
        lines.push(format!(
            "- status: {}",
            if self.execution_enabled {
                "running"
            } else {
                "planning"
            }
        ));

        lines.push(String::new());
        lines.push("Rolling Task Context".to_string());
        if self.rolling_context.is_empty() {
            lines.push("  (no context yet)".to_string());
        } else {
            for entry in &self.rolling_context {
                lines.push(format!("- {entry}"));
            }
        }

        lines
    }

    pub fn task_detail_keys(&self) -> HashSet<String> {
        let mut keys = HashSet::new();
        fn walk(node: &TaskNode, keys: &mut HashSet<String>) {
            let key = task_detail_key(node);
            keys.insert(key.clone());
            if node.kind != TaskKind::TestRunner && !node.docs.is_empty() {
                keys.insert(docs_toggle_key(&key));
            }
            for child in &node.children {
                walk(child, keys);
            }
        }
        for task in &self.tasks {
            walk(task, &mut keys);
        }
        keys
    }

    pub fn right_pane_block_view(
        &self,
        content_width: u16,
        expanded_detail_keys: &HashSet<String>,
    ) -> RightPaneBlockView {
        let mut lines = Vec::new();
        let mut toggles = Vec::new();
        if self.tasks.is_empty() {
            lines.push("  (no tasks queued)".to_string());
        } else {
            let width = content_width.max(8) as usize;
            let roots = self.ordered_root_nodes();
            let section_divider = format!("  {}", "─".repeat(width.saturating_sub(2).max(1)));
            for (idx, task) in roots.iter().enumerate() {
                if idx > 0 {
                    lines.push(String::new());
                    lines.push(section_divider.clone());
                    lines.push(String::new());
                }
                lines.push(format!("  {}. {}", idx + 1, task.title));
                lines.push(String::new());
                lines.extend(render_detail_lines(&task.details, width, false, 2, false));
                if task.kind != TaskKind::TestRunner && !task.docs.is_empty() {
                    let task_key = task_detail_key(task);
                    let docs_key = docs_toggle_key(&task_key);
                    let docs_collapsed = !expanded_detail_keys.contains(&docs_key);
                    let docs_line_index = lines.len();
                    lines.push(format!(
                        "  [documentation attached] {}",
                        detail_toggle_label(docs_collapsed)
                    ));
                    toggles.push(RightPaneToggleLine {
                        line_index: docs_line_index,
                        task_key: docs_key,
                    });
                    if !docs_collapsed {
                        lines.extend(render_docs_lines(&task.docs, width, 4));
                    }
                }
                lines.push(String::new());
                for child in &task.children {
                    render_subtree_box(
                        child,
                        width.saturating_sub(2).max(8),
                        &mut lines,
                        &mut toggles,
                        expanded_detail_keys,
                        2,
                    );
                }
            }
        }

        lines.push(String::new());
        lines.push("Execution".to_string());
        lines.push(format!(
            "- status: {}",
            if self.execution_enabled {
                "running"
            } else {
                "planning"
            }
        ));

        lines.push(String::new());
        lines.push("Rolling Task Context".to_string());
        if self.rolling_context.is_empty() {
            lines.push("  (no context yet)".to_string());
        } else {
            for entry in &self.rolling_context {
                lines.push(format!("- {entry}"));
            }
        }

        RightPaneBlockView { lines, toggles }
    }

    fn run_for_job(&self, job: &WorkerJob) -> JobRun {
        match &job.kind {
            WorkerJobKind::Implementor {
                implementor_id,
                feedback,
                ..
            } => {
                let prompt = implementor::build_prompt(
                    self,
                    job.top_task_id,
                    *implementor_id,
                    feedback.as_deref(),
                );
                JobRun::AgentPrompt(self.prepend_task_docs_to_prompt(*implementor_id, prompt))
            }
            WorkerJobKind::Auditor {
                implementor_id,
                auditor_id,
                implementation_report,
                changed_files_summary,
                pass,
                ..
            } => {
                let prompt = implementation_auditor::build_prompt(
                    self,
                    job.top_task_id,
                    *implementor_id,
                    *auditor_id,
                    implementation_report,
                    changed_files_summary,
                    *pass,
                );
                JobRun::AgentPrompt(self.prepend_task_docs_to_prompt(*auditor_id, prompt))
            }
            WorkerJobKind::TestWriterAuditor {
                auditor_id,
                test_writer_id,
                test_report,
                pass,
                ..
            } => {
                let prompt = test_auditor::build_prompt(
                    self,
                    job.top_task_id,
                    *test_writer_id,
                    *auditor_id,
                    test_report,
                    *pass,
                );
                JobRun::AgentPrompt(self.prepend_task_docs_to_prompt(*auditor_id, prompt))
            }
            WorkerJobKind::TestWriter {
                test_writer_id,
                feedback,
                skip_test_runner_on_success,
                ..
            } => {
                let prompt = test_writer::build_prompt(
                    self,
                    job.top_task_id,
                    *test_writer_id,
                    feedback.as_deref(),
                    *skip_test_runner_on_success,
                );
                JobRun::AgentPrompt(self.prepend_task_docs_to_prompt(*test_writer_id, prompt))
            }
            WorkerJobKind::TestRunner { .. } => JobRun::DeterministicTestRun,
            WorkerJobKind::ImplementorTestRunner { .. } => JobRun::DeterministicTestRun,
            WorkerJobKind::FinalAudit {
                final_audit_id,
                feedback,
                ..
            } => {
                let prompt = format!(
                    "You are a final audit sub-agent.\n\
                 Perform a holistic audit across all completed tasks and their outcomes.\n\
                 Focus on cross-task correctness, missing edge cases, integration risk, and overall quality gaps.\n\
                 Rolling task context:\n{}\n\
                 Current task tree:\n{}\n\
                 {}\n\
                 Response protocol (required):\n\
                 - First line must be exactly one of:\n\
                   AUDIT_RESULT: PASS\n\
                   AUDIT_RESULT: FAIL\n\
                 - Then provide concise findings. If PASS, include a brief rationale.\n\
                 - If FAIL, include concrete issues and suggested fixes.",
                    self.context_block(),
                    self.task_tree_compact(),
                    feedback
                        .as_deref()
                        .map(|f| format!("Previous final-audit feedback to address:\n{f}"))
                        .unwrap_or_else(|| "No prior final-audit feedback.".to_string())
                );
                JobRun::AgentPrompt(self.prepend_task_docs_to_prompt(*final_audit_id, prompt))
            }
        }
    }

    fn prepend_task_docs_to_prompt(&self, task_id: u64, prompt: String) -> String {
        let prefix = self.task_docs_prefix(task_id);
        if prefix.is_empty() {
            prompt
        } else {
            format!("{prefix}{prompt}")
        }
    }

    fn task_docs_prefix(&self, task_id: u64) -> String {
        let Some(node) = find_node(&self.tasks, task_id) else {
            return String::new();
        };
        if node.docs.is_empty() {
            return String::new();
        }

        let mut lines = vec![
            "Task documentation requirements:".to_string(),
            "- Before starting this task, read every linked document from the web.".to_string(),
            "- Use these docs as primary references while completing this task.".to_string(),
            "Task docs:".to_string(),
        ];

        for (idx, doc) in node.docs.iter().enumerate() {
            lines.push(format!("{}. {}", idx + 1, doc.title.trim()));
            lines.push(format!("   URL: {}", doc.url.trim()));
            if !doc.summary.trim().is_empty() {
                lines.push(format!("   Summary: {}", doc.summary.trim()));
            }
        }
        lines.push(String::new());
        lines.join("\n")
    }

    fn start_kind_for_top(&mut self, top_id: u64, kind: TaskKind, title: &str) -> Option<u64> {
        if let Some(existing) = find_node(&self.tasks, top_id)?
            .children
            .iter()
            .find(|child| child.kind == kind)
        {
            return Some(existing.id);
        }

        let id = self.alloc_id();
        if let Some(top) = find_node_mut(&mut self.tasks, top_id) {
            top.children.push(TaskNode {
                id,
                external_id: None,
                title: title.to_string(),
                details: default_generated_details(kind).to_string(),
                docs: Vec::new(),
                status: TaskStatus::Pending,
                kind,
                children: Vec::new(),
            });
        }
        Some(id)
    }

    fn find_or_create_child_kind(
        &mut self,
        parent_id: u64,
        kind: TaskKind,
        title: &str,
    ) -> Option<u64> {
        if let Some(existing) = find_node(&self.tasks, parent_id)?
            .children
            .iter()
            .find(|child| child.kind == kind)
        {
            return Some(existing.id);
        }

        let id = self.alloc_id();
        if let Some(parent) = find_node_mut(&mut self.tasks, parent_id) {
            parent.children.push(TaskNode {
                id,
                external_id: None,
                title: title.to_string(),
                details: default_generated_details(kind).to_string(),
                docs: Vec::new(),
                status: TaskStatus::Pending,
                kind,
                children: Vec::new(),
            });
        }
        Some(id)
    }

    fn find_child_kind(&self, parent_id: u64, kind: TaskKind) -> Option<u64> {
        find_node(&self.tasks, parent_id)?
            .children
            .iter()
            .find(|child| child.kind == kind)
            .map(|child| child.id)
    }

    fn find_next_pending_child_kind(&self, parent_id: u64, kind: TaskKind) -> Option<u64> {
        find_node(&self.tasks, parent_id)?
            .children
            .iter()
            .find(|child| child.kind == kind && child.status != TaskStatus::Done)
            .map(|child| child.id)
    }

    fn queue_next_implementor_audit(
        &mut self,
        top_task_id: u64,
        implementor_id: u64,
        pass: u8,
        implementation_report: Option<String>,
        changed_files_summary: Option<String>,
        messages: &mut Vec<String>,
    ) -> bool {
        let Some(auditor_id) = self.find_next_pending_child_kind(implementor_id, TaskKind::Auditor)
        else {
            if let Some(test_runner_id) =
                self.find_next_pending_child_kind(implementor_id, TaskKind::TestRunner)
            {
                self.queue.push_back(WorkerJob {
                    top_task_id,
                    kind: WorkerJobKind::ImplementorTestRunner {
                        implementor_id,
                        test_runner_id,
                        pass,
                    },
                });
                messages.push(format!(
                    "System: Task #{} all audits complete; existing-test runner queued.",
                    top_task_id
                ));
                return true;
            }
            self.set_status(implementor_id, TaskStatus::Done);
            messages.push(format!(
                "System: Task #{} implementation branch passed all audits.",
                top_task_id
            ));
            self.try_mark_top_done(top_task_id, messages);
            return false;
        };
        self.queue.push_back(WorkerJob {
            top_task_id,
            kind: WorkerJobKind::Auditor {
                implementor_id,
                auditor_id,
                pass,
                implementation_report,
                changed_files_summary,
            },
        });
        messages.push(format!(
            "System: Task #{} audit queued (audit pass {}).",
            top_task_id, pass
        ));
        true
    }

    fn enqueue_ready_top_tasks(&mut self) -> usize {
        let root_ids: Vec<u64> = self
            .ordered_root_nodes()
            .iter()
            .filter(|node| matches!(node.kind, TaskKind::Top | TaskKind::FinalAudit))
            .map(|node| node.id)
            .collect();

        let mut queued = 0usize;
        let mut on_completion_messages = Vec::<String>::new();
        let mut non_final_all_done = self
            .tasks
            .iter()
            .filter(|node| node.kind != TaskKind::FinalAudit)
            .all(|node| node.status == TaskStatus::Done);

        if !non_final_all_done {
            self.queue
                .retain(|job| !matches!(job.kind, WorkerJobKind::FinalAudit { .. }));
        }

        for top_id in &root_ids {
            let Some(top) = find_node(&self.tasks, *top_id) else {
                continue;
            };
            let top_status = top.status;
            let top_kind = top.kind;
            let top_children_empty = top.children.is_empty();
            let has_existing_top_level_test_writer = top
                .children
                .iter()
                .any(|child| child.kind == TaskKind::TestWriter);

            if top_status == TaskStatus::Done {
                continue;
            }

            if top_kind == TaskKind::FinalAudit {
                break;
            }

            let has_top_level_test_writer = if top_children_empty {
                if let Some(test_writer_id) =
                    self.start_kind_for_top(*top_id, TaskKind::TestWriter, "Test Writing")
                {
                    let _ = self.find_or_create_child_kind(
                        test_writer_id,
                        TaskKind::TestRunner,
                        "Deterministic Test Run",
                    );
                }
                true
            } else {
                has_existing_top_level_test_writer
            };

            let Some(implementor_id) =
                self.start_kind_for_top(*top_id, TaskKind::Implementor, "Implementation")
            else {
                continue;
            };
            self.find_or_create_child_kind(implementor_id, TaskKind::Auditor, "Audit");

            if let Some(implementor_id) =
                self.find_next_pending_child_kind(*top_id, TaskKind::Implementor)
            {
                if !self.branch_has_active_or_queued(*top_id, TaskKind::Implementor) {
                    let impl_status = self.status_of(implementor_id);
                    let has_pending_impl_audit = self
                        .find_next_pending_child_kind(implementor_id, TaskKind::Auditor)
                        .is_some();
                    if impl_status == Some(TaskStatus::InProgress) && has_pending_impl_audit {
                        // Backward-compat: older runs can leave implementor InProgress while
                        // an auditor is pending/running. Resume at auditor, not implementor.
                        self.set_status(implementor_id, TaskStatus::Done);
                        if self.queue_next_implementor_audit(
                            *top_id,
                            implementor_id,
                            1,
                            None,
                            None,
                            &mut on_completion_messages,
                        ) {
                            queued += 1;
                        }
                        return queued;
                    }
                    self.queue.push_back(WorkerJob {
                        top_task_id: *top_id,
                        kind: WorkerJobKind::Implementor {
                            implementor_id,
                            pass: 1,
                            feedback: None,
                            resume_auditor_id: None,
                            resume_audit_pass: None,
                        },
                    });
                    queued += 1;
                }
                return queued;
            }

            if self.branch_has_active_or_queued(*top_id, TaskKind::Implementor) {
                return queued;
            }

            let queued_next_impl_step = self.queue_next_implementor_audit(
                *top_id,
                implementor_id,
                1,
                None,
                None,
                &mut on_completion_messages,
            );
            if queued_next_impl_step {
                return queued;
            }

            if has_top_level_test_writer {
                if let Some(test_writer_id) =
                    self.find_next_pending_child_kind(*top_id, TaskKind::TestWriter)
                {
                    if !self.branch_has_active_or_queued(*top_id, TaskKind::TestWriter) {
                        self.queue.push_front(WorkerJob {
                            top_task_id: *top_id,
                            kind: WorkerJobKind::TestWriter {
                                test_writer_id,
                                pass: 1,
                                feedback: None,
                                skip_test_runner_on_success: false,
                                resume_auditor_id: None,
                                resume_audit_pass: None,
                            },
                        });
                        queued += 1;
                    }
                    return queued;
                }
            }
            // No additional work was enqueued for this top; allow scanning subsequent top-level
            // tasks (including final-audit scheduling) when non-final work is complete.
        }

        non_final_all_done = self
            .tasks
            .iter()
            .filter(|node| node.kind != TaskKind::FinalAudit)
            .all(|node| node.status == TaskStatus::Done);

        if non_final_all_done {
            for top_id in root_ids {
                let Some(top) = find_node(&self.tasks, top_id) else {
                    continue;
                };
                if top.kind != TaskKind::FinalAudit || top.status == TaskStatus::Done {
                    continue;
                }
                if self.exhausted_final_audits.contains(&top_id) {
                    continue;
                }
                if self.final_audit_has_active_or_queued(top_id) {
                    continue;
                }
                self.queue.push_back(WorkerJob {
                    top_task_id: top_id,
                    kind: WorkerJobKind::FinalAudit {
                        final_audit_id: top_id,
                        pass: 1,
                        feedback: None,
                    },
                });
                queued += 1;
            }
        }

        queued
    }

    fn final_audit_has_active_or_queued(&self, final_audit_id: u64) -> bool {
        if self.active.as_ref().is_some_and(|active| {
            matches!(
                active.job.kind,
                WorkerJobKind::FinalAudit { final_audit_id: id, .. } if id == final_audit_id
            )
        }) {
            return true;
        }
        self.queue.iter().any(|job| {
            matches!(
                job.kind,
                WorkerJobKind::FinalAudit { final_audit_id: id, .. } if id == final_audit_id
            )
        })
    }

    fn queue_test_writer_next_step(
        &mut self,
        top_task_id: u64,
        test_writer_id: u64,
        pass: u8,
        allow_test_writer_auditor: bool,
        test_report: Option<String>,
        messages: &mut Vec<String>,
    ) -> bool {
        if allow_test_writer_auditor
            && let Some(auditor_id) =
                self.find_next_pending_child_kind(test_writer_id, TaskKind::Auditor)
        {
            self.queue.push_back(WorkerJob {
                top_task_id,
                kind: WorkerJobKind::TestWriterAuditor {
                    test_writer_id,
                    auditor_id,
                    pass,
                    test_report,
                },
            });
            messages.push(format!(
                "System: Task #{} test-writer pass {} complete; test-writer audit queued.",
                top_task_id, pass
            ));
            return true;
        }

        let Some(test_runner_id) = self.find_or_create_child_kind(
            test_writer_id,
            TaskKind::TestRunner,
            "Deterministic Test Run",
        ) else {
            messages.push(format!(
                "System: Task #{} could not queue deterministic test runner; test branch blocked.",
                top_task_id
            ));
            return false;
        };
        self.queue.push_back(WorkerJob {
            top_task_id,
            kind: WorkerJobKind::TestRunner {
                test_writer_id,
                test_runner_id,
                pass,
            },
        });
        messages.push(format!(
            "System: Task #{} test-writer pass {} complete; deterministic test run queued.",
            top_task_id, pass
        ));
        true
    }

    fn branch_has_active_or_queued(&self, top_id: u64, branch: TaskKind) -> bool {
        let in_branch = |kind: &WorkerJobKind| match branch {
            TaskKind::Implementor => {
                matches!(
                    kind,
                    WorkerJobKind::Implementor { .. }
                        | WorkerJobKind::ImplementorTestRunner { .. }
                        | WorkerJobKind::Auditor { .. }
                )
            }
            TaskKind::TestWriter => {
                matches!(
                    kind,
                    WorkerJobKind::TestWriter { .. }
                        | WorkerJobKind::TestRunner { .. }
                        | WorkerJobKind::TestWriterAuditor { .. }
                )
            }
            _ => false,
        };

        if self
            .active
            .as_ref()
            .is_some_and(|active| active.job.top_task_id == top_id && in_branch(&active.job.kind))
        {
            return true;
        }

        self.queue
            .iter()
            .any(|job| job.top_task_id == top_id && in_branch(&job.kind))
    }

    fn mark_job_started(&mut self, job: &WorkerJob) {
        self.set_status(job.top_task_id, TaskStatus::InProgress);
        match &job.kind {
            WorkerJobKind::Implementor { implementor_id, .. } => {
                self.set_status(*implementor_id, TaskStatus::InProgress)
            }
            WorkerJobKind::Auditor { auditor_id, .. } => {
                self.set_status(*auditor_id, TaskStatus::InProgress)
            }
            WorkerJobKind::TestWriterAuditor { auditor_id, .. } => {
                self.set_status(*auditor_id, TaskStatus::InProgress)
            }
            WorkerJobKind::TestWriter { test_writer_id, .. } => {
                self.set_status(*test_writer_id, TaskStatus::InProgress)
            }
            WorkerJobKind::TestRunner { test_runner_id, .. } => {
                self.set_status(*test_runner_id, TaskStatus::InProgress)
            }
            WorkerJobKind::ImplementorTestRunner { test_runner_id, .. } => {
                self.set_status(*test_runner_id, TaskStatus::InProgress)
            }
            WorkerJobKind::FinalAudit { final_audit_id, .. } => {
                self.set_status(*final_audit_id, TaskStatus::InProgress)
            }
        }
    }

    fn set_status(&mut self, node_id: u64, status: TaskStatus) {
        if let Some(node) = find_node_mut(&mut self.tasks, node_id) {
            node.status = status;
        }
    }

    fn status_of(&self, node_id: u64) -> Option<TaskStatus> {
        find_node(&self.tasks, node_id).map(|node| node.status)
    }

    fn task_title(&self, top_task_id: u64) -> String {
        find_node(&self.tasks, top_task_id)
            .map(|node| node.title.clone())
            .unwrap_or_else(|| format!("Task #{top_task_id}"))
    }

    fn node_title(&self, node_id: u64, fallback: &str) -> String {
        find_node(&self.tasks, node_id)
            .map(|node| node.title.clone())
            .filter(|title| !title.trim().is_empty())
            .unwrap_or_else(|| fallback.to_string())
    }

    fn node_details(&self, node_id: u64) -> String {
        find_node(&self.tasks, node_id)
            .map(|node| node.details.trim().to_string())
            .filter(|details| !details.is_empty())
            .unwrap_or_else(|| "(no details provided)".to_string())
    }

    fn try_mark_top_done(&mut self, top_task_id: u64, messages: &mut Vec<String>) {
        let (impl_done, test_done, already_done) = {
            let Some(top) = find_node(&self.tasks, top_task_id) else {
                return;
            };
            let impl_done = top
                .children
                .iter()
                .filter(|child| child.kind == TaskKind::Implementor)
                .all(|node| Self::subtree_done(node));
            let requires_test_writer = top.children.iter().any(|c| c.kind == TaskKind::TestWriter);
            let test_done = if requires_test_writer {
                top.children
                    .iter()
                    .filter(|child| child.kind == TaskKind::TestWriter)
                    .all(|node| Self::subtree_done(node))
            } else {
                true
            };
            (impl_done, test_done, top.status == TaskStatus::Done)
        };

        if impl_done && test_done && !already_done {
            self.set_status(top_task_id, TaskStatus::Done);
            self.push_context(format!(
                "Task \"{}\" is complete after implementation, audit, test writing, and deterministic test runs all finished successfully.",
                self.task_title(top_task_id)
            ));
            messages.push(format!(
                "System: Task #{} completed after implementation and testing branches converged.",
                top_task_id
            ));
        }
    }

    fn subtree_done(node: &TaskNode) -> bool {
        if node.status != TaskStatus::Done {
            return false;
        }
        node.children.iter().all(Self::subtree_done)
    }

    fn push_context(&mut self, entry: String) {
        if self.rolling_context.len() >= self.max_context_entries {
            self.rolling_context.pop_front();
        }
        self.rolling_context.push_back(entry);
    }

    fn context_block(&self) -> String {
        if self.rolling_context.is_empty() {
            return "No prior rolling task context.".to_string();
        }
        self.rolling_context
            .iter()
            .enumerate()
            .map(|(idx, entry)| format!("{}. {}", idx + 1, entry))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn task_tree_compact(&self) -> String {
        if self.tasks.is_empty() {
            return "(no tasks)".to_string();
        }
        let mut lines = Vec::new();
        for task in self.ordered_root_nodes() {
            render_tree(task, 0, &mut lines);
        }
        lines.join("\n")
    }

    fn ordered_root_nodes(&self) -> Vec<&TaskNode> {
        let mut ordered = Vec::with_capacity(self.tasks.len());
        ordered.extend(
            self.tasks
                .iter()
                .filter(|node| node.kind != TaskKind::FinalAudit),
        );
        ordered.extend(
            self.tasks
                .iter()
                .filter(|node| node.kind == TaskKind::FinalAudit),
        );
        ordered
    }

    fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }
}

fn find_node(nodes: &[TaskNode], id: u64) -> Option<&TaskNode> {
    for node in nodes {
        if node.id == id {
            return Some(node);
        }
        if let Some(found) = find_node(&node.children, id) {
            return Some(found);
        }
    }
    None
}

fn find_node_mut(nodes: &mut [TaskNode], id: u64) -> Option<&mut TaskNode> {
    for node in nodes {
        if node.id == id {
            return Some(node);
        }
        if let Some(found) = find_node_mut(&mut node.children, id) {
            return Some(found);
        }
    }
    None
}

fn render_tree(node: &TaskNode, depth: usize, lines: &mut Vec<String>) {
    let indent = "  ".repeat(depth);
    let kind = match node.kind {
        TaskKind::Top => "Task",
        TaskKind::FinalAudit => "FinalAudit",
        TaskKind::Implementor => "Impl",
        TaskKind::Auditor => "Audit",
        TaskKind::TestWriter => "Tests",
        TaskKind::TestRunner => "TestRun",
    };
    lines.push(format!(
        "{indent}- {} {}: {}",
        status_label(node.status),
        kind,
        node.title
    ));
    for child in &node.children {
        render_tree(child, depth + 1, lines);
    }
}

fn render_subtree_box(
    node: &TaskNode,
    width: usize,
    out: &mut Vec<String>,
    toggles: &mut Vec<RightPaneToggleLine>,
    expanded_detail_keys: &HashSet<String>,
    left_indent: usize,
) {
    let width = width.max(8);
    out.push(format!(
        "{}┌{}┐",
        " ".repeat(left_indent),
        "─".repeat(width.saturating_sub(2))
    ));

    let key = task_detail_key(node);
    let collapsed = !expanded_detail_keys.contains(&key);
    let header = format!(
        "{} {}: {}",
        status_label(node.status),
        kind_label(node.kind),
        node.title
    );
    let inner_text_width = width.saturating_sub(4).max(1);
    let header_segments = wrap_words(&header, inner_text_width);
    for segment in header_segments {
        out.push(format!(
            "{}│ {} │",
            " ".repeat(left_indent),
            pad_segment(&segment, inner_text_width)
        ));
    }
    let detail_start = out.len();
    for detail_line in render_detail_lines(&node.details, inner_text_width, collapsed, 0, true) {
        out.push(format!(
            "{}│ {} │",
            " ".repeat(left_indent),
            pad_segment(&detail_line, inner_text_width)
        ));
    }
    toggles.push(RightPaneToggleLine {
        line_index: detail_start,
        task_key: key.clone(),
    });

    if node.kind != TaskKind::TestRunner && !node.docs.is_empty() {
        let docs_key = docs_toggle_key(&key);
        let docs_collapsed = !expanded_detail_keys.contains(&docs_key);
        let docs_line_index = out.len();
        out.push(format!(
            "{}│ {} │",
            " ".repeat(left_indent),
            pad_segment(
                &format!(
                    "[documentation attached] {}",
                    detail_toggle_label(docs_collapsed)
                ),
                inner_text_width
            )
        ));
        toggles.push(RightPaneToggleLine {
            line_index: docs_line_index,
            task_key: docs_key,
        });
        if !docs_collapsed {
            for docs_line in render_docs_lines(&node.docs, inner_text_width, 0) {
                out.push(format!(
                    "{}│ {} │",
                    " ".repeat(left_indent),
                    pad_segment(&docs_line, inner_text_width)
                ));
            }
        }
    }

    if !node.children.is_empty() {
        out.push(format!(
            "{}├{}┤",
            " ".repeat(left_indent),
            "─".repeat(width.saturating_sub(2))
        ));
        let child_width = width.saturating_sub(4).max(4);
        for (idx, child) in node.children.iter().enumerate() {
            let mut child_lines = Vec::new();
            let mut child_toggles = Vec::new();
            render_subtree_box(
                child,
                child_width,
                &mut child_lines,
                &mut child_toggles,
                expanded_detail_keys,
                0,
            );
            let line_offset = out.len();
            for line in child_lines {
                out.push(format!(
                    "{}│ {} │",
                    " ".repeat(left_indent),
                    pad_segment(&line, child_width)
                ));
            }
            for toggle in child_toggles {
                toggles.push(RightPaneToggleLine {
                    line_index: line_offset + toggle.line_index,
                    task_key: toggle.task_key,
                });
            }
            if idx + 1 < node.children.len() {
                out.push(format!(
                    "{}│ {} │",
                    " ".repeat(left_indent),
                    "─".repeat(child_width)
                ));
            }
        }
    }

    out.push(format!(
        "{}└{}┘",
        " ".repeat(left_indent),
        "─".repeat(width.saturating_sub(2))
    ));
}

fn kind_label(kind: TaskKind) -> &'static str {
    match kind {
        TaskKind::Top => "Task",
        TaskKind::FinalAudit => "FinalAudit",
        TaskKind::Implementor => "Impl",
        TaskKind::Auditor => "Audit",
        TaskKind::TestWriter => "Tests",
        TaskKind::TestRunner => "TestRun",
    }
}

fn validate_required_subtask_structure(nodes: &[TaskNode]) -> Result<(), String> {
    fn node_label(node: &TaskNode) -> String {
        node.external_id
            .clone()
            .unwrap_or_else(|| format!("internal:{}", node.id))
    }

    fn walk(node: &TaskNode, parent_kind: Option<TaskKind>) -> Result<(), String> {
        if node.kind == TaskKind::FinalAudit && parent_kind.is_some() {
            return Err(format!(
                "Final-audit task \"{}\" must be a top-level task (parent_id must be null)",
                node_label(node)
            ));
        }

        if node.kind == TaskKind::Implementor && parent_kind != Some(TaskKind::Top) {
            return Err(format!(
                "Implementor task \"{}\" must be a direct child of a top-level task",
                node_label(node)
            ));
        }

        if node.kind == TaskKind::Implementor {
            let audit_positions = node
                .children
                .iter()
                .enumerate()
                .filter(|(_, child)| child.kind == TaskKind::Auditor)
                .map(|(idx, _)| idx)
                .collect::<Vec<_>>();
            if audit_positions.is_empty() {
                return Err(format!(
                    "Implementor task \"{}\" must include at least one auditor subtask",
                    node_label(node)
                ));
            }
            let last_audit_idx = *audit_positions.last().unwrap_or(&0);
            for (idx, child) in node.children.iter().enumerate() {
                if child.kind == TaskKind::TestRunner && idx <= last_audit_idx {
                    return Err(format!(
                        "Implementor task \"{}\" has test_runner before audit; test_runner must come after audit",
                        node_label(node)
                    ));
                }
            }
            let test_runner_count = node
                .children
                .iter()
                .filter(|child| child.kind == TaskKind::TestRunner)
                .count();
            if test_runner_count > 1 {
                return Err(format!(
                    "Implementor task \"{}\" must include at most one test_runner subtask",
                    node_label(node)
                ));
            }
        }

        if node.kind == TaskKind::TestWriter && parent_kind != Some(TaskKind::Top) {
            return Err(format!(
                "Test-writer task \"{}\" must be a direct child of a top-level task (no nested test_writer groups)",
                node_label(node)
            ));
        }

        if node.kind == TaskKind::Auditor
            && parent_kind != Some(TaskKind::Implementor)
            && parent_kind != Some(TaskKind::TestWriter)
        {
            return Err(format!(
                "Auditor task \"{}\" must be a child of implementor or test_writer",
                node_label(node)
            ));
        }

        if node.kind == TaskKind::TestRunner
            && parent_kind != Some(TaskKind::Implementor)
            && parent_kind != Some(TaskKind::TestWriter)
        {
            return Err(format!(
                "Test-runner task \"{}\" must be a child of implementor or test_writer",
                node_label(node)
            ));
        }

        if node.kind == TaskKind::TestWriter
            && !node
                .children
                .iter()
                .any(|child| child.kind == TaskKind::TestRunner)
        {
            return Err(format!(
                "Test-writer task \"{}\" must include at least one test_runner subtask",
                node_label(node)
            ));
        }
        if node.kind == TaskKind::TestWriter {
            let test_runner_count = node
                .children
                .iter()
                .filter(|child| child.kind == TaskKind::TestRunner)
                .count();
            if test_runner_count > 1 {
                return Err(format!(
                    "Test-writer task \"{}\" must include at most one test_runner subtask",
                    node_label(node)
                ));
            }
        }

        for child in &node.children {
            walk(child, Some(node.kind))?;
        }
        Ok(())
    }

    for node in nodes {
        walk(node, None)?;
    }
    Ok(())
}

fn wrap_words(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out = Vec::new();
    let mut current = String::new();

    for word in text.split_whitespace() {
        if current.is_empty() {
            if word.chars().count() <= width {
                current.push_str(word);
            } else {
                for chunk in split_word(word, width) {
                    out.push(chunk);
                }
            }
            continue;
        }

        let candidate_len = current.chars().count() + 1 + word.chars().count();
        if candidate_len <= width {
            current.push(' ');
            current.push_str(word);
        } else {
            out.push(current);
            current = String::new();
            if word.chars().count() <= width {
                current.push_str(word);
            } else {
                for chunk in split_word(word, width) {
                    out.push(chunk);
                }
            }
        }
    }

    if !current.is_empty() {
        out.push(current);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

fn split_word(word: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for ch in word.chars() {
        current.push(ch);
        if current.chars().count() >= width {
            out.push(current.clone());
            current.clear();
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

fn render_detail_lines(
    details: &str,
    width: usize,
    collapsed: bool,
    base_indent: usize,
    show_toggle: bool,
) -> Vec<String> {
    let width = width.max(1);
    let mut out = Vec::new();
    let clean = details.trim();
    let content = if clean.is_empty() {
        "No details provided."
    } else {
        clean
    };
    let prefix = " ".repeat(base_indent);
    let first_prefix = if show_toggle {
        format!("{prefix}details {}: ", detail_toggle_label(collapsed))
    } else {
        format!("{prefix}details: ")
    };
    let continued_prefix = format!("{prefix}         ");
    let content_width = width.saturating_sub(first_prefix.chars().count()).max(1);

    if collapsed {
        let mut collapsed_text = wrap_words(content, content_width)
            .into_iter()
            .next()
            .unwrap_or_default();
        if !collapsed_text.is_empty() {
            collapsed_text.push_str("...");
        } else {
            collapsed_text = "...".to_string();
        }
        out.push(format!("{first_prefix}{collapsed_text}"));
        return out;
    }

    let wrapped = wrap_words(content, content_width);
    if let Some((first, rest)) = wrapped.split_first() {
        out.push(format!("{first_prefix}{first}"));
        for segment in rest {
            out.push(format!("{continued_prefix}{segment}"));
        }
    } else {
        out.push(format!("{first_prefix}"));
    }
    out
}

fn detail_toggle_label(collapsed: bool) -> &'static str {
    if collapsed { "[+]" } else { "[-]" }
}

fn docs_toggle_key(task_key: &str) -> String {
    format!("docs:{task_key}")
}

fn task_detail_key(node: &TaskNode) -> String {
    node.external_id
        .clone()
        .unwrap_or_else(|| format!("internal:{}", node.id))
}

fn render_docs_lines(
    docs: &[PlannerTaskDocFileEntry],
    width: usize,
    base_indent: usize,
) -> Vec<String> {
    let mut out = Vec::new();
    for doc in docs {
        out.extend(render_doc_field_lines(
            "title",
            &doc.title,
            width,
            base_indent,
            true,
        ));
        out.extend(render_doc_field_lines(
            "url",
            &doc.url,
            width,
            base_indent,
            false,
        ));
        if !doc.summary.trim().is_empty() {
            out.extend(render_doc_field_lines(
                "summary",
                &doc.summary,
                width,
                base_indent,
                false,
            ));
        }
    }
    out
}

fn render_doc_field_lines(
    label: &str,
    value: &str,
    width: usize,
    base_indent: usize,
    bullet: bool,
) -> Vec<String> {
    let indent = " ".repeat(base_indent);
    let first_prefix = if bullet {
        format!("{indent}- {label}: ")
    } else {
        format!("{indent}  {label}: ")
    };
    let continued_prefix = " ".repeat(first_prefix.chars().count());
    let content = value.trim();
    let content = if content.is_empty() {
        "(none)"
    } else {
        content
    };
    let content_width = width.saturating_sub(first_prefix.chars().count()).max(1);
    let wrapped = wrap_words(content, content_width);
    let mut out = Vec::new();
    if let Some((first, rest)) = wrapped.split_first() {
        out.push(format!("{first_prefix}{first}"));
        for segment in rest {
            out.push(format!("{continued_prefix}{segment}"));
        }
    } else {
        out.push(first_prefix);
    }
    out
}

fn pad_segment(segment: &str, width: usize) -> String {
    let used = segment.chars().count();
    if used >= width {
        segment.chars().take(width).collect()
    } else {
        format!("{segment}{}", " ".repeat(width - used))
    }
}

fn status_label(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Pending => "[ ]",
        TaskStatus::InProgress => "[~]",
        TaskStatus::NeedsChanges => "[!]",
        TaskStatus::Done => "[x]",
    }
}

fn task_kind_from_file(kind: PlannerTaskKindFile) -> TaskKind {
    match kind {
        PlannerTaskKindFile::Task => TaskKind::Top,
        PlannerTaskKindFile::FinalAudit => TaskKind::FinalAudit,
        PlannerTaskKindFile::Implementor => TaskKind::Implementor,
        PlannerTaskKindFile::Auditor => TaskKind::Auditor,
        PlannerTaskKindFile::TestWriter => TaskKind::TestWriter,
        PlannerTaskKindFile::TestRunner => TaskKind::TestRunner,
    }
}

fn task_kind_to_file(kind: TaskKind) -> PlannerTaskKindFile {
    match kind {
        TaskKind::Top => PlannerTaskKindFile::Task,
        TaskKind::FinalAudit => PlannerTaskKindFile::FinalAudit,
        TaskKind::Implementor => PlannerTaskKindFile::Implementor,
        TaskKind::Auditor => PlannerTaskKindFile::Auditor,
        TaskKind::TestWriter => PlannerTaskKindFile::TestWriter,
        TaskKind::TestRunner => PlannerTaskKindFile::TestRunner,
    }
}

fn task_status_to_file(status: TaskStatus) -> PlannerTaskStatusFile {
    match status {
        TaskStatus::Pending => PlannerTaskStatusFile::Pending,
        TaskStatus::InProgress => PlannerTaskStatusFile::InProgress,
        TaskStatus::NeedsChanges => PlannerTaskStatusFile::NeedsChanges,
        TaskStatus::Done => PlannerTaskStatusFile::Done,
    }
}

fn make_context_summary(
    role: &str,
    task_title: &str,
    transcript: &[String],
    success: bool,
) -> String {
    let preview = transcript
        .iter()
        .rev()
        .find(|line| !line.trim().is_empty())
        .map(|line| line.trim().to_string())
        .unwrap_or_else(|| "No detailed output was captured.".to_string());
    format!(
        "{role} worked on \"{task_title}\" and {}. Key result: {preview}.",
        if success {
            "finished its pass successfully"
        } else {
            "ended with a failure state"
        }
    )
}

fn extract_changed_files_summary(transcript: &[String]) -> String {
    let merged = transcript.join("\n");
    if let Some(summary) = extract_tagged_block(&merged, FILES_CHANGED_BEGIN, FILES_CHANGED_END) {
        let normalized = summary
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if !normalized.is_empty() {
            return normalized;
        }
    }
    "(no structured changed-files summary found in implementor output)".to_string()
}

fn extract_tagged_block(text: &str, begin_tag: &str, end_tag: &str) -> Option<String> {
    let begin_idx = text.find(begin_tag)?;
    let after_begin = &text[begin_idx + begin_tag.len()..];
    let end_rel = after_begin.find(end_tag)?;
    Some(after_begin[..end_rel].trim().to_string())
}

#[cfg(test)]
fn audit_strictness_policy(pass: u8) -> &'static str {
    match pass {
        1 => {
            "Pass 1 (strict): report all meaningful correctness, safety, reliability, and testability issues."
        }
        2 => {
            "Pass 2 (moderate): prioritize substantial issues and avoid minor nits that do not materially affect behavior."
        }
        3 => "Pass 3 (targeted): focus only on high-impact defects or likely regressions.",
        _ => {
            "Pass 4+ (critical only): only fail for truly critical blockers that would prevent the broader plan from running."
        }
    }
}

fn audit_detects_issues(transcript: &[String]) -> bool {
    if let Some(protocol_result) = parse_audit_result_token(transcript) {
        return !protocol_result;
    }
    let text = transcript.join("\n").to_lowercase();
    if text.contains("no issues found") || text.contains("no findings") {
        return false;
    }
    text.contains("issue")
        || text.contains("bug")
        || text.contains("error")
        || text.contains("fix required")
        || text.contains("needs change")
}

fn parse_audit_result_token(transcript: &[String]) -> Option<bool> {
    for line in transcript {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let upper = trimmed.to_ascii_uppercase();
        if upper == "AUDIT_RESULT: PASS" {
            return Some(true);
        }
        if upper == "AUDIT_RESULT: FAIL" {
            return Some(false);
        }
    }
    None
}

fn default_generated_details(kind: TaskKind) -> &'static str {
    match kind {
        TaskKind::Implementor => "Implement the required code changes for this top-level task.",
        TaskKind::Auditor => {
            "Audit implementation and tests for correctness, regressions, and completeness."
        }
        TaskKind::TestWriter => {
            "Write or update tests that validate the intended behavior and regressions."
        }
        TaskKind::TestRunner => {
            "Run deterministic tests and report pass/fail outcomes for this task branch."
        }
        TaskKind::FinalAudit => {
            "Perform a final cross-task audit after all implementation and testing complete."
        }
        TaskKind::Top => "Top-level task scope and expected outcome.",
    }
}

fn audit_feedback(transcript: &[String], code: i32, success: bool) -> String {
    if !success {
        return format!(
            "Audit process exited with code {code}; re-run implementation and validate."
        );
    }
    let merged = transcript.join(" ");
    if merged.trim().is_empty() {
        "Audit requested fixes without detailed notes; review implementation against requirements."
            .to_string()
    } else {
        format!("Audit feedback: {merged}")
    }
}

fn test_runner_feedback(transcript: &[String], code: i32) -> String {
    let merged = transcript.join("\n");
    if merged.trim().is_empty() {
        return format!("Deterministic test run failed with code {code} and no output.");
    }
    format!("Deterministic test run failed with code {code}. Output:\n{merged}")
}

#[cfg(test)]
#[path = "../tests/unit/workflow_tests.rs"]
mod tests;
