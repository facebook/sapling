//! Events.

use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use termwiz::input::InputEvent;
use termwiz::terminal::Terminal;
use termwiz::terminal::TerminalWaker;

use crate::action::Action;
use crate::action::ActionSender;
use crate::error::Error;
use crate::file::FileIndex;

/// An event.
///
/// Events drive most of the main processing of `sp`.  This includes user
/// input, state changes, and display refresh requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Event {
    /// An action.
    Action(Action),
    /// An input event.
    Input(InputEvent),
    /// A file has finished loading.
    Loaded(FileIndex),
    #[cfg(feature = "load_file")]
    /// A file has started loading more data.
    Appending(FileIndex),
    /// A file has started reloading.
    Reloading(FileIndex),
    /// Render an update to the screen.
    Render,
    /// Refresh the whole screen.
    Refresh,
    /// Refresh the overlay.
    RefreshOverlay,
    /// A new progress display is available.
    Progress,
    /// Search has found the first match.
    SearchFirstMatch(FileIndex),
    /// Search has finished.
    SearchFinished(FileIndex),
}

#[derive(Debug, Clone)]
pub(crate) struct UniqueInstance(Arc<AtomicBool>);

impl UniqueInstance {
    pub(crate) fn new() -> UniqueInstance {
        UniqueInstance(Arc::new(AtomicBool::new(false)))
    }
}

pub(crate) enum Envelope {
    Normal(Event),
    Unique(Event, UniqueInstance),
}

/// An event sender endpoint.
#[derive(Clone)]
pub(crate) struct EventSender(mpsc::Sender<Envelope>, TerminalWaker);

impl EventSender {
    pub(crate) fn send(&self, event: Event) -> Result<(), Error> {
        self.0.send(Envelope::Normal(event))?;
        self.1.wake()?;
        Ok(())
    }
    pub(crate) fn send_unique(&self, event: Event, unique: &UniqueInstance) -> Result<(), Error> {
        if unique
            .0
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            self.0.send(Envelope::Unique(event, unique.clone()))?;
            self.1.wake()?;
        }
        Ok(())
    }
}

/// An event stream.  This is a wrapper multi-producer, single-consumer
/// stream of `Event`s.
pub(crate) struct EventStream {
    send: mpsc::Sender<Envelope>,
    recv: mpsc::Receiver<Envelope>,
    waker: TerminalWaker,
}

impl EventStream {
    /// Create a new event stream.
    pub(crate) fn new(waker: TerminalWaker) -> EventStream {
        let (send, recv) = mpsc::channel();
        EventStream { send, recv, waker }
    }

    /// Create a sender for the event stream.
    pub(crate) fn sender(&self) -> EventSender {
        EventSender(self.send.clone(), self.waker.clone())
    }

    /// Create an action sender for the event stream.
    pub(crate) fn action_sender(&self) -> ActionSender {
        ActionSender::new(self.sender())
    }

    /// Attempt to receive an event. If timeout is specified, wait up to timeout
    /// for an event, returning None if there is no event. With no timeout,
    /// return None immediately if there is no event.
    pub(crate) fn try_recv(&self, timeout: Option<Duration>) -> Result<Option<Event>, Error> {
        let envelope = match timeout {
            Some(timeout) => match self.recv.recv_timeout(timeout) {
                Ok(envelope) => envelope,
                Err(mpsc::RecvTimeoutError::Timeout) => return Ok(None),
                Err(e) => return Err(e.into()),
            },
            None => match self.recv.try_recv() {
                Ok(envelope) => envelope,
                Err(mpsc::TryRecvError::Empty) => return Ok(None),
                Err(e) => return Err(e.into()),
            },
        };

        match envelope {
            Envelope::Normal(event) => Ok(Some(event)),
            Envelope::Unique(event, unique) => {
                unique.0.store(false, Ordering::SeqCst);
                Ok(Some(event))
            }
        }
    }

    /// Get an event, either from the event stream or from the terminal.
    pub(crate) fn get(
        &self,
        term: &mut dyn Terminal,
        wait: Option<Duration>,
    ) -> Result<Option<Event>, Error> {
        loop {
            if let Some(event) = self.try_recv(None)? {
                return Ok(Some(event));
            }

            // The queue is empty.  Try to get an input event from the terminal.
            match term.poll_input(wait).map_err(Error::Termwiz)? {
                Some(InputEvent::Wake) => {}
                Some(input_event) => return Ok(Some(Event::Input(input_event))),
                None => return Ok(None),
            }
        }
    }
}
