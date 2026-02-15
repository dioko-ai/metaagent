use std::fs;
use std::path::Path;

use ratatui::style::Color;
use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct Theme {
    pub left_top_bg: Color,
    pub chat_bg: Color,
    pub right_bg: Color,
    pub input_bg: Color,
    pub status_bg: Color,
    pub text_fg: Color,
    pub muted_fg: Color,
    pub active_fg: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            left_top_bg: Color::Rgb(44, 44, 44),
            chat_bg: Color::Rgb(54, 54, 54),
            right_bg: Color::Rgb(48, 48, 48),
            input_bg: Color::Rgb(62, 62, 62),
            status_bg: Color::Rgb(36, 36, 36),
            text_fg: Color::Rgb(225, 225, 225),
            muted_fg: Color::Rgb(185, 185, 185),
            active_fg: Color::Rgb(255, 255, 255),
        }
    }
}

impl Theme {
    pub fn load_or_default(path: impl AsRef<Path>) -> Self {
        let path_ref = path.as_ref();
        match fs::read_to_string(path_ref) {
            Ok(contents) => match Self::from_toml_str(&contents) {
                Ok(theme) => theme,
                Err(err) => {
                    eprintln!(
                        "Failed to parse theme file '{}': {err}. Using defaults.",
                        path_ref.display()
                    );
                    Self::default()
                }
            },
            Err(err) => {
                eprintln!(
                    "Failed to read theme file '{}': {err}. Using defaults.",
                    path_ref.display()
                );
                Self::default()
            }
        }
    }

    pub fn from_toml_str(s: &str) -> Result<Self, toml::de::Error> {
        let cfg: ThemeToml = toml::from_str(s)?;
        Ok(Self {
            left_top_bg: cfg.colors.left_top_bg.to_color(),
            chat_bg: cfg.colors.chat_bg.to_color(),
            right_bg: cfg.colors.right_bg.to_color(),
            input_bg: cfg.colors.input_bg.to_color(),
            status_bg: cfg.colors.status_bg.to_color(),
            text_fg: cfg.colors.text_fg.to_color(),
            muted_fg: cfg.colors.muted_fg.to_color(),
            active_fg: cfg.colors.active_fg.to_color(),
        })
    }
}

#[derive(Debug, Deserialize)]
struct ThemeToml {
    colors: ThemeColorsToml,
}

#[derive(Debug, Deserialize)]
struct ThemeColorsToml {
    left_top_bg: RgbToml,
    chat_bg: RgbToml,
    right_bg: RgbToml,
    input_bg: RgbToml,
    status_bg: RgbToml,
    text_fg: RgbToml,
    muted_fg: RgbToml,
    active_fg: RgbToml,
}

#[derive(Debug, Deserialize)]
struct RgbToml {
    r: u8,
    g: u8,
    b: u8,
}

impl RgbToml {
    fn to_color(&self) -> Color {
        Color::Rgb(self.r, self.g, self.b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_theme_from_toml() {
        let input = r#"
[colors]
left_top_bg = { r = 1, g = 2, b = 3 }
chat_bg = { r = 4, g = 5, b = 6 }
right_bg = { r = 7, g = 8, b = 9 }
input_bg = { r = 10, g = 11, b = 12 }
status_bg = { r = 13, g = 14, b = 15 }
text_fg = { r = 16, g = 17, b = 18 }
muted_fg = { r = 19, g = 20, b = 21 }
active_fg = { r = 22, g = 23, b = 24 }
"#;

        let theme = Theme::from_toml_str(input).expect("theme should parse");
        assert_eq!(theme.left_top_bg, Color::Rgb(1, 2, 3));
        assert_eq!(theme.chat_bg, Color::Rgb(4, 5, 6));
        assert_eq!(theme.right_bg, Color::Rgb(7, 8, 9));
        assert_eq!(theme.input_bg, Color::Rgb(10, 11, 12));
        assert_eq!(theme.status_bg, Color::Rgb(13, 14, 15));
        assert_eq!(theme.text_fg, Color::Rgb(16, 17, 18));
        assert_eq!(theme.muted_fg, Color::Rgb(19, 20, 21));
        assert_eq!(theme.active_fg, Color::Rgb(22, 23, 24));
    }

    #[test]
    fn uses_default_on_missing_file() {
        let theme = Theme::load_or_default("/definitely-not-a-real-theme-file.toml");
        assert_eq!(theme.left_top_bg, Theme::default().left_top_bg);
    }
}
