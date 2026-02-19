use std::cell::RefCell;
use std::path::Path;
use std::sync::Arc;

use ratatui::prelude::*;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Clear, Padding, Paragraph};
use ratatui_core::layout::Alignment as CoreAlignment;
use ratatui_core::style::{Color as CoreColor, Modifier as CoreModifier, Style as CoreStyle};
use ratatui_core::text::{Line as CoreLine, Span as CoreSpan, Text as CoreText};
use tui_markdown::from_str;

use crate::app::{App, CommandSuggestion, Pane};
use crate::text_layout::wrap_word_with_positions;
use crate::theme::Theme;

const MAX_INPUT_TEXT_LINES: u16 = 5;
const TEXT_PADDING: u16 = 1;
const STATUS_HEIGHT: u16 = 3;
const TITLE_BAR_HEIGHT: u16 = 3;
const ACTIVE_TITLE_BG: Color = Color::Rgb(90, 145, 200);
const ACTIVE_TITLE_FG: Color = Color::Black;
const STATUS_HELP_TEXT: &str = "Tab/Shift+Tab focus | Ctrl+U/Ctrl+D or PgUp/PgDn scroll main right pane | Wheel scrolls focused pane";

#[derive(Debug, Clone)]
struct ChatLinesCache {
    width: u16,
    generation: u64,
    lines: Arc<Vec<ChatDisplayLine>>,
}

thread_local! {
    static CHAT_LINES_CACHE: RefCell<Option<ChatLinesCache>> = const { RefCell::new(None) };
}

pub fn chat_input_text_width(screen: Rect) -> u16 {
    let [body, _status] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(STATUS_HEIGHT)]).areas(screen);
    let [left, _right] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(body);
    let [_left_top, left_bottom] =
        Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(left);
    let [_title_bar, content] =
        Layout::vertical([Constraint::Length(TITLE_BAR_HEIGHT), Constraint::Min(0)])
            .areas(left_bottom);
    content.width.saturating_sub(TEXT_PADDING * 2).max(1)
}

pub fn chat_max_scroll(screen: Rect, app: &App) -> u16 {
    let [body, _status] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(STATUS_HEIGHT)]).areas(screen);
    let [left, _right] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(body);
    let [_left_top, left_bottom] =
        Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(left);
    let [_title_bar, content] =
        Layout::vertical([Constraint::Length(TITLE_BAR_HEIGHT), Constraint::Min(0)])
            .areas(left_bottom);

    if content.width < 1 || content.height < 2 {
        return 0;
    }

    let input_text_width = content.width.saturating_sub(TEXT_PADDING * 2).max(1);
    let input_text_lines = app.chat_input_line_count(input_text_width);
    let max_input_height = content.height.saturating_sub(1).max(1);
    let (input_height, _) = input_box_metrics(input_text_lines, 0, max_input_height);
    let [messages_area, _input_area] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(input_height)]).areas(content);

    let visible_message_lines = messages_area.height.saturating_sub(TEXT_PADDING * 2);
    let total_message_lines = cached_chat_display_lines(
        app,
        content.width.saturating_sub(TEXT_PADDING * 2).max(1),
    )
    .len() as u16;
    total_message_lines.saturating_sub(visible_message_lines)
}

pub fn left_top_max_scroll(screen: Rect, app: &App) -> u16 {
    let [body, _status] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(STATUS_HEIGHT)]).areas(screen);
    let [left, _right] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(body);
    let [left_top, _left_bottom] =
        Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(left);
    let [_title_bar, content] =
        Layout::vertical([Constraint::Length(TITLE_BAR_HEIGHT), Constraint::Min(0)])
            .areas(left_top);
    if content.width < 1 || content.height < 1 {
        return 0;
    }
    let text_width = content.width.saturating_sub(TEXT_PADDING * 2).max(1);
    let total_lines = app.left_top_wrapped_text(text_width).lines().count() as u16;
    let visible_lines = content.height.saturating_sub(TEXT_PADDING * 2);
    total_lines.saturating_sub(visible_lines)
}

pub fn right_max_scroll(screen: Rect, app: &App) -> u16 {
    let [body, _status] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(STATUS_HEIGHT)]).areas(screen);
    let [_left, right] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(body);
    let [_title_bar, content] =
        Layout::vertical([Constraint::Length(TITLE_BAR_HEIGHT), Constraint::Min(0)]).areas(right);
    if content.width < 1 || content.height < 1 {
        return 0;
    }
    let text_width = content.width.saturating_sub(TEXT_PADDING * 2).max(1);
    let total_lines = if app.is_planner_mode() && app.active_pane == Pane::Right {
        wrap_word_with_positions(app.planner_markdown(), text_width).line_count
    } else {
        app.right_block_lines(text_width).len() as u16
    };
    let visible_lines = content.height.saturating_sub(TEXT_PADDING * 2);
    total_lines.saturating_sub(visible_lines)
}

pub fn pane_hit_test(screen: Rect, x: u16, y: u16) -> Option<Pane> {
    let [body, _status] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(STATUS_HEIGHT)]).areas(screen);
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(body);
    let [left_top, left_bottom] =
        Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(left);

    if point_in_rect(left_top, x, y) {
        return Some(Pane::LeftTop);
    }
    if point_in_rect(left_bottom, x, y) {
        return Some(Pane::LeftBottom);
    }
    if point_in_rect(right, x, y) {
        return Some(Pane::Right);
    }
    None
}

