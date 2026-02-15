#[derive(Debug, Clone)]
pub struct WrappedText {
    pub rendered: String,
    pub positions: Vec<(u16, u16)>,
    pub line_count: u16,
}

pub fn wrap_word_with_positions(text: &str, width: u16) -> WrappedText {
    let width = width.max(1);
    let chars: Vec<char> = text.chars().collect();
    let mut rendered = String::new();
    let mut positions = Vec::with_capacity(chars.len() + 1);
    let mut line = 0u16;
    let mut col = 0u16;

    positions.push((line, col));

    for (idx, ch) in chars.iter().copied().enumerate() {
        if ch == '\n' {
            rendered.push('\n');
            line = line.saturating_add(1);
            col = 0;
            positions.push((line, col));
            continue;
        }

        if should_wrap_before_word(&chars, idx, col, width) {
            rendered.push('\n');
            line = line.saturating_add(1);
            col = 0;
        } else if col >= width {
            rendered.push('\n');
            line = line.saturating_add(1);
            col = 0;
        }

        rendered.push(ch);
        col = col.saturating_add(1);
        if col >= width {
            rendered.push('\n');
            line = line.saturating_add(1);
            col = 0;
        }

        positions.push((line, col));
    }

    let line_count = positions
        .iter()
        .map(|(l, _)| *l)
        .max()
        .unwrap_or(0)
        .saturating_add(1);

    WrappedText {
        rendered,
        positions,
        line_count,
    }
}

fn should_wrap_before_word(chars: &[char], idx: usize, col: u16, width: u16) -> bool {
    if col == 0 {
        return false;
    }
    let ch = chars[idx];
    if ch.is_whitespace() {
        return false;
    }
    if idx > 0 {
        let prev = chars[idx - 1];
        if !prev.is_whitespace() && prev != '\n' {
            return false;
        }
    }

    let word_len = chars[idx..]
        .iter()
        .take_while(|c| !c.is_whitespace() && **c != '\n')
        .count() as u16;

    word_len <= width && col.saturating_add(word_len) > width
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_by_word_when_possible() {
        let wrapped = wrap_word_with_positions("hello world", 6);
        assert_eq!(wrapped.rendered, "hello \nworld");
        assert_eq!(wrapped.line_count, 2);
    }

    #[test]
    fn breaks_long_words_when_needed() {
        let wrapped = wrap_word_with_positions("abcdefghij", 4);
        assert_eq!(wrapped.rendered, "abcd\nefgh\nij");
        assert_eq!(wrapped.line_count, 3);
    }

    #[test]
    fn produces_cursor_positions_for_each_char_boundary() {
        let wrapped = wrap_word_with_positions("abc def", 4);
        assert_eq!(wrapped.positions.len(), "abc def".chars().count() + 1);
        assert_eq!(wrapped.positions[0], (0, 0));
    }
}
