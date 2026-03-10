use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, BorderType, Borders, Cell, Padding, Row, Table};
use ratatui::Frame;
use time::macros::format_description;

use crate::model::TuiState;
use crate::theme::Theme;

pub fn render(frame: &mut Frame, area: Rect, state: &TuiState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Theme::border())
        .title(Span::styled("Requests", Theme::accent_bold()))
        .padding(Padding::horizontal(1));

    let visible = state.visible_requests();

    let header = Row::new(vec![
        Cell::from(Span::styled("TIME", Theme::text_dim())),
        Cell::from(Span::styled("METHOD", Theme::text_dim())),
        Cell::from(Span::styled("PATH", Theme::text_dim())),
        Cell::from(Span::styled("STATUS", Theme::text_dim())),
    ]);

    let time_fmt = format_description!("[hour]:[minute]:[second]");

    let highlight_style = Style::default()
        .bg(Theme::ACCENT)
        .fg(ratatui::style::Color::Black)
        .add_modifier(Modifier::BOLD);

    let rows: Vec<Row> = visible
        .iter()
        .map(|(_, req, is_selected)| {
            let time_str = req
                .timestamp
                .format(&time_fmt)
                .unwrap_or_else(|_| "--:--:--".to_string());

            let row = Row::new(vec![
                Cell::from(Span::styled(time_str, Theme::text_dim())),
                Cell::from(Span::styled(&req.method, Theme::method_style(&req.method))),
                Cell::from(Span::styled(&req.path, Theme::text())),
                Cell::from(Span::styled(
                    format!("{} ({}ms)", req.status, req.duration_ms),
                    Theme::status_style(req.status),
                )),
            ]);

            if *is_selected {
                row.style(highlight_style)
            } else {
                row
            }
        })
        .collect();

    let widths = [
        Constraint::Length(10),
        Constraint::Length(8),
        Constraint::Fill(1),
        Constraint::Length(14),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .column_spacing(1)
        .block(block);

    frame.render_widget(table, area);
}
