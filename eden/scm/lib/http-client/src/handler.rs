/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::any::Any;
use std::io::SeekFrom;

use curl::easy::Handler;
use curl::easy::ReadError;
use curl::easy::SeekResult;
use curl::easy::WriteError;

use crate::Receiver;
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
#[auto_impl::auto_impl(Box)]
pub(crate) trait HandlerExt: Handler {
    /// Obtain the mutable `RequestContext` state.
    fn request_context_mut(&mut self) -> &mut RequestContext;

    /// Obtain the immutable `RequestContext` state.
    fn request_context(&self) -> &RequestContext;

    fn take_receiver(&mut self) -> Option<Box<dyn Receiver>> {
        None
    }

    fn as_any_mut(&mut self) -> &mut dyn Any;

    fn is_paused(&self) -> bool {
        false
    }

    fn needs_unpause(&mut self) -> bool {
        false
    }
}

impl Handler for Box<dyn HandlerExt> {
    fn write(&mut self, data: &[u8]) -> Result<usize, WriteError> {
        self.as_mut().write(data)
    }

    fn read(&mut self, data: &mut [u8]) -> Result<usize, ReadError> {
        self.as_mut().read(data)
    }

    fn seek(&mut self, whence: SeekFrom) -> SeekResult {
        self.as_mut().seek(whence)
    }

    fn header(&mut self, data: &[u8]) -> bool {
        self.as_mut().header(data)
    }

    fn progress(&mut self, dltotal: f64, dlnow: f64, ultotal: f64, ulnow: f64) -> bool {
        self.as_mut().progress(dltotal, dlnow, ultotal, ulnow)
    }
}
