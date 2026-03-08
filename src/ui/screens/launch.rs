//! Launch progress screen: shows download progress and game log output.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph, Wrap},
    Frame,
};

use crate::minecraft::download::DownloadProgress;
use crate::ui::theme;

#[derive(Debug, Clone)]
pub enum LaunchState {
    Downloading,
    Starting,
    Running,
    Finished(i32), // exit code
    Error(String),
}

pub struct LaunchScreen {
    pub state: LaunchState,
    pub instance_name: String,
    pub progress: Option<DownloadProgress>,
    pub log_lines: Vec<String>,
}

impl LaunchScreen {
    pub fn new(instance_name: String) -> Self {
        Self {
            state: LaunchState::Downloading,
            instance_name,
            progress: None,
            log_lines: Vec::new(),
        }
    }

    pub fn add_log_line(&mut self, line: String) {
        self.log_lines.push(line);
        // Keep only last 500 lines
        if self.log_lines.len() > 500 {
            self.log_lines.drain(..self.log_lines.len() - 500);
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2), // Header
                Constraint::Length(3), // Progress bar / status
                Constraint::Min(5),    // Log output
                Constraint::Length(3), // Footer
            ])
            .split(area);

        // Header
        let status_text = match &self.state {
            LaunchState::Downloading => "Downloading...",
            LaunchState::Starting => "Starting Minecraft...",
            LaunchState::Running => "Running",
            LaunchState::Finished(code) => {
                if *code == 0 {
                    "Game exited normally"
                } else {
                    "Game exited with error"
                }
            }
            LaunchState::Error(_) => "Error",
        };
        let header = Paragraph::new(format!(
            " Launching: {}  |  {}",
            self.instance_name, status_text
        ))
        .style(theme::title_style())
        .block(Block::default().borders(Borders::BOTTOM));
        frame.render_widget(header, chunks[0]);

        // Progress bar
        if let Some(ref progress) = self.progress {
            let ratio = if progress.total_files > 0 {
                progress.completed_files as f64 / progress.total_files as f64
            } else {
                0.0
            };
            let label = format!(
                "{}/{} files  |  {}",
                progress.completed_files, progress.total_files, progress.current_file
            );
            let gauge = Gauge::default()
                .block(Block::default().borders(Borders::ALL).title(" Progress "))
                .gauge_style(
                    ratatui::style::Style::default()
                        .fg(theme::GREEN)
                        .bg(theme::SURFACE),
                )
                .ratio(ratio)
                .label(label);
            frame.render_widget(gauge, chunks[1]);
        } else {
            let status = match &self.state {
                LaunchState::Error(e) => {
                    Paragraph::new(Span::styled(format!("  Error: {e}"), theme::error_style()))
                }
                _ => Paragraph::new(Span::styled(
                    format!("  {status_text}"),
                    theme::status_style(),
                )),
            };
            let status = status.block(Block::default().borders(Borders::ALL));
            frame.render_widget(status, chunks[1]);
        }

        // Log output
        let log_lines: Vec<Line> = self
            .log_lines
            .iter()
            .rev()
            .take(chunks[2].height as usize)
            .rev()
            .map(|l| Line::from(Span::styled(l.clone(), theme::dim_style())))
            .collect();

        let log = Paragraph::new(log_lines).wrap(Wrap { trim: false }).block(
            Block::default()
                .title(" Game Output ")
                .borders(Borders::ALL)
                .border_style(ratatui::style::Style::default().fg(theme::DARK_GRAY)),
        );
        frame.render_widget(log, chunks[2]);

        // Footer
        let footer_text = match &self.state {
            LaunchState::Running => Line::from(vec![
                Span::styled(" Esc", theme::keybind_style()),
                Span::raw(" Back (game continues)"),
            ]),
            LaunchState::Finished(_) | LaunchState::Error(_) => Line::from(vec![
                Span::styled(" Esc", theme::keybind_style()),
                Span::raw(" Back to home"),
            ]),
            _ => Line::from(Span::styled(" Please wait...", theme::dim_style())),
        };
        let footer = Paragraph::new(footer_text).block(Block::default().borders(Borders::TOP));
        frame.render_widget(footer, chunks[3]);
    }
}
