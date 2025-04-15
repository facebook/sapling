/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Error;
use bytes::Bytes;
use either::Either;
use futures::channel::oneshot;
use futures::stream::Stream;
use futures::stream::StreamExt;
use futures_stats::TimedStreamExt;
use gotham::handler::HandlerError;
use gotham::helpers::http::response::create_response;
use gotham::state::State;
use hyper::Body;
use hyper::Response;
use hyper::StatusCode;
use hyper::header::CONTENT_ENCODING;
use hyper::header::CONTENT_LENGTH;
use hyper::header::CONTENT_TYPE;
use hyper::header::HeaderValue;
use mime::Mime;

use super::content_meta::ContentMetaProvider;
use super::error_meta::ErrorMetaProvider;
use super::response_meta::HeadersMeta;
use super::response_meta::PendingResponseMeta;
use super::signal_stream::SignalStream;
use super::stream_stats::PendingStreamStats;
use crate::content_encoding::ContentEncoding;
use crate::error::ErrorFormatter;
use crate::error::HttpError;

pub trait TryIntoResponse {
    fn try_into_response(self, state: &mut State) -> Result<Response<Body>, Error>;
}

impl<L, R> TryIntoResponse for Either<L, R>
where
    L: TryIntoResponse,
    R: TryIntoResponse,
{
    fn try_into_response(self, state: &mut State) -> Result<Response<Body>, Error> {
        match self {
            Either::Left(l) => l.try_into_response(state),
            Either::Right(r) => r.try_into_response(state),
        }
    }
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
        Err(err) => build_error_response(err, state, formatter),
    }
}

pub fn build_error_response<F: ErrorFormatter>(
    err: HttpError,
    mut state: State,
    formatter: &F,
) -> Result<(State, Response<Body>), (State, HandlerError)> {
    let formatted = formatter.format(&err.error, &state);

    state.put(PendingResponseMeta::error(err.error));

    match formatted {
        Ok((body, mime)) => {
            let res = create_response(&state, err.status_code, mime, body);
            Ok((state, res))
        }
        Err(error) => Err((state, error.into())),
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
        state.put(PendingStreamStats::none());
        state.put(PendingResponseMeta::immediate(0));

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

        let size = bytes.len().try_into()?;
        state.put(PendingResponseMeta::immediate(size));
        state.put(PendingStreamStats::none());

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
    S: Stream<Item = Bytes> + ContentMetaProvider + ErrorMetaProvider<Error> + Send + 'static,
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

        let (res, headers_meta) = match content_encoding {
            ContentEncoding::Compressed(compression) => (res, HeadersMeta::Compressed(compression)),
            ContentEncoding::Identity => match content_length {
                Some(content_length) => (
                    res.header(CONTENT_LENGTH, content_length),
                    HeadersMeta::Sized(content_length),
                ),
                None => (res, HeadersMeta::Chunked),
            },
        };

        let (meta_tx, meta_rx) = oneshot::channel();
        state.put(PendingResponseMeta::deferred(headers_meta, meta_rx));

        // Set up a SignalStream to send the PostSendMeta.
        let stream = SignalStream::new(stream, meta_tx);

        let (stats_tx, stats_rx) = oneshot::channel();
        state.put(PendingStreamStats::deferred(stats_rx));

        let stream = stream.timed(move |stats| {
            let _ = stats_tx.send(stats);
        });

        // Turn the stream into a TryStream, as expected by hyper::Body.
        let stream = stream.map(<Result<_, Error>>::Ok);

        Ok(res.body(Body::wrap_stream(stream))?)
    }
}
