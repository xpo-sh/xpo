use crossterm::event::{self, Event, KeyEvent, MouseEvent};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::model::{ConnStatus, RequestLog};

pub enum AppEvent {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Tick,
    Resize(u16, u16),
    Request(RequestLog),
    Connection(ConnStatus),
}

pub struct EventHandler {
    rx: mpsc::Receiver<AppEvent>,
    _input_thread: thread::JoinHandle<()>,
}

impl EventHandler {
    pub fn next(&self) -> Result<AppEvent, mpsc::RecvError> {
        self.rx.recv()
    }
}

pub fn create_event_channel() -> (mpsc::Sender<AppEvent>, EventHandler) {
    let (app_tx, app_rx) = mpsc::channel();
    let tick_rate = Duration::from_millis(100);

    let input_tx = app_tx.clone();
    let input_thread = thread::spawn(move || {
        let mut last_tick = Instant::now();
        loop {
            let timeout = tick_rate.saturating_sub(last_tick.elapsed());
            if event::poll(timeout).unwrap_or(false) {
                match event::read() {
                    Ok(Event::Key(key)) => {
                        if input_tx.send(AppEvent::Key(key)).is_err() {
                            return;
                        }
                    }
                    Ok(Event::Mouse(mouse)) => {
                        if input_tx.send(AppEvent::Mouse(mouse)).is_err() {
                            return;
                        }
                    }
                    Ok(Event::Resize(w, h)) => {
                        if input_tx.send(AppEvent::Resize(w, h)).is_err() {
                            return;
                        }
                    }
                    _ => {}
                }
            }
            if last_tick.elapsed() >= tick_rate {
                if input_tx.send(AppEvent::Tick).is_err() {
                    return;
                }
                last_tick = Instant::now();
            }
        }
    });

    let handler = EventHandler {
        rx: app_rx,
        _input_thread: input_thread,
    };

    (app_tx, handler)
}
