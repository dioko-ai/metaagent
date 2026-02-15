use std::collections::HashSet;
use std::cell::RefCell;
use std::sync::Arc;

use crate::session_store::PlannerTaskFileEntry;
use crate::text_layout::wrap_word_with_positions;
use crate::workflow::{RightPaneBlockView, StartedJob, WorkerRole, Workflow, WorkflowFailure};

const COMMAND_INDEX: [(&str, &str); 15] = [
    ("/start", "Start execution"),
    ("/planner", "Show collaborative planner markdown"),
    ("/convert", "Convert planner markdown to tasks"),
    ("/skip-plan", "Show task list view"),
    ("/quit", "Quit app"),
    ("/exit", "Quit app"),
    ("/attach-docs", "Attach docs to tasks"),
    ("/newmaster", "Start a new master session"),
    ("/resume", "Resume a prior session"),
    ("/split-audits", "Split audits per concern"),
    ("/merge-audits", "Merge audits"),
    ("/split-tests", "Split tests per concern"),
    ("/merge-tests", "Merge tests"),
    ("/add-final-audit", "Add final audit task"),
    ("/remove-final-audit", "Remove final audit task"),
];
const MAX_LEFT_TOP_LINES: usize = 2000;

#[derive(Debug, Clone)]
struct WrappedPaneCache {
    width: u16,
    generation: u64,
    rendered: Arc<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandSuggestion {
    pub command: &'static str,
    pub description: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumeSessionOption {
    pub session_dir: String,
    pub workspace: String,
    pub title: Option<String>,
    pub created_at_label: Option<String>,
    pub last_used_epoch_secs: u64,
}

#[derive(Debug, Clone)]
struct ResumePickerState {
    entries: Vec<ResumeSessionOption>,
    selected: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    LeftTop,
    LeftBottom,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RightPaneMode {
    TaskList,
    PlannerMarkdown,
}

#[derive(Debug)]
pub struct App {
    pub running: bool,
    pub ticks: u64,
    pub active_pane: Pane,
    right_pane_mode: RightPaneMode,
    left_top_lines: Vec<String>,
    left_top_generation: u64,
    left_top_wrap_cache: RefCell<Option<WrappedPaneCache>>,
    chat_messages: Vec<String>,
    right_lines: Vec<String>,
    planner_markdown: String,
    left_top_scroll: u16,
    chat_scroll: u16,
    right_scroll: u16,
    chat_input: String,
    chat_cursor: usize,
    chat_cursor_goal_col: Option<u16>,
    reported_context_entries: usize,
    expanded_detail_keys: HashSet<String>,
    resume_picker: Option<ResumePickerState>,
    task_check_in_progress: bool,
    docs_attach_in_progress: bool,
    master_in_progress: bool,
    workflow: Workflow,
}

impl Default for App {
    fn default() -> Self {
        let workflow = Workflow::default();
        Self {
            running: true,
            ticks: 0,
            active_pane: Pane::LeftBottom,
            right_pane_mode: RightPaneMode::PlannerMarkdown,
            left_top_lines: vec![
                "Sub-agent output stream.".to_string(),
                "Implementor and auditor logs appear here.".to_string(),
            ],
            left_top_generation: 0,
            left_top_wrap_cache: RefCell::new(None),
            chat_messages: Vec::new(),
            right_lines: vec![
                "# Collaborative Planner".to_string(),
                String::new(),
                "The planner markdown is currently empty.".to_string(),
                "Chat with the agent in the left pane to build a codebase-aware plan.".to_string(),
                "Use /convert when you're ready to implement from the plan.".to_string(),
            ],
            planner_markdown: String::new(),
            left_top_scroll: 0,
            chat_scroll: 0,
            right_scroll: 0,
            chat_input: String::new(),
            chat_cursor: 0,
            chat_cursor_goal_col: None,
            reported_context_entries: 0,
            expanded_detail_keys: HashSet::new(),
            resume_picker: None,
            task_check_in_progress: false,
            docs_attach_in_progress: false,
            master_in_progress: false,
            workflow,
        }
    }
}

impl App {
    pub fn on_tick(&mut self) {
        self.ticks = self.ticks.saturating_add(1);
    }

    pub fn quit(&mut self) {
        self.running = false;
    }

    pub fn next_pane(&mut self) {
        self.active_pane = match self.active_pane {
            Pane::LeftTop => Pane::LeftBottom,
            Pane::LeftBottom => Pane::Right,
            Pane::Right => Pane::LeftTop,
        };
    }

    pub fn prev_pane(&mut self) {
        self.active_pane = match self.active_pane {
            Pane::LeftTop => Pane::Right,
            Pane::LeftBottom => Pane::LeftTop,
            Pane::Right => Pane::LeftBottom,
        };
    }

    pub fn scroll_up(&mut self) {
        let scroll = self.scroll_mut(self.active_pane);
        *scroll = scroll.saturating_sub(1);
    }

    pub fn scroll_down(&mut self) {
        let max_scroll = self.max_scroll(self.active_pane);
        let scroll = self.scroll_mut(self.active_pane);
        *scroll = (*scroll + 1).min(max_scroll);
    }

    pub fn input_char(&mut self, c: char) {
        let byte_idx = char_to_byte_idx(&self.chat_input, self.chat_cursor);
        self.chat_input.insert(byte_idx, c);
        self.chat_cursor = self.chat_cursor.saturating_add(1);
        self.chat_cursor_goal_col = None;
    }

    pub fn backspace_input(&mut self) {
        if self.chat_cursor == 0 {
            return;
        }

        let start = char_to_byte_idx(&self.chat_input, self.chat_cursor.saturating_sub(1));
        let end = char_to_byte_idx(&self.chat_input, self.chat_cursor);
        self.chat_input.drain(start..end);
        self.chat_cursor = self.chat_cursor.saturating_sub(1);
        self.chat_cursor_goal_col = None;
    }

    pub fn move_cursor_left(&mut self) {
        self.chat_cursor = self.chat_cursor.saturating_sub(1);
        self.chat_cursor_goal_col = None;
    }

    pub fn move_cursor_right(&mut self) {
        let char_len = self.chat_input.chars().count();
        self.chat_cursor = (self.chat_cursor + 1).min(char_len);
        self.chat_cursor_goal_col = None;
    }

    pub fn move_cursor_up(&mut self, width: u16) {
        let width = width.max(1);
        let positions = wrap_word_with_positions(&self.chat_input, width).positions;
        let (line, col) = positions[self.chat_cursor];
        if line == 0 {
            return;
        }
        let goal_col = self.chat_cursor_goal_col.unwrap_or(col);
        self.chat_cursor = nearest_index_for_line_col(&positions, line - 1, goal_col);
        self.chat_cursor_goal_col = Some(goal_col);
    }

    pub fn move_cursor_down(&mut self, width: u16) {
        let width = width.max(1);
        let positions = wrap_word_with_positions(&self.chat_input, width).positions;
        let (line, col) = positions[self.chat_cursor];
        let max_line = positions.iter().map(|(l, _)| *l).max().unwrap_or(0);
        if line >= max_line {
            return;
        }
        let goal_col = self.chat_cursor_goal_col.unwrap_or(col);
        self.chat_cursor = nearest_index_for_line_col(&positions, line + 1, goal_col);
        self.chat_cursor_goal_col = Some(goal_col);
    }

    pub fn scroll_chat_up(&mut self) {
        self.chat_scroll = self.chat_scroll.saturating_sub(1);
    }

    pub fn scroll_chat_down(&mut self, max_scroll: u16) {
        self.chat_scroll = (self.chat_scroll + 1).min(max_scroll);
    }

    pub fn scroll_right_up(&mut self) {
        self.right_scroll = self.right_scroll.saturating_sub(1);
    }

    pub fn scroll_right_down(&mut self, max_scroll: u16) {
        self.right_scroll = (self.right_scroll + 1).min(max_scroll);
    }

    pub fn submit_chat_message(&mut self) -> Option<String> {
        let message = self.chat_input.trim().to_string();
        if message.is_empty() {
            return None;
        }

        self.chat_messages.push(format!("You: {message}"));
        self.chat_input.clear();
        self.chat_cursor = 0;
        self.chat_cursor_goal_col = None;
        Some(message)
    }

    pub fn submit_direct_message(&mut self, raw_message: &str) -> Option<String> {
        let message = raw_message.trim().to_string();
        if message.is_empty() {
            return None;
        }
        self.chat_messages.push(format!("You: {message}"));
        self.chat_input.clear();
        self.chat_cursor = 0;
        self.chat_cursor_goal_col = None;
        Some(message)
    }

    pub fn push_agent_message(&mut self, message: impl Into<String>) {
        self.chat_messages.push(message.into());
    }

    pub fn prepare_master_prompt(&self, message: &str, tasks_file: &str) -> String {
        format!(
            "{}\n\
             Planner storage:\n\
             - Read and update this JSON file directly: {tasks_file}\n\
             - Never modify project workspace/source files directly.\n\
             - You may only edit files in the current meta-agent session directory (including tasks/context artifacts).\n\
             - If user requests more work after existing tasks are completed, append new tasks; do not delete completed task history.\n\
             - After task list updates are ready, tell the user `/start` is ready to run.\n\
             - `/start` always resumes from the last unfinished task.\n\
             - File schema: array of objects with fields id, title, details, docs, kind, status, parent_id, order\n\
             - kind values: task, final_audit, implementor, auditor, test_writer, test_runner\n\
             - `docs` is reserved for `/attach-docs`. Do not populate or modify `docs` in master edits.\n\
             - For new tasks created by master, set `docs` to [] and leave it empty.\n\
             - Every task and sub-task must include a non-empty details field with concrete implementation/audit/test intent.\n\
             - Testing-decision flow before initial planning in a session:\n\
               If project info indicates tests are absent/unknown, ask user whether to set up a testing system first.\n\
               If tests exist, ask whether to write new tests as part of this work.\n\
               If tests exist, also ask whether to enforce existing tests (do not break) or ignore tests entirely.\n\
             - Use task structure based on user testing choices:\n\
               Always include implementor under each top-level task.\n\
               Every implementor must include at least one auditor subtask.\n\
               Implementor scope should avoid test-writing/modification unless tied to a direct implementor test_runner branch for existing-test verification.\n\
               Implementor auditors must not include test-related checks; test concerns belong only to test_runner/test_writer branches.\n\
               Include test_writer branch only when user wants new tests written.\n\
               Every test_writer must be a direct child of the top-level task (no nested/umbrella test_writer grouping tasks).\n\
               If user wants existing tests enforced, include test_runner under implementor so failures report back to implementor.\n\
               If implementor has a test_runner, order it after implementor audit subtasks.\n\
               Every test_writer must include at least one test_runner subtask.\n\
               If user chooses to ignore tests, omit test_writer and implementor test_runner branches.\n\
               Special case when tests are absent/unknown and user wants tests:\n\
               Create a dedicated testing-setup top-level task at the earliest position (before feature work that depends on tests).\n\
               That setup task must include an implementor + auditor flow where implementor sets up the test framework/tooling and updates session meta.json test_command to the exact bash-runnable command string.\n\
               Do not add non-setup test_writer or test_runner branches until after that setup task in task order.\n\
             - If user testing choices are still missing, ask concise questions first and wait; do not write tasks.json yet.\n\
             - Update tasks.json only when task state should change.\n\
             - Conversational answers that do not change task state do not require tasks.json edits.\n\
             - Do not ask the user to start execution until task updates are ready.\n\
             - After updating tasks.json, explain to the user what changed.",
            self.workflow.prepare_master_prompt(message)
        )
    }

    pub fn prepare_context_report_prompt(&self, context_entries: &[String]) -> String {
        format!(
            "Rolling task context has new updates:\n{}\n\
             Read these updates and respond with exactly one brief user-facing sentence.\n\
             The sentence must start with: \"Here's what just happened:\"\n\
             Focus on concrete progress and outcomes.\n\
             Do not emit TASK_OPS or modify planner state for this update.\n\
             Do not make any file changes. Simply return the message.",
            context_entries
                .iter()
                .map(|entry| format!("- {entry}"))
                .collect::<Vec<_>>()
                .join("\n")
        )
    }

    pub fn sync_planner_tasks_from_file(
        &mut self,
        entries: Vec<PlannerTaskFileEntry>,
    ) -> Result<(), String> {
        self.workflow.sync_planner_tasks_from_file(entries)?;
        self.prune_expanded_detail_keys();
        self.refresh_right_lines();
        Ok(())
    }

    pub fn rolling_context_entries(&self) -> Vec<String> {
        self.workflow.rolling_context_entries()
    }

    pub fn planner_tasks_for_file(&self) -> Vec<PlannerTaskFileEntry> {
        self.workflow.planner_tasks_for_file()
    }

    pub fn is_start_execution_command(message: &str) -> bool {
        let normalized = message.trim().to_lowercase();
        matches!(
            normalized.as_str(),
            "/start" | "start" | "start execution" | "/run" | "run tasks"
        )
    }

    pub fn is_planner_mode_command(message: &str) -> bool {
        message.trim().eq_ignore_ascii_case("/planner")
    }

    pub fn is_skip_plan_command(message: &str) -> bool {
        message.trim().eq_ignore_ascii_case("/skip-plan")
    }

    pub fn is_convert_command(message: &str) -> bool {
        message.trim().eq_ignore_ascii_case("/convert")
    }

    pub fn is_attach_docs_command(message: &str) -> bool {
        message.trim().eq_ignore_ascii_case("/attach-docs")
    }

    pub fn is_quit_command(message: &str) -> bool {
        let normalized = message.trim();
        normalized.eq_ignore_ascii_case("/quit") || normalized.eq_ignore_ascii_case("/exit")
    }

    pub fn is_new_master_command(message: &str) -> bool {
        message.trim().eq_ignore_ascii_case("/newmaster")
    }

    pub fn is_resume_command(message: &str) -> bool {
        message.trim().eq_ignore_ascii_case("/resume")
    }

    pub fn is_split_audits_command(message: &str) -> bool {
        message.trim().eq_ignore_ascii_case("/split-audits")
    }

    pub fn is_merge_audits_command(message: &str) -> bool {
        message.trim().eq_ignore_ascii_case("/merge-audits")
    }

    pub fn is_split_tests_command(message: &str) -> bool {
        message.trim().eq_ignore_ascii_case("/split-tests")
    }

    pub fn is_merge_tests_command(message: &str) -> bool {
        message.trim().eq_ignore_ascii_case("/merge-tests")
    }

    pub fn is_add_final_audit_command(message: &str) -> bool {
        message.trim().eq_ignore_ascii_case("/add-final-audit")
    }

    pub fn is_remove_final_audit_command(message: &str) -> bool {
        message.trim().eq_ignore_ascii_case("/remove-final-audit")
    }

    pub fn prepare_attach_docs_prompt(&self, tasks_file: &str) -> String {
        format!(
            "You are a docs-research sub-agent.\n\
             Goal: update the planner task file with implementation documentation links.\n\
             Read and edit this JSON file directly: {tasks_file}\n\
             Requirements:\n\
             - For every task/subtask where kind != \"test_runner\", populate or refresh a `docs` array.\n\
             - Each docs item must include: title, url, summary.\n\
             - Use the latest authoritative online docs relevant to implementing that task.\n\
             - Keep existing task structure/order/status intact; only add/update docs.\n\
             - Leave test_runner tasks with docs as-is (do not add docs there).\n\
             - Save tasks.json, then output a short confirmation summary."
        )
    }

    pub fn prepare_planner_prompt(
        &self,
        message: &str,
        planner_file: &str,
        project_info_file: &str,
    ) -> String {
        let context_entries = self.workflow.rolling_context_entries();
        let context_text = if context_entries.is_empty() {
            "(no rolling task context yet)".to_string()
        } else {
            context_entries
                .iter()
                .map(|entry| format!("- {entry}"))
                .collect::<Vec<_>>()
                .join("\n")
        };
        format!(
            "You are the master Codex agent in planner mode.\n\
             Goal: collaboratively build a codebase-aware implementation plan before task generation.\n\
             Planner storage:\n\
             - Read and update this markdown file directly: {planner_file}\n\
             - Do not edit tasks.json while planner mode is active.\n\
             Codebase context:\n\
             - Prefer grounding plan details using local repository files and this context brief: {project_info_file}\n\
             - Keep references concrete by naming likely files/modules when confident.\n\
             Clarification-first behavior:\n\
             - Do not generate or update planner markdown until you have asked follow-up questions that clarify scope, constraints, and success criteria.\n\
             - If key details are ambiguous, ask concise follow-up questions first and wait for answers before planning.\n\
             Plan formatting requirements:\n\
             - Break work down into concrete numbered steps.\n\
             - For every step, include self-contained sections for Implementation, Auditing, and Test Writing.\n\
             - Maintain readable markdown with sections and checklists.\n\
             - Track open questions/risks and assumptions.\n\
             - Keep it collaborative and iterative; update the markdown on each turn when the plan changes.\n\
             - When the plan is ready for execution, explicitly tell the user to run `/convert` to proceed to implementation.\n\
             Rolling task context:\n\
             {context_text}\n\
             User message:\n\
             {message}\n\
             After saving planner markdown updates, send a concise conversational summary of what changed and remind the user they can run `/convert` when ready to implement."
        )
    }

    pub fn start_execution(&mut self) -> Vec<String> {
        let messages = self.workflow.start_execution();
        self.prune_expanded_detail_keys();
        self.refresh_right_lines();
        messages
    }

    pub fn start_next_worker_job(&mut self) -> Option<StartedJob> {
        let started = self.workflow.start_next_job();
        if started.is_some() {
            self.prune_expanded_detail_keys();
            self.refresh_right_lines();
        }
        started
    }

    pub fn on_worker_output(&mut self, line: String) {
        if let Some(meta) = self.workflow.active_job_meta() {
            let role = match meta.role {
                WorkerRole::Implementor => "Impl",
                WorkerRole::Auditor => "Audit",
                WorkerRole::TestWriter => "Tests",
                WorkerRole::TestRunner => "TestRun",
                WorkerRole::FinalAudit => "FinalAudit",
            };
            self.append_left_top_line(format!("{role}#{}: {line}", meta.top_task_id));
        } else {
            self.append_left_top_line(format!("Worker: {line}"));
        }
        self.workflow.append_active_output(line);
    }

    pub fn on_worker_system_output(&mut self, line: String) {
        self.append_left_top_line(format!("WorkerSystem: {line}"));
    }

    pub fn push_subagent_output(&mut self, line: impl Into<String>) {
        self.append_left_top_line(line.into());
    }

    pub fn on_worker_completed(&mut self, success: bool, code: i32) -> Vec<String> {
        let messages = self.workflow.finish_active_job(success, code);
        for message in messages {
            self.chat_messages.push(message);
        }
        self.prune_expanded_detail_keys();
        self.refresh_right_lines();
        let context_entries = self.workflow.rolling_context_entries();
        let new_entries = if context_entries.len() > self.reported_context_entries {
            context_entries[self.reported_context_entries..].to_vec()
        } else {
            Vec::new()
        };
        self.reported_context_entries = context_entries.len();
        new_entries
    }

    pub fn drain_worker_failures(&mut self) -> Vec<WorkflowFailure> {
        self.workflow.drain_recent_failures()
    }

    pub fn set_chat_scroll(&mut self, scroll: u16) {
        self.chat_scroll = scroll;
    }

    pub fn left_top_lines(&self) -> &[String] {
        &self.left_top_lines
    }

    pub fn left_top_wrapped_text(&self, width: u16) -> Arc<String> {
        let width = width.max(1);
        if let Some(cache) = self.left_top_wrap_cache.borrow().as_ref()
            && cache.width == width
            && cache.generation == self.left_top_generation
        {
            return Arc::clone(&cache.rendered);
        }

        let rendered = Arc::new(wrap_word_with_positions(&self.left_top_lines.join("\n"), width).rendered);
        *self.left_top_wrap_cache.borrow_mut() = Some(WrappedPaneCache {
            width,
            generation: self.left_top_generation,
            rendered: Arc::clone(&rendered),
        });
        rendered
    }

    pub fn left_bottom_lines(&self) -> &[String] {
        &self.chat_messages
    }

    pub fn right_block_lines(&self, width: u16) -> Vec<String> {
        if self.right_pane_mode == RightPaneMode::PlannerMarkdown {
            self.planner_block_lines(width)
        } else {
            self.workflow
                .right_pane_block_view(width, &self.expanded_detail_keys)
                .lines
        }
    }

    pub fn right_block_view(&self, width: u16) -> RightPaneBlockView {
        if self.right_pane_mode == RightPaneMode::PlannerMarkdown {
            RightPaneBlockView {
                lines: self.planner_block_lines(width),
                toggles: Vec::new(),
            }
        } else {
            self.workflow
                .right_pane_block_view(width, &self.expanded_detail_keys)
        }
    }

    pub fn toggle_task_details(&mut self, task_key: &str) {
        if self.expanded_detail_keys.contains(task_key) {
            self.expanded_detail_keys.remove(task_key);
        } else {
            self.expanded_detail_keys.insert(task_key.to_string());
        }
    }

    fn prune_expanded_detail_keys(&mut self) {
        let valid = self.workflow.task_detail_keys();
        self.expanded_detail_keys.retain(|key| valid.contains(key));
    }

    pub fn left_top_scroll(&self) -> u16 {
        self.left_top_scroll
    }

    pub fn left_bottom_scroll(&self) -> u16 {
        self.chat_scroll
    }

    pub fn right_scroll(&self) -> u16 {
        self.right_scroll
    }

    pub fn chat_input(&self) -> &str {
        &self.chat_input
    }

    pub fn consume_chat_input_trimmed(&mut self) -> Option<String> {
        let message = self.chat_input.trim().to_string();
        self.chat_input.clear();
        self.chat_cursor = 0;
        self.chat_cursor_goal_col = None;
        if message.is_empty() {
            None
        } else {
            Some(message)
        }
    }

    pub fn chat_cursor_line_col(&self, width: u16) -> (u16, u16) {
        let positions = wrap_word_with_positions(&self.chat_input, width.max(1)).positions;
        positions[self.chat_cursor]
    }

    pub fn command_suggestions(&self) -> Vec<CommandSuggestion> {
        let Some(query) = command_query(&self.chat_input) else {
            return Vec::new();
        };
        COMMAND_INDEX
            .iter()
            .filter(|(command, _)| command.starts_with(query))
            .map(|(command, description)| CommandSuggestion {
                command,
                description,
            })
            .collect()
    }

    pub fn should_show_command_index(&self) -> bool {
        self.resume_picker.is_none() && !self.command_suggestions().is_empty()
    }

    pub fn autocomplete_top_command(&mut self) -> bool {
        if self.resume_picker.is_some() {
            return false;
        }
        let Some(top) = self.command_suggestions().first().copied() else {
            return false;
        };
        self.chat_input = top.command.to_string();
        self.chat_cursor = self.chat_input.chars().count();
        self.chat_cursor_goal_col = None;
        true
    }

    pub fn open_resume_picker(&mut self, entries: Vec<ResumeSessionOption>) {
        if entries.is_empty() {
            self.resume_picker = None;
        } else {
            self.resume_picker = Some(ResumePickerState {
                entries,
                selected: 0,
            });
        }
    }

    pub fn is_resume_picker_open(&self) -> bool {
        self.resume_picker.is_some()
    }

    pub fn set_task_check_in_progress(&mut self, in_progress: bool) {
        self.task_check_in_progress = in_progress;
    }

    pub fn is_task_check_in_progress(&self) -> bool {
        self.task_check_in_progress
    }

    pub fn set_docs_attach_in_progress(&mut self, in_progress: bool) {
        self.docs_attach_in_progress = in_progress;
    }

    pub fn is_docs_attach_in_progress(&self) -> bool {
        self.docs_attach_in_progress
    }

    pub fn set_master_in_progress(&mut self, in_progress: bool) {
        self.master_in_progress = in_progress;
    }

    pub fn is_master_in_progress(&self) -> bool {
        self.master_in_progress
    }

    pub fn resume_picker_options(&self) -> &[ResumeSessionOption] {
        match self.resume_picker.as_ref() {
            Some(state) => &state.entries,
            None => &[],
        }
    }

    pub fn resume_picker_selected_index(&self) -> usize {
        self.resume_picker
            .as_ref()
            .map(|state| state.selected)
            .unwrap_or(0)
    }

    pub fn resume_picker_move_up(&mut self) {
        let Some(state) = self.resume_picker.as_mut() else {
            return;
        };
        state.selected = state.selected.saturating_sub(1);
    }

    pub fn resume_picker_move_down(&mut self) {
        let Some(state) = self.resume_picker.as_mut() else {
            return;
        };
        if state.selected + 1 < state.entries.len() {
            state.selected += 1;
        }
    }

    pub fn select_resume_session(&mut self) -> Option<ResumeSessionOption> {
        let state = self.resume_picker.take()?;
        state.entries.get(state.selected).cloned()
    }

    pub fn replace_rolling_context_entries(&mut self, entries: Vec<String>) {
        self.workflow
            .replace_rolling_context_entries(entries.clone());
        self.reported_context_entries = entries.len();
        self.refresh_right_lines();
    }

    pub fn reset_execution_for_session_switch(&mut self) {
        self.workflow.reset_execution_runtime();
        self.refresh_right_lines();
    }

    pub fn is_planner_mode(&self) -> bool {
        self.right_pane_mode == RightPaneMode::PlannerMarkdown
    }

    pub fn right_pane_title(&self) -> &'static str {
        if self.is_planner_mode() {
            "Planner Markdown"
        } else {
            "Task List"
        }
    }

    pub fn set_right_pane_mode(&mut self, mode: RightPaneMode) {
        if self.right_pane_mode == mode {
            return;
        }
        self.right_pane_mode = mode;
        self.right_scroll = 0;
        self.refresh_right_lines();
    }

    pub fn set_planner_markdown(&mut self, markdown: String) {
        self.planner_markdown = markdown;
        self.refresh_right_lines();
    }

    fn planner_raw_lines(&self) -> Vec<String> {
        if self.planner_markdown.trim().is_empty() {
            return vec![
                "# Collaborative Planner".to_string(),
                String::new(),
                "The planner markdown is currently empty.".to_string(),
                "Chat with the agent in the left pane to build a codebase-aware plan.".to_string(),
                "Use /convert when you're ready to implement from the plan.".to_string(),
            ];
        }
        self.planner_markdown
            .lines()
            .map(|line| line.to_string())
            .collect()
    }

    fn planner_block_lines(&self, width: u16) -> Vec<String> {
        let width = width.max(1);
        let mut out = Vec::new();
        for line in self.planner_raw_lines() {
            let wrapped = wrap_word_with_positions(&line, width).rendered;
            out.extend(wrapped.lines().map(|part| part.to_string()));
        }
        if out.is_empty() {
            out.push(String::new());
        }
        out
    }

    fn refresh_right_lines(&mut self) {
        self.right_lines = if self.right_pane_mode == RightPaneMode::PlannerMarkdown {
            self.planner_raw_lines()
        } else {
            self.workflow.right_pane_lines()
        };
        let max = self.max_scroll(Pane::Right);
        self.right_scroll = self.right_scroll.min(max);
    }

    fn append_left_top_line(&mut self, line: String) {
        self.left_top_lines.push(line);
        if self.left_top_lines.len() > MAX_LEFT_TOP_LINES {
            let overflow = self.left_top_lines.len().saturating_sub(MAX_LEFT_TOP_LINES);
            self.left_top_lines.drain(0..overflow);
        }
        self.left_top_generation = self.left_top_generation.saturating_add(1);
        self.left_top_scroll = self.max_scroll(Pane::LeftTop);
    }

    fn scroll_mut(&mut self, pane: Pane) -> &mut u16 {
        match pane {
            Pane::LeftTop => &mut self.left_top_scroll,
            Pane::LeftBottom => &mut self.chat_scroll,
            Pane::Right => &mut self.right_scroll,
        }
    }

    fn max_scroll(&self, pane: Pane) -> u16 {
        let len = match pane {
            Pane::LeftTop => self.left_top_lines.len(),
            Pane::LeftBottom => self.chat_messages.len(),
            Pane::Right => self.right_lines.len(),
        };
        len.saturating_sub(1) as u16
    }
}

fn char_to_byte_idx(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(byte_idx, _)| byte_idx)
        .unwrap_or_else(|| s.len())
}

fn nearest_index_for_line_col(positions: &[(u16, u16)], target_line: u16, goal_col: u16) -> usize {
    let mut best: Option<(usize, u16)> = None;
    let mut fallback: Option<usize> = None;

    for (idx, (line, col)) in positions.iter().copied().enumerate() {
        if line != target_line {
            continue;
        }
        if fallback.is_none() {
            fallback = Some(idx);
        }
        if col <= goal_col {
            best = match best {
                Some((_, best_col)) if best_col >= col => best,
                _ => Some((idx, col)),
            };
        }
    }

    if let Some((idx, _)) = best {
        idx
    } else {
        fallback.unwrap_or(positions.len().saturating_sub(1))
    }
}

fn command_query(input: &str) -> Option<&str> {
    let trimmed = input.trim_start();
    if !trimmed.starts_with('/') {
        return None;
    }
    Some(trimmed.split_whitespace().next().unwrap_or(trimmed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_store::{PlannerTaskKindFile, PlannerTaskStatusFile};
    use std::sync::Arc;

    fn load_default_plan(app: &mut App, top_title: &str) {
        app.sync_planner_tasks_from_file(vec![
            PlannerTaskFileEntry {
                id: "top".to_string(),
                title: top_title.to_string(),
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
                title: "Tests".to_string(),
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

    #[test]
    fn default_state_is_running_with_expected_panes() {
        let app = App::default();
        assert!(app.running);
        assert_eq!(app.ticks, 0);
        assert_eq!(app.active_pane, Pane::LeftBottom);
        assert_eq!(app.left_top_lines().len(), 2);
        assert!(app.left_bottom_lines().is_empty());
        assert!(
            app.right_block_lines(80)
                .iter()
                .any(|line| line == "# Collaborative Planner")
        );
        assert!(app.is_planner_mode());
        assert_eq!(app.left_top_scroll(), 0);
        assert_eq!(app.left_bottom_scroll(), 0);
        assert_eq!(app.right_scroll(), 0);
        assert!(app.chat_input().is_empty());
        assert_eq!(app.chat_cursor_line_col(200), (0, 0));
        assert!(!app.is_task_check_in_progress());
        assert!(!app.is_docs_attach_in_progress());
        assert!(!app.is_master_in_progress());
    }

    #[test]
    fn task_check_modal_state_toggles() {
        let mut app = App::default();
        app.set_task_check_in_progress(true);
        assert!(app.is_task_check_in_progress());
        app.set_task_check_in_progress(false);
        assert!(!app.is_task_check_in_progress());
    }

    #[test]
    fn docs_attach_modal_state_toggles() {
        let mut app = App::default();
        app.set_docs_attach_in_progress(true);
        assert!(app.is_docs_attach_in_progress());
        app.set_docs_attach_in_progress(false);
        assert!(!app.is_docs_attach_in_progress());
    }

    #[test]
    fn master_modal_state_toggles() {
        let mut app = App::default();
        app.set_master_in_progress(true);
        assert!(app.is_master_in_progress());
        app.set_master_in_progress(false);
        assert!(!app.is_master_in_progress());
    }

    #[test]
    fn tick_and_quit_update_app_state() {
        let mut app = App::default();
        app.on_tick();
        app.on_tick();
        assert_eq!(app.ticks, 2);
        app.quit();
        assert!(!app.running);
    }

    #[test]
    fn pane_focus_cycles_forward_and_backward() {
        let mut app = App::default();
        app.next_pane();
        assert_eq!(app.active_pane, Pane::Right);
        app.next_pane();
        assert_eq!(app.active_pane, Pane::LeftTop);
        app.next_pane();
        assert_eq!(app.active_pane, Pane::LeftBottom);

        app.prev_pane();
        assert_eq!(app.active_pane, Pane::LeftTop);
        app.prev_pane();
        assert_eq!(app.active_pane, Pane::Right);
        app.prev_pane();
        assert_eq!(app.active_pane, Pane::LeftBottom);
    }

    #[test]
    fn scrolling_is_bounded_for_each_pane() {
        let mut app = App::default();
        app.active_pane = Pane::LeftTop;

        for _ in 0..500 {
            app.scroll_down();
        }
        assert_eq!(
            app.left_top_scroll(),
            app.left_top_lines().len().saturating_sub(1) as u16
        );
        app.scroll_up();
        assert_eq!(
            app.left_top_scroll(),
            app.left_top_lines().len().saturating_sub(2) as u16
        );
        for _ in 0..500 {
            app.scroll_up();
        }
        assert_eq!(app.left_top_scroll(), 0);

        app.active_pane = Pane::LeftBottom;
        for _ in 0..500 {
            app.scroll_down();
        }
        assert_eq!(
            app.left_bottom_scroll(),
            app.left_bottom_lines().len().saturating_sub(1) as u16
        );

        app.active_pane = Pane::Right;
        for _ in 0..500 {
            app.scroll_down();
        }
        let max_right_scroll = app.right_scroll();
        app.scroll_down();
        assert_eq!(app.right_scroll(), max_right_scroll);
    }

    #[test]
    fn chat_input_and_submit_flow() {
        let mut app = App::default();
        app.input_char('h');
        app.input_char('i');
        assert_eq!(app.chat_input(), "hi");
        assert_eq!(app.chat_cursor_line_col(200), (0, 2));
        app.backspace_input();
        assert_eq!(app.chat_input(), "h");
        assert_eq!(app.submit_chat_message(), Some("h".to_string()));

        assert!(app.chat_input().is_empty());
        assert_eq!(app.chat_cursor_line_col(200), (0, 0));
        assert_eq!(
            app.left_bottom_lines()
                .last()
                .expect("chat message expected"),
            "You: h"
        );
    }

    #[test]
    fn submit_ignores_whitespace_only_messages() {
        let mut app = App::default();
        app.input_char(' ');
        app.input_char(' ');
        assert_eq!(app.submit_chat_message(), None);
        assert!(app.left_bottom_lines().is_empty());
    }

    #[test]
    fn submit_direct_message_adds_user_chat_line() {
        let mut app = App::default();
        let out = app.submit_direct_message(" hello from file ");
        assert_eq!(out, Some("hello from file".to_string()));
        assert_eq!(
            app.left_bottom_lines().last().expect("chat line"),
            "You: hello from file"
        );
    }

    #[test]
    fn push_agent_message_appends_to_chat() {
        let mut app = App::default();
        app.push_agent_message("Codex: hello");
        assert_eq!(
            app.left_bottom_lines().last().expect("chat line expected"),
            "Codex: hello"
        );
    }

    #[test]
    fn queuing_task_updates_right_pane_and_prompts() {
        let mut app = App::default();
        app.set_right_pane_mode(RightPaneMode::TaskList);
        load_default_plan(&mut app, "Add feature Y");
        assert!(
            app.right_block_lines(80)
                .iter()
                .any(|line| line.contains("Add feature Y"))
        );

        let master_prompt = app.prepare_master_prompt("Add feature Y", "/tmp/tasks.json");
        assert!(master_prompt.contains("Rolling task context"));
        assert!(master_prompt.contains("Execution is currently disabled"));
        assert!(master_prompt.contains("/tmp/tasks.json"));
        assert!(master_prompt.contains("Never modify project workspace/source files directly."));
        assert!(
            master_prompt
                .contains("You may only edit files in the current meta-agent session directory")
        );
        assert!(master_prompt.contains("append new tasks; do not delete completed task history"));
        assert!(master_prompt.contains("`/start` is ready to run"));
        assert!(master_prompt.contains("`/start` always resumes from the last unfinished task"));
        assert!(master_prompt.contains("`docs` is reserved for `/attach-docs`"));
        assert!(master_prompt.contains("Testing-decision flow before initial planning"));
        assert!(master_prompt.contains("include test_runner under implementor"));
        assert!(
            master_prompt
                .contains("Every test_writer must be a direct child of the top-level task")
        );
        assert!(
            master_prompt.contains(
                "Create a dedicated testing-setup top-level task at the earliest position"
            )
        );
        assert!(master_prompt.contains(
            "updates session meta.json test_command to the exact bash-runnable command string"
        ));
        assert!(
            master_prompt
                .contains("Do not add non-setup test_writer or test_runner branches until after that setup task in task order")
        );
        assert!(
            master_prompt.contains("Implementor auditors must not include test-related checks")
        );

        assert!(app.start_next_worker_job().is_none());
        let messages = app.start_execution();
        assert!(
            messages
                .iter()
                .any(|line| line.contains("Execution enabled"))
        );
        let started = app.start_next_worker_job().expect("worker should start");
        assert_eq!(started.top_task_id, 1);
        assert_eq!(started.role, WorkerRole::Implementor);
    }

    #[test]
    fn planner_prompt_requires_clarification_and_convert_guidance() {
        let app = App::default();
        let prompt = app.prepare_planner_prompt(
            "Help me design this feature",
            "/tmp/session/planner.md",
            "/tmp/session/project-info.md",
        );
        assert!(prompt.contains("Do not generate or update planner markdown"));
        assert!(prompt.contains("ask concise follow-up questions first"));
        assert!(prompt.contains("Break work down into concrete numbered steps"));
        assert!(
            prompt.contains(
                "For every step, include self-contained sections for Implementation, Auditing, and Test Writing"
            )
        );
        assert!(prompt.contains("run `/convert` to proceed to implementation"));
    }

    #[test]
    fn worker_completion_updates_chat_and_tree() {
        let mut app = App::default();
        app.set_right_pane_mode(RightPaneMode::TaskList);
        load_default_plan(&mut app, "Ship fix");
        app.start_execution();
        let started = app.start_next_worker_job().expect("first job");
        assert_eq!(started.role, WorkerRole::Implementor);
        app.on_worker_output("Implemented change".to_string());
        let new_entries = app.on_worker_completed(true, 0);
        assert!(!new_entries.is_empty());

        let started = app.start_next_worker_job().expect("second job");
        assert_eq!(started.role, WorkerRole::TestWriter);
        app.on_worker_output("Added tests".to_string());
        let new_entries = app.on_worker_completed(true, 0);
        assert!(!new_entries.is_empty());

        let started = app.start_next_worker_job().expect("third job");
        assert_eq!(started.role, WorkerRole::Auditor);
        app.on_worker_output("No issues found".to_string());
        app.on_worker_completed(true, 0);

        let started = app.start_next_worker_job().expect("fourth job");
        assert_eq!(started.role, WorkerRole::TestRunner);
        app.on_worker_output("all tests passed".to_string());
        app.on_worker_completed(true, 0);
        assert!(app.start_next_worker_job().is_none());
        let tree = app.right_block_lines(80).join("\n");
        assert!(tree.contains("Ship fix"));
    }

    #[test]
    fn start_command_detection_handles_aliases() {
        assert!(App::is_start_execution_command("/start"));
        assert!(App::is_start_execution_command("start execution"));
        assert!(App::is_start_execution_command("/run"));
        assert!(!App::is_start_execution_command("please plan more"));
        assert!(App::is_attach_docs_command("/attach-docs"));
        assert!(!App::is_attach_docs_command("/start"));
        assert!(App::is_quit_command("/quit"));
        assert!(App::is_quit_command("/exit"));
        assert!(!App::is_quit_command("/start"));
        assert!(App::is_new_master_command("/newmaster"));
        assert!(!App::is_new_master_command("/start"));
        assert!(App::is_resume_command("/resume"));
        assert!(!App::is_resume_command("/start"));
        assert!(App::is_convert_command("/convert"));
        assert!(!App::is_convert_command("/start"));
        assert!(App::is_skip_plan_command("/skip-plan"));
        assert!(!App::is_skip_plan_command("/start"));
        assert!(App::is_split_audits_command("/split-audits"));
        assert!(App::is_merge_audits_command("/merge-audits"));
        assert!(!App::is_split_audits_command("/start"));
        assert!(!App::is_merge_audits_command("/start"));
        assert!(App::is_split_tests_command("/split-tests"));
        assert!(App::is_merge_tests_command("/merge-tests"));
        assert!(!App::is_split_tests_command("/start"));
        assert!(!App::is_merge_tests_command("/start"));
        assert!(App::is_add_final_audit_command("/add-final-audit"));
        assert!(App::is_remove_final_audit_command("/remove-final-audit"));
        assert!(!App::is_add_final_audit_command("/start"));
        assert!(!App::is_remove_final_audit_command("/start"));
    }

    #[test]
    fn worker_output_streams_to_top_left_pane() {
        let mut app = App::default();
        load_default_plan(&mut app, "Stream output");
        app.start_execution();
        let _ = app
            .start_next_worker_job()
            .expect("worker job should start");
        let before_chat_len = app.left_bottom_lines().len();
        app.on_worker_output("line from worker".to_string());
        app.on_worker_system_output("stderr line".to_string());

        assert_eq!(app.left_bottom_lines().len(), before_chat_len);
        assert!(
            app.left_top_lines()
                .iter()
                .any(|line| line.contains("line from worker"))
        );
        assert!(
            app.left_top_lines()
                .iter()
                .any(|line| line.contains("stderr line"))
        );
    }

    #[test]
    fn context_report_prompt_mentions_updates() {
        let app = App::default();
        let prompt = app.prepare_context_report_prompt(&[
            "Implementor completed pass.".to_string(),
            "Audit found no issues.".to_string(),
        ]);
        assert!(prompt.contains("Here's what just happened:"));
        assert!(prompt.contains("Do not make any file changes"));
        assert!(prompt.contains("Implementor completed pass."));
        assert!(prompt.contains("Audit found no issues."));
    }

    #[test]
    fn inserts_and_deletes_at_cursor_position() {
        let mut app = App::default();
        app.input_char('a');
        app.input_char('c');
        app.move_cursor_left();
        app.input_char('b');
        assert_eq!(app.chat_input(), "abc");
        assert_eq!(app.chat_cursor_line_col(200), (0, 2));
        app.backspace_input();
        assert_eq!(app.chat_input(), "ac");
        assert_eq!(app.chat_cursor_line_col(200), (0, 1));
    }

    #[test]
    fn cursor_moves_up_and_down_over_wrapped_lines() {
        let mut app = App::default();
        for c in "abcdefghij".chars() {
            app.input_char(c);
        }
        assert_eq!(app.chat_cursor_line_col(4), (2, 2));
        app.move_cursor_up(4);
        assert_eq!(app.chat_cursor_line_col(4), (1, 2));
        app.move_cursor_up(4);
        assert_eq!(app.chat_cursor_line_col(4), (0, 2));
        app.move_cursor_down(4);
        assert_eq!(app.chat_cursor_line_col(4), (1, 2));
    }

    #[test]
    fn chat_scroll_helpers_are_bounded() {
        let mut app = App::default();
        app.scroll_chat_up();
        assert_eq!(app.left_bottom_scroll(), 0);
        for _ in 0..200 {
            app.scroll_chat_down(2);
        }
        assert_eq!(app.left_bottom_scroll(), 2);
    }

    #[test]
    fn task_details_toggle_changes_block_rendering() {
        let mut app = App::default();
        app.set_right_pane_mode(RightPaneMode::TaskList);
        app.sync_planner_tasks_from_file(vec![
            PlannerTaskFileEntry {
                id: "task-a".to_string(),
                title: "Task A".to_string(),
                details: "Top detail should always show".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Task,
                status: PlannerTaskStatusFile::Pending,
                parent_id: None,
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "impl-a".to_string(),
                title: "Implementation".to_string(),
                details: "Longer detail text for preview".to_string(),
                docs: vec![crate::session_store::PlannerTaskDocFileEntry {
                    title: "Docs".to_string(),
                    url: "https://example.com/docs".to_string(),
                    summary: "Reference".to_string(),
                }],
                kind: PlannerTaskKindFile::Implementor,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("task-a".to_string()),
                order: Some(0),
            },
            PlannerTaskFileEntry {
                id: "impl-a-audit".to_string(),
                title: "Audit".to_string(),
                details: "Audit details".to_string(),
                docs: Vec::new(),
                kind: PlannerTaskKindFile::Auditor,
                status: PlannerTaskStatusFile::Pending,
                parent_id: Some("impl-a".to_string()),
                order: Some(0),
            },
        ])
        .expect("sync should succeed");

        let collapsed = app.right_block_lines(80).join("\n");
        assert!(collapsed.contains("[+]"));
        assert!(collapsed.contains("details [+]:"));
        assert!(collapsed.contains("..."));
        assert!(collapsed.contains("Task A"));
        assert!(collapsed.contains("details: Top detail should always show"));
        assert!(collapsed.contains("[documentation attached] [+]"));
        assert!(!collapsed.contains("https://example.com/docs"));
        assert!(!collapsed.contains("[ ] Task A"));

        app.toggle_task_details("impl-a");
        let expanded = app.right_block_lines(80).join("\n");
        assert!(expanded.contains("[-]"));
        assert!(expanded.contains("Longer detail text for preview"));

        app.toggle_task_details("docs:impl-a");
        let docs_expanded = app.right_block_lines(80).join("\n");
        assert!(docs_expanded.contains("[documentation attached] [-]"));
        assert!(docs_expanded.contains("https://example.com/docs"));
    }

    #[test]
    fn subagent_output_auto_follows_latest_lines() {
        let mut app = App::default();
        assert_eq!(app.left_top_scroll(), 0);
        app.push_subagent_output("line 1");
        assert_eq!(
            app.left_top_scroll(),
            app.left_top_lines().len().saturating_sub(1) as u16
        );
        app.on_worker_system_output("stderr".to_string());
        assert_eq!(
            app.left_top_scroll(),
            app.left_top_lines().len().saturating_sub(1) as u16
        );
    }

    #[test]
    fn left_top_output_is_capped_to_ring_buffer_limit() {
        let mut app = App::default();
        for idx in 0..(MAX_LEFT_TOP_LINES + 50) {
            app.push_subagent_output(format!("line {idx}"));
        }

        assert_eq!(app.left_top_lines().len(), MAX_LEFT_TOP_LINES);
        assert!(app.left_top_lines()[0].contains("line 50"));
        assert!(
            app.left_top_lines()
                .last()
                .is_some_and(|line| line.contains("line 2049"))
        );
    }

    #[test]
    fn left_top_wrap_cache_reuses_rendered_text_until_content_changes() {
        let mut app = App::default();
        let first = app.left_top_wrapped_text(40);
        let second = app.left_top_wrapped_text(40);
        assert!(Arc::ptr_eq(&first, &second));

        app.push_subagent_output("new cached line");
        let third = app.left_top_wrapped_text(40);
        assert!(!Arc::ptr_eq(&second, &third));
        assert!(third.contains("new cached line"));
    }

    #[test]
    fn command_index_filters_by_prefix() {
        let mut app = App::default();
        app.input_char('/');
        app.input_char('a');
        app.input_char('t');
        let suggestions = app.command_suggestions();
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].command, "/attach-docs");
        assert!(app.should_show_command_index());
    }

    #[test]
    fn command_index_matches_newmaster_prefix() {
        let mut app = App::default();
        app.input_char('/');
        app.input_char('n');
        let suggestions = app.command_suggestions();
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].command, "/newmaster");
    }

    #[test]
    fn command_index_matches_resume_prefix() {
        let mut app = App::default();
        app.input_char('/');
        app.input_char('r');
        app.input_char('e');
        app.input_char('s');
        let suggestions = app.command_suggestions();
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].command, "/resume");
    }

    #[test]
    fn command_index_matches_split_and_merge_prefixes() {
        let mut app = App::default();
        app.input_char('/');
        app.input_char('s');
        app.input_char('p');
        app.input_char('l');
        app.input_char('i');
        app.input_char('t');
        app.input_char('-');
        app.input_char('a');
        let suggestions = app.command_suggestions();
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].command, "/split-audits");

