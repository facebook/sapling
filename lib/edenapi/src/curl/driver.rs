// Copyright Facebook, Inc. 2019

use std::{cell::RefCell, time::Duration};

use curl::{
    self,
    easy::{Easy2, Handler},
    multi::{Easy2Handle, Multi},
};
use failure::Fallible;

use crate::progress::ProgressManager;

/// Timeout for a single iteration of waiting for activity
/// on any active transfer in a curl::Multi session.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// Struct that manages a curl::Multi session, synchronously driving
/// all of the transfers therein to completion.
pub struct MultiDriver<'a, H> {
    multi: &'a mut Multi,
    handles: RefCell<Vec<Option<Easy2Handle<H>>>>,
    progress: Option<ProgressManager>,
    num_transfers: usize,
    fail_early: bool,
}

impl<'a, H: Handler> MultiDriver<'a, H> {
    pub fn with_capacity(multi: &'a mut Multi, capacity: usize) -> Self {
        Self {
            multi,
            handles: RefCell::new(Vec::with_capacity(capacity)),
            progress: None,
            num_transfers: 0,
            fail_early: false,
        }
    }

    pub fn set_progress_manager(&mut self, progress: ProgressManager) {
        self.progress = Some(progress);
    }

    pub fn progress(&self) -> Option<&ProgressManager> {
        self.progress.as_ref()
    }

    /// Add an Easy2 handle to the Multi stack.
    pub fn add(&mut self, easy: Easy2<H>) -> Fallible<()> {
        // Assign a token to this Easy2 handle so we can correlate messages
        // for this handle with the corresponding Easy2Handle while the
        // Easy2 is owned by the Multi handle.
        let mut handles = self.handles.borrow_mut();
        let token = handles.len();
        let mut handle = self.multi.add2(easy)?;
        handle.set_token(token)?;
        handles.push(Some(handle));
        self.num_transfers += 1;
        Ok(())
    }

    /// If `fail_early` is set to true, then the driver will return early if
    /// any transfers fail (leaving the remaining transfers in an unfinished
    /// state); otherwise, the driver will only return once all transfers
    /// have completed (successfully or otherwise).
    pub fn fail_early(&mut self, fail_early: bool) {
        self.fail_early = fail_early;
    }

    /// Drive all of the Easy2 handles in the Multi stack to completion.
    ///
    /// The caller-supplied callback will be called whenever a transfer
    /// completes, successfully or otherwise.
    pub fn perform<F>(&mut self, mut callback: F) -> Fallible<()>
    where
        F: FnMut(Result<Easy2<H>, curl::Error>) -> Fallible<()>,
    {
        let mut in_progress = self.num_transfers;
        let mut i = 0;

        loop {
            log::trace!(
                "Iteration {}: {}/{} transfers complete",
                i,
                self.num_transfers - in_progress,
                self.num_transfers
            );
            i += 1;

            in_progress = self.multi.perform()? as usize;

            // Check for messages; a message indicates a transfer completed (successfully or not).
            let mut should_report_progress = false;
            let mut errors = Vec::new();
            self.multi.messages(|msg| {
                let token = msg.token().unwrap();
                log::trace!("Got message for transfer {}", token);

                should_report_progress = true;

                match msg.result() {
                    Some(Ok(())) => {
                        log::trace!("Transfer {} complete", token);
                        match self.take_handle(token) {
                            Ok(Some(handle)) => {
                                if let Err(e) = callback(Ok(handle)) {
                                    errors.push(e);
                                }
                            }
                            Ok(None) => {
                                log::trace!("Handle already taken");
                            }
                            Err(e) => {
                                errors.push(e);
                            }
                        }
                    }
                    Some(Err(e)) => {
                        log::trace!("Transfer {} failed: {}", token, &e);
                        if let Err(e) = callback(Err(e)) {
                            errors.push(e);
                        }
                    }
                    None => {
                        // Theoretically this should never happen because
                        // this closure is only called on completion.
                        log::trace!("Transfer {} incomplete", token);
                    }
                }
            });

            if self.fail_early && !errors.is_empty() {
                log::debug!("{} transfer(s) failed; aborting.", errors.len());
                return Err(errors.pop().unwrap());
            }

            if should_report_progress {
                if let Some(ref mut progress) = self.progress {
                    progress.report();
                }
            }

            if in_progress == 0 {
                log::debug!("All transfers finished successfully.");
                break;
            }

            let timeout = self.multi.get_timeout()?.unwrap_or(DEFAULT_TIMEOUT);
            log::trace!("Waiting for I/O with timeout: {:?}", &timeout);

            let num_active_transfers = self.multi.wait(&mut [], Duration::from_secs(1))?;
            if num_active_transfers == 0 {
                log::trace!("Timed out waiting for I/O; polling active transfers anyway.");
            }
        }

        Ok(())
    }

    fn take_handle(&self, index: usize) -> Fallible<Option<Easy2<H>>> {
        let mut handles = self.handles.borrow_mut();
        if let Some(handle) = handles[index].take() {
            let easy = self.multi.remove2(handle)?;
            Ok(Some(easy))
        } else {
            Ok(None)
        }
    }
}
