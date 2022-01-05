/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use curl::easy::Handler;

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
    /// Obtain the mutable `RequestContext` state.
    fn request_context_mut(&mut self) -> &mut RequestContext;

    /// Obtain the immutable `RequestContext` state.
    fn request_context(&self) -> &RequestContext;
}
