//! A shared log buffer and TUI widget for displaying log lines in real-time.
//!
//! The `TuiLogLayer` is a tracing `Layer` that captures formatted log events
//! into a thread-safe ring buffer. The `LogPanel` widget renders the most
//! recent lines from that buffer.

use std::sync::{Arc, Mutex};

use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use tracing_subscriber::Layer;

use crate::ui::theme;

const MAX_LOG_LINES: usize = 200;

/// Thread-safe ring buffer of formatted log lines.
#[derive(Debug, Clone)]
pub struct LogBuffer {
    inner: Arc<Mutex<Vec<LogLine>>>,
}

#[derive(Debug, Clone)]
struct LogLine {
    level: tracing::Level,
    text: String,
}

impl LogBuffer {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Vec::with_capacity(MAX_LOG_LINES))),
        }
    }

    /// Push a line at a given level.
    fn push(&self, level: tracing::Level, text: String) {
        let mut lines = self.inner.lock().unwrap();
        lines.push(LogLine { level, text });
        if lines.len() > MAX_LOG_LINES {
            let excess = lines.len() - MAX_LOG_LINES;
            lines.drain(..excess);
        }
    }

    /// Push a plain info-level message (for non-tracing status updates).
    pub fn push_info(&self, text: String) {
        self.push(tracing::Level::INFO, text);
    }

    /// Get the most recent `n` lines for rendering.
    fn recent(&self, n: usize) -> Vec<LogLine> {
        let lines = self.inner.lock().unwrap();
        let start = lines.len().saturating_sub(n);
        lines[start..].to_vec()
    }
}

/// Render the log panel into a given area.
pub fn render_log_panel(buf: &LogBuffer, frame: &mut Frame, area: Rect, title: &str) {
    // Available lines inside the bordered block (subtract 2 for top+bottom border)
    let inner_height = area.height.saturating_sub(2) as usize;
    let lines = buf.recent(inner_height);

    let styled_lines: Vec<Line> = lines
        .iter()
        .map(|entry| {
            let level_style = match entry.level {
                tracing::Level::ERROR => theme::error_style(),
                tracing::Level::WARN => Style::default().fg(theme::YELLOW),
                tracing::Level::INFO => Style::default().fg(theme::BLUE),
                tracing::Level::DEBUG => theme::dim_style(),
                tracing::Level::TRACE => theme::dim_style(),
            };
            let level_tag = match entry.level {
                tracing::Level::ERROR => "ERR",
                tracing::Level::WARN => "WRN",
                tracing::Level::INFO => "INF",
                tracing::Level::DEBUG => "DBG",
                tracing::Level::TRACE => "TRC",
            };
            Line::from(vec![
                Span::styled(format!(" {level_tag} "), level_style),
                Span::styled(&entry.text, theme::normal_style()),
            ])
        })
        .collect();

    let paragraph = Paragraph::new(styled_lines)
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .title(format!(" {title} "))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::DARK_GRAY)),
        );

    frame.render_widget(paragraph, area);
}

// ── Tracing Layer ────────────────────────────────────────────────────

/// A `tracing_subscriber::Layer` that captures log events into a `LogBuffer`.
pub struct TuiLogLayer {
    buffer: LogBuffer,
}

impl TuiLogLayer {
    pub fn new(buffer: LogBuffer) -> Self {
        Self { buffer }
    }
}

impl<S> Layer<S> for TuiLogLayer
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        // Format the message from the event's fields
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);

        let text = visitor.message;
        if !text.is_empty() {
            self.buffer.push(*event.metadata().level(), text);
        }
    }
}

/// Visitor that extracts the `message` field from a tracing event.
#[derive(Default)]
struct MessageVisitor {
    message: String,
}

impl tracing::field::Visit for MessageVisitor {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else {
            // Append non-message fields
            if !self.message.is_empty() {
                self.message.push(' ');
            }
            self.message
                .push_str(&format!("{}={}", field.name(), value));
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{value:?}");
        } else {
            if !self.message.is_empty() {
                self.message.push(' ');
            }
            self.message
                .push_str(&format!("{}={:?}", field.name(), value));
        }
    }
}
