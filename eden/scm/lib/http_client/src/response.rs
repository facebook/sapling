/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use curl::easy::Easy2;
use http::{
    header::{HeaderMap, HeaderName, HeaderValue},
    status::StatusCode,
};
use serde::de::DeserializeOwned;

use crate::errors::HttpClientError;
use crate::handler::Buffered;

#[derive(Debug)]
pub struct Response {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

impl Response {
    pub(crate) fn from_handle(mut easy: Easy2<Buffered>) -> Result<Self, HttpClientError> {
        let status = get_status_code(&mut easy)?;

        let handler = easy.get_mut();
        let headers = handler
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                let name = HeaderName::from_bytes(name.as_ref()).ok()?;
                let value = HeaderValue::from_bytes(value.as_ref()).ok()?;
                Some((name, value))
            })
            .collect::<HeaderMap>();
        let body = handler.take_data();

        Ok(Self {
            status,
            headers,
            body,
        })
    }

    /// Deserialize the response body from JSON.
    pub fn json<T: DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_slice(&self.body)
    }

    /// Deserialize the response body from CBOR.
    pub fn cbor<T: DeserializeOwned>(&self) -> Result<T, serde_cbor::Error> {
        serde_cbor::from_slice(&self.body)
    }
}

pub(crate) fn get_status_code<H>(easy: &mut Easy2<H>) -> Result<StatusCode, HttpClientError> {
    let code = easy.response_code()?;
    StatusCode::from_u16(code as u16).map_err(|_| HttpClientError::InvalidStatusCode(code))
}
