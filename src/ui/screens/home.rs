//! Home screen: header, instance list, log panel, and footer.
//!
//! Layout:
//! ┌─────────────────────────────────────────────────────────┐
//! │ MUI  |  Logged in as username                           │
//! ├─────────────────────────────────────────────────────────┤
//! │ Instances                                               │
//! │ ▸ Minecraft 1.21.4                                      │
//! │   1.21.4  |  Last played: 2026-03-05                    │
//! │                                                         │
//! │   Minecraft 1.20.1                                      │
//! │   1.20.1  |  Last played: Never                         │
//! ├─────────────────────────────────────────────────────────┤
//! │  INF MUI starting up                                    │
//! │  INF Loaded auth for username                            │
//! ├─────────────────────────────────────────────────────────┤
//! │ Enter Launch  n New  e Edit  d Delete  l Login  q Quit  │
//! └─────────────────────────────────────────────────────────┘

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

use crate::{
    instance::manager::Instance,
    ui::{
        theme,
        widgets::log_panel::{self, LogBuffer},
    },
};

pub struct HomeScreen {
    pub list_state: ListState,
    pub instances: Vec<Instance>,
}

impl HomeScreen {
    pub fn new() -> Self {
        Self {
            list_state: ListState::default(),
            instances: Vec::new(),
        }
    }

    pub fn select_next(&mut self) {
        if self.instances.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => (i + 1) % self.instances.len(),
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    pub fn select_previous(&mut self) {
        if self.instances.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.instances.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    pub fn selected_instance(&self) -> Option<&Instance> {
        self.list_state
            .selected()
            .and_then(|i| self.instances.get(i))
    }

    pub fn render(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        username: Option<&str>,
        log_buffer: &LogBuffer,
    ) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2),  // Header
                Constraint::Min(5),     // Instance list
                Constraint::Length(12), // Log panel
                Constraint::Length(3),  // Footer
            ])
            .split(area);

        self.render_header(frame, chunks[0], username);
        self.render_instance_list(frame, chunks[1]);
        log_panel::render_log_panel(log_buffer, frame, chunks[2], "Log");
        self.render_footer(frame, chunks[3]);
    }

    fn render_header(&self, frame: &mut Frame, area: Rect, username: Option<&str>) {
        let header_text = match username {
            Some(name) => Line::from(vec![
                Span::styled(
                    " MUI",
                    Style::default()
                        .fg(theme::GREEN)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("  |  ", Style::default().fg(theme::DARK_GRAY)),
                Span::styled("Logged in as ", theme::dim_style()),
                Span::styled(
                    name,
                    Style::default()
                        .fg(theme::TEXT)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            None => Line::from(vec![
                Span::styled(
                    " MUI",
                    Style::default()
                        .fg(theme::GREEN)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("  |  ", Style::default().fg(theme::DARK_GRAY)),
                Span::styled("Not logged in", theme::dim_style()),
            ]),
        };
        let header = Paragraph::new(header_text).block(Block::default().borders(Borders::BOTTOM));
        frame.render_widget(header, area);
    }

    fn render_instance_list(&mut self, frame: &mut Frame, area: Rect) {
        if self.instances.is_empty() {
            let empty = Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled("  No instances yet.", theme::dim_style())),
                Line::from(""),
                Line::from(Span::styled(
                    "  Press 'n' to create a new instance.",
                    theme::status_style(),
                )),
            ])
            .block(
                Block::default()
                    .title(" Instances ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme::DARK_GRAY)),
            );
            frame.render_widget(empty, area);
        } else {
            let items: Vec<ListItem> = self
                .instances
                .iter()
                .map(|inst| {
                    let version = &inst.config.version_id;
                    let last_played =
                        crate::ui::format_last_played(inst.config.last_played.as_deref());
                    ListItem::new(vec![
                        Line::from(Span::styled(
                            format!("  {}", inst.config.name),
                            Style::default().add_modifier(Modifier::BOLD),
                        )),
                        Line::from(Span::styled(
                            format!("    {version}  |  Last played: {last_played}"),
                            theme::dim_style(),
                        )),
                    ])
                })
                .collect();

            let list = List::new(items)
                .block(
                    Block::default()
                        .title(" Instances ")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(theme::DARK_GRAY)),
                )
                .highlight_style(theme::selected_style())
                .highlight_symbol("▸ ");

            frame.render_stateful_widget(list, area, &mut self.list_state);
        }
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        let keybinds = Line::from(vec![
            Span::styled(" Enter", theme::keybind_style()),
            Span::raw(" Launch  "),
            Span::styled("n", theme::keybind_style()),
            Span::raw(" New  "),
            Span::styled("e", theme::keybind_style()),
            Span::raw(" Edit  "),
            Span::styled("d", theme::keybind_style()),
            Span::raw(" Delete  "),
            Span::styled("l", theme::keybind_style()),
            Span::raw(" Login  "),
            Span::styled("q", theme::keybind_style()),
            Span::raw(" Quit"),
        ]);
        let footer = Paragraph::new(keybinds).block(Block::default().borders(Borders::TOP));
        frame.render_widget(footer, area);
    }
}
