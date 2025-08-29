/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::cell::RefCell;
use std::time::Duration;
use std::time::Instant;

use anyhow::Context;
use curl::multi::Easy2Handle;
use curl::multi::Message;
use curl::multi::Multi;

use crate::Easy2H;
use crate::errors::Abort;
use crate::errors::HttpClientError;
use crate::handler::HandlerExt;
use crate::progress::ProgressReporter;
use crate::stats::Stats;

/// Maximum time that libcurl should wait for socket activity during a call to
/// `Multi::wait`. The Multi session maintains its own timeout internally based
/// on the state of the underlying transfers; this default value will only be
/// used if there is no internal timer value set at the time `wait` is called.
const MULTI_WAIT_TIMEOUT: Duration = Duration::from_secs(10);

/// A complete transfer, along with the associated error
/// if the transfer did not complete successfully.
struct Complete {
    token: usize,
    handle: Easy2H,
    result: Result<(), curl::Error>,
}

impl Complete {
    /// If we encountered an error, we should still return the
    /// handle, as the callback may want to access the Handler
    /// inside.
    fn into_result(self) -> Result<Easy2H, (Easy2H, curl::Error)> {
        let Self { handle, result, .. } = self;
        match result {
            Ok(()) => Ok(handle),
            Err(e) => Err((handle, e)),
        }
    }
}

/// Struct that manages a curl::Multi session, synchronously driving
/// all of the transfers therein to completion.
pub(crate) struct MultiDriver<'a> {
    multi: &'a Multi,
    handles: RefCell<Vec<Option<Easy2Handle<Box<dyn HandlerExt>>>>>,
    progress: ProgressReporter,
    verbose: bool,
}

impl<'a> MultiDriver<'a> {
    pub(crate) fn new(multi: &'a Multi, verbose: bool) -> Self {
        Self {
            multi,
            handles: RefCell::new(Vec::new()),
            progress: ProgressReporter::default(),
            verbose,
        }
    }

    pub(crate) fn num_transfers(&self) -> usize {
        (*self.handles.borrow()).len()
    }

    /// Add an Easy2 handle to the Multi stack.
    pub(crate) fn add(&self, mut easy: Easy2H) -> Result<(), HttpClientError> {
        // Register this Easy2 handle's Handler with our ProgressReporter
        // so we can aggregate progress across all transfers in the stack.
        easy.get_mut()
            .request_context_mut()
            .event_listeners()
            .on_progress({
                let updater = self.progress.updater();
                move |_req, progress| updater.update(progress)
            });

        // Assign a token to this Easy2 handle so we can correlate messages
        // for this handle with the corresponding Easy2Handle while the
        // Easy2 is owned by the Multi handle.
        let mut handles = self.handles.borrow_mut();
        let token = handles.len();
        let mut handle = self.multi.add2(easy)?;
        handle.set_token(token)?;

        handles.push(Some(handle));

        Ok(())
    }

