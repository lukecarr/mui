//! Instance detail/settings screen.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::instance::manager::Instance;
use crate::java::{detect, runtime};
use crate::ui::theme;

pub struct InstanceScreen {
    pub instance: Option<Instance>,
    /// Path to the MUI-managed Java directory, for checking installed runtimes.
    pub java_dir: std::path::PathBuf,
}

impl InstanceScreen {
    pub fn new(java_dir: std::path::PathBuf) -> Self {
        Self {
            instance: None,
            java_dir,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Header
                Constraint::Min(5),    // Details
                Constraint::Length(3), // Footer
            ])
            .split(area);

        let inst = match &self.instance {
            Some(inst) => inst,
            None => {
                let msg = Paragraph::new("No instance selected").style(theme::dim_style());
                frame.render_widget(msg, area);
                return;
            }
        };

        // Header
        let header = Paragraph::new(format!(" Instance: {}", inst.config.name))
            .style(theme::title_style())
            .block(Block::default().borders(Borders::BOTTOM));
        frame.render_widget(header, chunks[0]);

        // Details
        let last_played = crate::ui::format_last_played(inst.config.last_played.as_deref());

        // Resolve Java info for display
        let resolved = detect::resolve_java(
            &self.java_dir,
            None, // We don't have version meta here, so skip component check
            None,
            inst.config.java_path.as_deref(),
        );
        let java_display = match &resolved {
            Some(java) => {
                let version_str = java
                    .major_version
                    .map(|v| format!("Java {v}"))
                    .unwrap_or_else(|| "unknown version".to_string());
                let source_str = match &java.source {
                    detect::JavaSource::InstanceOverride => "override",
                    detect::JavaSource::Managed { component } => component.as_str(),
                    detect::JavaSource::System => "system",
                };
                format!("{} ({source_str})", version_str)
            }
            None => "(not found)".to_string(),
        };
        let java_path_display = resolved
            .as_ref()
            .map(|j| j.path.as_str())
            .unwrap_or("(auto-detect)");

        // Show managed runtimes installed
        let installed_runtimes = runtime::list_installed(&self.java_dir);
        let managed_display = if installed_runtimes.is_empty() {
            "none".to_string()
        } else {
            installed_runtimes
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        };

        let mut lines = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled(
                    "  Version:     ",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::styled(&inst.config.version_id, theme::normal_style()),
            ]),
            Line::from(vec![
                Span::styled(
                    "  Memory:      ",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(
                        "{} - {} MB",
                        inst.config.min_memory_mb, inst.config.max_memory_mb
                    ),
                    theme::normal_style(),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    "  Window:      ",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{}x{}", inst.config.window_width, inst.config.window_height),
                    theme::normal_style(),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    "  Java:        ",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::styled(&java_display, theme::normal_style()),
            ]),
            Line::from(vec![
                Span::styled(
                    "  Java path:   ",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::styled(java_path_display, theme::dim_style()),
            ]),
            Line::from(vec![
                Span::styled(
                    "  Managed JRE: ",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::styled(&managed_display, theme::dim_style()),
            ]),
            Line::from(vec![
                Span::styled(
                    "  Last played: ",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::styled(last_played, theme::dim_style()),
            ]),
            Line::from(vec![
                Span::styled(
                    "  Directory:   ",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::styled(inst.dir.to_string_lossy().to_string(), theme::dim_style()),
            ]),
        ];

        if !inst.config.jvm_args.is_empty() {
            lines.push(Line::from(vec![
                Span::styled(
                    "  JVM args:    ",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::styled(inst.config.jvm_args.join(" "), theme::dim_style()),
            ]));
        }

        let details = Paragraph::new(lines).block(
            Block::default()
                .title(" Settings ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::DARK_GRAY)),
        );
        frame.render_widget(details, chunks[1]);

        // Footer
        let footer_text = Line::from(vec![
            Span::styled(" Enter", theme::keybind_style()),
            Span::raw(" Launch  "),
            Span::styled("Esc", theme::keybind_style()),
            Span::raw(" Back"),
        ]);
        let footer = Paragraph::new(footer_text).block(Block::default().borders(Borders::TOP));
        frame.render_widget(footer, chunks[2]);
    }
}
