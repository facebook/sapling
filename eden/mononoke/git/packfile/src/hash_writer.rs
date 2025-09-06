/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use pin_project::pin_project;
use sha1_checked::Digest;
use sha1_checked::Sha1;

use crate::owned_async_writer::OwnedAsyncWrite;

/// A writer that uses the underlying write handle for writing data while at
/// the same time hashing the content written so far.
#[pin_project]
pub struct AsyncHashWriter<T>
where
    T: OwnedAsyncWrite,
{
    /// Underlying write handle.
    #[pin]
    pub inner: T,
    /// SHA1 hasher
    pub hasher: Sha1,
}

impl<T: OwnedAsyncWrite> AsyncHashWriter<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            hasher: Sha1::new(),
        }
    }
}

impl<T: OwnedAsyncWrite> OwnedAsyncWrite for AsyncHashWriter<T> {
    async fn write_all(&mut self, src: Vec<u8>) -> anyhow::Result<()> {
        self.hasher.update(src.as_slice());
        self.inner.write_all(src).await
    }

    async fn flush(&mut self) -> anyhow::Result<()> {
        self.inner.flush().await
    }
}
