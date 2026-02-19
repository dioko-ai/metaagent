use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::Arc;

use crate::session_store::PlannerTaskFileEntry;
use crate::subagents;
use crate::text_layout::{WrappedText, wrap_word_with_positions};
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

#[derive(Debug, Clone)]
struct WrappedInputCache {
    width: u16,
    generation: u64,
    wrapped: Arc<WrappedText>,
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
    chat_messages_generation: u64,
    right_lines: Vec<String>,
    planner_markdown: String,
    planner_cursor: usize,
    planner_cursor_goal_col: Option<u16>,
    left_top_scroll: u16,
    chat_scroll: u16,
    right_scroll: u16,
    chat_input: String,
    chat_input_generation: u64,
    chat_input_wrap_cache: RefCell<Option<WrappedInputCache>>,
    chat_cursor: usize,
    chat_cursor_goal_col: Option<u16>,
    last_reported_context: Vec<String>,
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
            chat_messages_generation: 0,
            right_lines: vec![
                "# Collaborative Planner".to_string(),
                String::new(),
                "The planner markdown is currently empty.".to_string(),
                "Chat with the agent in the left pane to build a codebase-aware plan.".to_string(),
                "Use /convert when you're ready to implement from the plan.".to_string(),
            ],
            planner_markdown: String::new(),
            planner_cursor: 0,
            planner_cursor_goal_col: None,
            left_top_scroll: 0,
            chat_scroll: 0,
            right_scroll: 0,
            chat_input: String::new(),
            chat_input_generation: 0,
            chat_input_wrap_cache: RefCell::new(None),
            chat_cursor: 0,
            chat_cursor_goal_col: None,
            last_reported_context: Vec::new(),
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

    pub fn input_char(&mut self, c: char) {
        let byte_idx = char_to_byte_idx(&self.chat_input, self.chat_cursor);
        self.chat_input.insert(byte_idx, c);
        self.chat_cursor = self.chat_cursor.saturating_add(1);
        self.chat_cursor_goal_col = None;
        self.invalidate_chat_input_cache();
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
        self.invalidate_chat_input_cache();
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
        let positions = &self.wrapped_chat_input_layout(width).positions;
        let (line, col) = positions[self.chat_cursor];
        if line == 0 {
            return;
        }
        let goal_col = self.chat_cursor_goal_col.unwrap_or(col);
        self.chat_cursor = nearest_index_for_line_col(positions, line - 1, goal_col);
        self.chat_cursor_goal_col = Some(goal_col);
    }

