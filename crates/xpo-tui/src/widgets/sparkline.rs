use std::collections::VecDeque;

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::Sparkline;
use ratatui::Frame;

use crate::theme::Theme;

pub fn render(frame: &mut Frame, area: Rect, data: &VecDeque<u64>) {
    let values: Vec<u64> = data.iter().copied().collect();
    let sparkline = Sparkline::default()
        .data(&values)
        .style(Style::default().fg(Theme::SPARKLINE));
    frame.render_widget(sparkline, area);
}
