/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! cbor.rs - Utilities for working with CBOR data in HTTP requests and responses.
use std::pin::Pin;

use anyhow::format_err;
use anyhow::Context;
use anyhow::Error;
use bytes::Bytes;
use edenapi_types::ToApi;
use edenapi_types::ToWire;
use futures::prelude::*;
use futures::ready;
use futures::task::Poll;
use gotham::state::State;
use mime::Mime;
use once_cell::sync::Lazy;
use pin_project::pin_project;
use serde::de::DeserializeOwned;
use serde::Serialize;

use gotham_ext::content_encoding::ContentEncoding;
use gotham_ext::error::HttpError;
use gotham_ext::response::ContentMetaProvider;
use gotham_ext::response::ErrorMeta;
use gotham_ext::response::ErrorMetaProvider;
use gotham_ext::response::ResponseStream;
use gotham_ext::response::ResponseTryStreamExt;
use gotham_ext::response::StreamBody;
use gotham_ext::response::TryIntoResponse;

use crate::errors::ErrorKind;

use super::get_request_body;

static CBOR_MIME: Lazy<Mime> = Lazy::new(|| "application/cbor".parse().unwrap());

pub fn cbor_mime() -> Mime {
    CBOR_MIME.clone()
}

pub fn to_cbor_bytes<S: Serialize>(s: &S) -> Result<Bytes, Error> {
    serde_cbor::to_vec(s)
        .map(Bytes::from)
        .context(ErrorKind::SerializationFailed)
}

/// Serialize each item of the input stream as CBOR and return a streaming
/// response. Any errors yielded by the stream will be filtered out.
pub fn cbor_stream_filtered_errors<S, T>(stream: S) -> impl TryIntoResponse
where
    S: Stream<Item = Result<T, Error>> + Send + 'static,
    T: Serialize + Send + 'static,
{
    let byte_stream = stream.and_then(|item| async move { to_cbor_bytes(&item) });
    let content_stream = ResponseStream::new(byte_stream).capture_first_err();

    StreamBody::new(content_stream, cbor_mime())
}

pub fn custom_cbor_stream<S, T, C, E>(stream: S, error_classifier: C) -> impl TryIntoResponse
where
    S: Stream<Item = T> + Send + 'static,
    T: ToWire + Send + 'static,
    C: (Fn(&T) -> Option<&E>) + Send + 'static,
    E: std::error::Error,
{
    let content_stream = CustomCborStream::new(stream, error_classifier);
    StreamBody::new(content_stream, cbor_mime())
}

pub async fn parse_cbor_request<R: DeserializeOwned>(state: &mut State) -> Result<R, HttpError> {
    let body = get_request_body(state).await?;
    serde_cbor::from_slice(&body)
        .context(ErrorKind::DeserializationFailed)
        .map_err(HttpError::e400)
}

pub async fn parse_wire_request<R: DeserializeOwned + ToApi>(
    state: &mut State,
) -> Result<<R as ToApi>::Api, HttpError>
where
    <R as ToApi>::Error: Send + Sync + 'static + std::error::Error,
{
    let cbor = parse_cbor_request::<R>(state).await?;
    cbor.to_api().map_err(HttpError::e400)
}

/// Supports error reporting for endpoint that serialize errors to clients.
#[pin_project]
pub struct CustomCborStream<S, F> {
    #[pin]
    stream: S,
    /// Evaluates whether an item is an error.
    classifier: F,
    /// First error this stream encountered.
    first_error: Option<Error>,
    /// Count of errors that were observed but not captured.
    extra_error_count: u64,
}

impl<S, F> CustomCborStream<S, F> {
    pub fn new(stream: S, classifier: F) -> Self {
        Self {
            stream,
            classifier,
            first_error: None,
            extra_error_count: 0,
        }
    }
}