    pub fn move_cursor_down(&mut self, width: u16) {
        let positions = &self.wrapped_chat_input_layout(width).positions;
        let (line, col) = positions[self.chat_cursor];
        let max_line = positions.iter().map(|(l, _)| *l).max().unwrap_or(0);
        if line >= max_line {
            return;
        }
        let goal_col = self.chat_cursor_goal_col.unwrap_or(col);
        self.chat_cursor = nearest_index_for_line_col(positions, line + 1, goal_col);
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

    pub fn scroll_left_top_down(&mut self, max_scroll: u16) {
        self.left_top_scroll = (self.left_top_scroll + 1).min(max_scroll);
    }

    pub fn submit_chat_message(&mut self) -> Option<String> {
        let message = self.chat_input.trim().to_string();
        if message.is_empty() {
            return None;
        }

        self.push_chat_message_line(format!("You: {message}"));
        self.chat_input.clear();
        self.chat_cursor = 0;
        self.chat_cursor_goal_col = None;
        self.invalidate_chat_input_cache();
        Some(message)
    }

    pub fn submit_direct_message(&mut self, raw_message: &str) -> Option<String> {
        let message = raw_message.trim().to_string();
        if message.is_empty() {
            return None;
        }
        self.push_chat_message_line(format!("You: {message}"));
        self.chat_input.clear();
        self.chat_cursor = 0;
        self.chat_cursor_goal_col = None;
        self.invalidate_chat_input_cache();
        Some(message)
    }

    pub fn push_agent_message(&mut self, message: impl Into<String>) {
        self.push_chat_message_line(message.into());
    }

    pub fn prepare_master_prompt(&self, message: &str, tasks_file: &str) -> String {
        subagents::build_master_prompt(tasks_file, &self.workflow.prepare_master_prompt(message))
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

    #[cfg(test)]
    pub fn is_execution_enabled(&self) -> bool {
        self.workflow.execution_enabled()
    }

    pub fn is_execution_busy(&self) -> bool {
        self.workflow.execution_busy()
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
            self.push_chat_message_line(message);
        }
        self.prune_expanded_detail_keys();
        self.refresh_right_lines();
        let context_entries = self.workflow.rolling_context_entries();
        let new_entries =
            new_context_entries_since_snapshot(&self.last_reported_context, &context_entries);
        self.last_reported_context = context_entries;
        new_entries
    }

    pub fn drain_worker_failures(&mut self) -> Vec<WorkflowFailure> {
        self.workflow.drain_recent_failures()
    }

    pub fn set_chat_scroll(&mut self, scroll: u16) {
        self.chat_scroll = scroll;
    }

    #[cfg(test)]
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

        let rendered =
            Arc::new(wrap_word_with_positions(&self.left_top_lines.join("\n"), width).rendered);
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

    pub fn wrapped_chat_input_layout(&self, width: u16) -> Arc<WrappedText> {
        let width = width.max(1);
        if let Some(cache) = self.chat_input_wrap_cache.borrow().as_ref()
            && cache.width == width
            && cache.generation == self.chat_input_generation
        {
            return Arc::clone(&cache.wrapped);
        }
        let wrapped = Arc::new(wrap_word_with_positions(&self.chat_input, width));
        *self.chat_input_wrap_cache.borrow_mut() = Some(WrappedInputCache {
            width,
            generation: self.chat_input_generation,
            wrapped: Arc::clone(&wrapped),
        });
        wrapped
    }

    pub fn chat_input_line_count(&self, width: u16) -> u16 {
        self.wrapped_chat_input_layout(width).line_count
    }

    pub fn chat_messages_generation(&self) -> u64 {
        self.chat_messages_generation
    }

    pub fn consume_chat_input_trimmed(&mut self) -> Option<String> {
        let message = self.chat_input.trim().to_string();
        self.chat_input.clear();
        self.chat_cursor = 0;
        self.chat_cursor_goal_col = None;
        self.invalidate_chat_input_cache();
        if message.is_empty() {
            None
        } else {
            Some(message)
        }
    }

    pub fn chat_cursor_line_col(&self, width: u16) -> (u16, u16) {
        let positions = &self.wrapped_chat_input_layout(width).positions;
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
        self.invalidate_chat_input_cache();
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

    pub fn is_any_agent_in_progress(&self) -> bool {
        self.master_in_progress
            || self.task_check_in_progress
            || self.docs_attach_in_progress
            || self.is_execution_busy()
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
        self.last_reported_context = entries;
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
        let max_cursor = markdown.chars().count();
        self.planner_markdown = markdown;
        self.planner_cursor = self.planner_cursor.min(max_cursor);
        self.planner_cursor_goal_col = None;
        self.refresh_right_lines();
    }

    pub fn planner_markdown(&self) -> &str {
        &self.planner_markdown
    }

    pub fn planner_input_char(&mut self, c: char) {
        let byte_idx = char_to_byte_idx(&self.planner_markdown, self.planner_cursor);
        self.planner_markdown.insert(byte_idx, c);
        self.planner_cursor = self.planner_cursor.saturating_add(1);
        self.planner_cursor_goal_col = None;
        self.refresh_right_lines();
    }

    pub fn planner_insert_newline(&mut self) {
        self.planner_input_char('\n');
    }

    pub fn planner_backspace(&mut self) {
        if self.planner_cursor == 0 {
            return;
        }
        let start = char_to_byte_idx(&self.planner_markdown, self.planner_cursor.saturating_sub(1));
        let end = char_to_byte_idx(&self.planner_markdown, self.planner_cursor);
        self.planner_markdown.drain(start..end);
        self.planner_cursor = self.planner_cursor.saturating_sub(1);
        self.planner_cursor_goal_col = None;
        self.refresh_right_lines();
    }

    pub fn planner_move_cursor_left(&mut self) {
        self.planner_cursor = self.planner_cursor.saturating_sub(1);
        self.planner_cursor_goal_col = None;
    }

    pub fn planner_move_cursor_right(&mut self) {
        let char_len = self.planner_markdown.chars().count();
        self.planner_cursor = (self.planner_cursor + 1).min(char_len);
        self.planner_cursor_goal_col = None;
    }

    pub fn planner_move_cursor_up(&mut self, width: u16) {
        let width = width.max(1);
        let positions = wrap_word_with_positions(&self.planner_markdown, width).positions;
        let (line, col) = positions[self.planner_cursor];
        if line == 0 {
            return;
        }
        let goal_col = self.planner_cursor_goal_col.unwrap_or(col);
        self.planner_cursor = nearest_index_for_line_col(&positions, line - 1, goal_col);
        self.planner_cursor_goal_col = Some(goal_col);
    }

    pub fn planner_move_cursor_down(&mut self, width: u16) {
        let width = width.max(1);
        let positions = wrap_word_with_positions(&self.planner_markdown, width).positions;
        let (line, col) = positions[self.planner_cursor];
        let max_line = positions.iter().map(|(l, _)| *l).max().unwrap_or(0);
        if line >= max_line {
            return;
        }
        let goal_col = self.planner_cursor_goal_col.unwrap_or(col);
        self.planner_cursor = nearest_index_for_line_col(&positions, line + 1, goal_col);
        self.planner_cursor_goal_col = Some(goal_col);
    }

    pub fn planner_cursor_line_col(&self, width: u16) -> (u16, u16) {
        let positions = wrap_word_with_positions(&self.planner_markdown, width.max(1)).positions;
        positions[self.planner_cursor]
    }

    pub fn planner_cursor_index_for_line_col(&self, width: u16, line: u16, col: u16) -> usize {
        let positions = wrap_word_with_positions(&self.planner_markdown, width.max(1)).positions;
        nearest_index_for_line_col(&positions, line, col)
    }

    pub fn set_planner_cursor(&mut self, cursor: usize) {
        let max_cursor = self.planner_markdown.chars().count();
        self.planner_cursor = cursor.min(max_cursor);
        self.planner_cursor_goal_col = None;
    }

    pub fn ensure_planner_cursor_visible(
        &mut self,
        width: u16,
        visible_lines: u16,
        max_scroll: u16,
    ) {
        let visible_lines = visible_lines.max(1);
        let (line, _) = self.planner_cursor_line_col(width.max(1));
        if line < self.right_scroll {
            self.right_scroll = line;
        } else {
            let visible_bottom = self
                .right_scroll
                .saturating_add(visible_lines.saturating_sub(1));
            if line > visible_bottom {
                self.right_scroll = line.saturating_sub(visible_lines.saturating_sub(1));
            }
        }
        self.right_scroll = self.right_scroll.min(max_scroll);
    }

    pub fn has_planner_markdown(&self) -> bool {
        !self.planner_markdown.trim().is_empty()
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

    fn push_chat_message_line(&mut self, message: String) {
        self.chat_messages.push(message);
        self.chat_messages_generation = self.chat_messages_generation.saturating_add(1);
    }

    fn invalidate_chat_input_cache(&mut self) {
        self.chat_input_generation = self.chat_input_generation.saturating_add(1);
        *self.chat_input_wrap_cache.borrow_mut() = None;
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

fn new_context_entries_since_snapshot(previous: &[String], current: &[String]) -> Vec<String> {
    let max_overlap = previous.len().min(current.len());
    let overlap = (0..=max_overlap)
        .rev()
        .find(|shared| {
            let left_start = previous.len().saturating_sub(*shared);
            previous[left_start..] == current[..*shared]
        })
        .unwrap_or(0);
    current[overlap..].to_vec()
}

#[cfg(test)]
#[path = "../tests/unit/app_tests.rs"]
mod tests;