pub fn render(frame: &mut Frame, app: &App, theme: &Theme) {
    let [body, status] = Layout::vertical([Constraint::Min(0), Constraint::Length(STATUS_HEIGHT)])
        .areas(frame.area());
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(body);
    let [left_top, left_bottom] =
        Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(left);

    render_worker_output_pane(
        frame,
        left_top,
        app.active_pane == Pane::LeftTop,
        app,
        theme,
    );
    render_chat_pane(
        frame,
        left_bottom,
        app,
        app.active_pane == Pane::LeftBottom,
        theme,
    );
    render_right_task_pane(frame, right, app, app.active_pane == Pane::Right, theme);
    if app.is_docs_attach_in_progress() {
        render_center_overlay(frame, right, "Attaching Documentation...");
    }
    if app.is_task_check_in_progress() {
        render_center_overlay(frame, right, "Checking Tasks...");
    }

    frame.render_widget(
        Block::default().style(Style::default().bg(theme.status_bg)),
        status,
    );
    let help = Paragraph::new(status_line_text())
        .style(Style::default().bg(theme.status_bg).fg(theme.muted_fg))
        .block(
            Block::default()
                .style(Style::default().bg(theme.status_bg))
                .padding(Padding::uniform(TEXT_PADDING)),
        );
    frame.render_widget(help, status);

    if app.is_resume_picker_open() {
        render_resume_picker(frame, app, theme);
    }
}

fn render_worker_output_pane(
    frame: &mut Frame,
    area: Rect,
    active: bool,
    app: &App,
    theme: &Theme,
) {
    let [title_area, content_area] =
        Layout::vertical([Constraint::Length(TITLE_BAR_HEIGHT), Constraint::Min(0)]).areas(area);
    let title_bg = title_bar_bg(theme.left_top_bg, active);
    let header_fg = if active {
        ACTIVE_TITLE_FG
    } else {
        theme.muted_fg
    };

    frame.render_widget(
        Block::default().style(Style::default().bg(title_bg)),
        title_area,
    );
    frame.render_widget(
        Paragraph::new("Worker Output")
            .style(Style::default().bg(title_bg).fg(header_fg))
            .block(
                Block::default()
                    .style(Style::default().bg(title_bg))
                    .padding(Padding::uniform(TEXT_PADDING)),
            ),
        title_area,
    );

    let width = content_area.width.saturating_sub(TEXT_PADDING * 2).max(1);
    let content = app.left_top_wrapped_text(width);
    frame.render_widget(
        Paragraph::new(content.as_str())
            .style(Style::default().bg(theme.left_top_bg).fg(theme.text_fg))
            .scroll((app.left_top_scroll(), 0))
            .block(
                Block::default()
                    .style(Style::default().bg(theme.left_top_bg))
                    .padding(Padding::uniform(TEXT_PADDING)),
            ),
        content_area,
    );
}

fn status_line_text() -> String {
    STATUS_HELP_TEXT.to_string()
}

fn master_working_dots(ticks: u64) -> &'static str {
    const FRAMES: [&str; 6] = ["[   ]", "[.  ]", "[.. ]", "[...]", "[ ..]", "[  .]"];
    FRAMES[((ticks / 2) as usize) % FRAMES.len()]
}

fn render_center_overlay(frame: &mut Frame, right_area: Rect, text: &str) {
    let [_, content] = Layout::vertical([Constraint::Length(TITLE_BAR_HEIGHT), Constraint::Min(0)])
        .areas(right_area);
    let width = 32u16.min(content.width.saturating_sub(2)).max(20);
    let height = 3u16;
    let x = content.x + (content.width.saturating_sub(width)) / 2;
    let y = content.y + (content.height.saturating_sub(height)) / 2;
    let overlay = Rect::new(x, y, width, height);
    frame.render_widget(Clear, overlay);
    frame.render_widget(
        Paragraph::new(text)
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::Rgb(255, 165, 0)))
            .block(
                Block::default()
                    .style(Style::default().bg(Color::Rgb(20, 20, 20)))
                    .padding(Padding::uniform(1)),
            ),
        overlay,
    );
}

