use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::theme::Theme;

pub struct ListRow {
    pub kind: String,
    pub domain: String,
    pub target: String,
    pub status: String,
    pub details: Vec<(String, String)>,
}

pub struct ListTuiState {
    pub items: Vec<ListRow>,
    pub list_state: ListState,
}

impl ListTuiState {
    pub fn new(items: Vec<ListRow>) -> Self {
        let mut list_state = ListState::default();
        if !items.is_empty() {
            list_state.select(Some(0));
        }
        Self { items, list_state }
    }

    pub fn next(&mut self) {
        if self.items.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => (i + 1) % self.items.len(),
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    pub fn previous(&mut self) {
        if self.items.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.items.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    pub fn selected(&self) -> Option<&ListRow> {
        self.list_state.selected().and_then(|i| self.items.get(i))
    }
}

pub fn render(frame: &mut Frame, area: Rect, state: &mut ListTuiState) {
    if state.items.is_empty() {
        let block = Block::default()
            .title(Line::from(vec![Span::styled(
                " xpo list ",
                Theme::accent_bold(),
            )]))
            .borders(Borders::ALL)
            .border_style(Theme::border());
        let msg = Paragraph::new(Line::from(vec![Span::styled(
            "No active tunnels or dev domains",
            Theme::text_dim(),
        )]))
        .block(block);
        frame.render_widget(msg, area);
        return;
    }

    let chunks =
        Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)]).split(area);

    render_list_panel(frame, chunks[0], state);
    render_detail_panel(frame, chunks[1], state);
}

fn render_list_panel(frame: &mut Frame, area: Rect, state: &mut ListTuiState) {
    let items: Vec<ListItem> = state
        .items
        .iter()
        .map(|row| {
            let kind_style = match row.kind.as_str() {
                "share" => Style::default().fg(Theme::ACCENT),
                "dev" => Style::default().fg(Theme::METHOD_PUT),
                _ => Theme::text(),
            };
            let status_style = match row.status.as_str() {
                "active" => Theme::success(),
                _ => Theme::error(),
            };
            let icon = match row.status.as_str() {
                "active" => "\u{25cf}",
                _ => "\u{25cb}",
            };
            ListItem::new(Line::from(vec![
                Span::styled(icon, status_style),
                Span::raw(" "),
                Span::styled(format!("{:<5}", row.kind), kind_style),
                Span::raw(" "),
                Span::styled(&row.domain, Theme::text()),
            ]))
        })
        .collect();

    let count = state.items.len();
    let block = Block::default()
        .title(Line::from(vec![
            Span::styled(" xpo list ", Theme::accent_bold()),
            Span::styled(format!("({count}) "), Theme::text_dim()),
        ]))
        .borders(Borders::ALL)
        .border_style(Theme::border());

    let list = List::new(items)
        .block(block)
        .highlight_style(
            Style::default()
                .fg(Theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    frame.render_stateful_widget(list, area, &mut state.list_state);
}

fn render_detail_panel(frame: &mut Frame, area: Rect, state: &ListTuiState) {
    let block = Block::default()
        .title(Line::from(vec![Span::styled(
            " details ",
            Theme::accent_bold(),
        )]))
        .borders(Borders::ALL)
        .border_style(Theme::border());

    let Some(row) = state.selected() else {
        let empty = Paragraph::new("").block(block);
        frame.render_widget(empty, area);
        return;
    };

    let mut lines = Vec::new();

    lines.push(Line::from(vec![
        Span::styled("Domain    ", Theme::text_dim()),
        Span::styled(&row.domain, Theme::accent_bold()),
    ]));
    lines.push(Line::raw(""));

    let kind_style = match row.kind.as_str() {
        "share" => Style::default().fg(Theme::ACCENT),
        "dev" => Style::default().fg(Theme::METHOD_PUT),
        _ => Theme::text(),
    };
    lines.push(Line::from(vec![
        Span::styled("Type      ", Theme::text_dim()),
        Span::styled(&row.kind, kind_style),
    ]));

    lines.push(Line::from(vec![
        Span::styled("Target    ", Theme::text_dim()),
        Span::styled(&row.target, Theme::text()),
    ]));

    let status_style = match row.status.as_str() {
        "active" => Theme::success(),
        _ => Theme::error(),
    };
    let icon = match row.status.as_str() {
        "active" => "\u{25cf} ",
        _ => "\u{25cb} ",
    };
    lines.push(Line::from(vec![
        Span::styled("Status    ", Theme::text_dim()),
        Span::styled(icon, status_style),
        Span::styled(&row.status, status_style),
    ]));

    if !row.details.is_empty() {
        lines.push(Line::raw(""));
        for (key, val) in &row.details {
            let val_style = match key.as_str() {
                "Password" => {
                    if val == "yes" {
                        Theme::success()
                    } else {
                        Theme::text_dim()
                    }
                }
                "TTL" => Style::default().fg(Theme::METHOD_PUT),
                "Cert" => Theme::text_dim(),
                _ => Theme::text(),
            };
            lines.push(Line::from(vec![
                Span::styled(format!("{:<10}", key), Theme::text_dim()),
                Span::styled(val.as_str(), val_style),
            ]));
        }
    }

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

pub fn render_keybinds(frame: &mut Frame, area: Rect) {
    let keys = Line::from(vec![
        Span::styled("q", Theme::accent()),
        Span::styled(":quit  ", Theme::text_dim()),
        Span::styled("\u{2191}\u{2193}", Theme::accent()),
        Span::styled(":navigate", Theme::text_dim()),
    ]);
    frame.render_widget(Paragraph::new(keys), area);
}
