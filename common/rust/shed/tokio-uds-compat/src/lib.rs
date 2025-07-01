/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is dual-licensed under either the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree or the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree. You may select, at your option, one of the
 * above-listed licenses.
 */

#[cfg(unix)]
pub use tokio::net::UnixListener;
#[cfg(unix)]
pub use tokio::net::UnixStream;
#[cfg(unix)]
pub use tokio::net::unix::OwnedReadHalf;
#[cfg(unix)]
pub use tokio::net::unix::OwnedWriteHalf;

#[cfg(windows)]
mod windows {
    use std::future::Future;
    use std::io;
    use std::net::Shutdown;
    use std::path::Path;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::task::Context;
    use std::task::Poll;

    /// Compat layer for providing UNIX domain socket on Windows
    use async_io::Async;
    use tokio::io::AsyncRead;
    use tokio::io::AsyncWrite;
    use tokio::io::ReadBuf;

    pub struct OwnedReadHalf {
        inner: Arc<UnixStream>,
    }

    impl OwnedReadHalf {
        fn new(inner: Arc<UnixStream>) -> Self {
            Self { inner }
        }
    }

    impl AsyncRead for OwnedReadHalf {
        fn poll_read(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            self.inner.poll_read_priv(cx, buf)
        }
    }

    pub struct OwnedWriteHalf {
        inner: Arc<UnixStream>,
        shutdown_on_drop: bool,
    }

    impl OwnedWriteHalf {
        fn new(inner: Arc<UnixStream>) -> Self {
            Self {
                inner,
                shutdown_on_drop: true,
            }
        }
    }

    impl Drop for OwnedWriteHalf {
        fn drop(&mut self) {
            if self.shutdown_on_drop {
                let _ = self.inner.async_ref().as_ref().shutdown(Shutdown::Write);
            }
        }
    }

    impl AsyncWrite for OwnedWriteHalf {
        fn poll_write(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<Result<usize, io::Error>> {
            futures::AsyncWrite::poll_write(Pin::new(&mut self.inner.async_ref()), cx, buf)
        }

        fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
            futures::AsyncWrite::poll_flush(Pin::new(&mut self.inner.async_ref()), cx)
        }

        fn poll_shutdown(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<Result<(), io::Error>> {
            futures::AsyncWrite::poll_close(Pin::new(&mut self.inner.async_ref()), cx)
        }
    }

    #[derive(Debug)]
    pub struct UnixStream(Async<uds_windows::UnixStream>);

    impl UnixStream {
        pub async fn connect<P: AsRef<Path>>(path: P) -> io::Result<Self> {
            let stream = uds_windows::UnixStream::connect(path)?;
            Self::from_std(stream)
        }

        fn from_std(stream: uds_windows::UnixStream) -> io::Result<Self> {
            let stream = Async::new(stream)?;

            Ok(UnixStream(stream))
        }

        fn async_ref(&self) -> &Async<uds_windows::UnixStream> {
            &self.0
        }

        fn inner_mut(self: Pin<&mut Self>) -> Pin<&mut Async<uds_windows::UnixStream>> {
            Pin::new(&mut self.get_mut().0)
        }

        pub fn into_split(self) -> (OwnedReadHalf, OwnedWriteHalf) {
            let this = Arc::new(self);
            (OwnedReadHalf::new(this.clone()), OwnedWriteHalf::new(this))
        }

        fn poll_read_priv(
            &self,
            cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<Result<(), io::Error>> {
            let result = futures::AsyncRead::poll_read(
                Pin::new(&mut self.async_ref()),
                cx,
                buf.initialize_unfilled(),
            );

            match result {
                Poll::Ready(Ok(written)) => {
                    tracing::trace!(?written, "UnixStream::poll_read");
                    buf.set_filled(written);
                    Poll::Ready(Ok(()))
                }
                Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
                Poll::Pending => Poll::Pending,
            }
        }
    }

    impl AsyncRead for UnixStream {
        fn poll_read(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<Result<(), io::Error>> {
            self.poll_read_priv(cx, buf)
        }
    }

    impl AsyncWrite for UnixStream {
        fn poll_write(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<Result<usize, io::Error>> {
            futures::AsyncWrite::poll_write(self.inner_mut(), cx, buf)
        }

        fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
            futures::AsyncWrite::poll_flush(self.inner_mut(), cx)
        }

        fn poll_shutdown(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<Result<(), io::Error>> {
            futures::AsyncWrite::poll_close(self.inner_mut(), cx)
        }
    }

    #[derive(Debug)]
    pub struct UnixListener(Async<uds_windows::UnixListener>);

    impl UnixListener {
        pub fn bind<P: AsRef<Path>>(path: P) -> io::Result<Self> {
            let listener = uds_windows::UnixListener::bind(path)?;
            let listener = Async::new(listener)?;

            Ok(UnixListener(listener))
        }

        pub async fn accept(&self) -> io::Result<(UnixStream, uds_windows::SocketAddr)> {
            futures::future::poll_fn(|cx| self.poll_accept(cx)).await
        }

        pub fn poll_accept(
            &self,
            cx: &mut Context<'_>,
        ) -> Poll<io::Result<(UnixStream, uds_windows::SocketAddr)>> {
            match self.0.poll_readable(cx) {
                Poll::Ready(Ok(())) => {
                    let result = self.0.read_with(|io| io.accept());
                    let mut result = Box::pin(result);
                    result.as_mut().poll(cx).map(|x| {
                        x.and_then(|(stream, addr)| {
                            let stream = UnixStream::from_std(stream)?;
                            Ok((stream, addr))
                        })
                    })
                }
                Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
                Poll::Pending => Poll::Pending,
            }
        }
    }
}

#[cfg(windows)]
pub use self::windows::OwnedReadHalf;
#[cfg(windows)]
pub use self::windows::OwnedWriteHalf;
#[cfg(windows)]
pub use self::windows::UnixListener;
#[cfg(windows)]
pub use self::windows::UnixStream;
