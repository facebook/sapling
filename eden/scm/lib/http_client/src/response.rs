/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::convert::TryFrom;
use std::pin::Pin;

use futures::prelude::*;
use http::{header::HeaderMap, status::StatusCode, version::Version};
use serde::de::DeserializeOwned;

use crate::cbor::CborStream;
use crate::errors::HttpClientError;
use crate::handler::Buffered;
use crate::header::Header;
use crate::receiver::ResponseStreams;

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

pub type AsyncBody = Pin<Box<dyn Stream<Item = Result<Vec<u8>, HttpClientError>> + Send + 'static>>;
pub type CborStreamBody<T> = CborStream<T, AsyncBody, Vec<u8>, HttpClientError>;

pub struct AsyncResponse {
    pub version: Version,
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: AsyncBody,
}

impl AsyncResponse {
    pub async fn new(streams: ResponseStreams) -> Result<Self, HttpClientError> {
        let ResponseStreams {
            headers_rx,
            body_rx,
            done_rx,
        } = streams;

        let header_lines = headers_rx
            .take_while(|h| future::ready(h != &Header::EndOfHeaders))
            .collect::<Vec<_>>()
            .await;

        let mut version = None;
        let mut status = None;
        let mut headers = HeaderMap::new();

        for line in header_lines {
            match line {
                Header::Status(v, s) => {
                    version = Some(v);
                    status = Some(s);
                }
                Header::Header(k, v) => {
                    headers.insert(k, v);
                }
                Header::EndOfHeaders => unreachable!(),
            }
        }

        let (version, status) = match (version, status) {
            (Some(v), Some(s)) => (v, s),
            _ => {
                // If we didn't get a status line, we most likely
                // failed to connect to the server at all. In this
                // case, we should expect to receive an error in
                // the `done_rx` stream. Even if not, a response
                // without a status line is invalid so we should
                // fail regardless.
                done_rx.await??;
                return Err(HttpClientError::BadResponse);
            }
        };

        let body = body_rx
            .map(Ok)
            .chain(stream::once(async move {
                done_rx.await??;
                Ok(Vec::new())
            }))
            .try_filter(|chunk| future::ready(!chunk.is_empty()))
            .boxed();

        Ok(Self {
            version,
            status,
            headers,
            body,
        })
    }

    /// Consume the response and attempt to deserialize the
    /// incoming data as a stream of CBOR-serialized values.
    pub fn into_cbor_stream<T: DeserializeOwned>(self) -> CborStreamBody<T> {
        CborStream::new(self.body)
    }
}
