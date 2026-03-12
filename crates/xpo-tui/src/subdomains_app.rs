use std::io::{self, stdout};
use std::time::{Duration, Instant};

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame, Terminal, TerminalOptions, Viewport,
};

use crate::theme::Theme;

pub struct SubdomainRow {
    pub subdomain: String,
    pub created_at: String,
    pub age: String,
}

pub struct SubdomainsData {
    pub subdomains: Vec<SubdomainRow>,
    pub limit: i32,
    pub count: usize,
}

struct TuiState {
    data: SubdomainsData,
    list_state: ListState,
    confirm_delete: Option<usize>,
    status_msg: Option<(String, Instant)>,
}

impl TuiState {
    fn new(data: SubdomainsData) -> Self {
        let mut list_state = ListState::default();
        if !data.subdomains.is_empty() {
            list_state.select(Some(0));
        }
        Self {
            data,
            list_state,
            confirm_delete: None,
            status_msg: None,
        }
    }

    fn next(&mut self) {
        if self.data.subdomains.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => (i + 1) % self.data.subdomains.len(),
            None => 0,
        };
        self.list_state.select(Some(i));
        self.confirm_delete = None;
    }

    fn previous(&mut self) {
        if self.data.subdomains.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(0) => self.data.subdomains.len() - 1,
            Some(i) => i - 1,
            None => 0,
        };
        self.list_state.select(Some(i));
        self.confirm_delete = None;
    }

    fn selected(&self) -> Option<&SubdomainRow> {
        self.list_state
            .selected()
            .and_then(|i| self.data.subdomains.get(i))
    }

    fn set_status(&mut self, msg: String) {
        self.status_msg = Some((msg, Instant::now()));
    }
}

pub enum Action {
    Delete(String),
    Refresh,
}

pub fn run<F, D>(initial: SubdomainsData, mut refresh_fn: F, mut delete_fn: D) -> io::Result<()>
where
    F: FnMut() -> SubdomainsData,
    D: FnMut(&str) -> Result<(), String>,
{
    enable_raw_mode()?;
    let (_, term_rows) = crossterm::terminal::size()?;
    let height = std::cmp::min(term_rows.saturating_sub(2), 18);
    let mut terminal = Terminal::with_options(
        CrosstermBackend::new(stdout()),
        TerminalOptions {
            viewport: Viewport::Inline(height),
        },
    )?;

    let mut state = TuiState::new(initial);
    let mut last_refresh = Instant::now();

    loop {
        terminal.draw(|frame| {
            let chunks =
                Layout::vertical([Constraint::Min(3), Constraint::Length(1)]).split(frame.area());

            render_main(frame, chunks[0], &mut state);
            render_keybinds(frame, chunks[1], &state);
        })?;

        if let Some((_, ts)) = &state.status_msg {
            if ts.elapsed() > Duration::from_secs(3) {
                state.status_msg = None;
            }
        }

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(KeyEvent {
                code, modifiers, ..
            }) = event::read()?
            {
                match code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => break,
                    KeyCode::Up | KeyCode::Char('k') => state.previous(),
                    KeyCode::Down | KeyCode::Char('j') => state.next(),
                    KeyCode::Char('d') => {
                        if let Some(idx) = state.list_state.selected() {
                            if state.confirm_delete == Some(idx) {
                                let name = state.data.subdomains[idx].subdomain.clone();
                                match delete_fn(&name) {
                                    Ok(()) => {
                                        state.set_status(format!("Removed '{name}'"));
                                        let sel = state.list_state.selected();
                                        state = TuiState::new(refresh_fn());
                                        if let Some(i) = sel {
                                            let new_idx = if i >= state.data.subdomains.len() {
                                                state.data.subdomains.len().saturating_sub(1)
                                            } else {
                                                i
                                            };
                                            if !state.data.subdomains.is_empty() {
                                                state.list_state.select(Some(new_idx));
                                            }
                                        }
                                        state.set_status(format!("Removed '{name}'"));
                                        last_refresh = Instant::now();
                                    }
                                    Err(e) => state.set_status(format!("Error: {e}")),
                                }
                            } else {
                                state.confirm_delete = Some(idx);
                            }
                        }
                    }
                    KeyCode::Char('r') => {
                        let sel = state.list_state.selected();
                        state = TuiState::new(refresh_fn());
                        if let Some(i) = sel {
                            if i < state.data.subdomains.len() {
                                state.list_state.select(Some(i));
                            }
                        }
                        last_refresh = Instant::now();
                    }
                    KeyCode::Esc => {
                        state.confirm_delete = None;
                    }
                    _ => {
                        state.confirm_delete = None;
                    }
                }
            }
        }

        if last_refresh.elapsed() >= Duration::from_secs(10) {
            let sel = state.list_state.selected();
            state = TuiState::new(refresh_fn());
            if let Some(i) = sel {
                if i < state.data.subdomains.len() {
                    state.list_state.select(Some(i));
                }
            }
            last_refresh = Instant::now();
        }
    }

    disable_raw_mode()?;
    terminal.clear()?;
    Ok(())
}

