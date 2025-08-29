/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Result;

use crate::errors::Abort;
use crate::errors::HttpClientError;
use crate::header::Header;
use crate::progress::Progress;

pub mod channel;

pub use channel::ChannelReceiver;
pub use channel::ResponseStreams;

/// Interface for streaming HTTP response handlers.
#[auto_impl::auto_impl(Box)]
pub trait Receiver: Send + 'static {
    /// Handle received chunk of the response body.
    /// Return true to pause the transfer.
    fn chunk(&mut self, chunk: Vec<u8>) -> Result<bool>;

    /// Handle a received header.
    fn header(&mut self, header: Header) -> Result<()>;

    /// Get progress updates for this transfer.
    /// This function will be called whenever the underlying
    /// transfer makes progress.
    fn progress(&mut self, _progress: Progress) {}

    /// Called when the transfer has completed (successfully or not).
    ///
    /// If a fatal error occurred while performing the transfer, the error
    /// will be passed to this method so that the `Receiver` can decide how
    /// to proceed. If the `Receiver` returns an `Abort`, all other ongoing
    /// transfers will be aborted and the operation will return early.
    fn done(&mut self, _res: Result<(), HttpClientError>) -> Result<(), Abort> {
        Ok(())
    }

    /// Report whether this receiver was previously paused and now can handle writes.
    fn needs_unpause(&mut self) -> bool {
        false
    }

    /// Report whether this receiver is paused.
    fn is_paused(&self) -> bool {
        false
    }
}

#[cfg(test)]
pub(crate) mod testutil {
    #![allow(dead_code)]

    use std::sync::Arc;
    use std::sync::Mutex;

    use http::StatusCode;
    use http::header::HeaderName;
    use http::header::HeaderValue;

    use super::*;

    /// Simple receiver for use in tests.
    #[derive(Clone, Debug)]
    pub struct TestReceiver {
        inner: Arc<Mutex<TestReceiverInner>>,
    }

    impl TestReceiver {
        pub fn new() -> Self {
            Self {
                inner: Arc::new(Mutex::new(Default::default())),
            }
        }

        pub fn status(&self) -> Option<StatusCode> {
            self.inner.lock().unwrap().status
        }

        pub fn headers(&self) -> Vec<(HeaderName, HeaderValue)> {
            self.inner.lock().unwrap().headers.clone()
        }

        pub fn chunks(&self) -> Vec<Vec<u8>> {
            self.inner.lock().unwrap().chunks.clone()
        }

        pub fn progress(&self) -> Option<Progress> {
            self.inner.lock().unwrap().progress
        }
    }

    #[derive(Debug, Default)]
    struct TestReceiverInner {
        status: Option<StatusCode>,
        headers: Vec<(HeaderName, HeaderValue)>,
        chunks: Vec<Vec<u8>>,
        progress: Option<Progress>,
    }

    impl Receiver for TestReceiver {
        fn chunk(&mut self, chunk: Vec<u8>) -> Result<bool> {
            self.inner.lock().unwrap().chunks.push(chunk);
            Ok(false)
        }

        fn header(&mut self, header: Header) -> Result<()> {
            match header {
                Header::Status(_, status) => {
                    self.inner.lock().unwrap().status = Some(status);
                }
                Header::Header(name, value) => {
                    self.inner.lock().unwrap().headers.push((name, value));
                }
                Header::EndOfHeaders => {}
            };
            Ok(())
        }

        fn progress(&mut self, progress: Progress) {
            self.inner.lock().unwrap().progress = Some(progress);
        }
    }

    /// No-op receiver for use in tests.
    #[derive(Copy, Clone, Debug)]
    pub struct NullReceiver;

    impl Receiver for NullReceiver {
        fn chunk(&mut self, _chunk: Vec<u8>) -> Result<bool> {
            Ok(false)
        }
        fn header(&mut self, _header: Header) -> Result<()> {
            Ok(())
        }
    }
}
