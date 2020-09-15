/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::convert::TryInto;

use anyhow::Error;
use async_compression::stream::{BrotliEncoder, GzipEncoder, ZstdEncoder};
use bytes::Bytes;
use futures::{
    channel::mpsc,
    stream::{BoxStream, Stream, StreamExt, TryStreamExt},
    task::{Context, Poll},
};
use gotham::{handler::HandlerError, state::State};
use gotham_derive::StateData;
use hyper::{
    header::{HeaderValue, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE},
    Body, Response, StatusCode,
};
use mime::Mime;
use pin_project::pin_project;
use std::pin::Pin;

use crate::content_encoding::{ContentCompression, ContentEncoding};
use crate::error::HttpError;
use crate::middleware::PostRequestCallbacks;
use crate::signal_stream::SignalStream;

pub trait TryIntoResponse {
    fn try_into_response(self, state: &mut State) -> Result<Response<Body>, Error>;
}

pub fn build_response<IR: TryIntoResponse>(
    res: Result<IR, HttpError>,
    mut state: State,
) -> Result<(State, Response<Body>), (State, HandlerError)> {
    let res = res.and_then(|c| c.try_into_response(&mut state).map_err(HttpError::e500));
    match res {
        Ok(res) => Ok((state, res)),
        Err(e) => e.into_handler_response(state),
    }
}

#[derive(StateData, Copy, Clone, Debug)]
pub enum ResponseContentMeta {
    Sized(u64),
    Chunked,
    Compressed(ContentCompression),
}

impl ResponseContentMeta {
    pub fn content_length(&self) -> Option<u64> {
        match self {
            Self::Sized(s) => Some(*s),
            Self::Compressed(..) => None,
            Self::Chunked => None,
        }
    }
}

pub struct EmptyBody;

impl EmptyBody {
    pub fn new() -> Self {
        Self
    }
}

impl TryIntoResponse for EmptyBody {
    fn try_into_response(self, state: &mut State) -> Result<Response<Body>, Error> {
        state.put(ResponseContentMeta::Sized(0));

        Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_LENGTH, 0)
            .body(Body::empty())
            .map_err(Error::from)
    }
}

pub struct BytesBody<B> {
    bytes: B,
    mime: Mime,
}

impl<B> BytesBody<B> {
    pub fn new(bytes: B, mime: Mime) -> Self {
        Self { bytes, mime }
    }
}

impl<B> TryIntoResponse for BytesBody<B>
where
    B: Into<Bytes>,
{
    fn try_into_response(self, state: &mut State) -> Result<Response<Body>, Error> {
        let bytes = self.bytes.into();
        let mime_header: HeaderValue = self.mime.as_ref().parse()?;

        state.put(ResponseContentMeta::Sized(bytes.len().try_into()?));

        Response::builder()
            .header(CONTENT_TYPE, mime_header)
            .status(StatusCode::OK)
            .body(bytes.into())
            .map_err(Error::from)
    }
}

pub struct StreamBody<S> {
    stream: S,
    mime: Mime,
}

impl<S> StreamBody<S> {
    pub fn new(stream: S, mime: Mime) -> Self {
        Self { stream, mime }
    }
}

impl<S> TryIntoResponse for StreamBody<S>
where
    S: Stream<Item = Result<Bytes, Error>> + ContentMeta + Send + 'static,
{
    fn try_into_response(self, state: &mut State) -> Result<Response<Body>, Error> {
        let Self { stream, mime } = self;

        let mime_header: HeaderValue = mime.as_ref().parse()?;

        let content_encoding = stream.content_encoding();
        let content_length = stream.content_length();

        let res = Response::builder()
            .header(CONTENT_TYPE, mime_header)
            .header(CONTENT_ENCODING, content_encoding)
            .status(StatusCode::OK);

        let (res, meta) = match content_encoding {
            ContentEncoding::Compressed(compression) => {
                (res, ResponseContentMeta::Compressed(compression))
            }
            ContentEncoding::Identity => match content_length {
                Some(content_length) => (
                    res.header(CONTENT_LENGTH, content_length),
                    ResponseContentMeta::Sized(content_length),
                ),
                None => (res, ResponseContentMeta::Chunked),
            },
        };

        state.put(meta);

        // This is kind of annoying, but right now Hyper requires a Body's stream to be Sync (even
        // though it doesn't actually need it). For now, we have to work around by spawning the
        // stream on its own task, and giving Hyper a channel that receives from it. Note that the
        // map(Ok) is here because we want to forward Result<Bytes, Error> instances over our
        // stream.
        // TODO: This is fixed now in Hyper: https://github.com/hyperium/hyper/pull/2187
        let (sender, receiver) = mpsc::channel(0);
        tokio::spawn(stream.map(Ok).forward(sender));

        // If `PostRequestMiddleware` is in use, arrange for post-request
        // callbacks to be delayed until the entire stream has been sent.
        let stream = match state.try_borrow_mut::<PostRequestCallbacks>() {
            Some(callbacks) => SignalStream::new(receiver, callbacks.delay()).left_stream(),
            None => receiver.right_stream(),
        };

        Ok(res.body(Body::wrap_stream(stream))?)
    }
}

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
            ContentCompression::Brotli => BrotliEncoder::new(inner).map_err(Error::from).boxed(),
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