    /// Drive all of the Easy2 handles in the Multi stack to completion.
    ///
    /// The user-supplied callback will be called whenever a transfer
    /// completes, successfully or otherwise. The callback may cause this
    /// method to return early (aborting all other active transfers).
    pub(crate) fn perform<F>(&self, mut callback: F) -> Result<Stats, HttpClientError>
    where
        F: FnMut(Result<Easy2H, (Easy2H, curl::Error)>) -> Result<(), Abort>,
    {
        let mut total = self.num_transfers();
        let mut in_progress = total;
        tracing::debug!("Performing {total} transfer(s)");

        let start = Instant::now();

        loop {
            let active_transfers = self.multi.perform()? as usize;

            // Check how many transfers have completed so far.
            if active_transfers != in_progress {
                tracing::debug!(
                    "{}/{} transfer(s) complete",
                    total - active_transfers,
                    total
                );
                in_progress = active_transfers;
            }

            // Check for messages. A message indicates a transfer completed or failed.
            let mut completed = Vec::new();
            self.multi.messages(|msg| match self.handle_msg(&msg) {
                Ok(complete) => {
                    tracing::trace!("Transfer {} complete", complete.token);
                    completed.push(complete);
                }
                Err(e) => {
                    tracing::error!("Failed to handle message: {}", e);
                }
            });

            // Run the user-provided callback on each completed transfer. If it returns an
            // error (signalling that we should return early) abort all remaining transfers.
            for c in completed {
                let token = c.token;
                callback(c.into_result())?;
                tracing::trace!("Successfully handled transfer: {}", token);

                // Notice if the callback added new handles and adjust `in_progress`
                // accordingly so we continue looping.
                let num_transfers = self.num_transfers();
                if num_transfers > total {
                    let added = num_transfers - total;
                    tracing::trace!("Perform callback added {added} new handles");
                    in_progress += added;
                    total += added;
                }
            }

            // If any transfers reported progress, notify the user.
            self.progress.report_if_updated();

            if in_progress == 0 {
                tracing::trace!("All transfers finished successfully");
                break;
            }

            let has_paused_transfer = self
                .handles
                .borrow()
                .iter()
                .any(|easy| easy.as_ref().is_some_and(|easy| easy.get_ref().is_paused()));

            // Use a small timeout if we have paused transfers. We are waiting for the application
            // to consume response data - it shouldn't take very long.
            let timeout = if has_paused_transfer {
                Duration::from_millis(1)
            } else {
                MULTI_WAIT_TIMEOUT
            };

            tracing::trace!(?timeout, "Waiting for socket activity");
            let active_sockets = self.multi.wait(&mut [], timeout)?;
            if active_sockets == 0 {
                tracing::trace!("Timed out waiting for activity");
            }

            // Check for paused transfers that can be resumed.
            for easy in self.handles.borrow_mut().iter_mut() {
                if let Some(easy) = easy {
                    if easy.get_mut().needs_unpause() {
                        tracing::trace!("unpausing transfer");
                        tracing::trace!(target: "curl_pause", "unpausing transfer");
                        easy.unpause_write()?;
                    }
                }
            }
        }

        let elapsed = start.elapsed();

        let progress = self.progress.aggregate();
        let latency = self
            .progress
            .first_byte_received()
            .unwrap_or(start)
            .duration_since(start);

        let stats = Stats {
            downloaded: progress.downloaded,
            uploaded: progress.uploaded,
            requests: self.num_transfers(),
            time: elapsed,
            latency,
        };

        tracing::debug!("{}", &stats);
        if self.verbose {
            eprintln!("{}", &stats);
        }

        Ok(stats)
    }

    /// Handle a message emitted by the Multi session for any of the
    /// underlying transfers. Based on the current implementation details
    /// of libcurl, a message should only be emitted when a transfer has
    /// completed (successfully or otherwise).
    fn handle_msg(&self, msg: &Message<'_>) -> Result<Complete, HttpClientError> {
        let (token, result) = {
            let token = msg.token()?;
            let handles = self.handles.borrow();
            let handle = handles[token].as_ref().context("Handle already removed")?;
            let result = msg
                .result_for2(handle)
                .context("Failed to get result for handle")?;
            (token, result)
        };

        // If we've gotten this far, we can conclude the transfer has completed
        // (successfully or otherwise), so it can be removed from the stack.
        let handle = self.remove(token)?.context("Handle already removed")?;

        Ok(Complete {
            token,
            handle,
            result,
        })
    }

    /// Remove and return an Easy2 handle from the Multi stack.
    fn remove(&self, index: usize) -> Result<Option<Easy2H>, HttpClientError> {
        if let Some(handle) = self.handles.borrow_mut()[index].take() {
            let easy = self.multi.remove2(handle)?;
            Ok(Some(easy))
        } else {
            Ok(None)
        }
    }

    /// Drop all of the outstanding Easy2 handles in the Multi stack.
    fn drop_all(&mut self) {
        let mut dropped = 0;
        for handle in self.handles.borrow_mut().drain(..) {
            if let Some(handle) = handle {
                let _ = self.multi.remove2(handle);
                dropped += 1;
            }
        }

        if dropped > 0 {
            tracing::debug!("Dropped {} outstanding transfers", dropped);
        }
    }
}

impl<'a> Drop for MultiDriver<'a> {
    fn drop(&mut self) {
        self.drop_all();
    }
}