fn render_chat_pane(frame: &mut Frame, area: Rect, app: &App, active: bool, theme: &Theme) {
    let [title_area, content] =
        Layout::vertical([Constraint::Length(TITLE_BAR_HEIGHT), Constraint::Min(0)]).areas(area);
    let title_bg = title_bar_bg(theme.chat_bg, active);
    let title_fg = if active {
        ACTIVE_TITLE_FG
    } else {
        theme.muted_fg
    };
    frame.render_widget(
        Block::default().style(Style::default().bg(title_bg)),
        title_area,
    );
    frame.render_widget(
        Paragraph::new(chat_title_text(app))
            .style(Style::default().bg(title_bg).fg(title_fg))
            .block(
                Block::default()
                    .style(Style::default().bg(title_bg))
                    .padding(Padding::uniform(TEXT_PADDING)),
            ),
        title_area,
    );

    frame.render_widget(
        Block::default().style(Style::default().bg(theme.chat_bg)),
        content,
    );
    if content.width < 1 || content.height < 2 {
        return;
    }

    let input_text_width = content.width.saturating_sub(TEXT_PADDING * 2).max(1);
    let wrapped_input_layout = app.wrapped_chat_input_layout(input_text_width);
    let input_text_lines = wrapped_input_layout.line_count;
    let (cursor_line, cursor_col) = app.chat_cursor_line_col(input_text_width.max(1));
    let max_input_height = content.height.saturating_sub(1).max(1);
    let (input_height, input_scroll) =
        input_box_metrics(input_text_lines, cursor_line, max_input_height);

    let [messages_area, input_area] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(input_height)]).areas(content);

    let message_lines =
        cached_chat_display_lines(app, messages_area.width.saturating_sub(TEXT_PADDING * 2).max(1));
    let message_text = chat_text(message_lines.as_ref(), theme);
    let messages = Paragraph::new(message_text)
        .scroll((
            app.left_bottom_scroll()
                .min(chat_max_scroll(frame.area(), app)),
            0,
        ))
        .style(Style::default().bg(theme.chat_bg).fg(theme.text_fg))
        .block(
            Block::default()
                .style(Style::default().bg(theme.chat_bg))
                .padding(Padding::uniform(TEXT_PADDING)),
        );
    frame.render_widget(messages, messages_area);

    let input = Paragraph::new(wrapped_input_layout.rendered.as_str())
        .block(
            Block::default()
                .style(Style::default().bg(theme.input_bg))
                .padding(Padding::uniform(TEXT_PADDING)),
        )
        .style(Style::default().bg(theme.input_bg).fg(theme.text_fg))
        .scroll((input_scroll, 0));
    frame.render_widget(input, input_area);
    if app.should_show_command_index() {
        render_command_index(
            frame,
            app.command_suggestions(),
            messages_area,
            input_area,
            theme,
        );
    }

    if active {
        let input_inner = input_area.inner(Margin {
            horizontal: TEXT_PADDING,
            vertical: TEXT_PADDING,
        });
        if input_inner.width > 0 && input_inner.height > 0 {
            let visible_cursor_line = cursor_line.saturating_sub(input_scroll);
            if visible_cursor_line < input_inner.height {
                frame.set_cursor_position((
                    input_inner
                        .x
                        .saturating_add(cursor_col.min(input_inner.width.saturating_sub(1))),
                    input_inner.y.saturating_add(visible_cursor_line),
                ));
            }
        }
    }
}

fn render_command_index(
    frame: &mut Frame,
    suggestions: Vec<CommandSuggestion>,
    messages_area: Rect,
    input_area: Rect,
    theme: &Theme,
) {
    if suggestions.is_empty() || messages_area.height == 0 || input_area.width == 0 {
        return;
    }
    let max_items = messages_area.height.saturating_sub(2).max(1) as usize;
    let shown = suggestions.into_iter().take(max_items).collect::<Vec<_>>();
    let overlay_height = (shown.len() as u16)
        .saturating_add(2)
        .min(messages_area.height.max(1));
    let y = input_area
        .y
        .saturating_sub(overlay_height)
        .max(messages_area.y);
    let overlay = Rect::new(input_area.x, y, input_area.width, overlay_height);

    let mut lines = Vec::with_capacity(shown.len() + 1);
    for (idx, item) in shown.iter().enumerate() {
        let style = if idx == 0 {
            Style::default().fg(theme.active_fg)
        } else {
            Style::default().fg(theme.text_fg)
        };
        lines.push(Line::from(vec![
            Span::styled(item.command.to_string(), style),
            Span::raw(" "),
            Span::styled(
                item.description.to_string(),
                Style::default().fg(theme.muted_fg),
            ),
        ]));
    }

    frame.render_widget(Clear, overlay);
    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::default().bg(theme.input_bg))
            .block(
                Block::default()
                    .style(Style::default().bg(theme.input_bg))
                    .padding(Padding::uniform(TEXT_PADDING)),
            ),
        overlay,
    );
}

fn chat_title_text(app: &App) -> String {
    if app.is_any_agent_in_progress() {
        format!("Agent Chat | Working {}", master_working_dots(app.ticks))
    } else {
        "Agent Chat".to_string()
    }
}

