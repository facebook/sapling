/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! cbor.rs - Utilities for working with CBOR data in HTTP requests and responses.

use anyhow::{Context, Error};
use bytes::Bytes;
use edenapi_types::ToApi;
use futures::prelude::*;
use gotham::state::State;
use mime::Mime;
use once_cell::sync::Lazy;
use serde::{de::DeserializeOwned, Serialize};

use gotham_ext::{
    error::HttpError,
    response::{ResponseStream, ResponseTryStreamExt},
    response::{StreamBody, TryIntoResponse},
};

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
pub fn cbor_stream<S, T>(stream: S) -> impl TryIntoResponse
where
    S: Stream<Item = Result<T, Error>> + Send + 'static,
    T: Serialize + Send + 'static,
{
    let byte_stream = stream.and_then(|item| async move { to_cbor_bytes(&item) });
    let content_stream = ResponseStream::new(byte_stream).capture_first_err();

    StreamBody::new(content_stream, cbor_mime())
}

pub fn simple_cbor_stream<S, T>(stream: S) -> impl TryIntoResponse
where
    S: Stream<Item = T> + Send + 'static,
    T: Serialize + Send + 'static,
{
    let byte_stream = stream.then(|item| async move { to_cbor_bytes(&item) });
    let content_stream = ResponseStream::new(byte_stream).capture_first_err();

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
