use super::*;
use crate::agent::BackendKind;
use crate::app::RightPaneMode;
use crate::session_store::{PlannerTaskFileEntry, PlannerTaskKindFile, PlannerTaskStatusFile};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;

fn render_text(app: &App, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal should initialize");
    let theme = Theme::default();
    terminal
        .draw(|frame| render(frame, app, &theme))
        .expect("render should succeed");
    buffer_to_string(terminal.backend().buffer())
}

fn buffer_to_string(buffer: &Buffer) -> String {
    let mut text = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            text.push_str(buffer[(x, y)].symbol());
        }
        text.push('\n');
    }
    text
}

fn seed_execution_plan(app: &mut App) {
    app.sync_planner_tasks_from_file(vec![
        PlannerTaskFileEntry {
            id: "top-1".to_string(),
            title: "Top task".to_string(),
            details: "top details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Task,
            status: PlannerTaskStatusFile::Pending,
            parent_id: None,
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl-1".to_string(),
            title: "Implementation".to_string(),
            details: "implementor details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Implementor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("top-1".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl-1-audit".to_string(),
            title: "Audit".to_string(),
            details: "audit details".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Auditor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("impl-1".to_string()),
            order: Some(0),
        },
    ])
    .expect("seed plan should sync");
}

#[test]
fn render_shows_three_panes_and_help_text() {
    let app = App::default();
    let text = render_text(&app, 120, 30);
    assert!(text.contains("Worker Output"));
    assert!(text.contains("Agent Chat"));
    assert!(text.contains("Planner Markdown"));
    assert!(!text.contains("quit"));
    assert!(text.contains("Ctrl+U/Ctrl+D"));
    assert!(text.contains("PgUp/PgDn"));
    assert!(text.contains("Wheel scrolls focused pane"));
}

#[test]
fn render_shows_chat_header_working_indicator_when_master_busy() {
    let mut app = App::default();
    app.set_master_in_progress(true);
    let text = render_text(&app, 120, 30);
    assert!(text.contains("Agent Chat | Working ["));
    assert!(!text.contains("Master working"));
}

#[test]
fn render_shows_chat_header_working_indicator_when_worker_execution_busy() {
    let mut app = App::default();
    seed_execution_plan(&mut app);
    app.start_execution();
    assert!(app.is_execution_busy());
    let text = render_text(&app, 120, 30);
    assert!(text.contains("Agent Chat | Working ["));
    assert!(!text.contains("Master working"));
}

#[test]
fn master_working_dots_animate_over_ticks() {
    let first = master_working_dots(0);
    let second = master_working_dots(2);
    let third = master_working_dots(4);
    assert_ne!(first, second);
    assert_ne!(second, third);
}

#[test]
fn render_does_not_use_active_text_labels() {
    let mut app = App::default();
    app.active_pane = Pane::LeftBottom;
    let text = render_text(&app, 120, 30);
    assert!(text.contains("Agent Chat"));
    assert!(!text.contains(" active"));
}

#[test]
fn render_shows_command_index_when_input_starts_with_slash() {
    let mut app = App::default();
    app.active_pane = Pane::LeftBottom;
    app.input_char('/');
    let text = render_text(&app, 120, 30);
    assert!(text.contains("/start"));
    assert!(text.contains("/convert"));
    assert!(text.contains("/quit"));
}

#[test]
fn render_shows_resume_picker_overlay_when_open() {
    let mut app = App::default();
    app.open_resume_picker(vec![
        crate::app::ResumeSessionOption {
            session_dir: "/tmp/session-a".to_string(),
            workspace: "/tmp/work-a".to_string(),
            title: Some("Session A".to_string()),
            created_at_label: Some("2026-02-16T12:00:00Z".to_string()),
            last_used_epoch_secs: 100,
        },
        crate::app::ResumeSessionOption {
            session_dir: "/tmp/session-b".to_string(),
            workspace: "/tmp/work-b".to_string(),
            title: None,
            created_at_label: None,
            last_used_epoch_secs: 90,
        },
    ]);
    let text = render_text(&app, 120, 30);
    assert!(text.contains("Resume Session"));
    assert!(text.contains("Session A"));
}

