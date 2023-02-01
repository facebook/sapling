//! Progress indicator.
//!
//! sp can accept another file descriptor from its parent process via the
//! `--pager-fd` option or the PAGER_PROGRESS_FD environment variable.  This
//! should be a pipe on which the parent process sends progress indicator pages.
//!
//! Progress indicator pages are blocks of text terminated by an ASCII form-feed
//! character.  The progress indicator will display the most recently received
//! page.

use std::io::{BufRead, BufReader, Read};
use std::sync::{Arc, RwLock};
use std::thread;

use crate::error::Result;
use crate::event::{Event, EventSender, UniqueInstance};

/// Initial buffer size for progress indicator pages.
const PROGRESS_BUFFER_SIZE: usize = 4096;

/// Inner struct for the progress indicator.
pub(crate) struct ProgressInner {
    /// Buffer containing the currently displayed page.
    buffer: Vec<u8>,

    /// Offsets of all the newlines in the current page.
    newlines: Vec<usize>,

    /// Whether the progress indicator is finished because the other
    /// end of the pipe closed.
    finished: bool,
}

/// A progress indicator.
#[derive(Clone)]
pub(crate) struct Progress {
    /// The inner progress indicator data.
    inner: Arc<RwLock<ProgressInner>>,
}

impl Progress {
    /// Create a new progress indicator that receives progress pages on the
    /// given file descriptor.  Progress events are sent on the event_sender
    /// whenever a new page is received.
    pub(crate) fn new(reader: impl Read + Send + 'static, event_sender: EventSender) -> Progress {
        let inner = Arc::new(RwLock::new(ProgressInner {
            buffer: Vec::new(),
            newlines: Vec::new(),
            finished: false,
        }));
        let mut input = BufReader::new(reader);
        thread::Builder::new()
            .name(String::from("sp-progress"))
            .spawn({
                let inner = inner.clone();
                let progress_unique = UniqueInstance::new();
                move || -> Result<()> {
                    loop {
                        let mut buffer = Vec::with_capacity(PROGRESS_BUFFER_SIZE);
                        match input.read_until(b'\x0C', &mut buffer) {
                            Ok(0) | Err(_) => {
                                let mut inner = inner.write().unwrap();
                                inner.buffer = Vec::new();
                                inner.newlines = Vec::new();
                                inner.finished = true;
                                return Ok(());
                            }
                            Ok(len) => {
                                buffer.truncate(len - 1);
                                let mut newlines = Vec::new();
                                for (i, byte) in buffer.iter().enumerate().take(len - 1) {
                                    if *byte == b'\n' {
                                        newlines.push(i);
                                    }
                                }
                                let mut inner = inner.write().unwrap();
                                inner.buffer = buffer;
                                inner.newlines = newlines;
                                event_sender.send_unique(Event::Progress, &progress_unique)?;
                            }
                        }
                    }
                }
            })
            .unwrap();
        Progress { inner }
    }

    /// Returns the number of lines in the current page.
    pub(crate) fn lines(&self) -> usize {
        let inner = self.inner.read().unwrap();
        if inner.finished {
            return 0;
        }
        let mut lines = inner.newlines.len();
        let after_last_newline_offset = if lines == 0 {
            0
        } else {
            inner.newlines[lines - 1] + 1
        };
        if inner.buffer.len() > after_last_newline_offset {
            lines += 1;
        }
        lines
    }

    /// Calls the callback `call` with the given line of the current page.
    pub(crate) fn with_line<T, F>(&self, index: usize, mut call: F) -> Option<T>
    where
        F: FnMut(&[u8]) -> T,
    {
        let inner = self.inner.read().unwrap();
        if index > inner.newlines.len() {
            return None;
        }
        let start = if index == 0 {
            0
        } else {
            inner.newlines[index - 1] + 1
        };
        let end = if index < inner.newlines.len() {
            inner.newlines[index] + 1
        } else {
            inner.buffer.len()
        };
        if start == end {
            return None;
        }
        Some(call(&inner.buffer[start..end]))
    }
}
