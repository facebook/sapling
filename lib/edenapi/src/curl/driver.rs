// Copyright Facebook, Inc. 2019

use std::{fmt::Write, mem, time::Duration};

use curl::{
    self,
    easy::{Easy2, Handler},
    multi::{Easy2Handle, Multi},
};
use failure::{err_msg, Fallible};

/// Timeout for a single iteration of waiting for activity
/// on any active transfer in a curl::Multi session.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// The result of using a MultiDriver to manage a curl::Multi session.
/// Contains all of the Easy2 handles for the session along with
/// information about which (if any) of the transfers failed.
pub struct MultiDriverResult<H> {
    handles: Vec<Easy2<H>>,
    failed: Vec<(usize, curl::Error)>,
}

impl<H> MultiDriverResult<H> {
    pub fn into_result(self) -> Fallible<Vec<Easy2<H>>> {
        if self.failed.is_empty() {
            return Ok(self.handles);
        }

        let mut msg = "The following transfers failed:\n".to_string();
        for (i, e) in self.failed {
            write!(&mut msg, "{}: {}\n", i, e)?;
        }

        Err(err_msg(msg))
    }
}

/// Struct that manages a curl::Multi session, synchronously driving
/// all of the transfers therein to completion.
pub struct MultiDriver<H> {
    multi: Multi,
    handles: Vec<Easy2Handle<H>>,
}

impl<H: Handler> MultiDriver<H> {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            multi: Multi::new(),
            handles: Vec::with_capacity(capacity),
        }
    }

    /// Add an Easy2 handle to the Multi stack.
    pub fn add(&mut self, easy: Easy2<H>) -> Fallible<()> {
        // Assign a token to this Easy2 handle so we can correlate messages
        // for this handle with the corresponding Easy2Handle while the
        // Easy2 is owned by the Multi handle.
        let token = self.handles.len();
        let mut handle = self.multi.add2(easy)?;
        handle.set_token(token)?;
        self.handles.push(handle);
        Ok(())
    }

    /// Remove and return all of the Easy2 handles in the Multi stack.
    pub fn remove_all(&mut self) -> Fallible<Vec<Easy2<H>>> {
        let handles = mem::replace(&mut self.handles, Vec::with_capacity(0));
        let mut easy_vec = Vec::with_capacity(handles.len());
        for handle in handles {
            let easy = self.multi.remove2(handle)?;
            easy_vec.push(easy);
        }
        Ok(easy_vec)
    }

    /// Drive all of the Easy2 handles in the Multi stack to completion.
    ///
    /// If `fail_early` is set to true, then this method will return early if
    /// any transfers fail (leaving the remaining transfers in an unfinished
    /// state); otherwise, the driver will only return once all transfers
    /// have completed (successfully or otherwise).
    ///
    /// Returns all of the Easy2 handles in the Multi stack in the order
    /// they were added, along with the indices of any failed transfers
    /// (along with the corresponding error code).
    pub(super) fn perform(&mut self, fail_early: bool) -> Fallible<MultiDriverResult<H>> {
        let num_transfers = self.handles.len();
        let mut in_progress = num_transfers;
        let mut failed = Vec::new();
        let mut i = 0;

        while in_progress > 0 {
            log::trace!(
                "Iteration {}: {}/{} transfers complete",
                i,
                num_transfers - in_progress,
                num_transfers
            );
            i += 1;

            in_progress = self.multi.perform()? as usize;

            // Check for messages; a message indicates a transfer completed (successfully or not).
            self.multi.messages(|msg| {
                let token = msg.token().unwrap();
                log::trace!("Got message for transfer {}", token);
                match msg.result() {
                    Some(Ok(())) => {
                        log::trace!("Transfer {} complete", token);
                    }
                    Some(Err(e)) => {
                        log::trace!("Transfer {} failed: {}", token, &e);
                        failed.push((token, e));
                    }
                    None => {
                        // Theoretically this should never happen because
                        // this closure is only called on completion.
                        log::trace!("Transfer {} incomplete", token);
                    }
                }
            });

            if fail_early && failed.len() > 0 {
                log::debug!("At least one transfer failed; aborting.");
                break;
            }

            let timeout = self.multi.get_timeout()?.unwrap_or(DEFAULT_TIMEOUT);
            log::trace!("Waiting for I/O with timeout: {:?}", &timeout);

            let num_active_transfers = self.multi.wait(&mut [], Duration::from_secs(1))?;
            if num_active_transfers == 0 {
                log::trace!("Timed out waiting for I/O; polling active transfers anyway.");
            }
        }

        let handles = self.remove_all()?;
        Ok(MultiDriverResult { handles, failed })
    }
}