impl<S, F, E> Stream for CustomCborStream<S, F>
where
    S: Stream,
    S::Item: ToWire,
    F: Fn(&S::Item) -> Option<&E>,
    E: std::fmt::Debug,
{
    type Item = Bytes;

    fn poll_next(
        self: Pin<&mut Self>,
        ctx: &mut futures::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        match ready!(this.stream.as_mut().poll_next(ctx)) {
            Some(item) => {
                let classifier = &mut this.classifier;
                if let Some(e) = classifier(&item) {
                    if this.first_error.is_some() {
                        *this.extra_error_count += 1;
                    } else {
                        this.first_error.replace(format_err!("{:?}", e));
                    }
                }
                match to_cbor_bytes(&item.to_wire()) {
                    Ok(serialized) => Poll::Ready(Some(serialized)),
                    Err(e) => {
                        if this.first_error.is_some() {
                            *this.extra_error_count += 1;
                        } else {
                            this.first_error.replace(e);
                        }
                        // We end the stream early to signal that we have an unexpected error.
                        // Avoiding the error would make it too similar to "application" errors
                        // which are commonly omitted in the EdenApi response stream.
                        Poll::Ready(None)
                    }
                }
            }
            None => Poll::Ready(None),
        }
    }
}

impl<S, F> ErrorMetaProvider<Error> for CustomCborStream<S, F>
where
    S: Stream,
{
    fn report_errors(self: Pin<&mut Self>, errors: &mut ErrorMeta<Error>) {
        let this = self.project();
        errors.errors.extend(this.first_error.take());
        errors.extra_error_count += *this.extra_error_count;
    }
}

impl<S, F> ContentMetaProvider for CustomCborStream<S, F> {
    fn content_encoding(&self) -> ContentEncoding {
        ContentEncoding::Identity
    }

    fn content_length(&self) -> Option<u64> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde::Deserialize;

    #[derive(Serialize, Deserialize, Clone)]
    struct CustomWireResult {
        ok: Option<i32>,
        err: Option<i32>,
    }

    impl CustomWireResult {
        pub fn ok(ok: i32) -> Self {
            CustomWireResult {
                ok: Some(ok),
                err: None,
            }
        }

        pub fn err(err: i32) -> Self {
            CustomWireResult {
                ok: None,
                err: Some(err),
            }
        }
    }

    struct CustomResult(Result<i32, i32>);

    impl ToWire for CustomResult {
        type Wire = CustomWireResult;

        fn to_wire(self) -> Self::Wire {
            match self.0 {
                Ok(t) => CustomWireResult::ok(t),
                Err(e) => CustomWireResult::err(e),
            }
        }
    }

    impl ToApi for CustomWireResult {
        type Api = CustomResult;
        type Error = std::convert::Infallible;

        fn to_api(self) -> Result<Self::Api, Self::Error> {
            unimplemented!()
        }
    }

    #[tokio::test]
    async fn test_monitor_err() {
        let results = vec![
            CustomResult(Ok(10)),
            CustomResult(Ok(12)),
            CustomResult(Err(-1)),
            CustomResult(Ok(11)),
            CustomResult(Err(-2)),
        ];
        fn classify(x: &CustomResult) -> Option<&i32> {
            x.0.as_ref().err()
        }
        let s = CustomCborStream::new(stream::iter(results), classify);

        futures::pin_mut!(s);

        let tcb = |cr: &CustomWireResult| to_cbor_bytes(cr).unwrap();
        assert_eq!(s.next().await, Some(tcb(&CustomWireResult::ok(10))));
        assert_eq!(s.next().await, Some(tcb(&CustomWireResult::ok(12))));
        assert_eq!(s.next().await, Some(tcb(&CustomWireResult::err(-1))));
        assert_eq!(s.next().await, Some(tcb(&CustomWireResult::ok(11))));
        assert_eq!(s.next().await, Some(tcb(&CustomWireResult::err(-2))));
        assert_eq!(s.next().await, None);

        let mut errors = ErrorMeta::new();
        s.report_errors(&mut errors);
        assert_eq!(errors.errors.len(), 1);
        assert_eq!(errors.extra_error_count, 1);
        assert_eq!(format!("{:?}", errors.errors[0]).as_str(), "-1");
    }
}
