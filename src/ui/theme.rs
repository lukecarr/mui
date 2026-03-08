//! Color palette and styling constants for the TUI.

use ratatui::style::{Color, Modifier, Style};

// Brand colors
pub const GREEN: Color = Color::Rgb(76, 175, 80);
pub const BLUE: Color = Color::Rgb(66, 165, 245);
pub const RED: Color = Color::Rgb(239, 83, 80);
pub const YELLOW: Color = Color::Rgb(255, 202, 40);
pub const GRAY: Color = Color::Rgb(158, 158, 158);
pub const DARK_GRAY: Color = Color::Rgb(97, 97, 97);
pub const SURFACE: Color = Color::Rgb(42, 42, 42);
pub const TEXT: Color = Color::Rgb(224, 224, 224);
pub const TEXT_DIM: Color = Color::Rgb(158, 158, 158);

pub fn title_style() -> Style {
    Style::default().fg(GREEN).add_modifier(Modifier::BOLD)
}

pub fn selected_style() -> Style {
    Style::default()
        .bg(SURFACE)
        .fg(GREEN)
        .add_modifier(Modifier::BOLD)
}

pub fn normal_style() -> Style {
    Style::default().fg(TEXT)
}

pub fn dim_style() -> Style {
    Style::default().fg(TEXT_DIM)
}

pub fn error_style() -> Style {
    Style::default().fg(RED)
}

pub fn status_style() -> Style {
    Style::default().fg(BLUE)
}

pub fn keybind_style() -> Style {
    Style::default().fg(YELLOW).add_modifier(Modifier::BOLD)
}
