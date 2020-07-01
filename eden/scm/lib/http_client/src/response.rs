/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::convert::TryFrom;

use http::{header::HeaderMap, status::StatusCode, version::Version};
use serde::de::DeserializeOwned;

use crate::errors::HttpClientError;
use crate::handler::Buffered;

#[derive(Debug)]
pub struct Response {
    pub version: Version,
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

impl Response {
    /// Deserialize the response body from JSON.
    pub fn json<T: DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_slice(&self.body)
    }

    /// Deserialize the response body from CBOR.
    pub fn cbor<T: DeserializeOwned>(&self) -> Result<T, serde_cbor::Error> {
        serde_cbor::from_slice(&self.body)
    }
}

impl TryFrom<&mut Buffered> for Response {
    type Error = HttpClientError;

    fn try_from(buffered: &mut Buffered) -> Result<Self, Self::Error> {
        let (version, status) = match (buffered.version(), buffered.status()) {
            (Some(version), Some(status)) => (version, status),
            _ => {
                return Err(HttpClientError::BadResponse);
            }
        };

        Ok(Self {
            version,
            status,
            headers: buffered.take_headers(),
            body: buffered.take_body(),
        })
    }
}
