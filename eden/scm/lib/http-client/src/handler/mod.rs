/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use curl::easy::Handler;

use crate::progress::ProgressUpdater;
use crate::RequestContext;

mod buffered;
mod streaming;

pub use buffered::Buffered;
pub use streaming::Streaming;

/// Extends `curl::Handler` with APIs useful for this crate.
///
/// Trait allowing a `curl::Handler` to be configured in a generic way.
/// All of the handlers used by the HTTP client need to implement this
/// trait so that they can be properly configured prior to use.
pub(crate) trait HandlerExt: Handler {
    /// Configure the `Handler` to provide progress updates
    /// using the given `ProgressUpdater`.
    ///
    /// XXX: Note that we can't use a builder-like pattern
    /// for this method since it will typically called
    /// through a mutable reference while the handler is
    /// owned by an Easy2 handle.
    fn monitor_progress(&mut self, updater: ProgressUpdater);

    /// Obtain the mutable `RequestContext` state.
    fn request_context_mut(&mut self) -> &mut RequestContext;

    /// Obtain the immutable `RequestContext` state.
    fn request_context(&self) -> &RequestContext;
}
