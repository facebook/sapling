/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! cbor.rs - Utilities for working with CBOR data in HTTP requests and responses.

use anyhow::{Context, Error};
use bytes::Bytes;
use futures::{Stream, TryStreamExt};
use gotham::state::State;
use mime::Mime;
use once_cell::sync::Lazy;
use serde::{de::DeserializeOwned, Serialize};

use gotham_ext::{
    error::HttpError,
    response::{StreamBody, TryIntoResponse},
};

use crate::errors::ErrorKind;

use super::get_request_body;

static CBOR_MIME: Lazy<Mime> = Lazy::new(|| "application/cbor".parse().unwrap());

pub fn cbor_mime() -> Mime {
    CBOR_MIME.clone()
}

pub fn to_cbor_bytes<S: Serialize>(s: S) -> Result<Bytes, Error> {
    serde_cbor::to_vec(&s)
        .map(Bytes::from)
        .context(ErrorKind::SerializationFailed)
}

/// Serialize each item of the input stream as CBOR and return
/// a streaming response. Note that although the input stream
/// can fail, the error type is `anyhow::Error` rather than
/// `HttpError` because the HTTP status code would have already
/// been sent at the time of the failure.
pub fn cbor_stream<S, T>(stream: S) -> impl TryIntoResponse
where
    S: Stream<Item = Result<T, Error>> + Send + 'static,
    T: Serialize + Send + 'static,
{
    let byte_stream = stream.and_then(|item| async { to_cbor_bytes(item) });
    StreamBody::new(byte_stream, cbor_mime())
}

pub async fn parse_cbor_request<R: DeserializeOwned>(state: &mut State) -> Result<R, HttpError> {
    let body = get_request_body(state).await?;
    serde_cbor::from_slice(&body)
        .context(ErrorKind::DeserializationFailed)
        .map_err(HttpError::e400)
}
