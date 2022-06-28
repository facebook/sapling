/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use futures::stream::TryStreamExt;
use futures_ext::stream::StreamTimeoutError;
use futures_ext::FbStreamExt;
use futures_ext::FbTryStreamExt;
use http::HeaderMap;
use http::HeaderValue;
use http::Method;
use http::Response;
use hyper::Body;
use std::time::Duration;
use thiserror::Error;
use tokio::io::AsyncReadExt;
use tokio_util::codec::BytesCodec;
use tokio_util::codec::FramedRead;

use crate::http_service::HttpError;

const NETSPEEDTEST_MAX_NBYTES: u64 = 100 * 1024 * 1024;
const NETSPEEDTEST_TIMEOUT: Duration = Duration::from_secs(300);

const HEADER_DOWNLOAD_NBYTES: &str = "x-netspeedtest-nbytes";

#[derive(Error, Debug)]
pub enum RequestError {
    #[error("Request is invalid")]
    Invalid(#[source] Error),

    #[error("Client hung up")]
    Hangup(#[source] hyper::Error),

    #[error("Request is too large (only up to {} bytes are allowed)", .0)]
    TooLarge(u64),

    #[error("Request timed out")]
    Timeout,
}

impl From<StreamTimeoutError> for RequestError {
    fn from(_: StreamTimeoutError) -> Self {
        Self::Timeout
    }
}

impl From<RequestError> for HttpError {
    fn from(r: RequestError) -> Self {
        Self::BadRequest(Error::from(r).context("Invalid NetSpeedTest request"))
    }
}

pub async fn handle(
    method: Method,
    headers: &HeaderMap<HeaderValue>,
    body: Body,
) -> Result<Response<Body>, HttpError> {
    if method == Method::GET {
        return download(headers);
    }

    if method == Method::POST {
        return upload(body).await;
    }

    Err(HttpError::MethodNotAllowed)
}

fn download(headers: &HeaderMap<HeaderValue>) -> Result<Response<Body>, HttpError> {
    fn read_byte_count(headers: &HeaderMap<HeaderValue>) -> Result<u64, Error> {
        headers
            .get(HEADER_DOWNLOAD_NBYTES)
            .ok_or_else(|| anyhow!("Missing {} header", HEADER_DOWNLOAD_NBYTES))?
            .to_str()
            .with_context(|| format!("Invalid {} header (not UTF-8)", HEADER_DOWNLOAD_NBYTES))?
            .parse()
            .with_context(|| format!("Invalid {} header (invalid usize)", HEADER_DOWNLOAD_NBYTES))
    }

    let byte_count = read_byte_count(headers).map_err(RequestError::Invalid)?;
    let byte_count = std::cmp::min(byte_count, NETSPEEDTEST_MAX_NBYTES);

    let repeat = tokio::io::repeat(0x42).take(byte_count);
    let stream = FramedRead::new(repeat, BytesCodec::new());
    let stream = stream
        .map_err(Error::from)
        .whole_stream_timeout(NETSPEEDTEST_TIMEOUT)
        .flatten_err();

    let res = Response::builder()
        .status(http::StatusCode::OK)
        .header(http::header::CONTENT_LENGTH, byte_count.to_string())
        .body(Body::wrap_stream(stream))
        .map_err(HttpError::internal)?;

    Ok(res)
}

async fn upload(body: Body) -> Result<Response<Body>, HttpError> {
    let mut size = 0;
    let body = body
        .map_err(RequestError::Hangup)
        .whole_stream_timeout(NETSPEEDTEST_TIMEOUT)
        .flatten_err();

    futures::pin_mut!(body);

    while let Some(chunk) = body
        .try_next()
        .await
        .context("Error reading body")
        .map_err(HttpError::internal)?
    {
        let chunk_size: u64 = chunk
            .len()
            .try_into()
            .context("Chunk too large")
            .map_err(HttpError::internal)?;

        size += chunk_size;

        if size > NETSPEEDTEST_MAX_NBYTES {
            return Err(RequestError::TooLarge(NETSPEEDTEST_MAX_NBYTES).into());
        }
    }

    let res = Response::builder()
        .status(http::StatusCode::OK)
        .body(Body::empty())
        .map_err(HttpError::internal)?;

    Ok(res)
}
