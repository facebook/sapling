/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use gix_features::hash::Hasher;
use pin_project::pin_project;
use tokio::io::AsyncWrite;

/// A writer that uses the underlying write handle for writing data while at
/// the same time hashing the content written so far.
#[pin_project]
pub struct AsyncHashWriter<T>
where
    T: AsyncWrite,
{
    /// Underlying write handle.
    #[pin]
    pub inner: T,
    /// SHA1 hasher
    pub hasher: Hasher,
}

impl<T: AsyncWrite> AsyncHashWriter<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            hasher: Hasher::default(),
        }
    }
}

impl<T: AsyncWrite> AsyncWrite for AsyncHashWriter<T> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        let this = self.project();
        match this.inner.poll_write(cx, buf) {
            Poll::Ready(Ok(written)) => {
                this.hasher.update(&buf[..written]);
                Poll::Ready(Ok(written))
            }
            other_state => other_state,
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
        self.project().inner.poll_flush(cx)
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        self.project().inner.poll_flush(cx)
    }
}
