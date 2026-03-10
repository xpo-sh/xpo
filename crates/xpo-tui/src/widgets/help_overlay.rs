use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::theme::Theme;

pub fn render(frame: &mut Frame, area: Rect) {
    let overlay = centered_rect(50, 14, area);

    frame.render_widget(Clear, overlay);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Theme::border())
        .title(Span::styled("Keybindings", Theme::accent_bold()));

    let bindings = vec![
        ("q / Ctrl+C", "Quit"),
        ("f", "Filter requests"),
        ("x", "Clear log"),
        ("d / Enter", "Request detail"),
        ("r", "Toggle QR code"),
        ("\u{2191}\u{2193} / jk", "Scroll"),
        ("ESC", "Back / Close"),
        ("?", "This help"),
        ("Mouse", "Scroll / Click"),
    ];

    let lines: Vec<Line> = bindings
        .into_iter()
        .map(|(key, action)| {
            Line::from(vec![
                Span::styled(format!("  {:>14}", key), Theme::accent_bold()),
                Span::styled(format!("  {}", action), Theme::text()),
            ])
        })
        .collect();

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, overlay);
}

fn centered_rect(percent_x: u16, height: u16, area: Rect) -> Rect {
    let width = area.width * percent_x / 100;
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let h = height.min(area.height);
    let w = width.min(area.width);
    Rect::new(x, y, w, h)
}
