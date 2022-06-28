/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::pin::Pin;

use anyhow::Error;
use async_compression::tokio::bufread::GzipEncoder;
use async_compression::tokio::bufread::ZstdEncoder;
use bytes::Bytes;
use futures::future::Either;
use futures::stream::BoxStream;
use futures::stream::Stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures::task::Context;
use futures::task::Poll;
use pin_project::pin_project;
use tokio_util::io::ReaderStream;
use tokio_util::io::StreamReader;

use crate::content_encoding::ContentCompression;
use crate::content_encoding::ContentEncoding;

/// Create a response stream using the specified Content-Encoding.
///
/// The resulting stream may or may not be compressed depending on the chosen encoding. Optionally,
/// the caller can specify the value for the `Content-Length` header. This is only useful in cases
/// where the response isn't compressed (i.e., the encoding is set to `ContentEncoding::Identity`)
/// because otherwise, we would need to send the post-compression size of the content, which cannot
/// be known in advance.
pub fn encode_stream<S>(
    stream: S,
    encoding: ContentEncoding,
    length: Option<u64>,
) -> Either<ResponseStream<S>, CompressedResponseStream<'static>>
where
    S: Stream<Item = Result<Bytes, Error>> + Send + 'static,
{
    match (encoding, length) {
        (ContentEncoding::Identity, Some(size)) => ResponseStream::new(stream)
            .set_content_length(size)
            .left_stream(),
        (ContentEncoding::Identity, None) => ResponseStream::new(stream).left_stream(),
        (ContentEncoding::Compressed(c), _) => {
            CompressedResponseStream::new(stream, c).right_stream()
        }
    }
}

#[pin_project]
pub struct CompressedResponseStream<'a> {
    inner: BoxStream<'a, Result<Bytes, Error>>,
    content_compression: ContentCompression,
}

impl<'a> CompressedResponseStream<'a> {
    pub fn new<S>(inner: S, content_compression: ContentCompression) -> Self
    where
        S: Stream<Item = Result<Bytes, Error>> + Send + 'a,
    {
        use std::io;

        // 2MiB, for LFS that's at least once every content chunk.
        const YIELD_EVERY: usize = 2 * 1024 * 1024;

        let inner = inner.map_err(|e| io::Error::new(io::ErrorKind::Other, e));
        let inner = YieldStream::new(inner, YIELD_EVERY);
        let inner = StreamReader::new(inner);

        let inner = match content_compression {
            ContentCompression::Zstd => ReaderStream::new(ZstdEncoder::new(inner))
                .map_err(Error::from)
                .boxed(),
            ContentCompression::Gzip => ReaderStream::new(GzipEncoder::new(inner))
                .map_err(Error::from)
                .boxed(),
        };

        Self {
            inner,
            content_compression,
        }
    }

    pub fn content_compression(&self) -> ContentCompression {
        self.content_compression
    }
}

impl Stream for CompressedResponseStream<'_> {
    type Item = Result<Bytes, Error>;

    fn poll_next(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.project().inner.poll_next_unpin(ctx)
    }
}

#[pin_project]
pub struct ResponseStream<S> {
    #[pin]
    inner: S,
    content_length: Option<u64>,
}

impl<S> ResponseStream<S> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            content_length: None,
        }
    }

    /// Set a Content-Length for this stream. This *must* match the exact size of the uncompressed
    /// content that will be sent, since that is what the client will expect.
    pub fn set_content_length(self, content_length: u64) -> Self {
        Self {
            content_length: Some(content_length),
            ..self
        }
    }

    pub fn content_length(&self) -> Option<u64> {
        self.content_length
    }
}

impl<S> Stream for ResponseStream<S>
where
    S: Stream,
{
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.project().inner.poll_next(ctx)
    }
}

/// This is a helper that forces the underlying stream to yield (i.e. return Pending) periodically.
/// This is useful with compression, because our compression library will try to compress as much
/// as it can. If the data is always ready (which it often is with e.g. LFS, where we have
/// everything in cache most of the time), then it'll compress the entire stream before returning,
/// which is good for compression performance, but terrible for time-to-first-byte. So, we force
/// our compression to periodically stop compresing (every YIELD_EVERY).
#[pin_project]
pub struct YieldStream<S> {
    read: usize,
    yield_every: usize,
    #[pin]
    inner: S,
}

impl<S> YieldStream<S> {
    pub fn new(inner: S, yield_every: usize) -> Self {
        Self {
            read: 0,
            yield_every,
            inner,
        }
    }
}

impl<S, E> Stream for YieldStream<S>
where
    S: Stream<Item = Result<Bytes, E>>,
{
    type Item = Result<Bytes, E>;

    fn poll_next(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut projection = self.project();

        if *projection.read >= *projection.yield_every {
            *projection.read %= *projection.yield_every;
            ctx.waker().wake_by_ref();
            return Poll::Pending;
        }

        let ret = futures::ready!(projection.inner.poll_next_unpin(ctx));
        if let Some(Ok(ref bytes)) = ret {
            *projection.read += bytes.len();
        }

        Poll::Ready(ret)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use futures::stream;

    #[tokio::test]
    async fn test_yield_stream() {
        // NOTE: This tests that the yield probably wakes up but assumes it yields.

        let data = &[b"foo".as_ref(), b"bar2".as_ref()];
        let data = stream::iter(
            data.iter()
                .map(|d| Result::<_, ()>::Ok(Bytes::copy_from_slice(d))),
        );
        let mut stream = YieldStream::new(data, 1);

        assert_eq!(
            stream.next().await,
            Some(Ok(Bytes::copy_from_slice(b"foo")))
        );

        assert!(stream.read > stream.yield_every);

        assert_eq!(
            stream.next().await,
            Some(Ok(Bytes::copy_from_slice(b"bar2")))
        );

        assert_eq!(stream.next().await, None,);
    }
}