        let mut app = App::default();
        app.input_char('/');
        app.input_char('m');
        app.input_char('e');
        app.input_char('r');
        app.input_char('g');
        app.input_char('e');
        app.input_char('-');
        app.input_char('a');
        let suggestions = app.command_suggestions();
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].command, "/merge-audits");

        let mut app = App::default();
        app.input_char('/');
        app.input_char('s');
        app.input_char('p');
        app.input_char('l');
        app.input_char('i');
        app.input_char('t');
        app.input_char('-');
        app.input_char('t');
        let suggestions = app.command_suggestions();
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].command, "/split-tests");

        let mut app = App::default();
        app.input_char('/');
        app.input_char('m');
        app.input_char('e');
        app.input_char('r');
        app.input_char('g');
        app.input_char('e');
        app.input_char('-');
        app.input_char('t');
        let suggestions = app.command_suggestions();
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].command, "/merge-tests");

        let mut app = App::default();
        for ch in "/add-f".chars() {
            app.input_char(ch);
        }
        let suggestions = app.command_suggestions();
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].command, "/add-final-audit");

        let mut app = App::default();
        for ch in "/remove-f".chars() {
            app.input_char(ch);
        }
        let suggestions = app.command_suggestions();
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].command, "/remove-final-audit");
    }

    #[test]
    fn consume_chat_input_trimmed_clears_input_and_cursor() {
        let mut app = App::default();
        app.input_char(' ');
        app.input_char('/');
        app.input_char('x');
        app.input_char(' ');
        let consumed = app.consume_chat_input_trimmed();
        assert_eq!(consumed.as_deref(), Some("/x"));
        assert!(app.chat_input().is_empty());
        assert_eq!(app.chat_cursor_line_col(200), (0, 0));
    }

    #[test]
    fn command_index_tab_autocompletes_top_match() {
        let mut app = App::default();
        app.input_char('/');
        app.input_char('s');
        assert!(app.autocomplete_top_command());
        assert_eq!(app.chat_input(), "/start");
        assert_eq!(
            app.chat_cursor_line_col(200),
            (0, "/start".chars().count() as u16)
        );
    }

    #[test]
    fn resume_picker_navigation_and_selection_work() {
        let mut app = App::default();
        app.open_resume_picker(vec![
            ResumeSessionOption {
                session_dir: "/tmp/s1".to_string(),
                workspace: "/tmp/w1".to_string(),
                title: None,
                created_at_label: None,
                last_used_epoch_secs: 20,
            },
            ResumeSessionOption {
                session_dir: "/tmp/s2".to_string(),
                workspace: "/tmp/w2".to_string(),
                title: None,
                created_at_label: None,
                last_used_epoch_secs: 10,
            },
        ]);
        assert!(app.is_resume_picker_open());
        assert_eq!(app.resume_picker_selected_index(), 0);
        app.resume_picker_move_down();
        assert_eq!(app.resume_picker_selected_index(), 1);
        app.resume_picker_move_down();
        assert_eq!(app.resume_picker_selected_index(), 1);
        app.resume_picker_move_up();
        assert_eq!(app.resume_picker_selected_index(), 0);
        let selected = app.select_resume_session().expect("selection should exist");
        assert_eq!(selected.session_dir, "/tmp/s1");
        assert!(!app.is_resume_picker_open());
    }

    #[test]
    fn command_index_hides_while_resume_picker_open() {
        let mut app = App::default();
        app.input_char('/');
        assert!(app.should_show_command_index());
        app.open_resume_picker(vec![ResumeSessionOption {
            session_dir: "/tmp/s1".to_string(),
            workspace: "/tmp/w1".to_string(),
            title: None,
            created_at_label: None,
            last_used_epoch_secs: 1,
        }]);
        assert!(!app.should_show_command_index());
    }
}
