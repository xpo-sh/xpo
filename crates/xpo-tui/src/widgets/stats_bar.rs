use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::model::TuiState;
use crate::theme::Theme;

pub fn render(frame: &mut Frame, area: Rect, state: &TuiState) {
    let avg = state.avg_duration_ms();
    let rate = state.success_rate();

    let rate_style = if rate < 50 {
        Theme::error()
    } else {
        Theme::success()
    };

    let mut spans = vec![
        Span::styled("  ", Theme::text_dim()),
        Span::styled(
            format!("\u{2191}{}", state.total_requests),
            Theme::success(),
        ),
        Span::styled(" req", Theme::text_dim()),
        Span::styled("  \u{2502}  ", Theme::border()),
        Span::styled(format!("\u{2298}{}ms", avg), Theme::accent()),
        Span::styled(" avg", Theme::text_dim()),
        Span::styled("  \u{2502}  ", Theme::border()),
        Span::styled(format!("\u{2713}{}%", rate), rate_style),
        Span::styled(" ok", Theme::text_dim()),
    ];

    if !state.filter_text.is_empty() {
        spans.push(Span::styled("  \u{2502}  ", Theme::border()));
        spans.push(Span::styled(
            format!("[filter: {}]", state.filter_text),
            Theme::accent(),
        ));
    }

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line);
    frame.render_widget(paragraph, area);
}