fn render_resume_picker(frame: &mut Frame, app: &App, theme: &Theme) {
    let entries = app.resume_picker_options();
    if entries.is_empty() {
        return;
    }

    let width = frame.area().width.min(90).max(40);
    let max_rows = frame.area().height.saturating_sub(8).max(3);
    let shown_count = (entries.len() as u16).min(max_rows.saturating_sub(2).max(1));
    let height = shown_count
        .saturating_add(4)
        .min(frame.area().height.max(3));
    let x = frame
        .area()
        .x
        .saturating_add(frame.area().width.saturating_sub(width) / 2);
    let y = frame
        .area()
        .y
        .saturating_add(frame.area().height.saturating_sub(height) / 2);
    let overlay = Rect::new(x, y, width, height);

    let start = app
        .resume_picker_selected_index()
        .saturating_sub((shown_count as usize).saturating_sub(1));
    let shown = entries
        .iter()
        .skip(start)
        .take(shown_count as usize)
        .collect::<Vec<_>>();

    let mut lines = Vec::with_capacity(shown.len() + 1);
    lines.push(Line::from(vec![
        Span::styled(
            "Resume Session",
            Style::default()
                .fg(theme.active_fg)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            "(Up/Down select, Enter/Space resume)",
            Style::default().fg(theme.muted_fg),
        ),
    ]));
    for (idx, item) in shown.iter().enumerate() {
        let absolute_idx = start + idx;
        let selected = absolute_idx == app.resume_picker_selected_index();
        let style = if selected {
            Style::default()
                .fg(theme.active_fg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.text_fg)
        };
        let name = Path::new(&item.session_dir)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&item.session_dir);
        let display_title = item.title.as_deref().unwrap_or(name);
        let when = item.created_at_label.as_deref().unwrap_or("unknown date");
        lines.push(Line::from(vec![
            Span::styled(
                if selected { ">" } else { " " }.to_string(),
                Style::default().fg(theme.muted_fg),
            ),
            Span::raw(" "),
            Span::styled(display_title.to_string(), style),
            Span::raw(" "),
            Span::styled(
                format!("({when} | {})", item.workspace),
                Style::default().fg(theme.muted_fg),
            ),
        ]));
    }

    frame.render_widget(Clear, overlay);
    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::default().bg(theme.input_bg))
            .block(
                Block::default()
                    .style(Style::default().bg(theme.input_bg))
                    .padding(Padding::uniform(TEXT_PADDING)),
            ),
        overlay,
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChatPrefix {
    You,
    Agent,
    System,
}

#[derive(Debug, Clone)]
struct ChatDisplayLine {
    prefix: Option<ChatPrefix>,
    body: String,
    show_label: bool,
    is_separator: bool,
}

fn cached_chat_display_lines(app: &App, width: u16) -> Arc<Vec<ChatDisplayLine>> {
    let width = width.max(1);
    let generation = app.chat_messages_generation();
    CHAT_LINES_CACHE.with(|cache_cell| {
        if let Some(cache) = cache_cell.borrow().as_ref()
            && cache.width == width
            && cache.generation == generation
        {
            return Arc::clone(&cache.lines);
        }

        let lines = Arc::new(chat_display_lines(app.left_bottom_lines(), width));
        *cache_cell.borrow_mut() = Some(ChatLinesCache {
            width,
            generation,
            lines: Arc::clone(&lines),
        });
        lines
    })
}

fn chat_display_lines(messages: &[String], width: u16) -> Vec<ChatDisplayLine> {
    let width = width.max(1);
    let mut out = Vec::new();
    for (idx, message) in messages.iter().enumerate() {
        let (prefix, body) = parse_chat_prefix_and_body(message);
        if let Some(prefix) = prefix {
            let label = match prefix {
                ChatPrefix::You => "You:",
                ChatPrefix::Agent => "Agent:",
                ChatPrefix::System => "System:",
            };
            let prefix_width = label.chars().count() + 1;
            let body_width = (width as usize).saturating_sub(prefix_width).max(1) as u16;
            let wrapped = wrap_text_lines(body, body_width);
            if let Some((first, rest)) = wrapped.split_first() {
                out.push(ChatDisplayLine {
                    prefix: Some(prefix),
                    body: first.clone(),
                    show_label: true,
                    is_separator: false,
                });
                for line in rest {
                    out.push(ChatDisplayLine {
                        prefix: Some(prefix),
                        body: line.clone(),
                        show_label: false,
                        is_separator: false,
                    });
                }
            }
        } else {
            for line in wrap_text_lines(body, width) {
                out.push(ChatDisplayLine {
                    prefix: None,
                    body: line,
                    show_label: false,
                    is_separator: false,
                });
            }
        }

        if idx + 1 < messages.len() {
            out.push(ChatDisplayLine {
                prefix: None,
                body: "─".repeat(width as usize),
                show_label: false,
                is_separator: true,
            });
        }
    }
    out
}

fn chat_text(lines: &[ChatDisplayLine], theme: &Theme) -> Text<'static> {
    let mut out_lines = Vec::with_capacity(lines.len());
    for line in lines {
        if line.is_separator {
            out_lines.push(Line::from(Span::styled(
                line.body.clone(),
                Style::default().fg(chat_separator_color(theme)),
            )));
            continue;
        }
        if let Some(prefix) = line.prefix {
            let (label, label_style, body_style) = match prefix {
                ChatPrefix::You => (
                    "You:",
                    Style::default().fg(Color::Rgb(80, 190, 100)),
                    Style::default(),
                ),
                ChatPrefix::Agent => (
                    "Agent:",
                    Style::default().fg(Color::Rgb(230, 150, 60)),
                    Style::default(),
                ),
                ChatPrefix::System => {
                    let style = Style::default()
                        .fg(theme.muted_fg)
                        .add_modifier(Modifier::DIM);
                    ("System:", style, style)
                }
            };
            if line.show_label {
                out_lines.push(Line::from(vec![
                    Span::styled(label.to_string(), label_style),
                    Span::raw(" "),
                    Span::styled(line.body.clone(), body_style),
                ]));
            } else {
                out_lines.push(Line::from(vec![
                    Span::raw(" ".repeat(label.chars().count() + 1)),
                    Span::styled(line.body.clone(), body_style),
                ]));
            }
        } else {
            out_lines.push(Line::from(Span::raw(line.body.clone())));
        }
    }
    Text::from(out_lines)
}

