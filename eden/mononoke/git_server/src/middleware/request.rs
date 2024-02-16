/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use anyhow::Context;
use flate2::write::GzDecoder;
use gotham::prelude::FromState;
use gotham::state::State;
use gotham_ext::body_ext::BodyExt;
use gotham_ext::middleware::Middleware;
use http::header::CONTENT_ENCODING;
use hyper::body::Body;
use hyper::HeaderMap;
use hyper::Response;
use hyper::StatusCode;

const GZIP_ENCODING: &str = "gzip";

enum DecodingResponse {
    Success,
    UnsupportedEncoding(String),
}

async fn decode_body(state: &mut State) -> anyhow::Result<DecodingResponse> {
    let headers = HeaderMap::borrow_from(state);
    let encoding = headers
        .get(CONTENT_ENCODING)
        .map(|encoding| encoding.to_str())
        .transpose()?;
    match encoding {
        Some(GZIP_ENCODING) => {
            let body_bytes = Body::take_from(state)
                .try_concat_body(&HeaderMap::new())
                .context("Failure in generating body from state")?
                .await?;
            // If there is no data, we can just return success
            if body_bytes.is_empty() {
                return Ok(DecodingResponse::Success);
            }
            // Decode the bytes and try to recreate the git object
            let mut decoded_bytes = Vec::new();
            let mut decoder = GzDecoder::new(decoded_bytes);
            decoder
                .write_all(body_bytes.as_ref())
                .context("Failure in gzip decoding body content")?;
            decoded_bytes = decoder
                .finish()
                .context("Failure in finishing gzip decoding")?;
            state.put(Body::from(decoded_bytes));
            Ok(DecodingResponse::Success)
        }
        Some(encoding) => Ok(DecodingResponse::UnsupportedEncoding(encoding.to_string())),
        None => Ok(DecodingResponse::Success),
    }
}

#[derive(Clone)]
pub struct RequestContentEncodingMiddleware {}

#[async_trait::async_trait]
impl Middleware for RequestContentEncodingMiddleware {
    async fn inbound(&self, state: &mut State) -> Option<Response<Body>> {
        match decode_body(state).await {
            Ok(DecodingResponse::Success) => None,
            Ok(DecodingResponse::UnsupportedEncoding(encoding)) => {
                return Some(
                    Response::builder()
                        .status(StatusCode::BAD_REQUEST)
                        .body(format!("Unsupported Content-Encoding: {}", encoding).into())
                        .expect("Failed to build a response"),
                );
            }
            Err(err) => {
                return Some(
                    Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(format!("Error decoding request body: {:?}", err).into())
                        .expect("Failed to build a response"),
                );
            }
        }
    }
}
