/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use bytes::Bytes;
use futures::{
    channel::mpsc,
    stream::{Stream, StreamExt},
};
use gotham::state::State;
use gotham_ext::{
    error::HttpError,
    response::{ResponseContentLength, TryIntoResponse},
    signal_stream::SignalStream,
};
use hyper::{
    header::{HeaderValue, CONTENT_LENGTH, CONTENT_TYPE},
    Body, Response, StatusCode,
};
use mime::Mime;

use crate::errors::LfsServerContextErrorKind;
use crate::middleware::RequestContext;

impl From<LfsServerContextErrorKind> for HttpError {
    fn from(e: LfsServerContextErrorKind) -> HttpError {
        use LfsServerContextErrorKind::*;
        match e {
            Forbidden => HttpError::e403(e),
            RepositoryDoesNotExist(_) => HttpError::e400(e),
            PermissionCheckFailed(_) => HttpError::e500(e),
        }
    }
}

pub struct StreamBody<S> {
    stream: S,
    content_length: u64,
    mime: Mime,
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