fn chat_separator_color(theme: &Theme) -> Color {
    match theme.chat_bg {
        Color::Rgb(r, g, b) => Color::Rgb(
            r.saturating_add(12),
            g.saturating_add(12),
            b.saturating_add(12),
        ),
        _ => theme.muted_fg,
    }
}

fn parse_chat_prefix_and_body(message: &str) -> (Option<ChatPrefix>, &str) {
    if let Some(rest) = message.strip_prefix("You:") {
        return (Some(ChatPrefix::You), rest.trim_start());
    }
    if let Some(rest) = message.strip_prefix("Agent:") {
        return (Some(ChatPrefix::Agent), rest.trim_start());
    }
    if let Some(rest) = message.strip_prefix("Codex:") {
        return (Some(ChatPrefix::Agent), rest.trim_start());
    }
    if let Some(rest) = message.strip_prefix("System:") {
        return (Some(ChatPrefix::System), rest.trim_start());
    }
    (None, message)
}

fn wrap_text_lines(text: &str, width: u16) -> Vec<String> {
    let rendered = wrap_word_with_positions(text, width.max(1)).rendered;
    let mut lines = rendered
        .split('\n')
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn right_pane_layout(screen: Rect) -> [Rect; 2] {
    let [body, _status] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(STATUS_HEIGHT)]).areas(screen);
    let [_left, right] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(body);
    Layout::vertical([Constraint::Length(TITLE_BAR_HEIGHT), Constraint::Min(0)]).areas(right)
}

fn render_right_task_pane(frame: &mut Frame, area: Rect, app: &App, active: bool, theme: &Theme) {
    let [title_area, content_area] =
        Layout::vertical([Constraint::Length(TITLE_BAR_HEIGHT), Constraint::Min(0)]).areas(area);
    let title_bg = title_bar_bg(theme.right_bg, active);
    let title_fg = if active {
        ACTIVE_TITLE_FG
    } else {
        theme.muted_fg
    };

    frame.render_widget(
        Block::default().style(Style::default().bg(title_bg)),
        title_area,
    );
    frame.render_widget(
        Paragraph::new(app.right_pane_title())
            .style(Style::default().bg(title_bg).fg(title_fg))
            .block(
                Block::default()
                    .style(Style::default().bg(title_bg))
                    .padding(Padding::uniform(TEXT_PADDING)),
            ),
        title_area,
    );

    let inner_width = content_area.width.saturating_sub(TEXT_PADDING * 2).max(1);
    let view = app.right_block_view(inner_width);
    if app.is_planner_mode() {
        if active {
            let wrapped = wrap_word_with_positions(app.planner_markdown(), inner_width);
            frame.render_widget(
                Paragraph::new(wrapped.rendered)
                    .style(Style::default().bg(theme.right_bg).fg(theme.text_fg))
                    .scroll((app.right_scroll(), 0))
                    .block(
                        Block::default()
                            .style(Style::default().bg(theme.right_bg))
                            .padding(Padding::uniform(TEXT_PADDING)),
                    ),
                content_area,
            );
            let input_inner = content_area.inner(Margin {
                horizontal: TEXT_PADDING,
                vertical: TEXT_PADDING,
            });
            if input_inner.width > 0 && input_inner.height > 0 {
                let (cursor_line, cursor_col) = app.planner_cursor_line_col(inner_width);
                let visible_cursor_line = cursor_line.saturating_sub(app.right_scroll());
                if visible_cursor_line < input_inner.height {
                    frame.set_cursor_position((
                        input_inner
                            .x
                            .saturating_add(cursor_col.min(input_inner.width.saturating_sub(1))),
                        input_inner.y.saturating_add(visible_cursor_line),
                    ));
                }
            }
        } else {
            let right_text = if app.has_planner_markdown() {
                planner_markdown_text(app.planner_markdown())
            } else {
                Text::from(view.lines.join("\n"))
            };
            frame.render_widget(
                Paragraph::new(right_text)
                    .style(Style::default().bg(theme.right_bg).fg(theme.text_fg))
                    .scroll((app.right_scroll(), 0))
                    .block(
                        Block::default()
                            .style(Style::default().bg(theme.right_bg))
                            .padding(Padding::uniform(TEXT_PADDING)),
                    ),
                content_area,
            );
        }
    } else {
        frame.render_widget(
            Paragraph::new(right_pane_text(&view.lines))
                .style(Style::default().bg(theme.right_bg).fg(theme.text_fg))
                .scroll((app.right_scroll(), 0))
                .block(
                    Block::default()
                        .style(Style::default().bg(theme.right_bg))
                        .padding(Padding::uniform(TEXT_PADDING)),
                ),
            content_area,
        );
    }
}

