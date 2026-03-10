use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::Frame;

use crate::app::TuiApp;
use crate::model::ViewMode;
use crate::widgets::{banner, help_overlay, keybinds, log_table, qr_panel};

pub fn draw(frame: &mut Frame, app: &TuiApp) {
    let area = frame.area();

    let banner_height = 5 + app.banner.extra_lines.len() as u16;
    let log_height = app.state.visible_rows as u16 + 1;

    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(banner_height),
            Constraint::Length(log_height),
            Constraint::Length(1),
            Constraint::Fill(1),
        ])
        .split(area);

    let banner_area = main_layout[0];
    let log_area = main_layout[1];
    let footer_area = main_layout[2];

    render_banner(frame, banner_area, app);
    log_table::render(frame, log_area, &app.state);
    render_footer(frame, footer_area, app);

    if app.state.view_mode == ViewMode::Help {
        help_overlay::render(frame, area);
    }
}

fn render_banner(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let show_qr = app.state.show_qr && app.banner.has_qr;

    if show_qr {
        let qr_url = app.banner.qr_url.as_deref().unwrap_or(&app.banner.url);
        let qr_width = qr_panel::required_width(qr_url);

        if qr_width > 0 && area.width > qr_width + 20 {
            let layout = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Fill(1), Constraint::Length(qr_width)])
                .split(area);

            banner::render(frame, layout[0], &app.banner, &app.state);
            qr_panel::render(frame, layout[1], qr_url);
            return;
        }
    }

    banner::render(frame, area, &app.banner, &app.state);
}

fn render_footer(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let has_qr = app.banner.has_qr;
    let total = app.state.filtered_requests().len();
    let scroll_info = if total > 0 {
        Some((app.state.selected, app.state.scroll_offset, total))
    } else {
        None
    };
    keybinds::render(
        frame,
        area,
        &app.state.view_mode,
        &app.state.filter_text,
        has_qr,
        scroll_info,
    );
}
