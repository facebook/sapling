/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use futures::ready;
use futures::task::Context;
use futures::task::Poll;
use pin_project::pin_project;
use std::io::Error;
use std::io::ErrorKind;
use std::pin::Pin;
use tokio::io::AsyncRead;
use tokio::io::AsyncWrite;
use tokio::io::ReadBuf;

#[pin_project]
pub struct QuietShutdownStream<T> {
    #[pin]
    inner: T,
}

impl<T> QuietShutdownStream<T> {
    pub fn new(inner: T) -> Self {
        Self { inner }
    }
}

impl<T> AsyncRead for QuietShutdownStream<T>
where
    T: AsyncRead,
{
    #[inline]
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<Result<(), Error>> {
        let this = self.project();
        this.inner.poll_read(cx, buf)
    }
}

impl<T> AsyncWrite for QuietShutdownStream<T>
where
    T: AsyncWrite,
{
    #[inline]
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<Result<usize, Error>> {
        let this = self.project();
        this.inner.poll_write(cx, buf)
    }

    #[inline]
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Error>> {
        let this = self.project();
        this.inner.poll_flush(cx)
    }

    #[inline]
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Error>> {
        // This is useful to wrap a SslStream. See here for why:
        // https://github.com/sfackler/tokio-openssl/issues/27
        let this = self.project();
        let res = ready!(this.inner.poll_shutdown(cx));
        let res = match res {
            Ok(r) => Ok(r),
            Err(e) if e.kind() == ErrorKind::NotConnected => Ok(()),
            Err(e) if e.kind() == ErrorKind::BrokenPipe => Ok(()),
            Err(e) => Err(e),
        };
        Poll::Ready(res)
    }
}
