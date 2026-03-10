use std::io::{self, stdout};
use std::time::{Duration, Instant};

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
    Terminal, TerminalOptions, Viewport,
};

use crate::widgets::list_table::{self, ListRow, ListTuiState};

const REFRESH_INTERVAL: Duration = Duration::from_secs(5);

pub fn run<F>(initial_rows: Vec<ListRow>, mut refresh_fn: F) -> io::Result<()>
where
    F: FnMut() -> Vec<ListRow>,
{
    enable_raw_mode()?;
    let (_, term_rows) = crossterm::terminal::size()?;
    let height = std::cmp::min(term_rows.saturating_sub(2), 16);
    let mut terminal = Terminal::with_options(
        CrosstermBackend::new(stdout()),
        TerminalOptions {
            viewport: Viewport::Inline(height),
        },
    )?;

    let mut state = ListTuiState::new(initial_rows);
    let mut last_refresh = Instant::now();

    loop {
        terminal.draw(|frame| {
            let chunks =
                Layout::vertical([Constraint::Min(3), Constraint::Length(1)]).split(frame.area());

            list_table::render(frame, chunks[0], &mut state);
            list_table::render_keybinds(frame, chunks[1]);
        })?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(KeyEvent {
                code, modifiers, ..
            }) = event::read()?
            {
                match code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => break,
                    KeyCode::Up | KeyCode::Char('k') => state.previous(),
                    KeyCode::Down | KeyCode::Char('j') => state.next(),
                    KeyCode::Char('r') => {
                        let selected = state.list_state.selected();
                        state = ListTuiState::new(refresh_fn());
                        if let Some(idx) = selected {
                            if idx < state.items.len() {
                                state.list_state.select(Some(idx));
                            }
                        }
                        last_refresh = Instant::now();
                    }
                    _ => {}
                }
            }
        }

        if last_refresh.elapsed() >= REFRESH_INTERVAL {
            let selected = state.list_state.selected();
            state = ListTuiState::new(refresh_fn());
            if let Some(idx) = selected {
                if idx < state.items.len() {
                    state.list_state.select(Some(idx));
                }
            }
            last_refresh = Instant::now();
        }
    }

    disable_raw_mode()?;
    terminal.clear()?;
    Ok(())
}