#[test]
fn render_shows_backend_picker_overlay_when_open() {
    let mut app = App::default();
    app.open_backend_picker(vec![
        crate::app::BackendOption {
            kind: BackendKind::Codex,
            label: "Codex CLI",
            description: "Run codex locally",
        },
        crate::app::BackendOption {
            kind: BackendKind::Claude,
            label: "Claude CLI",
            description: "Run claude locally",
        },
    ]);

    let text = render_text(&app, 120, 30);
    assert!(text.contains("Select Backend"));
    assert!(text.contains("Codex CLI"));
    assert!(text.contains("(Up/Down select, Enter/Space choose)"));
}

#[test]
fn render_backend_picker_overlay_tracks_selected_option() {
    let mut app = App::default();
    app.open_backend_picker(vec![
        crate::app::BackendOption {
            kind: BackendKind::Codex,
            label: "Codex CLI",
            description: "Run codex locally",
        },
        crate::app::BackendOption {
            kind: BackendKind::Claude,
            label: "Claude CLI",
            description: "Run claude locally",
        },
    ]);

    let initial = render_text(&app, 120, 30);
    assert!(initial.contains("> Codex CLI"));

    app.backend_picker_move_down();
    let after_move = render_text(&app, 120, 30);
    assert!(!after_move.contains("> Codex CLI"));
    assert!(after_move.contains("> Claude CLI"));
}

#[test]
fn render_backend_picker_overlay_replaces_resume_picker_overlay() {
    let mut app = App::default();
    app.open_resume_picker(vec![crate::app::ResumeSessionOption {
        session_dir: "/tmp/session-a".to_string(),
        workspace: "/tmp/work-a".to_string(),
        title: Some("Session A".to_string()),
        created_at_label: Some("2026-02-16T12:00:00Z".to_string()),
        last_used_epoch_secs: 100,
    }]);
    app.open_backend_picker(vec![crate::app::BackendOption {
        kind: BackendKind::Codex,
        label: "Codex CLI",
        description: "Run codex locally",
    }]);

    let text = render_text(&app, 120, 30);
    assert!(text.contains("Select Backend"));
    assert!(!text.contains("Resume Session"));
    assert!(!text.contains("Session A"));
}

#[test]
fn render_shows_task_check_overlay_when_running() {
    let mut app = App::default();
    app.set_task_check_in_progress(true);
    let text = render_text(&app, 120, 30);
    assert!(text.contains("Checking Tasks..."));
    assert!(text.contains("Agent Chat | Working ["));
}

#[test]
fn render_shows_docs_attach_overlay_when_running() {
    let mut app = App::default();
    app.set_docs_attach_in_progress(true);
    let text = render_text(&app, 120, 30);
    assert!(text.contains("Attaching Documentation..."));
    assert!(text.contains("Agent Chat | Working ["));
}

#[test]
fn chat_render_shows_separators_and_agent_prefix() {
    let messages = vec!["You: hello".to_string(), "Codex: hi there".to_string()];
    let lines = chat_display_lines(&messages, 40);
    let text = chat_text(&lines, &Theme::default()).to_string();
    assert!(text.contains("You: hello"));
    assert!(text.contains("Agent: hi there"));
    assert!(text.contains("────────"));
}

#[test]
fn chat_separators_use_subtle_color_near_chat_background() {
    let messages = vec!["You: hello".to_string(), "Agent: hi".to_string()];
    let lines = chat_display_lines(&messages, 20);
    let theme = Theme::default();
    let text = chat_text(&lines, &theme);
    let sep = text
        .lines
        .iter()
        .find(|line| line.spans.len() == 1 && line.spans[0].content.as_ref().contains("─"))
        .expect("separator line should exist");
    assert_eq!(sep.spans[0].style.fg, Some(chat_separator_color(&theme)));
}

#[test]
fn system_messages_are_dim_and_muted_for_prefix_and_body() {
    let lines = chat_display_lines(&["System: hello".to_string()], 40);
    let theme = Theme::default();
    let text = chat_text(&lines, &theme);
    let line = &text.lines[0];
    assert_eq!(line.spans[0].style.fg, Some(theme.muted_fg));
    assert!(line.spans[0].style.add_modifier.contains(Modifier::DIM));
    assert_eq!(line.spans[2].style.fg, Some(theme.muted_fg));
    assert!(line.spans[2].style.add_modifier.contains(Modifier::DIM));
}

#[test]
fn shared_word_wrap_grows_with_text_length() {
    assert_eq!(wrap_word_with_positions("", 10).line_count, 1);
    assert_eq!(wrap_word_with_positions("abcd", 10).line_count, 1);
    assert_eq!(wrap_word_with_positions("abcdefghijk", 5).line_count, 3);
}

