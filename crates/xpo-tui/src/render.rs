use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::Frame;

use crate::app::TuiApp;
use crate::model::ViewMode;
use crate::widgets::{banner, help_overlay, keybinds, log_table, qr_panel};

pub fn draw(frame: &mut Frame, app: &TuiApp) {
    let area = frame.area();

    let banner_height = 5 + app.banner.extra_lines.len() as u16;
    let log_height = app.state.visible_rows as u16 + 3;

    let has_qr = app.banner.has_qr;
    let qr_url;
    let qr_width;
    let qr_height;

    if has_qr {
        qr_url = app
            .banner
            .qr_url
            .as_deref()
            .unwrap_or(&app.banner.url)
            .to_string();
        let w = qr_panel::required_width(&qr_url);
        let h = qr_panel::required_height(&qr_url);
        if w > 0 && area.width > w + 40 {
            qr_width = w;
            qr_height = h;
        } else {
            qr_width = 0;
            qr_height = 0;
        }
    } else {
        qr_url = String::new();
        qr_width = 0;
        qr_height = 0;
    }

    let content_height = if qr_width > 0 {
        log_height.max(qr_height)
    } else {
        log_height
    };

    let vert_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(banner_height),
            Constraint::Length(content_height),
            Constraint::Length(1),
            Constraint::Fill(1),
        ])
        .split(area);

    let banner_area = vert_layout[0];
    let content_area = vert_layout[1];
    let footer_area = vert_layout[2];

    banner::render(frame, banner_area, &app.banner, &app.state);

    if qr_width > 0 {
        let h_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(qr_width), Constraint::Fill(1)])
            .split(content_area);

        qr_panel::render(frame, h_layout[0], &qr_url);
        log_table::render(frame, h_layout[1], &app.state);
    } else {
        log_table::render(frame, content_area, &app.state);
    }

    render_footer(frame, footer_area, app);

    if app.state.view_mode == ViewMode::Help {
        help_overlay::render(frame, area);
    }
}

fn render_footer(frame: &mut Frame, area: Rect, app: &TuiApp) {
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
        scroll_info,
    );
}
