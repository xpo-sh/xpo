use std::io::{self, stdout};
use std::time::Duration;

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

pub fn run(rows: Vec<ListRow>) -> io::Result<()> {
    enable_raw_mode()?;
    let (_, term_rows) = crossterm::terminal::size()?;
    let height = std::cmp::min(term_rows.saturating_sub(2), 16);
    let mut terminal = Terminal::with_options(
        CrosstermBackend::new(stdout()),
        TerminalOptions {
            viewport: Viewport::Inline(height),
        },
    )?;

    let mut state = ListTuiState::new(rows);

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
                    _ => {}
                }
            }
        }
    }

    disable_raw_mode()?;
    terminal.clear()?;
    Ok(())
}