#[test]
fn input_box_metrics_caps_at_five_lines_and_scrolls_after() {
    let (height, scroll) = input_box_metrics(3, 2, 20);
    assert_eq!(height, 5);
    assert_eq!(scroll, 0);

    let (height, scroll) = input_box_metrics(5, 0, 20);
    assert_eq!(height, 7);
    assert_eq!(scroll, 0);

    let (height, scroll) = input_box_metrics(8, 6, 20);
    assert_eq!(height, 7);
    assert_eq!(scroll, 3);
}

#[test]
fn input_box_metrics_respects_small_available_height() {
    let (height, scroll) = input_box_metrics(10, 9, 4);
    assert_eq!(height, 4);
    assert_eq!(scroll, 8);
}

#[test]
fn chat_input_width_is_nonzero() {
    assert!(chat_input_text_width(Rect::new(0, 0, 20, 10)) >= 1);
}

#[test]
fn chat_max_scroll_increases_with_more_messages() {
    let mut app = App::default();
    let screen = Rect::new(0, 0, 120, 30);
    let baseline = chat_max_scroll(screen, &app);
    for c in "hello".chars() {
        app.input_char(c);
    }
    assert_eq!(app.submit_chat_message(), Some("hello".to_string()));
    assert!(chat_max_scroll(screen, &app) >= baseline);
}

#[test]
fn left_top_max_scroll_increases_with_wrapped_output() {
    let mut app = App::default();
    let screen = Rect::new(0, 0, 120, 30);
    let baseline = left_top_max_scroll(screen, &app);
    for _ in 0..40 {
        app.push_subagent_output(
            "wrapped worker output line with enough content to span multiple visual lines",
        );
    }
    assert!(left_top_max_scroll(screen, &app) > baseline);
}

#[test]
fn wraps_by_word_without_cutting_words() {
    assert_eq!(
        wrap_word_with_positions("hello world", 6).rendered,
        "hello \nworld"
    );
}

#[test]
fn title_bar_bg_changes_by_active_state() {
    assert_eq!(
        title_bar_bg(Color::Rgb(40, 40, 40), false),
        Color::Rgb(28, 28, 28)
    );
    assert_eq!(title_bar_bg(Color::Rgb(40, 40, 40), true), ACTIVE_TITLE_BG);
}

#[test]
fn active_title_foreground_is_black_for_contrast() {
    assert_eq!(ACTIVE_TITLE_FG, Color::Black);
}

#[test]
fn right_pane_text_styles_top_task_titles_bright_white_only() {
    let lines = vec![
        "  1. Top task".to_string(),
        "".to_string(),
        "  ─────".to_string(),
        "  ┌────┐".to_string(),
        "Execution".to_string(),
        "Another heading".to_string(),
    ];
    let text = right_pane_text(&lines);
    assert_eq!(text.lines[0].spans[0].style.fg, Some(Color::White));
    assert_eq!(text.lines[2].spans[0].style.fg, None);
    assert_eq!(text.lines[3].spans[0].style.fg, None);
    assert_eq!(text.lines[5].spans[0].style.fg, None);
}

#[test]
fn right_pane_text_styles_in_progress_tasks_orange() {
    let lines = vec!["[~] Active task".to_string(), "Execution".to_string()];
    let text = right_pane_text(&lines);
    assert_eq!(
        text.lines[0].spans[0].style.fg,
        Some(Color::Rgb(230, 150, 60))
    );
}

#[test]
fn right_pane_in_progress_keeps_box_borders_unstyled() {
    let lines = vec!["│ │ [~] Impl task │ │".to_string(), "Execution".to_string()];
    let text = right_pane_text(&lines);
    let line = &text.lines[0];
    let orange = Some(Color::Rgb(230, 150, 60));
    assert!(
        line.spans
            .iter()
            .any(|span| span.style.fg == orange && !span.content.as_ref().contains('│'))
    );
    assert!(
        line.spans
            .iter()
            .any(|span| span.content.as_ref().contains('│') && span.style.fg.is_none())
    );
}