pub fn planner_editor_metrics(screen: Rect) -> (u16, u16) {
    let [_title_area, content_area] = right_pane_layout(screen);
    let input_inner = content_area.inner(Margin {
        horizontal: TEXT_PADDING,
        vertical: TEXT_PADDING,
    });
    (input_inner.width.max(1), input_inner.height.max(1))
}

pub fn planner_cursor_hit_test(screen: Rect, app: &App, x: u16, y: u16) -> Option<usize> {
    if !app.is_planner_mode() {
        return None;
    }
    let [_title_area, content_area] = right_pane_layout(screen);
    if x < content_area.x
        || x >= content_area.x.saturating_add(content_area.width)
        || y < content_area.y
        || y >= content_area.y.saturating_add(content_area.height)
    {
        return None;
    }

    let input_inner = content_area.inner(Margin {
        horizontal: TEXT_PADDING,
        vertical: TEXT_PADDING,
    });
    if input_inner.width == 0 || input_inner.height == 0 {
        return Some(app.planner_markdown().chars().count());
    }
    let clamped_x = x.clamp(
        input_inner.x,
        input_inner
            .x
            .saturating_add(input_inner.width.saturating_sub(1)),
    );
    let clamped_y = y.clamp(
        input_inner.y,
        input_inner
            .y
            .saturating_add(input_inner.height.saturating_sub(1)),
    );
    let line = app
        .right_scroll()
        .saturating_add(clamped_y.saturating_sub(input_inner.y));
    let col = clamped_x.saturating_sub(input_inner.x);
    Some(app.planner_cursor_index_for_line_col(input_inner.width, line, col))
}

pub fn right_pane_toggle_hit_test(screen: Rect, app: &App, x: u16, y: u16) -> Option<String> {
    if app.is_planner_mode() {
        return None;
    }

    let [_title_area, content_area] = right_pane_layout(screen);

    if x < content_area.x
        || x >= content_area.x.saturating_add(content_area.width)
        || y < content_area.y
        || y >= content_area.y.saturating_add(content_area.height)
    {
        return None;
    }

    let inner_width = content_area.width.saturating_sub(TEXT_PADDING * 2).max(1);
    let view = app.right_block_view(inner_width);
    let inner_y = y.saturating_sub(content_area.y);
    if inner_y < TEXT_PADDING {
        return None;
    }
    let line_index = app.right_scroll() as usize + inner_y.saturating_sub(TEXT_PADDING) as usize;
    if line_index >= view.lines.len() {
        return None;
    }
    let line = &view.lines[line_index];
    if !(line.contains("[+]") || line.contains("[-]")) {
        return None;
    }
    view.toggles
        .iter()
        .find(|toggle| toggle.line_index == line_index)
        .map(|toggle| toggle.task_key.clone())
}

