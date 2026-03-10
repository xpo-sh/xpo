use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::model::ViewMode;
use crate::theme::Theme;

pub fn render(
    frame: &mut Frame,
    area: Rect,
    view_mode: &ViewMode,
    filter_text: &str,
    scroll_info: Option<(usize, usize, usize)>,
) {
    let line = match view_mode {
        ViewMode::Normal => {
            let mut spans = vec![
                Span::styled("q", Theme::accent_bold()),
                Span::styled(":quit  ", Theme::text_dim()),
                Span::styled("\u{2191}\u{2193}", Theme::accent_bold()),
                Span::styled(":scroll  ", Theme::text_dim()),
                Span::styled("f", Theme::accent_bold()),
                Span::styled(":filter  ", Theme::text_dim()),
                Span::styled("x", Theme::accent_bold()),
                Span::styled(":clear  ", Theme::text_dim()),
                Span::styled("?", Theme::accent_bold()),
                Span::styled(":help", Theme::text_dim()),
            ];
            if let Some((selected, _offset, total)) = scroll_info {
                if total > 0 {
                    spans.push(Span::styled(
                        format!("  [{}/{}]", selected + 1, total),
                        Theme::text_dim(),
                    ));
                }
            }
            Line::from(spans)
        }
        ViewMode::Filter => Line::from(vec![
            Span::styled("Filter: ", Theme::accent()),
            Span::styled(filter_text, Theme::text()),
            Span::styled("\u{2588}", Theme::accent()),
        ]),
        _ => Line::default(),
    };

    let paragraph = Paragraph::new(line);
    frame.render_widget(paragraph, area);
}
