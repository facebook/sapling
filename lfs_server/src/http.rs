/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::convert::TryInto;
use std::error::Error as StdError;
use std::str::FromStr;

use bytes::Bytes;
use failure_ext::Error;
use futures::{try_ready, Async, Poll, Stream};
use futures_ext::StreamExt;
use gotham::state::State;
use hyper::{
    header::{HeaderValue, CONTENT_LENGTH, CONTENT_TYPE},
    Body, Response, StatusCode,
};
use lazy_static::lazy_static;
use mime::Mime;
use tokio::sync::oneshot::Sender;

use crate::errors::LfsServerContextErrorKind;
use crate::middleware::{RequestContext, ScubaKey, ScubaMiddlewareState};

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
    fn try_into_response(self, _state: &mut State) -> Result<Response<Body>, Error> {
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
    fn try_into_response(self, _state: &mut State) -> Result<Response<Body>, Error> {
        let bytes = self.bytes.into();
        let mime_header: HeaderValue = self.mime.as_ref().parse()?;

        Response::builder()
            .header(CONTENT_TYPE, mime_header)
            .status(StatusCode::OK)
            .body(bytes.into())
            .map_err(Error::from)
    }
}

impl<S> TryIntoResponse for StreamBody<S>
where
    S: Stream<Item = Bytes> + Send + 'static,
    S::Error: Into<Box<dyn StdError + Send + Sync>>,
{
    fn try_into_response(self, state: &mut State) -> Result<Response<Body>, Error> {
        let Self {
            stream,
            content_length,
            mime,
        } = self;

        // Provide a response size hint for this: stream length cannot be derived from a
        // Response<Body> later!
        ScubaMiddlewareState::try_borrow_add(
            state,
            ScubaKey::ResponseContentLength,
            content_length,
        );

        let mime_header: HeaderValue = mime.as_ref().parse()?;

        let stream = if let Some(ctx) = state.try_borrow_mut::<RequestContext>() {
            let sender = ctx.delay_post_request();
            SignalStream::new(stream, sender).left_stream()
        } else {
            stream.right_stream()
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
}

impl<S: Stream> Stream for SignalStream<S>
where
    S::Item: Sizeable,
{
    type Item = S::Item;
    type Error = S::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        if self.sender.is_none() {
            return Ok(Async::Ready(None));
        }

        let poll = try_ready!(self.stream.poll());

        if let Some(ref item) = poll {
            // We have an item: increment the amount of data we sent.
            self.size_sent += item.size();
        } else {
            // No items left: signal our receiver.
            let _ = self
                .sender
                .take()
                .expect("presence checked above")
                .send(self.size_sent);
        }

        Ok(Async::Ready(poll))
    }
}

impl<S> Drop for SignalStream<S> {
    fn drop(&mut self) {
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(self.size_sent);
        }
    }
}
