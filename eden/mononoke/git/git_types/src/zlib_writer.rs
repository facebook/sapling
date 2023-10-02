/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use async_compression::tokio::write::ZlibEncoder;
use pin_project::pin_project;
use tokio::io::AsyncWrite;

/// An encoder that uses the underlying write handle for writing Zlib encoded data while at
/// the same time maintaining count of the raw bytes written so far.
#[pin_project]
pub struct AsyncZlibEncoder<T>
where
    T: AsyncWrite,
{
    /// Underlying write handle.
    #[pin]
    inner: ZlibEncoder<T>,
    /// Count of raw bytes written
    bytes_count: u64,
}

impl<T: AsyncWrite> AsyncZlibEncoder<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner: ZlibEncoder::new(inner),
            bytes_count: 0,
        }
    }

    pub fn into_inner(self) -> T {
        self.inner.into_inner()
    }

    pub fn bytes_written(&self) -> u64 {
        self.bytes_count
    }
}

impl<T: AsyncWrite> AsyncWrite for AsyncZlibEncoder<T> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        let this = self.project();
        match this.inner.poll_write(cx, buf) {
            Poll::Ready(Ok(written)) => {
                *this.bytes_count += written as u64;
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
        self.project().inner.poll_shutdown(cx)
    }
}
