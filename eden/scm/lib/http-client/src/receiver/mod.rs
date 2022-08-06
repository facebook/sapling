/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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
pub trait Receiver: Sized {
    /// Handle received chunk of the response body.
    fn chunk(&mut self, chunk: Vec<u8>) -> Result<()>;

    /// Handle a received header.
    fn header(&mut self, header: Header) -> Result<()>;

    /// Get progress updates for this transfer.
    /// This function will be called whenever the underlying
    /// transfer makes progress.
    fn progress(&mut self, _progress: Progress) {}

    /// Called when the transfer has completed (successfully or not).
    ///
    /// If a fatal error occured while performing the transfer, the error
    /// will be passed to this method so that the `Receiver` can decide how
    /// to proceed. If the `Receiver` returns an `Abort`, all other ongoing
    /// transfers will be aborted and the operation will return early.
    fn done(self, _res: Result<(), HttpClientError>) -> Result<(), Abort> {
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod testutil {
    #![allow(dead_code)]

    use std::cell::RefCell;
    use std::rc::Rc;

    use http::header::HeaderName;
    use http::header::HeaderValue;
    use http::StatusCode;

    use super::*;

    /// Simple receiver for use in tests.
    #[derive(Clone, Debug)]
    pub struct TestReceiver {
        inner: Rc<RefCell<TestReceiverInner>>,
    }

    impl TestReceiver {
        pub fn new() -> Self {
            Self {
                inner: Rc::new(RefCell::new(Default::default())),
            }
        }

        pub fn status(&self) -> Option<StatusCode> {
            self.inner.borrow().status
        }

        pub fn headers(&self) -> Vec<(HeaderName, HeaderValue)> {
            self.inner.borrow().headers.clone()
        }

        pub fn chunks(&self) -> Vec<Vec<u8>> {
            self.inner.borrow().chunks.clone()
        }

        pub fn progress(&self) -> Option<Progress> {
            self.inner.borrow().progress
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
        fn chunk(&mut self, chunk: Vec<u8>) -> Result<()> {
            self.inner.borrow_mut().chunks.push(chunk);
            Ok(())
        }

        fn header(&mut self, header: Header) -> Result<()> {
            match header {
                Header::Status(_, status) => {
                    self.inner.borrow_mut().status = Some(status);
                }
                Header::Header(name, value) => {
                    self.inner.borrow_mut().headers.push((name, value));
                }
                Header::EndOfHeaders => {}
            };
            Ok(())
        }

        fn progress(&mut self, progress: Progress) {
            self.inner.borrow_mut().progress = Some(progress);
        }
    }

    /// No-op receiver for use in tests.
    #[derive(Copy, Clone, Debug)]
    pub struct NullReceiver;

    impl Receiver for NullReceiver {
        fn chunk(&mut self, _chunk: Vec<u8>) -> Result<()> {
            Ok(())
        }
        fn header(&mut self, _header: Header) -> Result<()> {
            Ok(())
        }
    }
}
