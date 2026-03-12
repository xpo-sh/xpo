use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Padding, Paragraph};
use ratatui::Frame;

use crate::model::{PanelFocus, RequestLog, TuiState};
use crate::theme::Theme;

pub fn content_line_count(req: &RequestLog) -> usize {
    build_detail_lines(req, true).len()
}

pub fn render(frame: &mut Frame, area: Rect, state: &TuiState, is_share: bool, is_pro: bool) {
    let is_focused = state.focus == PanelFocus::Detail;
    let border_style = if is_focused {
        Theme::accent()
    } else {
        Theme::border()
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border_style)
        .title(Span::styled("Detail", Theme::accent_bold()))
        .padding(Padding::horizontal(1));

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

    let show_body = !is_share || is_pro;
    let lines = build_detail_lines(req, show_body);

    let paragraph = Paragraph::new(lines).scroll((state.detail_scroll as u16, 0));
    frame.render_widget(paragraph, inner);
}

fn build_detail_lines(req: &RequestLog, show_body: bool) -> Vec<Line<'static>> {
    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(vec![
        Span::styled(req.method.clone(), Theme::method_style(&req.method)),
        Span::styled(" ", Theme::text()),
        Span::styled(req.path.clone(), Theme::text()),
        Span::styled(" HTTP/1.1", Theme::text_dim()),
    ]));

    let status_text = format!("{}", req.status);
    let size_text = if req.body_size > 0 {
        format!(" - {}", format_bytes(req.body_size))
    } else {
        String::new()
    };
    lines.push(Line::from(vec![
        Span::styled(status_text, Theme::status_style(req.status)),
        Span::styled(format!(" - {}ms", req.duration_ms), Theme::text_dim()),
        Span::styled(size_text, Theme::text_dim()),
    ]));

    lines.push(Line::default());

    if req.request_headers.is_empty() {
        lines.push(Line::from(Span::styled(
            "--- Request Headers --- (none)",
            Theme::text_dim(),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "--- Request Headers ---",
            Theme::accent(),
        )));
        for (key, value) in &req.request_headers {
            lines.push(Line::from(vec![
                Span::styled(format!("{}: ", key), Theme::text_dim()),
                Span::styled(value.clone(), Theme::text()),
            ]));
        }
    }

    lines.push(Line::default());

    if req.response_headers.is_empty() {
        lines.push(Line::from(Span::styled(
            "--- Response Headers --- (none)",
            Theme::text_dim(),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "--- Response Headers ---",
            Theme::accent(),
        )));
        for (key, value) in &req.response_headers {
            lines.push(Line::from(vec![
                Span::styled(format!("{}: ", key), Theme::text_dim()),
                Span::styled(value.clone(), Theme::text()),
            ]));
        }
    }

    lines.push(Line::default());

    if let Some(body) = &req.body_preview {
        if show_body {
            let ext = xpo_core::content_type_to_extension(&req.response_headers);
            lines.push(Line::from(Span::styled(
                format!(
                    "--- Body ({} - {}) ---",
                    format_bytes(req.body_size),
                    &ext[1..]
                ),
                Theme::accent(),
            )));
            for body_line in body.lines() {
                lines.push(Line::from(Span::styled(
                    body_line.to_string(),
                    Theme::text(),
                )));
            }
        } else {
            lines.push(Line::from(Span::styled(
                "\u{2500}".repeat(40),
                Theme::border(),
            )));
            lines.push(Line::from(Span::styled(
                "Body inspection available with Pro",
                Theme::text_dim(),
            )));
            lines.push(Line::from(Span::styled("https://xpo.sh", Theme::accent())));
        }
    }

    lines
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_048_576 {
        format!("{:.1}M", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1}K", bytes as f64 / 1024.0)
    } else {
        format!("{}", bytes)
    }
}
