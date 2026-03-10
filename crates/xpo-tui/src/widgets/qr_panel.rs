use fast_qr::qr::QRBuilder;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use ratatui::Frame;

use crate::theme::Theme;

pub fn render(frame: &mut Frame, area: Rect, url: &str) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Theme::border())
        .title(Span::styled("QR", Theme::accent_bold()));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let qr = match QRBuilder::new(url).build() {
        Ok(qr) => qr,
        Err(_) => return,
    };

    let size = qr.size;
    let module_style = Style::default().fg(Theme::QR_MODULE);
    let empty_style = Style::default();

    let mut lines: Vec<Line> = Vec::new();

    let mut y = 0;
    let end = size;
    while y < end {
        let mut spans: Vec<Span> = Vec::new();
        for x in 0..size {
            let top = qr[y][x].value();
            let bottom = if y + 1 < size {
                qr[y + 1][x].value()
            } else {
                false
            };

            match (top, bottom) {
                (true, true) => spans.push(Span::styled("\u{2588}", module_style)),
                (true, false) => spans.push(Span::styled("\u{2580}", module_style)),
                (false, true) => spans.push(Span::styled("\u{2584}", module_style)),
                (false, false) => spans.push(Span::styled(" ", empty_style)),
            }
        }
        lines.push(Line::from(spans));
        y += 2;
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

pub fn required_width(url: &str) -> u16 {
    match QRBuilder::new(url).build() {
        Ok(qr) => qr.size as u16 + 2,
        Err(_) => 0,
    }
}

pub fn required_height(url: &str) -> u16 {
    match QRBuilder::new(url).build() {
        Ok(qr) => (qr.size as u16).div_ceil(2) + 2,
        Err(_) => 0,
    }
}