#[test]
fn right_pane_text_styles_done_tasks_gray_with_green_x() {
    let lines = vec!["│ [x] Done task │".to_string(), "Execution".to_string()];
    let text = right_pane_text(&lines);
    let done_line = &text.lines[0];
    assert_eq!(done_line.spans.len(), 5);
    assert_eq!(done_line.spans[0].content.as_ref(), "│");
    assert_eq!(done_line.spans[0].style.fg, None);
    assert_eq!(done_line.spans[2].content.as_ref(), "x");
    assert_eq!(done_line.spans[2].style.fg, Some(Color::Rgb(80, 190, 100)));
    assert_eq!(done_line.spans[1].style.fg, Some(Color::Rgb(145, 145, 145)));
    assert!(
        done_line.spans[1]
            .style
            .add_modifier
            .contains(Modifier::DIM)
    );
    assert_eq!(done_line.spans[3].style.fg, Some(Color::Rgb(145, 145, 145)));
    assert!(
        done_line.spans[3]
            .style
            .add_modifier
            .contains(Modifier::DIM)
    );
    assert_eq!(done_line.spans[4].content.as_ref(), "│");
    assert_eq!(done_line.spans[4].style.fg, None);
}

#[test]
fn right_pane_text_styles_details_lines_dim_gray() {
    let lines = vec![
        "│ details [-]: first part │".to_string(),
        "│            second part │".to_string(),
        "Execution".to_string(),
    ];
    let text = right_pane_text(&lines);
    assert_eq!(text.lines[0].spans[0].content.as_ref(), "│");
    assert_eq!(text.lines[0].spans[0].style.fg, None);
    assert_eq!(
        text.lines[0].spans[1].style.fg,
        Some(Color::Rgb(145, 145, 145))
    );
    assert!(
        text.lines[0].spans[1]
            .style
            .add_modifier
            .contains(Modifier::DIM)
    );
    assert_eq!(text.lines[0].spans[2].content.as_ref(), "│");
    assert_eq!(text.lines[0].spans[2].style.fg, None);
    assert_eq!(
        text.lines[1].spans[1].style.fg,
        Some(Color::Rgb(145, 145, 145))
    );
    assert!(
        text.lines[1].spans[1]
            .style
            .add_modifier
            .contains(Modifier::DIM)
    );
}

#[test]
fn right_pane_text_styles_documentation_badge_green() {
    let lines = vec![
        "│ [documentation attached] │".to_string(),
        "Execution".to_string(),
    ];
    let text = right_pane_text(&lines);
    assert_eq!(
        text.lines[0].spans[1].style.fg,
        Some(Color::Rgb(80, 190, 100))
    );
}

#[test]
fn system_chat_wrapped_continuation_stays_dim_gray() {
    let messages = vec!["System: this line should wrap across multiple pieces".to_string()];
    let lines = chat_display_lines(&messages, 18);
    assert!(
        lines
            .iter()
            .any(|l| l.prefix == Some(ChatPrefix::System) && !l.show_label)
    );
    let theme = Theme::default();
    let text = chat_text(&lines, &theme);
    let mut found_continued = false;
    for line in &text.lines {
        if line.spans.len() == 2 && line.spans[0].content.as_ref().starts_with("       ") {
            found_continued = true;
            assert_eq!(line.spans[1].style.fg, Some(theme.muted_fg));
            assert!(line.spans[1].style.add_modifier.contains(Modifier::DIM));
        }
    }
    assert!(found_continued);
}

#[test]
fn right_pane_toggle_hit_test_returns_task_key_on_marker_click() {
    let mut app = App::default();
    app.set_right_pane_mode(RightPaneMode::TaskList);
    app.sync_planner_tasks_from_file(vec![
        PlannerTaskFileEntry {
            id: "task-1".to_string(),
            title: "Task One".to_string(),
            details: "Detail text".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Task,
            status: PlannerTaskStatusFile::Pending,
            parent_id: None,
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl-1".to_string(),
            title: "Implementation".to_string(),
            details: "Impl detail text".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Implementor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("task-1".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl-1-audit".to_string(),
            title: "Audit".to_string(),
            details: "Audit detail text".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Auditor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("impl-1".to_string()),
            order: Some(0),
        },
    ])
    .expect("sync should succeed");

    let screen = Rect::new(0, 0, 120, 30);
    let [body, _status] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(STATUS_HEIGHT)]).areas(screen);
    let [_left, right] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(body);
    let [_title_area, content_area] =
        Layout::vertical([Constraint::Length(TITLE_BAR_HEIGHT), Constraint::Min(0)]).areas(right);
    let inner_width = content_area.width.saturating_sub(TEXT_PADDING * 2).max(1);
    let view = app.right_block_view(inner_width);
    let detail_line_index = view
        .lines
        .iter()
        .position(|line| line.contains("[+]"))
        .expect("collapsed marker should exist");
    let detail_line = &view.lines[detail_line_index];
    let marker_idx = detail_line
        .find("[+]")
        .expect("collapsed marker should exist") as u16;
    let x = content_area.x + TEXT_PADDING + marker_idx + 1;
    let y = content_area.y + TEXT_PADDING + detail_line_index as u16;
    let key = right_pane_toggle_hit_test(screen, &app, x, y);
    assert_eq!(key.as_deref(), Some("impl-1"));
}

