use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Padding, Paragraph};
use ratatui::Frame;

use crate::app::BannerInfo;
use crate::model::{ConnStatus, TuiState};
use crate::theme::Theme;

pub fn render(frame: &mut Frame, area: Rect, banner: &BannerInfo, state: &TuiState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Theme::border())
        .title(Span::styled(&banner.title, Theme::accent_bold()))
        .padding(Padding::horizontal(1));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = Vec::new();

    let url_line = Line::from(vec![
        Span::styled(&banner.url, Theme::accent()),
        Span::styled(" -> ", Theme::text_dim()),
        Span::styled(&banner.target, Theme::text()),
    ]);
    lines.push(url_line);

    let avg = state.avg_duration_ms();
    let rate = state.success_rate();
    let stats_line = Line::from(vec![
        Span::styled(
            format!("{} requests", state.total_requests),
            Theme::success(),
        ),
        Span::styled("  ", Style::default()),
        Span::styled(format!("{}ms avg", avg), Theme::accent()),
        Span::styled("  ", Style::default()),
        Span::styled(format!("{}% ok", rate), Theme::success()),
    ]);
    lines.push(stats_line);

    let status_line = match &state.conn_status {
        ConnStatus::Connected => Line::from(vec![
            Span::styled("\u{25cf} ", Theme::success()),
            Span::styled("Connected", Theme::success()),
        ]),
        ConnStatus::Reconnecting {
            attempt,
            next_retry_secs,
        } => Line::from(vec![
            Span::styled(
                "\u{25cf} ",
                Style::default().fg(ratatui::style::Color::Yellow),
            ),
            Span::styled(
                format!(
                    "Reconnecting... (attempt {}, retry in {}s)",
                    attempt, next_retry_secs
                ),
                Style::default().fg(ratatui::style::Color::Yellow),
            ),
        ]),
        ConnStatus::Disconnected { reason } => Line::from(vec![
            Span::styled("\u{25cf} ", Theme::error()),
            Span::styled(format!("Disconnected: {}", reason), Theme::error()),
        ]),
    };
    lines.push(status_line);

    for extra in &banner.extra_lines {
        lines.push(Line::from(Span::styled(extra, Theme::text_dim())));
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}
