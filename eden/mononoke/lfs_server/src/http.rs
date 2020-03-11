/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::convert::TryInto;
use std::str::FromStr;

use anyhow::Error;
use bytes::Bytes;
use futures::{
    channel::mpsc,
    stream::{Stream, StreamExt},
    task::{Context, Poll},
};
use gotham::state::State;
use gotham_derive::StateData;
use hyper::{
    header::{HeaderValue, CONTENT_LENGTH, CONTENT_TYPE},
    Body, Response, StatusCode,
};
use lazy_static::lazy_static;
use mime::Mime;
use std::pin::Pin;
use tokio_old::sync::oneshot::Sender;

use crate::errors::LfsServerContextErrorKind;
use crate::middleware::RequestContext;

#[derive(StateData)]
pub struct ResponseContentLength(pub u64);

// Provide an easy way to map from Error -> Http code
pub struct HttpError {
    pub error: Error,
    pub status_code: StatusCode,
}

impl From<LfsServerContextErrorKind> for HttpError {
    fn from(e: LfsServerContextErrorKind) -> HttpError {
        use LfsServerContextErrorKind::*;
        match e {
            Forbidden => HttpError::e403(e),
            RepositoryDoesNotExist(_) => HttpError::e400(e),
        }
    }
}

pub struct EmptyBody;

pub struct BytesBody<B> {
    bytes: B,
    mime: Mime,
}

pub struct StreamBody<S> {
    stream: S,
    content_length: u64,
    mime: Mime,
}

impl EmptyBody {
    pub fn new() -> Self {
        Self
    }
}

impl<B> BytesBody<B> {
    pub fn new(bytes: B, mime: Mime) -> Self {
        Self { bytes, mime }
    }
}

impl<S> StreamBody<S> {
    pub fn new(stream: S, content_length: u64, mime: Mime) -> Self {
        Self {
            stream,
            content_length,
            mime,
        }
    }
}

pub trait TryIntoResponse {
    fn try_into_response(self, state: &mut State) -> Result<Response<Body>, Error>;
}

impl TryIntoResponse for EmptyBody {
    fn try_into_response(self, state: &mut State) -> Result<Response<Body>, Error> {
        state.put(ResponseContentLength(0));

        Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_LENGTH, 0)
            .body(Body::empty())
            .map_err(Error::from)
    }
}

impl<B> TryIntoResponse for BytesBody<B>
where
    B: Into<Bytes>,
{
    fn try_into_response(self, state: &mut State) -> Result<Response<Body>, Error> {
        let bytes = self.bytes.into();
        let mime_header: HeaderValue = self.mime.as_ref().parse()?;

        state.put(ResponseContentLength(bytes.len().try_into()?));

        Response::builder()
            .header(CONTENT_TYPE, mime_header)
            .status(StatusCode::OK)
            .body(bytes.into())
            .map_err(Error::from)
    }
}

impl<S> TryIntoResponse for StreamBody<S>
where
    S: Stream<Item = Result<Bytes, Error>> + Send + 'static,
{
    fn try_into_response(self, state: &mut State) -> Result<Response<Body>, Error> {
        let Self {
            stream,
            content_length,
            mime,
        } = self;

        state.put(ResponseContentLength(content_length));

        let mime_header: HeaderValue = mime.as_ref().parse()?;

        // This is kind of annoying, but right now Hyper requires a Body's stream to be Sync (even
        // though it doesn't actually need it). For now, we have to work around by spawning the
        // stream on its own task, and giving Hyper a channel that receives from it. Note that the
        // map(Ok) is here because we want to forward Result<Bytes, Error> instances over our
        // stream.
        let (sender, receiver) = mpsc::channel(0);
        tokio::spawn(stream.map(Ok).forward(sender));

        let stream = if let Some(ctx) = state.try_borrow_mut::<RequestContext>() {
            let sender = ctx.delay_post_request();
            SignalStream::new(receiver, sender).left_stream()
        } else {
            receiver.right_stream()
        };

        Response::builder()
            .header(CONTENT_TYPE, mime_header)
            .header(CONTENT_LENGTH, content_length)
            .status(StatusCode::OK)
            .body(Body::wrap_stream(stream))
            .map_err(Error::from)
    }
}

