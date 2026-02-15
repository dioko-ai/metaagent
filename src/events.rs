use std::io;
use std::time::Duration;

use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEventKind,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppEvent {
    Tick,
    Quit,
    NextPane,
    PrevPane,
    MoveUp,
    MoveDown,
    CursorLeft,
    CursorRight,
    ScrollChatUp,
    ScrollChatDown,
    ScrollRightUpGlobal,
    ScrollRightDownGlobal,
    InputChar(char),
    Backspace,
    Submit,
    MouseScrollUp,
    MouseScrollDown,
    MouseLeftClick(u16, u16),
}

fn map_key_event(key_event: KeyEvent) -> AppEvent {
    if key_event.kind != KeyEventKind::Press {
        return AppEvent::Tick;
    }

    if key_event.code == KeyCode::Char('c') && key_event.modifiers.contains(KeyModifiers::CONTROL) {
        return AppEvent::Quit;
    }
    if key_event.code == KeyCode::Char('u') && key_event.modifiers.contains(KeyModifiers::CONTROL) {
        return AppEvent::ScrollRightUpGlobal;
    }
    if key_event.code == KeyCode::Char('d') && key_event.modifiers.contains(KeyModifiers::CONTROL) {
        return AppEvent::ScrollRightDownGlobal;
    }

    match key_event.code {
        KeyCode::Tab => AppEvent::NextPane,
        KeyCode::BackTab => AppEvent::PrevPane,
        KeyCode::Up
            if key_event.modifiers.contains(KeyModifiers::SHIFT)
                || key_event.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            AppEvent::ScrollChatUp
        }
        KeyCode::Down
            if key_event.modifiers.contains(KeyModifiers::SHIFT)
                || key_event.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            AppEvent::ScrollChatDown
        }
        KeyCode::PageUp => AppEvent::ScrollRightUpGlobal,
        KeyCode::PageDown => AppEvent::ScrollRightDownGlobal,
        KeyCode::Up => AppEvent::MoveUp,
        KeyCode::Down => AppEvent::MoveDown,
        KeyCode::Left => AppEvent::CursorLeft,
        KeyCode::Right => AppEvent::CursorRight,
        KeyCode::Backspace => AppEvent::Backspace,
        KeyCode::Enter => AppEvent::Submit,
        KeyCode::Char(c) => AppEvent::InputChar(c),
        _ => AppEvent::Tick,
    }
}

fn map_mouse_event_kind(kind: MouseEventKind) -> AppEvent {
    match kind {
        MouseEventKind::ScrollUp => AppEvent::MouseScrollUp,
        MouseEventKind::ScrollDown => AppEvent::MouseScrollDown,
        MouseEventKind::Down(crossterm::event::MouseButton::Left) => AppEvent::MouseLeftClick(0, 0),
        _ => AppEvent::Tick,
    }
}

pub fn next_event() -> io::Result<AppEvent> {
    if event::poll(Duration::from_millis(16))? {
        match event::read()? {
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                return Ok(map_key_event(key_event));
            }
            Event::Mouse(mouse_event) => {
                if let MouseEventKind::Down(crossterm::event::MouseButton::Left) = mouse_event.kind
                {
                    return Ok(AppEvent::MouseLeftClick(
                        mouse_event.column,
                        mouse_event.row,
                    ));
                }
                return Ok(map_mouse_event_kind(mouse_event.kind));
            }
            _ => {}
        }
    }

    Ok(AppEvent::Tick)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_navigation_and_quit_keys() {
        assert_eq!(
            map_key_event(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
            AppEvent::NextPane
        );
        assert_eq!(
            map_key_event(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT)),
            AppEvent::PrevPane
        );
        assert_eq!(
            map_key_event(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            AppEvent::Quit
        );
    }

    #[test]
    fn maps_escape_to_tick() {
        assert_eq!(
            map_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            AppEvent::Tick
        );
    }

    #[test]
    fn maps_movement_keys() {
        assert_eq!(
            map_key_event(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
            AppEvent::MoveDown
        );
        assert_eq!(
            map_key_event(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)),
            AppEvent::MoveUp
        );
        assert_eq!(
            map_key_event(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)),
            AppEvent::CursorLeft
        );
        assert_eq!(
            map_key_event(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)),
            AppEvent::CursorRight
        );
    }

    #[test]
    fn maps_shift_up_down_to_chat_scroll() {
        assert_eq!(
            map_key_event(KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT)),
            AppEvent::ScrollChatUp
        );
        assert_eq!(
            map_key_event(KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT)),
            AppEvent::ScrollChatDown
        );
        assert_eq!(
            map_key_event(KeyEvent::new(KeyCode::Up, KeyModifiers::CONTROL)),
            AppEvent::ScrollChatUp
        );
        assert_eq!(
            map_key_event(KeyEvent::new(KeyCode::Down, KeyModifiers::CONTROL)),
            AppEvent::ScrollChatDown
        );
    }

    #[test]
    fn maps_page_up_down_to_right_pane_global_scroll() {
        assert_eq!(
            map_key_event(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE)),
            AppEvent::ScrollRightUpGlobal
        );
        assert_eq!(
            map_key_event(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE)),
            AppEvent::ScrollRightDownGlobal
        );
    }

    #[test]
    fn maps_ctrl_u_d_to_right_pane_scroll_regardless_of_focus() {
        assert_eq!(
            map_key_event(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL)),
            AppEvent::ScrollRightUpGlobal
        );
        assert_eq!(
            map_key_event(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL)),
            AppEvent::ScrollRightDownGlobal
        );
    }

    #[test]
    fn maps_text_editing_keys() {
        assert_eq!(
            map_key_event(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE)),
            AppEvent::InputChar('k')
        );
        assert_eq!(
            map_key_event(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)),
            AppEvent::Backspace
        );
        assert_eq!(
            map_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            AppEvent::Submit
        );
    }

    #[test]
    fn maps_unhandled_keys_to_tick() {
        assert_eq!(
            map_key_event(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE)),
            AppEvent::Tick
        );
    }

    #[test]
    fn maps_mouse_wheel_to_active_scroll_events() {
        assert_eq!(
            map_mouse_event_kind(MouseEventKind::ScrollUp),
            AppEvent::MouseScrollUp
        );
        assert_eq!(
            map_mouse_event_kind(MouseEventKind::ScrollDown),
            AppEvent::MouseScrollDown
        );
    }

    #[test]
    fn maps_left_click_mouse_down() {
        assert_eq!(
            map_mouse_event_kind(MouseEventKind::Down(crossterm::event::MouseButton::Left)),
            AppEvent::MouseLeftClick(0, 0)
        );
    }
}
