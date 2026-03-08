//! Login screen: shows auth status and login progress.

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::ui::theme;

#[derive(Debug, Clone)]
pub enum LoginState {
    /// Waiting for user to confirm login
    Prompt,
    /// Browser opened, waiting for user to authenticate
    WaitingForBrowser,
    /// Login successful
    Success(String),
    /// Login failed
    Error(String),
}

pub struct LoginScreen {
    pub state: LoginState,
}

impl LoginScreen {
    pub fn new() -> Self {
        Self {
            state: LoginState::Prompt,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2), // Header
                Constraint::Min(5),    // Content
                Constraint::Length(3), // Footer
            ])
            .split(area);

        // Header
        let header = Paragraph::new(" Microsoft Login")
            .style(theme::title_style())
            .block(Block::default().borders(Borders::BOTTOM));
        frame.render_widget(header, chunks[0]);

        // Content
        let content_block = Block::default()
            .title(" Authentication ")
            .borders(Borders::ALL)
            .border_style(ratatui::style::Style::default().fg(theme::DARK_GRAY));

        let lines: Vec<Line> = match &self.state {
            LoginState::Prompt => vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  Press Enter to sign in with your Microsoft account.",
                    theme::normal_style(),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "  Your browser will open to the Microsoft login page.",
                    theme::dim_style(),
                )),
                Line::from(Span::styled(
                    "  After signing in, you'll be redirected back to MUI.",
                    theme::dim_style(),
                )),
            ],
            LoginState::WaitingForBrowser => vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  Waiting for you to sign in via browser...",
                    theme::status_style(),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "  A browser window should have opened.",
                    theme::dim_style(),
                )),
                Line::from(Span::styled(
                    "  Complete the login there, then return here.",
                    theme::dim_style(),
                )),
            ],
            LoginState::Success(username) => vec![
                Line::from(""),
                Line::from(Span::styled(
                    format!("  Successfully logged in as {username}!"),
                    theme::title_style(),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "  Press Esc to return to the home screen.",
                    theme::dim_style(),
                )),
            ],
            LoginState::Error(err) => vec![
                Line::from(""),
                Line::from(Span::styled(
                    format!("  Login failed: {err}"),
                    theme::error_style(),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "  Press Enter to try again, or Esc to go back.",
                    theme::dim_style(),
                )),
            ],
        };

        let content = Paragraph::new(lines).block(content_block);
        frame.render_widget(content, chunks[1]);

        // Footer
        let footer_text = match &self.state {
            LoginState::Prompt => Line::from(vec![
                Span::styled(" Enter", theme::keybind_style()),
                Span::raw(" Login  "),
                Span::styled("Esc", theme::keybind_style()),
                Span::raw(" Back"),
            ]),
            _ => Line::from(vec![
                Span::styled(" Esc", theme::keybind_style()),
                Span::raw(" Back"),
            ]),
        };
        let footer = Paragraph::new(footer_text)
            .alignment(Alignment::Left)
            .block(Block::default().borders(Borders::TOP));
        frame.render_widget(footer, chunks[2]);
    }
}
