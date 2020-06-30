/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use curl::easy::Handler;

use crate::progress::ProgressUpdater;

mod buffered;
mod util;

pub use buffered::Buffered;

/// Trait allowing a `curl::Handler` to be configured in a generic way.
/// All of the handlers used by the HTTP client need to implement this
/// trait so that they can be properly configured prior to use.
pub(crate) trait Configure: Handler {
    /// Specify the payload to be sent to the server in
    /// the request body.
    fn with_payload(self, payload: Option<Vec<u8>>) -> Self;

    /// Configure the `Handler` to provide progress updates
    /// using the given `ProgressUpdater`.
    ///
    /// XXX: Note that we can't use a builder-like pattern
    /// for this method since it will typically called
    /// through a mutable reference while the handler is
    /// owned by an Easy2 handle.
    fn monitor_progress(&mut self, updater: ProgressUpdater);
}
