/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::convert::TryInto;

use anyhow::Error;
use bytes::Bytes;
use futures::{
    channel::{mpsc, oneshot::Sender},
    Stream, StreamExt,
};
use gotham::{handler::HandlerError, state::State};
use gotham_derive::StateData;
use hyper::{
    header::{HeaderValue, CONTENT_LENGTH, CONTENT_TYPE},
    Body, Response, StatusCode,
};
use mime::Mime;

use crate::error::HttpError;
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

#[derive(StateData)]
pub struct ResponseContentLength(pub u64);

pub struct EmptyBody;

impl EmptyBody {
    pub fn new() -> Self {
        Self
    }
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

        state.put(ResponseContentLength(bytes.len().try_into()?));

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
    content_length: Option<u64>,
    signal_sender: Option<Sender<u64>>,
}

impl<S> StreamBody<S> {
    pub fn new(stream: S, mime: Mime) -> Self {
        Self {
            stream,
            mime,
            content_length: None,
            signal_sender: None,
        }
    }

    /// Set the value of the Content-Length HTTP header sent to
    /// the client. This should be equal to the total number of
    /// bytes that will be produced by the underlying Stream.
    /// If omitted, no Content-Length will be sent to the client.
    pub fn content_length(self, length: u64) -> Self {
        Self {
            content_length: Some(length),
            ..self
        }
    }

    /// Set a Sender to be notified when the Stream is exhausted
    /// and all data has been sent to the client. The total number
    /// of bytes sent will be passed along the channel.
    pub fn signal(self, sender: Sender<u64>) -> Self {
        Self {
            signal_sender: Some(sender),
            ..self
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
            mime,
            content_length,
            signal_sender,
        } = self;

        let mime_header: HeaderValue = mime.as_ref().parse()?;

        // This is kind of annoying, but right now Hyper requires a Body's stream to be Sync (even
        // though it doesn't actually need it). For now, we have to work around by spawning the
        // stream on its own task, and giving Hyper a channel that receives from it. Note that the
        // map(Ok) is here because we want to forward Result<Bytes, Error> instances over our
        // stream.
        let (sender, receiver) = mpsc::channel(0);
        tokio::spawn(stream.map(Ok).forward(sender));

        let stream = match signal_sender {
            Some(sender) => SignalStream::new(receiver, sender).left_stream(),
            None => receiver.right_stream(),
        };

        let mut res = Response::builder()
            .header(CONTENT_TYPE, mime_header)
            .status(StatusCode::OK);

        if let Some(content_length) = content_length {
            state.put(ResponseContentLength(content_length));
            res = res.header(CONTENT_LENGTH, content_length);
        }

        Ok(res.body(Body::wrap_stream(stream))?)
    }
}
