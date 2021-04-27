/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::convert::TryInto;

use anyhow::{Context, Error};
use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use gotham::{handler::HandlerError, state::State};
use gotham_derive::StateData;
use hyper::{
    header::{HeaderValue, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE},
    Body, Response, StatusCode,
};
use mime::Mime;

use crate::content_encoding::{ContentCompression, ContentEncoding};
use crate::error::{ErrorFormatter, HttpError};
use crate::middleware::PostResponseCallbacks;

use super::content_meta::ContentMeta;
use super::signal_stream::SignalStream;

pub trait TryIntoResponse {
    fn try_into_response(self, state: &mut State) -> Result<Response<Body>, Error>;
}

pub fn build_response<IR: TryIntoResponse, F: ErrorFormatter>(
    res: Result<IR, HttpError>,
    mut state: State,
    formatter: &F,
) -> Result<(State, Response<Body>), (State, HandlerError)> {
    let res = res.and_then(|c| {
        c.try_into_response(&mut state)
            .context("try_into_response failed!")
            .map_err(HttpError::e500)
    });
    match res {
        Ok(res) => Ok((state, res)),
        Err(e) => e.into_handler_response(state, formatter),
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
    pub partial: bool,
}

impl<S> StreamBody<S> {
    pub fn new(stream: S, mime: Mime) -> Self {
        Self {
            stream,
            mime,
            partial: false,
        }
    }
}

impl<S> TryIntoResponse for StreamBody<S>
where
    S: Stream<Item = Bytes> + ContentMeta + Send + 'static,
{
    fn try_into_response(self, state: &mut State) -> Result<Response<Body>, Error> {
        let Self {
            stream,
            mime,
            partial,
        } = self;

        let status = if partial {
            StatusCode::PARTIAL_CONTENT
        } else {
            StatusCode::OK
        };

        let mime_header: HeaderValue = mime.as_ref().parse()?;

        let content_encoding = stream.content_encoding();
        let content_length = stream.content_length();

        let res = Response::builder()
            .header(CONTENT_TYPE, mime_header)
            .header(CONTENT_ENCODING, content_encoding)
            .status(status);

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

        // If PostResponseMiddleware is in use, arrange for post-request
        // callbacks to be delayed until the entire stream has been sent.
        let stream = match state.try_borrow_mut::<PostResponseCallbacks>() {
            Some(callbacks) => SignalStream::new(stream, callbacks.delay()).left_stream(),
            None => stream.right_stream(),
        };

        // Turn the stream into a TryStream, as expected by hyper::Body.
        let stream = stream.map(<Result<_, Error>>::Ok);

        Ok(res.body(Body::wrap_stream(stream))?)
    }
}
