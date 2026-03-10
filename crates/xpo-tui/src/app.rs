use std::io::{self, stdout};
use std::sync::mpsc;

use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use ratatui::prelude::CrosstermBackend;
use ratatui::{Terminal, TerminalOptions, Viewport};

use crate::event::{create_event_channel, AppEvent, EventHandler};
use crate::model::{TuiState, ViewMode};

pub struct BannerInfo {
    pub title: String,
    pub url: String,
    pub target: String,
    pub extra_lines: Vec<String>,
    pub has_qr: bool,
    pub qr_url: Option<String>,
}

pub struct TuiApp {
    pub state: TuiState,
    pub banner: BannerInfo,
    pub should_quit: bool,
    pub start_time: std::time::Instant,
    pub ttl_deadline: Option<std::time::Instant>,
}

const MIN_COLS: u16 = 60;
const MIN_ROWS: u16 = 15;

impl TuiApp {
    pub fn new(banner: BannerInfo, max_requests: usize, visible_rows: usize) -> Self {
        Self {
            state: TuiState::new(max_requests, visible_rows),
            banner,
            should_quit: false,
            start_time: std::time::Instant::now(),
            ttl_deadline: None,
        }
    }

    pub fn check_terminal_size() -> bool {
        if let Ok((cols, rows)) = crossterm::terminal::size() {
            cols >= MIN_COLS && rows >= MIN_ROWS
        } else {
            false
        }
    }

    pub fn init_terminal() -> io::Result<Terminal<CrosstermBackend<io::Stdout>>> {
        enable_raw_mode()?;
        let (_, rows) = crossterm::terminal::size()?;
        let height = std::cmp::min(rows.saturating_sub(2), 24);
        let terminal = Terminal::with_options(
            CrosstermBackend::new(stdout()),
            TerminalOptions {
                viewport: Viewport::Inline(height),
            },
        )?;
        Ok(terminal)
    }

    pub fn restore_terminal() {
        let _ = disable_raw_mode();
    }

    pub fn create_channel() -> (mpsc::Sender<AppEvent>, EventHandler) {
        create_event_channel()
    }

    pub fn handle_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::Key(key) => self.handle_key(key),
            AppEvent::Mouse(mouse) => self.handle_mouse(mouse),
            AppEvent::Tick => self.state.tick(),
            AppEvent::Resize(_, _) => {}
            AppEvent::Request(req) => self.state.push_request(req),
            AppEvent::Connection(status) => self.state.conn_status = status,
            AppEvent::PfStatus(_active) => {}
            AppEvent::TtlDeadline(deadline) => {
                self.ttl_deadline = Some(deadline);
            }
        }
    }

    fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }

        match self.state.view_mode {
            ViewMode::Filter => self.handle_filter_key(key),
            ViewMode::Help => self.handle_help_key(key),
            _ => self.handle_normal_key(key),
        }
    }

    fn handle_normal_key(&mut self, key: crossterm::event::KeyEvent) {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('f') => self.state.view_mode = ViewMode::Filter,
            KeyCode::Char('x') => self.state.clear(),
            KeyCode::Char('?') => self.state.view_mode = ViewMode::Help,
            KeyCode::Up | KeyCode::Char('k') => self.state.select_up(),
            KeyCode::Down | KeyCode::Char('j') => self.state.select_down(),
            _ => {}
        }
    }

    fn handle_filter_key(&mut self, key: crossterm::event::KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.state.filter_text.clear();
                self.state.view_mode = ViewMode::Normal;
            }
            KeyCode::Enter => {
                self.state.view_mode = ViewMode::Normal;
            }
            KeyCode::Backspace => {
                if self.state.filter_text.is_empty() {
                    self.state.view_mode = ViewMode::Normal;
                } else {
                    self.state.filter_text.pop();
                }
            }
            KeyCode::Char(c) => {
                self.state.filter_text.push(c);
            }
            _ => {}
        }
    }

    fn handle_help_key(&mut self, key: crossterm::event::KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') => {
                self.state.view_mode = ViewMode::Normal;
            }
            _ => {}
        }
    }

    fn handle_mouse(&mut self, mouse: crossterm::event::MouseEvent) {
        match mouse.kind {
            MouseEventKind::ScrollUp => self.state.select_up(),
            MouseEventKind::ScrollDown => self.state.select_down(),
            MouseEventKind::Down(MouseButton::Left) => {}
            _ => {}
        }
    }

    pub fn summary_line(&self) -> String {
        let elapsed = self.start_time.elapsed();
        let mins = elapsed.as_secs() / 60;
        let secs = elapsed.as_secs() % 60;
        format!(
            "  \x1b[32;1m\u{2713}\x1b[0m Tunnel closed. {} requests served in {}m {}s.",
            self.state.total_requests, mins, secs
        )
    }
}