fn right_pane_text(lines: &[String]) -> Text<'static> {
    let mut out = Vec::with_capacity(lines.len());
    let mut in_task_section = true;
    let mut details_continuation = false;
    for line in lines {
        if line == "Execution" {
            in_task_section = false;
            details_continuation = false;
            out.push(Line::from(Span::raw(line.clone())));
            continue;
        }

        if in_task_section && line.contains("details") && line.contains(':') {
            details_continuation = true;
            if let Some((prefix, body, suffix)) = split_box_line_content(line) {
                let details_style = Style::default()
                    .fg(Color::Rgb(145, 145, 145))
                    .add_modifier(Modifier::DIM);
                out.push(Line::from(styled_box_body_preserving_borders(
                    prefix,
                    body,
                    suffix,
                    details_style,
                )));
            } else {
                out.push(Line::from(Span::styled(
                    line.clone(),
                    Style::default()
                        .fg(Color::Rgb(145, 145, 145))
                        .add_modifier(Modifier::DIM),
                )));
            }
            continue;
        }
        if in_task_section && line.contains("[documentation attached]") {
            if let Some((prefix, body, suffix)) = split_box_line_content(line) {
                out.push(Line::from(styled_box_body_preserving_borders(
                    prefix,
                    body,
                    suffix,
                    Style::default().fg(Color::Rgb(80, 190, 100)),
                )));
            } else {
                out.push(Line::from(Span::styled(
                    line.clone(),
                    Style::default().fg(Color::Rgb(80, 190, 100)),
                )));
            }
            continue;
        }
        if in_task_section && details_continuation {
            if is_detail_continuation_line(line) {
                if let Some((prefix, body, suffix)) = split_box_line_content(line) {
                    let details_style = Style::default()
                        .fg(Color::Rgb(145, 145, 145))
                        .add_modifier(Modifier::DIM);
                    out.push(Line::from(styled_box_body_preserving_borders(
                        prefix,
                        body,
                        suffix,
                        details_style,
                    )));
                } else {
                    out.push(Line::from(Span::styled(
                        line.clone(),
                        Style::default()
                            .fg(Color::Rgb(145, 145, 145))
                            .add_modifier(Modifier::DIM),
                    )));
                }
                continue;
            }
            details_continuation = false;
        }

        if in_task_section && line.contains("[~]") {
            let style = Style::default().fg(Color::Rgb(230, 150, 60));
            if let Some((prefix, body, suffix)) = split_box_line_content(line) {
                out.push(Line::from(styled_box_body_preserving_borders(
                    prefix, body, suffix, style,
                )));
            } else {
                out.push(Line::from(Span::styled(line.clone(), style)));
            }
            continue;
        }
        if in_task_section && line.contains("[x]") {
            if let Some(idx) = line.find("[x]") {
                let done_style = Style::default()
                    .fg(Color::Rgb(145, 145, 145))
                    .add_modifier(Modifier::DIM);
                if let Some((prefix, body, suffix)) = split_box_line_content(line) {
                    if let Some(inner_idx) = body.find("[x]") {
                        let mut spans = Vec::new();
                        spans.push(Span::raw(prefix.to_string()));
                        append_styled_preserving_borders(
                            &mut spans,
                            &body[..inner_idx + 1],
                            done_style,
                        );
                        spans.push(Span::styled(
                            "x".to_string(),
                            Style::default().fg(Color::Rgb(80, 190, 100)),
                        ));
                        append_styled_preserving_borders(
                            &mut spans,
                            &body[inner_idx + 2..],
                            done_style,
                        );
                        spans.push(Span::raw(suffix.to_string()));
                        out.push(Line::from(spans));
                    } else {
                        out.push(Line::from(vec![
                            Span::styled(line[..idx + 1].to_string(), done_style),
                            Span::styled(
                                "x".to_string(),
                                Style::default().fg(Color::Rgb(80, 190, 100)),
                            ),
                            Span::styled(line[idx + 2..].to_string(), done_style),
                        ]));
                    }
                } else {
                    out.push(Line::from(vec![
                        Span::styled(line[..idx + 1].to_string(), done_style),
                        Span::styled(
                            "x".to_string(),
                            Style::default().fg(Color::Rgb(80, 190, 100)),
                        ),
                        Span::styled(line[idx + 2..].to_string(), done_style),
                    ]));
                }
                continue;
            }
        }

        if in_task_section
            && (is_numbered_top_task_line(line)
                || (!line.is_empty()
                    && !line.starts_with("  ")
                    && !line.starts_with('┌')
                    && !line.starts_with('├')
                    && !line.starts_with('└')
                    && !line.starts_with('│')))
        {
            out.push(Line::from(Span::styled(
                line.clone(),
                Style::default().fg(Color::White),
            )));
        } else {
            out.push(Line::from(Span::raw(line.clone())));
        }
    }
    Text::from(out)
}

fn planner_markdown_text(markdown: &str) -> Text<'static> {
    convert_markdown_text(from_str(markdown))
}

