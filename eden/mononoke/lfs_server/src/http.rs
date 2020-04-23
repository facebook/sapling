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
};
use gotham::state::State;
use gotham_derive::StateData;
use gotham_ext::{error::HttpError, signal_stream::SignalStream};
use hyper::{
    header::{HeaderValue, CONTENT_LENGTH, CONTENT_TYPE},
    Body, Response, StatusCode,
};
use lazy_static::lazy_static;
use mime::Mime;

use crate::errors::LfsServerContextErrorKind;
use crate::middleware::RequestContext;

#[derive(StateData)]
pub struct ResponseContentLength(pub u64);

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

lazy_static! {
    static ref GIT_LFS_MIME: mime::Mime =
        mime::Mime::from_str("application/vnd.git-lfs+json").unwrap();
}

pub fn git_lfs_mime() -> mime::Mime {
    GIT_LFS_MIME.clone()
}