impl HttpError {
    pub fn e400<E: Into<Error>>(err: E) -> Self {
        Self {
            error: err.into(),
            status_code: StatusCode::BAD_REQUEST,
        }
    }

    pub fn e403<E: Into<Error>>(err: E) -> Self {
        Self {
            error: err.into(),
            status_code: StatusCode::FORBIDDEN,
        }
    }

    pub fn e404<E: Into<Error>>(err: E) -> Self {
        Self {
            error: err.into(),
            status_code: StatusCode::NOT_FOUND,
        }
    }

    pub fn e429<E: Into<Error>>(err: E) -> Self {
        Self {
            error: err.into(),
            status_code: StatusCode::TOO_MANY_REQUESTS,
        }
    }

    pub fn e500<E: Into<Error>>(err: E) -> Self {
        Self {
            error: err.into(),
            status_code: StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn e502<E: Into<Error>>(err: E) -> Self {
        Self {
            error: err.into(),
            status_code: StatusCode::BAD_GATEWAY,
        }
    }
}

lazy_static! {
    static ref GIT_LFS_MIME: mime::Mime =
        mime::Mime::from_str("application/vnd.git-lfs+json").unwrap();
}

pub fn git_lfs_mime() -> mime::Mime {
    GIT_LFS_MIME.clone()
}

trait Sizeable {
    fn size(&self) -> u64;
}

impl Sizeable for Bytes {
    fn size(&self) -> u64 {
        // NOTE: It is reasonable to unwrap here because we're not going to have buffers of bytes
        // that are larger than a u64.
        self.len().try_into().unwrap()
    }
}

/// A stream that will fire to the sender associated upon completing or being dropped. The Sender
/// will receive the amount of data that passed through the stream.
struct SignalStream<S> {
    stream: S,
    sender: Option<Sender<u64>>,
    size_sent: u64,
}

impl<S> SignalStream<S> {
    fn new(stream: S, sender: Sender<u64>) -> Self {
        Self {
            stream,
            sender: Some(sender),
            size_sent: 0,
        }
    }

    fn pin_get_parts(self: Pin<&mut Self>) -> (Pin<&mut S>, &mut Option<Sender<u64>>, &mut u64) {
        // Pinning is structural for stream, non-structural for sender and size_sent.
        let this = unsafe { self.get_unchecked_mut() };
        let stream = unsafe { Pin::new_unchecked(&mut this.stream) };
        (stream, &mut this.sender, &mut this.size_sent)
    }

    fn pin_drop(self: Pin<&mut Self>) {
        let (_, sender, size_sent) = self.pin_get_parts();

        if let Some(sender) = sender.take() {
            let _ = sender.send(*size_sent);
        }
    }
}

impl<S, I, E> Stream for SignalStream<S>
where
    S: Stream<Item = Result<I, E>>,
    I: Sizeable,
{
    type Item = Result<I, E>;

    fn poll_next(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Option<Self::Item>> {
        let (stream, sender, size_sent) = self.pin_get_parts();

        if sender.is_none() {
            return Poll::Ready(None);
        }

        let poll = match stream.poll_next(ctx) {
            Poll::Ready(poll) => poll,
            Poll::Pending => {
                return Poll::Pending;
            }
        };

        if let Some(Ok(ref item)) = poll {
            // We have an item: increment the amount of data we sent.
            *size_sent += item.size();
        } else {
            // No items left: signal our receiver.
            let _ = sender
                .take()
                .expect("presence checked above")
                .send(*size_sent);
        }

        Poll::Ready(poll)
    }
}

impl<S> Drop for SignalStream<S> {
    fn drop(&mut self) {
        // `new_unchecked` is okay because we know this value is never used again after being
        // dropped.
        unsafe { Pin::new_unchecked(self) }.pin_drop();
    }
}