fn convert_markdown_text(markdown: CoreText<'_>) -> Text<'static> {
    let alignment = markdown.alignment;
    let style = markdown.style;
    let mut text = Text::from(
        markdown
            .lines
            .into_iter()
            .map(convert_markdown_line)
            .collect::<Vec<_>>(),
    );
    if let Some(alignment) = alignment {
        text = text.alignment(convert_markdown_alignment(alignment));
    }
    text = text.style(convert_markdown_style(style));
    text
}

fn convert_markdown_line(line: CoreLine<'_>) -> Line<'static> {
    let alignment = line.alignment;
    let mut ui_line = Line::from(
        line.spans
            .into_iter()
            .map(convert_markdown_span)
            .collect::<Vec<_>>(),
    )
    .style(convert_markdown_style(line.style));
    if let Some(alignment) = alignment {
        ui_line = ui_line.alignment(convert_markdown_alignment(alignment));
    }
    ui_line
}

fn convert_markdown_span(span: CoreSpan<'_>) -> Span<'static> {
    Span::styled(
        span.content.to_string(),
        convert_markdown_style(span.style),
    )
}

fn convert_markdown_style(style: CoreStyle) -> Style {
    let mut output = Style::default();
    if let Some(fg) = style.fg {
        output = output.fg(convert_markdown_color(fg));
    }
    if let Some(bg) = style.bg {
        output = output.bg(convert_markdown_color(bg));
    }
    output = output.add_modifier(convert_markdown_modifier(style.add_modifier));
    output = output.remove_modifier(convert_markdown_modifier(style.sub_modifier));
    output
}

fn convert_markdown_color(color: CoreColor) -> Color {
    match color {
        CoreColor::Reset => Color::Reset,
        CoreColor::Black => Color::Black,
        CoreColor::Red => Color::Red,
        CoreColor::Green => Color::Green,
        CoreColor::Yellow => Color::Yellow,
        CoreColor::Blue => Color::Blue,
        CoreColor::Magenta => Color::Magenta,
        CoreColor::Cyan => Color::Cyan,
        CoreColor::Gray => Color::Gray,
        CoreColor::DarkGray => Color::DarkGray,
        CoreColor::LightRed => Color::LightRed,
        CoreColor::LightGreen => Color::LightGreen,
        CoreColor::LightYellow => Color::LightYellow,
        CoreColor::LightBlue => Color::LightBlue,
        CoreColor::LightMagenta => Color::LightMagenta,
        CoreColor::LightCyan => Color::LightCyan,
        CoreColor::White => Color::White,
        CoreColor::Rgb(r, g, b) => Color::Rgb(r, g, b),
        CoreColor::Indexed(i) => Color::Indexed(i),
    }
}

fn convert_markdown_modifier(modifier: CoreModifier) -> Modifier {
    let mut output = Modifier::empty();
    if modifier.contains(CoreModifier::BOLD) {
        output.insert(Modifier::BOLD);
    }
    if modifier.contains(CoreModifier::DIM) {
        output.insert(Modifier::DIM);
    }
    if modifier.contains(CoreModifier::ITALIC) {
        output.insert(Modifier::ITALIC);
    }
    if modifier.contains(CoreModifier::UNDERLINED) {
        output.insert(Modifier::UNDERLINED);
    }
    if modifier.contains(CoreModifier::SLOW_BLINK) {
        output.insert(Modifier::SLOW_BLINK);
    }
    if modifier.contains(CoreModifier::RAPID_BLINK) {
        output.insert(Modifier::RAPID_BLINK);
    }
    if modifier.contains(CoreModifier::REVERSED) {
        output.insert(Modifier::REVERSED);
    }
    if modifier.contains(CoreModifier::HIDDEN) {
        output.insert(Modifier::HIDDEN);
    }
    if modifier.contains(CoreModifier::CROSSED_OUT) {
        output.insert(Modifier::CROSSED_OUT);
    }
    output
}

fn convert_markdown_alignment(alignment: CoreAlignment) -> Alignment {
    match alignment {
        CoreAlignment::Left => Alignment::Left,
        CoreAlignment::Center => Alignment::Center,
        CoreAlignment::Right => Alignment::Right,
    }
}

fn split_box_line_content(line: &str) -> Option<(&str, &str, &str)> {
    let first = line.find('│')?;
    let last = line.rfind('│')?;
    if last <= first {
        return None;
    }
    let border_len = '│'.len_utf8();
    let prefix_end = first + border_len;
    let prefix = &line[..prefix_end];
    let body = &line[prefix_end..last];
    let suffix = &line[last..];
    Some((prefix, body, suffix))
}

fn styled_box_body_preserving_borders(
    prefix: &str,
    body: &str,
    suffix: &str,
    style: Style,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    spans.push(Span::raw(prefix.to_string()));
    append_styled_preserving_borders(&mut spans, body, style);
    spans.push(Span::raw(suffix.to_string()));
    spans
}

fn append_styled_preserving_borders(spans: &mut Vec<Span<'static>>, text: &str, style: Style) {
    let mut run = String::new();
    for ch in text.chars() {
        if ch == '│' {
            if !run.is_empty() {
                spans.push(Span::styled(run.clone(), style));
                run.clear();
            }
            spans.push(Span::raw(ch.to_string()));
        } else {
            run.push(ch);
        }
    }
    if !run.is_empty() {
        spans.push(Span::styled(run, style));
    }
}

fn is_detail_continuation_line(line: &str) -> bool {
    if line.trim().is_empty() {
        return false;
    }
    if line.contains('┌') || line.contains('├') || line.contains('└') {
        return false;
    }
    if line.contains("[ ]")
        || line.contains("[~]")
        || line.contains("[!]")
        || line.contains("[x]")
        || line.starts_with("Execution")
        || line.starts_with("Rolling Task Context")
        || line.starts_with("- ")
    {
        return false;
    }
    true
}

fn is_numbered_top_task_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    let Some((index, title)) = trimmed.split_once(". ") else {
        return false;
    };
    !index.is_empty() && index.chars().all(|ch| ch.is_ascii_digit()) && !title.trim().is_empty()
}

fn input_box_metrics(input_text_lines: u16, cursor_line: u16, max_input_height: u16) -> (u16, u16) {
    let capped_text_lines = input_text_lines.clamp(1, MAX_INPUT_TEXT_LINES);
    let desired_height = capped_text_lines.saturating_add(TEXT_PADDING * 2);
    let input_height = desired_height.clamp(1, max_input_height.max(1));
    let visible_text_lines = input_height.saturating_sub(TEXT_PADDING * 2).max(1);
    let max_scroll = input_text_lines.saturating_sub(visible_text_lines);
    let middle_line = visible_text_lines / 2;
    let input_scroll = cursor_line.saturating_sub(middle_line).min(max_scroll);
    (input_height, input_scroll)
}

fn title_bar_bg(base: Color, active: bool) -> Color {
    if active {
        return ACTIVE_TITLE_BG;
    }
    match base {
        Color::Rgb(r, g, b) => {
            let delta = -12;
            Color::Rgb(
                adjust_channel(r, delta),
                adjust_channel(g, delta),
                adjust_channel(b, delta),
            )
        }
        _ => base,
    }
}

fn point_in_rect(rect: Rect, x: u16, y: u16) -> bool {
    x >= rect.x
        && x < rect.x.saturating_add(rect.width)
        && y >= rect.y
        && y < rect.y.saturating_add(rect.height)
}

fn adjust_channel(channel: u8, delta: i16) -> u8 {
    let value = channel as i16 + delta;
    value.clamp(0, 255) as u8
}

#[cfg(test)]
#[path = "../tests/unit/ui_tests.rs"]
mod tests;
