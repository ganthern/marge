use std::convert::Infallible;

use anyhow::{anyhow, Context};
use crossterm::{
    cursor::position,
    event::{read, Event, EventStream, KeyCode, KeyEvent},
};
use futures::{
    future::{self, FutureExt},
    select, StreamExt,
};
use futures_timer::Delay;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::time::Duration;

#[derive(Debug)]
pub enum InputEvent {
    Input(KeyEvent),
    Error(anyhow::Error),
    Tick,
}

pub struct EventPump {
    rx: Receiver<InputEvent>,
    // Need to be kept around to prevent disposing the sender side.
    _tx: Sender<InputEvent>,
}

impl EventPump {
    pub fn new(tick_rate: Duration) -> EventPump {
        let (tx, rx) = channel(10);
        let sent_tx = tx.clone();
        tokio::spawn(async move {
            let result = poll_events(tick_rate, &sent_tx).await;
            let err = result.err().unwrap();
            let _ = sent_tx.send(InputEvent::Error(err)).await;
        });
        EventPump { rx, _tx: tx }
    }

    /// Attempts to read an event.
    /// This function block the current thread.
    pub async fn next(&mut self) -> Option<InputEvent> {
        self.rx.recv().await
    }
}

async fn poll_events(tick_rate: Duration, tx: &Sender<InputEvent>) -> anyhow::Result<Infallible> {
    let millis = tick_rate.as_millis() as u64;
    let mut reader = EventStream::new().filter_map(|e| future::ready(match e {
        Ok(Event::Key(key_event)) => Some(Ok(key_event)),
        Err(e) => Some(Err(e)),
        _ => None,
    }));

    let last_e = loop {
        let mut delay = Delay::new(Duration::from_millis(millis)).fuse();
        let mut event = reader.next().fuse();

        let e: InputEvent = select! {
            _ = delay => InputEvent::Tick,
            maybe_event = event => {
                match maybe_event {
                    Some(Ok(key_event)) => InputEvent::Input(key_event),
                    Some(Err(e)) => break Err(anyhow!(e)),
                    None => break Err(anyhow!("none")),
                }
            }
        };
        tx.send(e).await?;
    };
    last_e
}
