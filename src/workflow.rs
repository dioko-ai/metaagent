use std::collections::{HashSet, VecDeque};

use crate::session_store::{
    PlannerTaskDocFileEntry, PlannerTaskFileEntry, PlannerTaskKindFile, PlannerTaskStatusFile,
};

const FILES_CHANGED_BEGIN: &str = "FILES_CHANGED_BEGIN";
const FILES_CHANGED_END: &str = "FILES_CHANGED_END";
const MAX_AUDIT_RETRIES: u8 = 4;
const MAX_TEST_RETRIES: u8 = 5;

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
    }

    pub fn sync_planner_tasks_from_file(
        &mut self,
        entries: Vec<PlannerTaskFileEntry>,
    ) -> Result<usize, String> {
        if self.execution_enabled {
            return Err("Cannot reload planner tasks while execution is enabled".to_string());
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
                self.push_context(make_context_summary(
                    "Implementor",
                    &self.task_title(job.top_task_id),
                    &transcript,
                    success,
                ));

                if success {
                    // Mark implementation pass complete before moving into audit. If an audit fails,
                    // status is set back to NeedsChanges and implementor retries.
                    self.set_status(implementor_id, TaskStatus::Done);
                    if let Some(auditor_id) = resume_auditor_id {
                        self.queue.push_back(WorkerJob {
                            top_task_id: job.top_task_id,
                            kind: WorkerJobKind::Auditor {
                                implementor_id,
                                auditor_id,
                                pass: resume_audit_pass.unwrap_or(1),
                                implementation_report: Some(transcript.join("\n")),
                                changed_files_summary: Some(extract_changed_files_summary(
                                    &transcript,
                                )),
                            },
                        });
                        messages.push(format!(
                            "System: Task #{} implementation pass {} complete; resumed audit queued.",
                            job.top_task_id, pass
                        ));
                    } else {
                        if self
                            .find_child_kind(implementor_id, TaskKind::Auditor)
                            .is_none()
                        {
                            let _ = self.find_or_create_child_kind(
                                implementor_id,
                                TaskKind::Auditor,
                                "Audit",
                            );
                        }
                        let _ = self.queue_next_implementor_audit(
                            job.top_task_id,
                            implementor_id,
                            pass,
                            Some(transcript.join("\n")),
                            Some(extract_changed_files_summary(&transcript)),
                            &mut messages,
                        );
                    }
                } else {
                    self.set_status(implementor_id, TaskStatus::NeedsChanges);
                    self.queue.push_back(WorkerJob {
                        top_task_id: job.top_task_id,
                        kind: WorkerJobKind::Implementor {
                            implementor_id,
                            pass: pass.saturating_add(1),
                            feedback: Some(format!(
                                "Previous implementor run failed with code {code}."
                            )),
                            resume_auditor_id: None,
                            resume_audit_pass: None,
                        },
                    });
                    messages.push(format!(
                        "System: Task #{} implementation failed (code {}); retry queued.",
                        job.top_task_id, code
                    ));
                }
            }
            WorkerJobKind::Auditor {
                implementor_id,
                auditor_id,
                pass,
                implementation_report,
                changed_files_summary,
            } => {
                self.push_context(make_context_summary(
                    "Auditor",
                    &self.task_title(job.top_task_id),
                    &transcript,
                    success,
                ));

                let issues = !success || audit_detects_issues(&transcript);
                if issues {
                    self.set_status(implementor_id, TaskStatus::NeedsChanges);
                    if pass >= MAX_AUDIT_RETRIES {
                        self.set_status(auditor_id, TaskStatus::Done);
                        self.recent_failures.push(WorkflowFailure {
                            kind: WorkflowFailureKind::Audit,
                            top_task_id: job.top_task_id,
                            top_task_title: self.task_title(job.top_task_id),
                            attempts: pass,
                            reason: audit_feedback(&transcript, code, success),
                            action_taken:
                                "Audit retries exhausted; continued execution to next audit/step."
                                    .to_string(),
                        });
                        let _ = self.queue_next_implementor_audit(
                            job.top_task_id,
                            implementor_id,
                            1,
                            implementation_report,
                            changed_files_summary,
                            &mut messages,
                        );
                        messages.push(format!(
                            "System: Task #{} audit still found critical blockers at pass {}. Max retries ({}) reached; proceeding to next audit/step.",
                            job.top_task_id, pass, MAX_AUDIT_RETRIES
                        ));
                    } else {
                        self.set_status(auditor_id, TaskStatus::NeedsChanges);
                        self.queue.push_back(WorkerJob {
                            top_task_id: job.top_task_id,
                            kind: WorkerJobKind::Implementor {
                                implementor_id,
                                pass: pass.saturating_add(1),
                                feedback: Some(audit_feedback(&transcript, code, success)),
                                resume_auditor_id: Some(auditor_id),
                                resume_audit_pass: Some(pass.saturating_add(1)),
                            },
                        });
                        messages.push(format!(
                            "System: Task #{} audit requested fixes; implementor pass {} queued.",
                            job.top_task_id,
                            pass.saturating_add(1)
                        ));
                    }
                } else {
                    self.set_status(auditor_id, TaskStatus::Done);
                    let _ = self.queue_next_implementor_audit(
                        job.top_task_id,
                        implementor_id,
                        1,
                        implementation_report,
                        changed_files_summary,
                        &mut messages,
                    );
                }
            }
            WorkerJobKind::TestWriter {
                test_writer_id,
                pass,
                skip_test_runner_on_success,
                resume_auditor_id,
                resume_audit_pass,
                ..
            } => {
                self.push_context(make_context_summary(
                    "TestWriter",
                    &self.task_title(job.top_task_id),
                    &transcript,
                    success,
                ));

                if success {
                    if skip_test_runner_on_success {
                        self.set_status(test_writer_id, TaskStatus::Done);
                        messages.push(format!(
                            "System: Task #{} removed failing tests after retries and proceeded.",
                            job.top_task_id
                        ));
                        self.try_mark_top_done(job.top_task_id, &mut messages);
                        if self.execution_enabled {
                            let _ = self.enqueue_ready_top_tasks();
                        }
                        return messages;
                    }
                    if let Some(auditor_id) = resume_auditor_id {
                        self.queue.push_back(WorkerJob {
                            top_task_id: job.top_task_id,
                            kind: WorkerJobKind::TestWriterAuditor {
                                test_writer_id,
                                auditor_id,
                                pass: resume_audit_pass.unwrap_or(1),
                                test_report: Some(transcript.join("\n")),
                            },
                        });
                        messages.push(format!(
                            "System: Task #{} test-writer pass {} complete; resumed audit queued.",
                            job.top_task_id, pass
                        ));
                    } else {
                        let _ = self.queue_test_writer_next_step(
                            job.top_task_id,
                            test_writer_id,
                            pass,
                            true,
                            Some(transcript.join("\n")),
                            &mut messages,
                        );
                    }
                } else {
                    self.set_status(test_writer_id, TaskStatus::NeedsChanges);
                    if pass >= MAX_TEST_RETRIES {
                        self.set_status(test_writer_id, TaskStatus::Done);
                        self.recent_failures.push(WorkflowFailure {
                            kind: WorkflowFailureKind::Test,
                            top_task_id: job.top_task_id,
                            top_task_title: self.task_title(job.top_task_id),
                            attempts: pass,
                            reason: format!(
                                "Test-writer failed repeatedly; latest exit code {code}."
                            ),
                            action_taken:
                                "Test-writer retries exhausted; proceeded without adding tests."
                                    .to_string(),
                        });
                        messages.push(format!(
                            "System: Task #{} test-writer still failing at pass {}. Max retries ({}) reached; proceeding to next step.",
                            job.top_task_id, pass, MAX_TEST_RETRIES
                        ));
                        self.try_mark_top_done(job.top_task_id, &mut messages);
                    } else {
                        self.queue.push_back(WorkerJob {
                            top_task_id: job.top_task_id,
                            kind: WorkerJobKind::TestWriter {
                                test_writer_id,
                                pass: pass.saturating_add(1),
                                feedback: Some(format!(
                                    "Previous test-writer run failed with code {code}."
                                )),
                                skip_test_runner_on_success: false,
                                resume_auditor_id: None,
                                resume_audit_pass: None,
                            },
                        });
                        messages.push(format!(
                            "System: Task #{} test-writer failed (code {}); retry queued.",
                            job.top_task_id, code
                        ));
                    }
                }
            }
            WorkerJobKind::TestWriterAuditor {
                test_writer_id,
                auditor_id,
                pass,
                test_report,
            } => {
                self.push_context(make_context_summary(
                    "Auditor",
                    &self.task_title(job.top_task_id),
                    &transcript,
                    success,
                ));

                let issues = !success || audit_detects_issues(&transcript);
                if issues {
                    self.set_status(test_writer_id, TaskStatus::NeedsChanges);
                    if pass >= MAX_AUDIT_RETRIES {
                        self.set_status(auditor_id, TaskStatus::Done);
                        self.recent_failures.push(WorkflowFailure {
                            kind: WorkflowFailureKind::Audit,
                            top_task_id: job.top_task_id,
                            top_task_title: self.task_title(job.top_task_id),
                            attempts: pass,
                            reason: audit_feedback(&transcript, code, success),
                            action_taken:
                                "Test-writer audit retries exhausted; continued to deterministic test run."
                                    .to_string(),
                        });
                        let _ = self.queue_test_writer_next_step(
                            job.top_task_id,
                            test_writer_id,
                            pass,
                            true,
                            test_report,
                            &mut messages,
                        );
                        messages.push(format!(
                            "System: Task #{} test-writer audit still found critical blockers at pass {}. Max retries ({}) reached; proceeding to deterministic tests.",
                            job.top_task_id, pass, MAX_AUDIT_RETRIES
                        ));
                    } else {
                        self.set_status(auditor_id, TaskStatus::NeedsChanges);
                        self.queue.push_back(WorkerJob {
                            top_task_id: job.top_task_id,
                            kind: WorkerJobKind::TestWriter {
                                test_writer_id,
                                pass: pass.saturating_add(1),
                                feedback: Some(audit_feedback(&transcript, code, success)),
                                skip_test_runner_on_success: false,
                                resume_auditor_id: Some(auditor_id),
                                resume_audit_pass: Some(pass.saturating_add(1)),
                            },
                        });
                        messages.push(format!(
                            "System: Task #{} test-writer audit requested fixes; test-writer pass {} queued.",
                            job.top_task_id,
                            pass.saturating_add(1)
                        ));
                    }
                } else {
                    self.set_status(auditor_id, TaskStatus::Done);
                    let _ = self.queue_test_writer_next_step(
                        job.top_task_id,
                        test_writer_id,
                        pass,
                        true,
                        test_report,
                        &mut messages,
                    );
                    messages.push(format!(
                        "System: Task #{} test-writer audit pass {} complete.",
                        job.top_task_id, pass
                    ));
                }
            }
            WorkerJobKind::TestRunner {
                test_writer_id,
                test_runner_id,
                pass,
            } => {
                self.set_status(test_runner_id, TaskStatus::Done);
                self.push_context(make_context_summary(
                    "TestRunner",
                    &self.task_title(job.top_task_id),
                    &transcript,
                    success,
                ));

                if success {
                    self.set_status(test_writer_id, TaskStatus::Done);
                    messages.push(format!(
                        "System: Task #{} deterministic tests passed on run {}.",
                        job.top_task_id, pass
                    ));
                    self.try_mark_top_done(job.top_task_id, &mut messages);
                } else {
                    self.set_status(test_writer_id, TaskStatus::NeedsChanges);
                    if pass >= MAX_TEST_RETRIES {
                        let failure_reason = test_runner_feedback(&transcript, code);
                        self.recent_failures.push(WorkflowFailure {
                            kind: WorkflowFailureKind::Test,
                            top_task_id: job.top_task_id,
                            top_task_title: self.task_title(job.top_task_id),
                            attempts: pass,
                            reason: failure_reason.clone(),
                            action_taken:
                                "Requested test cleanup (remove failing tests) and continued."
                                    .to_string(),
                        });
                        self.queue.push_back(WorkerJob {
                            top_task_id: job.top_task_id,
                            kind: WorkerJobKind::TestWriter {
                                test_writer_id,
                                pass: pass.saturating_add(1),
                                feedback: Some(format!(
                                    "Deterministic test retries exhausted.\n\
                                     Remove the failing tests completely so they no longer fail.\n\
                                     Do not add replacement tests in this pass.\n\
                                     Then report exactly which tests/files were removed.\n\
                                     Failure details:\n{}",
                                    failure_reason
                                )),
                                skip_test_runner_on_success: true,
                                resume_auditor_id: None,
                                resume_audit_pass: None,
                            },
                        });
                        messages.push(format!(
                            "System: Task #{} tests still failing at pass {}. Max retries ({}) reached; queued cleanup removal pass.",
                            job.top_task_id, pass, MAX_TEST_RETRIES
                        ));
                    } else {
                        self.queue.push_back(WorkerJob {
                            top_task_id: job.top_task_id,
                            kind: WorkerJobKind::TestWriter {
                                test_writer_id,
                                pass: pass.saturating_add(1),
                                feedback: Some(test_runner_feedback(&transcript, code)),
                                skip_test_runner_on_success: false,
                                resume_auditor_id: None,
                                resume_audit_pass: None,
                            },
                        });
                        messages.push(format!(
                            "System: Task #{} tests failed; test-writer pass {} queued.",
                            job.top_task_id,
                            pass.saturating_add(1)
                        ));
                    }
                }
            }
            WorkerJobKind::ImplementorTestRunner {
                implementor_id,
                test_runner_id,
                pass,
            } => {
                self.push_context(make_context_summary(
                    "TestRunner",
                    &self.task_title(job.top_task_id),
                    &transcript,
                    success,
                ));

                if success {
                    self.set_status(test_runner_id, TaskStatus::Done);
                    self.set_status(implementor_id, TaskStatus::Done);
                    self.try_mark_top_done(job.top_task_id, &mut messages);
                    messages.push(format!(
                        "System: Task #{} existing-test runner passed on run {}; implementor branch complete.",
                        job.top_task_id, pass
                    ));
                } else {
                    self.set_status(test_runner_id, TaskStatus::NeedsChanges);
                    if pass >= MAX_TEST_RETRIES {
                        self.set_status(test_runner_id, TaskStatus::Done);
                        self.recent_failures.push(WorkflowFailure {
                            kind: WorkflowFailureKind::Test,
                            top_task_id: job.top_task_id,
                            top_task_title: self.task_title(job.top_task_id),
                            attempts: pass,
                            reason: test_runner_feedback(&transcript, code),
                            action_taken:
                                "Existing-tests runner retries exhausted; continued to next step."
                                    .to_string(),
                        });
                        self.set_status(implementor_id, TaskStatus::Done);
                        self.try_mark_top_done(job.top_task_id, &mut messages);
                        messages.push(format!(
                            "System: Task #{} existing tests still failing at pass {}. Max retries ({}) reached; proceeding to next step.",
                            job.top_task_id, pass, MAX_TEST_RETRIES
                        ));
                    } else {
                        self.set_status(implementor_id, TaskStatus::NeedsChanges);
                        self.queue.push_back(WorkerJob {
                            top_task_id: job.top_task_id,
                            kind: WorkerJobKind::Implementor {
                                implementor_id,
                                pass: pass.saturating_add(1),
                                feedback: Some(test_runner_feedback(&transcript, code)),
                                resume_auditor_id: None,
                                resume_audit_pass: None,
                            },
                        });
                        messages.push(format!(
                            "System: Task #{} existing tests failed; implementor pass {} queued.",
                            job.top_task_id,
                            pass.saturating_add(1)
                        ));
                    }
                }
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
                if success {
                    self.set_status(final_audit_id, TaskStatus::Done);
                    messages.push(format!(
                        "System: Final audit task #{} completed on pass {}.",
                        job.top_task_id, pass
                    ));
                } else {
                    self.set_status(final_audit_id, TaskStatus::NeedsChanges);
                    self.queue.push_back(WorkerJob {
                        top_task_id: job.top_task_id,
                        kind: WorkerJobKind::FinalAudit {
                            final_audit_id,
                            pass: pass.saturating_add(1),
                            feedback: Some(format!(
                                "Previous final audit run failed with code {code}."
                            )),
                        },
                    });
                    messages.push(format!(
                        "System: Final audit task #{} failed (code {}); retry queued.",
                        job.top_task_id, code
                    ));
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
                let prompt = format!(
                    "You are an implementation sub-agent.\n\
                 Top-level task: {}\n\
                 Implementation subtask: {}\n\
                 Implementation details:\n{}\n\
                 Rolling task context:\n{}\n\
                 {}\n\
                 Guardrail: do not create or modify tests unless this task explicitly includes a direct implementor test_runner flow reporting failing existing tests.\n\
                 End your response with a structured changed-files summary block using this exact format:\n\
                 FILES_CHANGED_BEGIN\n\
                 - path/to/file.ext: brief description of what changed\n\
                 FILES_CHANGED_END\n\
                 Include every file you changed. If no files changed, include a single bullet with reason.\n\
                 Provide concise progress updates and finish with what changed.",
                    self.task_title(job.top_task_id),
                    self.node_title(*implementor_id, "Implementation"),
                    self.node_details(*implementor_id),
                    self.context_block(),
                    feedback
                        .as_ref()
                        .map(|f| format!("Audit feedback to address:\n{f}"))
                        .unwrap_or_else(
                            || "No audit feedback yet; implement from task prompt.".to_string()
                        )
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
                let prompt = format!(
                    "You are an audit sub-agent reviewing implementation output.\n\
                 Top-level task: {}\n\
                 Parent implementor task: {}\n\
                 Parent implementor details:\n{}\n\
                 Audit subtask details:\n{}\n\
                 Audit pass: {} of {}\n\
                 Rolling task context:\n{}\n\
                 Implementor changed-files summary:\n{}\n\
                 Implementation output to audit:\n{}\n\
                 Guardrail: do not audit test quality/coverage or request test changes; limit findings to implementation concerns only.\n\
                 Scope lock (required): audit only the parent implementor task/details above. Do not audit unrelated tasks, broader roadmap items, or unrelated files.\n\
                 Execution guardrail: do not run tests and do not execute/check shell commands. Command/test execution is handled by a subsequent dedicated agent.\n\
                 Strictness policy for this audit pass:\n{}\n\
                 Response protocol (required):\n\
                 - First line must be exactly one of:\n\
                   AUDIT_RESULT: PASS\n\
                   AUDIT_RESULT: FAIL\n\
                 - Then provide concise findings. If PASS, include a brief rationale.\n\
                 - If FAIL, include concrete issues and suggested fixes. On pass 4, only FAIL for truly critical blockers that would prevent the broader plan from running.",
                    self.task_title(job.top_task_id),
                    self.node_title(*implementor_id, "Implementation"),
                    self.node_details(*implementor_id),
                    self.node_details(*auditor_id),
                    pass,
                    MAX_AUDIT_RETRIES,
                    self.context_block(),
                    changed_files_summary
                        .as_deref()
                        .unwrap_or("(implementor did not provide a changed-files summary)"),
                    implementation_report
                        .as_deref()
                        .unwrap_or("(no implementation output captured)"),
                    audit_strictness_policy(*pass),
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
                let prompt = format!(
                    "You are an audit sub-agent reviewing test-writing output.\n\
                 Top-level task: {}\n\
                 Parent test-writer task: {}\n\
                 Parent test-writer details:\n{}\n\
                 Audit subtask details:\n{}\n\
                 Audit pass: {} of {}\n\
                 Rolling task context:\n{}\n\
                 Test-writer output to audit:\n{}\n\
                 Focus on test quality, coverage relevance, and whether tests clearly validate intended behavior.\n\
                 Execution guardrail: do not run tests and do not execute/check shell commands. Command/test execution is handled by a subsequent dedicated agent.\n\
                 Strictness policy for this audit pass:\n{}\n\
                 Response protocol (required):\n\
                 - First line must be exactly one of:\n\
                   AUDIT_RESULT: PASS\n\
                   AUDIT_RESULT: FAIL\n\
                 - Then provide concise findings. If PASS, include a brief rationale.\n\
                 - If FAIL, include concrete issues and suggested fixes. On pass 4, only FAIL for truly critical blockers that would prevent the broader plan from running.",
                    self.task_title(job.top_task_id),
                    self.node_title(*test_writer_id, "Test Writing"),
                    self.node_details(*test_writer_id),
                    self.node_details(*auditor_id),
                    pass,
                    MAX_AUDIT_RETRIES,
                    self.context_block(),
                    test_report
                        .as_deref()
                        .unwrap_or("(no test-writer output captured)"),
                    audit_strictness_policy(*pass),
                );
                JobRun::AgentPrompt(self.prepend_task_docs_to_prompt(*auditor_id, prompt))
            }
            WorkerJobKind::TestWriter {
                test_writer_id,
                feedback,
                skip_test_runner_on_success,
                ..
            } => {
                let prompt = format!(
                    "You are a test-writer sub-agent.\n\
                 Top-level task: {}\n\
                 Test-writer subtask: {}\n\
                 Test-writing details:\n{}\n\
                 Rolling task context:\n{}\n\
                 {}\n\
                 Write or update tests to cover current implementation thoroughly.\n\
                 {}\n\
                 Keep output concise and include what test behavior was added.",
                    self.task_title(job.top_task_id),
                    self.node_title(*test_writer_id, "Test Writing"),
                    self.node_details(*test_writer_id),
                    self.context_block(),
                    feedback
                        .as_deref()
                        .map(|f| format!("Feedback to address before re-running deterministic tests:\n{f}"))
                        .unwrap_or_else(|| "No test feedback yet; infer tests from task and implementation branch progress.".to_string()),
                    if *skip_test_runner_on_success {
                        "Special instruction: this is a cleanup pass after exhausted deterministic test retries. Remove failing tests and do not add replacements."
                    } else {
                        ""
                    }
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
                 If no issues, explicitly say 'No issues found'. Otherwise list concrete issues and fixes.",
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
                details: String::new(),
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
                details: String::new(),
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
        let mut queued = 0usize;
        let next_top_id = self
            .tasks
            .iter()
            .find(|node| node.kind == TaskKind::Top && node.status != TaskStatus::Done)
            .map(|node| node.id);
        let final_audit_ids: Vec<u64> = self
            .tasks
            .iter()
            .filter(|node| node.kind == TaskKind::FinalAudit)
            .map(|node| node.id)
            .collect();

        self.retain_final_audit_jobs_only_when_non_final_done();

        if let Some(top_id) = next_top_id {
            let (has_any_children, existing_implementor_id, existing_test_writer_id) =
                if let Some(top) = find_node(&self.tasks, top_id) {
                    (
                        !top.children.is_empty(),
                        top.children
                            .iter()
                            .find(|child| child.kind == TaskKind::Implementor)
                            .map(|child| child.id),
                        top.children
                            .iter()
                            .find(|child| child.kind == TaskKind::TestWriter)
                            .map(|child| child.id),
                    )
                } else {
                    (false, None, None)
                };

            let implementor_id = if let Some(id) = existing_implementor_id {
                Some(id)
            } else if !has_any_children {
                self.start_kind_for_top(top_id, TaskKind::Implementor, "Implementation")
            } else {
                None
            };

            if let Some(implementor_id) = implementor_id {
                if !self.branch_has_active_or_queued(top_id, TaskKind::Implementor)
                    && self.status_of(top_id) != Some(TaskStatus::NeedsChanges)
                {
                    let impl_status = self.status_of(implementor_id);
                    let has_pending_impl_audit =
                        self.find_next_pending_child_kind(implementor_id, TaskKind::Auditor)
                            .is_some();

                    if impl_status == Some(TaskStatus::Done)
                        || (impl_status == Some(TaskStatus::InProgress) && has_pending_impl_audit)
                    {
                        if impl_status == Some(TaskStatus::InProgress) && has_pending_impl_audit {
                            // Backward-compat: older runs could leave implementor InProgress
                            // while an audit was pending/running.
                            self.set_status(implementor_id, TaskStatus::Done);
                        }
                        let mut resume_messages = Vec::new();
                        if self.queue_next_implementor_audit(
                            top_id,
                            implementor_id,
                            1,
                            None,
                            None,
                            &mut resume_messages,
                        ) {
                            queued = queued.saturating_add(1);
                        }
                    } else {
                        self.queue.push_back(WorkerJob {
                            top_task_id: top_id,
                            kind: WorkerJobKind::Implementor {
                                implementor_id,
                                pass: 1,
                                feedback: None,
                                resume_auditor_id: None,
                                resume_audit_pass: None,
                            },
                        });
                        queued = queued.saturating_add(1);
                    }
                }

                let maybe_test_writer_id = if let Some(id) = existing_test_writer_id {
                    Some(id)
                } else if !has_any_children {
                    self.start_kind_for_top(top_id, TaskKind::TestWriter, "Test Writing")
                } else {
                    None
                };
                if let Some(test_writer_id) = maybe_test_writer_id
                    && !self.branch_has_active_or_queued(top_id, TaskKind::TestWriter)
                    && self.status_of(top_id) != Some(TaskStatus::NeedsChanges)
                {
                    let writer_status = self.status_of(test_writer_id);
                    let has_pending_writer_audit =
                        self.find_next_pending_child_kind(test_writer_id, TaskKind::Auditor)
                            .is_some();
                    let has_pending_writer_runner =
                        self.find_next_pending_child_kind(test_writer_id, TaskKind::TestRunner)
                            .is_some();
                    if writer_status == Some(TaskStatus::Done)
                        && (has_pending_writer_audit || has_pending_writer_runner)
                    {
                        let mut resume_messages = Vec::new();
                        if self.queue_test_writer_next_step(
                            top_id,
                            test_writer_id,
                            1,
                            true,
                            None,
                            &mut resume_messages,
                        ) {
                            queued = queued.saturating_add(1);
                        }
                    } else if writer_status != Some(TaskStatus::Done) {
                        self.queue.push_back(WorkerJob {
                            top_task_id: top_id,
                            kind: WorkerJobKind::TestWriter {
                                test_writer_id,
                                pass: 1,
                                feedback: None,
                                skip_test_runner_on_success: false,
                                resume_auditor_id: None,
                                resume_audit_pass: None,
                            },
                        });
                        queued = queued.saturating_add(1);
                    }
                }
            }
        }

        let non_final_all_done = self
            .tasks
            .iter()
            .filter(|node| node.kind == TaskKind::Top)
            .all(|node| node.status == TaskStatus::Done);

        if !non_final_all_done {
            for final_id in final_audit_ids {
                if self.status_of(final_id) == Some(TaskStatus::Done) {
                    self.set_status(final_id, TaskStatus::Pending);
                }
            }
            return queued;
        }

        for final_id in final_audit_ids {
            if !self.final_audit_has_active_or_queued(final_id)
                && self.status_of(final_id) != Some(TaskStatus::Done)
            {
                self.queue.push_back(WorkerJob {
                    top_task_id: final_id,
                    kind: WorkerJobKind::FinalAudit {
                        final_audit_id: final_id,
                        pass: 1,
                        feedback: None,
                    },
                });
                queued = queued.saturating_add(1);
            }
        }

        queued
    }

    fn retain_final_audit_jobs_only_when_non_final_done(&mut self) {
        let non_final_all_done = self
            .tasks
            .iter()
            .filter(|node| node.kind == TaskKind::Top)
            .all(|node| node.status == TaskStatus::Done);
        if non_final_all_done {
            return;
        }
        self.queue
            .retain(|job| !matches!(job.kind, WorkerJobKind::FinalAudit { .. }));
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
                .find(|child| child.kind == TaskKind::Implementor)
                .is_some_and(|node| node.status == TaskStatus::Done);
            let requires_test_writer = top.children.iter().any(|c| c.kind == TaskKind::TestWriter);
            let test_done = if requires_test_writer {
                top.children
                    .iter()
                    .find(|child| child.kind == TaskKind::TestWriter)
                    .is_some_and(|node| node.status == TaskStatus::Done)
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
        }

        if node.kind == TaskKind::TestWriter && parent_kind != Some(TaskKind::Top) {
            return Err(format!(
                "Test-writer task \"{}\" must be a direct child of a top-level task (no nested test_writer groups)",
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
        return None;
    }
    None
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
mod tests {
    use super::*;

    fn seed_single_default_task(wf: &mut Workflow, title: &str) {
        wf.sync_planner_tasks_from_file(vec![
            PlannerTaskFileEntry {
                id: "top".to_string(),
                title: title.to_string(),
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
                details: "implementor details".to_string(),
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
                details: "test writer details".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::TestWriter,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("top".to_string()),
                order: Some(1),
            },
            PlannerTaskFileEntry {
                id: "tw-runner".to_string(),
                title: "Run tests".to_string(),
                details: "test runner details".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::TestRunner,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("tw".to_string()),
                order: Some(0),
            },
        ])
        .expect("seed plan should sync");
    }

    fn seed_two_default_tasks(wf: &mut Workflow, first_title: &str, second_title: &str) {
        wf.sync_planner_tasks_from_file(vec![
            PlannerTaskFileEntry {
                id: "top1".to_string(),
                title: first_title.to_string(),
                details: "top details".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Task,
                status: PlannerTaskStatusFile::Pending,
                parent_id: None,
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "impl1".to_string(),
                title: "Implementation".to_string(),
                details: "implementor details".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Implementor,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("top1".to_string()),
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "impl1-audit".to_string(),
                title: "Audit".to_string(),
                details: "audit details".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Auditor,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("impl1".to_string()),
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "tw1".to_string(),
                title: "Write tests".to_string(),
                details: "test writer details".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::TestWriter,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("top1".to_string()),
                order: Some(1),
            },
            PlannerTaskFileEntry {
                id: "tw1-runner".to_string(),
                title: "Run tests".to_string(),
                details: "test runner details".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::TestRunner,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("tw1".to_string()),
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "top2".to_string(),
                title: second_title.to_string(),
                details: "top details".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Task,
                status: PlannerTaskStatusFile::Pending,
                parent_id: None,
                order: Some(1),
            },
            PlannerTaskFileEntry {
                id: "impl2".to_string(),
                title: "Implementation".to_string(),
                details: "implementor details".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Implementor,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("top2".to_string()),
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "impl2-audit".to_string(),
                title: "Audit".to_string(),
                details: "audit details".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Auditor,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("impl2".to_string()),
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "tw2".to_string(),
                title: "Write tests".to_string(),
                details: "test writer details".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::TestWriter,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("top2".to_string()),
                order: Some(1),
            },
            PlannerTaskFileEntry {
                id: "tw2-runner".to_string(),
                title: "Run tests".to_string(),
                details: "test runner details".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::TestRunner,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("tw2".to_string()),
                order: Some(0),
            },
        ])
        .expect("seed plan should sync");
    }

    #[test]
    fn syncs_planner_tasks_from_file_entries() {
        let mut wf = Workflow::default();
        let count = wf
            .sync_planner_tasks_from_file(vec![
                PlannerTaskFileEntry {
                    id: "parent".to_string(),
                    title: "Parent".to_string(),
                    details: "Parent details".to_string(),
                    docs: Vec::new(),
                    kind: PlannerTaskKindFile::Task,
                    status: PlannerTaskStatusFile::Pending,
                    parent_id: None,
                    order: Some(0),
                },
                PlannerTaskFileEntry {
                    id: "child".to_string(),
                    title: "Child".to_string(),
                    details: "Child details".to_string(),
                    docs: Vec::new(),
                    kind: PlannerTaskKindFile::Implementor,
                    status: PlannerTaskStatusFile::InProgress,
                    parent_id: Some("parent".to_string()),
                    order: Some(0),
                },
                PlannerTaskFileEntry {
                    id: "child-audit".to_string(),
                    title: "Audit".to_string(),
                    details: "Audit details".to_string(),
                    docs: Vec::new(),
                    kind: PlannerTaskKindFile::Auditor,
                    status: PlannerTaskStatusFile::Pending,
                    parent_id: Some("child".to_string()),
                    order: Some(0),
                },
            ])
            .expect("sync should succeed");
        assert_eq!(count, 3);
        let lines = wf.right_pane_lines().join("\n");
        assert!(lines.contains("Task: Parent"));
        assert!(lines.contains("Impl: Child"));
    }

    #[test]
    fn renders_nested_task_blocks_with_local_wrapping() {
        let mut wf = Workflow::default();
        wf.sync_planner_tasks_from_file(vec![
            PlannerTaskFileEntry {
                id: "p".to_string(),
                title: "Parent task with a long title".to_string(),
                details: "Parent detail text for rendering.".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Task,
                status: PlannerTaskStatusFile::Pending,
                parent_id: None,
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "c".to_string(),
                title: "Child task title".to_string(),
                details: "Child detail text for rendering.".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Implementor,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("p".to_string()),
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "c-audit".to_string(),
                title: "Child audit".to_string(),
                details: "Audit detail text for rendering.".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Auditor,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("c".to_string()),
                order: Some(0),
            },
        ])
        .expect("sync should succeed");

        let lines = wf
            .right_pane_block_view(24, &HashSet::new())
            .lines
            .join("\n");
        assert!(lines.contains("Parent task with a long title"));
        assert!(lines.contains("┌"));
        assert!(lines.contains("└"));
        assert!(lines.contains("  ┌"));
        assert!(lines.contains("  └"));
        assert!(!lines.contains("Task: Parent task with a long title"));
    }

    #[test]
    fn right_pane_block_view_numbers_and_separates_top_level_tasks() {
        let mut wf = Workflow::default();
        seed_two_default_tasks(&mut wf, "Task One", "Task Two");

        let lines = wf.right_pane_block_view(40, &HashSet::new()).lines;
        let first_title = lines
            .iter()
            .position(|line| line == "  1. Task One")
            .expect("first top-level title should render");
        assert!(
            lines
                .get(first_title + 1)
                .is_some_and(|line| line.trim().is_empty()),
            "expected one spacer line between top-level title and details"
        );
        assert!(lines.iter().any(|line| line == "  2. Task Two"));
        assert!(
            lines
                .iter()
                .any(|line| line == &format!("  {}", "─".repeat(38)))
        );
    }

    #[test]
    fn execution_queues_implementor_and_test_writer_jobs() {
        let mut wf = Workflow::default();
        seed_single_default_task(&mut wf, "Do work");

        wf.start_execution();
        let first = wf.start_next_job().expect("first job should start");
        assert_eq!(first.role, WorkerRole::Implementor);
        assert!(matches!(first.run, JobRun::AgentPrompt(_)));
        wf.append_active_output("implemented".to_string());
        wf.finish_active_job(true, 0);

        let second = wf.start_next_job().expect("second job should start");
        assert_eq!(second.role, WorkerRole::TestWriter);
        assert!(matches!(second.run, JobRun::AgentPrompt(_)));
    }

    #[test]
    fn top_level_tasks_run_sequentially_in_order() {
        let mut wf = Workflow::default();
        seed_two_default_tasks(&mut wf, "Task One", "Task Two");
        wf.start_execution();

        let first = wf.start_next_job().expect("first top-level job");
        let first_top_id = first.top_task_id;
        let mut first_done = false;
        let mut saw_second_top = false;
        let mut next = Some(first);

        for _ in 0..24 {
            let job = next
                .take()
                .unwrap_or_else(|| wf.start_next_job().expect("queued job"));
            if job.top_task_id != first_top_id {
                saw_second_top = true;
                assert!(
                    first_done,
                    "second top-level task started before the first one completed"
                );
                break;
            }

            match job.role {
                WorkerRole::Implementor => wf.append_active_output("implemented".to_string()),
                WorkerRole::TestWriter => wf.append_active_output("wrote tests".to_string()),
                WorkerRole::Auditor => {
                    wf.append_active_output("AUDIT_RESULT: PASS".to_string());
                    wf.append_active_output("No issues found".to_string());
                }
                WorkerRole::TestRunner => wf.append_active_output("all passed".to_string()),
                WorkerRole::FinalAudit => wf.append_active_output("No issues found".to_string()),
            }
            let messages = wf.finish_active_job(true, 0);
            if messages
                .iter()
                .any(|m| m.contains(&format!("Task #{} completed", first_top_id)))
            {
                first_done = true;
            }
        }

        assert!(first_done, "first top-level task never completed");
        assert!(
            saw_second_top,
            "second top-level task never started after first completion"
        );
    }

    #[test]
    fn deterministic_test_runner_loops_back_to_test_writer_on_failure() {
        let mut wf = Workflow::default();
        seed_single_default_task(&mut wf, "Do work");
        wf.start_execution();

        let _ = wf.start_next_job().expect("implementor");
        wf.append_active_output("implemented".to_string());
        wf.finish_active_job(true, 0);

        let _ = wf.start_next_job().expect("test writer");
        wf.append_active_output("wrote tests".to_string());
        wf.finish_active_job(true, 0);

        let auditor = wf.start_next_job().expect("auditor");
        assert_eq!(auditor.role, WorkerRole::Auditor);
        wf.append_active_output("No issues found".to_string());
        wf.finish_active_job(true, 0);

        let runner = wf.start_next_job().expect("test runner");
        assert_eq!(runner.role, WorkerRole::TestRunner);
        assert!(matches!(runner.run, JobRun::DeterministicTestRun));
        wf.append_active_output("test failure output".to_string());
        let messages = wf.finish_active_job(false, 101);
        assert!(messages.iter().any(|m| m.contains("tests failed")));

        let retry = wf.start_next_job().expect("test writer retry");
        assert_eq!(retry.role, WorkerRole::TestWriter);
        match retry.run {
            JobRun::AgentPrompt(prompt) => {
                assert!(prompt.contains("Deterministic test run failed"));
                assert!(prompt.contains("test failure output"));
            }
            JobRun::DeterministicTestRun => panic!("expected agent prompt"),
        }
    }

    #[test]
    fn test_writer_child_auditor_runs_before_test_runner_when_present() {
        let mut wf = Workflow::default();
        wf.sync_planner_tasks_from_file(vec![
            PlannerTaskFileEntry {
                id: "top".to_string(),
                title: "Do work".to_string(),
                details: "top details".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Task,
                status: PlannerTaskStatusFile::Pending,
                parent_id: None,
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "impl".to_string(),
                title: "Impl".to_string(),
                details: "impl details".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Implementor,
                status: PlannerTaskStatusFile::Done,
                parent_id: Some("top".to_string()),
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "impl-audit".to_string(),
                title: "Impl audit".to_string(),
                details: "impl audit".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Auditor,
                status: PlannerTaskStatusFile::Done,
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
                id: "tw-audit".to_string(),
                title: "Audit tests".to_string(),
                details: "audit test details".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Auditor,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("tw".to_string()),
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "tw-runner".to_string(),
                title: "Run tests".to_string(),
                details: "run tests".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::TestRunner,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("tw".to_string()),
                order: Some(1),
            },
        ])
        .expect("sync should succeed");

        wf.start_execution();
        let writer = wf.start_next_job().expect("test writer");
        assert_eq!(writer.role, WorkerRole::TestWriter);
        wf.append_active_output("Wrote tests".to_string());
        let messages = wf.finish_active_job(true, 0);
        assert!(
            messages
                .iter()
                .any(|m| m.contains("test-writer audit queued"))
        );

        let auditor = wf.start_next_job().expect("test-writer auditor");
        assert_eq!(auditor.role, WorkerRole::Auditor);
        match auditor.run {
            JobRun::AgentPrompt(prompt) => {
                assert!(prompt.contains("reviewing test-writing output"));
                assert!(prompt.contains("Parent test-writer details:"));
                assert!(prompt.contains("tw details"));
                assert!(prompt.contains("do not run tests"));
                assert!(prompt.contains("do not execute/check shell commands"));
            }
            JobRun::DeterministicTestRun => panic!("expected auditor prompt"),
        }
        wf.append_active_output("AUDIT_RESULT: PASS".to_string());
        wf.append_active_output("No issues found".to_string());
        wf.finish_active_job(true, 0);

        let runner = wf.start_next_job().expect("test runner");
        assert_eq!(runner.role, WorkerRole::TestRunner);
    }

    #[test]
    fn failing_test_writer_child_auditor_queues_test_writer_retry() {
        let mut wf = Workflow::default();
        wf.sync_planner_tasks_from_file(vec![
            PlannerTaskFileEntry {
                id: "top".to_string(),
                title: "Do work".to_string(),
                details: "top details".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Task,
                status: PlannerTaskStatusFile::Pending,
                parent_id: None,
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "impl".to_string(),
                title: "Impl".to_string(),
                details: "impl details".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Implementor,
                status: PlannerTaskStatusFile::Done,
                parent_id: Some("top".to_string()),
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "impl-audit".to_string(),
                title: "Impl audit".to_string(),
                details: "impl audit".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Auditor,
                status: PlannerTaskStatusFile::Done,
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
                id: "tw-audit".to_string(),
                title: "Audit tests".to_string(),
                details: "audit test details".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Auditor,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("tw".to_string()),
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "tw-runner".to_string(),
                title: "Run tests".to_string(),
                details: "run tests".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::TestRunner,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("tw".to_string()),
                order: Some(1),
            },
        ])
        .expect("sync should succeed");

        wf.start_execution();
        let _ = wf.start_next_job().expect("test writer");
        wf.append_active_output("Wrote tests".to_string());
        wf.finish_active_job(true, 0);

        let _ = wf.start_next_job().expect("test-writer auditor");
        wf.append_active_output("AUDIT_RESULT: FAIL".to_string());
        wf.append_active_output("Missing edge-case assertions".to_string());
        let messages = wf.finish_active_job(true, 0);
        assert!(
            messages
                .iter()
                .any(|m| m.contains("test-writer audit requested fixes"))
        );

        let retry = wf.start_next_job().expect("test writer retry");
        assert_eq!(retry.role, WorkerRole::TestWriter);
        match retry.run {
            JobRun::AgentPrompt(prompt) => {
                assert!(prompt.contains("Audit feedback"));
                assert!(prompt.contains("Missing edge-case assertions"));
            }
            JobRun::DeterministicTestRun => panic!("expected test writer prompt"),
        }
    }

    #[test]
    fn multiple_implementor_audits_run_sequentially() {
        let mut wf = Workflow::default();
        wf.sync_planner_tasks_from_file(vec![
            PlannerTaskFileEntry {
                id: "top".to_string(),
                title: "Do work".to_string(),
                details: "task details".to_string(),
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
                id: "audit-1".to_string(),
                title: "Audit One".to_string(),
                details: "audit one details".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Auditor,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("impl".to_string()),
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "audit-2".to_string(),
                title: "Audit Two".to_string(),
                details: "audit two details".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Auditor,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("impl".to_string()),
                order: Some(1),
            },
        ])
        .expect("sync should succeed");

        wf.start_execution();
        let implementor = wf.start_next_job().expect("implementor");
        assert_eq!(implementor.role, WorkerRole::Implementor);
        wf.append_active_output("implemented".to_string());
        wf.finish_active_job(true, 0);

        let first_audit = wf.start_next_job().expect("first audit");
        assert_eq!(first_audit.role, WorkerRole::Auditor);
        wf.append_active_output("AUDIT_RESULT: PASS".to_string());
        wf.append_active_output("No issues found".to_string());
        wf.finish_active_job(true, 0);

        let second_audit = wf.start_next_job().expect("second audit");
        assert_eq!(second_audit.role, WorkerRole::Auditor);
    }

    #[test]
    fn exhausted_implementor_audit_moves_to_next_audit() {
        let mut wf = Workflow::default();
        wf.sync_planner_tasks_from_file(vec![
            PlannerTaskFileEntry {
                id: "top".to_string(),
                title: "Do work".to_string(),
                details: "task details".to_string(),
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
                id: "audit-1".to_string(),
                title: "Audit One".to_string(),
                details: "audit one details".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Auditor,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("impl".to_string()),
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "audit-2".to_string(),
                title: "Audit Two".to_string(),
                details: "audit two details".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Auditor,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("impl".to_string()),
                order: Some(1),
            },
        ])
        .expect("sync should succeed");

        wf.start_execution();
        let _ = wf.start_next_job().expect("implementor");
        wf.append_active_output("implemented".to_string());
        wf.finish_active_job(true, 0);

        for pass in 1..=4 {
            let _ = wf.start_next_job().expect("audit");
            wf.append_active_output("AUDIT_RESULT: FAIL".to_string());
            wf.append_active_output(format!("critical issue pass {pass}"));
            wf.finish_active_job(true, 0);
            if pass < 4 {
                let _ = wf.start_next_job().expect("implementor retry");
                wf.append_active_output(format!("fix pass {pass}"));
                wf.finish_active_job(true, 0);
            }
        }

        let next_audit = wf.start_next_job().expect("next audit after exhaustion");
        assert_eq!(next_audit.role, WorkerRole::Auditor);
    }

    #[test]
    fn started_jobs_keep_same_parent_context_key_per_branch() {
        let mut wf = Workflow::default();
        seed_single_default_task(&mut wf, "Do work");
        wf.start_execution();

        let implementor = wf.start_next_job().expect("implementor");
        let impl_key = implementor
            .parent_context_key
            .expect("implementor context key");
        wf.append_active_output("implemented".to_string());
        wf.finish_active_job(true, 0);

        let test_writer = wf.start_next_job().expect("test writer");
        let tw_key = test_writer
            .parent_context_key
            .expect("test writer context key");
        wf.append_active_output("wrote tests".to_string());
        wf.finish_active_job(true, 0);

        let auditor = wf.start_next_job().expect("auditor");
        let auditor_key = auditor
            .parent_context_key
            .clone()
            .expect("auditor context key");
        assert_ne!(auditor_key, impl_key);
        wf.append_active_output("AUDIT_RESULT: FAIL".to_string());
        wf.append_active_output("Fix needed".to_string());
        wf.finish_active_job(true, 0);

        let runner = wf.start_next_job().expect("test runner");
        assert_eq!(runner.parent_context_key.as_deref(), Some(tw_key.as_str()));
        wf.append_active_output("all passed".to_string());
        wf.finish_active_job(true, 0);

        let implementor_retry = wf.start_next_job().expect("implementor retry");
        assert_eq!(
            implementor_retry.parent_context_key.as_deref(),
            Some(impl_key.as_str())
        );
        wf.append_active_output("implemented retry".to_string());
        wf.finish_active_job(true, 0);

        let auditor_retry = wf.start_next_job().expect("auditor retry");
        assert_eq!(
            auditor_retry.parent_context_key.as_deref(),
            Some(auditor_key.as_str())
        );
    }

    #[test]
    fn test_fix_loop_stops_after_five_failed_test_runs() {
        let mut wf = Workflow::default();
        seed_single_default_task(&mut wf, "Do work");
        wf.start_execution();

        let _ = wf.start_next_job().expect("implementor");
        wf.append_active_output("implemented".to_string());
        wf.finish_active_job(true, 0);

        let _ = wf.start_next_job().expect("test writer");
        wf.append_active_output("wrote tests".to_string());
        wf.finish_active_job(true, 0);

        let _ = wf.start_next_job().expect("auditor");
        wf.append_active_output("AUDIT_RESULT: PASS".to_string());
        wf.append_active_output("No issues found".to_string());
        wf.finish_active_job(true, 0);

        let mut last_messages = Vec::new();
        for pass in 1..=5 {
            let runner = wf.start_next_job().expect("test runner");
            assert_eq!(runner.role, WorkerRole::TestRunner);
            wf.append_active_output(format!("test run {pass} failed"));
            last_messages = wf.finish_active_job(false, 101);

            if pass < 5 {
                assert!(
                    last_messages
                        .iter()
                        .any(|m| m.contains(&format!("test-writer pass {} queued", pass + 1)))
                );
                let writer = wf.start_next_job().expect("test writer retry");
                assert_eq!(writer.role, WorkerRole::TestWriter);
                wf.append_active_output(format!("updated tests pass {}", pass + 1));
                wf.finish_active_job(true, 0);
            }
        }

        assert!(
            last_messages
                .iter()
                .any(|m| m.contains("queued cleanup removal pass"))
        );
        let cleanup = wf.start_next_job().expect("cleanup writer pass");
        assert_eq!(cleanup.role, WorkerRole::TestWriter);
        match cleanup.run {
            JobRun::AgentPrompt(prompt) => {
                assert!(prompt.contains("Remove the failing tests completely"));
            }
            JobRun::DeterministicTestRun => panic!("expected cleanup writer prompt"),
        }
        wf.append_active_output("Removed failing tests".to_string());
        wf.finish_active_job(true, 0);
        assert!(wf.start_next_job().is_none());
        let tree = wf.right_pane_lines().join("\n");
        assert!(tree.contains("[x] Task: Do work"));
        let failures = wf.drain_recent_failures();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].kind, WorkflowFailureKind::Test);
        assert_eq!(failures[0].attempts, 5);
    }

    #[test]
    fn auditor_output_is_forwarded_to_implementor_retry_prompt() {
        let mut wf = Workflow::default();
        seed_single_default_task(&mut wf, "Do work");
        wf.start_execution();

        let _ = wf.start_next_job().expect("implementor");
        wf.append_active_output("implemented".to_string());
        wf.finish_active_job(true, 0);

        let _ = wf.start_next_job().expect("test writer");
        wf.append_active_output("wrote tests".to_string());
        wf.finish_active_job(true, 0);

        let auditor = wf.start_next_job().expect("auditor");
        assert_eq!(auditor.role, WorkerRole::Auditor);
        wf.append_active_output("Issue: missing edge-case handling".to_string());
        let messages = wf.finish_active_job(true, 0);
        assert!(messages.iter().any(|m| m.contains("audit requested fixes")));

        let runner = wf.start_next_job().expect("test runner");
        assert_eq!(runner.role, WorkerRole::TestRunner);
        wf.append_active_output("all passed".to_string());
        wf.finish_active_job(true, 0);

        let retry = wf.start_next_job().expect("implementor retry");
        assert_eq!(retry.role, WorkerRole::Implementor);
        match retry.run {
            JobRun::AgentPrompt(prompt) => {
                assert!(prompt.contains("Audit feedback"));
                assert!(prompt.contains("Issue: missing edge-case handling"));
            }
            JobRun::DeterministicTestRun => panic!("expected implementor prompt"),
        }
    }

    #[test]
    fn implementor_changed_files_summary_is_forwarded_to_auditor_prompt() {
        let mut wf = Workflow::default();
        seed_single_default_task(&mut wf, "Do work");
        wf.start_execution();

        let _ = wf.start_next_job().expect("implementor");
        wf.append_active_output("Implemented feature updates.".to_string());
        wf.append_active_output("FILES_CHANGED_BEGIN".to_string());
        wf.append_active_output(
            "- src/app.rs: added state transition for command handling".to_string(),
        );
        wf.append_active_output(
            "- src/ui.rs: updated rendering path for task block layout".to_string(),
        );
        wf.append_active_output("FILES_CHANGED_END".to_string());
        wf.finish_active_job(true, 0);

        let _ = wf.start_next_job().expect("test writer");
        wf.append_active_output("Wrote tests".to_string());
        wf.finish_active_job(true, 0);

        let auditor = wf.start_next_job().expect("auditor");
        assert_eq!(auditor.role, WorkerRole::Auditor);
        match auditor.run {
            JobRun::AgentPrompt(prompt) => {
                assert!(prompt.contains("Implementor changed-files summary"));
                assert!(prompt.contains("AUDIT_RESULT: PASS"));
                assert!(prompt.contains("AUDIT_RESULT: FAIL"));
                assert!(
                    prompt.contains("- src/app.rs: added state transition for command handling")
                );
                assert!(
                    prompt.contains("- src/ui.rs: updated rendering path for task block layout")
                );
            }
            JobRun::DeterministicTestRun => panic!("expected auditor prompt"),
        }
    }

    #[test]
    fn audit_result_token_pass_overrides_issue_keywords() {
        assert!(!audit_detects_issues(&[
            "AUDIT_RESULT: PASS".to_string(),
            "Issue: wording only".to_string(),
        ]));
    }

    #[test]
    fn audit_result_token_fail_overrides_no_issues_phrase() {
        assert!(audit_detects_issues(&[
            "AUDIT_RESULT: FAIL".to_string(),
            "No issues found".to_string(),
        ]));
    }

    #[test]
    fn missing_audit_result_token_falls_back_to_heuristics() {
        assert!(audit_detects_issues(&[
            "Issue: edge case missing".to_string()
        ]));
        assert!(!audit_detects_issues(&["No issues found".to_string()]));
    }

    #[test]
    fn audit_strictness_policy_relaxes_by_pass() {
        assert!(audit_strictness_policy(1).contains("strict"));
        assert!(audit_strictness_policy(2).contains("moderate"));
        assert!(audit_strictness_policy(3).contains("high-impact"));
        assert!(audit_strictness_policy(4).contains("critical only"));
        assert!(audit_strictness_policy(8).contains("critical only"));
    }

    #[test]
    fn audit_retry_limit_stops_after_four_failed_audits() {
        let mut wf = Workflow::default();
        seed_single_default_task(&mut wf, "Do work");
        wf.start_execution();

        let _ = wf.start_next_job().expect("implementor pass 1");
        wf.append_active_output("implemented v1".to_string());
        wf.finish_active_job(true, 0);

        let _ = wf.start_next_job().expect("test writer");
        wf.append_active_output("wrote tests".to_string());
        wf.finish_active_job(true, 0);

        let mut last_messages = Vec::new();
        for audit_pass in 1..=4 {
            let auditor = wf.start_next_job().expect("auditor");
            assert_eq!(auditor.role, WorkerRole::Auditor);
            match auditor.run {
                JobRun::AgentPrompt(prompt) => {
                    assert!(prompt.contains(&format!("Audit pass: {audit_pass} of 4")));
                    if audit_pass == 4 {
                        assert!(prompt.contains("truly critical blockers"));
                    }
                }
                JobRun::DeterministicTestRun => panic!("expected auditor prompt"),
            }
            wf.append_active_output("AUDIT_RESULT: FAIL".to_string());
            wf.append_active_output("Critical blocker still present".to_string());
            last_messages = wf.finish_active_job(true, 0);

            if audit_pass == 1 {
                let runner = wf.start_next_job().expect("test runner");
                assert_eq!(runner.role, WorkerRole::TestRunner);
                wf.append_active_output("all passed".to_string());
                wf.finish_active_job(true, 0);
            }

            if audit_pass < 4 {
                let implementor = wf.start_next_job().expect("implementor retry");
                assert_eq!(implementor.role, WorkerRole::Implementor);
                wf.append_active_output(format!("implemented retry {audit_pass}"));
                wf.finish_active_job(true, 0);
            }
        }

        assert!(
            last_messages
                .iter()
                .any(|m| m.contains("Max retries (4) reached"))
        );
        assert!(wf.start_next_job().is_none());
        let tree = wf.right_pane_lines().join("\n");
        assert!(tree.contains("[x] Task: Do work"));
        let failures = wf.drain_recent_failures();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].kind, WorkflowFailureKind::Audit);
        assert_eq!(failures[0].attempts, 4);
    }

    #[test]
    fn implementor_owned_test_runner_runs_after_audits_when_present() {
        let mut wf = Workflow::default();
        wf.sync_planner_tasks_from_file(vec![
            PlannerTaskFileEntry {
                id: "t1".to_string(),
                title: "Do work".to_string(),
                details: "task details".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Task,
                status: PlannerTaskStatusFile::Pending,
                parent_id: None,
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "impl1".to_string(),
                title: "Implementation".to_string(),
                details: "impl details".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Implementor,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("t1".to_string()),
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "runner1".to_string(),
                title: "Existing Tests".to_string(),
                details: "run existing suite".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::TestRunner,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("impl1".to_string()),
                order: Some(1),
            },
            PlannerTaskFileEntry {
                id: "audit1".to_string(),
                title: "Audit".to_string(),
                details: "audit details".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Auditor,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("impl1".to_string()),
                order: Some(0),
            },
        ])
        .expect("sync should succeed");

        wf.start_execution();
        let implementor = wf.start_next_job().expect("implementor");
        assert_eq!(implementor.role, WorkerRole::Implementor);
        wf.append_active_output("implemented".to_string());
        wf.finish_active_job(true, 0);

        let auditor = wf.start_next_job().expect("auditor");
        assert_eq!(auditor.role, WorkerRole::Auditor);
        wf.append_active_output("AUDIT_RESULT: PASS".to_string());
        wf.append_active_output("No issues found".to_string());
        wf.finish_active_job(true, 0);

        let existing_runner = wf.start_next_job().expect("existing runner");
        assert_eq!(existing_runner.role, WorkerRole::TestRunner);
        assert!(matches!(existing_runner.run, JobRun::DeterministicTestRun));
        wf.append_active_output("existing tests passed".to_string());
        wf.finish_active_job(true, 0);

        let tree = wf.right_pane_lines().join("\n");
        assert!(tree.contains("[x] Task: Do work"));
    }

    #[test]
    fn implementor_owned_test_runner_exhaustion_records_failure_and_continues_to_next_step() {
        let mut wf = Workflow::default();
        wf.sync_planner_tasks_from_file(vec![
            PlannerTaskFileEntry {
                id: "t1".to_string(),
                title: "Do work".to_string(),
                details: "task details".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Task,
                status: PlannerTaskStatusFile::Pending,
                parent_id: None,
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "impl1".to_string(),
                title: "Implementation".to_string(),
                details: "impl details".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Implementor,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("t1".to_string()),
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "runner1".to_string(),
                title: "Existing Tests".to_string(),
                details: "run existing suite".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::TestRunner,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("impl1".to_string()),
                order: Some(1),
            },
            PlannerTaskFileEntry {
                id: "audit1".to_string(),
                title: "Audit".to_string(),
                details: "audit details".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Auditor,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("impl1".to_string()),
                order: Some(0),
            },
        ])
        .expect("sync should succeed");

        wf.start_execution();
        let implementor = wf.start_next_job().expect("implementor");
        assert_eq!(implementor.role, WorkerRole::Implementor);
        wf.append_active_output("implemented initial".to_string());
        wf.finish_active_job(true, 0);

        let auditor = wf.start_next_job().expect("auditor");
        assert_eq!(auditor.role, WorkerRole::Auditor);
        wf.append_active_output("AUDIT_RESULT: PASS".to_string());
        wf.append_active_output("No issues found".to_string());
        wf.finish_active_job(true, 0);

        for pass in 1..=5 {
            let existing_runner = wf.start_next_job().expect("existing runner");
            assert_eq!(existing_runner.role, WorkerRole::TestRunner);
            wf.append_active_output(format!("existing tests failed pass {pass}"));
            let messages = wf.finish_active_job(false, 101);
            if pass == 5 {
                assert!(
                    messages
                        .iter()
                        .any(|m| m.contains("Max retries (5) reached"))
                );
            } else {
                let implementor = wf.start_next_job().expect("implementor retry");
                assert_eq!(implementor.role, WorkerRole::Implementor);
                wf.append_active_output(format!("implemented pass {}", pass + 1));
                wf.finish_active_job(true, 0);
            }
        }

        let failures = wf.drain_recent_failures();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].kind, WorkflowFailureKind::Test);
        assert_eq!(failures[0].attempts, 5);
        let tree = wf.right_pane_lines().join("\n");
        assert!(tree.contains("[x] Task: Do work"));
    }

    #[test]
    fn top_task_done_requires_impl_and_test_branches() {
        let mut wf = Workflow::default();
        seed_single_default_task(&mut wf, "Do work");
        wf.start_execution();

        let _ = wf.start_next_job().expect("implementor");
        wf.append_active_output("implemented".to_string());
        wf.finish_active_job(true, 0);

        let _ = wf.start_next_job().expect("test writer");
        wf.append_active_output("wrote tests".to_string());
        wf.finish_active_job(true, 0);

        let _ = wf.start_next_job().expect("auditor");
        wf.append_active_output("No issues found".to_string());
        wf.finish_active_job(true, 0);

        let tree = wf.right_pane_lines().join("\n");
        assert!(!tree.contains("[x] Task: Do work"));

        let _ = wf.start_next_job().expect("test runner");
        wf.append_active_output("all passed".to_string());
        wf.finish_active_job(true, 0);

        let tree = wf.right_pane_lines().join("\n");
        assert!(tree.contains("[x] Task: Do work"));
    }

    #[test]
    fn sync_rejects_tasks_missing_details() {
        let mut wf = Workflow::default();
        let err = wf
            .sync_planner_tasks_from_file(vec![PlannerTaskFileEntry {
                id: "t1".to_string(),
                title: "Task".to_string(),
                details: "".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Task,
                status: PlannerTaskStatusFile::Pending,
                parent_id: None,
                order: Some(0),
            }])
            .expect_err("missing details should fail");
        assert!(err.contains("non-empty details"));
    }

    #[test]
    fn sync_rejects_implementor_without_auditor() {
        let mut wf = Workflow::default();
        let err = wf
            .sync_planner_tasks_from_file(vec![
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
            ])
            .expect_err("should reject missing auditor");
        assert!(err.contains("must include at least one auditor"));
    }

    #[test]
    fn sync_rejects_implementor_test_runner_before_audit() {
        let mut wf = Workflow::default();
        let err = wf
            .sync_planner_tasks_from_file(vec![
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
                PlannerTaskFileEntry {
                    id: "impl-runner".to_string(),
                    title: "Runner".to_string(),
                    details: "d".to_string(),
                    docs: Vec::new(),
                    kind: PlannerTaskKindFile::TestRunner,
                    status: PlannerTaskStatusFile::Pending,
                    parent_id: Some("impl".to_string()),
                    order: Some(0),
                },
                PlannerTaskFileEntry {
                    id: "impl-audit".to_string(),
                    title: "Audit".to_string(),
                    details: "d".to_string(),
                    docs: Vec::new(),
                    kind: PlannerTaskKindFile::Auditor,
                    status: PlannerTaskStatusFile::Pending,
                    parent_id: Some("impl".to_string()),
                    order: Some(1),
                },
            ])
            .expect_err("should reject runner before audit");
        assert!(err.contains("test_runner must come after audit"));
    }

    #[test]
    fn sync_rejects_test_writer_without_test_runner() {
        let mut wf = Workflow::default();
        let err = wf
            .sync_planner_tasks_from_file(vec![
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
                PlannerTaskFileEntry {
                    id: "impl-audit".to_string(),
                    title: "Audit".to_string(),
                    details: "d".to_string(),
                    docs: Vec::new(),
                    kind: PlannerTaskKindFile::Auditor,
                    status: PlannerTaskStatusFile::Pending,
                    parent_id: Some("impl".to_string()),
                    order: Some(0),
                },
                PlannerTaskFileEntry {
                    id: "tw".to_string(),
                    title: "Tests".to_string(),
                    details: "d".to_string(),
                    docs: Vec::new(),
                    kind: PlannerTaskKindFile::TestWriter,
                    status: PlannerTaskStatusFile::Pending,
                    parent_id: Some("top".to_string()),
                    order: Some(1),
                },
            ])
            .expect_err("should reject missing test runner");
        assert!(err.contains("must include at least one test_runner"));
    }

    #[test]
    fn sync_rejects_nested_test_writer_groups() {
        let mut wf = Workflow::default();
        let err = wf
            .sync_planner_tasks_from_file(vec![
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
                PlannerTaskFileEntry {
                    id: "impl-audit".to_string(),
                    title: "Audit".to_string(),
                    details: "d".to_string(),
                    docs: Vec::new(),
                    kind: PlannerTaskKindFile::Auditor,
                    status: PlannerTaskStatusFile::Pending,
                    parent_id: Some("impl".to_string()),
                    order: Some(0),
                },
                PlannerTaskFileEntry {
                    id: "tests-parent".to_string(),
                    title: "Tests".to_string(),
                    details: "d".to_string(),
                    docs: Vec::new(),
                    kind: PlannerTaskKindFile::TestWriter,
                    status: PlannerTaskStatusFile::Pending,
                    parent_id: Some("top".to_string()),
                    order: Some(1),
                },
                PlannerTaskFileEntry {
                    id: "tests-parent-runner".to_string(),
                    title: "Run parent".to_string(),
                    details: "d".to_string(),
                    docs: Vec::new(),
                    kind: PlannerTaskKindFile::TestRunner,
                    status: PlannerTaskStatusFile::Pending,
                    parent_id: Some("tests-parent".to_string()),
                    order: Some(0),
                },
                PlannerTaskFileEntry {
                    id: "tests-child".to_string(),
                    title: "Nested tests".to_string(),
                    details: "d".to_string(),
                    docs: Vec::new(),
                    kind: PlannerTaskKindFile::TestWriter,
                    status: PlannerTaskStatusFile::Pending,
                    parent_id: Some("tests-parent".to_string()),
                    order: Some(1),
                },
                PlannerTaskFileEntry {
                    id: "tests-child-runner".to_string(),
                    title: "Run child".to_string(),
                    details: "d".to_string(),
                    docs: Vec::new(),
                    kind: PlannerTaskKindFile::TestRunner,
                    status: PlannerTaskStatusFile::Pending,
                    parent_id: Some("tests-child".to_string()),
                    order: Some(0),
                },
            ])
            .expect_err("should reject nested test writer grouping");
        assert!(err.contains("must be a direct child of a top-level task"));
    }

    #[test]
    fn implementor_and_auditor_prompts_include_test_guardrails() {
        let mut wf = Workflow::default();
        seed_single_default_task(&mut wf, "Do work");
        wf.start_execution();
        let implementor = wf.start_next_job().expect("implementor");
        match implementor.run {
            JobRun::AgentPrompt(prompt) => {
                assert!(prompt.contains("do not create or modify tests unless"));
                assert!(prompt.contains("Implementation details:"));
                assert!(prompt.contains("implementor details"));
            }
            JobRun::DeterministicTestRun => panic!("expected implementor prompt"),
        }

        wf.append_active_output("implemented".to_string());
        wf.finish_active_job(true, 0);
        let _ = wf.start_next_job().expect("test writer");
        wf.append_active_output("tests".to_string());
        wf.finish_active_job(true, 0);
        let auditor = wf.start_next_job().expect("auditor");
        match auditor.run {
            JobRun::AgentPrompt(prompt) => {
                assert!(prompt.contains("do not audit test quality/coverage"));
                assert!(prompt.contains("Parent implementor details:"));
                assert!(prompt.contains("implementor details"));
                assert!(prompt.contains("Scope lock (required): audit only the parent implementor"));
                assert!(prompt.contains("do not run tests"));
                assert!(prompt.contains("do not execute/check shell commands"));
            }
            JobRun::DeterministicTestRun => panic!("expected auditor prompt"),
        }
    }

    #[test]
    fn worker_prompts_prepend_task_docs_and_web_read_instruction() {
        let mut wf = Workflow::default();
        wf.sync_planner_tasks_from_file(vec![
            PlannerTaskFileEntry {
                id: "top".to_string(),
                title: "Do work".to_string(),
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
                docs: vec![PlannerTaskDocFileEntry {
                    title: "Impl guide".to_string(),
                    url: "https://docs.example/impl".to_string(),
                    summary: "Implementation guidance".to_string(),
                }],
                kind: PlannerTaskKindFile::Implementor,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("top".to_string()),
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "impl-audit".to_string(),
                title: "Implementation audit".to_string(),
                details: "audit details".to_string(),
                docs: vec![PlannerTaskDocFileEntry {
                    title: "Audit guide".to_string(),
                    url: "https://docs.example/impl-audit".to_string(),
                    summary: "Audit guidance".to_string(),
                }],
                kind: PlannerTaskKindFile::Auditor,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("impl".to_string()),
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "tw".to_string(),
                title: "Write tests".to_string(),
                details: "test writer details".to_string(),
                docs: vec![PlannerTaskDocFileEntry {
                    title: "Testing guide".to_string(),
                    url: "https://docs.example/test-writer".to_string(),
                    summary: "Test-writing guidance".to_string(),
                }],
                kind: PlannerTaskKindFile::TestWriter,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("top".to_string()),
                order: Some(1),
            },
            PlannerTaskFileEntry {
                id: "tw-audit".to_string(),
                title: "Test audit".to_string(),
                details: "test audit details".to_string(),
                docs: vec![PlannerTaskDocFileEntry {
                    title: "Test audit guide".to_string(),
                    url: "https://docs.example/test-audit".to_string(),
                    summary: "Test audit guidance".to_string(),
                }],
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
                docs: vec![PlannerTaskDocFileEntry {
                    title: "Final review guide".to_string(),
                    url: "https://docs.example/final-audit".to_string(),
                    summary: "Final audit guidance".to_string(),
                }],
                kind: PlannerTaskKindFile::FinalAudit,
                status: PlannerTaskStatusFile::Pending,
                parent_id: None,
                order: Some(1),
            },
        ])
        .expect("sync should succeed");

        wf.start_execution();
        let mut saw_impl = false;
        let mut saw_impl_auditor = false;
        let mut saw_test_writer = false;
        let mut saw_test_auditor = false;
        let mut saw_final_audit = false;

        for _ in 0..24 {
            let Some(job) = wf.start_next_job() else {
                break;
            };
            match &job.run {
                JobRun::AgentPrompt(prompt) => {
                    assert!(prompt.starts_with("Task documentation requirements:"));
                    assert!(prompt.contains("read every linked document from the web"));
                    if prompt.contains("You are an implementation sub-agent.") {
                        saw_impl = true;
                        assert!(prompt.contains("https://docs.example/impl"));
                    } else if prompt.contains("reviewing implementation output") {
                        saw_impl_auditor = true;
                        assert!(prompt.contains("https://docs.example/impl-audit"));
                    } else if prompt.contains("You are a test-writer sub-agent.") {
                        saw_test_writer = true;
                        assert!(prompt.contains("https://docs.example/test-writer"));
                    } else if prompt.contains("reviewing test-writing output") {
                        saw_test_auditor = true;
                        assert!(prompt.contains("https://docs.example/test-audit"));
                    } else if prompt.contains("You are a final audit sub-agent.") {
                        saw_final_audit = true;
                        assert!(prompt.contains("https://docs.example/final-audit"));
                    } else {
                        panic!("unexpected prompt variant: {prompt}");
                    }
                }
                JobRun::DeterministicTestRun => {}
            }

            match job.role {
                WorkerRole::Auditor | WorkerRole::FinalAudit => {
                    wf.append_active_output("AUDIT_RESULT: PASS".to_string());
                    wf.append_active_output("No issues found".to_string());
                }
                WorkerRole::TestRunner => {
                    wf.append_active_output("all passed".to_string());
                }
                _ => wf.append_active_output("completed".to_string()),
            }
            wf.finish_active_job(true, 0);
        }

        assert!(saw_impl, "implementor prompt was not observed");
        assert!(
            saw_impl_auditor,
            "implementor auditor prompt was not observed"
        );
        assert!(saw_test_writer, "test-writer prompt was not observed");
        assert!(
            saw_test_auditor,
            "test-writer auditor prompt was not observed"
        );
        assert!(saw_final_audit, "final-audit prompt was not observed");
    }

    #[test]
    fn start_execution_picks_unfinished_task_when_some_are_done() {
        let mut wf = Workflow::default();
        wf.sync_planner_tasks_from_file(vec![
            PlannerTaskFileEntry {
                id: "done-task".to_string(),
                title: "Done task".to_string(),
                details: "already done".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Task,
                status: PlannerTaskStatusFile::Done,
                parent_id: None,
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "todo-task".to_string(),
                title: "Pending task".to_string(),
                details: "needs execution".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Task,
                status: PlannerTaskStatusFile::Pending,
                parent_id: None,
                order: Some(1),
            },
        ])
        .expect("sync should succeed");

        wf.start_execution();
        let first = wf.start_next_job().expect("should start unfinished task");
        assert_eq!(first.role, WorkerRole::Implementor);
        match first.run {
            JobRun::AgentPrompt(prompt) => assert!(prompt.contains("Pending task")),
            JobRun::DeterministicTestRun => panic!("expected implementor prompt"),
        }
    }

    #[test]
    fn start_execution_resumes_with_pending_implementor_auditor_subtask() {
        let mut wf = Workflow::default();
        wf.sync_planner_tasks_from_file(vec![
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
                status: PlannerTaskStatusFile::Done,
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
                status: PlannerTaskStatusFile::Done,
                parent_id: Some("top".to_string()),
                order: Some(1),
            },
            PlannerTaskFileEntry {
                id: "tw-runner".to_string(),
                title: "Run tests".to_string(),
                details: "runner details".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::TestRunner,
                status: PlannerTaskStatusFile::Done,
                parent_id: Some("tw".to_string()),
                order: Some(0),
            },
        ])
        .expect("sync should succeed");

        wf.start_execution();
        let started = wf.start_next_job().expect("should resume pending auditor");
        assert_eq!(started.role, WorkerRole::Auditor);
        match started.run {
            JobRun::AgentPrompt(prompt) => {
                assert!(prompt.contains("reviewing implementation output"));
            }
            JobRun::DeterministicTestRun => panic!("expected auditor prompt"),
        }
    }

    #[test]
    fn planner_tasks_for_file_reflects_runtime_status_transitions() {
        let mut wf = Workflow::default();
        seed_single_default_task(&mut wf, "Do work");
        wf.start_execution();
        let _ = wf.start_next_job().expect("implementor");
        wf.append_active_output("implemented".to_string());
        wf.finish_active_job(true, 0);

        let snapshot = wf.planner_tasks_for_file();
        let impl_status = snapshot
            .iter()
            .find(|entry| entry.id == "impl")
            .map(|entry| entry.status);
        assert_eq!(impl_status, Some(PlannerTaskStatusFile::Done));

        let audit_status = snapshot
            .iter()
            .find(|entry| entry.id == "impl-audit")
            .map(|entry| entry.status);
        assert_eq!(audit_status, Some(PlannerTaskStatusFile::Pending));
    }

    #[test]
    fn final_audit_runs_after_non_final_tasks_complete() {
        let mut wf = Workflow::default();
        wf.sync_planner_tasks_from_file(vec![
            PlannerTaskFileEntry {
                id: "t1".to_string(),
                title: "Do work".to_string(),
                details: "task details".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Task,
                status: PlannerTaskStatusFile::Pending,
                parent_id: None,
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "fa".to_string(),
                title: "Final Audit".to_string(),
                details: "final audit details".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::FinalAudit,
                status: PlannerTaskStatusFile::Pending,
                parent_id: None,
                order: Some(1),
            },
        ])
        .expect("sync should succeed");

        wf.start_execution();
        let _ = wf.start_next_job().expect("implementor");
        wf.append_active_output("implemented".to_string());
        wf.finish_active_job(true, 0);

        let _ = wf.start_next_job().expect("test writer");
        wf.append_active_output("tests".to_string());
        wf.finish_active_job(true, 0);

        let _ = wf.start_next_job().expect("auditor");
        wf.append_active_output("No issues found".to_string());
        wf.finish_active_job(true, 0);

        let _ = wf.start_next_job().expect("test runner");
        wf.append_active_output("all passed".to_string());
        wf.finish_active_job(true, 0);

        let final_job = wf.start_next_job().expect("final audit");
        assert_eq!(final_job.role, WorkerRole::FinalAudit);
    }

    #[test]
    fn replace_rolling_context_entries_trims_to_limit() {
        let mut wf = Workflow::default();
        let entries = (0..40)
            .map(|idx| format!("entry-{idx}"))
            .collect::<Vec<_>>();
        wf.replace_rolling_context_entries(entries);
        let loaded = wf.rolling_context_entries();
        assert_eq!(loaded.len(), 16);
        assert_eq!(loaded.first().map(String::as_str), Some("entry-24"));
        assert_eq!(loaded.last().map(String::as_str), Some("entry-39"));
    }

    #[test]
    fn right_pane_shows_docs_badge_for_non_test_runner_only() {
        let mut wf = Workflow::default();
        wf.sync_planner_tasks_from_file(vec![
            PlannerTaskFileEntry {
                id: "task".to_string(),
                title: "Task".to_string(),
                details: "Top details".to_string(),
                docs: vec![PlannerTaskDocFileEntry {
                    title: "Doc".to_string(),
                    url: "https://example.com".to_string(),
                    summary: "summary".to_string(),
                }],
                kind: PlannerTaskKindFile::Task,
                status: PlannerTaskStatusFile::Pending,
                parent_id: None,
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "runner".to_string(),
                title: "Runner".to_string(),
                details: "Runner details".to_string(),
                docs: vec![PlannerTaskDocFileEntry {
                    title: "Should hide".to_string(),
                    url: "https://example.com".to_string(),
                    summary: "summary".to_string(),
                }],
                kind: PlannerTaskKindFile::TestRunner,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("task".to_string()),
                order: Some(0),
            },
        ])
        .expect("sync should succeed");
        let text = wf
            .right_pane_block_view(80, &HashSet::new())
            .lines
            .join("\n");
        assert!(text.contains("[documentation attached]"));
        assert_eq!(text.matches("[documentation attached]").count(), 1);
    }

    #[test]
    fn right_pane_docs_toggle_expands_full_docs_without_collapsed_preview() {
        let mut wf = Workflow::default();
        wf.sync_planner_tasks_from_file(vec![PlannerTaskFileEntry {
            id: "task".to_string(),
            title: "Task".to_string(),
            details: "Top details".to_string(),
            docs: vec![PlannerTaskDocFileEntry {
                title: "Doc Title".to_string(),
                url: "https://example.com/doc".to_string(),
                summary: "Doc summary".to_string(),
            }],
            kind: PlannerTaskKindFile::Task,
            status: PlannerTaskStatusFile::Pending,
            parent_id: None,
            order: Some(0),
        }])
        .expect("sync should succeed");

        let collapsed = wf
            .right_pane_block_view(80, &HashSet::new())
            .lines
            .join("\n");
        assert!(collapsed.contains("[documentation attached] [+]"));
        assert!(!collapsed.contains("Doc Title"));
        assert!(!collapsed.contains("https://example.com/doc"));
        assert!(!collapsed.contains("Doc summary"));

        let mut expanded = HashSet::new();
        expanded.insert(docs_toggle_key("task"));
        let expanded_text = wf.right_pane_block_view(80, &expanded).lines.join("\n");
        assert!(expanded_text.contains("[documentation attached] [-]"));
        assert!(expanded_text.contains("Doc Title"));
        assert!(expanded_text.contains("https://example.com/doc"));
        assert!(expanded_text.contains("Doc summary"));
    }
}
