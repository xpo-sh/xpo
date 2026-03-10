use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use ratatui::Frame;

use crate::model::{RequestLog, TuiState};
use crate::theme::Theme;

pub fn content_line_count(req: &RequestLog) -> usize {
    build_detail_lines(req).len()
}

pub fn render(frame: &mut Frame, area: Rect, state: &TuiState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Theme::border())
        .title(Span::styled("Request Detail", Theme::accent_bold()));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let filtered = state.filtered_requests();
    let entry = filtered.get(state.selected).map(|(_, req)| *req);

    let Some(req) = entry else {
        let empty = Paragraph::new(Line::from(Span::styled(
            "No request selected",
            Theme::text_dim(),
        )));
        frame.render_widget(empty, inner);
        return;
    };

    let lines = build_detail_lines(req);

    let paragraph = Paragraph::new(lines).scroll((state.scroll_offset as u16, 0));
    frame.render_widget(paragraph, inner);
}

fn build_detail_lines(req: &RequestLog) -> Vec<Line<'_>> {
    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(vec![
        Span::styled(&req.method, Theme::method_style(&req.method)),
        Span::styled(" ", Theme::text()),
        Span::styled(&req.path, Theme::text()),
        Span::styled(" HTTP/1.1", Theme::text_dim()),
    ]));

    lines.push(Line::default());

    for (key, value) in &req.request_headers {
        lines.push(Line::from(vec![
            Span::styled(format!("{}: ", key), Theme::text_dim()),
            Span::styled(value, Theme::text()),
        ]));
    }

    lines.push(Line::default());

    lines.push(Line::from(vec![
        Span::styled("Response: ", Theme::text_dim()),
        Span::styled(format!("{}", req.status), Theme::status_style(req.status)),
        Span::styled(format!(" ({}ms)", req.duration_ms), Theme::text_dim()),
    ]));

    for (key, value) in &req.response_headers {
        lines.push(Line::from(vec![
            Span::styled(format!("{}: ", key), Theme::text_dim()),
            Span::styled(value, Theme::text()),
        ]));
    }

    lines.push(Line::default());

    lines.push(Line::from(Span::styled(
        format!("Timing: TTFB {}ms", req.duration_ms),
        Theme::text_dim(),
    )));

    if let Some(body) = &req.body_preview {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            format!("Body ({} bytes):", req.body_size),
            Theme::text_dim(),
        )));

        for body_line in body.lines().take(20) {
            lines.push(Line::from(Span::styled(body_line, Theme::text())));
        }
    }

    lines
}
