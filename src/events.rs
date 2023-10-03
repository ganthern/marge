use std::convert::Infallible;

use anyhow::anyhow;
use crossterm::event::{ Event, EventStream, KeyCode, KeyEvent, KeyModifiers,
};
use futures::{
    future::{self, FutureExt},
    select, StreamExt,
};

use futures_timer::Delay;
use tokio::signal::unix;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::time::Duration;
use tokio_stream::wrappers::SignalStream;

#[derive(Debug)]
pub enum AppEvent {
    Input(KeyEvent),
    Signal,
    Error(anyhow::Error),
    Tick,
}

pub struct EventPump {
    rx: Receiver<AppEvent>,
    // Need to be kept around to prevent disposing the sender side.
    _tx: Sender<AppEvent>,
}

impl EventPump {
    pub fn new(tick_rate: Duration) -> EventPump {
        let (tx, rx) = channel(10);
        let sent_tx = tx.clone();
        tokio::spawn(async move {
            let result = poll_events(tick_rate, &sent_tx).await;
            let err = result.err().unwrap();
            let _ = sent_tx.send(AppEvent::Error(err)).await;
        });
        EventPump { rx, _tx: tx }
    }

    /// Attempts to read an event.
    /// This function block the current thread.
    pub async fn next(&mut self) -> Option<AppEvent> {
        self.rx.recv().await
    }
}

async fn poll_events(tick_rate: Duration, tx: &Sender<AppEvent>) -> anyhow::Result<Infallible> {
    let millis = tick_rate.as_millis() as u64;
    let mut reader = EventStream::new().filter_map(|e| {
        future::ready(match e {
            Ok(Event::Key(key_event)) => Some(Ok(key_event)),
            Err(e) => Some(Err(e)),
            _ => None,
        })
    });
    let mut signal_int = SignalStream::new(unix::signal(unix::SignalKind::interrupt())?);
    let mut signal_quit = SignalStream::new(unix::signal(unix::SignalKind::quit())?);
    let mut signal_term = SignalStream::new(unix::signal(unix::SignalKind::terminate())?);

    let mut since_last_tick = 0;

    let last_e = loop {
        while since_last_tick > millis {
            since_last_tick = since_last_tick .saturating_sub(millis)
        }
        let next_tick_in = millis.saturating_sub(since_last_tick);
        let start = std::time::Instant::now();
        let mut delay = Delay::new(Duration::from_millis(next_tick_in)).fuse();
        let mut sigint = signal_int.next().fuse();
        let mut sigquit = signal_quit.next().fuse();
        let mut sigterm = signal_term.next().fuse();
        let mut event = reader.next().fuse();

        let e: AppEvent = select! {
            _ = delay => {
                AppEvent::Tick
            },
            maybe_event = event => {
                match maybe_event {
                    Some(Ok(key_event)) => map_event(key_event),
                    Some(Err(e)) => break Err(anyhow!(e)),
                    None => break Err(anyhow!("none in event stream!")),
                }
            },
            maybe_sigint = sigint => {
                match maybe_sigint {
                Some(()) => AppEvent::Signal,
                None => break Err(anyhow!("none in sigint stream!"))
                }
            },
            maybe_sigquit = sigquit => {
                match maybe_sigquit {
                    Some(()) => AppEvent::Signal,
                    None => break Err(anyhow!("none in sigquit stream!"))
                }
            },
            maybe_sigterm = sigterm => {
                match maybe_sigterm {
                    Some(()) => AppEvent::Signal,
                    None => break Err(anyhow!("none in sigterm stream!"))
                }
            }
        };
        since_last_tick = if let AppEvent::Tick = e {
            0
        } else {
            let elapsed = start.elapsed().as_millis() as u64;
            since_last_tick.saturating_add(elapsed)
        };
        tx.send(e).await?;
    };
    last_e
}

fn map_event(key_event: KeyEvent) -> AppEvent {
    match key_event {
        KeyEvent {
            code: KeyCode::Char('d'),
            modifiers: KeyModifiers::CONTROL,
            ..
        } => AppEvent::Signal,
        KeyEvent {
            code: KeyCode::Char('c'),
            modifiers: KeyModifiers::CONTROL,
            ..
        } => AppEvent::Signal,
        _ => AppEvent::Input(key_event),
    }
}
