use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::model::{PanelFocus, ViewMode};
use crate::theme::Theme;

pub fn render(
    frame: &mut Frame,
    area: Rect,
    view_mode: &ViewMode,
    filter_text: &str,
    scroll_info: Option<(usize, usize, usize)>,
    focus: &PanelFocus,
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
                Span::styled("  ", Theme::text_dim()),
                Span::styled("Enter", Theme::accent_bold()),
                Span::styled(":detail", Theme::text_dim()),
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
        ViewMode::Detail => {
            let (cols, _) = crossterm::terminal::size().unwrap_or((80, 24));
            let is_wide = cols >= 100;
            let mut spans = vec![
                Span::styled("Esc", Theme::accent_bold()),
                Span::styled(":back  ", Theme::text_dim()),
                Span::styled("\u{2191}\u{2193}/j/k", Theme::accent_bold()),
                Span::styled(":scroll  ", Theme::text_dim()),
            ];
            if is_wide {
                spans.push(Span::styled("Tab", Theme::accent_bold()));
                spans.push(Span::styled(":focus  ", Theme::text_dim()));
                let focus_label = match focus {
                    PanelFocus::LogTable => "[log]",
                    PanelFocus::Detail => "[detail]",
                };
                spans.push(Span::styled(focus_label, Theme::text_dim()));
            }
            Line::from(spans)
        }
        ViewMode::Help => Line::default(),
    };

    let paragraph = Paragraph::new(line);
    frame.render_widget(paragraph, area);
}
