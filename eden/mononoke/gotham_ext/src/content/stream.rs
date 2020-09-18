/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::pin::Pin;

use anyhow::Error;
use async_compression::stream::{GzipEncoder, ZstdEncoder};
use bytes::Bytes;
use futures::{
    stream::{BoxStream, Stream, StreamExt, TryStreamExt},
    task::{Context, Poll},
};
use pin_project::pin_project;

use super::encoding::{ContentCompression, ContentEncoding};

pub trait ContentMeta {
    /// Provide the content (i.e. Content-Encoding) for the underlying content. This will be sent
    /// to the client.
    fn content_encoding(&self) -> ContentEncoding;

    /// Provide the length of the content in this stream, if available (i.e. Content-Length). If
    /// provided, this must be the actual length of the stream. If missing, the transfer will be
    /// chunked.
    fn content_length(&self) -> Option<u64>;
}

#[pin_project]
pub struct CompressedContentStream<'a> {
    inner: BoxStream<'a, Result<Bytes, Error>>,
    content_compression: ContentCompression,
}

impl<'a> CompressedContentStream<'a> {
    pub fn new<S>(inner: S, content_compression: ContentCompression) -> Self
    where
        S: Stream<Item = Result<Bytes, Error>> + Send + 'a,
    {
        use std::io;

        let inner = inner.map_err(|e| io::Error::new(io::ErrorKind::Other, e));

        let inner = match content_compression {
            ContentCompression::Zstd => ZstdEncoder::new(inner).map_err(Error::from).boxed(),
            ContentCompression::Gzip => GzipEncoder::new(inner).map_err(Error::from).boxed(),
        };

        Self {
            inner,
            content_compression,
        }
    }
}

impl ContentMeta for CompressedContentStream<'_> {
    fn content_length(&self) -> Option<u64> {
        None
    }

    fn content_encoding(&self) -> ContentEncoding {
        ContentEncoding::Compressed(self.content_compression)
    }
}

impl Stream for CompressedContentStream<'_> {
    type Item = Result<Bytes, Error>;

    fn poll_next(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.project().inner.poll_next_unpin(ctx)
    }
}

#[pin_project]
pub struct ContentStream<S> {
    #[pin]
    inner: S,
    content_length: Option<u64>,
}

impl<S> ContentStream<S> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            content_length: None,
        }
    }

    /// Set a Content-Length for this stream. This *must* match the exact size of the uncompressed
    /// content that will be sent, since that is what the client will expect.
    pub fn content_length(self, content_length: u64) -> Self {
        Self {
            content_length: Some(content_length),
            ..self
        }
    }
}

impl<S> ContentMeta for ContentStream<S> {
    fn content_length(&self) -> Option<u64> {
        self.content_length
    }

    fn content_encoding(&self) -> ContentEncoding {
        ContentEncoding::Identity
    }
}

impl<S> Stream for ContentStream<S>
where
    S: Stream,
{
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.project().inner.poll_next(ctx)
    }
}

/// Provide an implementation of ContentMeta that propagates through Either (i.e. left_stream(),
/// right_stream()).
impl<A, B> ContentMeta for futures::future::Either<A, B>
where
    A: ContentMeta,
    B: ContentMeta,
{
    fn content_length(&self) -> Option<u64> {
        // left_stream(), right_stream() doesn't change the stream data.
        match self {
            Self::Left(a) => a.content_length(),
            Self::Right(b) => b.content_length(),
        }
    }

    fn content_encoding(&self) -> ContentEncoding {
        // left_stream(), right_stream() doesn't change the stream data.
        match self {
            Self::Left(a) => a.content_encoding(),
            Self::Right(b) => b.content_encoding(),
        }
    }
}

impl<S, F> ContentMeta for futures::stream::InspectOk<S, F>
where
    S: ContentMeta,
{
    fn content_length(&self) -> Option<u64> {
        // inspect_ok doesn't change the stream data.
        self.get_ref().content_length()
    }

    fn content_encoding(&self) -> ContentEncoding {
        // inspect_ok doesn't change the stream data.
        self.get_ref().content_encoding()
    }
}
