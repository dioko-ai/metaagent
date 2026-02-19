use super::*;
use crate::agent::BackendKind;
use crate::session_store::{PlannerTaskKindFile, PlannerTaskStatusFile};
use crate::text_layout::wrap_word_with_positions;
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
fn execution_enabled_flag_updates_on_start() {
    let mut app = App::default();
    assert!(!app.is_execution_enabled());
    app.start_execution();
    assert!(app.is_execution_enabled());
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
    let max_left_top_scroll = app.left_top_lines().len().saturating_sub(1) as u16;

    for _ in 0..500 {
        app.scroll_left_top_down(max_left_top_scroll);
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
    let max_left_bottom_scroll = app.left_bottom_lines().len().saturating_sub(1) as u16;

    for _ in 0..500 {
        app.scroll_chat_down(max_left_bottom_scroll);
    }
    assert_eq!(
        app.left_bottom_scroll(),
        app.left_bottom_lines().len().saturating_sub(1) as u16
    );

    app.active_pane = Pane::Right;
    let max_right_scroll = app.right_block_lines(80).len().saturating_sub(1) as u16;
    for _ in 0..500 {
        app.scroll_right_down(max_right_scroll);
    }
    app.scroll_right_down(max_right_scroll);
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
fn chat_input_layout_cache_reuses_and_invalidates() {
    let mut app = App::default();
    for ch in "hello world".chars() {
        app.input_char(ch);
    }

    let cached = app.wrapped_chat_input_layout(5);
    let reused = app.wrapped_chat_input_layout(5);
    assert!(Arc::ptr_eq(&cached, &reused));

    app.move_cursor_left();
    let still_cached = app.wrapped_chat_input_layout(5);
    assert!(Arc::ptr_eq(&cached, &still_cached));

    app.input_char('!');
    let refreshed = app.wrapped_chat_input_layout(5);
    assert!(!Arc::ptr_eq(&cached, &refreshed));
    assert_eq!(
        refreshed.rendered,
        wrap_word_with_positions(app.chat_input(), 5).rendered
    );
}

#[test]
fn chat_message_generation_tracks_new_chat_lines() {
    let mut app = App::default();
    assert_eq!(app.chat_messages_generation(), 0);
    app.push_agent_message("Agent: one");
    assert_eq!(app.chat_messages_generation(), 1);
    let _ = app.submit_direct_message("hello");
    assert_eq!(app.chat_messages_generation(), 2);
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
        master_prompt.contains("Every test_writer must be a direct child of the top-level task")
    );
    assert!(
        master_prompt
            .contains("Create a dedicated testing-setup top-level task at the earliest position")
    );
    assert!(master_prompt.contains(
        "updates session meta.json test_command to the exact bash-runnable command string"
    ));
    assert!(
            master_prompt
                .contains("Do not add non-setup test_writer or test_runner branches until after that setup task in task order")
        );
    assert!(master_prompt.contains("Implementor auditors must not include test-related checks"));

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
    assert_eq!(started.role, WorkerRole::Auditor);
    app.on_worker_output("AUDIT_RESULT: PASS".to_string());
    app.on_worker_output("No issues found".to_string());
    let new_entries = app.on_worker_completed(true, 0);
    assert!(!new_entries.is_empty());

    let started = app.start_next_worker_job().expect("third job");
    assert_eq!(started.role, WorkerRole::TestWriter);
    app.on_worker_output("Added tests".to_string());
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
fn worker_completion_reports_new_context_when_rolling_context_is_at_capacity() {
    let mut app = App::default();
    app.set_right_pane_mode(RightPaneMode::TaskList);
    load_default_plan(&mut app, "Ship fix");
    app.replace_rolling_context_entries((0..16).map(|idx| format!("existing-{idx}")).collect());

    app.start_execution();
    let started = app.start_next_worker_job().expect("first job");
    assert_eq!(started.role, WorkerRole::Implementor);
    app.on_worker_output("Implemented change".to_string());
    let new_entries = app.on_worker_completed(true, 0);

    assert_eq!(
        new_entries.len(),
        1,
        "expected one newly appended context entry"
    );
    assert!(new_entries[0].contains("Implementor worked on \"Ship fix\""));
    let rolling = app.rolling_context_entries();
    assert_eq!(rolling.len(), 16, "rolling context should remain capped");
    assert_eq!(rolling.last(), Some(&new_entries[0]));
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
fn command_index_matches_backend_prefix() {
    let mut app = App::default();
    app.input_char('/');
    app.input_char('b');
    let suggestions = app.command_suggestions();
    assert_eq!(suggestions.len(), 1);
    assert_eq!(suggestions[0].command, "/backend");
    assert_eq!(suggestions[0].description, "Choose backend");
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
fn command_index_tab_autocompletes_backend_command() {
    let mut app = App::default();
    app.input_char('/');
    app.input_char('b');
    assert!(app.autocomplete_top_command());
    assert_eq!(app.chat_input(), "/backend");
    assert_eq!(
        app.chat_cursor_line_col(200),
        (0, "/backend".chars().count() as u16)
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
fn backend_picker_navigation_and_selection_work() {
    let mut app = App::default();
    app.open_backend_picker(vec![
        BackendOption {
            kind: BackendKind::Codex,
            label: "Codex",
            description: "Codex backend",
        },
        BackendOption {
            kind: BackendKind::Claude,
            label: "Claude",
            description: "Claude backend",
        },
    ]);

    assert!(app.is_backend_picker_open());
    assert_eq!(app.backend_picker_options().len(), 2);
    assert_eq!(app.backend_picker_selected_index(), 0);

    app.backend_picker_move_down();
    assert_eq!(app.backend_picker_selected_index(), 1);
    app.backend_picker_move_down();
    assert_eq!(app.backend_picker_selected_index(), 1);
    app.backend_picker_move_up();
    assert_eq!(app.backend_picker_selected_index(), 0);

    let selected = app.select_backend_option().expect("selection should exist");
    assert_eq!(selected.kind, BackendKind::Codex);
    assert!(!app.is_backend_picker_open());
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

#[test]
fn command_index_hides_while_backend_picker_open() {
    let mut app = App::default();
    app.input_char('/');
    assert!(app.should_show_command_index());
    app.open_backend_picker(vec![BackendOption {
        kind: BackendKind::Codex,
        label: "Codex",
        description: "Codex backend",
    }]);
    assert!(!app.should_show_command_index());
    assert!(!app.autocomplete_top_command());
}

#[test]
fn backend_and_resume_pickers_remain_mutually_exclusive() {
    let mut app = App::default();
    app.open_resume_picker(vec![ResumeSessionOption {
        session_dir: "/tmp/s1".to_string(),
        workspace: "/tmp/w1".to_string(),
        title: None,
        created_at_label: None,
        last_used_epoch_secs: 1,
    }]);
    assert!(app.is_resume_picker_open());
    assert!(!app.is_backend_picker_open());

    app.open_backend_picker(vec![BackendOption {
        kind: BackendKind::Codex,
        label: "Codex",
        description: "Codex backend",
    }]);
    assert!(app.is_backend_picker_open());
    assert!(!app.is_resume_picker_open());

    app.open_resume_picker(vec![ResumeSessionOption {
        session_dir: "/tmp/s2".to_string(),
        workspace: "/tmp/w2".to_string(),
        title: None,
        created_at_label: None,
        last_used_epoch_secs: 2,
    }]);
    assert!(app.is_resume_picker_open());
    assert!(!app.is_backend_picker_open());
}
