/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! cbor.rs - Utilities for working with CBOR data in HTTP requests and responses.

use anyhow::{Context, Error};
use bytes::Bytes;
use gotham::state::State;
use mime::Mime;
use once_cell::sync::Lazy;
use serde::{de::DeserializeOwned, Serialize};

use crate::errors::ErrorKind;

use gotham_ext::{
    error::HttpError,
    response::{BytesBody, TryIntoResponse},
};

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

pub fn cbor_response<S: Serialize>(s: S) -> Result<impl TryIntoResponse, HttpError> {
    let bytes = to_cbor_bytes(s).map_err(HttpError::e500)?;
    Ok(BytesBody::new(bytes, cbor_mime()))
}

pub async fn parse_cbor_request<R: DeserializeOwned>(state: &mut State) -> Result<R, HttpError> {
    let body = get_request_body(state).await?;
    serde_cbor::from_slice(&body)
        .context(ErrorKind::DeserializationFailed)
        .map_err(HttpError::e400)
}