#[test]
fn right_pane_toggle_hit_test_returns_docs_toggle_key_on_docs_marker_click() {
    let mut app = App::default();
    app.set_right_pane_mode(RightPaneMode::TaskList);
    app.sync_planner_tasks_from_file(vec![
        PlannerTaskFileEntry {
            id: "task-1".to_string(),
            title: "Task One".to_string(),
            details: "Detail text".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Task,
            status: PlannerTaskStatusFile::Pending,
            parent_id: None,
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl-1".to_string(),
            title: "Implementation".to_string(),
            details: "Impl detail text".to_string(),
            docs: vec![crate::session_store::PlannerTaskDocFileEntry {
                title: "Doc".to_string(),
                url: "https://example.com/doc".to_string(),
                summary: "Summary".to_string(),
            }],
            kind: PlannerTaskKindFile::Implementor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("task-1".to_string()),
            order: Some(0),
        },
        PlannerTaskFileEntry {
            id: "impl-1-audit".to_string(),
            title: "Audit".to_string(),
            details: "Audit detail text".to_string(),
            docs: Vec::new(),
            kind: PlannerTaskKindFile::Auditor,
            status: PlannerTaskStatusFile::Pending,
            parent_id: Some("impl-1".to_string()),
            order: Some(0),
        },
    ])
    .expect("sync should succeed");

    let screen = Rect::new(0, 0, 120, 30);
    let [body, _status] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(STATUS_HEIGHT)]).areas(screen);
    let [_left, right] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(body);
    let [_title_area, content_area] =
        Layout::vertical([Constraint::Length(TITLE_BAR_HEIGHT), Constraint::Min(0)]).areas(right);
    let inner_width = content_area.width.saturating_sub(TEXT_PADDING * 2).max(1);
    let view = app.right_block_view(inner_width);
    let docs_line_index = view
        .lines
        .iter()
        .position(|line| line.contains("[documentation attached]"))
        .expect("docs line should exist");
    let docs_line = &view.lines[docs_line_index];
    let marker_idx = docs_line.find("[+]").expect("docs marker should exist") as u16;
    let x = content_area.x + TEXT_PADDING + marker_idx + 1;
    let y = content_area.y + TEXT_PADDING + docs_line_index as u16;
    let key = right_pane_toggle_hit_test(screen, &app, x, y);
    assert_eq!(key.as_deref(), Some("docs:impl-1"));
}

#[test]
fn pane_hit_test_identifies_each_pane() {
    let screen = Rect::new(0, 0, 120, 30);
    let [body, _status] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(STATUS_HEIGHT)]).areas(screen);
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(body);
    let [left_top, left_bottom] =
        Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(left);

    let lt_x = left_top.x + left_top.width.saturating_sub(1).min(1);
    let lt_y = left_top.y + left_top.height.saturating_sub(1).min(1);
    assert_eq!(pane_hit_test(screen, lt_x, lt_y), Some(Pane::LeftTop));

    let lb_x = left_bottom.x + left_bottom.width.saturating_sub(1).min(1);
    let lb_y = left_bottom.y + left_bottom.height.saturating_sub(1).min(1);
    assert_eq!(pane_hit_test(screen, lb_x, lb_y), Some(Pane::LeftBottom));

    let right_x = right.x + right.width.saturating_sub(1).min(1);
    let right_y = right.y + right.height.saturating_sub(1).min(1);
    assert_eq!(pane_hit_test(screen, right_x, right_y), Some(Pane::Right));
}

#[test]
fn pane_hit_test_ignores_status_area() {
    let screen = Rect::new(0, 0, 120, 30);
    let [body, status] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(STATUS_HEIGHT)]).areas(screen);
    assert!(body.height > 0);
    assert!(status.height > 0);
    assert_eq!(pane_hit_test(screen, status.x, status.y), None);
}