fn render_main(frame: &mut Frame, area: Rect, state: &mut TuiState) {
    if state.data.subdomains.is_empty() {
        let block = Block::default()
            .title(title_spans(&state.data))
            .borders(Borders::ALL)
            .border_style(Theme::border());

        let mut lines = vec![
            Line::raw(""),
            Line::from(vec![Span::styled(
                "  No reserved subdomains yet",
                Theme::text_dim(),
            )]),
            Line::raw(""),
            Line::from(vec![
                Span::styled("  Use ", Theme::text_dim()),
                Span::styled("xpo share -s <name>", Theme::accent()),
                Span::styled(" to auto-reserve", Theme::text_dim()),
            ]),
        ];

        if let Some((msg, _)) = &state.status_msg {
            lines.push(Line::raw(""));
            lines.push(Line::from(vec![Span::styled(
                format!("  {msg}"),
                Theme::success(),
            )]));
        }

        let p = Paragraph::new(lines).block(block);
        frame.render_widget(p, area);
        return;
    }

    let chunks =
        Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)]).split(area);

    render_list(frame, chunks[0], state);
    render_detail(frame, chunks[1], state);
}

fn title_spans(data: &SubdomainsData) -> Line<'static> {
    Line::from(vec![
        Span::styled(" subdomains ", Theme::accent_bold()),
        Span::styled(
            format!("({}/{}) ", data.count, data.limit),
            Theme::text_dim(),
        ),
    ])
}

fn render_list(frame: &mut Frame, area: Rect, state: &mut TuiState) {
    let items: Vec<ListItem> = state
        .data
        .subdomains
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let is_confirm = state.confirm_delete == Some(i);
            if is_confirm {
                ListItem::new(Line::from(vec![
                    Span::styled("\u{25cf} ", Theme::error()),
                    Span::styled(&row.subdomain, Style::default().fg(Theme::ERROR)),
                    Span::styled(
                        " [d to confirm]",
                        Style::default()
                            .fg(Theme::ERROR)
                            .add_modifier(Modifier::DIM),
                    ),
                ]))
            } else {
                ListItem::new(Line::from(vec![
                    Span::styled("\u{25cf} ", Theme::success()),
                    Span::styled(&row.subdomain, Theme::text()),
                    Span::raw("  "),
                    Span::styled(&row.age, Theme::text_dim()),
                ]))
            }
        })
        .collect();

    let block = Block::default()
        .title(title_spans(&state.data))
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

fn render_detail(frame: &mut Frame, area: Rect, state: &TuiState) {
    let block = Block::default()
        .title(Line::from(vec![Span::styled(
            " details ",
            Theme::accent_bold(),
        )]))
        .borders(Borders::ALL)
        .border_style(Theme::border());

    let Some(row) = state.selected() else {
        frame.render_widget(Paragraph::new("").block(block), area);
        return;
    };

    let mut lines = vec![
        Line::from(vec![
            Span::styled("Subdomain   ", Theme::text_dim()),
            Span::styled(&row.subdomain, Theme::accent_bold()),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled("URL         ", Theme::text_dim()),
            Span::styled(format!("https://{}.xpo.sh", row.subdomain), Theme::text()),
        ]),
        Line::from(vec![
            Span::styled("Reserved    ", Theme::text_dim()),
            Span::styled(&row.age, Theme::text()),
        ]),
        Line::from(vec![
            Span::styled("Status      ", Theme::text_dim()),
            Span::styled("\u{25cf} ", Theme::success()),
            Span::styled("reserved", Theme::success()),
        ]),
    ];

    if let Some((msg, _)) = &state.status_msg {
        lines.push(Line::raw(""));
        lines.push(Line::from(vec![Span::styled(
            msg.as_str(),
            if msg.starts_with("Error") {
                Theme::error()
            } else {
                Theme::success()
            },
        )]));
    }

    if state.confirm_delete.is_some() {
        lines.push(Line::raw(""));
        lines.push(Line::from(vec![Span::styled(
            "Press d again to confirm delete",
            Theme::error(),
        )]));
    }

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn render_keybinds(frame: &mut Frame, area: Rect, state: &TuiState) {
    let mut spans = vec![
        Span::styled("q", Theme::accent()),
        Span::styled(":quit  ", Theme::text_dim()),
        Span::styled("\u{2191}\u{2193}", Theme::accent()),
        Span::styled(":navigate  ", Theme::text_dim()),
        Span::styled("d", Theme::accent()),
        Span::styled(":delete  ", Theme::text_dim()),
        Span::styled("r", Theme::accent()),
        Span::styled(":refresh", Theme::text_dim()),
    ];

    if let Some((msg, _)) = &state.status_msg {
        spans.push(Span::styled("  ", Theme::text_dim()));
        spans.push(Span::styled(
            msg.as_str(),
            if msg.starts_with("Error") {
                Theme::error()
            } else {
                Theme::success()
            },
        ));
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}
