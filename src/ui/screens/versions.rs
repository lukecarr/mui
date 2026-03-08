//! Version browser screen: browse Minecraft versions and create instances.

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

use crate::minecraft::manifest::{VersionEntry, VersionType};
use crate::ui::theme;

pub struct VersionsScreen {
    pub list_state: ListState,
    pub versions: Vec<VersionEntry>,
    pub show_snapshots: bool,
    pub loading: bool,
    /// Instance name being typed
    pub input_name: Option<String>,
}

impl VersionsScreen {
    pub fn new() -> Self {
        Self {
            list_state: ListState::default(),
            versions: Vec::new(),
            show_snapshots: false,
            loading: false,
            input_name: None,
        }
    }

    pub fn filtered_versions(&self) -> Vec<&VersionEntry> {
        self.versions
            .iter()
            .filter(|v| {
                if self.show_snapshots {
                    v.version_type == VersionType::Release
                        || v.version_type == VersionType::Snapshot
                } else {
                    v.version_type == VersionType::Release
                }
            })
            .collect()
    }

    pub fn select_next(&mut self) {
        let count = self.filtered_versions().len();
        if count == 0 {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => (i + 1) % count,
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    pub fn select_previous(&mut self) {
        let count = self.filtered_versions().len();
        if count == 0 {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => {
                if i == 0 {
                    count - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    pub fn selected_version(&self) -> Option<&VersionEntry> {
        let filtered = self.filtered_versions();
        self.list_state
            .selected()
            .and_then(|i| filtered.get(i).copied())
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Header
                Constraint::Min(5),    // Version list or name input
                Constraint::Length(3), // Footer
            ])
            .split(area);

        // Header
        let filter = if self.show_snapshots {
            "Releases + Snapshots"
        } else {
            "Releases only"
        };
        let header = Paragraph::new(format!(" New Instance  |  Filter: {filter}"))
            .style(theme::title_style())
            .block(Block::default().borders(Borders::BOTTOM));
        frame.render_widget(header, chunks[0]);

        // Content
        if let Some(ref name) = self.input_name {
            // Name input mode
            let selected = self.selected_version();
            let version_name = selected.map(|v| v.id.as_str()).unwrap_or("?");

            let lines = vec![
                Line::from(""),
                Line::from(Span::styled(
                    format!("  Creating instance for Minecraft {version_name}"),
                    theme::status_style(),
                )),
                Line::from(""),
                Line::from(vec![
                    Span::styled("  Instance name: ", theme::normal_style()),
                    Span::styled(
                        format!("{name}_"),
                        Style::default()
                            .fg(theme::GREEN)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(""),
                Line::from(Span::styled(
                    "  Press Enter to confirm, Esc to cancel.",
                    theme::dim_style(),
                )),
            ];

            let content = Paragraph::new(lines).block(
                Block::default()
                    .title(" Instance Name ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme::DARK_GRAY)),
            );
            frame.render_widget(content, chunks[1]);
        } else if self.loading {
            let content = Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  Loading version list...",
                    theme::status_style(),
                )),
            ])
            .block(
                Block::default()
                    .title(" Versions ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme::DARK_GRAY)),
            );
            frame.render_widget(content, chunks[1]);
        } else {
            let filtered = self.filtered_versions();
            let items: Vec<ListItem> = filtered
                .iter()
                .map(|v| {
                    let type_label = match v.version_type {
                        VersionType::Release => {
                            Span::styled(" release", Style::default().fg(theme::GREEN))
                        }
                        VersionType::Snapshot => {
                            Span::styled(" snapshot", Style::default().fg(theme::YELLOW))
                        }
                        VersionType::OldBeta => {
                            Span::styled(" beta", Style::default().fg(theme::GRAY))
                        }
                        VersionType::OldAlpha => {
                            Span::styled(" alpha", Style::default().fg(theme::GRAY))
                        }
                    };
                    ListItem::new(Line::from(vec![
                        Span::raw("  "),
                        Span::styled(v.id.clone(), Style::default().add_modifier(Modifier::BOLD)),
                        type_label,
                    ]))
                })
                .collect();

            let list = List::new(items)
                .block(
                    Block::default()
                        .title(format!(" Versions ({}) ", filtered.len()))
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(theme::DARK_GRAY)),
                )
                .highlight_style(theme::selected_style())
                .highlight_symbol("▸ ");

            frame.render_stateful_widget(list, chunks[1], &mut self.list_state);
        }

        // Footer
        let footer_text = if self.input_name.is_some() {
            Line::from(vec![
                Span::styled(" Enter", theme::keybind_style()),
                Span::raw(" Create  "),
                Span::styled("Esc", theme::keybind_style()),
                Span::raw(" Cancel"),
            ])
        } else {
            Line::from(vec![
                Span::styled(" Enter", theme::keybind_style()),
                Span::raw(" Select  "),
                Span::styled("s", theme::keybind_style()),
                Span::raw(" Toggle snapshots  "),
                Span::styled("Esc", theme::keybind_style()),
                Span::raw(" Back"),
            ])
        };
        let footer = Paragraph::new(footer_text).block(Block::default().borders(Borders::TOP));
        frame.render_widget(footer, chunks[2]);
    }
}
